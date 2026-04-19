---
supersigil:
  id: vscode-explorer-webview/tasks
  type: tasks
  status: done
title: "VS Code Explorer Webview"
---

```supersigil-xml
<DependsOn refs="vscode-explorer-webview/design" />
```

## Overview

Historical implementation plan for the original VS Code webview
integration. The current runtime rollout is tracked by
`graph-explorer-runtime/tasks`.

Tasks 1-11 cover the initial webview implementation (done). Tasks
12-19 cover the UX restructuring: tree view relocation, editor
title action, multi-instance panels, root selector, and explorer
bar wrapping.

JS tests use vitest (existing test infrastructure for the website
explorer and preview kit). Rust tests use `cargo nextest run`.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="vscode-explorer-webview/req#req-1-3"
>
  Relocate `GraphJson` types and `build_graph_json` from
  `supersigil-cli/src/commands/graph/json.rs` to
  `supersigil-verify`. Add `path` field to `DocumentNode` and add
  `project_root` parameter to `build_graph_json`. Update the
  CLI's `graph` and `explore` commands to import from the new
  location. Move existing tests and update them for the new
  signature and `path` field assertions. Run `cargo nextest run` to
  confirm no regressions.
</Task>

<Task
  id="task-2"
  status="done"
  implements="vscode-explorer-webview/req#req-1-1, vscode-explorer-webview/req#req-1-2"
  depends="task-1"
>
  TDD: write tests for the `supersigil/graphData` LSP endpoint in
  `crates/supersigil-lsp/src/graph_data.rs`. Test cases: response
  shape matches `GraphJson` schema, `path` field is
  workspace-folder-relative, edges are present, empty graph returns
  empty arrays. Then implement the `GraphDataRequest` type, register
  the handler in the LSP router, and add the
  `workspace/executeCommand` mirror (`supersigil.graphData`).
</Task>

<Task
  id="task-3"
  status="done"
  implements="vscode-explorer-webview/req#req-3-2"
>
  Add `unmount()` to `graph-explorer.js`. Refactor `mount()` to use
  named handler references for all `document.addEventListener` calls
  (click x2, mousemove, mouseup, keydown) and the hashchange
  listener. `mount()` returns `{ unmount }`. `unmount()` stops the
  d3 simulation, removes all document/window listeners, and is safe
  to call multiple times. TDD: write vitest tests first. Test cases:
  mount returns handle with unmount function; after unmount, document
  click/mousemove/mouseup/keydown listeners are removed (spy on
  removeEventListener); hashchange listener is unsubscribed; d3
  simulation is stopped; calling mount then unmount then mount again
  does not duplicate document-level handlers (count active listeners
  via spy). Run existing explorer tests to confirm no regressions.
</Task>

<Task
  id="task-4"
  status="done"
  implements="vscode-explorer-webview/req#req-2-3, vscode-explorer-webview/req#req-4-2, vscode-explorer-webview/req#req-4-3"
  depends="task-3"
>
  Add optional `linkResolver` parameter to `mount()` in
  `graph-explorer.js`. When provided, pass it through to
  `renderDetail` and the presentation kit instead of calling
  `createExplorerLinkResolver(repositoryInfo)`. When absent,
  behavior is unchanged. Add vitest tests: mount with custom
  linkResolver uses it for evidence links, mount without
  linkResolver falls back to repositoryInfo-based resolver. Run
  existing explorer and preview kit tests to confirm no regressions.
</Task>

<Task
  id="task-5"
  status="done"
  implements="vscode-explorer-webview/req#req-7-1, vscode-explorer-webview/req#req-7-2, vscode-explorer-webview/req#req-7-3"
  depends="task-4,task-6,task-7"
>
  Update the VS Code extension's `esbuild.mjs` to add a webview
  build pass. Bundle `graph-explorer.js` from the website source
  into `dist/webview/explorer.js` (IIFE, no d3 alias — resolve from
  node_modules). Bundle `explorerBootstrap.ts` into
  `dist/webview/bootstrap.js`. Copy CSS files (landing-tokens,
  explorer styles, supersigil-preview) and preview kit scripts
  (render-iife.js, supersigil-preview.js) to `dist/webview/`. Add
  `d3` and `force-in-a-box` (^1.0.2) as devDependencies. Verify
  `pnpm run build` produces all expected files in `dist/webview/`.
