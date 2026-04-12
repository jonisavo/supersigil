---
supersigil:
  id: version-mismatch/tasks
  type: tasks
  status: done
title: "Version Mismatch Detection"
---

## Overview

Server-side first (one-line change + test), then extension-side
(comparison logic, warning dialog, output channel logging). TDD: write
the LSP test before changing the handler, then manually verify the
extension behavior.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="version-mismatch/req#req-1-1"
>
  Populate `ServerInfo` in the LSP server's `InitializeResult`. Set
  `name` to `"supersigil-lsp"` and `version` to
  `env!("CARGO_PKG_VERSION")`. In `state.rs`, replace
  `..InitializeResult::default()` with an explicit `server_info` field.
  Add a test asserting `server_info` is present with the expected name
  and a non-empty version string.
</Task>
```

```supersigil-xml
<Task
  id="task-2"
  status="done"
  depends="task-1"
  implements="version-mismatch/req#req-2-1, version-mismatch/req#req-2-2, version-mismatch/req#req-3-1, version-mismatch/req#req-3-2, version-mismatch/req#req-3-3, version-mismatch/req#req-3-4"
>
  In `extension.ts`, after `client.start()` in `startClientForFolder`:
  read the server version from
  `client.initializeResult?.serverInfo?.version`. If absent, skip
  silently. Compare to the extension's own version from
  `vscode.extensions.getExtension("supersigil.supersigil")`. If they
  differ and the module-level `mismatchShown` flag is false, set the
  flag, log to the output channel, and show an information message. If
  the server version is numerically newer (split on `.`, compare
  major/minor/patch), include an "Update Extension" button that runs
  `workbench.extensions.action.showExtensionsWithIds`.
</Task>
```
