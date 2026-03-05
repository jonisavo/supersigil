# Phase 4a CLI Command Contract (Pre-Implementation)

## 1) Scope and Non-Scope

### Scope (Phase 4a)

- Implement `clap` command wiring for: `init`, `lint`, `ls` (`list` alias), `schema`, `new`, `context`, `plan`, `graph`, and `import`.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1970-1972`, `/home/joni/.local/src/supersigil/supersigil-design.md:1450-1469`
- Maintain stdout/stderr discipline and stable machine-readable output modes where applicable.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1066-1067`, `/home/joni/.local/src/supersigil/supersigil-design.md:1973-1974`
- Include Kiro import dogfooding path via `supersigil import --from kiro` with bootstrap import flags.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1468-1469`, `/home/joni/.local/src/supersigil/supersigil-design.md:1984-2004`

### Non-scope (Phase 4a)

- Do not implement `verify`, `status`, or `affected` command wiring yet.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1976-1977`
- Do not finalize terminal polish and full exit-code policy in 4a.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1978-1980`

### 4a Exit Code Contract (concrete bootstrap policy)

- `0`: command completed successfully.
- `1`: command failed (usage/config/parse/graph/import fatal errors).
- `2`: reserved for phase 4b warning policy; not emitted by 4a commands.

Rationale for reserving `2`: final warning exit policy is deferred to phase 4b.  
Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1978-1980`

## 2) Clap Command Grammar

```text
supersigil <COMMAND>

COMMAND :=
    init
  | lint
  | ls [--type <doc_type>] [--status <status>] [--project <project>] [--format <terminal|json>]
  | list [--type <doc_type>] [--status <status>] [--project <project>] [--format <terminal|json>]
  | schema [--format <json|yaml>]
  | new <type> <id>
  | context <id> [--format <terminal|json>]
  | plan [<id_or_prefix>] [--format <terminal|json>]
  | graph [--format <mermaid|dot>]
  | import --from kiro [--dry-run] [--output-dir <path>] [--prefix <id_prefix>] [--force]
```

Normative basis:

- Command set and examples: `/home/joni/.local/src/supersigil/supersigil-design.md:1443-1469`
- `ls` alias `list`: `/home/joni/.local/src/supersigil/supersigil-design.md:1450`
- `plan` input modes and JSON mode: `/home/joni/.local/src/supersigil/supersigil-design.md:1150-1159`, `/home/joni/.local/src/supersigil/supersigil-design.md:1457-1459`
- `schema` JSON/YAML: `/home/joni/.local/src/supersigil/supersigil-design.md:1189-1193`
- `graph` formats: `/home/joni/.local/src/supersigil/supersigil-design.md:1461-1462`
- `import` flags (`--dry-run`, `--output-dir`, `--prefix`): `/home/joni/.local/src/supersigil/supersigil-design.md:1823-1831`, `/home/joni/.local/src/supersigil/supersigil-design.md:2003-2004`

## 3) Per-Command Contract Table (Phase 4a)

| Command | Args | Defaults | Stdout contract | Stderr contract | Exit behavior |
|---|---|---|---|---|---|
| `init` | none | Writes `./supersigil.toml` | Single success line with created path | IO/conflict diagnostics | `0` success, `1` failure |
| `lint` | none | n/a | Structural check report only (no verify engine rules) | Parse/config/graph diagnostics | `0` clean structural checks, `1` structural failures |
| `ls` / `list` | `--type`, `--status`, `--project`, `--format <terminal\|json>` | no filters; `terminal` | Filtered document list rows (or JSON array) | Config/parse/graph diagnostics | `0` success (incl. empty result), `1` failure |
| `schema` | `--format <json\|yaml>` | `json` | Component + document type schema payload only | Serialization/config diagnostics | `0` success, `1` failure |
| `new` | `<type> <id>` | `status=draft` in front matter | Created file path(s) | Invalid type, path conflict, IO diagnostics | `0` success, `1` failure |
| `context` | `<id>`, `--format <terminal\|json>` | `terminal` | Structured context for one document; includes linked tasks in topo order | Document-not-found and pipeline diagnostics | `0` success, `1` failure |
| `plan` | `[<id_or_prefix>]`, `--format <terminal\|json>` | query=`all`; `terminal` | Outstanding work for document/prefix/all, JSON mode for orchestration | No-match and pipeline diagnostics | `0` success, `1` failure |
| `graph` | `--format <mermaid\|dot>` | `mermaid` | Graph serialization only | Parse/graph diagnostics | `0` success, `1` failure |
| `import` | `--from kiro` + import flags | `--output-dir specs/`; no prefix; `force=false`; `dry-run=false` | Import report: plan in dry-run mode, write result in write mode | Fatal import errors + non-fatal diagnostics stream | `0` success (even with ambiguities), `1` fatal error |

