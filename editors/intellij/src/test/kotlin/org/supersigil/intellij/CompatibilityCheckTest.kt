package org.supersigil.intellij

import java.nio.file.Files
import java.nio.file.Path
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CompatibilityCheckTest {
    private fun tempCommandScript(
        prefix: String,
        unixBody: String,
        windowsBody: String,
    ): Path {
        val suffix = if (System.getProperty("os.name").startsWith("Windows", ignoreCase = true)) ".cmd" else ".sh"
        val body =
            if (suffix == ".cmd") {
                windowsBody
            } else {
                unixBody
            }

        return Files.createTempFile(prefix, suffix).also { path ->
            Files.writeString(path, body)
            path.toFile().setExecutable(true)
            path.toFile().deleteOnExit()
        }
    }

    @After
    fun clearCache() {
        clearCompatibilityInfoCache()
    }

    @Test
    fun `parses compatibility info JSON`() {
        assertEquals(
            CompatibilityInfo(
                compatibilityVersion = 1,
                serverVersion = "0.10.0",
            ),
            parseCompatibilityInfo("""{"compatibility_version":1,"server_version":"0.10.0"}"""),
        )
    }

    @Test
    fun `returns null when compatibility version is missing`() {
        assertNull(parseCompatibilityInfo("""{"server_version":"0.10.0"}"""))
    }

    @Test
    fun `accepts a matching compatibility version`() {
        assertEquals(
            CompatibilityResult.Compatible(
                supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                reportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                serverVersion = "0.10.0",
            ),
            checkCompatibilityInfo(
                CompatibilityInfo(
                    compatibilityVersion = SUPPORTED_COMPATIBILITY_VERSION,
                    serverVersion = "0.10.0",
                ),
            ),
        )
    }

    @Test
    fun `rejects a mismatched compatibility version`() {
        assertEquals(
            CompatibilityResult.Incompatible(
                reason = CompatibilityFailureReason.Mismatch,
                supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                reportedVersion = SUPPORTED_COMPATIBILITY_VERSION + 1,
                serverVersion = "0.11.0",
            ),
            checkCompatibilityInfo(
                CompatibilityInfo(
                    compatibilityVersion = SUPPORTED_COMPATIBILITY_VERSION + 1,
                    serverVersion = "0.11.0",
                ),
            ),
        )
    }

    @Test
    fun `treats a failed preflight command as incompatible`() {
        assertEquals(
            CompatibilityResult.Incompatible(
                reason = CompatibilityFailureReason.QueryFailed,
                supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                reportedVersion = null,
                serverVersion = null,
            ),
            queryCompatibilityInfo("/tmp/supersigil-lsp") {
                CompatibilityCommandResult(
                    exitCode = 1,
                    stdout = "",
                    stderr = "boom",
                )
            },
        )
    }

    @Test
    fun `treats probe launch exceptions as incompatible`() {
        assertEquals(
            CompatibilityResult.Incompatible(
                reason = CompatibilityFailureReason.QueryFailed,
                supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                reportedVersion = null,
                serverVersion = null,
            ),
            queryCompatibilityInfo("/tmp/supersigil-lsp") {
                throw IllegalStateException("boom")
            },
        )
    }

    @Test
    fun `accepts valid stdout even when stderr contains warnings`() {
        val script =
            tempCommandScript(
                prefix = "supersigil-compatibility-stderr",
                unixBody =
                    """
                    |#!/usr/bin/env sh
                    |printf 'warning: wrapper noise\n' >&2
                    |printf '{"compatibility_version":1,"server_version":"0.10.0"}\n'
                    """.trimMargin(),
                windowsBody =
                    """
                    |@echo off
                    |echo warning: wrapper noise 1>&2
                    |echo {"compatibility_version":1,"server_version":"0.10.0"}
                    """.trimMargin(),
            )

        assertEquals(
            CompatibilityResult.Compatible(
                supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                reportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                serverVersion = "0.10.0",
            ),
            queryCompatibilityInfo(script.toString()),
        )
    }

    @Test(timeout = 1000)
    fun `treats a hanging preflight command as incompatible`() {
        val script =
            tempCommandScript(
                prefix = "supersigil-compatibility-hang",
                unixBody =
                    """
                    |#!/usr/bin/env sh
                    |sleep 5
                    """.trimMargin(),
                windowsBody =
                    """
                    |@echo off
                    |timeout /t 5 /nobreak >nul
                    """.trimMargin(),
            )

        assertEquals(
            CompatibilityResult.Incompatible(
                reason = CompatibilityFailureReason.QueryFailed,
                supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                reportedVersion = null,
                serverVersion = null,
            ),
            queryCompatibilityInfo(script.toString()),
        )
    }

    @Test
    fun `reuses cached compatibility queries for the same binary stamp`() {
        var invocations = 0

        val first =
            queryCompatibilityInfoCached(
                binaryPath = "/tmp/supersigil-lsp",
                commandRunner = {
                    invocations += 1
                    CompatibilityCommandResult(
                        exitCode = 0,
                        stdout = """{"compatibility_version":1,"server_version":"0.10.0"}""",
                        stderr = "",
                    )
                },
                stampProvider = { "stable-stamp" },
            )
        val second =
            queryCompatibilityInfoCached(
                binaryPath = "/tmp/supersigil-lsp",
                commandRunner = {
                    invocations += 1
                    CompatibilityCommandResult(
                        exitCode = 0,
                        stdout = """{"compatibility_version":1,"server_version":"0.10.0"}""",
                        stderr = "",
                    )
                },
                stampProvider = { "stable-stamp" },
            )

        assertEquals(first, second)
        assertEquals(1, invocations)
    }

    @Test
    fun `does not cache transient query failures`() {
        var invocations = 0

        val first =
            queryCompatibilityInfoCached(
                binaryPath = "/tmp/supersigil-lsp",
                commandRunner = {
                    invocations += 1
                    CompatibilityCommandResult(
                        exitCode = 1,
                        stdout = "",
                        stderr = "boom",
                    )
                },
                stampProvider = { "stable-stamp" },
            )
        val second =
            queryCompatibilityInfoCached(
                binaryPath = "/tmp/supersigil-lsp",
                commandRunner = {
                    invocations += 1
                    CompatibilityCommandResult(
                        exitCode = 0,
                        stdout = """{"compatibility_version":1,"server_version":"0.10.0"}""",
                        stderr = "",
                    )
                },
                stampProvider = { "stable-stamp" },
            )

        assertEquals(
            CompatibilityResult.Incompatible(
                reason = CompatibilityFailureReason.QueryFailed,
                supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                reportedVersion = null,
                serverVersion = null,
            ),
            first,
        )
        assertEquals(
            CompatibilityResult.Compatible(
                supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                reportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                serverVersion = "0.10.0",
            ),
            second,
        )
        assertEquals(2, invocations)
    }

    @Test
    fun `invalidates cached compatibility queries when the binary stamp changes`() {
        var invocations = 0
        var stamp = "stamp-a"

        queryCompatibilityInfoCached(
            binaryPath = "/tmp/supersigil-lsp",
            commandRunner = {
                invocations += 1
                CompatibilityCommandResult(
                    exitCode = 0,
                    stdout = """{"compatibility_version":1,"server_version":"0.10.0"}""",
                    stderr = "",
                )
            },
            stampProvider = { stamp },
        )

        stamp = "stamp-b"

        queryCompatibilityInfoCached(
            binaryPath = "/tmp/supersigil-lsp",
            commandRunner = {
                invocations += 1
                CompatibilityCommandResult(
                    exitCode = 0,
                    stdout = """{"compatibility_version":1,"server_version":"0.10.1"}""",
                    stderr = "",
                )
            },
            stampProvider = { stamp },
        )

        assertEquals(2, invocations)
    }
}
