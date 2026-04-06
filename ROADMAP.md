# Roadmap / Open Questions

- **Editor extensions**: Neovim and Zed extensions that
  surface the full LSP feature set. VS Code and IntelliJ extensions are implemented.

- **Watch mode**: `supersigil verify --watch` for continuous feedback
  during authoring. Requires file watching and incremental verification
  (re-verify only documents whose files or dependencies changed).

- **MCP server**: `supersigil mcp` exposing context, verify,
  affected, and plan as MCP tools for direct agent integration
  without shelling out to the CLI.

- **Additional ecosystem plugins**: Language-specific plugins for
  Python, Go, and others — each understanding native test frameworks
  for automatic evidence discovery (like the Rust plugin's
  `#[verifies(...)]` attribute and the JS/TS plugin's `verifies()`
  helper).

- **Ecosystem plugin improvements**: Make evidence from tests more
  visible and useful:
  - **Test body rendering**: The spec browser could pull in and render
    linked test source alongside the criteria it verifies, making tests
    serve as live examples without execution machinery in supersigil.
  - **Convention-based mapping**: Auto-map test names to criteria
    (e.g., `test_auth_session_expiry` → `auth/req#session-expiry`)
    so simple cases need no annotation at all.
  - **Doctest evidence**: Parse `cargo test --doc` output so Rust
    doctests become first-class verification evidence.

- **WASM plugins**: For verification rules that need more than the
  current built-in ecosystem plugins, WASM plugins (via Extism or similar) could
  provide sandboxed, cross-language extensibility. Not planned for v1.

- **CI integrations**: First-party GitHub Action
  (`supersigil/setup-action`) and GitLab CI docker image for
  streamlined CI setup. Structured output formats for GitHub
  annotations and GitLab code quality reports.

- **Show info findings in terminal**: `supersigil verify --show-info`
  to include info-level findings (draft-gated downgrades) in terminal
  output. Currently info findings are suppressed in terminal mode but
  hinted at in the summary, which is confusing. The flag would render
  them dimmed or with an `[info]` prefix.

- **Verify auto-fix**: `supersigil verify --fix` to automatically
  correct simple structural issues (missing attributes, ID
  formatting).
