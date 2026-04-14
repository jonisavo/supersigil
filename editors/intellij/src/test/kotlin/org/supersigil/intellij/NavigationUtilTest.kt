package org.supersigil.intellij

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Path

class NavigationUtilTest {
    private data class FakeProject(
        val disposed: Boolean = false,
        val basePath: String? = null,
    )

    @Test
    fun `splitAction splits on unescaped colons`() {
        val parts = splitAction("open-file:src/main.rs:42")
        assertEquals(listOf("open-file", "src/main.rs", "42"), parts)
    }

    @Test
    fun `splitAction preserves escaped colons`() {
        val parts = splitAction("open-file:C\\:\\\\Users\\\\file.rs:10")
        assertEquals(listOf("open-file", "C:\\Users\\file.rs", "10"), parts)
    }

    @Test
    fun `splitAction with empty parts`() {
        val parts = splitAction("open-criterion:my-doc:")
        assertEquals(listOf("open-criterion", "my-doc", ""), parts)
    }

    @Test
    fun `splitAction single segment`() {
        val parts = splitAction("something")
        assertEquals(listOf("something"), parts)
    }

    @Test
    fun `splitAction empty string`() {
        val parts = splitAction("")
        assertEquals(listOf(""), parts)
    }

    @Test
    fun `findCriterionLine finds criterion in flat list`() {
        val result =
            mapOf(
                "fences" to
                    listOf(
                        mapOf(
                            "components" to
                                listOf(
                                    mapOf(
                                        "kind" to "Criterion",
                                        "id" to "req-1",
                                        "source_range" to mapOf("start_line" to 10),
                                        "children" to emptyList<Any>(),
                                    ),
                                ),
                        ),
                    ),
            )

        assertEquals(10, findCriterionLine(result, "req-1"))
    }

    @Test
    fun `findCriterionLine finds nested criterion`() {
        val result =
            mapOf(
                "fences" to
                    listOf(
                        mapOf(
                            "components" to
                                listOf(
                                    mapOf(
                                        "kind" to "AcceptanceCriteria",
                                        "id" to null,
                                        "children" to
                                            listOf(
                                                mapOf(
                                                    "kind" to "Criterion",
                                                    "id" to "req-2",
                                                    "source_range" to mapOf("start_line" to 25),
                                                    "children" to emptyList<Any>(),
                                                ),
                                            ),
                                    ),
                                ),
                        ),
                    ),
            )

        assertEquals(25, findCriterionLine(result, "req-2"))
    }

    @Test
    fun `findCriterionLine returns null for missing criterion`() {
        val result =
            mapOf(
                "fences" to
                    listOf(
                        mapOf(
                            "components" to
                                listOf(
                                    mapOf(
                                        "kind" to "Criterion",
                                        "id" to "req-1",
                                        "source_range" to mapOf("start_line" to 10),
                                        "children" to emptyList<Any>(),
                                    ),
                                ),
                        ),
                    ),
            )

        assertNull(findCriterionLine(result, "nonexistent"))
    }

    @Test
    fun `findCriterionLine returns null for null input`() {
        assertNull(findCriterionLine(null, "req-1"))
    }

    @Test
    fun `findCriterionLine returns null for empty fences`() {
        val result = mapOf("fences" to emptyList<Any>())
        assertNull(findCriterionLine(result, "req-1"))
    }

    @Test
    // supersigil: intellij-graph-explorer-navigation-paths
    fun `navigation path resolution keeps absolute unix paths`() {
        assertEquals("/tmp/specs/doc.md", resolveNavigationPath("/workspace", "/tmp/specs/doc.md"))
    }

    @Test
    fun `navigation path resolution keeps absolute windows paths`() {
        assertEquals("""C:\Users\specs\doc.md""", resolveNavigationPath("/workspace", """C:\Users\specs\doc.md"""))
    }

    @Test
    fun `navigation path resolution resolves relative paths against the project base`() {
        assertEquals("/workspace/specs/doc.md", resolveNavigationPath("/workspace", "specs/doc.md"))
    }

    @Test
    fun `navigation file uri resolves relative paths through the shared path resolver`() {
        assertEquals(
            Path.of("/workspace", "specs/my doc.md").toUri().toString(),
            navigationFileUri("/workspace", "specs/my doc.md"),
        )
    }

    @Test
    fun `navigation file uri preserves absolute paths`() {
        assertEquals(
            Path.of("/tmp/specs/doc.md").toUri().toString(),
            navigationFileUri("/workspace", "/tmp/specs/doc.md"),
        )
    }

    @Test
    // supersigil: intellij-graph-explorer-open-file-navigation
    fun `open file across projects opens the first matching spec file in the editor`() {
        val openCalls = mutableListOf<Triple<FakeProject, String, Int>>()
        val firstProject = FakeProject(basePath = "/workspace-a")
        val secondProject = FakeProject(basePath = "/workspace-b")

        val opened =
            openFileAcrossProjects(
                path = "specs/doc.md",
                line = 7,
                projects = listOf(firstProject, secondProject),
                isDisposed = { it.disposed },
                projectBasePath = { it.basePath },
                findFileByPath = { resolvedPath ->
                    when (resolvedPath) {
                        "/workspace-b/specs/doc.md" -> "file:///workspace-b/specs/doc.md"
                        else -> null
                    }
                },
                openTextEditor = { project, file, zeroBasedLine ->
                    openCalls += Triple(project, file, zeroBasedLine)
                },
            )

        assertTrue(opened)
        assertEquals(
            listOf(
                Triple(secondProject, "file:///workspace-b/specs/doc.md", 6),
            ),
            openCalls,
        )
    }

    @Test
    // supersigil: intellij-graph-explorer-evidence-navigation
    fun `open file across projects preserves evidence line numbers when opening the editor`() {
        val openCalls = mutableListOf<Pair<String, Int>>()

        val opened =
            openFileAcrossProjects(
                path = "/workspace/specs/doc.md",
                line = 42,
                projects = listOf(FakeProject(basePath = "/workspace")),
                isDisposed = { it.disposed },
                projectBasePath = { it.basePath },
                findFileByPath = { resolvedPath -> resolvedPath },
                openTextEditor = { _, file, zeroBasedLine ->
                    openCalls += file to zeroBasedLine
                },
            )

        assertTrue(opened)
        assertEquals(listOf("/workspace/specs/doc.md" to 41), openCalls)
    }
}
