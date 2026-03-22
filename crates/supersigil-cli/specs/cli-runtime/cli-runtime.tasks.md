---
supersigil:
  id: cli-runtime/tasks
  type: tasks
  status: done
title: "CLI Runtime Tasks"
---

## Overview

This tasks document tracks the first bounded recovery pass for the CLI project:
the startup, dispatch, loader, and formatting runtime layer.

```supersigil-xml
<Task id="task-1" status="done" implements="cli-runtime/req#req-1-1, cli-runtime/req#req-1-2, cli-runtime/req#req-1-3, cli-runtime/req#req-1-4, cli-runtime/req#req-2-1, cli-runtime/req#req-2-2, cli-runtime/req#req-2-3, cli-runtime/req#req-2-4, cli-runtime/req#req-2-5, cli-runtime/req#req-3-1, cli-runtime/req#req-3-2, cli-runtime/req#req-3-3, cli-runtime/req#req-3-4, cli-runtime/req#req-4-1, cli-runtime/req#req-4-2, cli-runtime/req#req-4-3, cli-runtime/req#req-4-4">
  Recover the current CLI runtime boundary into project-local req, design, and
  tasks docs under `crates/supersigil-cli/specs/cli-runtime/`.
</Task>

<Task id="task-2" status="done">
  Add binary-level coverage for runtime exit semantics, especially the verify
  warnings-only path that should exit with code 2 rather than 1.
</Task>

<Task id="task-3" status="done">
  Add process-level coverage for color-resolution precedence across `--color`,
  `FORCE_COLOR`, `NO_COLOR`, and TTY-sensitive default behavior. The current
  coverage is unit-level only.
</Task>

<Task id="task-4" status="done">
  Decide whether the runtime should keep Unicode coupled to the color decision
  or expose a separately testable display-mode concept. The current behavior is
  simple and intentional, but it is a policy choice rather than a necessity.
</Task>

<Task id="task-5" status="done" implements="cli-runtime/req#req-2-2">
  Add a test asserting that the project root is the parent directory of the
  resolved config path. Set up a fixture with `supersigil.toml` at a known
  location, invoke the CLI, and verify the loader treats the parent of
  `supersigil.toml` as the project root for path resolution.
</Task>

<Task id="task-6" status="done" implements="cli-runtime/req#req-2-5">
  Add a test verifying that the loader normalizes paths to project-root-relative
  before graph construction. Use a fixture where spec files are discovered via
  glob, and assert that the resulting document paths in the graph are relative
  to the project root rather than absolute or cwd-relative.
</Task>
```
