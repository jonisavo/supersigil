---
supersigil:
  id: spec-explorer/design
  type: design
  status: approved
title: "Spec Explorer Tree View"
---

```supersigil-xml
<Implements refs="spec-explorer/req" />
<TrackedFiles paths="crates/supersigil-lsp/src/**/*.rs, editors/vscode/src/**/*.ts, editors/vscode/package.json" />
```

## Overview

Two-sided feature: a custom LSP request/notification pair on the Rust
server side, and a `TreeDataProvider` on the VS Code extension side.
The LSP serves document metadata from its in-memory graph; the
extension groups, renders, and navigates.

## LSP Side

### Custom Protocol Types

New module `crates/supersigil-lsp/src/document_list.rs` defining
the custom request and notification types using `lsp_types` traits.

```rust
use lsp_types::{notification::Notification, request::Request};
use serde::{Deserialize, Serialize};

// -- Request: supersigil/documentList --

pub struct DocumentListRequest;

impl Request for DocumentListRequest {
    type Params = DocumentListParams;
    type Result = DocumentListResult;
    const METHOD: &'static str = "supersigil/documentList";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentListParams {}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentListResult {
    pub documents: Vec<DocumentEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentEntry {
    pub id: String,
    pub doc_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

// -- Notification: supersigil/documentsChanged --

pub struct DocumentsChanged;

impl Notification for DocumentsChanged {
    type Params = ();
    const METHOD: &'static str = "supersigil/documentsChanged";
}
```

### Request Handler Registration

The `async_lsp::router::Router` supports custom request handlers via
`router.request::<R, _>(handler)` in addition to the standard
`LanguageServer` trait methods. Modify `SupersigilLsp::new_router()`
to register the custom handler after `from_language_server()`:

```rust
pub fn new_router(client: ClientSocket) -> Router<Self, ResponseError> {
    let mut router = Router::from_language_server(Self { /* ... */ });
    router.request::<DocumentListRequest, _>(Self::handle_document_list);
    router
}
```

### Request Handler Implementation

The handler reads from the existing `file_parses` map and `config`.
It follows the existing `&mut self` → clone-and-move pattern but
since the response is synchronous (no I/O), it can resolve immediately:

```rust
fn handle_document_list(
    &mut self,
    _params: DocumentListParams,
) -> BoxFuture<'static, Result<DocumentListResult, ResponseError>> {
    let documents = self.build_document_entries();
    Box::pin(async move { Ok(DocumentListResult { documents }) })
}
```

The `build_document_entries()` helper iterates
`self.graph.documents()`, extracting frontmatter fields. The `path`
field is made relative to `self.project_root`. The `project` field
comes from the config's project mapping (match document path against
each project's glob patterns to determine membership).

### Notification Injection Points

Send `supersigil/documentsChanged` via `self.client.notify::<DocumentsChanged>(())`
at the end of every graph rebuild. Three existing code paths:

1. **`initialized()`** — after initial indexing completes and
   `republish_all_diagnostics()` returns.
2. **`did_save()`** — after the graph rebuild and diagnostic
   republishing cycle.
3. **`did_change_watched_files()`** — after processing file system
   events and rebuilding the graph.

Extract a helper to avoid triplication:

```rust
fn notify_documents_changed(&self) {
    let _ = self.client.notify::<DocumentsChanged>(());
}
```

Call it after `republish_all_diagnostics()` in each of the three
paths.

## Extension Side

### New File: `specExplorer.ts`

A new module alongside `extension.ts` containing the tree data
provider and supporting types.

### Tree Item Types

```typescript
type SpecTreeItem = WorkspaceRootItem | ProjectItem | GroupItem | DocumentItem;

interface WorkspaceRootItem {
  kind: "workspace-root";
  label: string;           // workspace folder name
  folderUri: vscode.Uri;
}

interface ProjectItem {
  kind: "project";
  label: string;           // project name from config
  folderUri: vscode.Uri;
}

interface GroupItem {
  kind: "group";
  label: string;           // prefix (e.g. "document-graph")
  folderUri: vscode.Uri;
  documentCount: number;
  project: string | null;
}

interface DocumentItem {
  kind: "document";
  id: string;
  docType: string;
  status: string | null;
  path: string;            // relative to project root
  folderUri: vscode.Uri;
  project: string | null;
}
```

### `SpecExplorerProvider` Class

Implements `vscode.TreeDataProvider<SpecTreeItem>`.

**Data fetching:** `getChildren(element?)` sends
`supersigil/documentList` to the relevant client(s). At the root
level with multiple workspace roots, it queries all clients. For
deeper levels, it filters by the parent's folder/project/prefix.

**Grouping logic:**
1. If multiple workspace roots have active clients → top level is
   `WorkspaceRootItem` nodes.
2. Within a root, if any document has a non-null `project` → insert
   `ProjectItem` nodes.
3. Within a project (or root if single-project), group documents by
   prefix. Documents sharing a prefix before the first `/` → one
   `GroupItem`. Documents with no `/` in ID → direct children.
4. `DocumentItem` nodes are always leaves.

**Icon and color assignment:** `getTreeItem(element)` returns a
`vscode.TreeItem` with:
- Document nodes: codicon from doc_type mapping, `ThemeColor` from
  status mapping, description as `docType · status`. Diagnostic
  override checked via `vscode.languages.getDiagnostics(uri)`.
- Group nodes: `$(folder)` icon, description `N documents`.
- Project nodes: `$(tag)` icon.
- Workspace root nodes: `$(root-folder)` icon.

