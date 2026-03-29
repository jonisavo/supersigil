package org.supersigil.intellij

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.platform.lsp.api.LspServerState
import org.eclipse.lsp4j.ExecuteCommandParams

class VerifyAction : AnAction() {
    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        val project = e.project
        e.presentation.isEnabled = project != null && hasRunningSupersigil(project)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return

        for (server in supersigilServers(project)) {
            if (server.state == LspServerState.Running) {
                // Fire-and-forget: the verify command's useful output is the
                // diagnostics it publishes as a side effect, not the RPC response.
                // Using sendNotification avoids the synchronous timeout that
                // sendRequestSync imposes.
                server.sendNotification { languageServer ->
                    val params = ExecuteCommandParams()
                    params.command = COMMAND_VERIFY
                    params.arguments = emptyList<Any>()
                    languageServer.workspaceService.executeCommand(params)
                }
            }
        }
    }
}
