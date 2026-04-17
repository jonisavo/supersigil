package org.supersigil.intellij

import com.intellij.icons.AllIcons
import com.intellij.ide.BrowserUtil
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.options.ShowSettingsUtil
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.SimpleToolWindowPanel
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.openapi.wm.ToolWindowManager
import com.intellij.platform.lsp.api.LspServerState
import com.intellij.ui.JBColor
import com.intellij.ui.SimpleTextAttributes
import com.intellij.ui.TreeSpeedSearch
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.treeStructure.Tree
import com.intellij.util.Alarm
import javax.swing.Icon
import javax.swing.JTree
import javax.swing.tree.DefaultMutableTreeNode
import javax.swing.tree.DefaultTreeModel

private val LOG = Logger.getInstance(SpecExplorerToolWindowFactory::class.java)

class SpecExplorerToolWindowFactory : ToolWindowFactory, DumbAware {
    override suspend fun isApplicableAsync(project: Project): Boolean = hasSupersigilConfig(project)

    override fun shouldBeAvailable(project: Project): Boolean = hasSupersigilConfig(project)

    override fun createToolWindowContent(
        project: Project,
        toolWindow: ToolWindow,
    ) {
        val tree = Tree(DefaultTreeModel(DefaultMutableTreeNode("root")))
        tree.isRootVisible = false
        tree.showsRootHandles = true
        tree.cellRenderer = SpecTreeCellRenderer()
        tree.emptyText.setText("Waiting for language server\u2026")
        TreeSpeedSearch.installOn(tree)

        tree.addMouseListener(
            object : java.awt.event.MouseAdapter() {
                override fun mouseClicked(e: java.awt.event.MouseEvent) {
                    if (e.clickCount == 2) {
                        val node = tree.lastSelectedPathComponent as? DefaultMutableTreeNode ?: return
                        val docNode = node.userObject as? DocumentNode ?: return
                        openDocument(project, docNode)
                    }
                }
            },
        )

        val actionGroup = DefaultActionGroup()
        val verifyAction = ActionManager.getInstance().getAction("org.supersigil.ij.verify")
        if (verifyAction != null) {
            actionGroup.add(verifyAction)
        }
        val toolbar =
            ActionManager
                .getInstance()
                .createActionToolbar("SupersigilSpecExplorer", actionGroup, true)

        val panel = SimpleToolWindowPanel(true)
        toolbar.targetComponent = panel
        panel.toolbar = toolbar.component
        panel.setContent(JBScrollPane(tree))

        val content =
            toolWindow.contentManager.factory
                .createContent(panel, null, false)
        toolWindow.contentManager.addContent(content)

        scheduleRefreshWithRetry(project, tree, toolWindow)
    }
}

/**
 * Polls until the LSP server is ready and the tree has data. On each
 * attempt it also tries to attach a `documentsChanged` listener to any
 * newly-appeared server so that subsequent updates arrive without
 * polling. Once a listener is attached, the poll stops — the listener
 * handles all further refreshes.
 *
 * On the very first attempt, if no LSP server is running yet, this
 * proactively triggers server startup so the explorer populates
 * without the user having to open a spec file first.
 */
private fun scheduleRefreshWithRetry(
    project: Project,
    tree: Tree,
    toolWindow: ToolWindow,
) {
    val retryAlarm = Alarm(Alarm.ThreadToUse.POOLED_THREAD, toolWindow.disposable)
    val refreshAlarm = Alarm(Alarm.ThreadToUse.POOLED_THREAD, toolWindow.disposable)
    var listenerAttached = false

    fun tryRefresh() {
        if (project.isDisposed) return

        // Proactively start the LSP server if it hasn't been started yet.
        // The IntelliJ LSP framework normally only starts the server when a
        // matching file is opened in the editor, but the spec explorer needs
        // the server before any file is opened. Retry on each poll so that
        // installing the binary while the IDE is open is picked up
        // automatically.
        if (supersigilServers(project).isEmpty()) {
            ensureSupersigilServerStarted(project)
        }

        // Try to attach the documentsChanged listener if we haven't yet.
        if (!listenerAttached) {
            for (server in supersigilServers(project)) {
                val descriptor = server.descriptor as? SupersigilLspServerDescriptor ?: continue
                descriptor.addDocumentsChangedListener({
                    refreshAlarm.cancelAllRequests()
                    refreshAlarm.addRequest({ scheduleRefresh(project, tree) }, 200)
                }, toolWindow.disposable)
                listenerAttached = true
            }
        }

        val root = fetchTreeRoot(project)
        val settings = SupersigilSettings.getInstance()
        val hasBinary = resolveServerBinary(settings.serverPath) != null

        ToolWindowManager.getInstance(project).invokeLater {
            if (project.isDisposed) return@invokeLater
            updateTree(tree, root, project, hasBinary)

            // Keep retrying until the listener is attached. Once it is,
            // the documentsChanged notification handles all future updates.
            if (!listenerAttached) {
                retryAlarm.addRequest(::tryRefresh, 2000)
            }
        }
    }

    retryAlarm.addRequest(::tryRefresh, 0)
}

