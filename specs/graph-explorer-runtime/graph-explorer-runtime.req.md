---
supersigil:
  id: graph-explorer-runtime/req
  type: requirements
  status: implemented
title: "Graph Explorer Runtime"
---

## Introduction

Define a transport-driven runtime for the editor-hosted and standalone website
graph explorer. The runtime uses one shared stateful application that renders
from a lightweight snapshot, hydrates document detail lazily, and reacts to
revisioned change notifications from the LSP when the host provides them.

Scope: the explorer snapshot/detail/change contract, the shared explorer
runtime and cache model, VS Code and IntelliJ transport adapters, standalone
website explorer integration through the same runtime, and retirement of the
older `graphData` + `renderData` integration path.

Out of scope: new graph-analysis features, CLI-specific surfaces, and
editor-specific UX features unrelated to explorer data loading.

```supersigil-xml
<References refs="graph-explorer/req, spec-rendering/req, vscode-explorer-webview/req, intellij-graph-explorer/req, lsp-server/req" />
```

## Definitions

- **ExplorerSnapshot**: The summary payload used for first paint. Contains the
  document graph, per-document coverage summaries, and only the component
  outline needed by the graph shell.
- **ExplorerDocument**: The per-document detail payload used by the detail
  panel. Contains fenced component trees, evidence-backed verification detail,
  and document-level edges.
- **ExplorerChangedEvent**: A revisioned notification from the LSP containing
  the new explorer revision plus changed and removed document IDs.
- **ExplorerTransport**: The host adapter interface implemented by VS Code,
  IntelliJ, and the standalone website explorer. Provides initial context,
  snapshot loading, document loading, change subscription, and optional file
  opening.
- **ExplorerApp**: The shared long-lived controller created once per panel or
  tool window. Owns snapshot state, URL state, hydration cache, and UI updates.
- **Revision**: An opaque string that identifies one coherent explorer data
  snapshot. Cached document detail is valid only for the revision that produced
  it.
- **Graph Component Outline**: The graph-visible component summary carried in
  `ExplorerSnapshot`. Includes enough information for node sizing, component
  drilldown, and cluster summaries without carrying fenced render trees.

## Requirement 1: Revisioned Explorer Data Contract

As an editor-hosted explorer runtime, I want a dedicated revisioned data
contract, so that the graph shell, document detail, and change notifications
share one consistent model across hosts.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE LSP SHALL expose an `ExplorerSnapshot` request for the graph explorer
    runtime. VS Code SHALL consume it through a custom request and IntelliJ
    SHALL consume an equivalent execute-command mirror.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/explorer_runtime.rs, crates/supersigil-lsp/src/state.rs, crates/supersigil-lsp/src/state/**/*.rs, crates/supersigil-lsp/tests/explorer_runtime_contract_tests.rs, editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
  <Criterion id="req-1-2">
    THE LSP SHALL expose an `ExplorerDocument` request that accepts a document
    ID and revision and returns detail for exactly one document.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/explorer_runtime.rs, crates/supersigil-lsp/src/state.rs, crates/supersigil-lsp/src/state/**/*.rs, crates/supersigil-lsp/tests/explorer_runtime_contract_tests.rs, crates/supersigil-verify/tests/explorer_runtime_tests.rs" />
  </Criterion>
  <Criterion id="req-1-3">
    `ExplorerSnapshot` SHALL include `revision`, document summaries, graph
    edges, per-document coverage summaries, and graph component outlines. It
    SHALL NOT require a full `renderData` batch before first paint.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/explorer_runtime.rs, crates/supersigil-verify/tests/explorer_runtime_tests.rs" />
  </Criterion>
  <Criterion id="req-1-4">
    THE LSP SHALL emit an `ExplorerChangedEvent` containing `revision`,
    `changed_document_ids`, and `removed_document_ids`, so the runtime can
    invalidate selectively instead of treating all changes as a full refresh.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/explorer_runtime.rs, crates/supersigil-lsp/src/state.rs, crates/supersigil-lsp/src/state/**/*.rs, crates/supersigil-verify/src/explorer_runtime.rs, editors/vscode/src/explorerWebview.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Shared Stateful Explorer Runtime