</Task>

<Task
  id="task-6"
  status="done"
  implements="vscode-explorer-webview/req#req-6-1, vscode-explorer-webview/req#req-6-2"
>
  Create `editors/vscode/media/vscode-theme-adapter.css` that maps
  the explorer's design tokens to `--vscode-*` CSS custom
  properties. Include `.open-file-btn` styles using VS Code button
  variables. Use VS Code font families instead of Google Fonts.
  The copy to `dist/webview/` is handled by `task-5`'s esbuild
  changes.
</Task>

<Task
  id="task-7"
  status="done"
  implements="vscode-explorer-webview/req#req-4-1, vscode-explorer-webview/req#req-5-2"
  depends="task-4"
>
  TDD: write vitest tests for the bootstrap script's link
  interception and Open File button injection. Test cases: evidence
  scheme href is parsed correctly into path + line, hash-based hrefs
  are not intercepted, Open File button is injected when
  detail-panel-header appears, button click sends correct
  postMessage. Then implement `explorerBootstrap.ts`: message
  listener, link resolver with `supersigil-evidence:` scheme,
  click handler, MutationObserver for Open File button injection
  with proper disconnect on re-mount.
</Task>

<Task
  id="task-8"
  status="done"
  implements="vscode-explorer-webview/req#req-1-4, vscode-explorer-webview/req#req-2-5, vscode-explorer-webview/req#req-2-6, vscode-explorer-webview/req#req-4-4, vscode-explorer-webview/req#req-5-1"
  depends="task-2,task-5"
>
  TDD: write tests for `ExplorerWebviewManager`'s core logic using
  mock LSP responses. Test cases: `pushData` assembles graph +
  render data and posts correct message shape; `handleMessage`
  resolves openFile path against workspace root with and without
  line; singleton panel behavior (second open reveals existing);
  workspace folder selection picks first active client; batch
  documentComponents failure for one doc still includes others in
  render data. Then implement `explorerWebview.ts` with
  `ExplorerWebviewManager`: `open()`, `pushData()`,
  `handleMessage()`, and `getHtmlContent()` (nonce-based CSP with
  `'unsafe-inline'` for styles, resource URIs via `asWebviewUri`).
</Task>

<Task
  id="task-9"
  status="done"
  implements="vscode-explorer-webview/req#req-2-1, vscode-explorer-webview/req#req-2-2, vscode-explorer-webview/req#req-2-4, vscode-explorer-webview/req#req-3-1, vscode-explorer-webview/req#req-3-2"
  depends="task-8"
>
  Wire the webview into the extension lifecycle. In `package.json`:
  register `supersigil.openExplorer` command with `$(graph)` icon,
  add toolbar button on the Spec Explorer view. In `extension.ts`:
  instantiate `ExplorerWebviewManager`, register the command, wire
  `documentsChanged` notification to `refresh()`. Handle error
  states (no LSP client shows info message, fetch failure shows
  retry in webview).
</Task>

<Task
  id="task-10"
  status="done"
  implements="vscode-explorer-webview/req#req-7-1, vscode-explorer-webview/req#req-7-2"
  depends="task-9"
>
  End-to-end smoke test: run `supersigil verify`, `cargo fmt
  --all`, `cargo clippy --workspace --all-targets --all-features`,
  `cargo nextest run`, `pnpm run build` (VSCode extension), and
  all vitest suites. Confirm no errors or warnings. Build the VSIX
  and verify `dist/webview/` contains all expected assets.
</Task>

<Task
  id="task-11"
  status="done"
  implements="vscode-explorer-webview/req#req-2-1, vscode-explorer-webview/req#req-4-1, vscode-explorer-webview/req#req-4-2, vscode-explorer-webview/req#req-3-1"
  depends="task-10"
>
  Manual verification: install the extension in VS Code, open a
  workspace with `supersigil.toml`, click the graph explorer toolbar
  button, confirm the graph renders with correct theme, click a
  document node and verify the detail panel shows spec content with
  verification badges, click "Open File" and verify the spec file
  opens, click an evidence link and verify the test file opens at
  the correct line, edit a spec file and verify the graph updates.
</Task>

<Task
  id="task-12"
  status="done"
  implements="vscode-explorer-webview/req#req-8-1, vscode-explorer-webview/req#req-8-2, vscode-explorer-webview/req#req-8-3"
