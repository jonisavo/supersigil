---
name: ss-refactoring
description: Use when restructuring code that has Supersigil specs and behavior must not change. Activates for module extraction, file moves, renames, hierarchy flattening, or any structural cleanup. Keeps specs, criteria, tracked files, and verification evidence valid throughout.
---

# Refactoring

Use this skill when specs exist, tests pass, and the goal is to change
code structure without changing behavior. The spec graph is the behavioral
contract — it must stay green throughout.

## Current Contract

Use the current CLI as the source of truth:

```bash
supersigil status <id> [--format json]
supersigil plan [<id_or_prefix>] [--format json]
supersigil context <id> [--format json]
supersigil verify [--format terminal|json|markdown]
supersigil affected --since <ref> [--format json]
supersigil verify
```

If the spec graph is missing, broken, or incomplete for the area being
refactored, stop and hand the job to `ss-retroactive-specification` or
`ss-feature-specification` first. Do not refactor code that has no
behavioral contract.

## Workflow

1. Establish the refactoring boundary.
   Confirm with the user which module, subsystem, or code area is being
   restructured and what the structural goal is (extract module, rename
   abstraction, flatten hierarchy, etc.).
   Do not expand the boundary during the refactoring.

2. Snapshot the current verification state.
   Run `supersigil verify` and `supersigil status <id>` for all docs
   in the affected area.
   Record the current state: which criteria are covered, which tasks
   are done, which findings exist.
   This is the baseline — the refactoring must not make it worse.

3. Run all tests before changing anything.
   Confirm the test suite passes. If tests fail before the refactoring
   starts, that is a bug, not a refactoring problem. Stop and address
   it separately.

4. Make structural changes in small, verifiable steps.
   Move files, rename modules, extract types, reorganize imports — one
   logical change at a time.
   After each step, run the test suite to confirm behavior is preserved.

5. Update spec artifacts that reference moved code.
   After structural changes, update:
   - `<TrackedFiles paths="...">` when source paths changed.
   - `<VerifiedBy strategy="file-glob" paths="...">` when test file
     paths changed.
   - `supersigil: {tag}` comments if they moved to new files (tags
     themselves should not change).
   Run `supersigil verify` after each spec edit.

6. Do not change criteria, requirements, or design intent.
   The refactoring skill changes code structure and spec plumbing
   (paths, globs, tracked files). It does not change what the system
   does or what the specs say it should do.
   If the refactoring reveals that a criterion is wrong or missing,
   stop the refactoring for that area and hand it to
   `ss-feature-specification`.

7. Verify the refactoring preserved the contract.
   Run `supersigil verify` and compare against the baseline snapshot.
   The refactoring is complete when:
   - All tests still pass.
   - `verify` findings are the same or better than the baseline.
   - No new coverage gaps were introduced.
   - `supersigil affected --since HEAD` shows only the docs you
     intentionally updated.

8. End with a concrete summary.
   Report what structural changes were made, which spec artifacts were
   updated (paths, globs, tracked files), and confirm the verification
   state matches or improves on the baseline.

## Authoring Rules

- Do not change criterion IDs, document IDs, or task IDs during a
  refactoring. These are stable references that other documents depend on.
- Do not change document statuses during a refactoring unless the
  refactoring itself was the last remaining task for a status promotion.
- Update `TrackedFiles` and `VerifiedBy` paths to reflect the new
  file locations. Use `supersigil affected --since HEAD` to find docs
  that need path updates.
- Keep `supersigil: {tag}` comment tags stable even when moving test
  files. The tag is the stable identifier; the file location is not.

## Failure Modes

- Do not refactor code that has no specs. Create specs first.
- Do not change behavior during a refactoring. If behavior needs to
  change, that is a feature or bugfix, not a refactoring.
- Do not leave broken `TrackedFiles` or `VerifiedBy` paths behind.
  `verify` will catch stale globs — fix them before claiming done.
- Do not skip the baseline snapshot. Without it, you cannot prove the
  refactoring preserved the contract.

## Handoff

If the refactoring reveals missing specs, suggest `ss-retroactive-specification`
or `ss-feature-specification`.
If the refactoring is preparation for a new feature, suggest
`ss-spec-driven-development` or `ss-feature-development` for the next phase.
If the user wants to continue with more structural changes, stay in this
skill for the next bounded refactoring.
