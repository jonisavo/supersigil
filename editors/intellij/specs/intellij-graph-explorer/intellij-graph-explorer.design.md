---
supersigil:
  id: intellij-graph-explorer/design
  type: design
  status: approved
title: "IntelliJ Graph Explorer"
---

```supersigil-xml
<Implements refs="intellij-graph-explorer/req" />
<DependsOn refs="intellij-plugin/design, vscode-explorer-webview/design" />
<TrackedFiles paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorer*.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/ExplorerResourceHandler.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/NavigationUtil.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilProjectUtil.kt, editors/intellij/src/main/resources/supersigil-explorer/**/*, editors/intellij/src/main/resources/icons/supersigil-graph*.svg, editors/intellij/src/main/resources/META-INF/plugin.xml, editors/intellij/build.gradle.kts, website/build-explorer-iife.mjs, website/package.json, website/src/components/explore/styles.css" />
```

## Overview

A JCEF-based tool window that hosts the same D3-based graph explorer
used by the VS Code extension and the website. The architecture
follows the same pattern as the existing Markdown preview extension:
JCEF browser + JBCefJSQuery bridge for JVM-to-JS communication.

```
IntelliJ project
  │
  ├─ GraphExplorerToolWindowFactory
  │    ├─ isApplicableAsync()  →  checks for supersigil.toml
  │    └─ creates JCEF browser panel
  │
  ├─ GraphExplorerPanel (JCEF)
  │    ├─ loads HTML shell from classpath resources
  │    ├─ injects bridge functions via JBCefJSQuery
  │    └─ renders graph explorer modules
  │         │
  │    JBCefJSQuery (browser → JVM)
  │         ├─ dataQuery: component data requests
  │         └─ actionQuery: navigation actions
  │         │
  │    executeJavaScript (JVM → browser)
  │         └─ pushes graph data + render data
  │
  ├─ LSP server (existing)
  │    ├─ supersigil.graphData    →  full document graph
  │    └─ supersigil.documentComponents  →  per-doc render data
  │
  └─ Gradle build tasks
       ├─ buildExplorerKit   →  pnpm build (website explorer IIFE)
       └─ copyExplorerAssets →  into src/main/resources/supersigil-explorer/
```

## Tool Window Registration

A `ToolWindowFactory` registered in `plugin.xml` alongside the
existing Spec Explorer:

```xml
<toolWindow id="Graph Explorer"
            factoryClass="org.supersigil.intellij.GraphExplorerToolWindowFactory"
            anchor="right"
            icon="/icons/supersigil-graph.svg" />
```

`isApplicableAsync` and `shouldBeAvailable` check for
`supersigil.toml`, same as `SpecExplorerToolWindowFactory`.

The tool window toolbar includes:
- Refresh button (re-fetches data and re-renders)
- Verify button (reuses existing `VerifyAction`)

## JCEF Panel

`createToolWindowContent` creates a `JBCefBrowser` and adds it to the
tool window. The browser loads an HTML shell from classpath resources
that references the bundled explorer JS and CSS.

```kotlin
class GraphExplorerToolWindowFactory : ToolWindowFactory, DumbAware {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val browser = JBCefBrowser()
        // ... set up bridge, load HTML
        val panel = SimpleToolWindowPanel(true)
        panel.setContent(browser.component)
        toolWindow.contentManager.addContent(
            toolWindow.contentManager.factory.createContent(panel, null, false)
        )
    }
}
```

The HTML is generated as a string and loaded via
`loadHTML(html, "https://supersigil-explorer/")`. The second parameter
sets the page origin, which is important: without it, `loadHTML` uses
`about:blank` as the origin and subsequent resource fetches to the
custom scheme would fail with same-origin violations.

Resource files (JS, CSS) are served via a `CefRequestHandler`
registered on the `JBCefClient`. The handler intercepts requests
matching the `https://supersigil-explorer/` prefix and returns
classpath resources via a `CefResourceHandler`. This is different
from the Markdown preview's `ResourceProvider` approach, which is
specific to the Markdown plugin's framework and not available for
standalone JCEF browsers.

```kotlin
browser.jbCefClient.addRequestHandler(
    ExplorerResourceRequestHandler(),
    browser.cefBrowser,
)
browser.loadHTML(html, "https://supersigil-explorer/")
```

