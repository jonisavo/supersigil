package org.supersigil.intellij

import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.options.ShowSettingsUtil
import com.intellij.openapi.project.Project

private const val NOTIFICATION_GROUP = "Supersigil"

fun notifyBinaryNotFound(
    project: Project,
    configuredPath: String?,
) {
    val (message, type) =
        if (configuredPath != null) {
            "Supersigil LSP server not found at configured path: $configuredPath" to NotificationType.ERROR
        } else {
            (
                "Supersigil LSP server not found. Install with <code>cargo install supersigil-lsp</code> " +
                    "or configure the path in Settings > Tools > Supersigil."
            ) to NotificationType.WARNING
        }

    NotificationGroupManager
        .getInstance()
        .getNotificationGroup(NOTIFICATION_GROUP)
        .createNotification(message, type)
        .addAction(
            com.intellij.notification.NotificationAction.createSimpleExpiring("Open Settings") {
                ShowSettingsUtil
                    .getInstance()
                    .showSettingsDialog(project, SupersigilSettingsConfigurable::class.java)
            },
        ).notify(project)
}
