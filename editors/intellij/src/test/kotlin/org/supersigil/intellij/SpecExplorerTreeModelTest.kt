package org.supersigil.intellij

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SpecExplorerTreeModelTest {
    @Test
    fun `single project groups documents by prefix`() {
        val docs =
            listOf(
                DocumentEntry("auth/req", "requirements", "approved", "specs/auth/req.md", null),
                DocumentEntry("auth/design", "design", "draft", "specs/auth/design.md", null),
                DocumentEntry("config/req", "requirements", "implemented", "specs/config/req.md", null),
            )

        val tree = buildSpecTree(docs)
        assertEquals(2, tree.size)

        val authGroup = tree[0] as GroupNode
        assertEquals("auth", authGroup.label)
        assertEquals(2, authGroup.documentCount)
        assertEquals("req", authGroup.children[0].label)
        assertEquals("design", authGroup.children[1].label)

        val configGroup = tree[1] as GroupNode
        assertEquals("config", configGroup.label)
        assertEquals(1, configGroup.documentCount)
    }

    @Test
    fun `multi-project creates project nodes`() {
        val docs =
            listOf(
                DocumentEntry("auth/req", "requirements", "approved", "specs/auth/req.md", "core"),
                DocumentEntry("db/design", "design", "draft", "specs/db/design.md", "infra"),
            )

        val tree = buildSpecTree(docs)
        assertEquals(2, tree.size)

        val core = tree[0] as ProjectNode
        assertEquals("core", core.label)
        assertEquals(1, core.children.size)

        val infra = tree[1] as ProjectNode
        assertEquals("infra", infra.label)
        assertEquals(1, infra.children.size)
    }

    @Test
    fun `ungrouped documents appear at top level`() {
        val docs =
            listOf(
                DocumentEntry("auth/req", "requirements", "approved", "specs/auth/req.md", null),
                DocumentEntry("contributing", "documentation", null, "specs/contributing.md", null),
            )

        val tree = buildSpecTree(docs)
        assertEquals(2, tree.size)

        val authGroup = tree[0] as GroupNode
        assertEquals("auth", authGroup.label)

        val contributing = tree[1] as DocumentNode
        assertEquals("contributing", contributing.label)
        assertEquals("contributing", contributing.id)
    }

    @Test
    fun `document node has correct description with status`() {
        val doc = DocumentEntry("auth/req", "requirements", "approved", "specs/auth/req.md", null)
        val tree = buildSpecTree(listOf(doc))
        val group = tree[0] as GroupNode
        val node = group.children[0]
        assertEquals("requirements · approved", node.description)
    }

    @Test
    fun `document node has correct description without status`() {
        val doc = DocumentEntry("readme", "documentation", null, "readme.md", null)
        val tree = buildSpecTree(listOf(doc))
        val node = tree[0] as DocumentNode
        assertEquals("documentation", node.description)
    }

    @Test
    fun `icon mapping for all doc types`() {
        assertEquals(DocTypeIcon.REQUIREMENTS, docTypeIcon("requirements"))
        assertEquals(DocTypeIcon.DESIGN, docTypeIcon("design"))
        assertEquals(DocTypeIcon.TASKS, docTypeIcon("tasks"))
        assertEquals(DocTypeIcon.DECISION, docTypeIcon("adr"))
        assertEquals(DocTypeIcon.DECISION, docTypeIcon("decision"))
        assertEquals(DocTypeIcon.DOCUMENTATION, docTypeIcon("documentation"))
        assertEquals(DocTypeIcon.OTHER, docTypeIcon("unknown"))
    }

    @Test
    fun `status color mapping`() {
        assertEquals(StatusColor.GREEN, statusColor("approved"))
        assertEquals(StatusColor.GREEN, statusColor("implemented"))
        assertEquals(StatusColor.GREEN, statusColor("done"))
        assertEquals(StatusColor.GREEN, statusColor("accepted"))
        assertEquals(StatusColor.GRAY, statusColor("superseded"))
        assertEquals(StatusColor.DEFAULT, statusColor("draft"))
        assertEquals(StatusColor.DEFAULT, statusColor(null))
    }

    @Test
    fun `multi-project with ungrouped docs at end`() {
        val docs =
            listOf(
                DocumentEntry("auth/req", "requirements", "approved", "specs/auth/req.md", "core"),
                DocumentEntry("readme", "documentation", null, "readme.md", null),
            )

        val tree = buildSpecTree(docs)
        assertEquals(2, tree.size)

        val core = tree[0] as ProjectNode
        assertEquals("core", core.label)

        val readme = tree[1] as DocumentNode
        assertEquals("readme", readme.label)
    }

    @Test
    fun `empty document list produces empty tree`() {
        val tree = buildSpecTree(emptyList())
        assertTrue(tree.isEmpty())
    }

    @Test
    fun `projects are sorted alphabetically`() {
        val docs =
            listOf(
                DocumentEntry("x/req", "requirements", null, "x/req.md", "zebra"),
                DocumentEntry("y/req", "requirements", null, "y/req.md", "alpha"),
            )

        val tree = buildSpecTree(docs)
        assertEquals("alpha", (tree[0] as ProjectNode).label)
        assertEquals("zebra", (tree[1] as ProjectNode).label)
    }

    @Test
    fun `groups are sorted alphabetically`() {
        val docs =
            listOf(
                DocumentEntry("zconfig/req", "requirements", null, "z/req.md", null),
                DocumentEntry("auth/req", "requirements", null, "a/req.md", null),
            )

        val tree = buildSpecTree(docs)
        assertEquals("auth", (tree[0] as GroupNode).label)
        assertEquals("zconfig", (tree[1] as GroupNode).label)
    }
}
