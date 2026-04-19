---
supersigil:
  id: vscode-explorer-webview/design
  type: design
  status: superseded
title: "VS Code Explorer Webview"
---

```supersigil-xml
<Implements refs="vscode-explorer-webview/req" />
```

```supersigil-xml
<References refs="spec-rendering/design, graph-explorer/design" />
```

```supersigil-xml
<TrackedFiles paths="crates/supersigil-lsp/src/explorer_runtime.rs, crates/supersigil-lsp/src/state.rs, editors/vscode/src/explorerWebview.ts, editors/vscode/src/explorerBootstrap.ts, editors/vscode/esbuild.mjs, editors/vscode/package.json, website/src/components/explore/graph-explorer.js, website/src/components/explore/styles.css" />
```

## Overview

Historical design for the original VS Code webview integration. The
current runtime architecture is specified by `graph-explorer-runtime/design`.

Three layers: a new LSP endpoint that serves the full graph JSON,
extension host code that manages the webview lifecycle and data flow,
and a thin bootstrap script inside the webview that bridges the
explorer modules to the VS Code environment.

```
Rust LSP
  supersigil/graphData (full graph + paths)
  supersigil/documentComponents (per-doc, existing)
        |
Extension Host (explorerWebview.ts)
  Fetches data, manages webview panel, handles messages
        |  postMessage + command URIs
Webview (explorerBootstrap.ts)
  Calls mount(), emits command URIs, injects "Open File" button
        |
  Explorer modules (bundled from website/, with unmount() addition)
  Preview kit IIFE (bundled from packages/preview/)
```

## Explorer Module Adaptations

Minor changes to the explorer modules to support the webview host.

### `unmount()` Export

`graph-explorer.js` gains an exported `unmount()` function that
cleans up resources allocated by `mount()`. The existing code
already notes (line 1848) that teardown is needed if `mount()` is
called multiple times. `unmount()` performs:

1. Stops the d3 force simulation.
2. Removes the `hashchange` listener registered by the URL router.
3. Removes `document`-level event listeners (click, mousemove,
   mouseup, keydown) registered during mount.

`mount()` returns a handle object `{ unmount }`. The bootstrap
calls `unmount()` before each re-mount.

Implementation note: `mount()` currently registers listeners on
`document` (click x2, mousemove, mouseup, keydown) and a
`hashchange` listener via the URL router. `unmount()` must remove
all of these. To make removal possible, `mount()` must use named
handler references (not inline arrow functions) for each
`addEventListener` call so that `removeEventListener` can target
them.

### `linkResolver` Parameter on `mount()`

`mount()` gains an optional fifth parameter: a `linkResolver`
object with `evidenceLink(file, line)`, `documentLink(docId)`, and
`criterionLink(docId, criterionId)` methods. When provided, the
explorer passes it to `renderDetail` and the presentation kit
instead of calling `createExplorerLinkResolver(repositoryInfo)`.
When absent, behavior is unchanged (the existing
`createExplorerLinkResolver` is used).

This lets the VS Code webview provide a link resolver that returns
`command:supersigil.openGraphFile` URIs for file-opening actions
while keeping in-explorer navigation via hash-based document links.
Because command URIs are handled globally by the extension, they keep
working even if the webview loses its panel-local `onDidReceiveMessage`
bridge during an extension-host restart.

## LSP Side

### Custom Request: `supersigil/graphData`

New module `crates/supersigil-lsp/src/graph_data.rs`. Reuses the
existing `build_graph_json` function from the CLI crate, which must
be relocated to a shared crate.

```rust
pub struct GraphDataRequest;

impl Request for GraphDataRequest {
    type Params = serde_json::Value;
    type Result = GraphDataResult;
    const METHOD: &'static str = "supersigil/graphData";
}
```

The response type wraps the existing `GraphJson` from
`supersigil-cli/src/commands/graph/json.rs`. That module's types
(`GraphJson`, `DocumentNode`, `Edge`, `Component`) and conversion
function (`build_graph_json`) move to `supersigil-verify` (which
both the CLI and LSP already depend on).

