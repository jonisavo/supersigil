---
supersigil:
  id: kiro-import/tasks
  type: tasks
  status: done
title: "Kiro Import Tasks"
---

## Overview

This tasks document tracks the bounded ss-retroactive-specification pass for the
current Kiro import domain.

```supersigil-xml
<Task id="task-1" status="done" implements="kiro-import/req#req-1-1, kiro-import/req#req-1-2, kiro-import/req#req-1-3, kiro-import/req#req-1-4, kiro-import/req#req-2-1, kiro-import/req#req-2-2, kiro-import/req#req-2-3, kiro-import/req#req-2-4, kiro-import/req#req-2-5, kiro-import/req#req-3-1, kiro-import/req#req-3-2, kiro-import/req#req-3-3, kiro-import/req#req-3-4, kiro-import/req#req-3-5, kiro-import/req#req-4-1, kiro-import/req#req-4-2, kiro-import/req#req-4-3, kiro-import/req#req-5-1, kiro-import/req#req-5-2, kiro-import/req#req-5-3, kiro-import/req#req-5-4">
  Recover the current import behavior into project-local req, design, and
  tasks docs under `crates/supersigil-import/specs/kiro-import/`, then retire
  the stale root `specs/kiro-import/*` set.
</Task>

<Task id="task-2" status="done">
  Decide how `supersigil import` should target a named project in a
  multi-project workspace.
  Decision: no change needed. `--output-dir` is explicit and sufficient;
  auto-resolving from project globs is fragile with multi-glob projects.
</Task>

<Task id="task-3" status="done">
  Decide whether document IDs and `id_prefix` should stay manual or be derived
  automatically from the selected project and repository conventions.
  Decision: keep manual. `--prefix` handles the use case; auto-derivation
  would require guessing conventions and creates harder cleanup on mistakes.
</Task>

<Task id="task-4" status="done">
  Add end-to-end CLI coverage for the config-aware next-step hints and for
  write-conflict behavior, not just the library-level write semantics.
</Task>

<Task id="task-5" status="done">
  Decide whether the current best-effort, non-transactional write phase is an
  acceptable steady-state behavior or should become transactional.
</Task>

<Task id="task-6" status="done">
  Decide whether future non-Kiro importers should live in this same domain or
  whether the `import` project should be split into source-specific domains.
  Decision: keep unified. No second importer exists; if one appears, it
  naturally becomes a new module in `supersigil-import` with its own discovery
  logic. No pre-factoring needed.
</Task>

<Task id="task-7" status="done" implements="kiro-import/req#req-2-4">
  Add a test for Feature_Title precedence fallback. Assert that the importer
  chooses one Feature_Title per feature following the documented precedence
  order, and falls back correctly when higher-precedence sources are absent.
</Task>

<Task id="task-8" status="done" implements="kiro-import/req#req-4-3">
  Add a test for write-mode CLI output. When write mode succeeds, assert that
  the CLI prints the written file list, a summary, and a next-step hint to
  the expected output streams.
</Task>

<Task id="task-9" status="done" implements="kiro-import/req#req-2-1">
  Fix document ID generation to use `{feature}/{type}` convention instead of
  `{type}/{feature}`, matching `supersigil new`. Update `make_document_id`,
  its call sites, and all snapshot and property-based tests.
</Task>
```