private val LINK_ATTRIBUTES =
    SimpleTextAttributes(
        SimpleTextAttributes.STYLE_PLAIN,
        JBColor.namedColor("Link.activeForeground", JBColor(0x2470B3, 0x589DF6)),
    )

private fun setNotInstalledEmptyText(
    tree: Tree,
    project: Project,
) {
    tree.emptyText.clear()
    tree.emptyText.setText("Supersigil LSP server not found. Install to get started")
    tree.emptyText.appendLine(
        notInstalledDetailLine(),
        SimpleTextAttributes.GRAYED_ATTRIBUTES,
        null,
    )
    tree.emptyText.appendLine(
        "This panel updates automatically",
        SimpleTextAttributes.GRAYED_ATTRIBUTES,
        null,
    )
    tree.emptyText.appendLine("")
    tree.emptyText.appendLine("Installation guide", LINK_ATTRIBUTES) { _: java.awt.event.ActionEvent ->
        BrowserUtil.browse(EDITOR_SETUP_URL)
    }
    tree.emptyText.appendLine("Open Settings", LINK_ATTRIBUTES) { _: java.awt.event.ActionEvent ->
        ShowSettingsUtil
            .getInstance()
            .showSettingsDialog(project, SupersigilSettingsConfigurable::class.java)
    }
}

private fun notInstalledDetailLine(): String =
    binaryInstallHint().plainText

private fun setCompatibilityFailureEmptyText(
    tree: Tree,
    project: Project,
    failure: CompatibilityResult.Incompatible,
) {
    tree.emptyText.clear()
    val message =
        if (failure.reason == CompatibilityFailureReason.Mismatch) {
            "Supersigil LSP server is incompatible. Update to get started"
        } else {
            "Supersigil LSP compatibility check failed. Update or reconfigure to get started"
        }
    tree.emptyText.setText(message)
    tree.emptyText.appendLine(
        "This panel updates automatically",
        SimpleTextAttributes.GRAYED_ATTRIBUTES,
        null,
    )
    tree.emptyText.appendLine("")
    tree.emptyText.appendLine("Installation guide", LINK_ATTRIBUTES) { _: java.awt.event.ActionEvent ->
        BrowserUtil.browse(EDITOR_SETUP_URL)
    }
    tree.emptyText.appendLine("Open Settings", LINK_ATTRIBUTES) { _: java.awt.event.ActionEvent ->
        ShowSettingsUtil
            .getInstance()
            .showSettingsDialog(project, SupersigilSettingsConfigurable::class.java)
    }
}

private fun updateTree(
    tree: Tree,
    root: DefaultMutableTreeNode,
    project: Project,
    hasBinary: Boolean,
) {
    val model = tree.model as DefaultTreeModel
    model.setRoot(root)
    model.reload()
    for (i in 0 until tree.rowCount) {
        tree.expandRow(i)
    }
    if (root.childCount == 0) {
        val compatibilityFailure = lastCompatibilityFailure(project)
        if (hasRunningSupersigil(project)) {
            tree.emptyText.setText("No spec documents found")
        } else if (compatibilityFailure != null) {
            setCompatibilityFailureEmptyText(tree, project, compatibilityFailure)
        } else if (hasBinary) {
            tree.emptyText.setText("Waiting for language server\u2026")
        } else {
            setNotInstalledEmptyText(tree, project)
        }
    }
}

private fun scheduleRefresh(
    project: Project,
    tree: Tree,
) {
    ApplicationManager.getApplication().executeOnPooledThread {
        if (project.isDisposed) return@executeOnPooledThread
        val root = fetchTreeRoot(project)
        val settings = SupersigilSettings.getInstance()
        val hasBinary = resolveServerBinary(settings.serverPath) != null
        ToolWindowManager.getInstance(project).invokeLater {
            if (project.isDisposed) return@invokeLater
            updateTree(tree, root, project, hasBinary)
        }
    }
}

