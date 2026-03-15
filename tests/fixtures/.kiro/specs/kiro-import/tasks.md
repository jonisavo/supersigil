# Implementation Plan: Kiro Import

## Overview

Build the `supersigil-import` crate following TDD: for each pipeline stage, write property tests first (red), implement to make them pass (green), then refactor. The pipeline is sequential — crate scaffolding → ID generation → ref parsing → discovery → requirements parsing → tasks parsing → design parsing → ref resolution → requirements emission → design emission → tasks emission → import plan → file writing. Each task starts with the property test, then the implementation.

## Tasks

- [x] 1. Set up crate skeleton, module structure, and type stubs
  - [x] 1.1 Create `crates/supersigil-import/` with `Cargo.toml` and module structure
    - Add `supersigil-import` to workspace `Cargo.toml` members
    - Create `Cargo.toml` with dependencies: `regex`, `thiserror`, `serde` from workspace; dev-dependencies: `proptest`, `tempfile`, `insta` (with `yaml` feature)
    - Create `src/lib.rs` with public API types: `ImportConfig`, `ImportResult`, `ImportPlan`, `PlannedDocument`, `OutputFile`, `ImportSummary`, `Diagnostic`, `ImportError`
    - Stub `import_kiro` and `plan_kiro_import` function signatures (return `todo!()`)
    - Create module files following new module syntax: `src/discover.rs`, `src/parse.rs`, `src/parse/requirements.rs`, `src/parse/design.rs`, `src/parse/tasks.rs`, `src/refs.rs`, `src/emit.rs`, `src/emit/requirements.rs`, `src/emit/design.rs`, `src/emit/tasks.rs`, `src/ids.rs`, `src/write.rs`
    - Define internal IR types in parse modules: `ParsedRequirements`, `ParsedRequirement`, `ParsedCriterion`, `ParsedDesign`, `DesignSection`, `DesignBlock`, `RawRef`, `ParsedTasks`, `ParsedTask`, `ParsedSubTask`, `TaskStatus`, `TaskRefs`
    - Define `KiroSpecDir` in `discover.rs`
    - _Requirements: 18.1, 18.2, 18.3_

  - [x] 1.2 Create proptest generators in `tests/generators.rs`
    - `arb_feature_name()` — valid kebab-case feature names
    - `arb_id_prefix()` — optional ID prefixes, some with trailing slashes
    - `arb_requirement_number()` — alphanumeric requirement numbers
    - `arb_criterion_index()` — alphanumeric criterion indices (e.g., `1`, `8a`)
    - `arb_raw_ref()` — `RawRef` values with random requirement numbers and criterion indices
    - `arb_raw_ref_list()` — lists of `RawRef` values
    - `arb_parsed_criterion()` — `ParsedCriterion` with random index and EARS-style text
    - `arb_parsed_requirement()` — `ParsedRequirement` with number, title, user story, criteria
    - `arb_parsed_requirements()` — `ParsedRequirements` with introduction, optional glossary, requirement sections
    - `arb_parsed_task()` — `ParsedTask` with number, title, status, description, sub-tasks
    - `arb_parsed_tasks()` — `ParsedTasks` with preamble and task list
    - `arb_task_status()` — random `TaskStatus` values
    - `arb_prose_block()` — prose paragraphs (no markdown special chars)
    - `arb_code_block()` — fenced code blocks with language tags
    - `arb_mermaid_block()` — mermaid diagram blocks
    - `arb_kiro_requirements_md()` — complete Kiro `requirements.md` string from `ParsedRequirements`
    - `arb_kiro_tasks_md()` — complete Kiro `tasks.md` string from `ParsedTasks`
    - _Requirements: all (generators support all property tests)_

- [x] 2. Checkpoint — Verify crate compiles
  - Ensure `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features`, and `cargo nextest run` all pass. Ask the user if questions arise.

