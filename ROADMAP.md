# Roadmap / Open Questions

- **LSP support**: A language server providing autocomplete on refs,
  go-to-definition, and diagnostics would significantly improve the
  authoring experience. This is a v2 concern but the parser architecture
  should be designed to support incremental re-parsing.

- **Editor extensions**: VS Code, Neovim, JetBrains, and Zed
  extensions that surface diagnostics, coverage indicators, and
  go-to-definition for spec references. Depends on LSP support.

- **Watch mode**: `supersigil verify --watch` for continuous feedback
  during authoring. Requires file watching and incremental verification
  (re-verify only documents whose files or dependencies changed).

- **Visual graph explorer**: A browser-based interactive visualization
  of the spec graph (`supersigil explore`). Force-directed layout,
  filtering by type/status/coverage, and impact trace highlighting.

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
