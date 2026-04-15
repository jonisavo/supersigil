---
supersigil:
  id: intellij-graph-explorer/req
  type: requirements
  status: implemented
title: "IntelliJ Graph Explorer"
---

## Introduction

An interactive graph explorer tool window for IntelliJ that visualizes
the full Supersigil document graph. Spec authors can see document
relationships, drill into spec details with verification status, and
navigate to source files — all without leaving the IDE.

The tool window hosts the same D3-based graph visualization used by
the VS Code extension and the website, rendered inside IntelliJ's
embedded Chromium browser (JCEF). Data comes from the LSP server, so
the graph stays live as specs change.

Scope: a JCEF-based tool window hosting the graph explorer, a
JBCefJSQuery bridge for bidirectional communication between the JVM
and browser, the Gradle build integration to bundle explorer web
assets into the plugin JAR, live refresh on spec changes, and editor
navigation from the graph.

Out of scope: changes to the shared graph explorer modules, changes
to the LSP server (the `supersigil/graphData` and
`supersigil/documentComponents` endpoints already exist), and
multi-project root switching (IntelliJ projects are single-root).

```supersigil-xml
<References refs="vscode-explorer-webview/req, graph-explorer/req, intellij-plugin/req" />
```

## Definitions

- **Explorer modules**: The vanilla JS modules from the website
  (`graph-explorer.js`, detail panel, etc.) that implement graph
  visualization, filtering, search, impact trace, and URL routing.
  Bundled as an IIFE for use in the JCEF browser.
- **JCEF**: JetBrains CEF (Chromium Embedded Framework), the
  embedded browser available in IntelliJ for rendering web content
  inside tool windows and panels.
- **JBCefJSQuery**: IntelliJ's API for bidirectional communication
  between JVM code and JavaScript running in a JCEF browser. Used
  by the existing Markdown preview extension.
- **Graph data**: The `{ documents, edges }` JSON shape produced by
  the `supersigil/graphData` LSP endpoint.
- **Render data**: Per-document component trees with verification
  status, produced by the `supersigil/documentComponents` endpoint.

## Requirement 1: Tool Window

As a spec author using IntelliJ, I want to open an interactive graph
explorer in a tool window, so that I can visualize spec relationships
and drill into document details inside the IDE.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE plugin SHALL register a "Graph Explorer" tool window via
    `ToolWindowFactory` in `plugin.xml`. THE tool window SHALL only
    be available when the project contains a `supersigil.toml` file.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-availability" />
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-plugin-xml" />
  </Criterion>
  <Criterion id="req-1-2">
    THE tool window SHALL host a JCEF browser panel that loads the
    bundled explorer modules and renders the graph visualization.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-html-shell" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
  <Criterion id="req-1-3">
    THE tool window toolbar SHALL include a refresh button that
    re-fetches graph data and re-renders the explorer.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Data Flow

As a spec author, I want the graph explorer to show live data from
the LSP server, so that the visualization reflects the current
project state.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    WHEN the tool window opens, THE plugin SHALL fetch graph data
    from the LSP server via `workspace/executeCommand` with command
    `supersigil.graphData` and fetch render data by calling
    `supersigil.documentComponents` for each document.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-data-fetch" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
  <Criterion id="req-2-2">
    THE plugin SHALL use bounded concurrency (limit of 10) when
    fetching document components, so that large projects do not
    overwhelm the LSP server.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-bounded-fetch" />
  </Criterion>
  <Criterion id="req-2-3">
    THE plugin SHALL post graph data and render data to the JCEF
    browser via `executeJavaScript`, calling the explorer's
    `SupersigilExplorer.mount()` function with the assembled payload.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-data-push" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Live Updates

As a spec author, I want the graph to update when specs change, so
that the explorer always reflects the current project state.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN the plugin receives a `supersigil/documentsChanged`
    notification from the LSP server, THE graph explorer SHALL
    re-fetch data and re-render if the tool window is visible. IF
    the tool window is hidden, THE plugin SHALL mark it as stale and
    refresh when it becomes visible.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-live-updates" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
  <Criterion id="req-3-2">
    THE browser SHALL preserve the current view state (selected
    document, filter state) across re-renders by capturing and
    restoring the URL hash before and after remounting.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-state-preservation" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/supersigil-explorer/explorer-bridge.js" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Editor Navigation