- [x] 3. ID generation (ids.rs)
  - [x] 3.1 Write property test for document ID construction
    - **Property 1: Document ID construction**
    - For any optional ID prefix (with or without trailing slash) and any feature name, `make_document_id` produces the correct ID with prefix stripped of trailing slashes
    - Test file: `tests/prop_ids.rs`
    - **Validates: Requirements 16.1, 16.2, 16.3**

  - [x] 3.2 Write property test for criterion ID generation and uniqueness
    - **Property 2: Criterion ID generation and uniqueness**
    - For any requirement number and criterion index (including alphanumeric like `8a`), `make_criterion_id` produces `req-{N}-{Y}`. Deduplication appends suffixes and emits ambiguity markers on collision.
    - Test file: `tests/prop_ids.rs`
    - **Validates: Requirements 3.1, 3.2, 3.3**

  - [x] 3.3 Write property test for task ID generation and uniqueness
    - **Property 3: Task ID generation and uniqueness**
    - For any task number, `make_task_id` produces `task-{N}` for top-level and `task-{N}-{M}` for sub-tasks. Deduplication handles collisions.
    - Test file: `tests/prop_ids.rs`
    - **Validates: Requirements 9.1, 9.2, 9.3, 9.4**

  - [x] 3.4 Implement ID generation functions in `ids.rs`
    - Implement `make_document_id`, `make_criterion_id`, `make_task_id`, `deduplicate_ids`
    - Strip trailing slashes from ID prefix
    - Handle alphanumeric criterion indices
    - _Requirements: 3.1, 3.2, 3.3, 9.1, 9.2, 9.3, 9.4, 16.1, 16.2, 16.3, 16.4_

- [x] 4. Requirement ref parsing (refs.rs)
  - [x] 4.1 Write property test for ref parsing round-trip
    - **Property 4: Requirement ref parsing round-trip**
    - For any list of `(requirement_number, criterion_index)` pairs, format as `Requirements X.Y, Z.W` and parse with `parse_requirement_refs` — should recover original pairs
    - Test file: `tests/prop_refs.rs`
    - **Validates: Requirements 20.1, 20.2, 20.3, 20.4**

  - [x] 4.2 Write property test for ref range expansion
    - **Property 5: Requirement ref range expansion**
    - For any range `X.Y–X.Z` with numeric Y ≤ Z, parser expands to individual refs. Non-numeric ranges emit ambiguity markers.
    - Test file: `tests/prop_refs.rs`
    - **Validates: Requirements 20.5**

  - [x] 4.3 Write property test for unparseable ref detection
    - **Property 6: Unparseable ref detection**
    - For any reference string with tokens not matching `X.Y` pattern, parser emits ambiguity markers
    - Test file: `tests/prop_refs.rs`
    - **Validates: Requirements 20.6**

  - [x] 4.4 Implement `parse_requirement_refs` in `refs.rs`
    - Handle single refs, comma-separated lists, optional `Requirements` prefix
    - Implement range expansion for numeric indices
    - Emit ambiguity markers for non-numeric ranges and unparseable tokens
    - _Requirements: 20.1, 20.2, 20.3, 20.4, 20.5, 20.6_

- [x] 5. Checkpoint — ID and ref parsing complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Discovery (discover.rs)
  - [x] 6.1 Write property test for discovery
    - **Property 11: Discovery includes valid dirs and skips empty ones**
    - For any directory structure under `.kiro/specs/` with mixed valid/empty subdirectories, `discover_kiro_specs` returns exactly the valid dirs with correct feature names and emits `SkippedDir` diagnostics for empty ones
    - Test file: `tests/prop_parse.rs`
    - **Validates: Requirements 1.1, 1.2, 1.3, 1.5**

  - [x] 6.2 Implement `discover_kiro_specs` in `discover.rs`
    - Scan directory for subdirectories, check for presence of `requirements.md`, `design.md`, `tasks.md`
    - Derive feature name from directory name
    - Return error if `.kiro/specs/` does not exist
    - Emit `SkippedDir` diagnostic for dirs with no recognized files
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

- [x] 7. Requirements parsing (parse/requirements.rs)
  - [x] 7.1 Write property test for requirements parsing completeness
    - **Property 9: Requirements parsing completeness**
    - For any well-formed Kiro `requirements.md` (generated via `arb_kiro_requirements_md`), parser extracts all sections with correct requirement numbers, titles, user stories, criterion texts, and document title
    - Test file: `tests/prop_parse.rs`
    - **Validates: Requirements 2.1, 2.2, 2.3, 2.4, 2.6**

  - [x] 7.2 Implement `parse_requirements` in `parse/requirements.rs`
    - Line-by-line parsing with regex patterns for requirement headings, user stories, acceptance criteria headers, criterion lines
    - Extract document title from `# Requirements Document: Title`
    - Extract introduction text, glossary, requirement sections
    - Handle alphanumeric criterion indices (e.g., `8a`)
    - Emit diagnostic warning for files with no parseable requirement sections
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

