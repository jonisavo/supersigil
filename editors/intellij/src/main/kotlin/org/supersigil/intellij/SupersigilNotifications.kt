package org.supersigil.intellij

import com.intellij.ide.BrowserUtil
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.options.ShowSettingsUtil
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.SystemInfo
import com.intellij.openapi.wm.ToolWindowManager

private const val NOTIFICATION_GROUP = "Supersigil"
internal const val EDITOR_SETUP_URL = "https://supersigil.org/guides/editor-setup/"

private val notifiedProjects = java.util.Collections.newSetFromMap(java.util.WeakHashMap<Project, Boolean>())

fun notifyBinaryNotFound(
    project: Project,
    configuredPath: String?,
) {
    if (!notifiedProjects.add(project)) return
    if (configuredPath != null) {
        NotificationGroupManager
            .getInstance()
            .getNotificationGroup(NOTIFICATION_GROUP)
            .createNotification(
                "Supersigil LSP server not found at configured path: $configuredPath",
                NotificationType.ERROR,
            ).addAction(
                com.intellij.notification.NotificationAction.createSimpleExpiring("Open Settings") {
                    ShowSettingsUtil
                        .getInstance()
                        .showSettingsDialog(project, SupersigilSettingsConfigurable::class.java)
                },
            ).notify(project)
        return
    }

    val installHint =
        when {
            SystemInfo.isMac ->
                "Install with <code>brew install jonisavo/supersigil/supersigil</code>"
            SystemInfo.isWindows ->
                "Download from <a href=\"https://github.com/jonisavo/supersigil/releases\">GitHub Releases</a> " +
                    "or install with <code>cargo install supersigil-lsp</code>"
            else ->
                "Install with your package manager or <code>cargo install supersigil-lsp</code>"
        }

    NotificationGroupManager
        .getInstance()
        .getNotificationGroup(NOTIFICATION_GROUP)
        .createNotification(
            "Supersigil LSP server not found. $installHint, " +
                "or configure the path in Settings &gt; Tools &gt; Supersigil.",
            NotificationType.WARNING,
        ).setImportant(true)
        .addAction(
            com.intellij.notification.NotificationAction.createSimpleExpiring("Open Terminal") {
                ToolWindowManager.getInstance(project).getToolWindow("Terminal")?.activate(null)
            },
        ).addAction(
            com.intellij.notification.NotificationAction.createSimpleExpiring("Open Settings") {
                ShowSettingsUtil
                    .getInstance()
                    .showSettingsDialog(project, SupersigilSettingsConfigurable::class.java)
            },
        ).addAction(
            com.intellij.notification.NotificationAction.createSimpleExpiring("Installation Guide") {
                BrowserUtil.browse(EDITOR_SETUP_URL)
            },
        ).notify(project)
}
