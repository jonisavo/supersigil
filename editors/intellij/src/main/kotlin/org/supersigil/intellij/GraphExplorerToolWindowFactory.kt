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
import org.cef.browser.CefFrame
import org.cef.handler.CefLoadHandlerAdapter
import org.eclipse.lsp4j.ExecuteCommandParams
import java.net.URI
import java.nio.file.Path
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Future
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

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

internal fun graphExplorerHtml(): String =
    """
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
      <script src="${EXPLORER_ORIGIN}explorer-bridge.js"></script>
    </body>
    </html>
    """.trimIndent()

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

    val component = browser.component

    init {
        Disposer.register(panelDisposable, browser)
        Disposer.register(panelDisposable, dataQuery)
        Disposer.register(panelDisposable, actionQuery)
        browser.jbCefClient.addRequestHandler(ExplorerResourceRequestHandler(), browser.cefBrowser)
        dataQuery.addHandler { request ->
            when (val result = resolveGraphExplorerDocumentComponentsQuery(request, ::fetchDocumentComponents)) {
                is GraphExplorerDocumentComponentsQueryResult.Success -> JBCefJSQuery.Response(result.payload)
                is GraphExplorerDocumentComponentsQueryResult.Error ->
                    JBCefJSQuery.Response(null, result.code, result.message)
            }
        }
        actionQuery.addHandler { action ->
            dispatchGraphExplorerAction(action)
            JBCefJSQuery.Response("")
        }
        browser.jbCefClient.addLoadHandler(
            object : CefLoadHandlerAdapter() {
                override fun onLoadEnd(
                    browser: CefBrowser?,
                    frame: CefFrame?,
                    httpStatusCode: Int,
                ) {
                    val cefBrowser = browser ?: return
                    if (frame?.isMain == true) {
                        cefBrowser.executeJavaScript(
                            graphExplorerBridgeInitScript(
                                themeScript = graphExplorerThemeScript(JBColor.isBright()),
                                queryInjection = dataQuery.inject("request", "onSuccess", "onFailure"),
                                actionInjection = actionQuery.inject("action"),
                            ),
                            "supersigil-graph-explorer-bridge-init",
                            0,
                        )
                        scheduleGraphDataRefreshWithRetry(cefBrowser)
                    }
                }
            },
            browser.cefBrowser,
        )
        installVisibilityListeners()
        refresh()
    }

    fun refresh() {
        ApplicationManager.getApplication().invokeLater {
            if (!shouldLoadGraphExplorerHtml(panelDisposable.isDisposed)) return@invokeLater
            browser.loadHTML(graphExplorerHtml(), EXPLORER_ORIGIN)
        }
    }

    private fun fetchDocumentComponents(uri: String): String? {
        if (project.isDisposed) return null

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
                LOG.debug("Failed to fetch document components from LSP server for $uri", e)
            }
        }

        return null
    }

    private fun fetchGraphData(): Any? {
        if (project.isDisposed) return null

        for (server in supersigilServers(project)) {
            if (server.state != LspServerState.Running) continue

            try {
                @Suppress("UNCHECKED_CAST")
                val result =
                    requestGraphExplorerGraphData { params ->
                        server.sendRequestSync { languageServer ->
                            languageServer.workspaceService.executeCommand(params) as java.util.concurrent.CompletableFuture<Any?>
                        }
                    }

                if (result != null) {
                    return result
                }
            } catch (e: Exception) {
                LOG.debug("Failed to fetch graph data from LSP server", e)
            }
        }

        return null
    }

    private fun fetchRenderDocumentComponents(uri: String): Any? =
        fetchDocumentComponents(uri)?.let(JsonParser::parseString)

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

    private fun handleDocumentsChanged() {
        handleGraphExplorerDocumentsChanged(
            isVisible = toolWindow.isVisible,
            requestRefresh = ::requestGraphExplorerRefresh,
            markStale = { staleWhileHidden.set(true) },
        )
    }

    private fun requestGraphExplorerRefresh() {
        requestGraphExplorerDebouncedRefresh(
            cancelPendingRefreshes = refreshAlarm::cancelAllRequests,
            scheduleRefresh = { delayMs ->
                refreshAlarm.addRequest({ scheduleGraphDataRefreshWithRetry(browser.cefBrowser) }, delayMs)
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
                descriptor.addDocumentsChangedListener(::handleDocumentsChanged, parentDisposable)
            },
        )
    }

    private fun scheduleGraphDataRefreshWithRetry(cefBrowser: CefBrowser) {
        dataRetryAlarm.cancelAllRequests()

        fun tryPush() {
            if (project.isDisposed || panelDisposable.isDisposed) return
            attachLiveUpdateListenersIfAvailable()

            runGraphExplorerRefreshAttempt(
                hasServers = supersigilServers(project).isNotEmpty(),
                ensureServerStarted = { ensureSupersigilServerStarted(project) },
                pushGraphData = { pushGraphDataToBrowser(cefBrowser) },
                scheduleRetry = { dataRetryAlarm.addRequest(::tryPush, 2000) },
            )
        }

        dataRetryAlarm.addRequest(::tryPush, 0)
    }

    private fun pushGraphDataToBrowser(cefBrowser: CefBrowser): Boolean {
        return pushGraphExplorerData(
            projectBasePath = project.basePath,
            fetchGraphData = ::fetchGraphData,
            fetchDocumentComponents = ::fetchRenderDocumentComponents,
            submit = { task -> ApplicationManager.getApplication().executeOnPooledThread(task) },
            executeScript = { script ->
                ApplicationManager.getApplication().invokeLater {
                    if (panelDisposable.isDisposed) return@invokeLater
                    cefBrowser.executeJavaScript(script, "supersigil-graph-explorer-data", 0)
                }
            },
        )
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

internal fun graphExplorerReceiveDataScript(payloadJson: String): String =
    "window.__supersigilReceiveData($payloadJson);"

internal sealed class GraphExplorerDocumentComponentsQueryResult {
    data class Success(
        val payload: String,
    ) : GraphExplorerDocumentComponentsQueryResult()

    data class Error(
        val code: Int,
        val message: String,
    ) : GraphExplorerDocumentComponentsQueryResult()
}

internal fun resolveGraphExplorerDocumentComponentsQuery(
    request: String,
    fetchDocumentComponents: (String) -> String?,
): GraphExplorerDocumentComponentsQueryResult =
    try {
        val json = JsonParser.parseString(request).asJsonObject
        val type = json.get("type")?.asString
        val uri = json.get("uri")?.asString

        when {
            type != "documentComponents" -> GraphExplorerDocumentComponentsQueryResult.Error(1, "Unknown query type: $type")
            uri == null -> GraphExplorerDocumentComponentsQueryResult.Error(1, "Missing uri")
            else -> GraphExplorerDocumentComponentsQueryResult.Success(fetchDocumentComponents(uri) ?: "{}")
        }
    } catch (e: Exception) {
        LOG.debug("Error handling data query", e)
        GraphExplorerDocumentComponentsQueryResult.Error(2, e.message ?: "Unknown error")
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

        "open-criterion" -> {
            if (parts.size >= 3) {
                val docId = parts[1]
                val criterionId = parts[2]
                openCriterion(docId, criterionId)
            }
        }
    }
}