### Adding `path` to `DocumentNode`

`DocumentNode` gains a `path` field: the workspace-folder-relative
file path. `build_graph_json` gains a `project_root: &Path` parameter
(matching the existing `build_document_entries` signature). The path
is derived from `SpecDocument.path.strip_prefix(project_root)`, same
as in `document_list.rs`.

```rust
#[derive(Debug, Serialize)]
pub struct DocumentNode {
    pub id: String,
    pub doc_type: Option<String>,
    pub status: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub path: String,            // new field
    pub components: Vec<Component>,
}
```

### Handler

Registered in the LSP router alongside `DocumentListRequest`:

```rust
router.request::<GraphDataRequest, _>(Self::handle_graph_data);
```

The handler reads the `DocumentGraph` from the shared state and
calls `build_graph_json(graph, project_root)`. No parameters needed
— it always returns the full graph for the workspace.

A `workspace/executeCommand` mirror (`supersigil.graphData`) is
also registered for future IntelliJ use, following the existing
pattern.

## Extension Host

### `editors/vscode/src/explorerWebview.ts`

Each graph panel is an independent instance. The extension tracks
all open panels in an array for refresh routing.

```typescript
interface ExplorerPanel {
  panel: vscode.WebviewPanel;
  folderUri: vscode.Uri;
  clientKey: string;
  refreshGeneration: number;
  focusDocumentId: string | null;
}

const openPanels: ExplorerPanel[] = [];

function openExplorerPanel(
  context: vscode.ExtensionContext,
  clients: Map<string, LanguageClient>,
): void;

function refreshPanelsForClient(clientKey: string): void;
```

### Root Resolution on Open

When `supersigil.openExplorer` is invoked:

1. Check `vscode.window.activeTextEditor?.document.uri`.
2. Resolve the workspace folder via
   `vscode.workspace.getWorkspaceFolder(uri)`.
3. Look up the LSP client for that folder key.
4. If no active editor or no matching client, fall back to the
   first folder with a running LSP client.
5. Determine `focusDocumentId`: after fetching graph data, find
   the document whose `path` matches the active file's
   workspace-relative path. Pass as `focusDocumentId` in the
   `graphData` message.

### Multi-Instance Panels

Each invocation creates a new `WebviewPanel`. No singleton check.
The panel is added to `openPanels`. On dispose, it is removed.
The panel title is `"Spec Explorer (${folderName})"`.

### Data Fetching

`pushData()` performs two LSP requests with bounded concurrency
(limit 10):

1. `supersigil/graphData` from the panel's stored client.
2. `supersigil/documentComponents` for each document (parallel,
   tolerating individual failures via `Promise.allSettled`).

The assembled data is posted to the webview with root info:

```typescript
panel.webview.postMessage({
  type: 'graphData',
  graph: graphJson,
  renderData: renderDataArray,
  currentRoot: { uri: folderUri, name: folderName },
  availableRoots: allRunningRoots,
  focusDocumentId: focusDocId ?? undefined,
});
```

A generation counter discards stale results from concurrent
refreshes.

### Message Handling

```typescript
switch (msg.type) {
  case 'ready':
    pushData(panel);
    break;
  case 'switchRoot': {
    // Validate against running clients before accepting
    const newClient = clients.get(msg.folderUri);
    if (!newClient?.isRunning()) break;
    panel.folderUri = vscode.Uri.parse(msg.folderUri);
    panel.clientKey = msg.folderUri;
    panel.focusDocumentId = null;
    pushData(panel, { isRootSwitch: true });
    break;
  }
}
```

The primary file-opening path is the globally registered
`supersigil.openGraphFile` command, which accepts either an explicit
`file:` URI or a `{ path, folderUri }` pair plus an optional line.
The panel-local `openFile` message handler may remain as a
compatibility fallback for already-rendered legacy links. Path
validation rejects absolute paths, `..` segments, and any resolved
path outside the workspace root.

### Command and Editor Title Action

In `package.json`:

