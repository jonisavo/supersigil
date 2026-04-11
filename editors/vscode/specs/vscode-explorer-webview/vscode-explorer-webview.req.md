---
supersigil:
  id: vscode-explorer-webview/req
  type: requirements
  status: implemented
title: "VS Code Explorer Webview"
---

## Introduction

Bring the interactive graph explorer into the VS Code extension as a
webview panel. Spec authors can visualize the full document graph,
drill into spec details, and jump to source files — all without
leaving the editor.

The webview reuses the existing website explorer modules
(`graph-explorer.js`, `detail-panel.js`, etc.) by bundling them into
the extension. Data comes from the LSP (not static JSON), so the
graph stays live as specs change.

Scope: a new LSP endpoint for graph data, a VS Code webview panel
hosting the explorer, a message protocol for editor integration
("open file" actions), the build pipeline to bundle the explorer
modules into the extension, relocation of the Spec Explorer tree
view to the built-in Explorer sidebar, multi-instance graph panels
with per-root scoping and auto-focus, and a root selector dropdown
for switching roots within a panel.

Minor adaptations to the explorer modules for host integration
(e.g. accepting an external link resolver, adding an "Open File"
button to the detail panel) are in scope. Larger feature additions
to the explorer are not.

Out of scope: IntelliJ webview (follow-up work), new explorer
features beyond what the website already provides.

```supersigil-xml
<References refs="graph-explorer/req, spec-rendering/req, spec-explorer/req" />
```

## Definitions

- **Explorer modules**: The vanilla JS modules in
  `website/src/components/explore/` that implement the graph
  visualization, detail panel, filtering, search, impact trace,
  and URL routing.
- **Graph data**: The `GraphJson` shape (`{ documents, edges }`)
  produced by `build_graph_json` in the CLI crate. Contains document
  nodes with ID, type, status, title, project, path, and components,
  plus edges with from/to/kind. The `path` field is a
  workspace-folder-relative file path added for editor integration (not
  present in the current CLI-only `GraphJson`).
- **Render data**: Per-document component trees with verification
  status, produced by the `documentComponents` endpoint. The array
  shape `[{ document_id, fences, edges }]` matches what the website
  explorer's `mount()` function expects as `renderData`.
- **Message protocol**: The `postMessage` API between the VS Code
  extension host and the webview, used to push data in and receive
  navigation actions out.

## Requirement 1: LSP Graph Data Endpoint

As an editor extension, I need the LSP to provide the full document
graph in a single request, so that the webview can render the
explorer without shelling out to the CLI.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE LSP server SHALL handle a custom `supersigil/graphData`
    request that accepts no parameters and returns a JSON payload
    matching the `GraphJson` schema: a `documents` array and an
    `edges` array.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/graph_data.rs" />
  </Criterion>
  <Criterion id="req-1-2">
    Each document in the response SHALL include `id`, `doc_type`,
    `status`, `title`, `project`, `path` (workspace-folder-relative file
    path), and `components`. Each edge SHALL include `from`, `to`,
    and `kind`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/graph_json.rs" />
  </Criterion>
  <Criterion id="req-1-3">
    THE `build_graph_json` function SHALL be available to the LSP
    crate. It SHALL be moved or re-exported from a shared location
    (not remain private to the CLI crate).
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/graph_json.rs" />
  </Criterion>
  <Criterion id="req-1-4">
    THE extension SHALL also fetch render data for all documents
    by calling the existing `supersigil/documentComponents` endpoint
    for each document in the graph response. The results SHALL be
    assembled into the render data array shape expected by the
    explorer's `mount()` function.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerWebview.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Webview Panel

