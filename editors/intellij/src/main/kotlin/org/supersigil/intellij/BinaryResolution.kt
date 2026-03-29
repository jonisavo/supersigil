package org.supersigil.intellij

import com.intellij.execution.configurations.PathEnvironmentVariableUtil
import java.io.File

private const val BINARY_NAME = "supersigil-lsp"

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
            PathEnvironmentVariableUtil.findInPath(BINARY_NAME)?.absolutePath
        },
    )

fun defaultFallbackPaths(): List<String> {
    val home = System.getProperty("user.home")
    return listOf(
        "$home/.cargo/bin/$BINARY_NAME",
        "$home/.local/bin/$BINARY_NAME",
    )
}