Normative anchors for command behavior:

- `init`: create config defaults.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1443`
- `lint`: structural checks only.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1449`
- `ls` filters and alias.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1450-1453`
- `context` purpose and task ordering.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1075-1080`, `/home/joni/.local/src/supersigil/supersigil-design.md:1456`
- `plan` query modes and JSON mode.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1150-1159`, `/home/joni/.local/src/supersigil/supersigil-design.md:1457-1459`
- `schema` format contract.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1189-1193`, `/home/joni/.local/src/supersigil/supersigil-design.md:1460`
- `graph` format contract.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1461-1462`
- `import` command presence and dry-run example.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1468-1469`
- stdout/stderr discipline.  
  Source: `/home/joni/.local/src/supersigil/supersigil-design.md:1066-1067`, `/home/joni/.local/src/supersigil/supersigil-design.md:1973-1974`

## 4) Import-Specific Contract (`--from kiro`)

### Flag-level contract

| Flag | Contract |
|---|---|
| `--from kiro` | Required selector for this importer mode in 4a. Other `--from` values are rejected with usage error. |
| `--dry-run` | Must call planning path and perform zero writes. Returns full `ImportPlan` report with docs, ambiguity count, mapping summary. |
| `--output-dir <path>` | Output base directory for generated files. Defaults to `specs/` if omitted. |
| `--prefix <id-prefix>` | Prefix for generated document IDs. Trailing slash is stripped before ID construction. |
| `--force` | Write mode only. If set, existing files are overwritten; if not set, existing output path triggers `FileExists` error. |

Normative sources:

- CLI import flags and defaults: `/home/joni/.local/src/supersigil/supersigil-design.md:1823-1831`, `/home/joni/.local/src/supersigil/supersigil-design.md:2003-2004`
- Dry-run plan requirement: `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:184-188`
- Write path/default/force semantics: `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:195-199`
- Prefix construction and trailing slash stripping: `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:207-210`
- API shape (`ImportConfig`, `import_kiro`, `plan_kiro_import`): `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:227-230`, `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/design.md:92-113`

### Error/diagnostic contract

- Fatal errors: `SpecsDirNotFound`, `Io`, `FileExists` -> command fails with exit `1`.  
  Source: `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/design.md:767-770`
- Ambiguity markers and diagnostics are non-fatal; import may still succeed.  
  Source: `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/design.md:773-783`, `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:174-177`

## 5) API Mapping (Current Crates) and Gaps

### Existing APIs usable now

| CLI command | Current API mapping | Evidence |
|---|---|---|
| `import` | `supersigil_import::ImportConfig`, `plan_kiro_import`, `import_kiro` | `/home/joni/.local/src/supersigil/crates/supersigil-import/src/lib.rs:12-17`, `/home/joni/.local/src/supersigil/crates/supersigil-import/src/lib.rs:80-123` |
| `context` | `DocumentGraph::context(id)` | `/home/joni/.local/src/supersigil/crates/supersigil-core/src/graph.rs:219-221` |
| `plan` | `PlanQuery::parse(...)` + `DocumentGraph::plan(...)` | `/home/joni/.local/src/supersigil/crates/supersigil-core/src/graph/query.rs:111-155`, `/home/joni/.local/src/supersigil/crates/supersigil-core/src/graph.rs:228-229` |
| `lint`/`ls`/`context`/`plan`/`graph` pipeline core | `load_config` + `parse_file` + `build_graph` + graph accessors | `/home/joni/.local/src/supersigil/crates/supersigil-core/src/config.rs:257-320`, `/home/joni/.local/src/supersigil/crates/supersigil-parser/src/lib.rs:69-125`, `/home/joni/.local/src/supersigil/crates/supersigil-core/src/graph.rs:246-320`, `/home/joni/.local/src/supersigil/crates/supersigil-core/src/graph.rs:116-169` |
| `schema` | Built-in component definitions and config document type model are available | `/home/joni/.local/src/supersigil/crates/supersigil-core/src/component_defs.rs:21-220`, `/home/joni/.local/src/supersigil/crates/supersigil-core/src/config.rs:189-220` |