internal fun requestGraphExplorerGraphData(
    requestGraphData: (ExecuteCommandParams) -> Any?,
): Any? =
    requestGraphData(
        ExecuteCommandParams(
            COMMAND_GRAPH_DATA,
            emptyList(),
        ),
    )

internal data class GraphExplorerDocumentTarget(
    val id: String,
    val uri: String,
)

internal data class ResolvedGraphExplorerData(
    val graphData: Any,
    val documentTargets: List<GraphExplorerDocumentTarget>,
)

internal fun graphExplorerDocumentUri(
    projectBasePath: String,
    documentPath: String,
    documentFileUri: String? = null,
): String = documentFileUri ?: navigationFileUri(projectBasePath, documentPath)

internal fun graphExplorerDocumentAbsolutePath(
    projectBasePath: String,
    documentPath: String,
    documentFileUri: String? = null,
): String = documentFileUri?.let(::graphExplorerPathFromFileUri) ?: resolveNavigationPath(projectBasePath, documentPath)

private fun graphExplorerPathFromFileUri(documentFileUri: String): String =
    Path.of(URI(documentFileUri)).toString()

internal fun resolveGraphExplorerData(
    graphData: Any?,
    projectBasePath: String,
): ResolvedGraphExplorerData? {
    if (graphData == null) return null

    val root = Gson().toJsonTree(graphData).asJsonObject.deepCopy()
    val documents = root.getAsJsonArray("documents") ?: return ResolvedGraphExplorerData(graphData, emptyList())
    val documentTargets = mutableListOf<GraphExplorerDocumentTarget>()
    for (element in documents) {
        val document = element.asJsonObject
        val id = document.get("id")?.asString ?: continue
        val path = document.get("path")?.asString ?: continue
        val fileUri = document.get("file_uri")?.asString
        val resolvedPath = graphExplorerDocumentAbsolutePath(projectBasePath, path, fileUri)
        document.addProperty("filePath", resolvedPath)
        documentTargets += GraphExplorerDocumentTarget(id, graphExplorerDocumentUri(projectBasePath, path, fileUri))
    }

    return ResolvedGraphExplorerData(
        graphData = Gson().fromJson(root, Any::class.java),
        documentTargets = documentTargets,
    )
}

