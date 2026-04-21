---
supersigil:
  id: lsp-server/design
  type: design
  status: approved
title: "Language Server Protocol Support"
---

```supersigil-xml
<Implements refs="lsp-server/req" />
<TrackedFiles paths="crates/supersigil-lsp/src/**/*.rs, editors/vscode/src/extension.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerSupportProvider.kt, editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilLspServerDescriptor.kt" />
```

## Overview

A new workspace crate `supersigil-lsp` implements an LSP server using
`async-lsp` 0.2.x with `lsp-types` 0.95.x. The server communicates over
stdio, reuses the existing parser, core, and verify crates, and uses
async-lsp's `&mut self` notification model for lock-free state management.

## Architecture

The server is a single long-running process per editor workspace. It owns
its state directly — no shared-memory concurrency primitives for the hot
path.

```
Editor ──stdio/JSON-RPC──▶ SupersigilLsp
                              │
                              ├── didOpen/didChange  (&mut self)
                              │     └── re-parse single file
                              │         └── publish per-file diagnostics
                              │
                              ├── didSave  (&mut self)
                              │     ├── rebuild DocumentGraph
                              │     ├── publish cross-doc diagnostics
                              │     └── run verify tier if configured
                              │
                              ├── completion/hover/definition  (snapshot Arc)
                              │     └── concurrent future on graph snapshot
                              │
                              ├── supersigil/documentList + supersigil/documentComponents
                              │     └── serve document tree and per-document detail
                              │
                              ├── supersigil/explorerSnapshot + supersigil/explorerDocument
                              │     └── serve lazy explorer shell and detail payloads
                              │
                              └── workspace/executeCommand
                                    └── verify plus mirrors for request-only surfaces
```

Notifications get `&mut self` via async-lsp's design, ensuring sequential
processing (LSP spec requirement). Request handlers snapshot
`Arc<DocumentGraph>` in the `&mut self` phase, then return a future that
runs concurrently on the snapshot.

## Key Types

```rust
struct SupersigilLsp {
    // Config
    config: ProjectConfig,
    project_root: PathBuf,

    // Document state
    open_files: HashMap<Url, String>,
    file_parses: HashMap<PathBuf, SpecDocument>,

    // Cross-document state (initialized empty, populated on first
    // successful build; retained as Last_Good_Graph on failure)
    graph: Arc<DocumentGraph>,

    // Static knowledge
    component_defs: ComponentDefs,

    // Diagnostics cache (per-URI, merged on publish)
    file_diagnostics: HashMap<Url, Vec<Diagnostic>>,
    graph_diagnostics: HashMap<Url, Vec<Diagnostic>>,
}
```

The diagnostics cache splits per-file and cross-document findings so that
`didChange` can update one half without losing the other. Publishing always
merges both halves for each URI.

### Position encoding

`SourcePosition` uses 1-based line and column (byte offset). LSP `Position`
uses 0-based line and 0-based character (UTF-16 code units).

Conversion: `line - 1` for the line, byte-to-UTF-16 scan within the line
for the character offset. The server advertises `PositionEncodingKind::UTF16`.
Since spec files are overwhelmingly ASCII, this conversion is a no-op in
practice.

### Crate layout

```
crates/supersigil-lsp/
├── Cargo.toml
├── src/
│   ├── lib.rs          # SupersigilLsp, LspService impl
│   ├── main.rs         # binary entrypoint (stdio transport)
│   ├── state.rs        # LSP surface, capability wiring, and request routing
│   ├── state/
│   │   ├── access.rs   # URI, document, and content lookup helpers
│   │   ├── commands.rs # custom request handlers and executeCommand helpers
│   │   ├── explorer.rs # explorer snapshot/detail state helpers
│   │   ├── indexing.rs # parse/graph/verify primitives
│   │   ├── lifecycle.rs # initialized/save/close/watch helper flows
│   │   └── tests.rs    # state-layer tests
│   ├── document_list.rs # supersigil/documentList request + documentsChanged notification
│   ├── document_components.rs # supersigil/documentComponents request wrapper
│   ├── explorer_runtime.rs # explorer snapshot/detail/change request wrappers
│   ├── diagnostics.rs  # Finding/ParseError → Diagnostic, tier filtering
│   ├── completion.rs   # ref, component name, attribute value completions
│   ├── definition.rs   # go-to-definition for refs
│   ├── hover.rs        # component docs, ref target preview
│   ├── commands.rs     # executeCommand names for verify, explorer, docs, and create-document
│   └── position.rs     # SourcePosition ↔ lsp_types::Position
```

