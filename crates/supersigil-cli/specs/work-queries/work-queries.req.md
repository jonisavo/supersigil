---
supersigil:
  id: work-queries/req
  type: requirements
  status: implemented
title: "CLI Work Queries"
---

## Introduction

This spec recovers the CLI query surface for `context` and `plan`. It captures
the current post-Illustrates query model, including ArtifactGraph-backed plan
filtering and the split between default actionable output and full output.

It does not attempt to re-spec `ls`, `schema`, `graph`, `verify`, `status`,
`affected`, `init`, or `new`. Those are separate CLI domains.

## Definitions

- **Work_Query_Command**: Either `context` or `plan`.
- **Context_View**: The structured document-centric query result returned by
  `DocumentGraph::context`.
- **Plan_Scope**: The resolved query mode for `plan`: one exact document, a
  prefix slice, or the whole workspace.
- **Outstanding_Target**: A requirement target that still appears in the final
  `plan` output after ArtifactGraph evidence filtering.
- **Actionable_Target**: An Outstanding_Target whose implementing task is
  currently unblocked, or that has no pending implementing task at all.

## Requirement 1: Query Resolution

As an operator, I want `context` and `plan` to resolve query input
consistently, so that I can inspect one document, a prefix slice, or the whole
workspace without guessing how the CLI will interpret my input.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE `context` command SHALL load the graph and resolve exactly one document
    by explicit ID. IF the requested ID is absent, THEN it SHALL fail the
    command rather than falling back to prefix behavior.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/tests/cmd_context.rs" />
  </Criterion>
  <Criterion id="req-1-2">
    THE `plan` command SHALL interpret no query argument as workspace scope, an
    exact document ID as single-document scope, and a non-empty non-exact
    string with at least one matching document ID as prefix scope. IF no IDs
    match, THEN it SHALL fail the command.
  </Criterion>
  <Criterion id="req-1-3">
    WHEN either Work_Query_Command fails because the requested document or
    query cannot be resolved, THEN it SHALL print a `supersigil ls` remediation
    hint to stderr before returning the query failure.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Context Views

As a developer, I want `context` to show the current query neighborhood around
one document, so that I can inspect its verification targets, incoming refs,
implementations, and linked tasks from one command.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    In terminal mode, THE `context` command SHALL print the document heading and
    status, then conditionally render sections for verification targets,
    implementing documents, referencing documents, and linked tasks when those
    collections are non-empty.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/src/commands/context.rs" />
  </Criterion>
  <Criterion id="req-2-2">
    In JSON mode, THE `context` command SHALL write the current `Context_View`
    structure with `document`, `criteria`, `implemented_by`, `referenced_by`,
    and `tasks`.
  </Criterion>
  <Criterion id="req-2-3">
    THE current `context` query model SHALL expose verification targets, task
    links, and document refs only. It SHALL NOT expose a separate illustrations
    collection.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Plan Views and Evidence Filtering

As a developer, I want `plan` to show only work that still lacks verification
evidence, so that planning output reflects the current ArtifactGraph rather
than only raw graph relationships.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    AFTER building the graph-level `PlanOutput`, THE `plan` command SHALL build
    plugin evidence, warn about plugin findings on stderr, and remove any
    target already backed by ArtifactGraph evidence before emitting the final
    output.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/src/commands/plan.rs" />
  </Criterion>
  <Criterion id="req-3-2">
    In JSON mode, THE `plan` command SHALL write the filtered `PlanOutput`
    structure with `outstanding_targets`, `pending_tasks`, and
    `completed_tasks`.
  </Criterion>
  <Criterion id="req-3-3">
    Plugin warnings SHALL remain on stderr so `plan --format json` keeps stdout
    valid JSON even when evidence discovery reports non-fatal plugin findings.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-cli/tests/cmd_plan.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Terminal Planning Semantics

As a developer, I want the terminal `plan` output to distinguish immediate work
from blocked work, so that I can see what to do next without losing the full
dependency picture.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    In default terminal mode, THE `plan` command SHALL render the dependency
    graph first and SHALL then show only Actionable_Targets plus a blocked-count
    summary when additional targets are waiting on upstream tasks.
  </Criterion>
  <Criterion id="req-4-2">
    In full terminal mode (`--full`), THE `plan` command SHALL render all remaining
    outstanding targets and SHALL include pending tasks in dependency order.
  </Criterion>
  <Criterion id="req-4-3">
    THE terminal `plan` output SHALL append a completed-task summary when
    completed tasks exist. IF there are no outstanding targets, no pending
    tasks, and no completed tasks, THEN it SHALL print `No outstanding work.`
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Qualified Task Identity in Plan Output

