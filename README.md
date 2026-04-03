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

## Principles

- **Everything-as-code.** Specs are Markdown files in your repository,
  with structured components in `supersigil-xml` fenced code blocks. No
  separate system of record.

- **Verifiable by default.** Cross-references are typed and checked.
  Criterion-to-test mappings are discovered and reported. Staleness,
  orphans, and coverage gaps surface as warnings and errors.

- **Workflow-agnostic.** Write requirements first, or design first, or
  start with the criterion you care about.

## Quick start

```sh
# Create a config file
supersigil init

# Scaffold a requirements doc
supersigil new requirements my-feature

# Verify everything
supersigil verify
```

## Commands

```
supersigil verify              # Cross-document verification
supersigil lint                # Per-file structural checks (fast)
supersigil ls                  # List all documents
supersigil context <id>        # Agent-friendly view of a document
supersigil plan [id]           # Outstanding work overview
supersigil status [id]         # Coverage and staleness summary
supersigil affected --since <ref>  # Docs affected by file changes
supersigil schema              # Component and type definitions
supersigil graph               # Document dependency graph (Mermaid/Graphviz)
supersigil refs                # List criterion refs
supersigil new <type> <id>     # Scaffold a new spec document
supersigil init                # Create supersigil.toml and install agent skills
supersigil skills install      # Install or update agent skills
supersigil import --from kiro  # Import from Kiro format
supersigil examples            # List executable examples
supersigil explore             # Interactive graph explorer (browser)
```

## Configuration

A minimal `supersigil.toml`:

```toml
paths = ["specs/**/*.md"]
```

For monorepos, use the `[projects]` table:

```toml
[projects.backend]
paths = ["services/api/specs/**/*.md"]

[projects.frontend]
paths = ["apps/web/specs/**/*.md"]
```

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
Test files / executable examples
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

Install the **Supersigil** extension from `editors/vscode/`. It
activates automatically when a workspace contains `supersigil.toml` and
discovers the `supersigil-lsp` binary from your `$PATH`,
`~/.cargo/bin/`, or `~/.local/bin/`.

Features:
- Inline diagnostics (parse errors, broken refs, coverage gaps)
- Go-to-definition for cross-references
- Autocomplete for document IDs, criterion IDs, and component attributes
- Hover tooltips with document context and clickable links
- Status bar indicator with server health
- Commands: **Supersigil: Verify**, **Restart Server**, **Show Status**

Configure a custom server path with `supersigil.lsp.serverPath` if
needed.

### Other editors

Any editor with LSP support can use `supersigil-lsp` directly. Point
your editor's LSP client at the binary and register it for Markdown files.

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
  supersigil-cli/          # CLI entry point
editors/
  vscode/                  # VS Code extension
  intellij/                # IntelliJ plugin
```