```jsonc
{
  "commands": [
    {
      "command": "supersigil.openExplorer",
      "title": "Supersigil: Open Graph Explorer",
      "icon": "$(graph)"
    }
  ],
  "menus": {
    "editor/title": [
      {
        "command": "supersigil.openExplorer",
        "group": "navigation"
      }
    ]
  }
}
```

No `when` clause — the icon is always visible.

### Spec Explorer Tree View Relocation

In `package.json`, move the tree view registration:

```jsonc
// Remove viewsContainers.activitybar entirely.
// Change views target from "supersigil" to "explorer":
"views": {
  "explorer": [
    { "id": "supersigil.specExplorer", "name": "Spec Explorer" }
  ]
}
```

Welcome views and data provider are unchanged.

### Live Updates

On `documentsChanged` notification from a client, iterate
`openPanels` and call `refreshPanelsForClient(clientKey)` which
pushes fresh data to every visible panel whose `clientKey` matches.

Each panel also registers an `onDidChangeViewState` handler. When
a panel transitions from hidden to visible (`e.webviewPanel.visible`
becomes true), it re-fetches data via `pushData`. This handles the
case where `retainContextWhenHidden` keeps the DOM alive but
`documentsChanged` refreshes were skipped while the panel was
hidden. Combined with the bootstrap's `ready` message (which
handles DOM recreation), both stale-content scenarios are covered.

### Webview HTML

The `getHtmlContent` method generates the full HTML document:

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <meta http-equiv="Content-Security-Policy"
    content="default-src 'none';
      style-src ${webview.cspSource} 'unsafe-inline';
      script-src 'nonce-${nonce}';
      img-src ${webview.cspSource} data:;" />
  <link rel="stylesheet" href="${tokensUri}" />
  <link rel="stylesheet" href="${explorerStylesUri}" />
  <link rel="stylesheet" href="${previewStylesUri}" />
  <link rel="stylesheet" href="${themeAdapterUri}" />
</head>
<body>
  <div id="explorer" style="height: 100vh;"></div>
  <script nonce="${nonce}" src="${renderIifeUri}"></script>
  <script nonce="${nonce}" src="${previewScriptUri}"></script>
  <script nonce="${nonce}" src="${explorerBundleUri}"></script>
  <script nonce="${nonce}" src="${bootstrapUri}"></script>
</body>
</html>
```

All URIs are generated via `webview.asWebviewUri()` from the
extension's `dist/webview/` directory.

## Webview Bootstrap

### New File: `editors/vscode/src/explorerBootstrap.ts`

A small script that runs inside the webview. Bundled separately
from the explorer modules (it depends on the VS Code webview API).

### Initialization

```typescript
const vscode = acquireVsCodeApi();
const container = document.getElementById('explorer')!;

window.addEventListener('message', (event) => {
  const msg = event.data;
  if (msg.type === 'graphData') {
    mountExplorer(
      msg.graph, msg.renderData,
      msg.focusDocumentId, msg.isRootSwitch,
    );
    // Render root selector after mount, since mount creates
    // the .explorer-bar that the selector is prepended to.
    updateRootSelector(msg.currentRoot, msg.availableRoots);
  }
});

vscode.postMessage({ type: 'ready' });
```

### Link Resolver

The bootstrap provides a custom `linkResolver` to `mount()` (via
the new optional fifth parameter). Evidence links post messages
back to the extension; document and criterion links use the
existing hash-based in-explorer navigation:

```typescript
const EVIDENCE_SCHEME = 'supersigil-evidence';

