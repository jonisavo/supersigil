package org.supersigil.intellij

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.platform.lsp.api.LspServer
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.platform.lsp.api.LspServerState
import java.util.Collections
import java.util.WeakHashMap

private val LOG = Logger.getInstance("org.supersigil.intellij.compatibility")
private val compatibilityFailures =
    Collections.synchronizedMap(WeakHashMap<Project, CompatibilityResult.Incompatible>())

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

fun lastCompatibilityFailure(project: Project): CompatibilityResult.Incompatible? = compatibilityFailures[project]

fun resolveCompatibleServerBinary(
    project: Project,
    configuredPath: String?,
): String? {
    val binaryPath =
        resolveServerBinary(configuredPath) ?: run {
            compatibilityFailures.remove(project)
            notifyBinaryNotFound(project, configuredPath)
            return null
        }

    return when (val result = queryCompatibilityInfoCached(binaryPath)) {
        is CompatibilityResult.Compatible -> {
            compatibilityFailures.remove(project)
            binaryPath
        }
        is CompatibilityResult.Incompatible -> {
            compatibilityFailures[project] = result
            LOG.warn(
                "Supersigil compatibility check failed: supported=${result.supportedVersion}, reported=${result.reportedVersion ?: "unavailable"}, serverVersion=${result.serverVersion ?: "unavailable"}, reason=${result.reason}, binaryPath=$binaryPath",
            )
            notifyIncompatibleServer(project, binaryPath, result)
            null
        }
    }
}

fun ensureSupersigilServerStarted(project: Project) {
    if (supersigilServers(project).isNotEmpty()) return

    val settings = SupersigilSettings.getInstance()
    val binaryPath = resolveCompatibleServerBinary(project, settings.serverPath) ?: return

    LspServerManager.getInstance(project).ensureServerStarted(
        SupersigilLspServerSupportProvider::class.java,
        SupersigilLspServerDescriptor(project, binaryPath),
    )
}
