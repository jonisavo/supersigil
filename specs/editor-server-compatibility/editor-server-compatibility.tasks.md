---
supersigil:
  id: editor-server-compatibility/tasks
  type: tasks
  status: done
title: "Editor/Server Compatibility"
---

```supersigil-xml
<DependsOn refs="editor-server-compatibility/design" />
```

## Overview

Implement the shared compatibility contract in four slices: server preflight
reporting first, then VS Code startup enforcement, then IntelliJ startup
enforcement, and finally retirement of the old VS Code-only version-mismatch
spec.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="editor-server-compatibility/req#req-1-1, editor-server-compatibility/req#req-1-2, editor-server-compatibility/req#req-1-3"
>
  Add `--compatibility-info` to `crates/supersigil-lsp/src/main.rs`. When
  present, print a small JSON object with `compatibility_version` and
  `server_version`, then exit without starting the LSP main loop. Add
  server-side tests for that query path.
</Task>

<Task
  id="task-2"
  status="done"
  depends="task-1"
  implements="editor-server-compatibility/req#req-2-1, editor-server-compatibility/req#req-2-3, editor-server-compatibility/req#req-3-1, editor-server-compatibility/req#req-3-2, editor-server-compatibility/req#req-3-4, editor-server-compatibility/req#req-4-1, editor-server-compatibility/req#req-4-2"
>
  Replace the VS Code exact package-version mismatch check with a
  compatibility-info preflight before `client.start()`. Compare the response to
  a local `SUPPORTED_COMPATIBILITY_VERSION` constant, show an actionable error
  on mismatch, and do not start the client instead of continuing.
</Task>

<Task
  id="task-3"
  status="done"
  depends="task-1"
  implements="editor-server-compatibility/req#req-2-2, editor-server-compatibility/req#req-2-3, editor-server-compatibility/req#req-3-1, editor-server-compatibility/req#req-3-3, editor-server-compatibility/req#req-3-4, editor-server-compatibility/req#req-4-1, editor-server-compatibility/req#req-4-2"
>
  Add the same compatibility-info preflight to the IntelliJ startup path before
  `ensureServerStarted(...)`. On mismatch, show an actionable notification with
  update/install guidance and return without starting the server.
</Task>

<Task
  id="task-4"
  status="done"
  depends="task-2, task-3"
  implements="editor-server-compatibility/req#req-4-3"
>
  Audit the current custom request and editor-visible protocol surfaces and
  document which future changes require a Compatibility_Version bump. Add this
  guidance near the new constants so future edits do not silently break the
  contract.
</Task>

<Task
  id="task-5"
  status="done"
  depends="task-2"
>
  Remove the old `editors/vscode/specs/version-mismatch/*` docs and retire the
  exact package-version warning model in the same change that lands the new
  shared compatibility behavior, so the repo does not keep two conflicting
  editor compatibility specs.
</Task>

<Task
  id="task-6"
  status="done"
  depends="task-2, task-3, task-4, task-5"
>
  Run the full verification loop: `cargo run -p supersigil verify`,
  `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features`,
  and `cargo nextest run`. Manually verify one compatible and one incompatible
  editor/server pairing in both VS Code and IntelliJ.
</Task>
```
