---
name: retroactive-specification
description: Recover Supersigil specs from existing code for one bounded area of a brownfield project. Use when working code exists without Supersigil docs and the user wants to document current behavior, baseline a refactor, or expose specification and test coverage gaps.
---

# Retroactive Specification

Use this skill to reverse-engineer Supersigil specs from existing code. Drive the process, but do not silently decide intent when the evidence is ambiguous.

## Current Contract

Use the current CLI as the source of truth for the spec graph:

```bash
supersigil new <type> <feature>
supersigil schema [--format json]
supersigil lint
supersigil ls [--format json]
supersigil context <id> [--format json]
supersigil plan [<id_or_prefix>] [--format json]
supersigil verify [--format terminal|json|markdown]
supersigil status [<id>] [--format json]
supersigil affected --since <ref> [--format json]
```

Use [references/source-gathering.md](references/source-gathering.md) for the evidence order and the standard ambiguity questions.

## Workflow

1. Scope one bounded area before reading broadly.
   Ask the user which module, feature, API surface, or service boundary to capture.
   Propose a traversal order if the area is still too large.
   Do not attempt the whole repository at once.

2. Gather sources of truth before reading implementation details.
   Read existing docs first.
   Then read public APIs, types, and tests.
   Read internal implementation last.

3. Treat tests as evidence, not authority.
   Existing tests are strong input for observed behavior.
   They are not proof that the behavior is intended if the user or docs disagree.

4. Ask clarifying questions only where intent matters.
   Use questions to separate intended behavior, legacy behavior, and obvious bugs.
   When behavior looks accidental, call that out instead of baking it into the spec silently.

5. Draft the spec graph incrementally.
   Start with `supersigil new` when scaffolding helps.
   Keep every authored document in `status: draft`.
   Produce requirement docs first, then add property, design, and tasks docs only where they clarify the scoped area.

6. Reconnect existing evidence to the graph.
   Use `VerifiedBy strategy="tag"` or `strategy="file-glob"` to connect real tests.
   Add `TrackedFiles` only when the owning source paths are concrete and helpful.
   Run `supersigil lint` after every spec write.

7. Use `supersigil verify` to expose specification debt.
   Coverage gaps, missing test mappings, stale globs, and status inconsistencies are part of the output, not noise to hide.
   Keep the docs in `status: draft` until the user has reviewed what the recovered graph actually says.

8. End each bounded area with a gap report.
   Summarize what the code does, what is now captured in specs, what lacks tests, and what still needs human intent decisions.

## Authoring Rules

- Work one bounded area at a time.
- Prefer existing docs and tests over inference from internals alone.
- Capture current behavior explicitly, but flag questionable or legacy behavior instead of normalizing it.
- Keep criterion IDs, document IDs, and task IDs stable once introduced.
- Use `VerifiedBy` only with real tags or real file globs.
- Do not promote statuses optimistically during recovery work.

## Failure Modes

- Do not attempt to specify the whole project in one pass.
- Do not assume code is correct just because it exists.
- Do not assume tests are current if the surrounding docs or user intent disagree.
- Do not hide ambiguity; surface it and ask.
- Do not present draft recovered specs as final truth.

## Handoff

If the user wants to continue implementing against the recovered specs, suggest `feature-development`.
If the recovered work reveals a planned refactor or behavior change, suggest `spec-driven-development`.
