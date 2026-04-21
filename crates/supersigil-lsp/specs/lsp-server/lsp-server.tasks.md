---
supersigil:
  id: lsp-server/tasks
  type: tasks
  status: done
title: "Language Server Protocol Support"
---

```supersigil-xml
<DependsOn refs="lsp-server/design" />
```

## Overview

Implementation sequence for the LSP server. Prerequisite crate changes come
first, then the server skeleton, then features one at a time, each building
on the last. Each task is independently testable.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="lsp-server/req#req-8-1"
>
  Add `parse_content(path, content, defs)` to `supersigil-parser`. Refactor
  `parse_file` to read from disk then delegate to `parse_content`. Existing
  tests must continue to pass with no behavior change.
</Task>

<Task
  id="task-2"
  status="done"
  implements="lsp-server/req#req-5-1, lsp-server/req#req-5-2"
  depends="task-1"
>
  Create `crates/supersigil-lsp` with `async-lsp` and `lsp-types`
  dependencies. Implement `main.rs` (stdio transport), `lib.rs`
  (SupersigilLsp struct, LspService trait), and `state.rs` (config
  discovery, file discovery, initial parallel parse, graph build).
  Handle the no-config case (empty capabilities, watch for creation).
  Implement `position.rs` (SourcePosition to/from LSP Position).
  Add workspace member to root `Cargo.toml`.
</Task>

<Task
  id="task-3"
  status="done"
  implements="lsp-server/req#req-5-3, lsp-server/req#req-5-4, lsp-server/req#req-5-5, lsp-server/req#req-5-6"
  depends="task-2"
>
  Implement hybrid re-indexing in the LSP state layer (`state.rs`, later
  extracted into `state/indexing.rs` and `state/lifecycle.rs`): `didOpen`
  (init buffer, parse), `didChange` (update buffer, re-parse), `didSave`
  (rebuild graph only for project files, retain last-good on failure),
  `didClose` (clear buffer and diagnostics). Implement
  `didChangeWatchedFiles` for config and `.md` file changes, including
  deleted-file diagnostic cleanup. Add `window/workDoneProgress` during
  initial indexing.
</Task>

<Task
  id="task-4"
  status="done"
  implements="lsp-server/req#req-1-1, lsp-server/req#req-1-2, lsp-server/req#req-1-3, lsp-server/req#req-1-4, lsp-server/req#req-1-5"
  depends="task-3"
>
  Implement `diagnostics.rs`: convert `ParseError`, `GraphError`, and
  `Finding` to LSP `Diagnostic`. Filter by tier and exclude
  `ReportSeverity::Off`. Maintain split diagnostic caches (per-file vs
  cross-doc) and merge on publish. Wire into `didChange` and `didSave`
  handlers.
</Task>

<Task
  id="task-5"
  status="done"
  implements="lsp-server/req#req-2-1, lsp-server/req#req-2-2, lsp-server/req#req-2-3"
  depends="task-4"
>
  Implement `definition.rs`: detect cursor on ref string in attribute
  context, parse ref, look up in graph (fragment or document-level),
  return Location or empty. Wire into `textDocument/definition` handler.
</Task>

<Task
  id="task-6"
  status="done"
  implements="lsp-server/req#req-3-1, lsp-server/req#req-3-2, lsp-server/req#req-3-3, lsp-server/req#req-3-4"
  depends="task-4"
>
  Implement `completion.rs`: context detection (ref attribute, component
  name after `&lt;`, attribute value), document ID prefix matching, fragment
  completion within document, component name snippets from ComponentDefs,
  attribute value enumeration. Wire into `textDocument/completion` handler.
</Task>

<Task
  id="task-7"
  status="done"
  implements="lsp-server/req#req-4-1, lsp-server/req#req-4-2"
  depends="task-4"
>
  Implement `hover.rs`: component name lookup in ComponentDefs (format as
  Markdown attribute table), ref lookup in graph (document title, criterion
  body, verification status). Wire into `textDocument/hover` handler.
</Task>

<Task
  id="task-8"
  status="done"
  implements="lsp-server/req#req-6-1"
  depends="task-4"
>
  Implement the `supersigil.verify` command flow in the LSP state command
  layer (`commands.rs`, with `workspace/executeCommand` wiring in
  `state.rs`): run the verify pipeline and publish diagnostics.
</Task>

<Task
  id="task-9"
  status="done"
  implements="lsp-server/req#req-7-1, lsp-server/req#req-7-2, lsp-server/req#req-7-3, lsp-server/req#req-8-2"
  depends="task-2"
>
  Markdown integration and capability handling: editor integrations
  register or start the server for `markdown` and `mdx` documents; the
  server applies activation guard on `supersigil.toml` presence,
  fence-aware context detection, distinct CompletionItemKind and label
  detail for Supersigil items, UTF-16 position encoding advertisement.
</Task>

<Task
  id="task-10"
  status="draft"
  depends="task-9"
>
  Editor extensions and documentation. Build at least one editor extension
  (VS Code or Neovim) that activates supersigil-lsp for spec files,
  and add user-facing documentation to the website covering installation,
  configuration, and feature overview. This is the user-facing surface
  that makes the LSP server accessible. Requires its own design pass.
</Task>

```
