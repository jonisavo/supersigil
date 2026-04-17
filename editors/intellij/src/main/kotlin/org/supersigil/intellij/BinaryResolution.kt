package org.supersigil.intellij

import com.intellij.execution.configurations.PathEnvironmentVariableUtil
import java.io.File

private const val BINARY_NAME = "supersigil-lsp"

internal fun binaryNameForOs(osName: String = System.getProperty("os.name")): String =
    if (osName.startsWith("Windows", ignoreCase = true)) "$BINARY_NAME.exe" else BINARY_NAME

/**
 * Returns the path to the supersigil-lsp binary, or null if not found.
 *
 * Resolution order:
 * 1. Configured path (if set and file exists)
 * 2. PATH lookup
 * 3. Fallback paths (e.g. ~/.cargo/bin, ~/.local/bin)
 */
fun resolveBinaryPath(
    configuredPath: String?,
    fileExists: (String) -> Boolean,
    pathLookup: () -> String?,
    fallbackPaths: List<String> = defaultFallbackPaths(),
): String? {
    if (configuredPath != null) {
        return if (fileExists(configuredPath)) configuredPath else null
    }

    val fromPath = pathLookup()
    if (fromPath != null) {
        return fromPath
    }

    for (candidate in fallbackPaths) {
        if (fileExists(candidate)) {
            return candidate
        }
    }

    return null
}

/**
 * Resolve the binary using real filesystem and PATH lookup.
 */
fun resolveServerBinary(configuredPath: String?): String? =
    resolveBinaryPath(
        configuredPath = configuredPath,
        fileExists = { File(it).let { f -> f.exists() && f.canExecute() } },
        pathLookup = {
            PathEnvironmentVariableUtil.findInPath(binaryNameForOs())?.absolutePath
        },
    )

fun defaultFallbackPaths(
    home: String = System.getProperty("user.home"),
    osName: String = System.getProperty("os.name"),
): List<String> {
    val binaryName = binaryNameForOs(osName)

    return if (osName.startsWith("Windows", ignoreCase = true)) {
        listOf("$home\\.cargo\\bin\\$binaryName")
    } else {
        listOf(
            "$home/.cargo/bin/$binaryName",
            "$home/.local/bin/$binaryName",
        )
    }
}
