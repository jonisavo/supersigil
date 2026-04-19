package org.supersigil.intellij

import com.google.gson.Gson
import com.google.gson.JsonParser
import com.intellij.icons.AllIcons
import com.intellij.openapi.Disposable
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.DumbAwareAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.SimpleToolWindowPanel
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.openapi.wm.ToolWindowManager
import com.intellij.openapi.wm.ex.ToolWindowManagerListener
import com.intellij.openapi.wm.ex.ToolWindowManagerListener.ToolWindowManagerEventType
import com.intellij.platform.lsp.api.LspServerState
import com.intellij.ui.JBColor
import com.intellij.ui.content.ContentManagerEvent
import com.intellij.ui.content.ContentManagerListener
import com.intellij.util.Alarm
import com.intellij.ui.jcef.JBCefApp
import com.intellij.ui.jcef.JBCefBrowser
import com.intellij.ui.jcef.JBCefJSQuery
import org.cef.browser.CefBrowser
import org.eclipse.lsp4j.ExecuteCommandParams
import java.net.URI
import java.nio.file.Path
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

private val LOG = Logger.getInstance(GraphExplorerToolWindowFactory::class.java)

class GraphExplorerToolWindowFactory : ToolWindowFactory, DumbAware {
    override suspend fun isApplicableAsync(project: Project): Boolean =
        isGraphExplorerAvailable(hasSupersigilConfig(project), JBCefApp.isSupported())

    override fun shouldBeAvailable(project: Project): Boolean =
        isGraphExplorerAvailable(hasSupersigilConfig(project), JBCefApp.isSupported())

    override fun createToolWindowContent(
        project: Project,
        toolWindow: ToolWindow,
    ) {
        val panel = GraphExplorerPanel(project, toolWindow)

        val actionGroup = DefaultActionGroup()
        actionGroup.add(
            object : DumbAwareAction("Refresh", "Reload Graph Explorer", AllIcons.Actions.Refresh) {
                override fun actionPerformed(e: AnActionEvent) {
                    panel.refresh()
                }
            },
        )
        ActionManager.getInstance().getAction("org.supersigil.ij.verify")?.let(actionGroup::add)

        val toolbar =
            ActionManager
                .getInstance()
                .createActionToolbar("SupersigilGraphExplorer", actionGroup, true)

        val toolWindowPanel = SimpleToolWindowPanel(true)
        toolbar.targetComponent = toolWindowPanel
        toolWindowPanel.toolbar = toolbar.component
        toolWindowPanel.setContent(panel.component)

        val content =
            toolWindow.contentManager.factory
                .createContent(toolWindowPanel, null, false)
        toolWindow.contentManager.addContent(content)
    }
}

internal fun isGraphExplorerAvailable(
    hasSupersigilConfig: Boolean,
    jcefSupported: Boolean,
): Boolean = hasSupersigilConfig && jcefSupported

internal fun shouldLoadGraphExplorerHtml(parentDisposed: Boolean): Boolean = !parentDisposed

internal fun graphExplorerHtml(): String = graphExplorerHtml("")

internal fun graphExplorerHtml(
    bridgeInitScript: String,
): String {
    val bridgeBootstrap =
        if (bridgeInitScript.isBlank()) {
            ""
        } else {
            "      <script>$bridgeInitScript</script>\n"
        }

    return """
        <!DOCTYPE html>
        <html lang="en">
        <head>
          <meta charset="UTF-8">
          <meta name="viewport" content="width=device-width, initial-scale=1.0">
          <link rel="stylesheet" href="${EXPLORER_ORIGIN}landing-tokens.css">
          <link rel="stylesheet" href="${EXPLORER_ORIGIN}explorer-styles.css">
          <link rel="stylesheet" href="${EXPLORER_ORIGIN}supersigil-preview.css">
          <link rel="stylesheet" href="${EXPLORER_ORIGIN}intellij-theme-adapter.css">
        </head>
        <body>
          <div id="explorer" style="height: 100vh;"></div>
          <script src="${EXPLORER_ORIGIN}render-iife.js"></script>
          <script src="${EXPLORER_ORIGIN}supersigil-preview.js"></script>
          <script src="${EXPLORER_ORIGIN}explorer.js"></script>
${bridgeBootstrap}          <script src="${EXPLORER_ORIGIN}explorer-bridge.js"></script>
        </body>
        </html>
        """.trimIndent()
}