internal fun resolveGraphExplorerDocumentPaths(
    graphData: Any?,
    projectBasePath: String,
): Any? = resolveGraphExplorerData(graphData, projectBasePath)?.graphData

internal fun graphExplorerDocumentTargets(
    graphData: Any?,
    projectBasePath: String,
): List<GraphExplorerDocumentTarget> = resolveGraphExplorerData(graphData, projectBasePath)?.documentTargets ?: emptyList()

internal fun runGraphExplorerRefreshAttempt(
    hasServers: Boolean,
    ensureServerStarted: () -> Unit,
    pushGraphData: () -> Boolean,
    scheduleRetry: () -> Unit,
) {
    if (!hasServers) {
        ensureServerStarted()
    }

    if (!pushGraphData()) {
        scheduleRetry()
    }
}

internal fun requestGraphExplorerDebouncedRefresh(
    cancelPendingRefreshes: () -> Unit,
    scheduleRefresh: (Int) -> Unit,
    delayMs: Int = 200,
) {
    cancelPendingRefreshes()
    scheduleRefresh(delayMs)
}

internal fun handleGraphExplorerDocumentsChanged(
    isVisible: Boolean,
    requestRefresh: () -> Unit,
    markStale: () -> Unit,
) {
    if (isVisible) {
        requestRefresh()
    } else {
        markStale()
    }
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

internal fun pushGraphExplorerData(
    projectBasePath: String?,
    fetchGraphData: () -> Any?,
    fetchDocumentComponents: (String) -> Any?,
    submit: ((() -> Unit) -> Future<*>),
    executeScript: (String) -> Unit,
): Boolean {
    val graphData = fetchGraphData() ?: return false
    val basePath = projectBasePath ?: return false
    val resolvedGraphData = resolveGraphExplorerData(graphData, basePath) ?: return false
    val renderData =
        fetchGraphExplorerRenderData(
            resolvedGraphData.documentTargets,
            fetchDocumentComponents = fetchDocumentComponents,
            submit = submit,
        )
    executeScript(
        graphExplorerReceiveDataScript(
            assembleGraphExplorerPayloadJson(resolvedGraphData.graphData, renderData),
        ),
    )
    return true
}

internal fun fetchGraphExplorerRenderData(
    documentTargets: List<GraphExplorerDocumentTarget>,
    maxConcurrency: Int = 10,
    fetchDocumentComponents: (String) -> Any?,
    submit: ((() -> Unit) -> Future<*>),
): List<Any> {
    if (documentTargets.isEmpty()) return emptyList()

    val concurrencyLimit = maxOf(1, maxConcurrency)
    val nextIndex = AtomicInteger(0)
    val results = arrayOfNulls<Any>(documentTargets.size)
    val futures =
        List(minOf(concurrencyLimit, documentTargets.size)) {
            submit {
                while (true) {
                    val index = nextIndex.getAndIncrement()
                    if (index >= documentTargets.size) return@submit

                    val target = documentTargets[index]
                    try {
                        val result = fetchDocumentComponents(target.uri)
                        results[index] = result
                    } catch (e: Exception) {
                        results[index] = null
                    }
                }
            }
        }
    futures.forEach { it.get() }
    return results.filterNotNull()
}

internal fun assembleGraphExplorerPayloadJson(
    graphData: Any?,
    renderData: List<Any>,
): String = Gson().toJson(mapOf("graphData" to graphData, "renderData" to renderData))
