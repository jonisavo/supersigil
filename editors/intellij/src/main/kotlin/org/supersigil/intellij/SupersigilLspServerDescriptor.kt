package org.supersigil.intellij

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.Disposable
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.Lsp4jClient
import com.intellij.platform.lsp.api.LspServerNotificationsHandler
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import java.util.concurrent.CopyOnWriteArrayList

class SupersigilLspServerDescriptor(
    project: Project,
    private val binaryPath: String,
) : ProjectWideLspServerDescriptor(project, "Supersigil") {
    private val documentsChangedListeners = CopyOnWriteArrayList<() -> Unit>()

    override fun isSupportedFile(file: VirtualFile): Boolean {
        val extension = file.extension?.lowercase() ?: return false
        return extension == "md" || extension == "mdx"
    }

    override fun createCommandLine(): GeneralCommandLine = createSupersigilCommandLine(binaryPath, project.basePath)

    override fun createLsp4jClient(handler: LspServerNotificationsHandler): Lsp4jClient =
        SupersigilLsp4jClient(handler) {
            for (listener in documentsChangedListeners) {
                listener()
            }
        }

    fun addDocumentsChangedListener(
        listener: () -> Unit,
        parentDisposable: Disposable,
    ) {
        documentsChangedListeners.add(listener)
        com.intellij.openapi.util.Disposer.register(parentDisposable) {
            documentsChangedListeners.remove(listener)
        }
    }
}

internal fun createSupersigilCommandLine(
    binaryPath: String,
    workDirectory: String?,
): GeneralCommandLine =
    GeneralCommandLine(binaryPath)
        .withWorkDirectory(workDirectory)