As an agent consuming `plan --format json`, I need task references to be
unambiguous across task documents, so that overlapping task IDs from different
documents do not collapse into misleading output.

### Definitions

- **Qualified_Task_Ref**: A string in `tasks_doc_id#task_id` form (e.g.
  `auth/tasks/login#task-1-1`) that uniquely identifies a task across all
  task documents in the project. Follows the same `doc_id#fragment` convention
  used for criterion refs.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE `actionable_tasks` and `blocked_tasks` fields in JSON `PlanOutput`
    SHALL contain Qualified_Task_Refs instead of bare task IDs.
  </Criterion>
  <Criterion id="req-5-2">
    THE `depends_on` field in `TaskInfo` SHALL contain Qualified_Task_Refs.
    Each bare `depends` value from the source document SHALL be qualified
    with the owning `tasks_doc_id` at build time. Cross-document task
    dependencies are not supported in the `depends` attribute; all
    dependencies are intra-document.
  </Criterion>
  <Criterion id="req-5-3">
    THE terminal dependency-graph renderer SHALL key tasks by
    Qualified_Task_Ref internally, so that tasks with the same bare ID
    from different documents do not collide or overwrite each other.
  </Criterion>
  <Criterion id="req-5-4">
    THE `partition_tasks` logic SHALL use Qualified_Task_Refs when comparing
    task identity, so that actionable/blocked classification is correct
    when multiple task documents share bare task IDs.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Compact JSON Defaults

As an agent consuming `context` and `verify` JSON output, I want the default
payload to contain only the derived, high-level fields, so that I can parse
responses efficiently without wading through redundant or debug-level data.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    In JSON mode, THE `context` command SHALL omit the raw
    `document.components` array by default. The derived fields (`criteria`,
    `decisions`, `linked_decisions`, `implemented_by`, `referenced_by`,
    `tasks`) SHALL remain present.
  </Criterion>
  <Criterion id="req-6-2">
    WHEN the `--detail full` flag is passed, THE `context` command SHALL
    include the raw `document.components` array in JSON output.
  </Criterion>
  <Criterion id="req-6-3">
    In JSON mode, THE `verify` command SHALL omit `evidence_summary.records`
    when the overall result is `Clean`. The `evidence_summary.coverage` and
    `evidence_summary.conflict_count` fields SHALL remain present when an
    evidence summary exists.
  </Criterion>
  <Criterion id="req-6-4">
    WHEN the `--detail full` flag is passed, THE `verify` command SHALL
    include the full `evidence_summary.records` array regardless of result
    status.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 7: Context Verification State

As a developer, I want `context` to show whether each verification target is
covered and how it is verified, so that I can assess document health from one
command without switching to `status`.

### Definitions

- **Evidence_Entry**: A discovered test that provides verification evidence for
  a criterion. Includes the test function name, file path, and source line
  number.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-7-1">
    THE `context` command SHALL build plugin evidence and construct an
    ArtifactGraph before rendering output, using the same evidence pipeline
    as `status` and `plan`.
  </Criterion>
  <Criterion id="req-7-2">
    In terminal mode, each verification target SHALL display a `[covered]` or
    `[uncovered]` marker after the criterion body text. Covered markers SHALL
    use `Token::StatusGood` coloring; uncovered markers SHALL use
    `Token::StatusBad`.
  </Criterion>
  <Criterion id="req-7-3">
    In terminal mode, each verification target SHALL display its VerifiedBy
    strategies and Evidence_Entry items as indented lines between the criterion
    line and any "Referenced by" lines. VerifiedBy lines SHALL use the format
    `verified by: strategy:value`. Evidence lines SHALL use the format
    `evidence: test_name (file:line)`.
  </Criterion>
  <Criterion id="req-7-4">
    In JSON mode, each criterion object SHALL include a `covered` boolean
    field, a `verified_by` string array, and an `evidence` array. These
    fields SHALL always be present, using empty arrays when no strategies
    or evidence exist. Each evidence entry SHALL contain `test_name`,
    `file`, and `line` fields. The enriched criterion type SHALL be a
    CLI-layer struct that wraps the core `TargetContext` without modifying
    `supersigil-core`.
  </Criterion>
</AcceptanceCriteria>
```
