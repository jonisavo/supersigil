---
supersigil:
  id: ecosystem-plugins/tasks
  type: tasks
  status: in-progress
title: "Ecosystem Plugins Tasks"
---

## Overview

This tasks document now tracks the recovered cross-cutting ecosystem layer and
the split from the old root monolith into crate-local domains.

```supersigil-xml
<Task id="task-1" status="done" implements="ecosystem-plugins/req#req-1-1, ecosystem-plugins/req#req-1-2, ecosystem-plugins/req#req-1-3, ecosystem-plugins/req#req-1-4, ecosystem-plugins/req#req-2-1, ecosystem-plugins/req#req-2-2, ecosystem-plugins/req#req-2-3, ecosystem-plugins/req#req-2-4, ecosystem-plugins/req#req-3-1, ecosystem-plugins/req#req-3-2, ecosystem-plugins/req#req-3-3, ecosystem-plugins/req#req-4-1, ecosystem-plugins/req#req-4-2, ecosystem-plugins/req#req-4-3">
  Recover the ecosystem project by splitting crate-local behavior into
  `evidence-contract`, `rust-plugin`, and `verifies-macro`, while narrowing the
  root `ecosystem-plugins` docs to cross-cutting activation, orchestration, and
  report-surfacing behavior.
</Task>

<Task id="task-2" status="done" implements="ecosystem-plugins/req#req-2-2">
  Move plugin-specific discovery-input planning behind the shared plugin
  boundary while keeping workspace-wide evidence semantics unchanged.
</Task>

<Task id="task-3" status="done" implements="ecosystem-plugins/req#req-1-2, ecosystem-plugins/req#req-1-3, ecosystem-plugins/req#req-2-4, ecosystem-plugins/req#req-3-1, ecosystem-plugins/req#req-3-3">
  Add end-to-end CLI coverage for ecosystem config-policy branches and report
  surfacing, not just unit coverage for plugin assembly and report formatting.
</Task>

<Task id="task-4" status="done" implements="ecosystem-plugins/req#req-4-3">
  Record the future ecosystem topology decision: keep root
  `ecosystem-plugins` docs in `workspace`, keep `evidence-contract` in a
  shared ecosystem project, and split each built-in plugin family into its own
  project once a second built-in ecosystem exists.
</Task>

<Task id="task-5" status="pending" implements="ecosystem-plugins/req#req-1-4">
  Add a test for `ecosystem.rust` config field exposure. Assert that
  `Config` exposes `ecosystem.rust` with `validation` policy and
  `project_scope` entries, and that the Rust-specific fields deserialize
  correctly from `supersigil.toml`.
</Task>

<Task id="task-6" status="pending" implements="ecosystem-plugins/req#req-4-1">
  Add a test verifying that `DocumentGraph` and core config remain usable
  without a compile-time dependency on Rust parsing. Assert that a config
  without Rust ecosystem enabled (e.g. `plugins = []`) can still build a
  graph and run structural verification successfully.
</Task>

<Task id="task-7" status="pending" implements="ecosystem-plugins/req#req-4-2">
  Add a test verifying that ecosystem implementation remains split between
  shared evidence-contract, Rust runtime, and Rust proc-macro crates. This
  can be a build-time or structural test asserting the crate dependency
  boundaries are maintained (e.g. `supersigil-core` does not depend on
  `supersigil-rust-macros`).
</Task>

<Task id="task-8" status="pending" implements="ecosystem-plugins/req#req-4-3">
  Add a test verifying that the ecosystem layer leaves room for future
  ecosystems. Assert that the plugin activation path accepts unknown plugin
  names gracefully (already tested as rejection), and that the shared
  `evidence-contract` types are not Rust-specific in their interface.
</Task>

<Task id="task-9" status="pending" implements="ecosystem-plugins/req#req-5-2">
  Add `RepositoryProvider` enum, `RepositoryInfo` struct, and
  `WorkspaceMetadata` struct to `supersigil-evidence`. Add
  `parse_repository_url` utility that parses HTTPS and SSH repository URLs,
  infers provider from hostname, and records the host. Unit tests for each
  provider format and unrecognized hosts.
</Task>

<Task id="task-10" status="pending" depends="task-9" implements="ecosystem-plugins/req#req-5-1">
  Add `workspace_metadata` method to the `EcosystemPlugin` trait with a
  default implementation returning empty metadata. Update `RustPlugin` to
  implement it by reading `Cargo.toml` at the workspace root
  (`workspace.package.repository` or `package.repository`) and passing the
  raw URL through `parse_repository_url`. Unit tests for workspace and
  single-crate manifests, missing repository field, and malformed TOML.
</Task>

<Task id="task-11" status="pending" depends="task-9" implements="config/req#req-3-6">
  Add `DocumentationConfig` and `RepositoryConfig` to the `Config` model in
  `supersigil-core`. Parse `[documentation.repository]` with `provider`,
  `repo`, optional `host`, and optional `main_branch`. Reject unknown provider
  values during config loading. Unit tests for valid config, missing optional
  fields, and unknown provider rejection.
</Task>

<Task id="task-12" status="pending" depends="task-10, task-11" implements="ecosystem-plugins/req#req-5-1, ecosystem-plugins/req#req-5-3">
  Add CLI resolution function that checks explicit config first, then
  iterates enabled plugins calling `workspace_metadata`. First `Some` wins.
  Log `Err` results as warnings. Unit tests for config-wins, plugin-fallback,
  and no-result cases.
</Task>
```
