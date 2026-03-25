---
supersigil:
  id: spec-explorer/req
  type: requirements
  status: implemented
title: "Spec Explorer Tree View"
---

## Introduction

A sidebar tree view in the VS Code extension that shows spec documents
grouped by feature area. Primary goal is fast navigation to spec files;
secondary goal is at-a-glance status visibility. Complements the file
explorer by presenting documents in their logical structure (by prefix
and type) rather than filesystem layout.

Scope: custom LSP request and notification for document listing, a
`TreeDataProvider` in the extension, and activity bar registration.

Out of scope: coverage data on nodes, relationship edges between
documents (deferred to graph explorer integration), context menu
actions, and custom filtering/sorting controls.

## Definitions

- **Prefix group**: Documents sharing an ID prefix before the first `/`
  (e.g. `document-graph/req` and `document-graph/design` share the
  prefix `document-graph`).
- **Document node**: A leaf tree item representing a single spec
  document.
- **Group node**: A collapsible tree item representing a prefix group.
- **Project node**: A collapsible tree item representing a supersigil
  project in multi-project mode.

## Requirement 1: Document List Data

As an editor extension, I need the LSP server to provide the document
list from its in-memory graph, so that the tree view can display spec
documents without shelling out to the CLI.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE LSP server SHALL handle a custom `supersigil/documentList`
    request and return a flat list of documents from the loaded
    `DocumentGraph`. Each entry SHALL include the document ID, document
    type, status (nullable), file path relative to the project root,
    and project name (nullable, for multi-project mode).
  </Criterion>
  <Criterion id="req-1-2">
    WHEN the document graph is empty (no spec files found), THE server
    SHALL return an empty document list, not an error.
  </Criterion>
  <Criterion id="req-1-3">
    THE LSP server SHALL send a `supersigil/documentsChanged`
    notification (no payload) to the client after every re-index
    (file change, config change, initial indexing).
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/state.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Tree Hierarchy

As a spec author, I want documents grouped by feature area so that I
can find related specs quickly without scanning a flat list.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE tree SHALL group documents by ID prefix. Documents sharing the
    same prefix before the first `/` SHALL appear under a collapsible
    group node labeled with that prefix.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
  <Criterion id="req-2-2">
    Documents with no `/` in their ID SHALL appear ungrouped at the
    root level (or directly under their project node in multi-project
    mode).
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
  <Criterion id="req-2-3">
    IN multi-project mode, THE tree SHALL add a project node above the
    prefix groups, one per project. In single-project mode, project
    nodes SHALL be omitted.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
  <Criterion id="req-2-4">
    IN multi-root workspaces, THE tree SHALL query all active LSP
    clients, merging results. When multiple workspace roots exist, a
    workspace root node SHALL appear above the project/group level.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Navigation

As a spec author, I want to click a document node to open its file,
so that the tree view serves as a navigation shortcut.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN the user clicks a document node, THE extension SHALL open the
    corresponding file in the editor. The file URI SHALL be resolved
    from the workspace folder root and the document's relative path.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Status Visibility

As a spec author, I want to see document type and status at a glance,
so that I can assess project state without opening each file.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    Each document node SHALL display a codicon based on the document
    type: `$(checklist)` for requirements, `$(tools)` for design,
    `$(tasklist)` for tasks, `$(law)` for adr/decision, `$(book)` for
    documentation, and `$(file)` for unknown types.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
  <Criterion id="req-4-2">
    The icon color SHALL reflect the document status: a success color
    for stable statuses (approved, implemented, done, accepted), a
    queued color for draft, a disabled color for superseded, and the
    default color when no status is set.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
  <Criterion id="req-4-3">
    WHEN a document has active LSP diagnostics (errors or warnings
    from the `supersigil` source), the icon SHALL override to
    `$(error)` or `$(warning)` with the corresponding error/warning
    color.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
  <Criterion id="req-4-4">
    Each document node SHALL show the document type and status as
    inline description text (e.g. `requirements · approved`).
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
  <Criterion id="req-4-5">
    Each group node SHALL show a folder icon and display the document
    count as description text.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Refresh Behavior

As a spec author, I want the tree to stay in sync with changes,
so that I always see the current state.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    WHEN the extension receives a `supersigil/documentsChanged`
    notification from any active LSP client, THE tree SHALL refresh
    its data by re-fetching the document list.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts" />
  </Criterion>
  <Criterion id="req-5-2">
    WHEN diagnostics change (via `onDidChangeDiagnostics`), THE tree
    SHALL update affected document node icons without a full data
    re-fetch.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/specExplorer.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Activity Bar Registration

As a VS Code user, I want the Spec Explorer to appear in the activity
bar as its own view container, so that it's always accessible.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE extension SHALL register a `supersigil` view container in the
    activity bar with a Supersigil icon and a `Spec Explorer` view.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/package.json" />
  </Criterion>
  <Criterion id="req-6-2">
    WHEN no LSP client is running (no supersigil root found), THE view
    SHALL show a welcome message with a button that runs
    `supersigil init` in the integrated terminal.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/package.json, editors/vscode/src/extension.ts" />
  </Criterion>
</AcceptanceCriteria>
```
