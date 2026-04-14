package org.supersigil.intellij

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class GraphExplorerThemeAdapterTest {
    @Test
    // supersigil: intellij-graph-explorer-theme-adapter
    fun `theme adapter css maps the explorer token set in light and dark blocks`() {
        val css = loadThemeAdapterCss()

        assertNotNull("Expected intellij-theme-adapter.css to be available on the classpath", css)

        val themeCss = css!!
        val lightBlock = cssBlock(themeCss, ":root")
        val darkBlock = cssBlock(themeCss, "html.dark")

        assertNotNull(lightBlock)
        assertNotNull(darkBlock)

        assertBlockContainsAll(
            lightBlock!!,
            listOf(
                "color-scheme: light;",
                "--bg-deep: #ffffff;",
                "--bg-surface: #f5f5f5;",
                "--bg-card: #ffffff;",
                "--bg-card-hover: #ececec;",
                "--text: #1e1f22;",
                "--text-muted: #6c707e;",
                "--text-dim: #8b8d98;",
                "--gold: #0a7aff;",
                "--gold-dim: rgba(10, 122, 255, 0.2);",
                "--teal: #1a8a9d;",
                "--green: #4caf50;",
                "--red: #d93025;",
                "--border: rgba(31, 35, 40, 0.12);",
                "--border-hover: rgba(31, 35, 40, 0.18);",
                "--font-body: system-ui, sans-serif;",
                "--font-heading: system-ui, sans-serif;",
                "--font-display: var(--font-heading);",
                """--font-mono: "JetBrains Mono", monospace;""",
            ),
        )
        assertBlockContainsAll(
            darkBlock!!,
            listOf(
                "color-scheme: dark;",
                "--bg-deep: #1e1f22;",
                "--bg-surface: #2b2d30;",
                "--bg-card: #31343a;",
                "--bg-card-hover: #3b3f46;",
                "--text: #dfe1e5;",
                "--text-muted: #a7adb8;",
                "--text-dim: #7e8594;",
                "--gold: #4da3ff;",
                "--gold-dim: rgba(77, 163, 255, 0.2);",
                "--teal: #5db7d5;",
                "--green: #6fbf73;",
                "--red: #f16c6c;",
                "--border: rgba(255, 255, 255, 0.12);",
                "--border-hover: rgba(255, 255, 255, 0.18);",
                "--font-body: system-ui, sans-serif;",
                "--font-heading: system-ui, sans-serif;",
                "--font-display: var(--font-heading);",
                """--font-mono: "JetBrains Mono", monospace;""",
            ),
        )
    }

    @Test
    fun `theme adapter css keeps the light theme as default`() {
        val css = loadThemeAdapterCss()

        assertNotNull(css)
        assertTrue(css!!.trimStart().startsWith(":root"))
    }

    @Test
    // supersigil: intellij-graph-explorer-theme-fonts
    fun `bundled explorer stylesheets do not load external fonts`() {
        val stylesheets = loadBundledStylesheets()

        assertEquals(
            listOf(
                "landing-tokens.css",
                "explorer-styles.css",
                "supersigil-preview.css",
                "intellij-theme-adapter.css",
            ),
            stylesheets.keys.toList(),
        )

        for ((name, css) in stylesheets) {
            assertFalse("$name should not import external stylesheets", css.contains("@import"))
            assertFalse("$name should not define custom font faces", css.contains("@font-face"))
            assertFalse("$name should not reference remote font URLs", css.contains("url(http://"))
            assertFalse("$name should not reference remote font URLs", css.contains("url(https://"))
            assertFalse("$name should not reference Google Fonts", css.contains("fonts.googleapis.com"))
            assertFalse("$name should not reference Google Fonts", css.contains("fonts.gstatic.com"))
        }
    }

    @Test
    // supersigil: intellij-graph-explorer-bundled-assets
    fun `bundled explorer asset set is present on the classpath`() {
        val requiredResources =
            setOf(
                "explorer.js",
                "render-iife.js",
                "supersigil-preview.js",
                "supersigil-preview.css",
                "landing-tokens.css",
                "explorer-styles.css",
                "intellij-theme-adapter.css",
                "explorer-bridge.js",
            )

        val bundledResources = loadBundledResourceNames()

        assertTrue(
            "Expected bundled assets to include $requiredResources but found ${bundledResources.toSet()}",
            bundledResources.containsAll(requiredResources),
        )
    }

    private fun loadThemeAdapterCss(): String? =
        loadCssResource("intellij-theme-adapter.css")

    private fun loadBundledStylesheets(): Map<String, String> =
        Regex("""href="${Regex.escape(EXPLORER_ORIGIN)}([^"]+\.css)"""")
            .findAll(graphExplorerHtml())
            .map { match -> match.groupValues[1] }
            .associateWith { name ->
                loadCssResource(name) ?: error("Expected $name to be available on the classpath")
            }

    private fun loadBundledResourceNames(): List<String> =
        Regex("""(?:href|src)="${Regex.escape(EXPLORER_ORIGIN)}([^"]+)"""")
            .findAll(graphExplorerHtml())
            .map { match -> match.groupValues[1] }
            .onEach { name ->
                assertNotNull(
                    "Expected $name to be available on the classpath",
                    javaClass.classLoader.getResource("supersigil-explorer/$name"),
                )
            }
            .toList()

    private fun loadCssResource(name: String): String? =
        javaClass.classLoader
            .getResourceAsStream("supersigil-explorer/$name")
            ?.bufferedReader()
            ?.use { it.readText() }

    private fun cssBlock(
        css: String,
        selector: String,
    ): String? =
        Regex("""${Regex.escape(selector)}\s*\{([^}]*)\}""", RegexOption.DOT_MATCHES_ALL)
            .find(css)
            ?.groupValues
            ?.get(1)

    private fun assertBlockContainsAll(
        block: String,
        declarations: List<String>,
    ) {
        for (declaration in declarations) {
            assertTrue("Expected declaration in CSS block: $declaration", block.contains(declaration))
        }
        assertFalse(block.contains("Crimson Pro"))
    }
}
