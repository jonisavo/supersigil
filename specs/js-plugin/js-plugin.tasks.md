---
supersigil:
  id: js-plugin/tasks
  type: tasks
  status: done
title: "JavaScript/TypeScript Ecosystem Plugin Tasks"
---

## Overview

Implementation proceeds bottom-up: evidence contract extensions first, then the
Rust crate with config and oxc-based discovery, then the npm packages, then
cross-cutting integration and dogfooding.

Tasks 3-9 follow TDD: write failing tests against the requirement criteria
first, then implement until the tests pass.

```supersigil-xml
<Task id="task-1" status="done" implements="ecosystem-plugins/req#req-1-5">
  Add `ecosystem.js` config surface to `supersigil-core`. Add `JsEcosystemConfig`
  with a `test_patterns` string list defaulting to
  `["**/*.test.{ts,tsx,js,jsx}", "**/*.spec.{ts,tsx,js,jsx}"]`. Expand the known
  built-in plugin set to include `"js"`.

  TDD: write tests first for JS config parsing, defaults, and that
  unknown-plugin rejection still works.
</Task>

<Task id="task-2" status="done" implements="evidence-contract/req#req-2-2, evidence-contract/req#req-2-3">
  Add `EvidenceKind::JsVerifies` variant with `as_str()` returning
  `"js-verifies"`. Add `PluginProvenance::JsVerifies { annotation_span }` variant
  with `kind()` returning `EvidenceKind::JsVerifies`.

  TDD: write tests first for the new variant string surfaces and provenance-to-kind
  mapping in `supersigil-evidence`.
</Task>

<Task id="task-3" status="done" depends="task-1, task-2" implements="js-plugin/req#req-1-1, js-plugin/req#req-1-6, js-plugin/req#req-3-1, js-plugin/req#req-3-2">
  Create the `supersigil-js` crate. Implement `JsPlugin` struct with
  `EcosystemPlugin` trait. Implement `plan_discovery_inputs` filtering the
  shared test-file baseline to JS/TS extensions. Register `"js"` in the plugin
  assembly match arm in `supersigil-verify/src/plugins.rs`.

  TDD: write tests first for file discovery with gitignore filtering,
  pattern matching, and empty-scope handling. Create fixture directory
  structures with `.gitignore` files.
</Task>

<Task id="task-4" status="done" depends="task-3" implements="js-plugin/req#req-1-2, js-plugin/req#req-1-4, js-plugin/req#req-2-1, js-plugin/req#req-2-2, js-plugin/req#req-2-3, js-plugin/req#req-2-4, js-plugin/req#req-2-5">
  Implement oxc-based AST extraction for `verifies()` call expressions.
  Recognize direct `verifies(...)` calls and spread `{ ...verifies(...) }` form.
  Extract string literal arguments as criterion refs. Emit diagnostics for
  non-string arguments. Drop records when all arguments are non-string. Reject
  malformed refs.

  TDD: write fixture `.test.ts` files first covering each case (single ref,
  multiple refs, spread form, non-string arguments, all-non-string, malformed
  refs), then write tests asserting the expected evidence records and
  diagnostics for each fixture. Implement the AST walker until tests pass.
</Task>

<Task id="task-5" status="done" depends="task-3" implements="js-plugin/req#req-1-3">
  Implement oxc-based AST extraction for raw `{ meta: { verifies: [...] } }`
  in `test`/`it` call options. Recognize the object literal form and extract
  string literal elements from the `verifies` array.

  TDD: write fixture files first with raw meta form, then tests asserting
  expected evidence records. Implement until tests pass.
</Task>

<Task id="task-6" status="done" depends="task-4, task-5" implements="js-plugin/req#req-1-5">
  Implement describe-nesting tracking for full test name resolution. Track
  `describe` call nesting to produce names in the form `"suite > test"`.

  TDD: write fixture files with nested describes first, then tests asserting
  full test names like `"outer > inner > test name"`. Implement until tests
  pass.
</Task>

<Task id="task-7" status="done" depends="task-3" implements="js-plugin/req#req-4-1, js-plugin/req#req-4-2">
  Implement fault tolerance: per-file parse errors produce diagnostics and
  skip the file. Zero annotations across all files succeed with empty evidence.

  TDD: write fixture files first (syntax-error file, annotation-free test
  file), then tests asserting diagnostic output and empty success. Implement
  until tests pass.
</Task>

<Task id="task-8" status="done" implements="js-plugin/req#req-5-1, js-plugin/req#req-5-2, js-plugin/req#req-5-3">
  Create the `@supersigil/vitest` npm package in `packages/vitest/`. Add
  `package.json` with Vitest >= 4.1 peer dependency and zero runtime
  dependencies.

  TDD: write Vitest tests first asserting `verifies('a#b', 'c#d')` returns
  `{ meta: { verifies: ['a#b', 'c#d'] } }` and that the result spreads
  correctly with other options. Implement `verifies()` until tests pass.
</Task>

<Task id="task-9" status="done" implements="js-plugin/req#req-6-1, js-plugin/req#req-6-2, js-plugin/req#req-6-3, js-plugin/req#req-6-4, js-plugin/req#req-6-5">
  Create the `@supersigil/eslint-plugin` npm package in
  `packages/eslint-plugin/`. Expose `configs.recommended`.

  TDD: write ESLint `RuleTester` tests first covering: valid refs pass,
  malformed refs (missing `#`) get distinct error, unknown document IDs get
  distinct error, unknown criterion IDs get distinct error, and missing
  `supersigil` binary emits warning and disables. Implement the
  `valid-criterion-ref` rule (shelling out to
  `supersigil refs --all --format json` with session caching) until tests pass.
</Task>

<Task id="task-10" status="done" depends="task-6, task-7, task-8" implements="ecosystem-plugins/req#req-2-1">
  End-to-end integration test: add a JS scenario to the eval framework that
  exercises `supersigil verify` with JS test files containing `verifies()`
  annotations. Verify that JS evidence appears in the verification report
  alongside Rust evidence.
</Task>

<Task id="task-11" status="done" depends="task-10">
  Dogfooding: annotate existing Supersigil JS/TS test suites with `verifies()`
  where they cover spec criteria. Target `eval/`, `website/`, and
  `packages/preview/`. Enable the `"js"` plugin in `supersigil.toml`.
</Task>
```
