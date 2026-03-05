# Phase 4a CLI Design — Tier 1 Cut

## Scope

5 commands for dogfooding: `import`, `ls` (alias `list`), `lint`, `context`, `plan`.
Binary name: `supersigil`.

Not in scope: `init`, `new`, `schema`, `graph`, `verify`, `status`, `affected`.
Those are deferred to tier 2 (rest of 4a) and phase 4b.

## Crate layout

New workspace member: `crates/supersigil-cli`.

```
crates/supersigil-cli/
├── Cargo.toml
├── src/
│   ├── main.rs          — clap derive, dispatch, exit codes
│   ├── discover.rs      — config path globs → Vec<PathBuf>
│   ├── loader.rs        — parse_all() and load_graph() orchestration
│   ├── format.rs        — OutputFormat enum, terminal vs json writers
│   ├── error.rs         — CliError enum
│   └── commands/
│       ├── import.rs    — maps flags → ImportConfig, delegates to supersigil_import
│       ├── ls.rs        — filters graph.documents(), formats list
│       ├── lint.rs      — per-file parse, reports structural errors
│       ├── context.rs   — graph.context(id) → format
│       └── plan.rs      — PlanQuery::parse → graph.plan → format
```

## Module responsibilities

### `discover.rs`

Resolves glob patterns from `Config.paths` (single-project) or per-project
`ProjectConfig.paths` (multi-project) relative to the config file's parent
directory. Returns `Vec<PathBuf>` of matching files. Uses the `glob` crate.

Testable in isolation with temp directories.

### `loader.rs`

Two entry points:

- `parse_all(config_path) → Result<(Config, Vec<SpecDocument>, Vec<ParseError>), CliError>`
  Discover files + parse each with `supersigil_parser::parse_file`. Non-supersigil
  files (returning `ParseResult::NotSupersigil`) are silently skipped. Used by `lint`.

- `load_graph(config_path) → Result<(Config, DocumentGraph), CliError>`
  Calls `parse_all`, then `supersigil_core::build_graph`. Parse errors and graph
  errors are fatal (exit 1).

Both find `supersigil.toml` by searching upward from the current directory
(or accept an explicit `--config` flag).

### `format.rs`

`OutputFormat { Terminal, Json }` parsed from `--format` flag values.

Helpers:
- `write_json<T: Serialize>(stdout, &T)` — pretty-printed JSON to stdout.
- Terminal formatting: plain structured text. No colors, no Unicode symbols
  in tier 1 (deferred to phase 4b polish).

### `error.rs`

Single `CliError` enum covering all failure modes:

| Variant | Source |
|---|---|
| `ConfigNotFound` | no `supersigil.toml` found |
| `Config(Vec<ConfigError>)` | config validation failures |
| `Parse(Vec<ParseError>)` | file parse failures |
| `Graph(Vec<GraphError>)` | graph construction failures |
| `Query(QueryError)` | context/plan query failures |
| `Import(ImportError)` | import failures |
| `Io(std::io::Error)` | filesystem errors |

All variants format diagnostics to stderr. Exit code is 1 for all errors.

## Command details

### `lint`

Per-file structural checks. Does **not** build the cross-document graph.

1. Call `loader::parse_all()`.
2. Files that parsed successfully: reported as clean (or silently passed).
3. Files with parse errors: errors formatted to stdout as a structural report.
4. Non-supersigil files: silently skipped.

Exit 0 if no structural errors, 1 if any.

Design doc reference: lines 896-911 — lint is per-file, does not build graph.

### `ls` (alias `list`)

```
supersigil ls [--type <doc_type>] [--status <status>] [--project <project>] [--format <terminal|json>]
```

1. Call `loader::load_graph()`.
2. Filter `graph.documents()` by optional `--type`, `--status`, `--project`.
3. Terminal mode: one line per document (`id  type  status  path`).
4. JSON mode: array of document objects.

Exit 0 always (empty result set is success, not an error).

### `context <id>`

```
supersigil context <id> [--format <terminal|json>]
```

1. Call `loader::load_graph()`.
2. Call `graph.context(id)`.
3. Terminal mode: follows the example output in the design doc (lines 1083-1114).
4. JSON mode: serializes `ContextOutput`.

Exit 0 success, 1 if document not found.

### `plan [id_or_prefix]`

```
supersigil plan [<id_or_prefix>] [--format <terminal|json>]
```

1. Call `loader::load_graph()`.
2. Call `PlanQuery::parse(input, &graph)` then `graph.plan(&query)`.
3. Terminal mode: follows the example output in the design doc (lines 1124-1148).
4. JSON mode: serializes `PlanOutput`.

Exit 0 success, 1 if no matching documents.

### `import --from kiro`

```
supersigil import --from kiro [--dry-run] [--output-dir <path>] [--prefix <id_prefix>] [--force]
```

1. Map CLI flags to `ImportConfig`.
2. `--dry-run`: call `plan_kiro_import()`, format the plan to stdout.
3. Without `--dry-run`: call `import_kiro()`, format the result to stdout.
4. Diagnostics (skipped dirs, warnings) to stderr.

Exit 0 success (even with ambiguity markers), 1 on fatal error.

## Changes to existing crates

### supersigil-core

Add `#[derive(Serialize)]` to output and data types:

- `ContextOutput`, `PlanOutput`, `CriterionContext`, `DocRef`, `TaskInfo`,
  `OutstandingCriterion`, `IllustrationRef`
- `SpecDocument`, `ExtractedComponent`, `SourcePosition`
- `ComponentDef`, `AttributeDef`, `DocumentTypeDef`

`Frontmatter` already has `Serialize`.

### supersigil-import

Add `#[derive(Serialize)]` to:

- `ImportPlan`, `ImportResult`, `ImportSummary`, `PlannedDocument`,
  `OutputFile`, `Diagnostic`

## Dependencies

```toml
[package]
name = "supersigil-cli"
version = "0.1.0"

[[bin]]
name = "supersigil"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde_json = "1"
glob = "0.3"
supersigil-core = { path = "../supersigil-core" }
supersigil-parser = { path = "../supersigil-parser" }
supersigil-import = { path = "../supersigil-import" }
serde = { workspace = true }
```

## Exit code contract

- `0` — command completed successfully (including empty results)
- `1` — command failed (config/parse/graph/import/query errors)
- `2` — reserved for phase 4b warning policy; not emitted by tier 1

## stdout/stderr discipline

- **stdout**: command output only (data, reports, lists)
- **stderr**: diagnostics, error messages

This ensures piping works: `supersigil ls | grep draft`.

## Testing strategy (TDD)

Tests follow the acceptance matrix from `docs/phase4a-cli-command-contract.md`:

- **Unit tests**: `discover.rs` (glob resolution), `format.rs` (serialization).
- **Integration tests**: Run the binary via `assert_cmd` against fixture
  directories with known `.mdx` files and `supersigil.toml` configs.
  Verify stdout content, stderr presence, and exit codes.

Key scenarios from the contract:
- A1: clap parses all tier 1 subcommands; unknown commands rejected.
- A3: data on stdout, diagnostics on stderr.
- A4: plan query modes (exact ID, prefix, all; JSON mode).
- A5: context task ordering (topological).
- A8-A12: import flag behaviors.
