package org.supersigil.intellij

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.ProjectManager
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.platform.lsp.api.LspServerState
import com.intellij.ui.jcef.JBCefJSQuery
import org.cef.browser.CefBrowser
import org.cef.browser.CefFrame
import org.cef.handler.CefLoadHandlerAdapter
import org.eclipse.lsp4j.ExecuteCommandParams
import org.intellij.plugins.markdown.extensions.MarkdownBrowserPreviewExtension
import org.intellij.plugins.markdown.ui.preview.MarkdownHtmlPanel
import org.intellij.plugins.markdown.ui.preview.ResourceProvider
import org.intellij.plugins.markdown.ui.preview.jcef.MarkdownJCEFHtmlPanel

private val LOG = Logger.getInstance(SupersigilPreviewExtensionProvider::class.java)

/**
 * Provides the Supersigil markdown preview extension for IntelliJ's
 * JCEF-based markdown preview.
 *
 * Injects the presentation kit (CSS + JS), a render module, and a
 * bridge script that fetches component data from the LSP server via
 * JBCefJSQuery and renders supersigil-xml fences as rich HTML.
 */
class SupersigilPreviewExtensionProvider : MarkdownBrowserPreviewExtension.Provider {
    override fun createBrowserExtension(
        panel: MarkdownHtmlPanel,
    ): MarkdownBrowserPreviewExtension {
        if (panel is MarkdownJCEFHtmlPanel) {
            return SupersigilPreviewExtension(panel)
        }
        // Non-JCEF panel: return a no-op extension that just injects
        // static CSS/JS (the bridge won't function without JCEF).
        return SupersigilStaticExtension()
    }
}

// ---------------------------------------------------------------------------
// Static-only extension (no-op for non-JCEF panels)
// ---------------------------------------------------------------------------

/**
 * Fallback extension for non-JCEF panels. Injects the CSS so that
 * supersigil-xml fences at least get some styling, but the bridge
 * script will not have query functions and will be inert.
 */
private class SupersigilStaticExtension : MarkdownBrowserPreviewExtension {
    override val scripts: List<String> get() = emptyList()
    override val styles: List<String> get() = listOf(RESOURCE_PREVIEW_CSS)
    override val resourceProvider: ResourceProvider get() = SupersigilResourceProvider()

    override fun dispose() {}
}

// ---------------------------------------------------------------------------
// Full JCEF extension with bridge
// ---------------------------------------------------------------------------

/**
 * The browser preview extension that injects scripts, styles, and
 * sets up JBCefJSQuery handlers for bidirectional communication
 * between the JCEF browser and the JVM.
 */
