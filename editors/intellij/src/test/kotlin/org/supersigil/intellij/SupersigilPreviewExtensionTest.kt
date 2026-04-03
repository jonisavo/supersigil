package org.supersigil.intellij

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SupersigilPreviewExtensionTest {
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
}
