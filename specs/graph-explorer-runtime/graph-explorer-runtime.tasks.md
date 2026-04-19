---
supersigil:
  id: graph-explorer-runtime/tasks
  type: tasks
  status: done
title: "Graph Explorer Runtime"
---

```supersigil-xml
<DependsOn refs="graph-explorer-runtime/design" />
```

## Overview

Replace the current explorer integration in one pass: define the new
snapshot/detail/change contract in the LSP, build the shared stateful runtime,
rewire VS Code and IntelliJ to thin transports first, adopt the standalone
website explorer second, then remove the legacy batch hydration and remount
path.

```supersigil-xml
<Task id="task-1" status="done" implements="graph-explorer-runtime/req#req-1-1, graph-explorer-runtime/req#req-1-2, graph-explorer-runtime/req#req-1-3, graph-explorer-runtime/req#req-1-4, graph-explorer-runtime/req#req-5-1, graph-explorer-runtime/req#req-5-2, graph-explorer-runtime/req#req-5-3">
  Define the explorer runtime server contract in `supersigil-verify` and
  `supersigil-lsp`: `ExplorerSnapshot`, `ExplorerDocument`, and
  `ExplorerChangedEvent`, including revision generation, snapshot summaries,
  graph component outlines, and command mirrors for IntelliJ.
</Task>
```

```supersigil-xml
<Task id="task-2" status="done" depends="task-1" implements="graph-explorer-runtime/req#req-2-1, graph-explorer-runtime/req#req-2-2, graph-explorer-runtime/req#req-2-3, graph-explorer-runtime/req#req-3-1, graph-explorer-runtime/req#req-3-2, graph-explorer-runtime/req#req-3-3, graph-explorer-runtime/req#req-3-4, graph-explorer-runtime/req#req-4-1, graph-explorer-runtime/req#req-4-2, graph-explorer-runtime/req#req-4-3, graph-explorer-runtime/req#req-4-4">
  Build the shared stateful explorer runtime around
  `createExplorerApp(container, transport)`, including snapshot store,
  revision-keyed detail cache, lazy document hydration, root switching, and
  change-event handling without host-driven remounts. Support both live editor
  transports and eager browser-side transports without forking the runtime
  state model.
</Task>
```

```supersigil-xml
<Task id="task-3" status="done" depends="task-2" implements="graph-explorer-runtime/req#req-2-2, graph-explorer-runtime/req#req-5-2, graph-explorer-runtime/req#req-5-3">
  Move graph-shell-owned UI responsibilities into the shared runtime: root
  selector rendering, loading and updating states, and the detail-panel
  "Open File" action. Gate editor-only controls on transport capabilities, and
  remove host-side DOM observers and post-render UI injection from the shared
  explorer integration surface.
</Task>
```

```supersigil-xml
<Task id="task-4" status="done" depends="task-1, task-2, task-3" implements="graph-explorer-runtime/req#req-2-3, graph-explorer-runtime/req#req-6-1">
  Rewire the VS Code explorer panel to implement `ExplorerTransport` over the
  new LSP requests and file-open command bridge. Delete full-document batch
  hydration, render-data assembly, host-owned root selector logic, and
  remount-based refresh behavior.
</Task>
```

```supersigil-xml
<Task id="task-5" status="done" depends="task-1, task-2, task-3" implements="graph-explorer-runtime/req#req-2-3, graph-explorer-runtime/req#req-6-2">
  Rewire the IntelliJ graph explorer to implement the same transport contract
  over execute-command requests and the existing action bridge. Delete payload
  assembly, batch hydration, remount logic, and bridge-owned UI injection.
</Task>
```

```supersigil-xml
<Task id="task-6" status="done" depends="task-4, task-5" implements="graph-explorer-runtime/req#req-4-1, graph-explorer-runtime/req#req-4-2, graph-explorer-runtime/req#req-4-3, graph-explorer-runtime/req#req-4-4">
  Wire revisioned change handling end to end, including the
  `ExplorerChangedEvent` notification payload, selective cache invalidation,
  selection preservation, removed-document cleanup, and root-switch snapshot
  replacement behavior across both hosts.
</Task>
```

```supersigil-xml
<Task id="task-7" status="done" depends="task-4, task-5" implements="graph-explorer-runtime/req#req-2-3, graph-explorer-runtime/req#req-6-3">
  Adopt the standalone website explorer on top of the same
  `createExplorerApp(container, transport)` entry point using a website
  transport that serves snapshot and document detail without reintroducing a
  one-shot mount path. Preserve shared UI behavior while suppressing
  editor-only controls that the website host does not support.
</Task>
```

```supersigil-xml
<Task id="task-8" status="done" depends="task-4, task-5, task-6, task-7" implements="graph-explorer-runtime/req#req-6-1, graph-explorer-runtime/req#req-6-2, graph-explorer-runtime/req#req-6-4">
  Remove legacy explorer contracts and tests that assume `graphData`,
  workspace-wide `renderData` batches, bare `documentsChanged` refreshes,
  host-injected controls, or remount-based updates. Replace them with tests
  for the new runtime, transport adapters, and revisioned LSP payloads.
</Task>
```