private class SupersigilPreviewExtension(
    private val panel: MarkdownJCEFHtmlPanel,
) : MarkdownBrowserPreviewExtension {
    // JBCefJSQuery for component data requests (browser -> JVM -> browser)
    private val dataQuery: JBCefJSQuery = JBCefJSQuery.create(panel as com.intellij.ui.jcef.JBCefBrowserBase)

    // JBCefJSQuery for navigation actions (browser -> JVM, fire-and-forget)
    private val actionQuery: JBCefJSQuery = JBCefJSQuery.create(panel as com.intellij.ui.jcef.JBCefBrowserBase)

    init {
        dataQuery.addHandler { request -> handleDataQuery(request) }
        actionQuery.addHandler { action ->
            handleAction(action)
            JBCefJSQuery.Response("")
        }

        // Inject the query bridge functions after each page load
        panel.jbCefClient.addLoadHandler(
            object : CefLoadHandlerAdapter() {
                override fun onLoadEnd(
                    browser: CefBrowser?,
                    frame: CefFrame?,
                    httpStatusCode: Int,
                ) {
                    if (frame?.isMain == true) {
                        injectBridgeFunctions(browser)
                    }
                }
            },
            panel.cefBrowser,
        )
    }

    override val scripts: List<String>
        get() =
            listOf(
                RESOURCE_RENDER_IIFE,
                RESOURCE_PREVIEW_JS,
                RESOURCE_BRIDGE_JS,
            )

    override val styles: List<String>
        get() = listOf(RESOURCE_PREVIEW_CSS)

    override val resourceProvider: ResourceProvider get() = SupersigilResourceProvider()

    override fun dispose() {
        dataQuery.dispose()
        actionQuery.dispose()
    }

    // -------------------------------------------------------------------------
    // JBCefJSQuery bridge injection
    // -------------------------------------------------------------------------

    /**
     * Inject the global `__supersigilQuery` and `__supersigilAction`
     * functions into the JCEF browser after page load. These wrap the
     * JBCefJSQuery injection code so the bridge JS can call them.
     */
    private fun injectBridgeFunctions(browser: CefBrowser?) {
        browser ?: return

        // Inject the current document URI for the bridge to use
        val docUri = detectDocumentUri()
        val uriJs =
            if (docUri != null) {
                "window.__supersigilDocUri = ${escapeJsString(docUri)};"
            } else {
                "window.__supersigilDocUri = null;"
            }

        // Build JS that wraps JBCefJSQuery calls into clean functions.
        // inject(request, onSuccess, onFailure) produces a JS expression
        // that routes the call through JBCefJSQuery's message router.
        val queryInjection = dataQuery.inject("request", "onSuccess", "onFailure")
        val actionInjection = actionQuery.inject("action")

        val js =
            """
            (function() {
                $uriJs
                window.__supersigilQuery = function(request, onSuccess, onFailure) {
                    $queryInjection
                };
                window.__supersigilAction = function(action) {
                    $actionInjection
                };
            })();
            """.trimIndent()

        browser.executeJavaScript(js, "supersigil-bridge-init", 0)
    }

    /**
     * Detect the document URI from the panel's virtual file.
     */
    private fun detectDocumentUri(): String? {
        return try {
            val field = MarkdownJCEFHtmlPanel::class.java.getDeclaredField("virtualFile")
            field.isAccessible = true
            val vf = field.get(panel) as? com.intellij.openapi.vfs.VirtualFile
            vf?.url
        } catch (e: Exception) {
            LOG.debug("Could not detect document URI from panel", e)
            null
        }
    }

    // -------------------------------------------------------------------------
    // Data query handler
    // -------------------------------------------------------------------------

    /**
     * Handle a data query from the bridge JS.
     * Request format: `{"type":"documentComponents","uri":"<uri>"}`
     * Returns the JSON response from the LSP server.
     */
    private fun handleDataQuery(request: String): JBCefJSQuery.Response {
        return try {
            val json = com.google.gson.JsonParser.parseString(request).asJsonObject
            val type = json.get("type")?.asString
            val uri = json.get("uri")?.asString

            if (type == "documentComponents" && uri != null) {
                val result = fetchDocumentComponents(uri)
                JBCefJSQuery.Response(result ?: "{}")
            } else {
                JBCefJSQuery.Response(null, 1, "Unknown query type: $type")
            }
        } catch (e: Exception) {
            LOG.debug("Error handling data query", e)
            JBCefJSQuery.Response(null, 2, e.message ?: "Unknown error")
        }
    }

    /**
     * Fetch document components from the LSP server via
     * workspace/executeCommand("supersigil.documentComponents").
     */
    private fun fetchDocumentComponents(uri: String): String? {
        for (project in ProjectManager.getInstance().openProjects) {
            if (project.isDisposed) continue

            for (server in supersigilServers(project)) {
                if (server.state != LspServerState.Running) continue

                try {
                    @Suppress("UNCHECKED_CAST")
                    val result =
                        server.sendRequestSync { languageServer ->
                            languageServer.workspaceService.executeCommand(
                                ExecuteCommandParams(
                                    COMMAND_DOCUMENT_COMPONENTS,
                                    listOf(uri),
                                ),
                            ) as java.util.concurrent.CompletableFuture<Any?>
                        }

                    if (result != null) {
                        return com.google.gson.Gson().toJson(result)
                    }
                } catch (e: Exception) {
                    LOG.debug("Failed to fetch document components from LSP server", e)
                }
            }
        }
        return null
    }

    // -------------------------------------------------------------------------
    // Navigation action handler
    // -------------------------------------------------------------------------

    /**
     * Handle a navigation action from the bridge JS.
     * Action format: `open-file:<path>:<line>` or
     *                `open-criterion:<docId>:<criterionId>`
     */
    private fun handleAction(action: String) {
        val parts = splitAction(action)
        if (parts.isEmpty()) return

        when (parts[0]) {
            "open-file" -> {
                if (parts.size >= 3) {
                    val path = parts[1]
                    val line = parts[2].toIntOrNull() ?: 0
                    openFile(path, line)
                }
            }

            "open-criterion" -> {
                if (parts.size >= 3) {
                    val docId = parts[1]
                    val criterionId = parts[2]
                    openCriterion(docId, criterionId)
                }
            }
        }
    }

    /**
     * Open a file at the given line in the editor.
     */
    private fun openFile(
        path: String,
        line: Int,
    ) {
        ApplicationManager.getApplication().invokeLater {
            for (project in ProjectManager.getInstance().openProjects) {
                if (project.isDisposed) continue
                val basePath = project.basePath ?: continue

                val file =
                    LocalFileSystem
                        .getInstance()
                        .findFileByPath("$basePath/$path")
                        ?: continue

                val descriptor = OpenFileDescriptor(project, file, maxOf(0, line - 1), 0)
                FileEditorManager.getInstance(project).openTextEditor(descriptor, true)
                return@invokeLater
            }
        }
    }

    /**
     * Open a criterion by resolving its location via the LSP server.
     * Falls back to opening the document file if the criterion cannot
     * be resolved to an exact position.
     */
    private fun openCriterion(
        docId: String,
        criterionId: String,
    ) {
        ApplicationManager.getApplication().executeOnPooledThread {
            for (project in ProjectManager.getInstance().openProjects) {
                if (project.isDisposed) continue

                for (server in supersigilServers(project)) {
                    if (server.state != LspServerState.Running) continue

                    try {
                        @Suppress("UNCHECKED_CAST")
                        val listResult =
                            server.sendRequestSync { languageServer ->
                                languageServer.workspaceService.executeCommand(
                                    ExecuteCommandParams(
                                        COMMAND_DOCUMENT_LIST,
                                        emptyList(),
                                    ),
                                ) as java.util.concurrent.CompletableFuture<Any?>
                            }

                        val documents = parseDocumentListResponse(listResult)
                        val targetDoc = documents.find { it.id == docId } ?: continue

                        if (criterionId.isBlank()) {
                            openFile(targetDoc.path, 1)
                            return@executeOnPooledThread
                        }

                        val basePath = project.basePath ?: continue
                        val fileUri = "file://$basePath/${targetDoc.path}"

                        @Suppress("UNCHECKED_CAST")
                        val compResult =
                            server.sendRequestSync { languageServer ->
                                languageServer.workspaceService.executeCommand(
                                    ExecuteCommandParams(
                                        COMMAND_DOCUMENT_COMPONENTS,
                                        listOf(fileUri),
                                    ),
                                ) as java.util.concurrent.CompletableFuture<Any?>
                            }

                        val line = findCriterionLine(compResult, criterionId)
                        openFile(targetDoc.path, line ?: 1)
                        return@executeOnPooledThread
                    } catch (e: Exception) {
                        LOG.debug("Failed to resolve criterion location", e)
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Resource constants
// ---------------------------------------------------------------------------

private const val RESOURCE_RENDER_IIFE = "supersigil-preview/render-iife.js"
private const val RESOURCE_PREVIEW_JS = "supersigil-preview/supersigil-preview.js"
private const val RESOURCE_BRIDGE_JS = "supersigil-preview/supersigil-bridge.js"
private const val RESOURCE_PREVIEW_CSS = "supersigil-preview/supersigil-preview.css"

// ---------------------------------------------------------------------------
// Resource provider
// ---------------------------------------------------------------------------

/**
 * Serves Supersigil preview resources (JS, CSS) from the plugin's
 * classpath resources.
 */
private class SupersigilResourceProvider : ResourceProvider {
    override fun canProvide(resourceName: String): Boolean = resourceName.startsWith("supersigil-preview/")

    override fun loadResource(resourceName: String): ResourceProvider.Resource? {
        val stream =
            SupersigilResourceProvider::class.java.classLoader.getResourceAsStream(resourceName)
                ?: return null
        return ResourceProvider.Resource(stream.readAllBytes())
    }
}

// ---------------------------------------------------------------------------
// Action string parsing
// ---------------------------------------------------------------------------

/**
 * Split an action string by unescaped colons.
 * Backslash-escaped colons (`\:`) are preserved as literal colons.
 */
internal fun splitAction(action: String): List<String> {
    val parts = mutableListOf<String>()
    val current = StringBuilder()

    var i = 0
    while (i < action.length) {
        when {
            action[i] == '\\' && i + 1 < action.length -> {
                current.append(action[i + 1])
                i += 2
            }

            action[i] == ':' -> {
                parts.add(current.toString())
                current.clear()
                i++
            }

            else -> {
                current.append(action[i])
                i++
            }
        }
    }
    parts.add(current.toString())
    return parts
}

// ---------------------------------------------------------------------------
// Criterion lookup helpers
// ---------------------------------------------------------------------------

/**
 * Find the source line of a criterion by ID in a document components
 * response. Walks the untyped JSON (LinkedTreeMap from Gson) to find
 * a component with the matching criterion ID.
 */
@Suppress("UNCHECKED_CAST")
internal fun findCriterionLine(
    result: Any?,
    criterionId: String,
): Int? {
    val map = result as? Map<*, *> ?: return null
    val fences = map["fences"] as? List<*> ?: return null

    for (fence in fences) {
        val fenceMap = fence as? Map<*, *> ?: continue
        val components = fenceMap["components"] as? List<*> ?: continue
        val line = findCriterionInComponents(components, criterionId)
        if (line != null) return line
    }
    return null
}

@Suppress("UNCHECKED_CAST")
private fun findCriterionInComponents(
    components: List<*>,
    criterionId: String,
): Int? {
    for (comp in components) {
        val compMap = comp as? Map<*, *> ?: continue
        val id = compMap["id"] as? String
        if (id == criterionId) {
            val sourceRange = compMap["source_range"] as? Map<*, *>
            val startLine = sourceRange?.get("start_line")
            return when (startLine) {
                is Number -> startLine.toInt()
                else -> null
            }
        }
        val children = compMap["children"] as? List<*>
        if (children != null) {
            val found = findCriterionInComponents(children, criterionId)
            if (found != null) return found
        }
    }
    return null
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

private fun escapeJsString(s: String): String {
    val escaped =
        s
            .replace("\\", "\\\\")
            .replace("'", "\\'")
            .replace("\"", "\\\"")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
    return "\"$escaped\""
}
