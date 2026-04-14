package org.supersigil.intellij

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.ProjectManager
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
    private fun handleAction(action: String) = dispatchGraphExplorerAction(action)
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
