package org.supersigil.intellij

import org.junit.Assert.assertFalse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

class GraphExplorerToolWindowFactoryTest {
    private fun testProjectBasePath(): String = Path.of(System.getProperty("java.io.tmpdir"), "supersigil-graph-explorer-project").toString()

    // supersigil: intellij-graph-explorer-availability
    @Test
    fun `availability requires supersigil config and JCEF support`() {
        assertTrue(isGraphExplorerAvailable(hasSupersigilConfig = true, jcefSupported = true))
        assertFalse(isGraphExplorerAvailable(hasSupersigilConfig = false, jcefSupported = true))
        assertFalse(isGraphExplorerAvailable(hasSupersigilConfig = true, jcefSupported = false))
        assertFalse(isGraphExplorerAvailable(hasSupersigilConfig = false, jcefSupported = false))
    }

    @Test
    fun `html load is skipped after parent disposal`() {
        assertTrue(shouldLoadGraphExplorerHtml(parentDisposed = false))
        assertFalse(shouldLoadGraphExplorerHtml(parentDisposed = true))
    }

    // supersigil: intellij-graph-explorer-html-shell
    @Test
    fun `html shell references all bundled explorer resources`() {
        val html = graphExplorerHtml()

        assertTrue(html.contains("""<div id="explorer""""))
        assertTrue(html.contains("""href="${EXPLORER_ORIGIN}landing-tokens.css""""))
        assertTrue(html.contains("""href="${EXPLORER_ORIGIN}explorer-styles.css""""))
        assertTrue(html.contains("""href="${EXPLORER_ORIGIN}supersigil-preview.css""""))
        assertTrue(html.contains("""href="${EXPLORER_ORIGIN}intellij-theme-adapter.css""""))
        assertTrue(html.contains("""src="${EXPLORER_ORIGIN}render-iife.js""""))
        assertTrue(html.contains("""src="${EXPLORER_ORIGIN}supersigil-preview.js""""))
        assertTrue(html.contains("""src="${EXPLORER_ORIGIN}explorer.js""""))
        assertTrue(html.contains("""src="${EXPLORER_ORIGIN}explorer-bridge.js""""))
    }

    @Test
    fun `html shell does not reference external network resources`() {
        val html = graphExplorerHtml()
        val urls = Regex("""https?://[^"'\s)]+""").findAll(html).map { it.value }.toList()

        assertTrue(
            "Unexpected external URLs: ${urls.filterNot { it.startsWith(EXPLORER_ORIGIN) }}",
            urls.all { it.startsWith(EXPLORER_ORIGIN) },
        )
    }

    @Test
    fun `theme script injects light and dark html classes`() {
        assertEquals("document.documentElement.className = 'dark';", graphExplorerThemeScript(isBright = false))
        assertEquals("document.documentElement.className = 'light';", graphExplorerThemeScript(isBright = true))
    }

    @Test
    // supersigil: intellij-graph-explorer-jsquery-bridge
    fun `bridge init script wires query wrappers and theme injection`() {
        val script =
            graphExplorerBridgeInitScript(
                themeScript = graphExplorerThemeScript(isBright = false),
                queryInjection = "__QUERY__",
                actionInjection = "__ACTION__",
            )

        assertTrue(script.contains("document.documentElement.className = 'dark';"))
        assertTrue(script.contains("window.__supersigilQuery = function(request, onSuccess, onFailure)"))
        assertTrue(script.contains("__QUERY__"))
        assertTrue(script.contains("window.__supersigilAction = function(action)"))
        assertTrue(script.contains("__ACTION__"))
    }

    @Test
    // supersigil: intellij-graph-explorer-jsquery-bridge
    fun `document components query resolves and fetches the requested uri`() {
        val requests = mutableListOf<String>()

        val result =
            resolveGraphExplorerDocumentComponentsQuery(
                """{"type":"documentComponents","uri":"file:///doc"}""",
            ) { uri ->
                requests += uri
                """{"fences":[]}"""
            }

        assertEquals(listOf("file:///doc"), requests)
        assertTrue(result is GraphExplorerDocumentComponentsQueryResult.Success)
        assertEquals(
            """{"fences":[]}""",
            (result as GraphExplorerDocumentComponentsQueryResult.Success).payload,
        )
    }

    @Test
    fun `document components query rejects unexpected request types`() {
        var called = false

        val result =
            resolveGraphExplorerDocumentComponentsQuery(
                """{"type":"graphData","uri":"file:///doc"}""",
            ) {
                called = true
                null
            }

        assertFalse(called)
        assertTrue(result is GraphExplorerDocumentComponentsQueryResult.Error)
    }

    @Test
    // supersigil: intellij-graph-explorer-data-fetch
    fun `graph data request uses the graph data command`() {
        val seen = mutableListOf<Any?>()

        val result =
            requestGraphExplorerGraphData { params ->
                seen += params
                mapOf("documents" to emptyList<Any>(), "edges" to emptyList<Any>())
            }

        assertEquals(1, seen.size)
        val params = seen.single() as org.eclipse.lsp4j.ExecuteCommandParams
        assertEquals(COMMAND_GRAPH_DATA, params.command)
        assertTrue(params.arguments.isEmpty())
        assertTrue(result is Map<*, *>)
    }

    @Test
    fun `graph data documents are converted into file uris`() {
        val projectBasePath = testProjectBasePath()
        val targets =
            graphExplorerDocumentTargets(
                mapOf(
                    "documents" to
                        listOf(
                            mapOf("id" to "doc/a", "path" to "specs/doc-a.md"),
                            mapOf("id" to "doc/b", "path" to "specs/doc-b.md"),
                        ),
                    "edges" to emptyList<Any>(),
                ),
                projectBasePath,
            )

        assertEquals(
            listOf(
                GraphExplorerDocumentTarget("doc/a", navigationFileUri(projectBasePath, "specs/doc-a.md")),
                GraphExplorerDocumentTarget("doc/b", navigationFileUri(projectBasePath, "specs/doc-b.md")),
            ),
            targets,
        )
    }

    @Test
    // supersigil: intellij-graph-explorer-navigation-paths
    fun `graph data documents gain absolute file paths for navigation`() {
        val resolvedGraphData =
            resolveGraphExplorerDocumentPaths(
                mapOf(
                    "documents" to
                        listOf(
                            mapOf("id" to "doc/a", "path" to "specs/doc-a.md"),
                        ),
                    "edges" to emptyList<Any>(),
                ),
                "/tmp/project",
            ) as Map<*, *>

        val document = (resolvedGraphData["documents"] as List<*>).single() as Map<*, *>
        assertEquals("specs/doc-a.md", document["path"])
        assertEquals(Path.of("/tmp/project", "specs/doc-a.md").toString(), document["filePath"])
    }

    @Test
    fun `graph data document targets prefer file uri when present`() {
        val targets =
            graphExplorerDocumentTargets(
                mapOf(
                    "documents" to
                        listOf(
                            mapOf(
                                "id" to "doc/a",
                                "path" to "../shared/specs/doc-a.md",
                                "file_uri" to "file:///tmp/shared/specs/doc-a.md",
                            ),
                        ),
                    "edges" to emptyList<Any>(),
                ),
                "/tmp/project",
            )

        assertEquals(
            listOf(GraphExplorerDocumentTarget("doc/a", "file:///tmp/shared/specs/doc-a.md")),
            targets,
        )
    }

    @Test
    fun `graph data documents derive navigation file path from file uri when present`() {
        val resolvedGraphData =
            resolveGraphExplorerDocumentPaths(
                mapOf(
                    "documents" to
                        listOf(
                            mapOf(
                                "id" to "doc/a",
                                "path" to "../shared/specs/doc-a.md",
                                "file_uri" to "file:///tmp/shared/specs/doc-a.md",
                            ),
                        ),
                    "edges" to emptyList<Any>(),
                ),
                "/tmp/project",
            ) as Map<*, *>

        val document = (resolvedGraphData["documents"] as List<*>).single() as Map<*, *>
        assertEquals("../shared/specs/doc-a.md", document["path"])
        assertEquals(Path.of("/tmp/shared/specs/doc-a.md").toString(), document["filePath"])
    }

    @Test
    // supersigil: intellij-graph-explorer-bounded-fetch
    fun `render data fetch respects the semaphore limit`() {
        val executor = Executors.newCachedThreadPool { runnable ->
            Thread(runnable, "graph-explorer-test").apply { isDaemon = true }
        }
        try {
            val docs =
                (1..12).map {
                    GraphExplorerDocumentTarget("doc-$it", "file:///tmp/project/specs/doc-$it.md")
                }
            val active = AtomicInteger(0)
            val maxActive = AtomicInteger(0)
            val entered = CountDownLatch(10)
            val release = CountDownLatch(1)

            val helperFuture =
                executor.submit<List<Any?>> {
                    fetchGraphExplorerRenderData(
                        docs,
                        fetchDocumentComponents = { uri ->
                            val now = active.incrementAndGet()
                            maxActive.updateAndGet { current -> maxOf(current, now) }
                            entered.countDown()
                            release.await(5, TimeUnit.SECONDS)
                            active.decrementAndGet()
                            mapOf("document_id" to uri, "fences" to emptyList<Any>())
                        },
                        submit = { task ->
                            FutureTask<Void>({ task(); null }, null).also(executor::execute)
                        },
                    )
                }

            assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertEquals(10, maxActive.get())
            release.countDown()

            val result = helperFuture.get(5, TimeUnit.SECONDS)
            assertEquals(12, result.size)
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun `render data fetch submits no more than the concurrency limit`() {
        val executor = Executors.newCachedThreadPool { runnable ->
            Thread(runnable, "graph-explorer-test").apply { isDaemon = true }
        }
        try {
            val docs =
                (1..25).map {
                    GraphExplorerDocumentTarget("doc-$it", "file:///tmp/project/specs/doc-$it.md")
                }
            val submitted = AtomicInteger(0)
            val entered = CountDownLatch(10)
            val release = CountDownLatch(1)

            val helperFuture =
                executor.submit<List<Any?>> {
                    fetchGraphExplorerRenderData(
                        docs,
                        fetchDocumentComponents = { uri ->
                            entered.countDown()
                            release.await(5, TimeUnit.SECONDS)
                            mapOf("document_id" to uri, "fences" to emptyList<Any>())
                        },
                        submit = { task ->
                            submitted.incrementAndGet()
                            FutureTask<Void>({ task(); null }, null).also(executor::execute)
                        },
                    )
                }

            assertTrue(entered.await(5, TimeUnit.SECONDS))
            assertEquals(10, submitted.get())
            release.countDown()

            val result = helperFuture.get(5, TimeUnit.SECONDS)
            assertEquals(25, result.size)
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun `render data fetch omits failed documents`() {
        val executor = Executors.newCachedThreadPool { runnable ->
            Thread(runnable, "graph-explorer-test").apply { isDaemon = true }
        }
        try {
            val docs =
                listOf(
                    GraphExplorerDocumentTarget("doc-1", "file:///tmp/project/specs/doc-1.md"),
                    GraphExplorerDocumentTarget("doc-2", "file:///tmp/project/specs/doc-2.md"),
                    GraphExplorerDocumentTarget("doc-3", "file:///tmp/project/specs/doc-3.md"),
                )

            val result =
                fetchGraphExplorerRenderData(
                    docs,
                    fetchDocumentComponents = { uri ->
                        if (uri.endsWith("doc-2.md")) {
                            throw IllegalStateException("boom")
                        }
                        mapOf("document_id" to uri, "fences" to emptyList<Any>())
                    },
                    submit = { task ->
                        FutureTask<Void>({ task(); null }, null).also(executor::execute)
                    },
                )

            assertEquals(2, result.size)
            assertEquals(
                listOf("file:///tmp/project/specs/doc-1.md", "file:///tmp/project/specs/doc-3.md"),
                result.map { (it as Map<*, *>)["document_id"] },
            )
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun `payload assembly and push script include graph and render data`() {
        val payload =
            assembleGraphExplorerPayloadJson(
                graphData =
                    mapOf(
                        "documents" to
                            listOf(
                                mapOf("id" to "doc/a", "path" to "specs/doc-a.md"),
                            ),
                        "edges" to emptyList<Any>(),
                    ),
                renderData =
                    listOf(
                        mapOf("document_id" to "doc/a", "fences" to emptyList<Any>()),
                    ),
            )
        val script = graphExplorerReceiveDataScript(payload)

        assertTrue(script.startsWith("window.__supersigilReceiveData("))
        assertTrue(script.contains("\"graphData\""))
        assertTrue(script.contains("\"renderData\""))
        assertTrue(script.contains("\"document_id\":\"doc/a\""))
    }

    @Test
    // supersigil: intellij-graph-explorer-data-push
    fun `graph explorer push executes receive data script with assembled payload`() {
        val executor = Executors.newCachedThreadPool { runnable ->
            Thread(runnable, "graph-explorer-test").apply { isDaemon = true }
        }
        try {
            val projectBasePath = testProjectBasePath()
            val scripts = mutableListOf<String>()

            val pushed =
                pushGraphExplorerData(
                    projectBasePath = projectBasePath,
                    fetchGraphData = {
                        mapOf(
                            "documents" to
                                listOf(
                                    mapOf("id" to "doc/a", "path" to "specs/doc-a.md"),
                                ),
                            "edges" to emptyList<Any>(),
                        )
                    },
                    fetchDocumentComponents = { uri: String ->
                        mapOf("document_id" to uri, "fences" to emptyList<Any>())
                    },
                    submit = { task: () -> Unit ->
                        FutureTask<Void>({ task(); null }, null).also(executor::execute)
                    },
                    executeScript = { script -> scripts.add(script) },
                )

            assertTrue(pushed)
            assertEquals(1, scripts.size)
            assertTrue(scripts.single().startsWith("window.__supersigilReceiveData("))
            assertTrue(scripts.single().contains("\"graphData\""))
            assertTrue(scripts.single().contains("\"renderData\""))
        assertTrue(
            scripts.single().contains(
                "\"document_id\":\"${navigationFileUri(projectBasePath, "specs/doc-a.md")}\"",
            ),
        )
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun `refresh attempt starts the server and schedules retry when graph push is not ready`() {
        var ensureCalls = 0
        var pushCalls = 0
        var retryCalls = 0

        runGraphExplorerRefreshAttempt(
            hasServers = false,
            ensureServerStarted = { ensureCalls += 1 },
            pushGraphData = {
                pushCalls += 1
                false
            },
            scheduleRetry = { retryCalls += 1 },
        )

        assertEquals(1, ensureCalls)
        assertEquals(1, pushCalls)
        assertEquals(1, retryCalls)
    }

    @Test
    fun `refresh attempt skips retry after a successful graph push`() {
        var ensureCalls = 0
        var retryCalls = 0

        runGraphExplorerRefreshAttempt(
            hasServers = true,
            ensureServerStarted = { ensureCalls += 1 },
            pushGraphData = { true },
            scheduleRetry = { retryCalls += 1 },
        )

        assertEquals(0, ensureCalls)
        assertEquals(0, retryCalls)
    }

    @Test
    fun `refresh requests debounce live updates at 200ms`() {
        var cancelCalls = 0
        val scheduledDelays = mutableListOf<Int>()

        requestGraphExplorerDebouncedRefresh(
            cancelPendingRefreshes = { cancelCalls += 1 },
            scheduleRefresh = { delayMs -> scheduledDelays += delayMs },
        )

        assertEquals(1, cancelCalls)
        assertEquals(listOf(200), scheduledDelays)
    }

    @Test
    // supersigil: intellij-graph-explorer-live-updates
    fun `documents changed requests a refresh when the tool window is visible`() {
        var refreshRequests = 0
        var staleMarks = 0

        handleGraphExplorerDocumentsChanged(
            isVisible = true,
            requestRefresh = { refreshRequests += 1 },
            markStale = { staleMarks += 1 },
        )

        assertEquals(1, refreshRequests)
        assertEquals(0, staleMarks)
    }

    @Test
    // supersigil: intellij-graph-explorer-live-updates
    fun `documents changed marks the explorer stale when the tool window is hidden`() {
        var refreshRequests = 0
        var staleMarks = 0

        handleGraphExplorerDocumentsChanged(
            isVisible = false,
            requestRefresh = { refreshRequests += 1 },
            markStale = { staleMarks += 1 },
        )

        assertEquals(0, refreshRequests)
        assertEquals(1, staleMarks)
    }

    @Test
    // supersigil: intellij-graph-explorer-live-updates
    fun `live update attachment registers new descriptors with the parent disposable`() {
        val registrations = mutableListOf<Pair<String, String>>()
        val attachedDescriptors = mutableSetOf("first")

        val attached =
            attachGraphExplorerLiveUpdateListeners(
                attachedDescriptors = attachedDescriptors,
                descriptors = listOf("first", "second", "third"),
                parentDisposable = "tool-window-disposable",
                registerListener = { descriptor: String, parentDisposable: String ->
                    registrations += descriptor to parentDisposable
                },
            )

        assertTrue(attached)
        assertEquals(
            listOf(
                "second" to "tool-window-disposable",
                "third" to "tool-window-disposable",
            ),
            registrations,
        )
        assertEquals(setOf("first", "second", "third"), attachedDescriptors)
    }

    @Test
    fun `becoming visible refreshes a stale explorer`() {
        var refreshRequests = 0
        var staleClears = 0

        handleGraphExplorerVisibilityChanged(
            isVisible = true,
            staleWhileHidden = true,
            requestRefresh = { refreshRequests += 1 },
            clearStale = { staleClears += 1 },
        )

        assertEquals(1, refreshRequests)
        assertEquals(1, staleClears)
    }

    @Test
    // supersigil: intellij-graph-explorer-live-updates
    fun `showing the graph explorer refreshes stale data`() {
        var refreshRequests = 0
        var staleClears = 0

        handleGraphExplorerToolWindowShown(
            toolWindowId = "Graph Explorer",
            shownToolWindowId = "Graph Explorer",
            staleWhileHidden = true,
            requestRefresh = { refreshRequests += 1 },
            clearStale = { staleClears += 1 },
        )

        assertEquals(1, refreshRequests)
        assertEquals(1, staleClears)
    }

    @Test
    // supersigil: intellij-graph-explorer-live-updates
    fun `visibility listener installation wires content and tool window callbacks`() {
        var contentListener: (() -> Unit)? = null
        var toolWindowListener: (() -> Unit)? = null
        var visibilityChanges = 0

        installGraphExplorerVisibilityListeners(
            installContentSelectionListener = { callback: () -> Unit -> contentListener = callback },
            installToolWindowVisibilityListener = { callback: () -> Unit -> toolWindowListener = callback },
            onVisibilityChanged = { visibilityChanges += 1 },
        )

        assertTrue(contentListener != null)
        assertTrue(toolWindowListener != null)

        contentListener!!()
        toolWindowListener!!()

        assertEquals(2, visibilityChanges)
    }

    @Test
    // supersigil: intellij-graph-explorer-jsquery-bridge
    // supersigil: intellij-graph-explorer-open-file-navigation
    // supersigil: intellij-graph-explorer-evidence-navigation
    fun `action dispatch delegates to navigation utilities`() {
        val fileCalls = mutableListOf<Pair<String, Int>>()
        val criterionCalls = mutableListOf<Pair<String, String>>()

        dispatchGraphExplorerAction(
            "open-file:src/main.rs:42",
            openFile = { path, line -> fileCalls += path to line },
            openCriterion = { docId, criterionId -> criterionCalls += docId to criterionId },
        )

        dispatchGraphExplorerAction(
            "open-criterion:req-1:req-1-2",
            openFile = { path, line -> fileCalls += path to line },
            openCriterion = { docId, criterionId -> criterionCalls += docId to criterionId },
        )

        assertEquals(listOf("src/main.rs" to 42), fileCalls)
        assertEquals(listOf("req-1" to "req-1-2"), criterionCalls)
    }

    // supersigil: intellij-graph-explorer-plugin-xml
    @Test
    fun `plugin xml registers graph explorer tool window`() {
        val pluginXml = Files.readString(Path.of("src/main/resources/META-INF/plugin.xml"))

        assertTrue(pluginXml.contains("""<toolWindow id="Graph Explorer""""))
        assertTrue(pluginXml.contains("""factoryClass="org.supersigil.intellij.GraphExplorerToolWindowFactory""""))
        assertTrue(pluginXml.contains("""anchor="right""""))
        assertTrue(pluginXml.contains("""icon="/icons/supersigil-graph.svg""""))
    }

    @Test
    fun `plugin xml registers spec explorer as supersigil specifications`() {
        val pluginXml = Files.readString(Path.of("src/main/resources/META-INF/plugin.xml"))

        assertTrue(pluginXml.contains("""<toolWindow id="Supersigil Specifications""""))
        assertTrue(pluginXml.contains("""factoryClass="org.supersigil.intellij.SpecExplorerToolWindowFactory""""))
        assertTrue(pluginXml.contains("""icon="/icons/supersigil.svg""""))
    }

    @Test
    fun `graph explorer icon resources are available`() {
        assertNotNull(javaClass.classLoader.getResource("icons/supersigil-graph.svg"))
        assertNotNull(javaClass.classLoader.getResource("icons/supersigil-graph_dark.svg"))
    }
}