### Identified gaps to implement in `supersigil-cli`

| Gap | Why it matters |
|---|---|
| No CLI crate/workspace member exists yet | Phase 4a requires `supersigil-cli` command wiring. Existing workspace has only core/parser/import crates. |
| No shared document discovery loader for normal spec files | `parse_file` is per-file; CLI still needs `paths`-based file discovery + pipeline orchestration. |
| No graph export formatter (`mermaid`/`dot`) | `graph` command contract requires serialized graph output formats. |
| No scaffold/init helper APIs | `init` and `new` need template/render/write behavior not exposed today. |
| No command output serializers | 4a requires stable machine-readable modes where applicable; serializers must be added for `ls/context/plan/lint` outputs. |

Evidence:

- Workspace members (no CLI crate): `/home/joni/.local/src/supersigil/Cargo.toml:1-6`
- Per-file parser API: `/home/joni/.local/src/supersigil/crates/supersigil-parser/src/lib.rs:69-125`
- Command expectations for 4a and graph formats: `/home/joni/.local/src/supersigil/supersigil-design.md:1970-1974`, `/home/joni/.local/src/supersigil/supersigil-design.md:1461-1462`

## 6) Minimal Acceptance Test Matrix (for implementation phase)

This matrix is intentionally minimal and TDD-friendly: each row should start as a failing test.

| ID | Scenario | Expected result | Normative source |
|---|---|---|---|
| A1 | `clap` parses all phase 4a subcommands | `init/lint/ls/list/schema/new/context/plan/graph/import` accepted; unknown commands rejected | `/home/joni/.local/src/supersigil/supersigil-design.md:1970-1972`, `/home/joni/.local/src/supersigil/supersigil-design.md:1450-1469` |
| A2 | Scope guard | `verify/status/affected` not wired in 4a | `/home/joni/.local/src/supersigil/supersigil-design.md:1976-1977` |
| A3 | Output channel discipline | Data on stdout; diagnostics/progress on stderr | `/home/joni/.local/src/supersigil/supersigil-design.md:1066-1067`, `/home/joni/.local/src/supersigil/supersigil-design.md:1973-1974` |
| A4 | `plan` query modes | Exact ID, prefix, and no-arg all mode all behave as specified; JSON mode supported | `/home/joni/.local/src/supersigil/supersigil-design.md:1150-1159`, `/home/joni/.local/src/supersigil/supersigil-design.md:1457-1459` |
| A5 | `context` task ordering | Linked tasks appear in topological order | `/home/joni/.local/src/supersigil/supersigil-design.md:1077-1080` |
| A6 | `schema` formats | `--format json|yaml` both valid and structurally stable | `/home/joni/.local/src/supersigil/supersigil-design.md:1189-1193` |
| A7 | `graph` formats | default `mermaid`; `--format dot` works | `/home/joni/.local/src/supersigil/supersigil-design.md:1461-1462` |
| A8 | `import --dry-run` | No files written; plan includes intended docs, ambiguity count, mapping summary | `/home/joni/.local/src/supersigil/supersigil-design.md:1825-1827`, `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:184-188` |
| A9 | `import` write mode + defaults | Writes `{output_dir}/{feature}/{type}.mdx`; default `output_dir=specs/` | `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:195-199`, `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:218-220` |
| A10 | `import --prefix` normalization | Trailing slash stripped; IDs follow `{prefix}/{type}/{feature}` | `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:207-210` |
| A11 | `import --force` conflict behavior | Without `--force`, preexisting file -> `FileExists`; with `--force`, overwrite | `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:197-199`, `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/design.md:769-770` |
| A12 | Import fatal/non-fatal distinction | Fatal errors fail command; ambiguity markers/diagnostics do not | `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/design.md:767-783`, `/home/joni/.local/src/supersigil/.kiro/specs/kiro-import/requirements.md:174-177` |
