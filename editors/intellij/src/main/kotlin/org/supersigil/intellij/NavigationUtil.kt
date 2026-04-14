package org.supersigil.intellij

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.ProjectManager
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.platform.lsp.api.LspServerState
import org.eclipse.lsp4j.ExecuteCommandParams
import java.nio.file.Path

private val LOG = Logger.getInstance(NavigationUtil::class.java)

/**
 * Shared navigation utilities for opening files and resolving criterion
 * locations via the LSP server. Used by the Markdown preview extension
 * and the Graph Explorer tool window.
 */
object NavigationUtil {
    /**
     * Open a file at the given line in the editor.
     * Searches all open projects for a matching file relative to the project base path.
     */
    fun openFile(
        path: String,
        line: Int,
    ) {
        ApplicationManager.getApplication().invokeLater {
            openFileAcrossProjects(
                path = path,
                line = line,
                projects = ProjectManager.getInstance().openProjects.asList(),
                isDisposed = { project -> project.isDisposed },
                projectBasePath = { project -> project.basePath },
                findFileByPath = { resolvedPath ->
                    LocalFileSystem
                        .getInstance()
                        .findFileByPath(resolvedPath)
                },
                openTextEditor = { project, file, zeroBasedLine ->
                    val descriptor = OpenFileDescriptor(project, file, zeroBasedLine, 0)
                    FileEditorManager.getInstance(project).openTextEditor(descriptor, true)
                },
            )
        }
    }

    /**
     * Open a criterion by resolving its location via the LSP server.
     * Falls back to opening the document file if the criterion cannot
     * be resolved to an exact position.
     */
    fun openCriterion(
        docId: String,
        criterionId: String,
    ) {
        ApplicationManager.getApplication().executeOnPooledThread {
            for (project in ProjectManager.getInstance().openProjects) {
                if (project.isDisposed) continue

                for (server in supersigilServers(project)) {
                    if (server.state != LspServerState.Running) continue

                    try {
                        @Suppress("UNCHECKED_CAST")
                        val listResult =
                            server.sendRequestSync { languageServer ->
                                languageServer.workspaceService.executeCommand(
                                    ExecuteCommandParams(
                                        COMMAND_DOCUMENT_LIST,
                                        emptyList(),
                                    ),
                                ) as java.util.concurrent.CompletableFuture<Any?>
                            }

                        val documents = parseDocumentListResponse(listResult)
                        val targetDoc = documents.find { it.id == docId } ?: continue

                        if (criterionId.isBlank()) {
                            openFile(targetDoc.path, 1)
                            return@executeOnPooledThread
                        }

                        val basePath = project.basePath ?: continue
                        val fileUri = navigationFileUri(basePath, targetDoc.path)

                        @Suppress("UNCHECKED_CAST")
                        val compResult =
                            server.sendRequestSync { languageServer ->
                                languageServer.workspaceService.executeCommand(
                                    ExecuteCommandParams(
                                        COMMAND_DOCUMENT_COMPONENTS,
                                        listOf(fileUri),
                                    ),
                                ) as java.util.concurrent.CompletableFuture<Any?>
                            }

                        val line = findCriterionLine(compResult, criterionId)
                        openFile(targetDoc.path, line ?: 1)
                        return@executeOnPooledThread
                    } catch (e: Exception) {
                        LOG.debug("Failed to resolve criterion location", e)
                    }
                }
            }
        }
    }
}

internal fun resolveNavigationPath(
    projectBasePath: String,
    path: String,
): String =
    if (isAbsoluteNavigationPath(path)) {
        path
    } else {
        Path.of(projectBasePath, path).toString()
    }

internal fun isAbsoluteNavigationPath(path: String): Boolean =
    Path.of(path).isAbsolute ||
        path.matches(Regex("""^[A-Za-z]:[\\/].*""")) ||
        path.startsWith("""\\""")

internal fun navigationFileUri(
    projectBasePath: String,
    path: String,
): String = Path.of(resolveNavigationPath(projectBasePath, path)).toUri().toString()

internal fun <P, F> openFileAcrossProjects(
    path: String,
    line: Int,
    projects: Iterable<P>,
    isDisposed: (P) -> Boolean,
    projectBasePath: (P) -> String?,
    findFileByPath: (String) -> F?,
    openTextEditor: (P, F, Int) -> Unit,
): Boolean {
    for (project in projects) {
        if (isDisposed(project)) continue
        val basePath = projectBasePath(project) ?: continue
        val resolvedPath = resolveNavigationPath(basePath, path)
        val file = findFileByPath(resolvedPath) ?: continue
        openTextEditor(project, file, maxOf(0, line - 1))
        return true
    }

    return false
}

// ---------------------------------------------------------------------------
// Action string parsing
// ---------------------------------------------------------------------------

/**
 * Split an action string by unescaped colons.
 * Backslash-escaped colons (`\:`) are preserved as literal colons.
 */
internal fun splitAction(action: String): List<String> {
    val parts = mutableListOf<String>()
    val current = StringBuilder()

    var i = 0
    while (i < action.length) {
        when {
            action[i] == '\\' && i + 1 < action.length -> {
                current.append(action[i + 1])
                i += 2
            }

            action[i] == ':' -> {
                parts.add(current.toString())
                current.clear()
                i++
            }

            else -> {
                current.append(action[i])
                i++
            }
        }
    }
    parts.add(current.toString())
    return parts
}

// ---------------------------------------------------------------------------
// Criterion lookup helpers
// ---------------------------------------------------------------------------

/**
 * Find the source line of a criterion by ID in a document components
 * response. Walks the untyped JSON (LinkedTreeMap from Gson) to find
 * a component with the matching criterion ID.
 */
@Suppress("UNCHECKED_CAST")
internal fun findCriterionLine(
    result: Any?,
    criterionId: String,
): Int? {
    val map = result as? Map<*, *> ?: return null
    val fences = map["fences"] as? List<*> ?: return null

    for (fence in fences) {
        val fenceMap = fence as? Map<*, *> ?: continue
        val components = fenceMap["components"] as? List<*> ?: continue
        val line = findCriterionInComponents(components, criterionId)
        if (line != null) return line
    }
    return null
}

@Suppress("UNCHECKED_CAST")
private fun findCriterionInComponents(
    components: List<*>,
    criterionId: String,
): Int? {
    for (comp in components) {
        val compMap = comp as? Map<*, *> ?: continue
        val id = compMap["id"] as? String
        if (id == criterionId) {
            val sourceRange = compMap["source_range"] as? Map<*, *>
            val startLine = sourceRange?.get("start_line")
            return when (startLine) {
                is Number -> startLine.toInt()
                else -> null
            }
        }
        val children = compMap["children"] as? List<*>
        if (children != null) {
            val found = findCriterionInComponents(children, criterionId)
            if (found != null) return found
        }
    }
    return null
}
