package org.supersigil.intellij

import com.intellij.platform.lsp.api.Lsp4jClient
import com.intellij.platform.lsp.api.LspServerNotificationsHandler
import org.eclipse.lsp4j.jsonrpc.services.JsonNotification

/**
 * Custom LSP client that handles Supersigil-specific notifications for the
 * tree explorer and graph explorer tool windows.
 */
class SupersigilLsp4jClient(
    handler: LspServerNotificationsHandler,
    private val onDocumentsChanged: () -> Unit,
    private val onExplorerChanged: (ExplorerChangedEvent) -> Unit,
) : Lsp4jClient(handler) {
    @JsonNotification("supersigil/documentsChanged")
    fun documentsChanged() {
        onDocumentsChanged()
    }

    @JsonNotification("supersigil/explorerChanged")
    fun explorerChanged(event: ExplorerChangedEvent) {
        onExplorerChanged(event)
    }
}
