package org.supersigil.intellij

/**
 * Data class matching the LSP `supersigil/documentList` response shape.
 */
data class DocumentEntry(
    val id: String,
    val docType: String,
    val status: String?,
    val path: String,
    val project: String?,
)

/**
 * Tree node types for the Spec Explorer.
 */
sealed interface SpecTreeNode {
    val label: String
}

data class ProjectNode(
    override val label: String,
    val children: List<SpecTreeNode>,
) : SpecTreeNode

data class GroupNode(
    override val label: String,
    val children: List<DocumentNode>,
) : SpecTreeNode {
    val documentCount: Int get() = children.size
}

data class DocumentNode(
    override val label: String,
    val id: String,
    val docType: String,
    val status: String?,
    val path: String,
    val project: String?,
) : SpecTreeNode {
    val description: String
        get() = if (status != null) "$docType · $status" else docType
}

/**
 * Pure data transformation: takes a flat list of documents and produces
 * a tree structure grouped by project (if multi-project) and ID prefix.
 *
 * This mirrors the VS Code extension's `groupDocuments` logic.
 */
fun buildSpecTree(documents: List<DocumentEntry>): List<SpecTreeNode> {
    val byProject = documents.groupBy { it.project }
    val hasProjects = byProject.keys.any { it != null }

    if (hasProjects) {
        val nodes = mutableListOf<SpecTreeNode>()

        // Named projects first, sorted
        for (projectName in byProject.keys.filterNotNull().sorted()) {
            val docs = byProject[projectName] ?: continue
            nodes.add(ProjectNode(projectName, buildGroupedNodes(docs)))
        }

        // Ungrouped documents (null project) at the end
        val ungrouped = byProject[null]
        if (ungrouped != null) {
            nodes.addAll(buildGroupedNodes(ungrouped))
        }

        return nodes
    }

    return buildGroupedNodes(documents)
}

private fun buildGroupedNodes(documents: List<DocumentEntry>): List<SpecTreeNode> {
    val grouped = mutableMapOf<String?, MutableList<DocumentEntry>>()

    for (doc in documents) {
        val slashIndex = doc.id.indexOf('/')
        val prefix = if (slashIndex >= 0) doc.id.substring(0, slashIndex) else null
        grouped.getOrPut(prefix) { mutableListOf() }.add(doc)
    }

    val nodes = mutableListOf<SpecTreeNode>()

    // Sorted prefixes first, nulls (ungrouped) last
    for (prefix in grouped.keys.sortedWith(nullsLast(compareBy { it }))) {
        val docs = grouped[prefix]!!
        val documentNodes = docs.map { toDocumentNode(it) }

        if (prefix == null) {
            nodes.addAll(documentNodes)
        } else {
            nodes.add(GroupNode(prefix, documentNodes))
        }
    }

    return nodes
}

private fun toDocumentNode(entry: DocumentEntry): DocumentNode {
    val slashIndex = entry.id.indexOf('/')
    val label = if (slashIndex >= 0) entry.id.substring(slashIndex + 1) else entry.id

    return DocumentNode(
        label = label,
        id = entry.id,
        docType = entry.docType,
        status = entry.status,
        path = entry.path,
        project = entry.project,
    )
}

private val STABLE_STATUSES = setOf("approved", "implemented", "done", "accepted")

enum class StatusColor {
    GREEN,
    GRAY,
    DEFAULT,
}

fun statusColor(status: String?): StatusColor {
    if (status == null) return StatusColor.DEFAULT
    if (status in STABLE_STATUSES) return StatusColor.GREEN
    if (status == "superseded") return StatusColor.GRAY
    return StatusColor.DEFAULT
}

enum class DocTypeIcon {
    REQUIREMENTS,
    DESIGN,
    TASKS,
    DECISION,
    DOCUMENTATION,
    OTHER,
}

fun docTypeIcon(docType: String): DocTypeIcon =
    when (docType) {
        "requirements" -> DocTypeIcon.REQUIREMENTS
        "design" -> DocTypeIcon.DESIGN
        "tasks" -> DocTypeIcon.TASKS
        "adr", "decision" -> DocTypeIcon.DECISION
        "documentation" -> DocTypeIcon.DOCUMENTATION
        else -> DocTypeIcon.OTHER
    }