const linkResolver = {
  evidenceLink: (file: string, line: number) =>
    `${EVIDENCE_SCHEME}:${encodeURIComponent(file)}?line=${line}`,
  documentLink: (docId: string) =>
    `#/doc/${encodeURIComponent(docId)}`,
  criterionLink: (docId: string, _criterionId: string) =>
    `#/doc/${encodeURIComponent(docId)}`,
};
```

A click handler intercepts evidence link clicks:

```typescript
container.addEventListener('click', (e) => {
  const anchor = (e.target as HTMLElement).closest('a');
  if (!anchor) return;

  const href = anchor.getAttribute('href') ?? '';
  if (href.startsWith(EVIDENCE_SCHEME + ':')) {
    e.preventDefault();
    const encoded = href.slice(EVIDENCE_SCHEME.length + 1);
    const [filePart, query] = encoded.split('?');
    const line = new URLSearchParams(query).get('line');
    vscode.postMessage({
      type: 'openFile',
      path: decodeURIComponent(filePart),
      line: line ? parseInt(line, 10) : undefined,
    });
    return;
  }

  // Hash-based links (#/doc/...) are in-explorer navigation;
  // let them proceed normally.
});
```

### "Open File" Button Injection

After each `mount()` call, the bootstrap observes the detail panel
for document selection and injects an "Open File" button. It stores
a document-ID-to-path lookup built from the graph data:

```typescript
let pathByDocId: Map<string, string>;
let currentUnmount: (() => void) | null = null;
let detailObserver: MutationObserver | null = null;

function mountExplorer(
  graph: GraphData,
  renderData: unknown[],
  focusDocumentId?: string,
  isRootSwitch?: boolean,
) {
  pathByDocId = new Map(
    graph.documents.map((d) => [d.id, d.path]),
  );

  // On root switch: clear hash to avoid carrying stale state
  // from the previous root. On focus: set hash to the target
  // document. On live update: preserve current hash.
  let targetHash: string;
  if (isRootSwitch) {
    targetHash = '';
  } else if (focusDocumentId) {
    targetHash = `#/doc/${encodeURIComponent(focusDocumentId)}`;
  } else {
    targetHash = window.location.hash;
  }

  // Teardown previous mount and observer
  if (detailObserver) { detailObserver.disconnect(); detailObserver = null; }
  if (currentUnmount) currentUnmount();

  container.innerHTML = '';
  const handle = SupersigilExplorer.mount(
    container, graph, renderData, null, linkResolver,
  );
  currentUnmount = handle?.unmount ?? null;

  if (targetHash) {
    window.location.hash = targetHash;
  }

  observeDetailPanel();
}
```

The observer watches for `.detail-panel-header` elements appearing
in the DOM and appends a button:

```typescript
function observeDetailPanel() {
  detailObserver = new MutationObserver(() => {
    const header = container.querySelector('.detail-panel-header');
    if (!header || header.querySelector('.open-file-btn')) return;

    const titleEl = header.querySelector('.detail-panel-title');
    if (!titleEl) return;

    const docId = titleEl.textContent?.trim();
    if (!docId || !pathByDocId.has(docId)) return;

    const btn = document.createElement('button');
    btn.className = 'open-file-btn';
    btn.textContent = 'Open File';
    btn.title = `Open ${pathByDocId.get(docId)}`;
    btn.addEventListener('click', () => {
      vscode.postMessage({
        type: 'openFile',
        path: pathByDocId.get(docId)!,
      });
    });
    header.insertBefore(btn, header.querySelector('.detail-panel-close'));
  });
  detailObserver.observe(container, { childList: true, subtree: true });
}
```

### Root Selector

The bootstrap renders a `<select>` element in the `.explorer-bar`
when `availableRoots.length > 1`. Selecting a different root sends
a `switchRoot` message:

```typescript
function updateRootSelector(
  currentRoot: { uri: string; name: string },
  availableRoots: Array<{ uri: string; name: string }>,
) {
  const existing = container.querySelector('.root-selector');
  if (existing) existing.remove();

  if (availableRoots.length <= 1) return;

  const bar = container.querySelector('.explorer-bar');
  if (!bar) return;

  const select = document.createElement('select');
  select.className = 'root-selector';
  for (const root of availableRoots) {
    const opt = document.createElement('option');
    opt.value = root.uri;
    opt.textContent = root.name;
    opt.selected = root.uri === currentRoot.uri;
    select.appendChild(opt);
  }
  select.addEventListener('change', () => {
    vscode.postMessage({ type: 'switchRoot', folderUri: select.value });
  });
  bar.prepend(select);
}
```

The `.root-selector` is styled in the theme adapter CSS to match
the filter dropdowns.

## Theme Adapter

### New File: `editors/vscode/media/vscode-theme-adapter.css`

Maps the explorer's design tokens to VS Code theme variables:

```css
:root {
  /* Surface and background */
  --bg: var(--vscode-editor-background);
  --bg-surface: var(--vscode-sideBar-background);
  --bg-card: var(--vscode-editorWidget-background);

  /* Text */
  --text: var(--vscode-editor-foreground);
  --text-muted: var(--vscode-descriptionForeground);
  --text-dim: var(--vscode-disabledForeground);

  /* Borders */
  --border: var(--vscode-panel-border);
  --border-hover: var(--vscode-focusBorder);

  /* Typography — use VS Code's fonts instead of Google Fonts */
  --font-body: var(--vscode-font-family);
  --font-mono: var(--vscode-editor-font-family);
  --font-heading: var(--vscode-font-family);

  /* Accent colors — map to VS Code testing colors where applicable */
  --accent: var(--vscode-focusBorder);
  --accent-hover: var(--vscode-focusBorder);

  /* Supersigil preview kit tokens */
  --supersigil-bg: transparent;
  --supersigil-border: var(--vscode-panel-border);
  --supersigil-text: var(--vscode-editor-foreground);
  --supersigil-text-muted: var(--vscode-descriptionForeground);
  --supersigil-font-mono: var(--vscode-editor-font-family);
}
```

Additional overrides for the `.open-file-btn` injected by the
bootstrap:

```css
.open-file-btn {
  font-family: var(--vscode-font-family);
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 2px;
  border: 1px solid var(--vscode-button-border, transparent);
  background: var(--vscode-button-secondaryBackground);
  color: var(--vscode-button-secondaryForeground);
  cursor: pointer;
}