### HTML Shell

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <link rel="stylesheet" href="https://supersigil-explorer/landing-tokens.css">
  <link rel="stylesheet" href="https://supersigil-explorer/explorer-styles.css">
  <link rel="stylesheet" href="https://supersigil-explorer/supersigil-preview.css">
  <link rel="stylesheet" href="https://supersigil-explorer/intellij-theme-adapter.css">
</head>
<body>
  <div id="explorer" style="height: 100vh;"></div>
  <script src="https://supersigil-explorer/render-iife.js"></script>
  <script src="https://supersigil-explorer/supersigil-preview.js"></script>
  <script src="https://supersigil-explorer/explorer.js"></script>
  <script src="https://supersigil-explorer/explorer-bridge.js"></script>
</body>
</html>
```

The `ExplorerResourceRequestHandler` maps URL paths to classpath
resources under `supersigil-explorer/`. It returns appropriate MIME
types (`.js` → `application/javascript`, `.css` → `text/css`) and
rejects requests outside the known resource set.

## JBCefJSQuery Bridge

Two `JBCefJSQuery` instances, following the preview extension pattern:

### Data Query (browser → JVM → browser)

Used by the bridge script to fetch document component data for the
detail panel. The bridge calls `__supersigilQuery(request, onSuccess,
onFailure)` where `request` is a JSON string.

Request types:
- `{"type":"documentComponents","uri":"<file-uri>"}` — returns the
  component tree for one document.

The handler iterates running Supersigil LSP servers, sends
`workspace/executeCommand("supersigil.documentComponents")`, and
returns the JSON response.

### Action Query (browser → JVM, fire-and-forget)

Used for navigation actions. The bridge calls
`__supersigilAction(action)` where `action` is a colon-delimited
string.

Action types:
- `open-file:<path>:<line>` — opens a file at the given line.
- `open-criterion:<docId>:<criterionId>` — resolves a criterion's
  location and opens the file.

Both action handlers are shared with the preview extension. The
existing `openFile` and `openCriterion` private methods in
`SupersigilPreviewExtensionProvider.kt` are extracted to a shared
`NavigationUtil.kt` so both the preview and graph explorer can
use them.

### Bridge Injection

After each page load (`CefLoadHandlerAdapter.onLoadEnd`), the panel
injects the bridge functions via `executeJavaScript`:

```javascript
window.__supersigilQuery = function(request, onSuccess, onFailure) {
    // JBCefJSQuery injection code
};
window.__supersigilAction = function(action) {
    // JBCefJSQuery injection code
};
```

## Bridge Script

A new `explorer-bridge.js` resource that runs in the JCEF browser.
Analogous to the VSCode `explorerBootstrap.ts` but adapted for the
JBCefJSQuery communication model instead of `postMessage`.

### Initialization

On `DOMContentLoaded`, the bridge:
1. Checks for `__supersigilQuery` availability (injected by JVM).
2. Waits for graph data to be pushed by the JVM (see Data Flow).
3. Calls `SupersigilExplorer.mount()` with the data and a link
   resolver.

### Link Resolver

```javascript
const linkResolver = {
    evidenceLink: (file, line) =>
        `#supersigil-action:open-file:${escapeColons(file)}:${line}`,
    documentLink: (docId) =>
        `#/doc/${encodeURIComponent(docId)}`,
    criterionLink: (docId, _criterionId) =>
        `#/doc/${encodeURIComponent(docId)}`,
};
```

Evidence links use the `#supersigil-action:` prefix (same pattern as
the preview bridge). A click handler intercepts these and routes them
through `__supersigilAction`.

Document and criterion links use hash-based routing and are handled
by the explorer's built-in URL router.

### "Open File" Button Injection

A `MutationObserver` watches for `.detail-panel-header` elements and
injects an "Open File" button, same pattern as the VSCode bootstrap.
The button click calls `__supersigilAction("open-file:<path>:1")`.

### State Preservation on Re-render

When new data arrives, the bridge:
1. Captures `window.location.hash`
2. Calls `unmount()` on the previous explorer instance
3. Clears the container
4. Calls `mount()` with new data
5. Restores the hash

## Data Flow

### Initial Load

When the tool window opens and the LSP server is running:

1. JVM fetches graph data via `workspace/executeCommand("supersigil.graphData")`
2. JVM fetches document components for each document via
   `workspace/executeCommand("supersigil.documentComponents")` with
   bounded concurrency. The refresh path submits at most 10 pooled
   worker tasks, and each worker pulls the next document from an
   atomic index until the list is exhausted. Individual failures are
   caught and tolerated — the render data array omits documents whose
   fetch failed.
