package org.supersigil.intellij

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

class SupersigilSettingsConfigurable : Configurable {
    private var serverPathField: TextFieldWithBrowseButton? = null
    private var panel: JPanel? = null

    override fun getDisplayName(): String = "Supersigil"

    override fun createComponent(): JComponent {
        val descriptor =
            FileChooserDescriptorFactory
                .singleFile()
                .withTitle("Select supersigil-lsp Binary")
                .withDescription("Path to the supersigil-lsp executable")
        val field = TextFieldWithBrowseButton()
        field.addBrowseFolderListener(null, descriptor)
        serverPathField = field

        panel =
            FormBuilder
                .createFormBuilder()
                .addLabeledComponent("Server path:", field)
                .addComponentFillVertically(JPanel(), 0)
                .panel

        return panel!!
    }

    override fun isModified(): Boolean {
        val settings = SupersigilSettings.getInstance()
        return serverPathField?.text?.ifBlank { null } != settings.serverPath
    }

    override fun apply() {
        val settings = SupersigilSettings.getInstance()
        settings.serverPath = serverPathField?.text
    }

    override fun reset() {
        val settings = SupersigilSettings.getInstance()
        serverPathField?.text = settings.serverPath ?: ""
    }

    override fun disposeUIResources() {
        serverPathField = null
        panel = null
    }
}
