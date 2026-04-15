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
private val compatibilityNotifications =
    java.util.WeakHashMap<Project, String>()

private fun openSupersigilSettingsAction(project: Project) =
    com.intellij.notification.NotificationAction.createSimpleExpiring("Open Settings") {
        ShowSettingsUtil
            .getInstance()
            .showSettingsDialog(project, SupersigilSettingsConfigurable::class.java)
    }

private fun openInstallationGuideAction() =
    com.intellij.notification.NotificationAction.createSimpleExpiring("Installation Guide") {
        BrowserUtil.browse(EDITOR_SETUP_URL)
    }

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
            ).addAction(openSupersigilSettingsAction(project))
            .notify(project)
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
        ).addAction(openSupersigilSettingsAction(project))
        .addAction(openInstallationGuideAction())
        .notify(project)
}

private fun reportedCompatibilityVersion(result: CompatibilityResult.Incompatible): String =
    result.reportedVersion?.toString() ?: "unavailable"

fun notifyIncompatibleServer(
    project: Project,
    binaryPath: String,
    result: CompatibilityResult.Incompatible,
) {
    val notificationKey =
        listOf(
            binaryPath,
            result.reason.name,
            result.supportedVersion.toString(),
            reportedCompatibilityVersion(result),
            result.serverVersion ?: "unavailable",
        ).joinToString("|")
    if (compatibilityNotifications[project] == notificationKey) {
        return
    }
    compatibilityNotifications[project] = notificationKey

    val message =
        "Supersigil compatibility mismatch for <code>$binaryPath</code>. " +
            "This plugin supports compatibility version <code>${result.supportedVersion}</code>, " +
            "and the server reported <code>${reportedCompatibilityVersion(result)}</code> " +
            "(server package version <code>${result.serverVersion ?: "unavailable"}</code>). " +
            "Update the plugin or supersigil-lsp before continuing."

    NotificationGroupManager
        .getInstance()
        .getNotificationGroup(NOTIFICATION_GROUP)
        .createNotification(
            message,
            NotificationType.ERROR,
        ).setImportant(true)
        .addAction(
            com.intellij.notification.NotificationAction.createSimpleExpiring("Open Plugins") {
                ShowSettingsUtil.getInstance().showSettingsDialog(project, "Plugins")
            },
        ).addAction(openSupersigilSettingsAction(project))
        .addAction(openInstallationGuideAction())
        .notify(project)
}