As a maintainer, I want the shared explorer to own runtime state itself, so
that VS Code, IntelliJ, and the standalone website explorer use the same
loading, navigation, and update model.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE shared explorer SHALL expose a stateful application entry point that is
    created once per host container and destroyed when the host panel closes.
    It SHALL replace the current one-shot `mount(container, data, renderData,
    ...)` integration contract for editor-hosted explorer panels.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, website/src/components/explore/explorer-app.test.js, website/src/components/explore/explorer-entry.js" />
  </Criterion>
  <Criterion id="req-2-2">
    THE shared runtime SHALL own the root selector, URL/hash state, detail
    panel rendering, loading and updating indicators, and the "Open File"
    control. Hosts SHALL NOT inject those UI elements after render, and the
    runtime SHALL suppress editor-only controls when the active transport does
    not expose file-opening capability.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/graph-explorer.js, website/src/components/explore/detail-panel.js, website/src/components/explore/graph-explorer-mount.test.js, website/src/components/explore/standalone-explorer.test.js" />
  </Criterion>
  <Criterion id="req-2-3">
    VS Code, IntelliJ, and the standalone website explorer SHALL integrate
    through an `ExplorerTransport` interface that provides initial context,
    snapshot loading, document loading, change subscription, and optional file
    opening. Hosts SHALL NOT assemble explorer batches or remount the shared
    explorer to apply normal data updates.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, editors/vscode/src/explorerBootstrap.ts, editors/vscode/src/explorerWebview.ts, editors/intellij/src/main/resources/supersigil-explorer/explorer-bridge.js, website/src/components/explore/standalone-explorer.js" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Lazy Hydration and Revision-Scoped Caching

As a spec author opening the graph explorer, I want first paint to depend only
on snapshot data and document detail to load on demand, so that large
workspaces do not pay full hydration cost before the graph appears.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    ON initial load, THE shared runtime SHALL render the graph shell from
    `ExplorerSnapshot` without waiting for document detail for every document in
    the workspace.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, website/src/components/explore/explorer-app.test.js" />
  </Criterion>
  <Criterion id="req-3-2">
    WHEN the selected document is requested by click, search, or URL hash, THE
    shared runtime SHALL load `ExplorerDocument` only for the selected
    document when that detail is not already cached for the active revision.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, website/src/components/explore/explorer-app.test.js" />
  </Criterion>
  <Criterion id="req-3-3">
    THE detail cache SHALL be keyed by `(revision, document_id)`, SHALL
    coalesce duplicate in-flight requests, and SHALL discard results that do
    not match the runtime's active revision when they complete.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, website/src/components/explore/explorer-app.test.js" />
  </Criterion>
  <Criterion id="req-3-4">
    THE runtime MAY prefetch document detail in the background, but interactive
    document loads SHALL take priority over background prefetch work and
    prefetch concurrency SHALL be bounded.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, website/src/components/explore/explorer-app.test.js" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Change Handling and Root Switching

As a spec author working in a live editor, I want the explorer to preserve my
context while reacting precisely to data changes, so that updates feel stable
instead of forcing full remounts.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    WHEN the runtime receives an `ExplorerChangedEvent`, IT SHALL reload the
    latest `ExplorerSnapshot`, selectively invalidate changed and removed
    document detail entries, and update the rendered graph shell without a
    host-driven remount.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, website/src/components/explore/explorer-app.test.js" />
  </Criterion>
  <Criterion id="req-4-2">
    WHEN the currently selected document remains present but changed, THE
    runtime SHALL preserve the selection, show an updating state in the detail
    panel, and replace the visible detail when the new `ExplorerDocument`
    payload arrives.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, website/src/components/explore/explorer-app.test.js, website/src/components/explore/detail-panel.js" />
  </Criterion>
  <Criterion id="req-4-3">
    WHEN the currently selected document is removed, THE runtime SHALL clear
    the selection, normalize the URL hash to the index view, and remove stale
    detail from the cache.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, website/src/components/explore/explorer-app.test.js" />
  </Criterion>
  <Criterion id="req-4-4">
    Root switching SHALL replace the active snapshot, reset revision-scoped
    detail cache state, and remain owned by the shared runtime rather than a
    host-specific remount protocol.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/explorer-app.js, website/src/components/explore/explorer-app.test.js, website/src/components/explore/graph-explorer-mount.test.js" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Snapshot-Oriented Graph Shell Data