- [x] 8. Tasks parsing (parse/tasks.rs)
  - [x] 8.1 Write property test for tasks parsing completeness
    - **Property 10: Tasks parsing completeness**
    - For any well-formed Kiro `tasks.md` (generated via `arb_kiro_tasks_md`), parser extracts all tasks with correct numbers, titles, status markers, descriptions, sub-task nesting, and metadata. Status mapping: `[x]`→Done, `[ ]`→Ready, `[-]`→InProgress, `[~]`→Draft. Both italic and bold metadata forms recognized. Non-ref values produce `TaskRefs::None` or `TaskRefs::Comment`.
    - Test file: `tests/prop_parse.rs`
    - **Validates: Requirements 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8**

  - [x] 8.2 Implement `parse_tasks` in `parse/tasks.rs`
    - Line-by-line parsing with regex patterns for task lines, sub-task lines, metadata lines
    - Extract document title from `# Implementation Plan: Title` or `# Tasks: Title`
    - Handle optional marker (`*` after status bracket)
    - Extract description lines, preamble/notes sections
    - Parse metadata lines in both italic and bold forms
    - Handle `N/A` and non-ref sentinel values
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8, 22.1_

- [x] 9. Design parsing (parse/design.rs)
  - [x] 9.1 Implement `parse_design` in `parse/design.rs`
    - Line-by-line parsing preserving sections, prose, code blocks, mermaid blocks
    - Extract `**Validates: Requirements X.Y**` lines and parse refs
    - Extract document title from `# Design Document: Title` or `# Design: Title`
    - Handle non-requirement Validates targets (preserve as prose, emit ambiguity marker)
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

- [x] 10. Checkpoint — Parsing complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 11. Reference resolution (refs.rs)
  - [x] 11.1 Write property test for Validates reference resolution
    - **Property 7: Validates reference resolution**
    - For any set of `RawRef` values and `ParsedRequirements` with matching entries, `resolve_refs` produces correct criterion ref strings. Unresolvable refs emit ambiguity markers. Mixed lines combine resolvable refs and emit markers for unresolvable ones.
    - Test file: `tests/prop_resolve.rs`
    - **Validates: Requirements 6.1, 6.2, 6.3, 7.4, 7.5**

  - [x] 11.2 Write property test for task implements resolution
    - **Property 8: Task implements resolution**
    - For any task with resolvable `RawRef` values, `implements` attribute contains correct comma-separated criterion ref strings. Unresolvable refs emit ambiguity markers inside `<Task>` body.
    - Test file: `tests/prop_resolve.rs`
    - **Validates: Requirements 11.1, 11.2, 11.3**

  - [x] 11.3 Implement `resolve_refs` in `refs.rs`
    - Map `RawRef` values against `ParsedRequirements` to produce criterion ref strings
    - Format as `{doc_id_base}#req-{X}-{Y}` for resolvable refs
    - Emit ambiguity markers for unresolvable refs
    - Handle mixed resolvable/unresolvable refs in a single Validates line
    - _Requirements: 6.1, 6.2, 6.3, 7.4, 7.5, 11.1, 11.2, 11.3_

- [x] 12. Checkpoint — Resolution complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 13. Requirements MDX emission (emit/requirements.rs)
  - [x] 13.1 Write property test for prose and code block round-trip fidelity
    - **Property 12: Prose and code block round-trip fidelity**
    - For any Kiro spec file containing prose paragraphs, fenced code blocks, and mermaid blocks, the MDX output contains each element verbatim as a substring
    - Test file: `tests/prop_emit.rs`
    - **Validates: Requirements 19.1, 19.2, 19.3, 4.2, 4.5, 5.2, 5.3, 7.3, 12.4, 12.5**

  - [x] 13.2 Write property test for front matter round-trip
    - **Property 13: Front matter round-trip**
    - For all generated MDX documents, parsing the front matter YAML produces correct `id`, `type`, `status` (always `draft`), and `title`. Front matter delimited by `---` lines.
    - Test file: `tests/prop_emit.rs`
    - **Validates: Requirements 4.1, 7.1, 12.1, 21.1, 21.2, 21.3**

  - [x] 13.3 Write property test for AcceptanceCriteria structure
    - **Property 14: AcceptanceCriteria structure**
    - For any parsed requirements with N requirement sections, emitted MDX contains exactly N `<AcceptanceCriteria>` blocks, each with correct `<Criterion>` components
    - Test file: `tests/prop_emit.rs`
    - **Validates: Requirements 4.3, 4.4**

  - [x] 13.4 Implement `emit_requirements_mdx` in `emit/requirements.rs`
    - Emit front matter with `id`, `type: requirement`, `status: draft`, `title`
    - Emit introduction prose, glossary (if present)
    - Emit per-requirement sections with user story prose and `<AcceptanceCriteria>` blocks
    - Each `<Criterion>` gets its generated ID and criterion text
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 21.1, 21.2_

