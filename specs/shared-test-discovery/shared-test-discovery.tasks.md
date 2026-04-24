---
supersigil:
  id: shared-test-discovery/tasks
  type: tasks
  status: done
title: "Shared Test Discovery Tasks"
---

```supersigil-xml
<DependsOn refs="shared-test-discovery/design" />
```

## Overview

Implement this feature in small TDD passes. Add failing tests for each behavior
boundary first, then land the minimum implementation needed to make that layer
pass. Keep the change scoped to config parsing, shared test-file resolution,
and documentation for the new user-facing setting.

```supersigil-xml
<Task id="task-1" status="done" implements="shared-test-discovery/req#req-1-1, shared-test-discovery/req#req-1-2, shared-test-discovery/req#req-1-3, shared-test-discovery/req#req-1-4">
  TDD: add failing config tests for the workspace-level test discovery policy.
  Cover default `test_discovery.ignore = "standard"` when the section is
  omitted, explicit `ignore = "off"`, unknown value rejection, and the fact
  that the setting is top-level and not per-project or per-plugin. Extend the
  config property generator and round-trip tests so the new config type
  participates in serialization stability.
</Task>

<Task id="task-2" status="done" depends="task-1" implements="shared-test-discovery/req#req-1-1, shared-test-discovery/req#req-1-2, shared-test-discovery/req#req-1-3, shared-test-discovery/req#req-1-4">
  Implement the config surface in `supersigil-core`. Add
  `TestDiscoveryConfig` and `TestDiscoveryIgnoreMode`, default the nested
  section through serde, expose the types from the crate root, and keep minimal
  configs such as `paths = ["specs/**/*.md"]` loading without a
  `[test_discovery]` table.
</Task>

<Task id="task-3" status="done" depends="task-2" implements="shared-test-discovery/req#req-2-1, shared-test-discovery/req#req-2-2, shared-test-discovery/req#req-2-4, shared-test-discovery/req#req-2-5, shared-test-discovery/req#req-3-1, shared-test-discovery/req#req-3-2, shared-test-discovery/req#req-3-3, shared-test-discovery/req#req-4-1, shared-test-discovery/req#req-4-2, shared-test-discovery/req#req-4-4">
  TDD: add failing shared resolver tests around `resolve_test_files` and
  `resolve_test_files_for_project`. Cover ignored `node_modules/` and `dist/`
  paths excluded in standard mode, the same paths included in off mode, nested
  ignore files, sorted and deduplicated output, unchanged top-level versus
  multi-project test-glob selection, and unchanged criterion-nested
  `VerifiedBy strategy="file-glob"` path expansion.
</Task>

<Task id="task-4" status="done" depends="task-2" implements="shared-test-discovery/req#req-2-3">
  TDD: add failing pipeline regression coverage for shared-baseline consumers.
  Cover tag scanning so ignored files do not produce tag matches in standard
  mode, and add a JS-flavored verification regression where an ignored
  malformed `.test.ts` file does not produce a JS plugin discovery warning
  because `JsPlugin` consumes the shared baseline directly.
</Task>

<Task id="task-5" status="done" depends="task-3, task-4" implements="shared-test-discovery/req#req-2-1, shared-test-discovery/req#req-2-2, shared-test-discovery/req#req-2-3, shared-test-discovery/req#req-2-4, shared-test-discovery/req#req-2-5, shared-test-discovery/req#req-3-1, shared-test-discovery/req#req-3-2, shared-test-discovery/req#req-3-3, shared-test-discovery/req#req-4-1, shared-test-discovery/req#req-4-2, shared-test-discovery/req#req-4-4">
  Implement policy-aware shared test-file resolution. Add the needed ignore
  and glob matching dependencies, route `resolve_test_files*` through a
  dedicated resolver that uses `ignore::WalkBuilder` in standard mode and raw
  glob expansion in off mode, preserve sorted and deduplicated output, and
  leave generic `expand_globs` plus `VerifiedBy file-glob` resolution
  unchanged.
</Task>

<Task id="task-6" status="done" depends="task-5" implements="shared-test-discovery/req#req-1-1, shared-test-discovery/req#req-4-1, shared-test-discovery/req#req-4-2, shared-test-discovery/req#req-4-3, shared-test-discovery/req#req-4-4">
  Update user-facing and internal documentation. Document `[test_discovery]`
  in the configuration reference, correct JS plugin docs so ignore behavior is
  owned by the shared baseline, and update the polish-audit note to record the
  chosen design. Keep the docs explicit that spec `paths`, plugin-owned
  widening, and `VerifiedBy strategy="file-glob"` are not changed by this
  feature.
</Task>

<Task id="task-7" status="done" depends="task-6">
  Run the final verification pass for the feature. Execute `cargo run -p
  supersigil verify`, `cargo fmt --all`, `cargo clippy --workspace
  --all-targets --all-features`, and `cargo nextest run`; fix any remaining
  warnings, failures, or stale spec findings before marking the tasks ready for
  implementation review.
</Task>
```
