package org.supersigil.intellij

import com.intellij.platform.lsp.api.Lsp4jClient
import com.intellij.platform.lsp.api.LspServerNotificationsHandler
import org.eclipse.lsp4j.jsonrpc.services.JsonNotification

/**
 * Custom LSP client that handles the `supersigil/documentsChanged`
 * notification from the server. When received, it fires the registered
 * callback so the Spec Explorer can refresh.
 */
class SupersigilLsp4jClient(
    handler: LspServerNotificationsHandler,
    private val onDocumentsChanged: () -> Unit,
) : Lsp4jClient(handler) {
    @JsonNotification("supersigil/documentsChanged")
    fun documentsChanged() {
        onDocumentsChanged()
    }
}
