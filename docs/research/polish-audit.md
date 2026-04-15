# Polish Audit

*April 2026 — v0.2.0*

UX gaps, rough edges, and improvement opportunities across the CLI, editors,
documentation, and onboarding experience.

## LSP and Editor Integration

### Graph Explorer Initial Load

The graph explorer's first paint is heavier than the spec list in both
IntelliJ and VS Code. Today both editors fetch `graphData`, then wait for a
full `documentComponents` / `renderData` batch before the first mount. That
means larger workspaces pay the full per-document hydration cost up front,
even when the user only needs the graph shell at first.

This is not an IntelliJ-only regression. The current architecture is shared in
practice across both editor integrations, so the fix should be designed once
and applied to both editors rather than patched ad hoc in one host.

**Likely follow-up options:**
- Two-phase load: mount immediately from `graphData`, then push a second update
  once `renderData` finishes.
- True lazy hydration: fetch `documentComponents` only for the selected
  document, or incrementally in the background.

The first option is the smaller change. The second is the more principled
design, but it likely requires a shared update model in the explorer modules
rather than a host-only tweak.

## Config Editing Experience

No JSON Schema exists for `supersigil.toml`. Users editing config in VS Code
or IntelliJ get no autocomplete, no validation, no hover docs. A published
JSON Schema (or TOML-compatible equivalent) would make config authoring much
smoother. The `schema` command exists for component schemas but not the
config file itself. A `supersigil schema --config` could emit this.

## Windows Support

Not currently built or tested. CI targets macOS and Linux (musl) only. This
is intentional for now — Windows support is planned once hardware is
available. Binary size is 5-7 MB (with LTO and stripping), which is
reasonable for all platforms.

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
