package org.supersigil.intellij

import com.google.gson.JsonParser
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.attribute.BasicFileAttributes
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

// Keep this aligned with the server's compatibility constant. Bump it only for
// editor-visible protocol changes; package-version bumps alone do not require it.
const val SUPPORTED_COMPATIBILITY_VERSION = 1
private const val COMPATIBILITY_INFO_TIMEOUT_MILLIS = 500L

data class CompatibilityInfo(
    val compatibilityVersion: Int,
    val serverVersion: String,
)

enum class CompatibilityFailureReason {
    Mismatch,
    QueryFailed,
    InvalidResponse,
}

sealed class CompatibilityResult {
    data class Compatible(
        val supportedVersion: Int,
        val reportedVersion: Int,
        val serverVersion: String,
    ) : CompatibilityResult()

    data class Incompatible(
        val reason: CompatibilityFailureReason,
        val supportedVersion: Int,
        val reportedVersion: Int?,
        val serverVersion: String?,
    ) : CompatibilityResult()
}

data class CompatibilityCommandResult(
    val exitCode: Int,
    val stdout: String,
    val stderr: String,
)

private data class CompatibilityCacheKey(
    val binaryPath: String,
    val binaryStamp: String,
)

private val compatibilityInfoCache = ConcurrentHashMap<CompatibilityCacheKey, CompatibilityResult>()

fun parseCompatibilityInfo(stdout: String): CompatibilityInfo? {
    val json =
        runCatching { JsonParser.parseString(stdout).asJsonObject }
            .getOrNull()
            ?: return null

    val compatibilityElement = json.get("compatibility_version")
    val serverVersionElement = json.get("server_version")
    if (
        compatibilityElement == null ||
        !compatibilityElement.isJsonPrimitive ||
        !compatibilityElement.asJsonPrimitive.isNumber ||
        serverVersionElement == null ||
        !serverVersionElement.isJsonPrimitive ||
        !serverVersionElement.asJsonPrimitive.isString
    ) {
        return null
    }

    return CompatibilityInfo(
        compatibilityVersion = compatibilityElement.asInt,
        serverVersion = serverVersionElement.asString,
    )
}

fun checkCompatibilityInfo(
    info: CompatibilityInfo?,
    supportedVersion: Int = SUPPORTED_COMPATIBILITY_VERSION,
): CompatibilityResult =
    when {
        info == null ->
            CompatibilityResult.Incompatible(
                reason = CompatibilityFailureReason.InvalidResponse,
                supportedVersion = supportedVersion,
                reportedVersion = null,
                serverVersion = null,
            )

        info.compatibilityVersion != supportedVersion ->
            CompatibilityResult.Incompatible(
                reason = CompatibilityFailureReason.Mismatch,
                supportedVersion = supportedVersion,
                reportedVersion = info.compatibilityVersion,
                serverVersion = info.serverVersion,
            )

        else ->
            CompatibilityResult.Compatible(
                supportedVersion = supportedVersion,
                reportedVersion = info.compatibilityVersion,
                serverVersion = info.serverVersion,
            )
    }

fun queryCompatibilityInfo(
    binaryPath: String,
    commandRunner: (List<String>) -> CompatibilityCommandResult = ::runCompatibilityInfoCommand,
): CompatibilityResult {
    val result =
        runCatching { commandRunner(listOf(binaryPath, "--compatibility-info")) }
            .getOrElse {
                return CompatibilityResult.Incompatible(
                    reason = CompatibilityFailureReason.QueryFailed,
                    supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
                    reportedVersion = null,
                    serverVersion = null,
                )
            }
    if (result.exitCode != 0) {
        return CompatibilityResult.Incompatible(
            reason = CompatibilityFailureReason.QueryFailed,
            supportedVersion = SUPPORTED_COMPATIBILITY_VERSION,
            reportedVersion = null,
            serverVersion = null,
        )
    }

    return checkCompatibilityInfo(parseCompatibilityInfo(result.stdout))
}

fun queryCompatibilityInfoCached(
    binaryPath: String,
    commandRunner: (List<String>) -> CompatibilityCommandResult = ::runCompatibilityInfoCommand,
    stampProvider: (String) -> String = ::compatibilityCacheStamp,
): CompatibilityResult {
    val cacheKey = CompatibilityCacheKey(binaryPath, stampProvider(binaryPath))
    compatibilityInfoCache.keys.removeIf { it.binaryPath == binaryPath && it != cacheKey }
    compatibilityInfoCache[cacheKey]?.let { return it }

    val result = queryCompatibilityInfo(binaryPath, commandRunner)
    if (
        result is CompatibilityResult.Incompatible &&
        result.reason == CompatibilityFailureReason.QueryFailed
    ) {
        return result
    }

    return compatibilityInfoCache.putIfAbsent(cacheKey, result) ?: result
}

fun clearCompatibilityInfoCache() {
    compatibilityInfoCache.clear()
}

private fun compatibilityCacheStamp(binaryPath: String): String =
    runCatching {
        val attributes = Files.readAttributes(Path.of(binaryPath), BasicFileAttributes::class.java)
        "${attributes.lastModifiedTime().toMillis()}:${attributes.size()}"
    }.getOrElse { "unavailable" }

private fun runCompatibilityInfoCommand(command: List<String>): CompatibilityCommandResult {
    val process = ProcessBuilder(command).start()
    val finished = process.waitFor(COMPATIBILITY_INFO_TIMEOUT_MILLIS, TimeUnit.MILLISECONDS)
    if (!finished) {
        process.destroy()
        if (!process.waitFor(100, TimeUnit.MILLISECONDS)) {
            process.destroyForcibly()
            process.waitFor(100, TimeUnit.MILLISECONDS)
        }
        return CompatibilityCommandResult(
            exitCode = -1,
            stdout = "",
            stderr = "",
        )
    }

    return CompatibilityCommandResult(
        exitCode = process.exitValue(),
        stdout = process.inputStream.bufferedReader().use { it.readText() },
        stderr = process.errorStream.bufferedReader().use { it.readText() },
    )
}