As a spec author, I want to open spec files and evidence source files
directly from the graph explorer, so that I can navigate from
visualization to code without manual file searching.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    THE detail panel's "Open File" button SHALL send a navigation
    action to the JVM via JBCefJSQuery. THE plugin SHALL open the
    corresponding spec file in the editor.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-open-file-navigation" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/supersigil-explorer/explorer-bridge.js, editors/intellij/src/main/kotlin/org/supersigil/intellij/NavigationUtil.kt" />
  </Criterion>
  <Criterion id="req-4-2">
    Evidence source links (test file + line number) in the detail
    panel SHALL be clickable. Clicking one SHALL send a navigation
    action to the JVM that opens the file at the specified line.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-evidence-navigation" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/supersigil-explorer/explorer-bridge.js, editors/intellij/src/main/kotlin/org/supersigil/intellij/NavigationUtil.kt" />
  </Criterion>
  <Criterion id="req-4-3">
    Document links in edges and criterion references SHALL navigate
    within the explorer (selecting the target node in the graph),
    not open a new editor tab.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/supersigil-explorer/explorer-bridge.js" />
  </Criterion>
  <Criterion id="req-4-4">
    THE plugin SHALL resolve workspace-relative file paths from the
    graph data against the project base path to produce absolute
    file system paths for editor navigation.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-navigation-paths" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: JCEF Bridge

As a maintainer, I want a clear communication contract between the
JVM and the JCEF browser, so that the integration is predictable
and testable.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE plugin SHALL inject global JavaScript functions into the JCEF
    browser via JBCefJSQuery after each page load: a data query
    function for fetching component data and a navigation action
    function for editor navigation.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-jsquery-bridge" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
  <Criterion id="req-5-2">
    THE bridge script SHALL detect "Open File" button clicks and
    evidence link clicks in the explorer DOM and route them through
    the action query to the JVM.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-bridge-routing" />
  </Criterion>
  <Criterion id="req-5-3">
    THE bridge script SHALL provide a link resolver to the explorer's
    `mount()` function that generates interceptable evidence links
    and hash-based URIs for in-explorer document navigation.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/supersigil-explorer/explorer-bridge.js" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Theme Integration

As an IntelliJ user, I want the explorer to match my IDE theme, so
that the tool window feels native.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE JCEF browser SHALL include a CSS adapter that maps the
    explorer's design tokens to IntelliJ's JCEF theme properties,
    matching light and dark themes.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-theme-adapter" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/supersigil-explorer/intellij-theme-adapter.css" />
  </Criterion>
  <Criterion id="req-6-2">
    THE JCEF browser SHALL NOT load external fonts. Typography SHALL
    use the IDE's default font families.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-theme-fonts" />
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/supersigil-explorer/intellij-theme-adapter.css" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 7: Asset Bundling

As a maintainer, I want the explorer web assets bundled into the
plugin JAR at build time, so that the tool window works without
runtime file resolution or network access.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-7-1">
    THE Gradle build SHALL include tasks that build the explorer
    web assets from the shared `packages/` and `website/` sources
    and copy them into `src/main/resources/` before JAR packaging,
    following the same pattern used by the existing preview asset
    pipeline.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/build.gradle.kts, website/package.json, website/build-explorer-iife.mjs" />
  </Criterion>
  <Criterion id="req-7-2">
    THE bundled assets SHALL include the explorer IIFE bundle, the
    presentation kit (render-iife.js, supersigil-preview.js,
    supersigil-preview.css), design tokens CSS, explorer styles CSS,
    a theme adapter CSS, and a bridge script.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-bundled-assets" />
  </Criterion>
  <Criterion id="req-7-3">
    A resource provider SHALL serve the bundled assets to the JCEF
    browser from the plugin's classpath.
    <VerifiedBy strategy="tag" tag="intellij-graph-explorer-resource-handler" />
  </Criterion>
</AcceptanceCriteria>
```
