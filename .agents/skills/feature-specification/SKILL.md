---
name: feature-specification
description: Author or refine draft Supersigil specification documents for one bounded feature using the current pre-verification CLI. Use when the user wants to turn a feature idea or imported `.kiro/specs` material into `requirement`, `property`, `design`, or `tasks` MDX documents, or wants to clean up existing draft specs so `supersigil lint`, `supersigil context`, and `supersigil plan` work reliably.
---

# Feature Specification

Use this skill to create or repair Supersigil spec documents with the current dogfooding workflow. Keep work scoped to one feature, keep documents at `status: draft`, and use `schema`, `lint`, `ls`, `context`, and `plan` as the feedback loop.

## Current Contract

Work only within the CLI that exists today:

```bash
supersigil import --from kiro ...
supersigil schema [--format json]
supersigil lint
supersigil ls [--format json]
supersigil context <id> [--format json]
supersigil plan [<id_or_prefix>] [--format json]
```

Do not depend on `supersigil verify`, `supersigil status`, `supersigil affected`, or `supersigil graph`.

Do not claim a document set is verified. Claim only that it is lint-clean and graph-usable.

## Workflow

1. Bound the scope to one feature, subsystem, or import batch.
   Do not try to normalize the whole repo at once.

2. Prefer existing material over blank documents.
   If `.kiro/specs` exists and the user wants Supersigil docs, import first, then refine the generated MDX.
   If Supersigil docs already exist, inspect them before rewriting.

3. Inspect the current state before editing.
   Run `supersigil schema --format json` to get the current component and document type definitions.
   Run `supersigil ls --format json` to see what exists.
   Run `supersigil context <id>` on the main requirement or design doc.
   Run `supersigil plan <id_or_prefix>` to see uncovered criteria and pending tasks.

4. Keep every authored document in `status: draft`.
   Do not promote status based only on `lint`, `context`, or `plan`.

5. Author or edit docs using [templates.md](references/templates.md).
   Reuse imported prose where it is already good enough.
   Normalize structure incrementally instead of rewriting large documents unnecessarily.

6. Run `supersigil lint` after every write.
   Treat lint cleanliness as the minimum quality bar.
   If `lint` fails, fix that before doing more structural work.

7. Rebuild the graph with `ls`, `context`, or `plan` after structural edits.
   If these commands fail after a change, assume you broke cross-document refs or task dependencies and fix that immediately.

8. End with a concrete handoff.
   Summarize which docs are now lint-clean, which IDs or prefixes to inspect with `context` and `plan`, and which gaps still require future `verify` support.

## Authoring Rules

- Use stable IDs. Prefer `{feature}/{type-hint}/{name}` such as `auth/req/login` or `git-worktrees/tasks/bootstrap`.
- Keep relationship direction concrete to abstract.
  Properties point to requirement criteria with `<Validates>`.
  Design docs point to requirement or property docs with `<Implements>`.
  Tasks point to requirement criteria with the `implements` attribute.
- Put acceptance criteria only in requirement docs.
- Keep criterion IDs stable once referenced.
- Make tasks dependency-ordered with `depends` and keep them actionable.
- Use `<DependsOn>` for document-level ordering only.
- Use `<VerifiedBy>` only when the user gives real test paths or tags. Treat it as provisional metadata until `verify` exists.
- Use `<TrackedFiles>` only when concrete source paths are already known.

## Failure Modes

- Do not invent components or attributes. Use `supersigil schema --format json` as the source of truth, and fall back to [templates.md](references/templates.md) only if the command is unavailable.
- Do not mark work complete because imported docs exist. Imported docs are starting material, not proof of correctness.
- Do not leave broken refs in place for later. `context` and `plan` depend on a loadable graph.
- Do not spread one feature across many unrelated IDs or folders without a clear prefix strategy.

## Handoff

Suggest `feature-development` only after a future `verify` command exists. For now, hand off by pointing the user to:

- `supersigil context <main-id>` for relationship review
- `supersigil plan <feature-prefix>` for outstanding work
- the edited draft docs for human review
