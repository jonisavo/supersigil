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

Supersigil is a CLI tool and verification framework that turns MDX spec
files into a verifiable graph of criteria, evidence, and test mappings.
Specs are code: they render as documentation, provide agent context, and
are checked by CI.

## Principles

- **Everything-as-code.** Specs are MDX files in your repository. They
  render with Astro, Docusaurus, or any MDX-aware site. No separate
  system of record.

- **Verifiable by default.** Cross-references are typed and checked.
  Criterion-to-test mappings are discovered and reported. Staleness,
  orphans, and coverage gaps surface as warnings and errors.

- **Workflow-agnostic.** Write requirements first, or design first, or
  start with the criterion you care about. The tool tells you what's
  missing — it doesn't prescribe an order.

## Quick start

```sh
# Create a config file
supersigil init

# Scaffold a requirements doc
supersigil new requirements my-feature/req/login

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
supersigil init                # Create supersigil.toml
supersigil import --from kiro  # Import from Kiro format
supersigil examples            # List executable examples
```

## Configuration

A minimal `supersigil.toml`:

```toml
paths = ["specs/**/*.mdx"]
```

For monorepos, use the `[projects]` table:

```toml
[projects.backend]
paths = ["services/api/specs/**/*.mdx"]

[projects.frontend]
paths = ["apps/web/specs/**/*.mdx"]
```

## How it works

Spec documents are MDX files with `supersigil:` front matter. Structured
components (`<Criterion>`, `<VerifiedBy>`, `<Implements>`, etc.) form a
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

See [DECISIONS.md](DECISIONS.md) for architectural rationale.

## Project structure

```
crates/
  supersigil-core/       # Document model, graph, config
  supersigil-parser/     # MDX parsing, front matter extraction
  supersigil-verify/     # Verification engine
  supersigil-evidence/   # Language-agnostic evidence primitives
  supersigil-rust/       # Rust ecosystem plugin
  supersigil-rust-macros/  # #[verifies(...)] proc macro
  supersigil-import/     # Kiro import
  supersigil-cli/        # CLI entry point
```
