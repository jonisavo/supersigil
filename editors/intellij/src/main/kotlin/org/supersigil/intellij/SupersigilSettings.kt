package org.supersigil.intellij

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage

@Service(Service.Level.APP)
@State(
    name = "org.supersigil.intellij.SupersigilSettings",
    storages = [Storage("Supersigil.xml")],
)
class SupersigilSettings : PersistentStateComponent<SupersigilSettings.State> {
    data class State(
        var serverPath: String? = null,
    )

    private var state = State()

    override fun getState(): State = state

    override fun loadState(state: State) {
        this.state = state
    }

    var serverPath: String?
        get() = state.serverPath?.ifBlank { null }
        set(value) {
            state.serverPath = value?.ifBlank { null }
        }

    companion object {
        fun getInstance(): SupersigilSettings = ApplicationManager.getApplication().getService(SupersigilSettings::class.java)
    }
}
