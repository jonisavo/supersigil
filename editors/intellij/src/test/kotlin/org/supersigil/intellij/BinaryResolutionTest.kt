package org.supersigil.intellij

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BinaryResolutionTest {
    @Test
    fun `configured path is used when file exists`() {
        val result =
            resolveBinaryPath(
                configuredPath = "/usr/local/bin/supersigil-lsp",
                fileExists = { it == "/usr/local/bin/supersigil-lsp" },
                pathLookup = { null },
            )
        assertEquals("/usr/local/bin/supersigil-lsp", result)
    }

    @Test
    fun `configured path returns null when file does not exist`() {
        val result =
            resolveBinaryPath(
                configuredPath = "/nonexistent/supersigil-lsp",
                fileExists = { false },
                pathLookup = { null },
            )
        assertNull(result)
    }

    @Test
    fun `PATH lookup is used when no configured path`() {
        val result =
            resolveBinaryPath(
                configuredPath = null,
                fileExists = { false },
                pathLookup = { "/usr/bin/supersigil-lsp" },
            )
        assertEquals("/usr/bin/supersigil-lsp", result)
    }

    @Test
    fun `cargo bin fallback is used when PATH lookup fails`() {
        val result =
            resolveBinaryPath(
                configuredPath = null,
                fileExists = { it == "/home/user/.cargo/bin/supersigil-lsp" },
                pathLookup = { null },
                fallbackPaths =
                    listOf(
                        "/home/user/.cargo/bin/supersigil-lsp",
                        "/home/user/.local/bin/supersigil-lsp",
                    ),
            )
        assertEquals("/home/user/.cargo/bin/supersigil-lsp", result)
    }

    @Test
    fun `local bin fallback is used when cargo bin does not exist`() {
        val result =
            resolveBinaryPath(
                configuredPath = null,
                fileExists = { it == "/home/user/.local/bin/supersigil-lsp" },
                pathLookup = { null },
                fallbackPaths =
                    listOf(
                        "/home/user/.cargo/bin/supersigil-lsp",
                        "/home/user/.local/bin/supersigil-lsp",
                    ),
            )
        assertEquals("/home/user/.local/bin/supersigil-lsp", result)
    }

    @Test
    fun `returns null when nothing is found`() {
        val result =
            resolveBinaryPath(
                configuredPath = null,
                fileExists = { false },
                pathLookup = { null },
            )
        assertNull(result)
    }

    @Test
    fun `configured path takes precedence over PATH`() {
        val result =
            resolveBinaryPath(
                configuredPath = "/custom/supersigil-lsp",
                fileExists = { true },
                pathLookup = { "/usr/bin/supersigil-lsp" },
            )
        assertEquals("/custom/supersigil-lsp", result)
    }

    @Test
    fun `PATH takes precedence over fallbacks`() {
        val result =
            resolveBinaryPath(
                configuredPath = null,
                fileExists = { true },
                pathLookup = { "/usr/bin/supersigil-lsp" },
                fallbackPaths = listOf("/home/user/.cargo/bin/supersigil-lsp"),
            )
        assertEquals("/usr/bin/supersigil-lsp", result)
    }
}
