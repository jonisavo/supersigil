package org.supersigil.intellij

import org.cef.misc.BoolRef
import org.cef.network.CefPostData
import org.cef.network.CefRequest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.InputStream

// supersigil: intellij-graph-explorer-resource-handler
class ExplorerResourceHandlerTest {

    // -------------------------------------------------------------------------
    // resolveExplorerResource
    // -------------------------------------------------------------------------

    @Test
    fun `resolves known JS resource`() {
        assertEquals("explorer.js", resolveExplorerResource("https://supersigil-explorer/explorer.js"))
    }

    @Test
    fun `resolves known CSS resource`() {
        assertEquals("landing-tokens.css", resolveExplorerResource("https://supersigil-explorer/landing-tokens.css"))
    }

    @Test
    fun `resolves render-iife js`() {
        assertEquals("render-iife.js", resolveExplorerResource("https://supersigil-explorer/render-iife.js"))
    }

    @Test
    fun `resolves explorer-bridge js`() {
        assertEquals("explorer-bridge.js", resolveExplorerResource("https://supersigil-explorer/explorer-bridge.js"))
    }

    @Test
    fun `resolves intellij-theme-adapter css`() {
        assertEquals(
            "intellij-theme-adapter.css",
            resolveExplorerResource("https://supersigil-explorer/intellij-theme-adapter.css"),
        )
    }

    @Test
    fun `returns null for unknown resource`() {
        assertNull(resolveExplorerResource("https://supersigil-explorer/malicious.js"))
    }

    @Test
    fun `returns null for wrong origin`() {
        assertNull(resolveExplorerResource("https://evil.com/explorer.js"))
    }

    @Test
    fun `returns null for origin-only URL`() {
        assertNull(resolveExplorerResource("https://supersigil-explorer/"))
    }

    @Test
    fun `returns null for directory traversal`() {
        assertNull(resolveExplorerResource("https://supersigil-explorer/../etc/passwd"))
    }

    @Test
    fun `returns null for nested path`() {
        assertNull(resolveExplorerResource("https://supersigil-explorer/sub/explorer.js"))
    }

    @Test
    fun `returns null for empty string`() {
        assertNull(resolveExplorerResource(""))
    }

    @Test
    fun `returns null for partial origin match`() {
        assertNull(resolveExplorerResource("https://supersigil-explorer-evil/explorer.js"))
    }

    @Test
    fun `request handler disables default handling for explorer origin`() {
        val handler = ExplorerResourceRequestHandler()
        val disableDefaultHandling = BoolRef(false)

        val resourceRequestHandler =
            handler.getResourceRequestHandler(
                null,
                null,
                FakeCefRequest("https://supersigil-explorer/explorer.js"),
                false,
                false,
                null,
                disableDefaultHandling,
            )

        assertNotNull(resourceRequestHandler)
        assertTrue(disableDefaultHandling.get())
    }

    @Test
    fun `request handler ignores non explorer origin`() {
        val handler = ExplorerResourceRequestHandler()
        val disableDefaultHandling = BoolRef(false)

        val resourceRequestHandler =
            handler.getResourceRequestHandler(
                null,
                null,
                FakeCefRequest("https://example.com/explorer.js"),
                false,
                false,
                null,
                disableDefaultHandling,
            )

        assertNull(resourceRequestHandler)
        assertFalse(disableDefaultHandling.get())
    }

    @Test
    fun `resource handler rejects unknown explorer resource without falling through`() {
        val handler = ExplorerResourceRequestHandler()
        val resourceRequestHandler =
            handler.getResourceRequestHandler(
                null,
                null,
                FakeCefRequest("https://supersigil-explorer/malicious.js"),
                false,
                false,
                null,
                BoolRef(false),
            )

        assertNotNull(
            "Unknown explorer-origin resources should return a handler so CEF does not fall through",
            resourceRequestHandler?.getResourceHandler(
                null,
                null,
                FakeCefRequest("https://supersigil-explorer/malicious.js"),
            ),
        )
    }