private class GraphExplorerPanel(
    private val project: Project,
    private val toolWindow: ToolWindow,
) {
    private val panelDisposable = Disposer.newCheckedDisposable(toolWindow.disposable, "GraphExplorerPanel")
    private val browser = JBCefBrowser()
    private val dataRetryAlarm = Alarm(Alarm.ThreadToUse.POOLED_THREAD, panelDisposable)
    private val refreshAlarm = Alarm(Alarm.ThreadToUse.POOLED_THREAD, panelDisposable)
    private val dataQuery = JBCefJSQuery.create(browser as com.intellij.ui.jcef.JBCefBrowserBase)
    private val actionQuery = JBCefJSQuery.create(browser as com.intellij.ui.jcef.JBCefBrowserBase)
    private val attachedLiveUpdateDescriptors = ConcurrentHashMap.newKeySet<SupersigilLspServerDescriptor>()
    private val staleWhileHidden = AtomicBoolean(false)
    private val bridgeReady = AtomicBoolean(false)
    private val hostInitialized = AtomicBoolean(false)
    private val pendingChangeEvent = AtomicReference<ExplorerChangedEvent?>(null)
    private var pendingFocusDocumentPath: String? = graphExplorerFocusDocumentPath(project)

    val component = browser.component

    init {
        Disposer.register(panelDisposable, browser)
        Disposer.register(panelDisposable, dataQuery)
        Disposer.register(panelDisposable, actionQuery)
        browser.jbCefClient.addRequestHandler(ExplorerResourceRequestHandler(), browser.cefBrowser)
        dataQuery.addHandler { request ->
            when (
                val result =
                    resolveGraphExplorerTransportQuery(
                        request = request,
                        projectBasePath = project.basePath,
                        focusDocumentPath = pendingFocusDocumentPath,
                        loadSnapshot = ::fetchExplorerSnapshot,
                        loadDocument = ::fetchExplorerDocument,
                    )
            ) {
                is GraphExplorerTransportQueryResult.Success -> JBCefJSQuery.Response(result.payload)
                is GraphExplorerTransportQueryResult.Error ->
                    JBCefJSQuery.Response(null, result.code, result.message)
            }
        }
        actionQuery.addHandler { action ->
            if (action == "ready") {
                bridgeReady.set(true)
                scheduleGraphExplorerHostReadyWithRetry(browser.cefBrowser)
            } else {
                dispatchGraphExplorerAction(action)
            }
            JBCefJSQuery.Response("")
        }
        installVisibilityListeners()
        refresh()
    }

    fun refresh() {
        ApplicationManager.getApplication().invokeLater {
            if (!shouldLoadGraphExplorerHtml(panelDisposable.isDisposed)) return@invokeLater
            bridgeReady.set(false)
            hostInitialized.set(false)
            staleWhileHidden.set(false)
            pendingChangeEvent.set(null)
            pendingFocusDocumentPath = graphExplorerFocusDocumentPath(project)
            val bridgeInitScript =
                graphExplorerBridgeInitScript(
                    themeScript = graphExplorerThemeScript(JBColor.isBright()),
                    queryInjection = dataQuery.inject("request", "onSuccess", "onFailure"),
                    actionInjection = actionQuery.inject("action"),
                )
            browser.loadHTML(graphExplorerHtml(bridgeInitScript), EXPLORER_ORIGIN)
        }
    }

    private fun fetchExplorerSnapshot(rootId: String): Any? {
        if (project.isDisposed) return null

        for (server in supersigilServers(project)) {
            if (server.state != LspServerState.Running) continue

            try {
                @Suppress("UNCHECKED_CAST")
                val result =
                    requestGraphExplorerSnapshot { params ->
                        server.sendRequestSync { languageServer ->
                            languageServer.workspaceService.executeCommand(params) as java.util.concurrent.CompletableFuture<Any?>
                        }
                    }

                if (result != null) {
                    return result
                }
            } catch (e: Exception) {
                LOG.debug("Failed to fetch explorer snapshot from LSP server for rootId=$rootId", e)
            }
        }

        return null
    }

    private fun fetchExplorerDocument(
        documentId: String,
        revision: String,
    ): Any? {
        if (project.isDisposed) return null

        for (server in supersigilServers(project)) {
            if (server.state != LspServerState.Running) continue

            try {
                @Suppress("UNCHECKED_CAST")
                val result =
                    requestGraphExplorerDocument(documentId, revision) { params ->
                        server.sendRequestSync { languageServer ->
                            languageServer.workspaceService.executeCommand(params) as java.util.concurrent.CompletableFuture<Any?>
                        }
                    }

                if (result != null) {
                    return result
                }
            } catch (e: Exception) {
                LOG.debug("Failed to fetch explorer document from LSP server for documentId=$documentId", e)
            }
        }

        return null
    }

    private fun installVisibilityListeners() {
        installGraphExplorerVisibilityListeners(
            installContentSelectionListener = { onVisibilityChanged ->
                val listener =
                    object : ContentManagerListener {
                        override fun selectionChanged(event: ContentManagerEvent) {
                            onVisibilityChanged()
                        }
                    }
                toolWindow.contentManager.addContentManagerListener(listener)
                Disposer.register(panelDisposable) {
                    toolWindow.contentManager.removeContentManagerListener(listener)
                }
            },
            installToolWindowVisibilityListener = { onVisibilityChanged ->
                project.messageBus.connect(panelDisposable).subscribe(
                    ToolWindowManagerListener.TOPIC,
                    object : ToolWindowManagerListener {
                        override fun stateChanged(
                            toolWindowManager: ToolWindowManager,
                            changeType: ToolWindowManagerEventType,
                        ) {
                            onVisibilityChanged()
                        }
                    },
                )
            },
            onVisibilityChanged = {
                handleGraphExplorerToolWindowShown(
                    toolWindowId = toolWindow.id,
                    shownToolWindowId = if (toolWindow.isVisible) toolWindow.id else "",
                    staleWhileHidden = staleWhileHidden.get(),
                    requestRefresh = ::requestGraphExplorerRefresh,
                    clearStale = { staleWhileHidden.set(false) },
                )
            },
        )
    }

    private fun handleExplorerChanged(event: ExplorerChangedEvent) {
        if (toolWindow.isVisible && bridgeReady.get()) {
            if (!hostInitialized.get()) {
                pendingChangeEvent.updateAndGet { current ->
                    mergeGraphExplorerChangedEvents(current, event)
                }
                requestGraphExplorerRefresh()
                return
            }

            postExplorerChangedToBrowser(browser.cefBrowser, event)
            return
        }

        pendingChangeEvent.updateAndGet { current ->
            mergeGraphExplorerChangedEvents(current, event)
        }
        staleWhileHidden.set(true)
    }

    private fun requestGraphExplorerRefresh() {
        requestGraphExplorerDebouncedRefresh(
            cancelPendingRefreshes = refreshAlarm::cancelAllRequests,
            scheduleRefresh = { delayMs ->
                refreshAlarm.addRequest({
                    if (panelDisposable.isDisposed || !bridgeReady.get()) return@addRequest
                    if (!hostInitialized.get()) {
                        scheduleGraphExplorerHostReadyWithRetry(browser.cefBrowser)
                        return@addRequest
                    }

                    val pending = pendingChangeEvent.getAndSet(null) ?: return@addRequest
                    staleWhileHidden.set(false)
                    postExplorerChangedToBrowser(browser.cefBrowser, pending)
                }, delayMs)
            },
        )
    }

    private fun attachLiveUpdateListenersIfAvailable() {
        attachGraphExplorerLiveUpdateListeners(
            attachedDescriptors = attachedLiveUpdateDescriptors,
            descriptors =
                supersigilServers(project).mapNotNull { server ->
                    server.descriptor as? SupersigilLspServerDescriptor
                },
            parentDisposable = toolWindow.disposable,
            registerListener = { descriptor, parentDisposable ->
                descriptor.addExplorerChangedListener(::handleExplorerChanged, parentDisposable)
            },
        )
    }

    private fun scheduleGraphExplorerHostReadyWithRetry(cefBrowser: CefBrowser) {
        dataRetryAlarm.cancelAllRequests()

        fun tryInitialize() {
            if (project.isDisposed || panelDisposable.isDisposed) return
            attachLiveUpdateListenersIfAvailable()

            val hasRunningServer = supersigilServers(project).any { it.state == LspServerState.Running }
            if (!hasRunningServer) {
                ensureSupersigilServerStarted(project)
                dataRetryAlarm.addRequest(::tryInitialize, 2000)
                return
            }

            val initialContext =
                buildGraphExplorerInitialContext(
                    projectBasePath = project.basePath ?: return,
                    focusDocumentPath = pendingFocusDocumentPath,
                )
            hostInitialized.set(true)
            staleWhileHidden.set(false)
            pendingChangeEvent.set(null)
            pendingFocusDocumentPath = null
            executeBrowserScript(
                cefBrowser = cefBrowser,
                script = graphExplorerHostReadyScript(Gson().toJson(initialContext)),
                scriptId = "supersigil-graph-explorer-host-ready",
            )
        }

        dataRetryAlarm.addRequest(::tryInitialize, 0)
    }

    private fun postExplorerChangedToBrowser(
        cefBrowser: CefBrowser,
        event: ExplorerChangedEvent,
    ) {
        executeBrowserScript(
            cefBrowser = cefBrowser,
            script = graphExplorerChangedScript(Gson().toJson(event)),
            scriptId = "supersigil-graph-explorer-changed",
        )
    }

    private fun executeBrowserScript(
        cefBrowser: CefBrowser,
        script: String,
        scriptId: String,
    ) {
        ApplicationManager.getApplication().invokeLater {
            if (panelDisposable.isDisposed) return@invokeLater
            cefBrowser.executeJavaScript(script, scriptId, 0)
        }
    }
}

