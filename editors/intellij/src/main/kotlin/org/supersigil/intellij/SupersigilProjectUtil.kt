package org.supersigil.intellij

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.platform.lsp.api.LspServer
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.platform.lsp.api.LspServerState

const val COMMAND_VERIFY = "supersigil.verify"
const val COMMAND_DOCUMENT_LIST = "supersigil.documentList"
const val COMMAND_DOCUMENT_COMPONENTS = "supersigil.documentComponents"
const val COMMAND_GRAPH_DATA = "supersigil.graphData"

fun hasSupersigilConfig(project: Project): Boolean {
    val basePath = project.basePath ?: return false
    val baseDir = LocalFileSystem.getInstance().findFileByPath(basePath) ?: return false
    return baseDir.findChild("supersigil.toml") != null
}

fun supersigilServers(project: Project): Collection<LspServer> =
    LspServerManager
        .getInstance(project)
        .getServersForProvider(SupersigilLspServerSupportProvider::class.java)

fun hasRunningSupersigil(project: Project): Boolean = supersigilServers(project).any { it.state == LspServerState.Running }

fun ensureSupersigilServerStarted(project: Project) {
    if (supersigilServers(project).isNotEmpty()) return

    val settings = SupersigilSettings.getInstance()
    val binaryPath = resolveServerBinary(settings.serverPath) ?: return

    LspServerManager.getInstance(project).ensureServerStarted(
        SupersigilLspServerSupportProvider::class.java,
        SupersigilLspServerDescriptor(project, binaryPath),
    )
}