**Click-to-open:** Document nodes set
`command: { command: 'vscode.open', arguments: [fileUri] }` on the
`TreeItem`.

### Refresh Triggers

The provider exposes an `onDidChangeTreeData` event. It fires when:

1. A `supersigil/documentsChanged` notification arrives from any
   client — registered via `client.onNotification('supersigil/documentsChanged', handler)`.
2. `vscode.languages.onDidChangeDiagnostics` fires with URIs that
   match known document paths (filtered to `supersigil` source) —
   this only fires `onDidChangeTreeData` for the affected items, not
   a full refresh.

### Lifecycle Management

The provider is created in `activate()` and registered with
`vscode.window.registerTreeDataProvider('supersigil.specExplorer', provider)`.
It hooks into the existing client lifecycle:

- When `startClientForFolder()` creates a new client, it registers
  the `documentsChanged` notification handler on that client and
  calls `provider.refresh()`.
- When a client is stopped (workspace folder removed), the handler
  is automatically cleaned up with the client, and the provider
  refreshes.

### Registration in `package.json`

```jsonc
{
  "viewsContainers": {
    "activitybar": [{
      "id": "supersigil",
      "title": "Supersigil",
      "icon": "icon.svg"
    }]
  },
  "views": {
    "supersigil": [{
      "id": "supersigil.specExplorer",
      "name": "Spec Explorer"
    }]
  },
  "viewsWelcome": [{
    "view": "supersigil.specExplorer",
    "contents": "No supersigil project found.\n[Initialize Project](command:supersigil.init)",
    "when": "supersigil.noRoots"
  }],
  "commands": [{
    "command": "supersigil.init",
    "title": "Supersigil: Initialize Project"
  }]
}
```

The `supersigil.init` command opens a terminal and runs
`supersigil init`. The `supersigil.noRoots` context key is set by
the extension when no LSP clients are active.

The `icon.svg` is a simplified monochrome version of the extension
icon, suitable for the activity bar.

## Testing Strategy

**LSP side:**
- Unit test in `document_list.rs`: build a `DocumentGraph` from
  known `SpecDocument` values, call `build_document_entries()`,
  assert the returned list matches expected IDs, types, statuses,
  paths, and project assignments.
- Test both single-project and multi-project configurations.
- Test empty graph returns empty list.

**Extension side:**
- The `SpecExplorerProvider` grouping logic is pure data
  transformation. Test with mock `DocumentEntry[]` arrays and assert
  the tree structure (group names, nesting, document counts).
- Icon and color assignment: test each doc_type and status
  combination returns the expected codicon and `ThemeColor`.
- No VS Code integration tests in v1 — the provider logic is
  testable in isolation.

## Decisions

```supersigil-xml
<Decision id="data-source">
  Use a custom LSP request rather than shelling out to the CLI.

  <References refs="spec-explorer/req#req-1-1" />

  <Rationale>
  The LSP already has the full DocumentGraph in memory. A custom
  request avoids process spawning latency, a separate binary
  dependency, and keeps the tree data consistent with the diagnostics
  the LSP publishes.
  </Rationale>

  <Alternative id="cli-shell-out" status="rejected">
    Shell out to `supersigil ls --format json` from the extension.
    Simpler on the LSP side but introduces latency, requires locating
    the CLI binary separately, and creates a consistency gap between
    tree data and LSP diagnostics.
  </Alternative>
</Decision>

<Decision id="refresh-strategy">
  Use an LSP notification (push) rather than polling or event-based refresh.

  <References refs="spec-explorer/req#req-5-1" />

  <Rationale>
  The LSP already knows when the graph changes. Pushing a notification
  keeps the tree always in sync without polling overhead or missed
  changes from outside VS Code (git operations, CLI runs).
  </Rationale>

  <Alternative id="event-polling" status="rejected">
    Refresh on VS Code file events (save, active editor change).
    No LSP changes needed but misses external changes and may refresh
    too eagerly on irrelevant saves.
  </Alternative>
</Decision>

<Decision id="grouping-strategy">
  Group documents by ID prefix rather than by document type.

  <References refs="spec-explorer/req#req-2-1" />

  <Rationale>
  The primary goal is navigation. Grouping by prefix keeps related
  specs together (e.g. document-graph/req, document-graph/design,
  document-graph/tasks). Grouping by type would scatter related specs
  across separate branches. Document type is still visible as inline
  description text on each node.
  </Rationale>

  <Alternative id="group-by-type" status="rejected">
    Group by document type (requirements, design, tasks) at the top
    level. Natural for status dashboards but fragments navigation —
    jumping between a feature's requirement and design requires
    expanding two different branches.
  </Alternative>
</Decision>

<Decision id="no-coverage-v1">
  Defer coverage data on document nodes to a future iteration.

  <References refs="spec-explorer/req#req-4-4" />

  <Rationale>
  Coverage requires the LSP to include verification results in the
  response, which is heavier and couples the tree view to the verify
  pipeline. Starting without it keeps the v1 scope tight and the LSP
  response lightweight. Coverage can be added later by extending
  DocumentEntry without breaking the existing protocol.
  </Rationale>

  <Alternative id="coverage-in-v1" status="rejected">
    Include criteria counts and coverage percentages per document from
    day one. Adds complexity to the LSP response and ties tree refresh
    to verification completion, which can be slow.
  </Alternative>
</Decision>
```