private fun fetchTreeRoot(project: Project): DefaultMutableTreeNode {
    val root = DefaultMutableTreeNode("root")

    for (server in supersigilServers(project)) {
        if (server.state != LspServerState.Running) continue

        try {
            // The LSP server's custom `supersigil/documentList` JSON-RPC request is not
            // accessible via IntelliJ's LSP client (can't override getLsp4jServerClass).
            // Instead, we use workspace/executeCommand which the server also handles.
            // The response arrives as untyped gson LinkedTreeMaps via lsp4j deserialization.
            @Suppress("UNCHECKED_CAST")
            val result =
                server.sendRequestSync { languageServer ->
                    languageServer.workspaceService.executeCommand(
                        org.eclipse.lsp4j.ExecuteCommandParams(
                            COMMAND_DOCUMENT_LIST,
                            emptyList(),
                        ),
                    ) as java.util.concurrent.CompletableFuture<Any?>
                }

            val documents = parseDocumentListResponse(result)
            val treeNodes = buildSpecTree(documents)
            for (node in treeNodes) {
                root.add(buildSwingNode(node))
            }
        } catch (e: Exception) {
            LOG.debug("Failed to fetch document list from LSP server", e)
        }
    }

    return root
}

/**
 * Parses the untyped JSON response from `workspace/executeCommand("supersigil.documentList")`.
 * The lsp4j library deserializes JSON objects as `LinkedTreeMap<String, Any?>` instances
 * since the executeCommand response type is `Object`.
 */
@Suppress("UNCHECKED_CAST")
internal fun parseDocumentListResponse(result: Any?): List<DocumentEntry> {
    val map = result as? Map<*, *> ?: return emptyList()
    val documents = map["documents"] as? List<*> ?: return emptyList()
    return documents.mapNotNull { entry ->
        val doc = entry as? Map<*, *> ?: return@mapNotNull null
        DocumentEntry(
            id = doc["id"] as? String ?: return@mapNotNull null,
            docType = doc["doc_type"] as? String ?: return@mapNotNull null,
            status = doc["status"] as? String,
            path = doc["path"] as? String ?: return@mapNotNull null,
            project = doc["project"] as? String,
        )
    }
}

private fun buildSwingNode(node: SpecTreeNode): DefaultMutableTreeNode {
    val treeNode = DefaultMutableTreeNode(node)
    when (node) {
        is ProjectNode -> {
            node.children.forEach { treeNode.add(buildSwingNode(it)) }
        }

        is GroupNode -> {
            node.children.forEach { treeNode.add(buildSwingNode(it)) }
        }

        is DocumentNode -> {}
    }
    return treeNode
}

private fun openDocument(
    project: Project,
    docNode: DocumentNode,
) {
    val basePath = project.basePath ?: return
    val file =
        LocalFileSystem
            .getInstance()
            .findFileByPath("$basePath/${docNode.path}") ?: return
    FileEditorManager.getInstance(project).openFile(file, true)
}

private val STATUS_GREEN =
    SimpleTextAttributes(
        SimpleTextAttributes.STYLE_PLAIN,
        JBColor.namedColor("JBUI.CurrentTheme.Label.successForeground", JBColor(0x368746, 0x499C54)),
    )

private class SpecTreeCellRenderer : com.intellij.ui.ColoredTreeCellRenderer() {
    override fun customizeCellRenderer(
        tree: JTree,
        value: Any?,
        selected: Boolean,
        expanded: Boolean,
        leaf: Boolean,
        row: Int,
        hasFocus: Boolean,
    ) {
        val node = (value as? DefaultMutableTreeNode)?.userObject

        when (node) {
            is ProjectNode -> {
                append(node.label)
                icon = AllIcons.Nodes.Tag
                toolTipText = null
            }

            is GroupNode -> {
                append(node.label)
                append("  ${node.documentCount} documents", SimpleTextAttributes.GRAYED_ATTRIBUTES)
                icon = AllIcons.Nodes.Folder
                toolTipText = null
            }

            is DocumentNode -> {
                append(node.label)
                val statusAttrs =
                    when (statusColor(node.status)) {
                        StatusColor.GREEN -> STATUS_GREEN
                        StatusColor.GRAY -> SimpleTextAttributes.GRAYED_ATTRIBUTES
                        StatusColor.DEFAULT -> SimpleTextAttributes.GRAYED_ATTRIBUTES
                    }
                append("  ${node.description}", statusAttrs)
                icon = iconForDocType(node.docType)
                toolTipText = node.id
            }
        }
    }
}

private fun iconForDocType(docType: String): Icon =
    when (docTypeIcon(docType)) {
        DocTypeIcon.REQUIREMENTS -> AllIcons.Actions.Checked
        DocTypeIcon.DESIGN -> AllIcons.Actions.Edit
        DocTypeIcon.TASKS -> AllIcons.Nodes.Tag
        DocTypeIcon.DECISION -> AllIcons.Actions.IntentionBulb
        DocTypeIcon.DOCUMENTATION -> AllIcons.FileTypes.Text
        DocTypeIcon.OTHER -> AllIcons.FileTypes.Any_type
    }