internal fun graphExplorerThemeScript(isBright: Boolean): String =
    if (isBright) {
        "document.documentElement.className = 'light';"
    } else {
        "document.documentElement.className = 'dark';"
    }

internal fun graphExplorerBridgeInitScript(
    themeScript: String,
    queryInjection: String,
    actionInjection: String,
): String =
    """
    (function() {
      $themeScript
      window.__supersigilQuery = function(request, onSuccess, onFailure) {
        $queryInjection
      };
      window.__supersigilAction = function(action) {
        $actionInjection
      };
    })();
    """.trimIndent()

internal fun graphExplorerHostReadyScript(initialContextJson: String): String =
    "window.__supersigilHostReady($initialContextJson);"

internal fun graphExplorerChangedScript(eventJson: String): String =
    "window.__supersigilExplorerChanged($eventJson);"

internal fun buildGraphExplorerInitialContext(
    projectBasePath: String,
    focusDocumentPath: String? = null,
): GraphExplorerInitialContext {
    val rootName = Path.of(projectBasePath).fileName?.toString().orEmpty().ifBlank { projectBasePath }
    return GraphExplorerInitialContext(
        rootId = projectBasePath,
        availableRoots = listOf(GraphExplorerRootContext(id = projectBasePath, name = rootName)),
        focusDocumentPath = focusDocumentPath,
    )
}

