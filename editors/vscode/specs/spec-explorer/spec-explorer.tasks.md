---
supersigil:
  id: spec-explorer/tasks
  type: tasks
  status: done
title: "Spec Explorer Tree View"
---

```supersigil-xml
<DependsOn refs="spec-explorer/design" />
```

## Overview

Five tasks: LSP protocol types and request handler, LSP notification
wiring, extension tree data provider, extension registration and
lifecycle wiring, and a final smoke test pass.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="spec-explorer/req#req-1-1, spec-explorer/req#req-1-2"
>
  Create `document_list.rs` in `supersigil-lsp` with custom LSP
  protocol types: `DocumentListRequest` (implementing
  `lsp_types::request::Request`), `DocumentListParams`,
  `DocumentListResult`, `DocumentEntry`, and `DocumentsChanged`
  (implementing `lsp_types::notification::Notification`). Implement
  `build_document_entries()` on `SupersigilLsp` to iterate
  `self.graph.documents()`, extract frontmatter fields, make paths
  relative to `self.project_root`, and resolve project membership
  from config. Register the request handler on the Router in
  `new_router()` via the Router's custom request registration API.
  Add unit tests: single-project, multi-project, and empty graph.
</Task>

<Task
  id="task-2"
  status="done"
  implements="spec-explorer/req#req-1-3"
  depends="task-1"
>
  Add `notify_documents_changed()` helper to `SupersigilLsp` that
  sends `supersigil/documentsChanged` via the client socket's
  notify method with the DocumentsChanged type. Call it after
  `republish_all_diagnostics()` in the three re-index paths:
  `initialized()`, `did_save()`, and `did_change_watched_files()`.
</Task>

<Task
  id="task-3"
  status="done"
  implements="spec-explorer/req#req-2-1, spec-explorer/req#req-2-2, spec-explorer/req#req-2-3, spec-explorer/req#req-2-4, spec-explorer/req#req-3-1, spec-explorer/req#req-4-1, spec-explorer/req#req-4-2, spec-explorer/req#req-4-3, spec-explorer/req#req-4-4, spec-explorer/req#req-4-5, spec-explorer/req#req-5-2"
  depends="task-1"
>
  Create `specExplorer.ts` in `editors/vscode/src/` with the
  `SpecExplorerProvider` class implementing
  TreeDataProvider for SpecTreeItem. Implement: tree item types
  (WorkspaceRoot, Project, Group, Document), `getChildren()` with
  grouping logic (workspace root → project → prefix → document),
  `getTreeItem()` with doc-type codicon mapping, status-based
  `ThemeColor`, diagnostic icon override via
  `vscode.languages.getDiagnostics()`, inline description text,
  group node document counts, and click-to-open via `vscode.open`
  command. Implement `onDidChangeDiagnostics` listener that fires
  `onDidChangeTreeData` for affected document items. Add unit tests
  for grouping logic and icon/color assignment with mock data.
</Task>

<Task
  id="task-4"
  status="done"
  implements="spec-explorer/req#req-5-1, spec-explorer/req#req-6-1, spec-explorer/req#req-6-2"
  depends="task-2,task-3"
>
  Wire the tree view into the extension lifecycle. In `package.json`:
  add `supersigil` view container in activity bar with `icon.svg`,
  `supersigil.specExplorer` view, welcome content with CTA button
  bound to `supersigil.init` command, and register the new command.
  Create `icon.svg` (monochrome activity bar icon). In `extension.ts`:
  instantiate `SpecExplorerProvider` in `activate()`, register it with
  `registerTreeDataProvider`, register `supersigil.init` command that
  opens a terminal and runs `supersigil init`, set
  `supersigil.noRoots` context key, register
  `supersigil/documentsChanged` notification handler on each client
  in `startClientForFolder()`, and call `provider.refresh()` on
  client start/stop.
</Task>

<Task
  id="task-5"
  status="done"
  depends="task-4"
>
  End-to-end smoke test: verify `supersigil verify`, `cargo clippy`,
  `cargo fmt`, `cargo nextest run` all pass. Build the VS Code
  extension with `pnpm run compile` and confirm no TypeScript errors.
  Manually verify the extension activates and shows the tree view
  in a workspace with `supersigil.toml`.
</Task>
```
