package org.supersigil.intellij

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.platform.lsp.api.LspServer
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.platform.lsp.api.LspServerState

const val COMMAND_VERIFY = "supersigil.verify"
const val COMMAND_DOCUMENT_LIST = "supersigil.documentList"

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