    @Test
    fun `cancel closes unopened classpath resource stream`() {
        val stream = TrackingInputStream("content".toByteArray())
        val classLoader =
            object : ClassLoader(null) {
                override fun getResourceAsStream(name: String): InputStream? =
                    if (name == "supersigil-explorer/explorer.js") stream else null
            }
        val handler = ExplorerResourceRequestHandler { classLoader }
        val resourceRequestHandler =
            handler.getResourceRequestHandler(
                null,
                null,
                FakeCefRequest("https://supersigil-explorer/explorer.js"),
                false,
                false,
                null,
                BoolRef(false),
            )
        val resourceHandler =
            resourceRequestHandler?.getResourceHandler(
                null,
                null,
                FakeCefRequest("https://supersigil-explorer/explorer.js"),
            )

        resourceHandler?.cancel()

        assertTrue(stream.closed)
    }

    // -------------------------------------------------------------------------
    // mimeTypeForResource
    // -------------------------------------------------------------------------

    @Test
    fun `js files get javascript mime type`() {
        assertEquals("application/javascript", mimeTypeForResource("explorer.js"))
    }

    @Test
    fun `css files get css mime type`() {
        assertEquals("text/css", mimeTypeForResource("landing-tokens.css"))
    }

    @Test
    fun `html files are unsupported`() {
        assertNull(mimeTypeForResource("index.html"))
    }

    @Test
    fun `unknown extension is unsupported`() {
        assertNull(mimeTypeForResource("data.bin"))
    }

    @Test
    fun `no extension is unsupported`() {
        assertNull(mimeTypeForResource("LICENSE"))
    }

    @Test
    fun `compound names resolve by final extension`() {
        assertEquals("application/javascript", mimeTypeForResource("render-iife.js"))
    }

    // -------------------------------------------------------------------------
    // All known resources are covered
    // -------------------------------------------------------------------------

    @Test
    fun `all known resources resolve correctly`() {
        val expected = listOf(
            "explorer.js",
            "render-iife.js",
            "supersigil-preview.js",
            "supersigil-preview.css",
            "landing-tokens.css",
            "explorer-styles.css",
            "intellij-theme-adapter.css",
            "explorer-bridge.js",
        )
        for (name in expected) {
            assertEquals(
                "Expected $name to resolve",
                name,
                resolveExplorerResource("$EXPLORER_ORIGIN$name"),
            )
        }
    }

    private class FakeCefRequest(
        private var requestUrl: String,
    ) : CefRequest() {
        override fun dispose() = Unit

        override fun getIdentifier(): Long = 0

        override fun isReadOnly(): Boolean = false

        override fun getURL(): String = requestUrl

        override fun setURL(url: String) {
            requestUrl = url
        }

        override fun getMethod(): String = "GET"

        override fun setMethod(method: String) = Unit

        override fun setReferrer(
            referrerUrl: String,
            policy: ReferrerPolicy,
        ) = Unit

        override fun getReferrerURL(): String = ""

        override fun getReferrerPolicy(): ReferrerPolicy = ReferrerPolicy.REFERRER_POLICY_DEFAULT

        override fun getPostData(): CefPostData? = null

        override fun setPostData(postData: CefPostData?) = Unit

        override fun getHeaderByName(name: String): String = ""

        override fun setHeaderByName(
            name: String,
            value: String,
            overwrite: Boolean,
        ) = Unit

        override fun getHeaderMap(headerMap: MutableMap<String, String>) = Unit

        override fun setHeaderMap(headerMap: MutableMap<String, String>) = Unit

        override fun set(
            url: String,
            method: String,
            postData: CefPostData?,
            headerMap: MutableMap<String, String>,
        ) {
            requestUrl = url
        }

        override fun getFlags(): Int = 0

        override fun setFlags(flags: Int) = Unit

        override fun getFirstPartyForCookies(): String = ""

        override fun setFirstPartyForCookies(url: String) = Unit

        override fun getResourceType(): ResourceType = ResourceType.RT_SUB_RESOURCE

        override fun getTransitionType(): TransitionType = TransitionType.TT_LINK
    }

    private class TrackingInputStream(
        data: ByteArray,
    ) : ByteArrayInputStream(data) {
        var closed: Boolean = false
            private set

        override fun close() {
            closed = true
            super.close()
        }
    }
}
