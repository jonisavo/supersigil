# Polish Audit

*April 2026 — v0.12.0*

UX gaps, rough edges, and improvement opportunities across the CLI, editors,
documentation, and onboarding experience.

## Config Editing Experience

No JSON Schema exists for `supersigil.toml`. Users editing config in VS Code
or IntelliJ get no autocomplete, no validation, no hover docs. A published
JSON Schema (or TOML-compatible equivalent) would make config authoring much
smoother. The `schema` command exists for component schemas but not the
config file itself. A `supersigil schema --config` could emit this.

## Shared Test File Resolver and .gitignore

`resolve_test_files` uses `expand_globs` (the `glob` crate) which does not
honor `.gitignore`. Broad patterns like `packages/**/*.test.ts` can match
files inside `node_modules` or `dist` if those directories exist on disk.
This affects all ecosystem plugins equally — both Rust and JS test discovery
go through the same shared baseline.

The old JS plugin used `ignore::WalkBuilder` which respected `.gitignore`
automatically, but that was removed when test discovery was unified through
project-level `tests` globs. The Rust plugin never had `.gitignore`
filtering either (it walks hardcoded directories like `tests/` and `src/`).

In practice this rarely causes problems because vendored test files
typically don't contain `verifies()` / `#[verifies]` annotations, so they
produce no evidence. But a file with a syntax error inside `node_modules`
would generate a spurious diagnostic.

**Options:**
- Switch `expand_globs` to use the `ignore` crate instead of `glob`, so all
  test file resolution respects `.gitignore` by default.
- Add a workspace-level `exclude` pattern list to `supersigil.toml`.
- Accept the current behavior and document that `tests` globs should be
  specific enough to avoid ignored directories.
