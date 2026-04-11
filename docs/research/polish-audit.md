# Polish Audit

*April 2026 — v0.2.0*

UX gaps, rough edges, and improvement opportunities across the CLI, editors,
documentation, and onboarding experience.

## Init and Onboarding

### Skills Installation Feedback

`supersigil skills install` prints `Installed 6 skills to {path}` — count
and path only, no individual skill names. The `init` command does print a
chooser guide listing all 6 skills with descriptions after installation,
which is good. The remaining gap: `skills install` (standalone) doesn't
show the chooser, and there's no `skills list` or `skills check` command
to inspect what's installed or whether skills are up to date.

### No Skills Listing/Check Command

No way to check which skills are installed, whether they match the current
binary version, or diff against embedded versions. `supersigil skills list`
(show installed) or `supersigil skills check` (compare against embedded)
would help users after upgrades.

## New Command

### Template Comments

HTML comments in scaffolds don't render in Markdown previewers. Use
Markdown-visible guidance or make the scaffold verify-clean out of the box.

## LSP and Editor Integration

### Binary Resolution Transparency

The VS Code extension doesn't indicate *which* binary was found on success.
Only failures produce messages.

**Suggestion:** Log `Supersigil LSP: using /path/to/supersigil-lsp` to
the output channel.

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

## Documentation Gaps

### Missing Pages

- **Troubleshooting / FAQ**: "0 documents found" (wrong paths), "LSP not
  starting" (binary not in PATH), common verify errors explained.
- **Migration guide**: Changes between versions, upgrade path.
- **Large project guide**: Performance, multi-project best practices,
  `--project` filtering, monorepo spec organization.
- **Custom components guide**: When/how to define custom components.
- **Common errors reference**: Each verify rule explained with fix examples.

### Existing Page Improvements

- Editor setup guide lacks troubleshooting section
- No performance characteristics documented
- Multi-project pitfalls not covered (cross-project refs, isolated mode)

## Context and Status Commands

### Context Missing Verification State

The `context` command shows document structure and relationships but not
whether criteria are verified. Adding coverage status per criterion would
make it much more useful for planning.

### Status Output Sparseness

`status` with no argument shows project-wide counts but no actionable
direction. A hint like "Run 'supersigil plan' for outstanding work" would
bridge the gap.

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
- No `--doc-type` filter (e.g., show only refs from requirements docs).
- Body text truncation at 72 chars is hardcoded — no way to control width.

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