As the shared graph shell, I want only the graph-visible document summary data
in the initial snapshot, so that the first paint payload is smaller without
regressing current graph interactions.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE graph explorer runtime SHALL stop using `GraphJson` as its primary
    first-paint payload. `ExplorerSnapshot` SHALL be a dedicated schema for the
    runtime.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/explorer_runtime.rs, crates/supersigil-lsp/src/explorer_runtime.rs, crates/supersigil-verify/tests/explorer_runtime_tests.rs" />
  </Criterion>
  <Criterion id="req-5-2">
    Each snapshot document summary SHALL include the existing graph shell
    fields (`id`, `title`, `doc_type`, `status`, `path`, `file_uri`, and
    `project`) plus `coverage_summary`, `component_count`, and a graph
    component outline.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/explorer_runtime.rs, crates/supersigil-verify/tests/explorer_runtime_tests.rs" />
  </Criterion>
  <Criterion id="req-5-3">
    The graph component outline SHALL include only the component data needed by
    the graph shell for node sizing, component drilldown, and cluster summaries
    (`id`, `kind`, `body`, `parent_component_id`, and `implements` when
    relevant). Full fenced component trees and evidence detail SHALL remain in
    `ExplorerDocument`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/explorer_runtime.rs, crates/supersigil-verify/tests/explorer_runtime_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Host Cutover, Website Adoption, and Legacy Contract Retirement

As a maintainer, I want the old batch-hydration integration path removed, so
that there is one canonical explorer runtime architecture across editors and
the standalone website explorer instead of multiple parallel models.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE VS Code explorer host SHALL retire full-document batch hydration,
    host-injected root selector logic, host-injected "Open File" controls, and
    remount-based update choreography in favor of the shared runtime and
    transport contract.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/explorerBootstrap.ts, editors/vscode/src/explorerWebview.ts, editors/vscode/src/explorerWebview.test.ts" />
  </Criterion>
  <Criterion id="req-6-2">
    THE IntelliJ explorer host SHALL retire full-document batch hydration,
    bridge-owned remount logic, DOM observers for host UI injection, and
    payload assembly that depends on preloaded `renderData` batches.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/supersigil-explorer/explorer-bridge.js, editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt, editors/intellij/src/test/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactoryTest.kt" />
  </Criterion>
  <Criterion id="req-6-3">
    THE standalone website explorer SHALL use the same shared runtime through a
    website-specific `ExplorerTransport` that can serve snapshot and document
    detail without reintroducing one-shot mount semantics.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/standalone-explorer.js, website/src/components/explore/standalone-explorer.test.js, website/src/pages/explore.astro" />
  </Criterion>
  <Criterion id="req-6-4">
    THE editor explorer runtime SHALL retire its use of
    `supersigil/graphData`, graph-explorer use of the old
    `supersigil/documentComponents` contract, and the bare
    `supersigil/documentsChanged` refresh path for explorer panels.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/commands.rs, crates/supersigil-lsp/src/state.rs, crates/supersigil-lsp/src/state/**/*.rs, editors/vscode/src/explorerWebview.test.ts, editors/intellij/src/main/kotlin/org/supersigil/intellij/GraphExplorerToolWindowFactory.kt" />
  </Criterion>
</AcceptanceCriteria>
```