### Dependencies

```toml
[dependencies]
async-lsp = { version = "0.2", features = ["omni-trait", "stdio"] }
lsp-types = "0.95"
tokio = { version = "1", features = ["rt", "macros"] }
supersigil-core = { path = "../supersigil-core" }
supersigil-parser = { path = "../supersigil-parser" }
supersigil-verify = { path = "../supersigil-verify" }
serde = { workspace = true }
serde_json = { workspace = true }
```

## Hybrid Re-indexing Detail

**didOpen**: Initialize `open_files` entry from `TextDocumentItem.text`.
Re-parse via `parse_content`. Update `file_diagnostics`. Publish merged.

**didChange**: Update `open_files` buffer. Re-parse from buffer. Update
`file_diagnostics`. Publish merged. Does NOT rebuild graph.

**didSave**: For saved files that belong to the Supersigil project, attempt
`DocumentGraph` rebuild from all `file_parses`. On success: replace `graph`
Arc. On failure: retain last-good graph, convert `GraphError`s to diagnostics.
Update `graph_diagnostics`. If tier is Verify: run verify pipeline with real
evidence (VerifyInputs). Publish merged. Saves for unrelated Markdown files are
ignored.

**didClose**: Remove from `open_files`. Clear buffer-specific
`file_diagnostics` for that URI, keep graph diagnostics if they still
apply, and publish merged diagnostics for the URI.

**didChangeWatchedFiles**: On `supersigil.toml` change: reload config,
re-discover files, full rebuild. On `.md` create/delete outside editor:
update file set, clear deleted-file buffer diagnostics, rebuild graph.

## Feature Implementation Detail

### Diagnostics

Source mapping:

| Existing type | LSP diagnostic |
|---|---|
| `ParseError::UnclosedFrontMatter` | ERROR at frontmatter position |
| `ParseError::XmlSyntaxError` | ERROR at syntax error position |
| `ParseError::MissingRequiredAttribute` | ERROR at component position |
| `ParseError::ExpressionAttribute` | ERROR at attribute position |
| `GraphError::BrokenRef` | ERROR at ref source position |
| `GraphError::DuplicateId` | ERROR at document position |
| `GraphError::Cycle` | ERROR at DependsOn position |
| `Finding { severity: Error }` | ERROR at finding location |
| `Finding { severity: Warning }` | WARNING at finding location |
| `Finding { severity: Info }` | HINT at finding location |
| `Finding { severity: Off }` | excluded |

### Go-to-definition

1. Extract word/string at cursor position within the open file buffer
2. Determine if cursor is inside a ref-accepting attribute (`refs`,
   `implements`, `depends`, or `implements` on Task)
3. If ref contains `#`: split via `split_criterion_ref`, look up component
   in graph, return its `SourcePosition` as `Location`
4. If no `#`: look up document in graph, return file start as `Location`
5. If target not found: return empty response

### Autocomplete

Context detection by scanning backward from cursor:

- Inside `refs="..."` / `implements="..."` / `depends="..."`:
  - Before `#` or no `#` yet → document ID completions from graph
  - After `doc-id#` → referenceable component IDs within that document
- After `<` at line start or after whitespace → component name completions
  from `ComponentDefs`, with snippet inserts for required attributes
- Inside `strategy="..."` or `status="..."` → context-sensitive attribute
  value completions (see below)

#### Attribute value context resolution

For `status` and `strategy`, the completion engine determines the
enclosing context by scanning backward from the cursor to find which
component (or frontmatter) the attribute belongs to:

| Enclosing context | Attribute | Completions |
|---|---|---|
| YAML frontmatter | `status` | Valid statuses from the document type definition in config (e.g. `draft`, `review`, `approved`, `implemented` for `requirements`) |
| `<Task>` | `status` | `draft`, `ready`, `in-progress`, `done` |
| `<Alternative>` | `status` | `rejected`, `deferred`, `superseded` |
| `<VerifiedBy>` | `strategy` | `tag`, `file-glob` |
| Other / unknown | any | *(none)* |

The enclosing component is detected by scanning backward from the cursor
for the nearest unmatched `<ComponentName` token. If the `status`
attribute is in the YAML frontmatter (before the closing `---`), the
document type is read from the parsed `SpecDocument` to look up valid
statuses from the config's `documents.types` section.