>
  Relocate the Spec Explorer tree view from the custom `supersigil`
  activity bar view container to the built-in `explorer` view
  container. In `package.json`: remove the `viewsContainers` section
  entirely, change the `views` target from `"supersigil"` to
  `"explorer"`. Update welcome view `when` clauses if needed. Verify
  the tree view appears in the Explorer sidebar alongside Outline
  and Timeline. Run `pnpm run check-types` and `pnpm run build`.
</Task>

<Task
  id="task-13"
  status="done"
  implements="vscode-explorer-webview/req#req-2-1"
  depends="task-12"
>
  Change the graph explorer entry point from a view toolbar button
  to an editor title action. In `package.json`: change the menu
  contribution from `view/title` (with `when` clause) to
  `editor/title` (no `when` clause). The `$(graph)` icon is always
  visible in the editor title bar.
</Task>

<Task
  id="task-14"
  status="done"
  implements="vscode-explorer-webview/req#req-2-2, vscode-explorer-webview/req#req-2-5, vscode-explorer-webview/req#req-2-6, vscode-explorer-webview/req#req-2-7"
  depends="task-13"
>
  TDD: Rewrite `explorerWebview.ts` to support multi-instance
  panels. Remove `ExplorerWebviewManager` class; replace with
  `openExplorerPanel` function and `openPanels` array. Tests:
  each invocation creates a new panel (no singleton), root resolved
  from active editor, fallback to first running client, panel title
  includes folder name, `focusDocumentId` resolved from active
  file path. Update `extension.ts` wiring.
</Task>

<Task
  id="task-15"
  status="done"
  implements="vscode-explorer-webview/req#req-3-1"
  depends="task-14"
>
  Update `documentsChanged` notification handler to route refreshes
  to all matching panels. Iterate `openPanels`, refresh every
  visible panel whose `clientKey` matches the notifying client.
  Test: two panels for different roots, notification from one root
  only refreshes the matching panel.
</Task>

<Task
  id="task-16"
  status="done"
  implements="vscode-explorer-webview/req#req-9-1, vscode-explorer-webview/req#req-9-2, vscode-explorer-webview/req#req-9-3, vscode-explorer-webview/req#req-5-1, vscode-explorer-webview/req#req-5-3"
  depends="task-14"
>
  TDD: Add root selector dropdown to the webview bootstrap. Update
  `graphData` message to include `currentRoot`, `availableRoots`,
  and `focusDocumentId` fields. The bootstrap renders a `select`
  element in the `.explorer-bar` when multiple roots are available,
  hidden when only one. Selecting a root sends `switchRoot` message.
  Extension handles `switchRoot` by updating the panel's root and
  re-fetching. Tests: dropdown rendered with correct options,
  hidden for single root, `switchRoot` message sent on change.
</Task>

<Task
  id="task-17"
  status="done"
  implements="vscode-explorer-webview/req#req-9-4"
>
  Fix `.explorer-search` min-width to allow the explorer bar to
  wrap earlier on narrow panels. Reduce `min-width` from `220px`
  to `0` and use `flex: 1 1 120px` so it fills available space
  but wraps when tight. Add `.root-selector` styles to the theme
  adapter CSS. Verify wrapping works at narrow widths.
</Task>

<Task
  id="task-18"
  status="done"
  depends="task-15,task-16,task-17"
>
  End-to-end smoke test: run `supersigil verify`, `cargo fmt
  --all`, `cargo clippy --workspace --all-targets --all-features`,
  `cargo nextest run`, `pnpm run build`, and all vitest suites.
  Confirm no errors or warnings.
</Task>

<Task
  id="task-19"
  status="done"
  implements="vscode-explorer-webview/req#req-8-1, vscode-explorer-webview/req#req-2-1, vscode-explorer-webview/req#req-2-2, vscode-explorer-webview/req#req-9-1"
  depends="task-18"
>
  Manual verification: confirm the Spec Explorer tree appears in
  the Explorer sidebar (not activity bar), the graph icon appears
  in the editor title bar, opening the graph from different files
  scopes to the correct root with the file's node focused, multiple
  panels can be open side by side, root selector works in
  multi-root workspace, and the explorer bar wraps on narrow panels.
</Task>
```