.open-file-btn:hover {
  background: var(--vscode-button-secondaryHoverBackground);
}
```

## Build Pipeline

### Extension esbuild Changes

The extension's `esbuild.mjs` currently has one entrypoint
(`src/extension.ts` → `dist/extension.js`). Add a second build
pass for the webview:

```javascript
// Webview bundle: explorer modules + d3 + force-in-a-box
// Note: no d3 alias here — unlike the standalone CLI bundle (which
// aliases d3 to a globalThis shim for CDN loading), the webview
// bundle resolves d3 from node_modules so it is fully self-contained.
await esbuild.build({
  entryPoints: ['../../website/src/components/explore/graph-explorer.js'],
  bundle: true,
  format: 'iife',
  globalName: 'SupersigilExplorer',
  mainFields: ['module', 'main'],
  outfile: 'dist/webview/explorer.js',
});

// Bootstrap script
await esbuild.build({
  entryPoints: ['src/explorerBootstrap.ts'],
  bundle: true,
  format: 'iife',
  outfile: 'dist/webview/bootstrap.js',
});
```

CSS and preview kit assets are copied to `dist/webview/`:

```javascript
// Copy CSS files
await fs.copyFile(
  '../../website/src/styles/landing-tokens.css',
  'dist/webview/landing-tokens.css',
);
await fs.copyFile(
  '../../website/src/components/explore/styles.css',
  'dist/webview/explorer-styles.css',
);
await fs.copyFile(
  '../../packages/preview/dist/supersigil-preview.css',
  'dist/webview/supersigil-preview.css',
);
await fs.copyFile(
  '../../packages/preview/dist/render-iife.js',
  'dist/webview/render-iife.js',
);
await fs.copyFile(
  '../../packages/preview/dist/supersigil-preview.js',
  'dist/webview/supersigil-preview.js',
);
```

The theme adapter CSS is in the extension source at
`media/vscode-theme-adapter.css` and is copied to
`dist/webview/vscode-theme-adapter.css`.

### devDependencies

The extension gains `d3` and `force-in-a-box` as devDependencies
(esbuild bundles them; they are not runtime dependencies):

```json
{
  "devDependencies": {
    "d3": "^7.9.0",
    "force-in-a-box": "^1.0.2"
  }
}
```

### VSIX Packaging

The `dist/webview/` directory must be included in the extension
package. The existing `.vscodeignore` is updated to include it.

## Error Handling

**LSP not running**: If no LSP client is active when the user
invokes `openExplorer`, show an information message: "No Supersigil
project found. Start by opening a workspace with a supersigil.toml."

**Graph data fetch failure**: If `supersigil/graphData` fails, show
an error state in the webview with a "Retry" button.

**documentComponents batch failure**: Individual failures are
tolerated — the render data array omits documents whose component
fetch failed. The explorer shows graph structure without spec
content for those documents.

**Webview disposed**: When a panel is closed, it is removed from
the `openPanels` array.

## Testing Strategy

**LSP `graphData` endpoint**: Unit tests in
`crates/supersigil-lsp/src/graph_data.rs` verifying the response
shape, `path` field correctness, and consistency with the existing
CLI `build_graph_json` output.

**`build_graph_json` relocation**: Existing CLI tests for
`build_graph_json` move with the function to `supersigil-verify`.
Tests require updates to account for the new `project_root`
parameter and the added `path` field in `DocumentNode` assertions.

**Extension host logic**: Panel creation functions are tested via
mock LSP responses. Key cases: root resolution from active editor,
fallback when no active editor, multi-panel tracking, per-client
refresh routing, `switchRoot` handling, `openFile` path validation,
`focusDocumentId` resolution from active file path.

**Bootstrap script**: The link interception logic (evidence scheme
URL parsing) is tested with href fixture strings. The DOM injection
logic (Open File button) is tested with mock HTML fragments.

**Theme adapter**: Visual verification — no automated test needed.
The adapter is pure CSS property mapping.

**d3 CSP compatibility**: Verified during initial implementation by
loading the webview and confirming no CSP violations in the
developer tools console.

## Decisions

```supersigil-xml
<Decision id="link-resolver-parameter">
  Add an optional linkResolver parameter to mount() so that the
  VS Code webview can provide its own evidence link scheme.

  <References refs="vscode-explorer-webview/req#req-2-3, vscode-explorer-webview/req#req-4-2" />

  <Rationale>
  The requirements explicitly permit minor explorer module
  adaptations for host integration. The existing
  createExplorerLinkResolver generates plain text (not clickable
  anchors) when repositoryInfo is null, making interception
  impossible. Adding an optional linkResolver parameter is a small,
  backward-compatible change: when omitted, mount() falls back to
  createExplorerLinkResolver(repositoryInfo) as before. This is
  cleaner than a synthetic repositoryInfo hack that would require
  fragile URL parsing in the bootstrap.
  </Rationale>

  <Alternative id="synthetic-repository-info" status="rejected">
    Pass a fake repositoryInfo with a recognizable hostname and
    intercept the generated URLs. Avoids module changes but produces
    brittle URL parsing logic in the bootstrap and couples the
    webview to the internal URL format of the GitHub link template.
  </Alternative>

  <Alternative id="dom-post-processing" status="rejected">
    Render with null repositoryInfo, then walk the DOM after mount()
    to find evidence spans and wrap them in anchor elements. Fragile
    — depends on the exact DOM structure of plain-text evidence
    entries, which is an implementation detail of the presentation
    kit.
  </Alternative>
