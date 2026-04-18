---
supersigil:
  id: decision-components/tasks
  type: tasks
  status: done
title: "Decision Components Tasks"
---

```supersigil-xml
<DependsOn refs="decision-components/design" />
```

## Overview

Implementation follows a test-first sequence: each task writes tests before
the production code that satisfies them. Tasks are ordered so that each layer
builds on a tested foundation — component definitions first, then graph
integration, then verification rules, then CLI and output integration.

The transitive affected-doc enhancement (`verification-engine/req#req-6-4`) is a
separate concern tracked in the verification-engine spec; it is not a task here.

## Phase 1: Component Definitions

```supersigil-xml
<Task id="task-1" status="done" implements="decision-components/req#req-1-1, decision-components/req#req-2-1, decision-components/req#req-3-1">
  **Tests:** Add unit tests in `component_defs_unit_tests.rs` asserting that
  `Decision`, `Rationale`, and `Alternative` are present in
  `ComponentDefs::defaults()` with the correct attribute schemas:
  - `Decision`: referenceable, not verifiable, required `id`.
  - `Rationale`: not referenceable, not verifiable, no required attrs.
  - `Alternative`: referenceable, not verifiable, required `id` and `status`.

  **Implementation:** Add the three component definitions to
  `ComponentDefs::defaults()` in `crates/supersigil-core/src/component_defs.rs`.
  Add `DECISION`, `RATIONALE`, `ALTERNATIVE` constants to
  `crates/supersigil-core/src/graph.rs`.
</Task>
```

## Phase 2: Graph Integration

```supersigil-xml
<Task id="task-2" status="done" depends="task-1" implements="decision-components/req#req-1-3, decision-components/req#req-1-4, decision-components/req#req-3-6">
  **Tests:** Add property and unit tests in
  `crates/supersigil-core/src/graph/tests/` verifying:
  - `Decision` and `Alternative` are indexed in the component index.
  - Fragment refs `doc#decision-id` and `doc#alternative-id` resolve.
  - `References` nested inside `Decision` produce correct reverse mappings.
  - `TrackedFiles` nested inside `Decision` are indexed for affected-doc checks.
  - `DependsOn` nested inside `Decision` creates document dependency edges.

  **Implementation:** Verify existing recursive walks in
  `build_component_index`, `resolve_refs`, `build_reverse_mappings`, and
  `build_tracked_files_index` handle the new components. If any walk is
  not recursive (particularly `build_tracked_files_index`), extend it.
  No changes expected for indexing, resolution, or reverse mappings.
</Task>
```

## Phase 3: ADR Document Type

```supersigil-xml
<Task id="task-3" status="done" depends="task-1" implements="decision-components/req#req-4-1, decision-components/req#req-4-3">
  **Tests:** Add a unit test asserting that `adr` is a recognized document
  type with statuses `draft`, `review`, `accepted`, `superseded`, and no
  required components. Add a lint test confirming that a document with
  `type: adr` and no `Decision` components is lint-clean.

  **Implementation:** Register the `adr` type as a built-in. Add `"adr"` to
  `BUILTIN_DOC_TYPES` in `crates/supersigil-cli/src/commands.rs`. Add the
  type definition with statuses to the config defaults.
</Task>

<Task id="task-4" status="done" depends="task-3" implements="decision-components/req#req-4-2">
  **Tests:** Add a test that `supersigil new adr &lt;feature&gt;` produces a
  lint-clean file with `type: adr`, `status: draft`, and appropriate
  scaffold content (Decision placeholder). Test that when a requirements
  doc exists for the feature, the scaffold includes a `References` link.

  **Implementation:** Add an `"adr"` branch to `generate_template` in
  `crates/supersigil-cli/src/commands/new.rs`.
