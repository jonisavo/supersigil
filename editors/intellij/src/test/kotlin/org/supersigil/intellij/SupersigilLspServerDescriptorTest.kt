package org.supersigil.intellij

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SupersigilLspServerDescriptorTest {
    @Test
    fun `creates a direct Windows command line for the resolved exe`() {
        val commandLine =
            createSupersigilCommandLine(
                "C:\\Users\\example-user\\.cargo\\bin\\supersigil-lsp.exe",
                "C:\\projects\\supersigil",
            )

        assertEquals(
            "C:\\Users\\example-user\\.cargo\\bin\\supersigil-lsp.exe",
            commandLine.exePath,
        )
        assertEquals("C:\\projects\\supersigil", commandLine.workDirectory?.path)
    }

    @Test
    fun `allows a missing work directory for descriptor startup`() {
        val commandLine = createSupersigilCommandLine("/usr/local/bin/supersigil-lsp", null)

        assertEquals("/usr/local/bin/supersigil-lsp", commandLine.exePath)
        assertNull(commandLine.workDirectory)
    }
}
