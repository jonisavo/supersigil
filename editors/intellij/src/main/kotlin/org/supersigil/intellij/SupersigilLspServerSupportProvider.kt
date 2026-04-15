package org.supersigil.intellij

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider

class SupersigilLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(
        project: Project,
        file: VirtualFile,
        serverStarter: LspServerSupportProvider.LspServerStarter,
    ) {
        val ext = file.extension?.lowercase()
        if (ext != "md" && ext != "mdx") return
        if (!hasSupersigilConfig(project)) return

        val settings = SupersigilSettings.getInstance()
        val binaryPath = resolveCompatibleServerBinary(project, settings.serverPath) ?: return

        serverStarter.ensureServerStarted(
            SupersigilLspServerDescriptor(project, binaryPath),
        )
    }
}