The `complete` function receives the config (for document type
definitions) and the current file's parsed document type (for
frontmatter status lookups).

Each `CompletionItem` includes:
- `label`: the ID or component name
- `detail`: document title, component description, or body preview
- `kind`: `CompletionItemKind::REFERENCE` for refs,
  `CompletionItemKind::CLASS` for components
- `insert_text` / `insert_text_format`: snippet for components

### Hover

**Component hover**: Look up component name in `ComponentDefs`. Format as
Markdown: description, attribute table (name, required, list), flags
(referenceable, verifiable).

**Ref hover**: Parse ref, look up in graph. Format as Markdown: document
title, document type and status, criterion body text (if fragment ref),
verification evidence summary (if available from last verify run).

### Custom requests

**`supersigil/documentList`**: Returns the current flat document list for tree
views and document pickers. Takes no parameters and derives project-relative
paths plus document metadata from the loaded `DocumentGraph`.

**`supersigil/documentComponents`**: Returns a rendered component tree for one
document, including verification-aware detail panel data. Takes the target URI
and resolves it through the current parse caches.

**`supersigil/explorerSnapshot`**: Returns the lazy graph explorer shell
payload for the current revision. Takes no parameters and snapshots the current
explorer state.

**`supersigil/explorerDocument`**: Returns lazy detail-panel payload for one
document in the explorer runtime. Takes document ID plus revision and resolves
detail from current or partial parses.

### Custom commands

`supersigil.verify` runs the full verify pipeline and publishes all findings
as diagnostics.

`supersigil.documentList`, `supersigil.documentComponents`,
`supersigil.explorerSnapshot`, and `supersigil.explorerDocument` mirror the
custom requests above via `workspace/executeCommand` for clients that cannot
send custom JSON-RPC requests.

`supersigil.createDocument` drives the interactive create-document flow used by
editor quick fixes when the target project must be chosen at runtime.

## Markdown Integration

Editor integrations register or start the Supersigil language server for both
`markdown` and `mdx` documents. Multiple LSP servers per language ID is
well-supported in VS Code, Neovim (nvim-lspconfig), and Zed. Fence-aware context detection
(`is_in_supersigil_fence`) scopes all interactive features (completions,
hover, definition) to `supersigil-xml` fenced blocks and YAML frontmatter,
so the server does not interfere with general Markdown editing.

Collision minimization:
- Component completions use `CompletionItemKind::CLASS` with "Supersigil"
  label detail
- Server is a no-op without `supersigil.toml` — starts with empty
  capabilities, watches for config creation
- Diagnostics are strictly Supersigil-specific; never duplicates Markdown
  syntax errors
- Features only trigger inside `supersigil-xml` fences, returning empty
  results for positions in regular Markdown content

## Prerequisite Changes

### supersigil-parser: in-memory parsing

Add `parse_content(path: &Path, content: &str, defs: &ComponentDefs) ->
ParseResult`. Refactor `parse_file` to read from disk then delegate to
`parse_content`. No behavior change for existing callers.

## Error Handling

- Parse errors in individual files: publish as diagnostics, continue serving
- Graph build failure: retain Last_Good_Graph, publish GraphErrors as
  diagnostics, log warning
- Config reload failure: retain previous config, publish
  `window/showMessage` warning
- Verify pipeline failure: publish available findings, log error, do not
  crash
- Invalid `didChangeConfiguration` payload: ignore with
  `window/showMessage` warning

The server should never crash on malformed input. All error paths publish
diagnostics or user-visible messages rather than panicking.

## Testing Strategy

- **Unit tests**: Each feature module tested with constructed state and
  synthetic `SpecDocument`s. Verify correct completions, hover content,
  definition locations, and diagnostic output for known inputs.
- **Integration tests**: Full LSP message exchange over stdio. Send
  `initialize` → `didOpen` → `didChange` → `completion` / `hover` /
  `definition` sequences and assert JSON-RPC responses.
- **Snapshot tests**: Diagnostic output for known-broken spec files compared
  against expected diagnostics (rule name, severity, position).
- **Last-good-graph tests**: Verify that introducing a graph error (broken
  ref) retains the previous graph and that go-to-definition still works on
  stale data.

## Alternatives Considered

See `lsp-server/adr` for the full decision record covering framework choice,
crate structure, re-indexing strategy, Markdown integration, and diagnostics
tiers.
