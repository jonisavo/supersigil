package org.supersigil.intellij

import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.lang.Language
import org.intellij.plugins.markdown.injection.CodeFenceLanguageProvider

class SupersigilCodeFenceLanguageProvider : CodeFenceLanguageProvider {
    override fun getLanguageByInfoString(infoString: String): Language? =
        when (infoString.trim().lowercase()) {
            "supersigil-xml" -> Language.findLanguageByID("XML")
            else -> null
        }

    override fun getCompletionVariantsForInfoString(parameters: CompletionParameters): List<LookupElement> = emptyList()
}
