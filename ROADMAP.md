# Roadmap / Open Questions

- **LSP: Code Actions / Quick Fixes**: Attach actionable fixes to
  diagnostics — add missing required attributes, create referenced
  documents, fix broken refs. Turns read-only warnings into one-click
  fixes via `textDocument/codeAction`.

- **LSP: Code Lenses**: Show inline metadata above components:
  reference counts ("3 references"), verification status ("verified by
  2 tests"), coverage percentage. Makes verification status visible
  without running the CLI. Via `textDocument/codeLens`.

- **LSP: Rename**: Rename a document ID or component ID and update all
  references across the spec tree via `textDocument/rename` and
  `textDocument/prepareRename`. Refactoring safety net for spec
  evolution.

- **VS Code: Spec Explorer Tree View**: A sidebar panel showing the
  document graph — features grouped by type (requirements, properties,
  design, tasks) with status icons and coverage indicators. Clicking
  navigates to the file. Requires a `TreeDataProvider` in the extension.

- **Editor extensions**: Neovim, JetBrains, and Zed extensions that
  surface the full LSP feature set. VS Code extension is implemented.

- **Watch mode**: `supersigil verify --watch` for continuous feedback
  during authoring. Requires file watching and incremental verification
  (re-verify only documents whose files or dependencies changed).

- **MCP server**: `supersigil mcp` exposing context, verify,
  affected, and plan as MCP tools for direct agent integration
  without shelling out to the CLI.

- **Additional ecosystem plugins**: Language-specific plugins for
  TypeScript, Python, Go, and others — each understanding native
  test frameworks for automatic evidence discovery (like the Rust
  plugin's `#[verifies(...)]`).

- **WASM plugins**: For verification rules that need more than
  stdin/stdout hooks, WASM plugins (via Extism or similar) could
  provide sandboxed, cross-language extensibility. Not planned for v1.

- **Spec rendering**: Render spec documents as browsable
  documentation (e.g., an Astro/Starlight integration that shows
  criteria, coverage badges, and graph relationships inline).

- **CI integrations**: First-party GitHub Action
  (`supersigil/setup-action`) and GitLab CI docker image for
  streamlined CI setup. Structured output formats for GitHub
  annotations and GitLab code quality reports.

- **Show info findings in terminal**: `supersigil verify --show-info`
  to include info-level findings (draft-gated downgrades) in terminal
  output. Currently info findings are suppressed in terminal mode but
  hinted at in the summary, which is confusing. The flag would render
  them dimmed or with an `[info]` prefix.

- **Lint auto-fix**: `supersigil lint --fix` to automatically
  correct simple structural issues (missing attributes, ID
  formatting).

- **Distribution**: Homebrew tap, npm wrapper package, and
  pre-built binaries for Linux and macOS to complement
  `cargo install`.