3. JVM serializes the assembled payload to JSON
4. JVM calls `browser.executeJavaScript("window.__supersigilReceiveData(${json})")`
5. Bridge script calls `SupersigilExplorer.mount()`

If the LSP server is not running when the tool window opens, the
panel shows a loading state and retries with the same polling pattern
used by `SpecExplorerToolWindowFactory` (retry alarm until a listener
is attached).

### Live Updates

The tool window subscribes to `documentsChanged` via
`SupersigilLspServerDescriptor.addDocumentsChangedListener()`, same
as the Spec Explorer tree.

On notification:
- If the tool window is visible: re-fetch and push data immediately,
  debounced at 200ms.
- If hidden: set a `staleWhileHidden` flag. When the tool window
  becomes visible again, re-fetch and push data.

Visibility tracking uses `ToolWindow.isVisible` checked on
`documentsChanged` and a content manager listener for visibility
changes.

## Theme Adapter

A new `intellij-theme-adapter.css` resource that maps the explorer's
design tokens to JCEF-accessible values. Unlike VSCode (which
exposes `--vscode-*` variables), IntelliJ's JCEF browser does not
automatically inject theme variables. Additionally, JCEF's
`prefers-color-scheme` media query does not reliably track the IDE's
light/dark theme setting.

Instead, the JVM injects a `dark` or `light` CSS class on the
`<html>` element via `executeJavaScript` after each page load, based
on `JBColor.isBright()`. The theme adapter CSS selects colors based
on this class:

```css
:root {
    --bg: #ffffff;
    --bg-surface: #f7f8fa;
    --text: #1e1e1e;
    --text-muted: #6e6e6e;
    --border: #d1d5db;
    --font-body: -apple-system, BlinkMacSystemFont, sans-serif;
    --font-mono: "JetBrains Mono", monospace;
    --font-heading: -apple-system, BlinkMacSystemFont, sans-serif;
}

html.dark {
    --bg: #2b2d30;
    --bg-surface: #1e1f22;
    --text: #bcbec4;
    --text-muted: #6f737a;
    --border: #43454a;
}
```

The JVM injects the class in the same `CefLoadHandlerAdapter.onLoadEnd`
callback that injects the bridge functions:

```kotlin
val themeClass = if (JBColor.isBright()) "light" else "dark"
browser.executeJavaScript(
    "document.documentElement.className = '$themeClass';",
    "supersigil-theme-init", 0,
)
```

The colors are sampled from IntelliJ's default light and dark
(Darcula/New UI) themes. Custom themes will have minor color
mismatches, which is acceptable for a v1 implementation.

## Build Integration

### New Website Build Script

The VSCode extension builds the explorer IIFE via its own
`esbuild.mjs`, outputting to `editors/vscode/dist/webview/explorer.js`.
That path is specific to the VSCode build and not suitable for
cross-editor sharing. The website also has a `bundle:standalone`
script, but that produces a different artifact for the CLI.

A new `build:explorer-iife` script is added to `website/package.json`
that produces the explorer IIFE bundle at a shared location:

```json
{
  "scripts": {
    "build:explorer-iife": "node build-explorer-iife.mjs"
  }
}
```

```javascript
// website/build-explorer-iife.mjs
import * as esbuild from 'esbuild';

await esbuild.build({
    entryPoints: ['src/components/explore/graph-explorer.js'],
    bundle: true,
    format: 'iife',
    globalName: 'SupersigilExplorer',
    platform: 'browser',
    target: 'es2020',
    mainFields: ['module', 'main'],
    minify: true,
    outfile: 'dist/explorer-iife/explorer.js',
});
```

The VSCode extension's `esbuild.mjs` can optionally be updated to
use this shared script instead of its inline build, but that is not
required for the IntelliJ plugin.

### New Gradle Tasks

Following the existing `buildPreviewKit` / `copyPreviewAssets`
pattern:

```kotlin
val buildExplorerKit by registering(Exec::class) {
    workingDir = file("../../website")
    commandLine("pnpm", "run", "build:explorer-iife")
    inputs.dir(layout.projectDirectory.dir("../../website/src/components/explore"))
    inputs.file(layout.projectDirectory.file("../../website/build-explorer-iife.mjs"))
    inputs.file(layout.projectDirectory.file("../../website/package.json"))
    inputs.file(layout.projectDirectory.file("../../website/tsconfig.json"))
    inputs.file(layout.projectDirectory.file("../../pnpm-workspace.yaml"))
    inputs.file(layout.projectDirectory.file("../../pnpm-lock.yaml"))
    outputs.file(layout.projectDirectory.file("../../website/dist/explorer-iife/explorer.js"))
}

val copyExplorerAssets by registering(Copy::class) {
    dependsOn(buildExplorerKit, buildPreviewKit)
    from("../../website/dist/explorer-iife") {
        include("explorer.js")
    }
    from("../../website/src/styles") {
        include("landing-tokens.css")
    }
    from("../../website/src/components/explore") {
        include("styles.css")
        rename("styles.css", "explorer-styles.css")
    }
    from("../../packages/preview/dist") {
        include("render-iife.js")
        include("supersigil-preview.js")
        include("supersigil-preview.css")
    }
    into("src/main/resources/supersigil-explorer")
}
```

The `processResources` task depends on both `copyPreviewAssets` and
`copyExplorerAssets`.

The preview asset build follows the same pattern: declared
inputs/outputs instead of marker-gated skipping, so Gradle rebuilds
the bundled assets when preview sources, scripts, or workspace lock
files change.

The `explorer-bridge.js` and `intellij-theme-adapter.css` are
hand-authored source files in `src/main/resources/supersigil-explorer/`
(not copied from elsewhere).

### Resource Layout

```
src/main/resources/
  supersigil-preview/          (existing — markdown preview)
  supersigil-explorer/         (new — graph explorer)
    explorer.js                (IIFE bundle from website, copied)
    render-iife.js             (from packages/preview, copied)
    supersigil-preview.js      (from packages/preview, copied)
    supersigil-preview.css     (from packages/preview, copied)
    landing-tokens.css         (from website, copied)
    explorer-styles.css        (from website, copied)
    intellij-theme-adapter.css (hand-authored)
    explorer-bridge.js         (hand-authored)
    .gitignore                 (ignores copied assets)
```

## Shared Navigation Utility

The preview extension's `openFile` and `openCriterion` methods are
extracted to `NavigationUtil.kt`:

```kotlin
object NavigationUtil {
    fun openFile(path: String, line: Int) { ... }
    fun openCriterion(docId: String, criterionId: String) { ... }
}
```

Both the preview extension and the graph explorer bridge call
these instead of duplicating the navigation logic. The `splitAction`
function and `findCriterionLine` helper also move here, as they
are used by both bridge implementations.

## Disposal

The JCEF browser and its associated resources are tied to the tool
window's lifecycle via `Disposer.register(toolWindow.disposable, ...)`.
This ensures cleanup when the tool window is closed or the project
is disposed.

Resources requiring disposal:
- **JBCefJSQuery instances** (dataQuery, actionQuery): registered
  with `toolWindow.disposable` so they auto-dispose. This follows
  the existing preview extension pattern where `dispose()` calls
  `dataQuery.dispose()` and `actionQuery.dispose()`.
- **JBCefBrowser**: registered with `toolWindow.disposable`.
  Disposing the browser also cleans up its underlying CefBrowser and
  any registered CefLoadHandlers.
- **CefRequestHandler**: tied to the browser lifetime — no separate
  disposal needed.
- **documentsChanged listener**: already uses disposable-based
  cleanup via `addDocumentsChangedListener(listener, parentDisposable)`
  where `parentDisposable` is `toolWindow.disposable`.
- **Alarm instances** (retry, refresh debounce): created with
  `Alarm(ThreadToUse.POOLED_THREAD, toolWindow.disposable)`, same
  as the Spec Explorer.

## Error Handling

**LSP not running**: The JCEF browser shows a loading/empty state.
The factory uses the same retry-with-alarm pattern as the Spec
Explorer, polling until the server starts and a `documentsChanged`
listener is attached.

**Graph data fetch failure**: Logged; the browser retains its
previous content (or shows loading state on first attempt).

**Component fetch failure**: Individual failures are tolerated.
The render data array omits documents whose component fetch failed.

**JCEF not available**: Some IntelliJ installations or remote
development scenarios may not have JCEF. The tool window factory
checks `JBCefApp.isSupported()` in `isApplicableAsync` and hides
the tool window if JCEF is unavailable.

## Testing Strategy

**Bridge script**: The shared `splitAction` and `findCriterionLine`
helpers are tested in `NavigationUtilTest.kt`, and the JCEF bridge
behavior is covered in `explorer-bridge.test.js`.

