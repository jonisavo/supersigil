package org.supersigil.intellij

import com.google.gson.annotations.SerializedName

data class ExplorerChangedEvent(
    val revision: String = "",
    @SerializedName("changed_document_ids")
    val changedDocumentIds: List<String> = emptyList(),
    @SerializedName("removed_document_ids")
    val removedDocumentIds: List<String> = emptyList(),
)

internal data class GraphExplorerRootContext(
    val id: String,
    val name: String,
)

internal data class GraphExplorerInitialContext(
    val rootId: String,
    val availableRoots: List<GraphExplorerRootContext>,
    val focusDocumentPath: String? = null,
)
