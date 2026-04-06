---
name: ci-review
description: Use Supersigil for CI gating and pull request review. Use when the user wants to set up spec verification in CI pipelines, review PRs against spec coverage, or interpret verification output in automated contexts.
---

# CI and Review

Use this skill when the goal is to integrate Supersigil verification into
CI pipelines, review pull requests against spec coverage, or interpret
verification output in automated contexts.

## Current Contract

```bash
supersigil verify [--format terminal|json|markdown]
supersigil affected --since <ref> [--format json]
supersigil status [<id>] [--format json]
supersigil plan [<id_or_prefix>] [--format json]
supersigil verify
```

## CI Pipeline Integration

### Basic Verification Gate

Add `supersigil verify` as a CI step. It exits non-zero when error-level
findings exist, making it a natural gate:

```yaml
# GitHub Actions example
- name: Verify specs
  run: supersigil verify --format terminal
```

### PR-Scoped Verification

Use `supersigil affected --since` to scope verification to documents
affected by the PR's changes:

```bash
# Find docs affected by changes in this PR
supersigil affected --since origin/main --format json
```

This returns documents whose `TrackedFiles` globs match changed files.
Use it to:
- Run targeted `supersigil status <id>` on each affected doc.
- Flag PRs that change tracked source files without updating specs.
- Detect stale specs before they reach the main branch.

### Structured Output for Annotations

Use `--format json` for machine-readable output that can drive GitHub
annotations, GitLab code quality reports, or custom dashboards:

```bash
supersigil verify --format json
```

The JSON output includes finding severity, affected document IDs, and
human-readable messages suitable for inline PR comments.

## PR Review Workflow

When reviewing a PR against Supersigil specs:

1. Run `supersigil affected --since origin/main` to identify which
   spec documents are touched by the change.

2. For each affected document, run `supersigil status <id>` to check
   current health: coverage, staleness, and status consistency.

3. Run `supersigil verify` to get the full finding set. Focus on:
   - New error-level findings introduced by the PR.
   - Coverage gaps: criteria without `VerifiedBy` evidence.
   - Stale tracked files: source changes not reflected in specs.
   - Status inconsistencies: tasks marked done but sibling docs
     not promoted.

4. Check that new code includes appropriate `supersigil: {tag}`
   comments or that `VerifiedBy` file-glob paths cover new test files.

5. If the PR introduces new behavior without spec coverage, flag it.
   New criteria should exist before or alongside the implementation,
   not after.

## Interpreting Findings

Supersigil has two layers of errors:

**Graph-build errors** (broken refs, cycles, duplicate IDs) are always
fatal. They prevent the graph from loading and cause `verify` to exit
non-zero regardless of document status. These are not affected by draft
gating or severity configuration.

**Verification findings** have configurable severity and are affected
by document status:

| Finding type | Default severity (non-draft) | CI action |
|--------------|------------------------------|-----------|
| Missing verification evidence | error | Block merge |
| Missing test files | error | Block merge |
| Status inconsistency | warning | Review required |
| Stale tracked files | warning | Review required |
| Orphan test tag | warning | Review required |
| Orphan decision | warning | Review required |

**Draft gating:** When a document is `status: draft`, all its
verification findings are unconditionally downgraded to `info`.
This means draft documents will not block CI even if they have
coverage gaps or status inconsistencies. Only graph-build errors
(broken refs, cycles, duplicates) remain fatal on draft documents.

## Coverage Reporting

Use `supersigil plan` to generate a coverage report showing outstanding
criteria and pending tasks:

```bash
supersigil plan --format json
```

This can feed into a coverage dashboard or be included as a PR comment
summarizing what work remains for a feature.

## Failure Modes

- Do not treat `supersigil verify` passing as proof that the code is
  correct. It proves the spec graph is internally consistent and that
  declared evidence exists. It does not run tests.
- Do not ignore `affected` output. Changed tracked files that do not
  trigger spec updates are a drift signal.
- Do not block PRs on info-level findings. They exist for iterative
  authoring awareness, not enforcement.

## Handoff

If CI reveals broken or missing specs, suggest `feature-specification`
or `retroactive-specification` to repair them.
If CI reveals implementation gaps (criteria without evidence), suggest
`feature-development` to close the coverage.