internal fun graphExplorerFocusDocumentPath(project: Project): String? {
    val projectBasePath = project.basePath ?: return null
    val selectedFile = FileEditorManager.getInstance(project).selectedFiles.firstOrNull() ?: return null
    val selectedPath = runCatching { Path.of(selectedFile.path) }.getOrNull() ?: return null
    val projectRoot = runCatching { Path.of(projectBasePath) }.getOrNull() ?: return null
    if (!selectedPath.startsWith(projectRoot)) return null
    val relativePath = runCatching { projectRoot.relativize(selectedPath) }.getOrNull() ?: return null
    val normalizedPath = relativePath.toString().replace('\\', '/')
    return normalizedPath.takeIf(::isGraphExplorerSpecDocumentPath)
}

internal fun isGraphExplorerSpecDocumentPath(path: String): Boolean {
    val normalized = path.lowercase()
    return normalized.endsWith(".md") || normalized.endsWith(".mdx")
}

internal sealed class GraphExplorerTransportQueryResult {
    data class Success(
        val payload: String,
    ) : GraphExplorerTransportQueryResult()

    data class Error(
        val code: Int,
        val message: String,
    ) : GraphExplorerTransportQueryResult()
}

internal fun resolveGraphExplorerTransportQuery(
    request: String,
    projectBasePath: String?,
    focusDocumentPath: String?,
    loadSnapshot: (String) -> Any?,
    loadDocument: (String, String) -> Any?,
): GraphExplorerTransportQueryResult =
    try {
        val json = JsonParser.parseString(request).asJsonObject
        val method = json.get("method")?.asString
        val params = json.getAsJsonObject("params")

        when (method) {
            "getInitialContext" -> {
                val rootId = projectBasePath ?: return GraphExplorerTransportQueryResult.Error(1, "Missing project base path")
                GraphExplorerTransportQueryResult.Success(
                    Gson().toJson(buildGraphExplorerInitialContext(rootId, focusDocumentPath)),
                )
            }

            "loadSnapshot" -> {
                val rootId = params?.get("rootId")?.asString ?: projectBasePath
                if (rootId.isNullOrBlank()) {
                    GraphExplorerTransportQueryResult.Error(1, "Missing rootId")
                } else {
                    val snapshot = loadSnapshot(rootId)
                    if (snapshot == null) {
                        GraphExplorerTransportQueryResult.Error(2, "Unable to load explorer snapshot")
                    } else {
                        GraphExplorerTransportQueryResult.Success(Gson().toJson(snapshot))
                    }
                }
            }

            "loadDocument" -> {
                val documentId = params?.get("documentId")?.asString ?: params?.get("document_id")?.asString
                if (documentId.isNullOrBlank()) {
                    GraphExplorerTransportQueryResult.Error(1, "Missing documentId")
                } else {
                    val revision = params?.get("revision")?.asString.orEmpty()
                    val document = loadDocument(documentId, revision)
                    if (document == null) {
                        GraphExplorerTransportQueryResult.Error(2, "Unable to load explorer document")
                    } else {
                        GraphExplorerTransportQueryResult.Success(Gson().toJson(document))
                    }
                }
            }

            else -> GraphExplorerTransportQueryResult.Error(1, "Unknown query method: $method")
        }
    } catch (e: Exception) {
        LOG.debug("Error handling explorer runtime query", e)
        GraphExplorerTransportQueryResult.Error(2, e.message ?: "Unknown error")
    }