- [x] 14. Design MDX emission (emit/design.rs)
  - [x] 14.1 Write property test for Design Implements emission
    - **Property 15: Design Implements emission**
    - When both requirements and design exist, design MDX contains `<Implements refs="{req_doc_id}" />`. When only design exists, MDX contains ambiguity marker and no `<Implements>`.
    - Test file: `tests/prop_emit.rs`
    - **Validates: Requirements 7.2**

  - [x] 14.2 Implement `emit_design_mdx` in `emit/design.rs`
    - Emit front matter with `id`, `type: design`, `status: draft`, `title`
    - Emit `<Implements>` component when requirements doc exists
    - Preserve all prose, code blocks, mermaid diagrams
    - Emit `<Validates>` components after correctness properties with resolved refs
    - Emit ambiguity markers for unresolvable refs
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 21.1, 21.2_

- [x] 15. Tasks MDX emission (emit/tasks.rs)
  - [x] 15.1 Write property test for task dependency chain
    - **Property 16: Task dependency chain**
    - For any sequence of top-level tasks, each after the first has `depends="{previous_task_id}"`. Same for sub-tasks within a parent. First in any sibling group has no `depends`.
    - Test file: `tests/prop_emit.rs`
    - **Validates: Requirements 10.1, 10.2, 10.3**

  - [x] 15.2 Write property test for task component structure
    - **Property 17: Task component structure**
    - For any parsed tasks, emitted MDX contains `<Task>` components with `id`, `status`, optional `depends` and `implements`. Sub-tasks are nested. Description text appears as body.
    - Test file: `tests/prop_emit.rs`
    - **Validates: Requirements 12.2, 12.3**

  - [x] 15.3 Implement `emit_tasks_mdx` in `emit/tasks.rs`
    - Emit front matter with `id`, `type: tasks`, `status: draft`, `title`
    - Emit preamble prose
    - Emit `<Task>` components with `id`, `status`, `depends`, `implements` attributes
    - Nest sub-tasks within parent `<Task>` components
    - Include task description as body
    - Preserve overview/notes sections as prose
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5, 21.1, 21.2_

- [x] 16. Checkpoint — Emission complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 17. Import plan and ambiguity tracking
  - [x] 17.1 Write property test for ambiguity marker count consistency
    - **Property 18: Ambiguity marker count consistency**
    - For any import result or plan, the reported `ambiguity_count` equals the number of `<!-- TODO(supersigil-import):` occurrences across all generated MDX documents
    - Test file: `tests/prop_plan.rs`
    - **Validates: Requirements 13.3, 14.3**

  - [x] 17.2 Write property test for import plan completeness
    - **Property 19: Import plan completeness**
    - For any import plan, each planned document has non-empty `output_path` and correct `document_id`. Summary reports correct `criteria_converted`, `tasks_converted`, `validates_resolved` counts.
    - Test file: `tests/prop_plan.rs`
    - **Validates: Requirements 14.2, 14.4**

  - [x] 17.3 Implement `plan_kiro_import` in `lib.rs`
    - Orchestrate the full pipeline: discover → parse → resolve → emit
    - Build `ImportPlan` with `PlannedDocument` entries, ambiguity count, and summary
    - No file writing — return plan only
    - _Requirements: 14.1, 14.2, 14.3, 14.4, 18.3_

- [x] 18. File writing (write.rs)
  - [x] 18.1 Write property test for file writing with force semantics
    - **Property 20: File writing with force semantics**
    - When `force` is false and target file exists, `write_files` returns `FileExists` error. When `force` is true, existing files are overwritten. Missing output directories are created.
    - Test file: `tests/prop_write.rs`
    - **Validates: Requirements 15.1, 15.3, 17.1**

  - [x] 18.2 Implement `write_files` in `write.rs`
    - Write each `PlannedDocument` to `{output_dir}/{feature_name}/{feature_name}.{type}.mdx`
    - Create parent directories if needed
    - Check for existing files when `force` is false
    - Best-effort semantics: sequential writes, no rollback on failure
    - _Requirements: 15.1, 15.2, 15.3, 15.4, 15.5, 17.1, 17.2_

  - [x] 18.3 Implement `import_kiro` in `lib.rs`
    - Call `plan_kiro_import` then `write_files`
    - Build `ImportResult` from plan and write results
    - _Requirements: 18.2, 18.4_