As a spec author using VS Code, I want to open an interactive graph
explorer in a webview panel, so that I can visualize spec
relationships and drill into document details inside the editor.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE extension SHALL register a command `supersigil.openExplorer`
    that opens a webview panel with the graph visualization. THE
    command SHALL be accessible via an editor title action icon
    (`$(graph)`) that is always visible in the editor title bar,
    regardless of file type.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/package.json, editors/vscode/src/extension.ts" />
  </Criterion>
  <Criterion id="req-2-2">
    Each invocation of `supersigil.openExplorer` SHALL create a new
    webview panel. Multiple panels MAY be open simultaneously. There
    is no singleton behavior.
  </Criterion>
  <Criterion id="req-2-3">
    THE webview SHALL load the bundled explorer modules and call the
    `mount()` function with graph data, render data, and a
    host-provided link resolver. THE link resolver SHALL generate
    evidence links using an interceptable URI scheme and hash-based
    URIs for in-explorer document navigation. THE webview bootstrap
    script SHALL intercept evidence link clicks and convert them to
    `postMessage` calls (`openFile`) instead of navigating.
    In-explorer document navigation (node selection, edge clicks)
    SHALL proceed normally without interception.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerBootstrap.ts, editors/vscode/src/explorerBootstrap.test.ts" />
  </Criterion>
  <Criterion id="req-2-4">
    THE webview SHALL use a Content Security Policy that permits
    scripts only from the extension's resource directory with the
    correct nonce. Stylesheets SHALL be permitted from the
    extension's resource directory. Inline styles SHALL be permitted
    (`'unsafe-inline'`) because d3 and the detail panel set inline
    styles programmatically.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerWebview.ts" />
  </Criterion>
  <Criterion id="req-2-5">
    WHEN the command is invoked, THE extension SHALL determine the
    workspace root from the active editor's file URI via
    `vscode.workspace.getWorkspaceFolder`. IF no active editor
    exists or the file is not in a workspace folder, THE extension
    SHALL fall back to the first folder with a running LSP client.
  </Criterion>
  <Criterion id="req-2-6">
    THE panel title SHALL include the workspace folder name:
    "Spec Explorer ({folderName})".
  </Criterion>
  <Criterion id="req-2-7">
    IF the active editor's file corresponds to a spec document in
    the graph, THE webview SHALL auto-focus that document's node
    on initial load.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Live Updates

As a spec author, I want the graph to update when specs change, so
that the webview always reflects the current project state.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN the extension receives a `supersigil/documentsChanged`
    notification from an LSP client, THE extension SHALL re-fetch
    graph data and render data and push an update to every open
    webview panel whose root matches the notifying client's
    workspace folder, provided the panel is visible. WHEN a panel
    transitions from hidden to visible (via `onDidChangeViewState`),
    THE extension SHALL re-fetch data to cover changes missed while
    the panel was hidden.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts, editors/vscode/src/explorerWebview.ts" />
  </Criterion>
  <Criterion id="req-3-2">
    THE webview SHALL handle incoming data updates by clearing the
    explorer container and calling `mount()` with the new data. THE
    bootstrap script SHALL capture the current view state (selected
    document ID, filter state, hash) before clearing and restore it
    after re-mounting via the URL router's hash-based state
    restoration.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerBootstrap.ts, website/src/components/explore/graph-explorer.js" />
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
    THE detail panel SHALL include an "Open File" button in the
    document header. Clicking it SHALL send a message to the
    extension that opens the corresponding spec file in the editor.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerBootstrap.test.ts" />
  </Criterion>
  <Criterion id="req-4-2">
    Evidence source links (test file + line number) in the detail
    panel SHALL be clickable. Clicking one SHALL send a message to
    the extension that opens the file at the specified line.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerBootstrap.test.ts" />
  </Criterion>
  <Criterion id="req-4-3">
    Document links in edges and criterion references SHALL navigate
    within the explorer (selecting the target node in the graph),
    not open a new editor tab.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerBootstrap.ts" />
  </Criterion>
  <Criterion id="req-4-4">
    THE extension SHALL resolve file paths from the graph data
    against the workspace root to produce absolute URIs for
    `vscode.open`.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerWebview.test.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Message Protocol

