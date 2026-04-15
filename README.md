<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="media/logo.png" />
    <source media="(prefers-color-scheme: light)" srcset="media/logo_light.png" />
    <img alt="Supersigil" src="media/logo.png" width="400" />
  </picture>
</p>

<p align="center">
  Spec-driven development with AI agents.
</p>

---

Supersigil is a CLI tool and verification framework that turns Markdown spec
files into a verifiable graph of criteria, evidence, and test mappings.
Specs are code: they render as documentation, provide agent context, and
are checked by CI.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="website/public/images/graph-explorer-dark.webp" />
    <source media="(prefers-color-scheme: light)" srcset="website/public/images/graph-explorer-light.webp" />
    <img alt="Image of spec authoring inside Visual Studio Code" src="website/public/images/graph-explorer-dark.webp" width="900" />
  </picture>
</p>

## Principles

- **Everything-as-code.** Specs are Markdown files in your repository,
  with structured components in `supersigil-xml` fenced code blocks. No
  separate system of record.

- **Verifiable by default.** Cross-references are typed and checked.
  Criterion-to-test mappings are discovered and reported. Staleness,
  orphans, and coverage gaps surface as warnings and errors.

- **Workflow-agnostic.** Write requirements first, or design first, or
  start with the criterion you care about. The tool tells you what's
  missing — it doesn't prescribe an order.

## Installation

### Homebrew (macOS / Linux)

```sh
brew install jonisavo/supersigil/supersigil
```

This installs both `supersigil` and `supersigil-lsp`.

### AUR (Arch Linux)

```sh
yay -S supersigil-bin supersigil-lsp-bin   # prebuilt binaries
yay -S supersigil supersigil-lsp           # build from source
```

### Cargo

```sh
cargo install supersigil supersigil-lsp
```

### GitHub Releases

Download prebuilt binaries for macOS (Intel / Apple Silicon) and Linux
(x86_64 / aarch64) from the
[releases page](https://github.com/jonisavo/supersigil/releases).

## Quick start

```sh
# Create a config file
supersigil init

# Scaffold a requirements doc
supersigil new requirements auth

# Verify everything
supersigil verify
```

## Commands

```
supersigil init                    # Create supersigil.toml and install agent skills
supersigil new <type> <id>         # Scaffold a new spec document
supersigil verify                  # Cross-document verification
supersigil ls                      # List all documents
supersigil context <id>            # Agent-friendly view of a document
supersigil plan [id_or_prefix]     # Outstanding work overview
supersigil status [id]             # Coverage and staleness summary
supersigil affected --since <ref>  # Docs affected by file changes
supersigil schema                  # Component and type definitions
supersigil graph                   # Document dependency graph (Mermaid/Graphviz)
supersigil refs                    # List criterion refs
supersigil export                  # Export component trees with verification data
supersigil explore                 # Interactive graph explorer (browser)
supersigil import --from kiro      # Import from Kiro format
supersigil skills install          # Install or update agent skills
supersigil completions <shell>     # Generate shell completions
```

See the [CLI reference](https://supersigil.org/reference/cli/) for
flags and detailed usage, and the
[configuration reference](https://supersigil.org/reference/configuration/)
for `supersigil.toml` options.

## How it works

Spec documents are Markdown files with `supersigil:` front matter.
Structured components (`<Criterion>`, `<VerifiedBy>`, `<Implements>`,
etc.) are written inside `supersigil-xml` fenced code blocks and form a
typed graph that supersigil verifies:

```
Criterion (in requirements doc)
    |
    | <VerifiedBy>              direct evidence
    |
    v
Test files
```

- Requirements define criteria. `<VerifiedBy>` links criteria to test
  evidence. `<Implements>` traces design docs back to criteria.
- References are unidirectional (concrete points to abstract). Reverse
  mappings are computed automatically.
- `status: draft` suppresses warnings so you can work iteratively.
  Hard errors (broken refs, cycles, duplicates) are always fatal.

## Editor integration

The Supersigil LSP server provides real-time feedback in your editor:
diagnostics, go-to-definition, autocomplete for document and criterion
IDs, and hover documentation.

### VS Code

Install the **Supersigil** extension from the
[VS Code Marketplace](https://marketplace.visualstudio.com/publishers/supersigil)
or [Open VSX](https://open-vsx.org/). It activates automatically when a
workspace contains `supersigil.toml` and discovers the `supersigil-lsp`
binary from your `$PATH`, `~/.cargo/bin/`, or `~/.local/bin/`.

Features:
- Inline diagnostics (parse errors, broken refs, coverage gaps)
- Go-to-definition for cross-references
- Autocomplete for document IDs, criterion IDs, and component attributes
- Hover tooltips with document context and clickable links
- Spec Explorer sidebar tree view
- Status bar indicator with server health
- Commands: **Supersigil: Verify**, **Restart Server**, **Show Status**

Configure a custom server path with `supersigil.lsp.serverPath` if
needed.

### IntelliJ

Install the **Supersigil** plugin from the
[JetBrains Marketplace](https://plugins.jetbrains.com/plugin/31213-supersigil)
or search for `Supersigil` in **Settings > Plugins > Marketplace**.

It provides the same LSP-backed diagnostics, navigation, and completion as the
VS Code extension, plus a Graph Explorer tool window. Compatible with IntelliJ
IDEA 2025.3+ and other IntelliJ-based IDEs that include the LSP client module.

### Other editors

Any editor with LSP support can use `supersigil-lsp` directly. Point
your editor's LSP client at the binary and register it for Markdown
files.

## Ecosystem packages

### Rust

The `supersigil-rust` crate provides a `#[verifies("doc-id#criterion-id")]`
attribute macro that links Rust test functions to spec criteria.

```sh
cargo add supersigil-rust
```

### JavaScript / TypeScript

- **[@supersigil/vitest](https://www.npmjs.com/package/@supersigil/vitest)** —
  Vitest helper for annotating tests with criterion refs.
- **[@supersigil/eslint-plugin](https://www.npmjs.com/package/@supersigil/eslint-plugin)** —
  ESLint plugin for validating criterion refs.

```sh
pnpm add -D @supersigil/vitest @supersigil/eslint-plugin
```

## Project structure

```
crates/
  supersigil-core/         # Document model, graph, config
  supersigil-parser/       # Markdown parsing, front matter extraction
  supersigil-verify/       # Verification engine
  supersigil-evidence/     # Language-agnostic evidence primitives
  supersigil-rust/         # Rust ecosystem plugin
  supersigil-rust-macros/  # #[verifies(...)] proc macro
  supersigil-import/       # Kiro import
  supersigil-lsp/          # Language Server Protocol server
  supersigil-js/           # JS/TS ecosystem plugin
  supersigil-cli/          # CLI entry point
dist/
  aur/                     # Arch User Repository build files
  homebrew/                # Homebrew formula template
editors/
  intellij/                # IntelliJ extension
  vscode/                  # VS Code extension
packages/
  eslint-plugin/           # ESLint plugin for Supersigil criterion refs
  preview/                 # Shared JS/CSS rendering assets
  vitest/                  # Vitest helpers for Supersigil criterion refs
```

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
