---
supersigil:
  id: work-queries/tasks
  type: tasks
  status: done
title: "CLI Work Queries Tasks"
---

## Overview

This tasks document tracks the bounded recovery pass for the CLI `context` and
`plan` domain.

```supersigil-xml
<Task id="task-1" status="done" implements="work-queries/req#req-1-1, work-queries/req#req-1-2, work-queries/req#req-1-3, work-queries/req#req-2-1, work-queries/req#req-2-2, work-queries/req#req-2-3, work-queries/req#req-3-1, work-queries/req#req-3-2, work-queries/req#req-3-3, work-queries/req#req-4-1, work-queries/req#req-4-2, work-queries/req#req-4-3">
  Recover the current `context` and `plan` command behavior into project-local
  req, design, and tasks docs under
  `crates/supersigil-cli/specs/work-queries/`.
</Task>

<Task id="task-2" status="done">
  Decide whether ArtifactGraph evidence suppression for `plan` should remain in
  `supersigil-cli` or move down into the lower-level query model.
  Decision: keep in CLI. Moving it would require `supersigil-core` to depend on
  `supersigil-verify`, inverting the intended layering.
</Task>

<Task id="task-3" status="done" implements="work-queries/req#req-2-3">
  Add a test for the negative constraint on the `context` query model. Assert
  that the context output exposes verification targets, task links, and refs,
  but does NOT expose illustrations (i.e. informational References edges are
  excluded from the verification-relevant context view).
</Task>

<Task id="task-4" status="done" implements="work-queries/req#req-4-3">
  Add a test for the completed-task summary and empty message in terminal
  `plan` output. When completed tasks exist, assert that the plan appends a
  completed-task summary section. When there is no outstanding work at all,
  assert that the plan prints "No outstanding work."
</Task>
```

## Qualified Task Identity

```supersigil-xml
<Task id="task-5" status="done" implements="work-queries/req#req-5-2">
  Qualify depends_on at TaskInfo build time. In the graph builder where
  TaskInfo is constructed from parsed Task components, qualify bare depends
  values with the owning tasks_doc_id (format: tasks_doc_id#task_id). Values
  already containing # pass through as pre-qualified cross-document refs.
</Task>

<Task id="task-6" status="done" depends="task-5" implements="work-queries/req#req-5-4">
  Update partition_tasks to use qualified refs. Change the completed/pending
  HashSet keys from bare task_id to tasks_doc_id#task_id. Update the
  depends_on comparison to match against qualified keys. Change the returned
  actionable/blocked vecs to contain qualified refs.
</Task>

<Task id="task-7" status="done" depends="task-6" implements="work-queries/req#req-5-1">
  Update PlanOutput consumers. The actionable_tasks and blocked_tasks fields
  now contain qualified refs from partition_tasks. Update the JSON
  serialization tests and any terminal rendering that reads these fields.
</Task>

<Task id="task-8" status="done" depends="task-5" implements="work-queries/req#req-5-3">
  Update GraphRenderer to key by qualified ref. Change the task_set,
  task_map, forward, back, roots, and visited data structures to use
  tasks_doc_id#task_id as keys. Terminal display continues to show bare
  task_id within group headings; cross-document edges show the full
  qualified ref.
</Task>

<Task id="task-9" status="done" depends="task-7, task-8" implements="work-queries/req#req-5-1, work-queries/req#req-5-3, work-queries/req#req-5-4">
  Add tests for qualified task identity. Unit tests in query.rs for
  partition_tasks with overlapping bare IDs from different documents. Unit
  tests in format.rs for GraphRenderer with duplicate bare IDs. Integration
  test in cmd_plan.rs with a two-doc fixture asserting qualified refs in JSON
  output and correct terminal rendering.
</Task>
```

## Compact JSON Defaults

```supersigil-xml
<Task id="task-10" status="done" implements="work-queries/req#req-6-1, work-queries/req#req-6-2">
  Add a Detail enum (Compact/Full, default Compact) to the CLI format module.
  Add a --detail flag to ContextArgs. In the context JSON path, clear
  document.components when detail is Compact before serializing.
</Task>

<Task id="task-11" status="done" implements="work-queries/req#req-6-3, work-queries/req#req-6-4">
  Add a --detail flag to VerifyArgs. In the verify JSON path, clear
  evidence_summary.records when detail is Compact and the overall result is
  Clean.
</Task>

<Task id="task-12" status="done" depends="task-10, task-11" implements="work-queries/req#req-6-1, work-queries/req#req-6-2, work-queries/req#req-6-3, work-queries/req#req-6-4">
  Add tests for compact JSON defaults. Integration test in cmd_context.rs:
  default JSON has no components inside document; --detail full restores it.
  Integration test for verify: default clean-run JSON has no
  evidence_summary.records; --detail full includes them.
</Task>
```