As a maintainer, I want a clear message contract between the
extension and webview, so that the integration is predictable and
testable.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE extension SHALL send messages of type `graphData` to the
    webview containing the graph JSON, render data array, the
    current root (`{ uri, name }`), the list of available roots
    (`[{ uri, name }]`), an optional `focusDocumentId` field, and
    an `isRootSwitch` boolean (true when the message results from
    a root switch). This message SHALL be sent when the bootstrap
    sends a `ready` message and on each `documentsChanged`
    notification.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerWebview.test.ts" />
  </Criterion>
  <Criterion id="req-5-2">
    THE webview SHALL send messages of type `openFile` to the
    extension containing a `path` field (workspace-folder-relative) and an
    optional `line` field (1-based line number). Document-to-file
    resolution SHALL happen in the webview using the `path` field
    from graph data; no extension round-trip is needed.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerBootstrap.test.ts" />
  </Criterion>
  <Criterion id="req-5-3">
    THE webview SHALL send messages of type `switchRoot` to the
    extension containing a `folderUri` field. THE extension SHALL
    validate the `folderUri` against the running clients map and
    reject unknown or non-running roots. On success, THE extension
    SHALL update the panel's root, re-fetch data from the new
    root's LSP client, and push a fresh `graphData` message with
    `isRootSwitch: true`. THE webview SHALL do a clean remount
    with no state preservation (hash cleared).
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Theme Integration

As a VS Code user, I want the explorer to match my editor theme, so
that the webview feels native.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE webview SHALL include a CSS adapter that maps the explorer's
    design tokens (`--bg-surface`, `--text`, `--border`, etc.) to
    VS Code's `--vscode-*` CSS custom properties.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/media/vscode-theme-adapter.css" />
  </Criterion>
  <Criterion id="req-6-2">
    THE webview SHALL NOT load external fonts. Typography SHALL use
    VS Code's built-in font families (`--vscode-font-family` and
    `--vscode-editor-font-family`).
    <VerifiedBy strategy="file-glob" paths="editors/vscode/media/vscode-theme-adapter.css" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 7: Explorer Module Bundling

As a maintainer, I want the explorer modules bundled into the
extension at build time, so that the webview works offline and
doesn't depend on runtime file resolution.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-7-1">
    THE extension's build pipeline SHALL produce a bundled JS file
    containing the explorer modules, d3, and force-in-a-box, suitable
    for loading as a webview script.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/esbuild.mjs" />
  </Criterion>
  <Criterion id="req-7-2">
    THE extension's build pipeline SHALL produce bundled CSS files
    containing the explorer styles, design tokens, and presentation
    kit styles.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/esbuild.mjs" />
  </Criterion>
  <Criterion id="req-7-3">
    THE webview SHALL also load the presentation kit IIFE
    (`render-iife.js`) and interactive script
    (`supersigil-preview.js`) as separate script tags, matching the
    website's loading order.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerWebview.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 8: Spec Explorer Tree View Relocation

As a spec author, I want the Spec Explorer tree view to appear in the
built-in Explorer sidebar (alongside Outline and Timeline), so that
spec navigation is integrated with standard VS Code file exploration
rather than isolated in a separate activity bar icon.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-8-1">
    THE Spec Explorer tree view SHALL be registered under the
    built-in `explorer` view container instead of a custom
    `supersigil` activity bar view container.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/package.json" />
  </Criterion>
  <Criterion id="req-8-2">
    THE custom `supersigil` activity bar view container SHALL be
    removed. THE Supersigil icon SHALL no longer appear in the
    activity bar.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/package.json" />
  </Criterion>
  <Criterion id="req-8-3">
    THE Spec Explorer tree view's data provider, welcome views,
    and `documentsChanged` notification wiring SHALL remain
    unchanged.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 9: Root Selector

As a spec author working in a multi-root workspace, I want to
switch the graph explorer's root without closing and reopening the
panel, so that I can quickly navigate between workspace roots.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-9-1">
    THE graph explorer SHALL include a root selector dropdown in
    its toolbar (inside the webview, alongside the filter bar).
    THE dropdown SHALL list all workspace folders that have a
    running LSP client.
  </Criterion>
  <Criterion id="req-9-2">
    WHEN the user selects a different root from the dropdown, THE
    webview SHALL send a `switchRoot` message to the extension. THE
    extension SHALL update the panel's root and push fresh graph
    data. THE webview SHALL do a clean remount with no state
    preservation.
  </Criterion>
  <Criterion id="req-9-3">
    WHEN only one workspace root has a running LSP client, THE root
    selector dropdown SHALL be hidden.
  </Criterion>
  <Criterion id="req-9-4">
    THE explorer bar (filter bar) SHALL wrap its children when
    horizontal space is insufficient, so that the root selector,
    type filter, status filter, and search input remain usable on
    narrow panels.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/styles.css" />
  </Criterion>
</AcceptanceCriteria>
```