internal fun dispatchGraphExplorerAction(
    action: String,
    openFile: (String, Int) -> Unit = NavigationUtil::openFile,
    openCriterion: (String, String) -> Unit = NavigationUtil::openCriterion,
) {
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

        "open-file-uri" -> {
            if (parts.size >= 3) {
                val uri = parts.subList(1, parts.size - 1).joinToString(":")
                val path = runCatching { Path.of(URI(uri)).toString() }.getOrNull() ?: return
                val line = parts.last().toIntOrNull() ?: 0
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

internal fun requestGraphExplorerSnapshot(
    requestSnapshot: (ExecuteCommandParams) -> Any?,
): Any? =
    requestSnapshot(
        ExecuteCommandParams(
            COMMAND_EXPLORER_SNAPSHOT,
            emptyList(),
        ),
    )

internal fun requestGraphExplorerDocument(
    documentId: String,
    revision: String,
    requestDocument: (ExecuteCommandParams) -> Any?,
): Any? =
    requestDocument(
        ExecuteCommandParams(
            COMMAND_EXPLORER_DOCUMENT,
            listOf(
                mapOf(
                    "document_id" to documentId,
                    "revision" to revision,
                ),
            ),
        ),
    )

internal fun requestGraphExplorerDebouncedRefresh(
    cancelPendingRefreshes: () -> Unit,
    scheduleRefresh: (Int) -> Unit,
    delayMs: Int = 200,
) {
    cancelPendingRefreshes()
    scheduleRefresh(delayMs)
}

internal fun mergeGraphExplorerChangedEvents(
    current: ExplorerChangedEvent?,
    next: ExplorerChangedEvent,
): ExplorerChangedEvent =
    if (current == null) {
        next.copy(
            changedDocumentIds = next.changedDocumentIds.distinct().sorted(),
            removedDocumentIds = next.removedDocumentIds.distinct().sorted(),
        )
    } else {
        ExplorerChangedEvent(
            revision = next.revision,
            changedDocumentIds = (current.changedDocumentIds + next.changedDocumentIds).distinct().sorted(),
            removedDocumentIds = (current.removedDocumentIds + next.removedDocumentIds).distinct().sorted(),
        )
    }

internal fun handleGraphExplorerVisibilityChanged(
    isVisible: Boolean,
    staleWhileHidden: Boolean,
    requestRefresh: () -> Unit,
    clearStale: () -> Unit,
) {
    if (isVisible && staleWhileHidden) {
        clearStale()
        requestRefresh()
    }
}

internal fun handleGraphExplorerToolWindowShown(
    toolWindowId: String,
    shownToolWindowId: String,
    staleWhileHidden: Boolean,
    requestRefresh: () -> Unit,
    clearStale: () -> Unit,
) {
    if (toolWindowId != shownToolWindowId) return

    handleGraphExplorerVisibilityChanged(
        isVisible = true,
        staleWhileHidden = staleWhileHidden,
        requestRefresh = requestRefresh,
        clearStale = clearStale,
    )
}

internal fun installGraphExplorerVisibilityListeners(
    installContentSelectionListener: ((() -> Unit) -> Unit),
    installToolWindowVisibilityListener: ((() -> Unit) -> Unit),
    onVisibilityChanged: () -> Unit,
) {
    installContentSelectionListener(onVisibilityChanged)
    installToolWindowVisibilityListener(onVisibilityChanged)
}

internal fun <T, D> attachGraphExplorerLiveUpdateListeners(
    attachedDescriptors: MutableSet<T>,
    descriptors: List<T>,
    parentDisposable: D,
    registerListener: (T, D) -> Unit,
): Boolean {
    var attachedAny = false

    for (descriptor in descriptors) {
        if (!attachedDescriptors.add(descriptor)) continue
        registerListener(descriptor, parentDisposable)
        attachedAny = true
    }

    return attachedAny
}
