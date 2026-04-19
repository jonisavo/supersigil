package org.supersigil.intellij

import com.google.gson.JsonParser
import org.junit.Assert.assertFalse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path

class GraphExplorerToolWindowFactoryTest {
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
    fun `transport query resolves initial context for the shared runtime`() {
        val result =
            resolveGraphExplorerTransportQuery(
                """{"method":"getInitialContext"}""",
                projectBasePath = "/tmp/project",
                focusDocumentPath = "specs/auth/auth.req.md",
                loadSnapshot = { error("loadSnapshot should not be called") },
                loadDocument = { _, _ -> error("loadDocument should not be called") },
            )

        assertTrue(result is GraphExplorerTransportQueryResult.Success)
        val payload = JsonParser.parseString((result as GraphExplorerTransportQueryResult.Success).payload).asJsonObject
        assertEquals("/tmp/project", payload.get("rootId").asString)
        assertEquals("specs/auth/auth.req.md", payload.get("focusDocumentPath").asString)
        val availableRoots = payload.getAsJsonArray("availableRoots")
        assertEquals(1, availableRoots.size())
        val root = availableRoots.first().asJsonObject
        assertEquals("/tmp/project", root.get("id").asString)
        assertEquals("project", root.get("name").asString)
    }

    @Test
    fun `transport query loads snapshots through the runtime loader`() {
        val calls = mutableListOf<String>()

        val result =
            resolveGraphExplorerTransportQuery(
                """{"method":"loadSnapshot","params":{"rootId":"/tmp/project"}}""",
                projectBasePath = "/tmp/project",
                focusDocumentPath = null,
                loadSnapshot = { rootId ->
                    calls += rootId
                    mapOf("revision" to "42", "documents" to emptyList<Any>(), "edges" to emptyList<Any>())
                },
                loadDocument = { _, _ -> error("loadDocument should not be called") },
            )

        assertEquals(listOf("/tmp/project"), calls)
        assertTrue(result is GraphExplorerTransportQueryResult.Success)
        val payload = JsonParser.parseString((result as GraphExplorerTransportQueryResult.Success).payload).asJsonObject
        assertEquals("42", payload.get("revision").asString)
    }

    @Test
    fun `transport query loads documents through the runtime loader`() {
        val calls = mutableListOf<Pair<String, String>>()

        val result =
            resolveGraphExplorerTransportQuery(
                """{"method":"loadDocument","params":{"documentId":"auth/req","revision":"99"}}""",
                projectBasePath = "/tmp/project",
                focusDocumentPath = null,
                loadSnapshot = { error("loadSnapshot should not be called") },
                loadDocument = { documentId, revision ->
                    calls += documentId to revision
                    mapOf(
                        "revision" to revision,
                        "document_id" to documentId,
                        "stale" to false,
                        "fences" to emptyList<Any>(),
                        "edges" to emptyList<Any>(),
                    )
                },
            )

        assertEquals(listOf("auth/req" to "99"), calls)
        assertTrue(result is GraphExplorerTransportQueryResult.Success)
        val payload = JsonParser.parseString((result as GraphExplorerTransportQueryResult.Success).payload).asJsonObject
        assertEquals("auth/req", payload.get("document_id").asString)
        assertEquals("99", payload.get("revision").asString)
    }

    @Test
    fun `transport query rejects unsupported runtime methods`() {
        val result =
            resolveGraphExplorerTransportQuery(
                """{"method":"reloadEverything"}""",
                projectBasePath = "/tmp/project",
                focusDocumentPath = null,
                loadSnapshot = { error("loadSnapshot should not be called") },
                loadDocument = { _, _ -> error("loadDocument should not be called") },
            )

        assertTrue(result is GraphExplorerTransportQueryResult.Error)
    }

    @Test
    fun `snapshot request uses the explorer snapshot command`() {
        val seen = mutableListOf<Any?>()

        val result =
            requestGraphExplorerSnapshot { params ->
                seen += params
                mapOf("revision" to "1", "documents" to emptyList<Any>(), "edges" to emptyList<Any>())
            }

        assertEquals(1, seen.size)
        val params = seen.single() as org.eclipse.lsp4j.ExecuteCommandParams
        assertEquals(COMMAND_EXPLORER_SNAPSHOT, params.command)
        assertTrue(params.arguments.isEmpty())
        assertTrue(result is Map<*, *>)
    }

    @Test
    fun `document request uses the explorer document command and revisioned params`() {
        val seen = mutableListOf<Any?>()

        val result =
            requestGraphExplorerDocument(
                documentId = "auth/req",
                revision = "123",
            ) { params ->
                seen += params
                mapOf("revision" to "123", "document_id" to "auth/req")
            }

        assertEquals(1, seen.size)
        val params = seen.single() as org.eclipse.lsp4j.ExecuteCommandParams
        assertEquals(COMMAND_EXPLORER_DOCUMENT, params.command)
        assertEquals(1, params.arguments.size)
        val request = params.arguments.single() as Map<*, *>
        assertEquals("auth/req", request["document_id"])
        assertEquals("123", request["revision"])
        assertTrue(result is Map<*, *>)
    }

    @Test
    fun `host ready and change scripts target the shared runtime bridge callbacks`() {
        val hostReadyScript =
            graphExplorerHostReadyScript(
                """{"rootId":"/tmp/project","availableRoots":[{"id":"/tmp/project","name":"project"}]}""",
            )
        val changedScript =
            graphExplorerChangedScript(
                """{"revision":"2","changed_document_ids":["auth/req"],"removed_document_ids":[]}""",
            )

        assertEquals(
            """window.__supersigilHostReady({"rootId":"/tmp/project","availableRoots":[{"id":"/tmp/project","name":"project"}]});""",
            hostReadyScript,
        )
        assertEquals(
            """window.__supersigilExplorerChanged({"revision":"2","changed_document_ids":["auth/req"],"removed_document_ids":[]});""",
            changedScript,
        )
    }

    @Test
    fun `changed event merge unions document ids and keeps the newest revision`() {
        val merged =
            mergeGraphExplorerChangedEvents(
                current =
                    ExplorerChangedEvent(
                        revision = "1",
                        changedDocumentIds = listOf("auth/req"),
                        removedDocumentIds = listOf("auth/tasks"),
                    ),
                next =
                    ExplorerChangedEvent(
                        revision = "2",
                        changedDocumentIds = listOf("auth/design", "auth/req"),
                        removedDocumentIds = listOf("auth/tasks", "auth/adr"),
                    ),
            )

        assertEquals("2", merged.revision)
        assertEquals(listOf("auth/design", "auth/req"), merged.changedDocumentIds)
        assertEquals(listOf("auth/adr", "auth/tasks"), merged.removedDocumentIds)
    }

    @Test
    fun `action dispatch opens file uris through navigation utilities`() {
        val fileCalls = mutableListOf<Pair<String, Int>>()

        dispatchGraphExplorerAction(
            "open-file-uri:file:///tmp/shared/specs/doc-a.md:7",
            openFile = { path, line -> fileCalls += path to line },
        )

        assertEquals(listOf(Path.of("/tmp/shared/specs/doc-a.md").toString() to 7), fileCalls)
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
