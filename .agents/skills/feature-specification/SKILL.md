---
name: feature-specification
description: Author or refine Supersigil specification documents for one bounded feature using the full Supersigil CLI workflow. Use when the user wants to turn a feature idea or imported `.kiro/specs` material into `requirement`, `property`, `design`, or `tasks` MDX documents; repair or extend existing specs so `supersigil lint`, `supersigil context`, `supersigil plan`, `supersigil status`, and `supersigil verify` all work; or bring one feature's docs to an implementation-ready state.
---

# Feature Specification

Use this skill to create or repair Supersigil spec documents for one bounded feature or prefix. Augment the user's existing brainstorming or planning workflow; do not invent product direction they have not asked for.

## Current Contract

Use the current CLI as the source of truth:

```bash
supersigil import --from kiro ...
supersigil new <type> <feature>
supersigil schema [--format json]
supersigil lint
supersigil ls [--format json]
supersigil context <id> [--format json]
supersigil plan [<id_or_prefix>] [--format json]
supersigil verify [--format terminal|json|markdown]
supersigil status [<id>] [--format json]
supersigil affected --since <ref> [--format json]
supersigil graph [--format mermaid|dot]
```

Prefer `supersigil new` over hand-writing boilerplate when starting from scratch.
Prefer `supersigil import --from kiro` when the user already has `.kiro/specs` material.
Use `supersigil schema` to discover the current component and document type surface before inventing structure.
Use [references/templates.md](references/templates.md) only as a fallback example set when `new` is too sparse or imported prose needs normalization.

Write string literal attributes only. Even if `schema` examples still show JSX expression syntax like `refs={["..."]}`, author docs as `refs="a, b"` and `paths="x, y"`. The parser and lint rules reject expression attributes.

## Workflow

1. Bound the scope to one feature, subsystem, or import batch.
   Do not try to normalize the whole repo at once.

2. Inspect the current state before editing.
   Run `supersigil schema --format json` to get the current component and document type definitions.
   Run `supersigil ls --format json` to see what already exists for the feature.
   Run `supersigil context <id>` and `supersigil status <id>` on the main requirement or design doc when one already exists.
   Run `supersigil plan <id_or_prefix>` to see uncovered criteria and pending tasks.

3. Prefer existing material over blank documents.
   If `.kiro/specs` exists and the user wants Supersigil docs, import first, then refine the generated MDX.
   If Supersigil docs already exist, inspect them before rewriting.
   If no Supersigil docs exist yet, scaffold with `supersigil new`.

4. Keep every authored document in `status: draft`.
   Use later statuses only when the document actually earned them.
   Do not promote requirement, design, or property docs based only on `lint`.
   Treat `draft` as the safe working state until `verify` is clean and the user has reviewed the result.

5. Author or edit docs incrementally.
   Reuse imported or existing prose when it is already good enough.
   Normalize structure in place instead of rewriting large documents unnecessarily.
   Start from `supersigil new` scaffolds, then expand them with the patterns in [references/templates.md](references/templates.md) when they need richer structure.

6. Run `supersigil lint` after every write.
   Treat lint cleanliness as the minimum quality bar.
   If `lint` fails, fix that before doing more structural work.

7. Rebuild the graph after structural edits.
   Run `supersigil ls`, `supersigil context`, `supersigil plan`, or `supersigil graph` after adding or changing refs.
   If these commands fail after a change, assume you broke cross-document refs or task dependencies and fix that immediately.

8. Run `supersigil verify` before claiming the feature spec is ready.
   Fix uncovered criteria, missing test mappings, stale tracked files, and status inconsistencies that matter for the scoped feature.
   If the user is still drafting and wants to defer a finding, keep the relevant doc at `status: draft` and state the remaining gap clearly.

9. Use `supersigil status` to decide handoff readiness.
   Project the next step from the actual state: more spec work, human review, or implementation.
   Use `supersigil affected --since <ref>` when the user wants to review which docs source changes may have invalidated.

10. End with a concrete handoff.
    Summarize which docs are lint-clean, which docs are verify-clean, which IDs or prefixes to inspect with `context`, `plan`, and `status`, and what remains open.

## Authoring Rules

- Use stable IDs and match the current repo convention unless the user has a stronger local convention.
  The current built-in scaffolds shorten only `requirement` to `req`, so common primary IDs are `{feature}/req`, `{feature}/property`, `{feature}/design`, and `{feature}/tasks`.
  Keep criterion and task IDs stable once other docs reference them. Numeric IDs such as `req-1-1` and `task-2-3` are acceptable.
- Keep relationship direction concrete to abstract.
  Requirement docs own `<Criterion>` entries.
  Property docs point to requirement criteria with `<Validates>` and usually carry `<VerifiedBy>`.
  Design docs point to requirement or property docs with `<Implements>`.
  Tasks point to criteria with the `implements` attribute on each `<Task>`.
- Put acceptance criteria only in requirement docs.
- Make tasks dependency-ordered with `depends` and keep them actionable.
- Use `<DependsOn>` for document-level ordering only.
- Write list attributes as comma-separated strings, not JSX expressions.
  Use `refs="doc#a, doc#b"` and `paths="src/**/*.rs, tests/**/*.rs"`.
  Never write `refs={...}` or `paths={...}`.
- Never emit empty placeholders like `refs=""`, `paths=""`, or `tag=""`.
  Omit the component until a real value exists.
- Use `<VerifiedBy>` with the strategies the verification engine understands today.
  Prefer `strategy="tag"` when tests carry `supersigil: ...` comments.
  Use `strategy="file-glob"` when concrete test file globs are known.
  Use `strategy="review"` only when the user explicitly wants manual evidence and understands that it does not provide automated test coverage.
- Use `<TrackedFiles>` only when concrete source paths are already known.

## Failure Modes

- Do not invent components or attributes. Use `supersigil schema --format json` as the source of truth, and fall back to [templates.md](references/templates.md) only if the command is unavailable.
- Do not trust `schema` example syntax over the parser. Expression attributes are illustrative noise right now; `lint` is authoritative.
- Do not mark work complete because imported docs exist. Imported docs are starting material, not proof of correctness.
- Do not leave broken refs in place for later. `context` and `plan` depend on a loadable graph.
- Do not spread one feature across many unrelated IDs or folders without a clear prefix strategy.
- Do not promote statuses optimistically just because `lint` is clean. Use `verify`, `status`, and human review to justify status changes.

## Handoff

When specs are lint-clean, verify-clean, and reviewed, suggest `feature-development` if that skill exists. Otherwise hand off by pointing the user to:

- `supersigil context <main-id>` for relationship review
- `supersigil plan <feature-prefix>` for outstanding work
- `supersigil status <main-id>` for readiness review
- `supersigil verify` for the final spec health check
- the edited docs for human review
