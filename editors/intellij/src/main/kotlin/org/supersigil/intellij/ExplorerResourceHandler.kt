package org.supersigil.intellij

import org.cef.browser.CefBrowser
import org.cef.browser.CefFrame
import org.cef.callback.CefCallback
import org.cef.handler.CefRequestHandlerAdapter
import org.cef.handler.CefResourceHandler
import org.cef.handler.CefResourceRequestHandler
import org.cef.handler.CefResourceRequestHandlerAdapter
import org.cef.misc.BoolRef
import org.cef.misc.IntRef
import org.cef.misc.StringRef
import org.cef.network.CefRequest
import org.cef.network.CefResponse
import java.io.ByteArrayInputStream
import java.io.InputStream

/** URL prefix for the synthetic explorer origin. */
internal const val EXPLORER_ORIGIN = "https://supersigil-explorer/"

/** Classpath directory where explorer assets live. */
private const val CLASSPATH_PREFIX = "supersigil-explorer/"

/**
 * Known explorer resources that may be served. Requests for paths
 * outside this set are rejected (return null) for security.
 */
private val ALLOWED_RESOURCES: Set<String> = setOf(
    "explorer.js",
    "render-iife.js",
    "supersigil-preview.js",
    "supersigil-preview.css",
    "landing-tokens.css",
    "explorer-styles.css",
    "intellij-theme-adapter.css",
    "explorer-bridge.js",
)

// ---------------------------------------------------------------------------
// Pure functions (testable without CEF)
// ---------------------------------------------------------------------------

/**
 * Extracts the resource filename from a full URL if the URL belongs to
 * the explorer origin and the filename is in the allowed set.
 *
 * Returns null if the URL doesn't match or the resource is not allowed.
 */
internal fun resolveExplorerResource(url: String): String? {
    if (!url.startsWith(EXPLORER_ORIGIN)) return null
    val path = url.removePrefix(EXPLORER_ORIGIN)
    // Reject empty path, paths with directory traversal, and unlisted files
    if (path.isEmpty() || path.contains("..") || path.contains("/")) return null
    return if (path in ALLOWED_RESOURCES) path else null
}

/**
 * Returns the MIME type for a resource filename based on its extension.
 */
internal fun mimeTypeForResource(filename: String): String? {
    val ext = filename.substringAfterLast('.', "")
    return when (ext) {
        "js" -> "application/javascript"
        "css" -> "text/css"
        else -> null
    }
}

// ---------------------------------------------------------------------------
// CefRequestHandler — entry point registered on JBCefClient
// ---------------------------------------------------------------------------

/**
 * A [CefRequestHandlerAdapter] that intercepts requests to the
 * `https://supersigil-explorer/` origin and serves classpath resources.
 *
 * Register on a [com.intellij.ui.jcef.JBCefClient]:
 * ```
 * client.addRequestHandler(ExplorerResourceRequestHandler(), browser)
 * ```
 *
 * @param classLoaderProvider supplies the ClassLoader used to locate
 *   resources. Defaults to the plugin's own classloader. Extracted as
 *   a parameter for testability.
 */
class ExplorerResourceRequestHandler(
    private val classLoaderProvider: () -> ClassLoader = { ExplorerResourceRequestHandler::class.java.classLoader },
) : CefRequestHandlerAdapter() {

    private val resourceRequestHandler = object : CefResourceRequestHandlerAdapter() {
        override fun getResourceHandler(
            browser: CefBrowser?,
            frame: CefFrame?,
            request: CefRequest?,
        ): CefResourceHandler? {
            val url = request?.url ?: return rejectedResourceHandler()
            val resourceName = resolveExplorerResource(url) ?: return rejectedResourceHandler()
            val mimeType = mimeTypeForResource(resourceName) ?: return rejectedResourceHandler()
            val classpathPath = "$CLASSPATH_PREFIX$resourceName"
            val stream = classLoaderProvider().getResourceAsStream(classpathPath) ?: return rejectedResourceHandler()
            return ExplorerCefResourceHandler(stream, 200, "OK", mimeType)
        }
    }

    override fun getResourceRequestHandler(
        browser: CefBrowser?,
        frame: CefFrame?,
        request: CefRequest?,
        isNavigation: Boolean,
        isDownload: Boolean,
        requestInitiator: String?,
        disableDefaultHandling: BoolRef?,
    ): CefResourceRequestHandler? {
        val url = request?.url ?: return null
        if (url.startsWith(EXPLORER_ORIGIN)) {
            disableDefaultHandling?.set(true)
            return resourceRequestHandler
        }
        return null
    }
}

// ---------------------------------------------------------------------------
// CefResourceHandler — streams a single classpath resource
// ---------------------------------------------------------------------------

/**
 * A [CefResourceHandler] that reads an [InputStream] into memory on
 * `processRequest` and serves it via `readResponse`.
 *
 * Resources are small (JS/CSS), so buffering the whole thing is fine.
 */
private class ExplorerCefResourceHandler(
    private val inputStream: InputStream,
    private val status: Int,
    private val statusText: String,
    private val mimeType: String?,
) : CefResourceHandler {

    private var data: ByteArray = ByteArray(0)
    private var offset: Int = 0

    override fun processRequest(request: CefRequest?, callback: CefCallback?): Boolean {
        data = inputStream.use { it.readAllBytes() }
        offset = 0
        callback?.Continue()
        return true
    }

    override fun getResponseHeaders(
        response: CefResponse?,
        responseLength: IntRef?,
        redirectUrl: StringRef?,
    ) {
        response?.apply {
            mimeType?.let { setMimeType(it) }
            setStatus(status)
            setStatusText(statusText)
            setHeaderByName("Access-Control-Allow-Origin", "*", true)
        }
        responseLength?.set(data.size)
    }

    override fun readResponse(
        dataOut: ByteArray?,
        bytesToRead: Int,
        bytesRead: IntRef?,
        callback: CefCallback?,
    ): Boolean {
        if (dataOut == null || bytesRead == null) return false
        val remaining = data.size - offset
        if (remaining <= 0) {
            bytesRead.set(0)
            return false
        }
        val toWrite = minOf(bytesToRead, remaining)
        System.arraycopy(data, offset, dataOut, 0, toWrite)
        offset += toWrite
        bytesRead.set(toWrite)
        return true
    }

    override fun cancel() {
        runCatching { inputStream.close() }
    }
}

private fun rejectedResourceHandler(): CefResourceHandler =
    ExplorerCefResourceHandler(ByteArrayInputStream(ByteArray(0)), 404, "Not Found", null)