**Graph data fetch logic**: The concurrent fetch + assembly logic
is extracted to a testable function that takes a list of document
URIs and a command executor, returning the assembled render data.
Tested with mock responses.

**Tool window registration**: Verified via `./gradlew runPluginVerifier`
for binary compatibility with target platforms.

**Visual correctness**: Manual testing — open the tool window, verify
graph renders, click nodes, verify navigation.

## Decisions

```supersigil-xml
<Decision id="jcef-tool-window">
  Use a JCEF-based tool window to host the graph explorer, reusing
  the same web assets as the VS Code extension.

  <References refs="intellij-graph-explorer/req#req-1-2" />

  <Rationale>
  The graph explorer is a D3-based web application. Reimplementing
  it as a native Swing UI would require a graph layout library, a
  force simulation, and extensive rendering code — duplicating what
  already exists. JCEF is available in all standard IntelliJ
  distributions (verified via JBCefApp.isSupported) and is already
  proven in this plugin by the Markdown preview extension.
  </Rationale>

  <Alternative id="native-swing" status="rejected">
    Implement the graph with a Swing-based graph library (e.g.
    JGraphX). Native look but massive implementation effort,
    visual divergence from the VS Code/website explorer, and an
    ongoing maintenance burden for a parallel rendering pipeline.
  </Alternative>
</Decision>

<Decision id="shared-navigation-util">
  Extract navigation helpers from the preview extension into a
  shared NavigationUtil so both the preview and graph explorer use
  the same code.

  <References refs="intellij-graph-explorer/req#req-4-1, intellij-graph-explorer/req#req-4-2" />

  <Rationale>
  The preview extension already implements openFile and
  openCriterion with correct handling of project base paths, pooled
  thread execution, and criterion line resolution. Duplicating this
  in the graph explorer would create two diverging copies. Extracting
  to a shared utility keeps both features consistent and testable.
  </Rationale>
</Decision>

<Decision id="custom-scheme-resource-handler">
  Serve web assets via a custom CefRequestHandler intercepting a
  synthetic scheme (https://supersigil-explorer/) rather than using
  file:// URIs or data URIs.

  <References refs="intellij-graph-explorer/req#req-7-3" />

  <Rationale>
  JCEF restricts file:// access and does not support loading
  classpath resources directly. The Markdown preview plugin uses
  ResourceProvider, but that API is specific to the Markdown
  plugin's framework and not available for standalone JCEF browsers.
  A CefRequestHandler registered on JBCefClient intercepting a
  known URL prefix is the correct pattern for standalone panels.
  Combined with loadHTML(html, baseUrl) to set the page origin,
  this avoids same-origin violations and works reliably across
  platforms.
  </Rationale>

  <Alternative id="data-uri-inlining" status="rejected">
    Inline all JS and CSS as data URIs in the HTML. Works but
    produces enormous HTML strings, breaks source mapping, and
    makes debugging difficult.
  </Alternative>

  <Alternative id="temp-file-extraction" status="rejected">
    Extract resources to a temp directory and load via file://.
    Platform-dependent, cleanup complexity, and security restrictions
    on file:// in JCEF.
  </Alternative>
</Decision>

<Decision id="jvm-injected-theme-class">
  Detect the IDE theme on the JVM side via JBColor.isBright() and
  inject a CSS class (dark/light) on the html element, rather than
  relying on prefers-color-scheme or runtime CSS variable injection.

  <References refs="intellij-graph-explorer/req#req-6-1" />

  <Rationale>
  JCEF's prefers-color-scheme media query does not reliably track
  the IDE's light/dark theme setting — it may default to the OS
  preference rather than the IDE theme. Injecting a CSS class from
  the JVM side is simple (one executeJavaScript call in the existing
  onLoadEnd handler), reliable (JBColor.isBright() is the
  authoritative theme check), and sufficient for selecting between
  two static color palettes. Custom themes will have minor color
  mismatches, which is acceptable for a v1 implementation.
  </Rationale>

  <Alternative id="prefers-color-scheme" status="rejected">
    Use CSS prefers-color-scheme media query with static palettes.
    Simpler CSS but unreliable — JCEF may not reflect the IDE
    theme, causing dark-theme users to see light colors.
  </Alternative>

  <Alternative id="runtime-css-variable-injection" status="deferred">
    Read JBColor values on the JVM side, serialize all design
    tokens, and inject as CSS variables via executeJavaScript on
    each theme change. More accurate for custom themes but complex.
    Can be added later if static palettes prove insufficient.
  </Alternative>
</Decision>
```