</Decision>

<Decision id="observer-injected-open-button">
  Inject the "Open File" button via a MutationObserver in the
  bootstrap script, rather than modifying the detail panel module.

  <References refs="vscode-explorer-webview/req#req-4-1" />

  <Rationale>
  The detail panel re-renders its innerHTML on every node selection.
  A MutationObserver that watches for the detail-panel-header element
  can inject the button each time, using the document ID from the
  title text and the path from the graph data lookup. This keeps the
  explorer modules untouched — the button is a host-specific addition
  injected by the thin wrapper layer.
  </Rationale>

  <Alternative id="modify-detail-panel" status="rejected">
    Add an "Open File" callback or slot to the detail panel's
    renderDetail function. Cleaner integration but modifies the shared
    module for a VS Code-specific feature.
  </Alternative>
</Decision>

<Decision id="unmount-and-remount">
  Add an unmount() function to the explorer and use
  unmount-then-remount for live updates, with hash-based state
  restoration, rather than implementing incremental data updates.

  <References refs="vscode-explorer-webview/req#req-3-2" />

  <Rationale>
  The explorer's mount() registers document-level listeners (click,
  mousemove, mouseup, keydown), a hashchange listener, and a d3
  force simulation that are not cleaned up when the container is
  cleared. Repeated mount() calls would leak handlers and stale
  simulations. Adding unmount() (stop simulation, remove all
  listeners) is a small, focused change. The bootstrap calls unmount() before
  each re-mount. Hash capture/restore via the URL router recovers
  selected node and filter state. Some transient state (pan/zoom,
  simulation convergence) is lost, which is acceptable for an
  infrequent operation triggered by spec file changes.
  </Rationale>

  <Alternative id="incremental-update-api" status="deferred">
    Add an update(data) method to the explorer that diffs the graph
    and updates d3's data join. More precise but significantly more
    complex and only benefits the webview consumer. Can be revisited
    if full reload proves too disruptive.
  </Alternative>
