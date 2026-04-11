---
supersigil:
  id: verifies-macro/tasks
  type: tasks
  status: done
title: "Verifies Macro Tasks"
---

## Overview

This tasks document tracks the ss-retroactive-specification pass for the
`supersigil-rust-macros` surface.

```supersigil-xml
<Task id="task-1" status="done" implements="verifies-macro/req#req-1-1, verifies-macro/req#req-1-2, verifies-macro/req#req-1-3, verifies-macro/req#req-1-4, verifies-macro/req#req-2-1, verifies-macro/req#req-2-2, verifies-macro/req#req-2-3, verifies-macro/req#req-2-4, verifies-macro/req#req-3-1, verifies-macro/req#req-3-2, verifies-macro/req#req-3-3, verifies-macro/req#req-3-4, verifies-macro/req#req-4-1, verifies-macro/req#req-4-2, verifies-macro/req#req-4-3, verifies-macro/req#req-4-4">
  Recover the current `#[verifies(...)]` proc-macro behavior into project-local
  req, design, and tasks docs under
  `crates/supersigil-rust-macros/specs/verifies-macro/`.
</Task>

<Task id="task-2" status="done" implements="verifies-macro/req#req-1-4, verifies-macro/req#req-3-2">
  Reject fragmentless and empty-fragment refs at the macro boundary so
  `#[verifies(...)]` only accepts full criterion refs.
</Task>

<Task id="task-3" status="done" implements="verifies-macro/req#req-4-2, verifies-macro/req#req-4-3, verifies-macro/req#req-4-4">
  Remove the duplicated multi-project resolution logic shared with
  `supersigil-rust::scope`.
</Task>

<Task id="task-4" status="done" implements="verifies-macro/req#req-3-4">
  Improve cache invalidation so graph rebuilds respond to spec-file changes and
  not only to `supersigil.toml` changes.
</Task>

<Task id="task-5" status="done" implements="verifies-macro/req#req-2-3">
  Add a trybuild test for the case where `SUPERSIGIL_PROJECT_ROOT` is set to
  a path that does not contain `supersigil.toml`. Assert the macro produces a
  compile-time error indicating the config file was not found.
</Task>

<Task id="task-6" status="done" implements="verifies-macro/req#req-2-4">
  Add tests for `ecosystem.rust.validation` policy variants (`off`, `dev`,
  `all`). Assert that the macro respects each policy: `off` skips validation,
  `dev` validates only in dev-dependencies, and `all` validates unconditionally.
</Task>

<Task id="task-7" status="done" implements="verifies-macro/req#req-3-3">
  Add a trybuild test for the failure path when config loading, parsing, or
  graph construction fails while validation is active. Assert the macro
  produces a compile-time diagnostic rather than silently passing.
</Task>

<Task id="task-8" status="done" implements="verifies-macro/req#req-4-2">
  Workspace-wide validation: the proc macro builds a graph from all configured
  projects so cross-project `#[verifies]` refs resolve. Covered by
  `resolve_workspace_validation_inputs_includes_all_projects` unit test.
</Task>

<Task id="task-9" status="done" implements="verifies-macro/req#req-4-3, verifies-macro/req#req-4-4">
  Remove per-project scoping from the proc macro. The macro uses
  `resolve_workspace_validation_inputs` which needs neither `project_scope`
  nor `CARGO_MANIFEST_DIR`. Missing `paths`/`projects` produces a
  compile-time error via `MissingPathsAndProjects`.
</Task>
```