</Task>
```

## Phase 4: Structural Verification Rules

```supersigil-xml
<Task id="task-5" status="done" depends="task-2" implements="decision-components/req#req-2-2, decision-components/req#req-3-4">
  **Tests:** Add tests for placement validation:
  - `Rationale` at document root → `invalid_rationale_placement` finding.
  - `Rationale` inside non-Decision component → finding.
  - `Rationale` inside `Decision` → no finding.
  - `Alternative` at document root → `invalid_alternative_placement` finding.
  - `Alternative` inside non-Decision component → finding.
  - `Alternative` inside `Decision` → no finding.
  - Draft gating suppresses both to info.

  **Implementation:** Add `InvalidRationalePlacement` and
  `InvalidAlternativePlacement` variants to `RuleName` in
  `crates/supersigil-verify/src/report.rs`. Add config keys to
  `KNOWN_RULES` in `crates/supersigil-core/src/config.rs`. Implement
  `check_rationale_placement` and `check_alternative_placement` in
  `crates/supersigil-verify/src/rules/structural.rs` using the
  exact-parent-name pattern from `check_expected_placement`. Wire into
  `verify_structural` in `crates/supersigil-verify/src/lib.rs`.
</Task>

<Task id="task-6" status="done" depends="task-2" implements="decision-components/req#req-2-3">
  **Tests:** Add tests for rationale cardinality:
  - Decision with zero Rationale children → no finding.
  - Decision with one Rationale child → no finding.
  - Decision with two Rationale children → `duplicate_rationale` finding
    on the second.
  - Draft gating suppresses to info.

  **Implementation:** Add `DuplicateRationale` variant to `RuleName`. Add
  config key to `KNOWN_RULES`. Implement `check_duplicate_rationale` in
  structural rules. Wire into `verify_structural`.
</Task>
```

## Phase 5: Decision Quality Rules

```supersigil-xml
<Task id="task-7" status="done" depends="task-5, task-6" implements="decision-components/req#req-5-1, decision-components/req#req-5-4">
  **Tests:** Add tests for `incomplete_decision`:
  - Decision with Rationale child → no finding.
  - Decision without Rationale child → finding.
  - Default severity is warning.
  - Draft gating suppresses to info.
  - Per-rule override to `off` suppresses entirely.

  **Implementation:** Add `IncompleteDecision` variant to `RuleName` with
  default severity warning. Add config key to `KNOWN_RULES`. Create
  `crates/supersigil-verify/src/rules/decision.rs` with
  `check_incomplete`. Register module in `rules.rs`. Wire into
  `verify_structural`.
</Task>

<Task id="task-8" status="done" depends="task-7" implements="decision-components/req#req-5-2, decision-components/req#req-5-4">
  **Tests:** Add tests for `orphan_decision`:
  - Decision with nested `References` → no finding.
  - Decision with nested `TrackedFiles` → no finding.
  - Decision with nested `DependsOn` → no finding.
  - Decision referenced by another document → no finding.
  - Decision with no outward connections and not referenced → finding.
  - Default severity is warning.
  - Draft gating and per-rule override work correctly.

  **Implementation:** Add `OrphanDecision` variant to `RuleName`. Add config
  key to `KNOWN_RULES`. Implement `check_orphan` in
  `crates/supersigil-verify/src/rules/decision.rs`. This function needs
  both the document list and the `DocumentGraph` for reverse lookups.
</Task>

<Task id="task-9" status="done" depends="task-8" implements="decision-components/req#req-5-3, decision-components/req#req-5-4">
  **Tests:** Add tests for `missing_decision_coverage`:
  - Design doc with a Decision in another doc referencing it → no finding.
  - Design doc with a Decision in the same doc → no finding.
  - Design doc with no Decision referencing it → finding.
  - Non-design doc with no Decision referencing it → no finding (rule
    only applies to design docs).
  - Default severity is off — verify the finding is suppressed by default.
  - Per-rule override to `warning` activates the check.

  **Implementation:** Add `MissingDecisionCoverage` variant to `RuleName`
  with default severity off. Add config key to `KNOWN_RULES`. Implement
  `check_coverage` in `crates/supersigil-verify/src/rules/decision.rs`.
