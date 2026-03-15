---
name: feature-development
description: Implement or refine a bounded feature against existing Supersigil specs. Use when requirement, property, design, or tasks documents already exist and the user wants to code, test, update task status, repair coverage, or bring the feature to a verify-clean, implementation-ready state.
---

# Feature Development

Use this skill when the spec graph already exists and the job is to build or finish the feature against it. Augment the user's existing implementation methodology; if a TDD or code review skill is available, use it instead of replacing it.

## Current Contract

Use the current CLI as the source of truth:

```bash
supersigil ls [--format json]
supersigil context <id> [--format json]
supersigil plan [<id_or_prefix>] [--format json]
supersigil verify [--format terminal|json|markdown]
supersigil status [<id>] [--format json]
supersigil affected --since <ref> [--format json]
supersigil lint
```

Use [references/implementation-loop.md](references/implementation-loop.md) for the concrete command loop.
Use [references/test-tagging.md](references/test-tagging.md) when adding or repairing `VerifiedBy` evidence.

If the spec graph is missing, broken, or obviously incomplete, stop and hand the job back to `feature-specification` instead of improvising new structure during implementation.

## Workflow

1. Bound the work to one feature, prefix, or tightly-related task chain.
   Do not roam across unrelated docs just because `plan` shows more work.

2. Inspect the current state before coding.
   Run `supersigil status <main-id>` to understand the feature's current health.
   Run `supersigil plan <id_or_prefix>` to identify outstanding criteria and pending tasks.
   Run `supersigil context <id>` on the main requirement or design doc before changing code.

3. Choose the smallest unfinished slice.
   Prefer one outstanding criterion plus the directly linked task chain.
   If tasks exist, follow them.
   If tasks do not exist but the spec is otherwise sound, work against one criterion at a time and note the missing task structure in the handoff.

4. Implement against the existing spec, not around it.
   Follow the user's implementation skill if one is installed.
   If `test-driven-development` is available, use it for every behavior change.
   Do not add speculative features outside the current slice.

5. Keep the spec graph honest while you work.
   Update `<Task status="...">` only when the real state changes.
   Edit spec docs only when the implementation reveals a genuine spec change or missing evidence.
   Run `supersigil lint` after any spec edit before doing more work.

6. Add or repair verification evidence as part of the implementation.
   Prefer `VerifiedBy strategy="tag"` when you can add `supersigil: {tag}` comments to tests.
   Use `strategy="file-glob"` when file existence is the best available evidence.
   Do not leave empty `tag`, `paths`, or `refs` placeholders behind.

7. Promote document statuses when warranted.
   After marking a task `done`, check whether all tasks in the tasks doc are now done.
   If so, set the tasks doc to `status: done`, the sibling design doc to `status: approved`, and the sibling requirements doc to `status: implemented`.
   If some tasks are done but others remain, set the tasks doc to `status: in-progress`.
   Run `supersigil verify` to confirm — the `status_inconsistency` rule will warn about any missed promotions.

8. Run `supersigil verify` before claiming the slice is complete.
   Treat error-level findings as blockers for the scoped feature.
   If the user explicitly defers a finding, keep the affected docs in `status: draft` or otherwise avoid overstating completion.

9. Use `supersigil status` and `supersigil affected` to summarize the result.
   `status` is the default close-out summary.
   `affected --since <ref>` is for cases where changed tracked files may have invalidated nearby docs. `--since HEAD` is for cases where the user has not yet committed.
   For each affected doc, check whether they should be updated.

10. End with a concrete handoff.
   Summarize what criterion or task chain was completed, which tests now back it, the latest `verify` result, and what `plan` still shows for the feature.

## Authoring Rules

- Work one bounded feature slice at a time.
- Read `plan` and `context` before changing code.
- Keep criterion refs, task IDs, and document IDs stable once they are in use.
- Update task statuses honestly: `draft` for not-ready work, `ready` for actionable work, `in-progress` for active work, `done` only after implementation and supporting evidence both exist.
- Prefer criterion-level progress over "done enough" prose summaries.
- Add `supersigil: {tag}` comments only in comment styles the scanner understands today. See [references/test-tagging.md](references/test-tagging.md).
- Do not promote requirement or property statuses based only on code changes; use `verify`, `status`, and human review to justify status changes.

## Failure Modes

- Do not implement before reading the current spec state.
- Do not silently rewrite a broken or underspecified feature during implementation; hand it back to `feature-specification`.
- Do not mark tasks or documents complete because the code "probably" works.
- Do not add tests without reconnecting them to `VerifiedBy` when the feature depends on verification coverage.
- Do not ignore `verify` findings just because the code compiles or the local tests pass.

## Handoff

If the user discovers the feature was underspecified, suggest `feature-specification`.
If the user has working code but no specs, suggest `retroactive-specification`.
If the user wants the full guided loop for a new feature, suggest `spec-driven-development`.
