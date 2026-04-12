# Polish Audit

*April 2026 — v0.2.0*

UX gaps, rough edges, and improvement opportunities across the CLI, editors,
documentation, and onboarding experience.

## LSP and Editor Integration

### Version Mismatch Detection

No warning when the LSP server version doesn't match the extension version.

### Code Action Gaps

- No action for "add a Criterion to satisfy coverage gap"
- Sequential ID gap warning has no renumbering action
- Missing attribute insertion uses empty placeholders instead of
  context-aware defaults (e.g., status enum values)

### Completion Prioritization

Autocomplete for `refs=` shows all document IDs without prioritization.
`<Implements>` should prefer requirement docs. `<References>` should prefer
same-project docs. Higher relevance first.

### IntelliJ Marketplace

A `publish-intellij` workflow exists but is currently commented out in the
release pipeline. The plugin is not yet published on the JetBrains
Marketplace. Users must build from source to install.

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

## Import Command

The import pipeline (`supersigil import --from kiro`) is well-engineered
and the output is verify-clean. The core UX concern is the TODO markers:
12+ distinct marker types are generated for ambiguities (unresolvable refs,
non-requirement targets, duplicate IDs, non-numeric ranges, optional task
markers, etc.). This is the right design — conservative, marks ambiguity
rather than guessing — but users need guidance on resolving them.

**Polish issues:**
- No user-facing documentation on the website. No "How to Import from Kiro"
  guide, no examples of before/after, no migration checklist.
- The TODO markers are HTML comments that don't render in Markdown preview.
  Users must read raw source to find them.
- No `--check` or `--lint` mode to count TODO markers in previously imported
  files (to track resolution progress).
- No guidance on resolving specific marker types. A troubleshooting table
  mapping each TODO pattern to recommended action would help.
- Partial write failure: if import fails midway, partial files are not rolled
  back. `--force` re-run recovers, but this isn't documented.
- Summary shows `ambiguity_count` but doesn't break down by type.

## Refs Command

The `refs` command is well-implemented with context-aware scoping (filters
refs by TrackedFiles matching the cwd, follows Implements relationships).
Terminal output is a clean aligned table; JSON output has structured fields.

**Polish issues:**
- The context scoping hint goes to stderr (`Showing refs scoped to: ...`),
  which is correct but may be missed in piped usage.

## Render Command

The `render` command produces structured data for the spec renderer (preview
package). The name suggests it renders HTML, but it actually emits JSON data
that a separate renderer consumes.

**Suggestion:** Consider renaming to `supersigil render-data` or
`supersigil export` to avoid the implication that it produces rendered
output. Alternatively, keep the name but improve `--help` to clarify that
it emits renderer input data, not rendered documents.

## Explore Command

### No Headless Fallback

On headless/remote systems, `open` fails with a cryptic error. Should detect
headless environments and suggest `--output file.html` instead.

### No Verification Overlay

The graph explorer shows document relationships but not verification status.
Green/red/yellow badges on nodes would make it immediately useful for
assessing project health at a glance.