- [x] 19. Checkpoint — Core pipeline complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 20. Edge case property tests
  - [x] 20.1 Write property test for non-requirement Validates targets
    - **Property 21: Non-requirement Validates targets produce ambiguity markers**
    - For any `**Validates:**` line referencing a non-requirement target (e.g., `Design Decision 5`), parser preserves line as prose and emits ambiguity marker
    - Test file: `tests/prop_edge.rs`
    - **Validates: Requirements 5.4**

  - [x] 20.2 Write property test for optional task marker handling
    - **Property 22: Optional task marker handling**
    - For any Kiro task line with optional marker (`[x]* 2.1 ...`), task is included in output with ambiguity marker noting optional status
    - Test file: `tests/prop_edge.rs`
    - **Validates: Requirements 22.1**

  - [x] 20.3 Write property test for unparseable structure preservation
    - **Property 23: Unparseable structure preservation**
    - For any task line or structural pattern not matching expected format, importer inserts ambiguity marker and preserves original text verbatim
    - Test file: `tests/prop_edge.rs`
    - **Validates: Requirements 13.2**

- [x] 21. Snapshot tests (tests/snapshots.rs)
  - [x] 21.1 Add `insta` (with `yaml` feature) as a dev-dependency in `crates/supersigil-import/Cargo.toml`
    - Create `tests/snapshots.rs` and `tests/snapshots/` directory
    - _Requirements: all (snapshot tests guard output fidelity across the full pipeline)_

  - [x] 21.2 Write full-pipeline snapshot tests for real Kiro specs
    - Feed `.kiro/specs/parser-and-config/` through `plan_kiro_import` and snapshot each generated MDX document (`parser-and-config.req.mdx`, `parser-and-config.design.mdx`, `parser-and-config.tasks.mdx`)
    - Feed `.kiro/specs/document-graph/` through `plan_kiro_import` and snapshot each generated MDX document
    - Use `insta::assert_snapshot!` for each output document
    - _Requirements: 2.1, 4.1, 5.1, 7.1, 8.1, 12.1_

  - [x] 21.3 Write synthetic snapshot tests for edge cases
    - Design-only feature (no requirements or tasks) → snapshot design MDX with ambiguity marker for missing requirements
    - Tasks with `N/A` metadata → snapshot tasks MDX with `TaskRefs::None` handling
    - Tasks with optional markers (`[x]* 2.1 ...`) → snapshot tasks MDX with ambiguity marker
    - Non-requirement Validates target (`Design Decision 5`) → snapshot design MDX with ambiguity marker
    - _Requirements: 5.4, 7.2, 8.7, 13.2, 22.1_

- [x] 22. Unit tests for examples and edge cases (tests/unit.rs)
  - [x] 22.1 Write unit tests for real-world Kiro spec inputs
    - Parse the existing `.kiro/specs/` directories in this repo as integration tests
    - Verify correct extraction of requirements, design sections, and tasks
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 5.1, 8.1, 8.2, 8.3_

  - [x] 22.2 Write unit tests for edge cases
    - Empty requirements file → diagnostic warning, MDX with raw prose only (Req 2.5)
    - Tasks with no sub-tasks → single-level `<Task>` components
    - Design with no Validates lines → no `<Validates>` components
    - `N/A` and non-ref metadata sentinels → `TaskRefs::None` / `TaskRefs::Comment` (Req 8.7)
    - Optional task marker detection (Req 22.1)
    - Non-requirement Validates targets like `Design Decision 5` (Req 5.4)
    - Discovery with nonexistent `.kiro/specs/` → `SpecsDirNotFound` error (Req 1.4)
    - File writing conflict detection and force override (Req 15.3)
    - Best-effort write semantics with partial failure (Req 15.5)
    - _Requirements: 1.4, 2.5, 5.4, 8.7, 13.2, 15.3, 15.5, 22.1_

- [x] 23. Final checkpoint — All tests pass
  - Run `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features`, `cargo nextest run`
  - Ensure zero warnings and zero errors
  - Ask the user if questions arise.

## Notes

- Each property test sub-task references its property number from the design document and the requirements it validates
- The TDD flow is: write property test (red) → implement code (green) → refactor → run `cargo fmt`/`clippy`/`nextest`
- Property tests live in `crates/supersigil-import/tests/` as integration tests, organized by property group per the design's test organization
- Generators in `tests/generators.rs` are shared across all property test files
- The `supersigil-import` crate has no dependency on the CLI — all functionality is testable as a library
- Checkpoints are placed after each major pipeline stage to catch regressions early
- `proptest` is already a workspace dependency; `tempfile` is needed for file writing tests; `insta` (with `yaml` feature) is needed for snapshot tests