</Decision>

<Decision id="relocate-graph-json-to-verify">
  Move the GraphJson types and build_graph_json function from
  supersigil-cli to supersigil-verify.

  <References refs="vscode-explorer-webview/req#req-1-3" />

  <Rationale>
  Both the CLI (for the graph command and explore command) and the LSP
  (for the new graphData endpoint) need to produce GraphJson. The CLI
  depends on supersigil-verify. The LSP depends on supersigil-verify.
  Moving the shared code to supersigil-verify avoids a dependency from
  the LSP on the CLI crate and keeps the conversion logic next to the
  DocumentGraph type it operates on.
  </Rationale>

  <Alternative id="new-shared-crate" status="rejected">
    Create a supersigil-graph-json crate. Over-engineered for a
    single struct and conversion function. Adds a crate to the
    workspace for minimal benefit.
  </Alternative>
</Decision>

<Decision id="multi-instance-panels">
  Each command invocation creates a new panel rather than using a
  singleton pattern. Root is resolved from the active file context.

  <References refs="vscode-explorer-webview/req#req-2-2, vscode-explorer-webview/req#req-2-5" />

  <Rationale>
  Multi-root workspaces often represent git worktrees where users
  switch between branches/features frequently. Multiple panels let
  users compare graphs side by side without losing context. Root
  resolution from the active file makes opening context-aware with
  no extra clicks. An in-panel root selector dropdown handles the
  less common case of wanting to switch a panel's root without
  opening a new one.
  </Rationale>

  <Alternative id="singleton-with-cache" status="rejected">
    Single panel with per-root cached state. Switching roots
    restores the cached zoom, selection, and filters. More complex
    (cache invalidation, state serialization) and prevents
    side-by-side comparison.
  </Alternative>

  <Alternative id="singleton-remount" status="rejected">
    Single panel, switching roots does a clean remount. Simpler than
    caching but still limits users to one graph at a time.
  </Alternative>
</Decision>

<Decision id="tree-view-in-explorer">
  Move the Spec Explorer tree view from a custom activity bar
  container to the built-in Explorer sidebar.

  <References refs="vscode-explorer-webview/req#req-8-1, vscode-explorer-webview/req#req-8-2" />

  <Rationale>
  The tree view is a file-navigation aid, like Outline and Timeline.
  Placing it alongside them in the Explorer sidebar is more
  discoverable and avoids a dedicated activity bar icon competing
  with Git, Search, and other core VS Code features. The graph
  explorer — which is the richer, more distinctive feature — gets
  the prominent editor title action instead.
  </Rationale>

  <Alternative id="keep-activity-bar" status="rejected">
    Keep the Supersigil activity bar icon with the tree view in its
    own sidebar. Wastes a sidebar slot for a simple tree that fits
    naturally in Explorer.
  </Alternative>
</Decision>
```