</Task>
```

## Phase 6: Document Type Enforcement

```supersigil-xml
<Task id="task-10" status="done" depends="task-1" implements="decision-components/req#req-1-2, decision-components/req#req-3-5">
  **Tests:** Add tests confirming no document-type enforcement and
  Alternative cardinality:
  - A document with `type: requirements` containing a `Decision` is
    lint-clean and verify-clean.
  - A document with `type: tasks` containing a `Decision` is lint-clean.
  - A Decision with zero, one, and three Alternative children — all
    lint-clean (no cardinality limit on alternatives).

  **Implementation:** No enforcement code needed. These are negative tests
  confirming that the components work in any document type and that
  alternatives have no upper limit.
</Task>

<Task id="task-10b" status="done" depends="task-5" implements="decision-components/req#req-3-2, decision-components/req#req-3-3, decision-components/req#req-5-4">
  **Tests:** Add tests for `invalid_alternative_status`:
  - Alternative with `status="rejected"` → no finding.
  - Alternative with `status="deferred"` → no finding.
  - Alternative with `status="superseded"` → no finding.
  - Alternative with `status="accepted"` → finding.
  - Alternative with `status=""` → finding.
  - Default severity is warning.
  - Draft gating suppresses to info.

  **Implementation:** Add `InvalidAlternativeStatus` variant to `RuleName`
  with default severity warning. Add config key to `KNOWN_RULES`.
  Implement `check_alternative_status` in structural rules. Wire into
  `verify_structural`.
</Task>
```

## Phase 7: Context Output Integration

```supersigil-xml
<Task id="task-11" status="done" depends="task-2" implements="decision-components/req#req-6-1">
  **Tests:** Add tests in context query tests verifying:
  - A document containing `Decision` components produces `decisions` in
    `ContextOutput` with correct `id`, `body_text`, `rationale_text`,
    and `alternatives` (id, status, body_text).
  - A document with no Decision components has empty `decisions`.
  - JSON serialization round-trips the new fields.

  **Implementation:** Add `DecisionContext` and `AlternativeContext` types
  to `crates/supersigil-core/src/graph/query.rs`. Add `decisions` field
  to `ContextOutput`. Implement `extract_decisions` following the pattern
  of `extract_criteria`. Update terminal and JSON formatters.
</Task>

<Task id="task-12" status="done" depends="task-11" implements="decision-components/req#req-6-2">
  **Tests:** Add tests verifying:
  - When doc A contains a Decision with `&lt;References refs="doc-B"&gt;`,
    context output for doc B includes doc A's decision in
    `linked_decisions`.
  - When no Decision references doc B, `linked_decisions` is empty.
  - JSON serialization includes the new field.

  **Implementation:** Add `LinkedDecision` type and `linked_decisions`
  field to `ContextOutput`. In `build_context`, after computing
  `referenced_by`, scan referencing docs for Decision components whose
  nested References target the current document. Update formatters.
</Task>
```

## Phase 8: Affected Integration

```supersigil-xml
<Task id="task-13" status="done" depends="task-2" implements="decision-components/req#req-6-3">
  **Tests:** Add tests verifying:
  - A document with a Decision containing `&lt;TrackedFiles paths="src/**"&gt;`,
    when matched files change, is reported as affected.
  - This works identically to top-level TrackedFiles.

  **Implementation:** Verify that `build_tracked_files_index` walks nested
  components recursively. If not, extend the walk. If it already does,
  this task is tests-only.
</Task>
```

## Phase 9: Website Documentation

```supersigil-xml
<Task id="task-14" status="done" depends="task-9, task-12">
  Update the website documentation to cover the new components, document
  type, and rules:
  - Add Decision, Rationale, Alternative to `reference/components.mdx`.
  - Add `adr` to the document types table in `reference/components.mdx`.
  - Add the six new rules to `concepts/verification.mdx`.
  - Add the new rules to `reference/configuration.mdx`.
  - Consider a new concepts page or section on rationale tracking.
</Task>
```

## Phase 10: Dogfooding

```supersigil-xml
<Task id="task-15" status="done" depends="task-4, task-9">
  Convert `decision-components/adr` from prose to actual Decision,
  Rationale, and Alternative components. Change its type from `design`
  to `adr`. Run `supersigil verify` to confirm the converted document
  is clean and the new rules produce the expected findings.

  This serves as the first real-world validation of the feature.
</Task>
```
