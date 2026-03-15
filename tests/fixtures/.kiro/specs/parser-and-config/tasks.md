# Implementation Plan: Parser and Config

## Overview

Build the two foundational crates of Supersigil: `supersigil-core` (data model, config loader, built-in component definitions, list splitting utility) and `supersigil-parser` (three-stage MDX parsing pipeline with lint-time validation). Uses TDD throughout — tests are written before implementation. Uses `cargo nextest` for test execution, `proptest` for property-based tests, and `clippy` for linting.

IMPORTANT: when adding dependencies, prefer `cargo add` to ensure we use latest versions.

Use your Rust skills while developing.

## Tasks

- [x] 1. Set up workspace structure and crate scaffolding
  - Convert root `Cargo.toml` to a Cargo workspace with `members = ["crates/supersigil-core", "crates/supersigil-parser"]`
  - Keep existing `workspace.lints` configuration in root `Cargo.toml`
  - Create `crates/supersigil-core/Cargo.toml` with dependencies: `serde` (features: derive), `serde_yaml`, `toml`, `regex`, and dev-dependency `proptest`.
  - Create `crates/supersigil-core/src/lib.rs` as the crate root
  - Create `crates/supersigil-parser/Cargo.toml` with dependencies: `markdown` (markdown-rs), `serde_yaml`, `supersigil-core` (path dep), and dev-dependency `proptest`
  - Create `crates/supersigil-parser/src/lib.rs` as the crate root
  - Both crates inherit `workspace.lints` via `[lints] workspace = true`
  - Create test file stubs: `crates/supersigil-core/tests/config_property_tests.rs`, `crates/supersigil-core/tests/config_unit_tests.rs`, `crates/supersigil-parser/tests/property_tests.rs`, `crates/supersigil-parser/tests/unit_tests.rs`, `crates/supersigil-parser/tests/fixtures/` directory
  - Verify workspace compiles with `cargo build` and `cargo nextest run` passes (no tests yet)
  - _Requirements: N/A (project setup)_

- [x] 2. Implement `supersigil-core` data model types
  - [x] 2.1 Write unit tests for data model types (TDD)
    - Test `Frontmatter` construction with all field combinations (id only, id+doc_type, id+status, all three)
    - Test `SourcePosition` equality
    - Test `ExtractedComponent` with empty children, with body_text, self-closing (None body_text)
    - Test `ParseResult::Document` and `ParseResult::NotSupersigil` variant construction
    - _Requirements: 4.1, 4.5, 8.3, 8.4, 10.1, 10.2_

  - [x] 2.2 Implement data model types
    - Implement `Frontmatter` with `Serialize`/`Deserialize`, `type` ↔ `doc_type` rename, `skip_serializing_if` for optional fields
    - Implement `SourcePosition`, `ExtractedComponent`, `SpecDocument`, `ParseResult`, `ParseError`, `ConfigError`, `ListSplitError`
    - All types derive `Debug`, `Clone`, `PartialEq` as specified in design
    - _Requirements: 4.1, 4.5, 8.2, 8.3, 8.4, 10.1, 10.2_

  - [x] 2.3 Write property test: Frontmatter YAML round-trip (Property 1)
    - **Property 1: Frontmatter YAML round-trip**
    - Create `arb_frontmatter()` generator: random `id` (non-empty alphanumeric + `/` + `-`), optional `doc_type`, optional `status`
    - Serialize to YAML, deserialize back, assert equality
    - Verify `type` ↔ `doc_type` rename survives round-trip
    - **Validates: Requirements 22.1, 4.1**

- [x] 3. Implement Config types and deserialization
  - [x] 3.1 Write unit tests for Config types (TDD)
    - Test minimal config: `paths = ["specs/**/*.mdx"]` produces valid Config with all defaults (Req 24)
    - Test default values: ecosystem plugins default to `["rust"]`, hooks timeout defaults to 30, tests defaults to `[]`
    - Test `Severity` enum deserialization for `"off"`, `"warning"`, `"error"`
    - Test `deny_unknown_fields` rejects unknown keys at top level
    - Test single-project config with `paths` and optional `tests`
    - Test multi-project config with `projects` table
    - Test `ProjectConfig` missing `paths` field produces serde error
    - Test document type definitions with status lists and required_components
    - Test component definitions with attributes (required, list flags), referenceable, target_component
    - Test hooks config with and without timeout_seconds
    - Test test_results config
    - Test ecosystem config with explicit empty plugins list
    - _Requirements: 11.1, 11.3, 12.1, 12.2, 12.6, 12.7, 13.1, 13.2, 13.3, 13.4, 14.1, 14.2, 14.3, 15.1, 15.2, 16.1, 16.2, 16.3, 17.1, 17.2, 17.3, 17.4, 18.1, 18.2, 19.1, 19.2, 19.3, 19.4, 19.5, 24.1_

  - [x] 3.2 Implement Config and all supporting types
    - Implement `Config`, `ProjectConfig`, `DocumentsConfig`, `DocumentTypeDef`, `ComponentDef`, `AttributeDef`, `VerifyConfig`, `Severity`, `EcosystemConfig`, `HooksConfig`, `TestResultsConfig` with `Serialize`/`Deserialize` and `deny_unknown_fields`
    - Implement `Default` for `EcosystemConfig` (plugins = `["rust"]`), `HooksConfig` (timeout = 30, empty lists), and other types with `#[serde(default)]`
    - _Requirements: 11.1, 13.1, 14.1, 15.1, 16.1, 17.1, 18.1, 19.1_

  - [x] 3.3 Write property test: Config TOML round-trip (Property 2)
    - **Property 2: Config TOML round-trip**
    - Create `arb_config()` generator: valid Config in single-project or multi-project mode with random document types, component defs, verify rules, plugins, hooks, test results. Must ensure mutual exclusivity invariant.
    - Serialize to TOML, deserialize back, assert equality
    - **Validates: Requirements 23.1, 11.1**

- [x] 4. Implement built-in component definitions and ComponentDefs
  - [x] 4.1 Write unit tests for ComponentDefs (TDD)
    - Test `ComponentDefs::defaults()` returns exactly the 9 built-in components (AcceptanceCriteria, Criterion, Validates, VerifiedBy, Implements, Illustrates, Task, TrackedFiles, DependsOn) with correct attribute schemas
    - Test `ComponentDefs::merge()`: user override replaces built-in, new user component added, unmentioned built-ins preserved
    - Test `ComponentDefs::is_known()` and `ComponentDefs::get()`
    - Test list-typed attributes: `refs` on Validates/Implements/Illustrates/DependsOn, `paths` on VerifiedBy/TrackedFiles, `implements`/`depends` on Task
    - _Requirements: 7.1, 7.3, 14.4, 14.5_

  - [x] 4.2 Implement `ComponentDefs` with `defaults()`, `merge()`, `is_known()`, `get()`
    - Define the 9 built-in component definitions matching the design table exactly
    - Implement merge: user defs override same-name built-ins, add new names, preserve unmentioned built-ins
    - _Requirements: 7.3, 14.4, 14.5_

  - [x] 4.3 Write property test: Component definition merge (Property 13)
    - **Property 13: Component definition merge is additive over built-in defaults**
    - Create `arb_component_def()` generator
    - Generate random user component defs, merge over defaults, verify: overrides replace, new names added, unmentioned built-ins remain
    - **Validates: Requirements 14.5**

- [x] 5. Implement `load_config` with validation
  - [x] 5.1 Write unit tests for `load_config` validation (TDD)
    - Test mutual exclusivity: `paths` + `projects` → error, `tests` + `projects` → error, neither → error
    - Test unknown verification rule names → error
    - Test invalid severity values → error
    - Test valid `id_pattern` regex accepted
    - Test invalid `id_pattern` regex → `InvalidIdPattern` error
    - Test no `id_pattern` → no validation
    - Test TOML syntax error → `TomlSyntax` error
    - Test multi-project missing `paths` → serde error
    - _Requirements: 12.3, 12.4, 12.5, 15.3, 15.4, 20.1, 20.2, 20.3, 11.2_

  - [x] 5.2 Implement `load_config(path) -> Result<Config, Vec<ConfigError>>`
    - Read and deserialize TOML (with `deny_unknown_fields` catching unknown keys)
    - Post-deserialization validation: mutual exclusivity check, unknown rule name check, id_pattern regex compilation
    - Collect all post-deserialization errors before returning
    - _Requirements: 11.1, 11.2, 11.3, 12.1, 12.2, 12.3, 12.4, 12.5, 15.3, 20.1, 20.2_

  - [x] 5.3 Write property test: Unknown TOML keys rejected (Property 11)
    - **Property 11: Unknown TOML keys are rejected at all nesting levels**
    - Generate valid Config TOML, inject unknown key at random nesting level, verify error
    - **Validates: Requirements 11.3**

  - [x] 5.4 Write property test: Mutual exclusivity (Property 12)
    - **Property 12: Single-project and multi-project modes are mutually exclusive**
    - Generate TOML configs with various combinations of `paths`, `tests`, `projects`, verify exactly one valid mode or error
    - **Validates: Requirements 12.1, 12.2, 12.3, 12.4, 12.5**

  - [x] 5.5 Write property test: Unknown verification rules rejected (Property 14)
    - **Property 14: Unknown verification rule names are rejected**
    - Generate rule names not in the known set, verify error
    - **Validates: Requirements 15.3**

  - [x] 5.6 Write property test: id_pattern regex validation (Property 19)
    - **Property 19: id_pattern accepts valid regex and rejects invalid regex**
    - Generate valid and invalid regex strings, verify acceptance/rejection
    - **Validates: Requirements 20.1, 20.2**

- [x] 6. Implement `split_list_attribute` utility
  - [x] 6.1 Write unit tests for `split_list_attribute` (TDD)
    - Test single item: `"foo"` → `["foo"]`
    - Test multiple items: `"a, b, c"` → `["a", "b", "c"]`
    - Test whitespace trimming: `"  a , b  ,c  "` → `["a", "b", "c"]`
    - Test trailing comma: `"a, b,"` → `ListSplitError`
    - Test consecutive commas: `"a,,b"` → `ListSplitError`
    - Test empty string: `""` → `ListSplitError`
    - Test whitespace-only items: `"a, , b"` → `ListSplitError`
    - _Requirements: 7.2_

  - [x] 6.2 Implement `split_list_attribute(raw: &str) -> Result<Vec<&str>, ListSplitError>`
    - Split on `,`, trim each item, reject empty items
    - _Requirements: 7.2_

  - [x] 6.3 Write property test: List splitting (Property 15)
    - **Property 15: List splitting produces trimmed non-empty items**
    - Create `arb_comma_separated()` generator
    - Verify all output items are non-empty and trimmed; trailing/consecutive commas rejected
    - **Validates: Requirements 7.2**

- [x] 7. Checkpoint — `supersigil-core` complete
  - Ensure all tests pass with `cargo nextest run -p supersigil-core`, ask the user if questions arise.
  - Run `cargo clippy -p supersigil-core` and fix any warnings.

- [-] 8. Implement parser Stage 1: Preprocessing
  - [x] 8.1 Write unit tests for preprocessing (TDD)
    - Test valid UTF-8 passthrough
    - Test non-UTF-8 bytes → `IoError`
    - Test BOM stripping: file with BOM → BOM removed
    - Test no BOM: content unchanged
    - Test file with only a BOM → empty string
    - Test BOM followed by `---`
    - Test CRLF normalization: all `\r\n` → `\n`
    - Test mixed `\r\n` and `\n` → all `\r\n` normalized, bare `\r` preserved
    - Test file with only `\r\n` → `\n`
    - _Requirements: 1.1, 1.2, 1.3, 2.1, 2.2_

  - [x] 8.2 Implement `preprocess(raw: &[u8]) -> Result<String, ParseError>`
    - Decode UTF-8 (return `IoError` on failure)
    - Strip leading BOM (U+FEFF) if present
    - Replace all `\r\n` with `\n`
    - _Requirements: 1.1, 1.2, 1.3, 2.1, 2.2_

  - [x] 8.3 Write property test: BOM stripping (Property 3)
    - **Property 3: BOM stripping preserves content**
    - Generate valid UTF-8 content, optionally prepend BOM, verify BOM stripped and content preserved (modulo CRLF)
    - **Validates: Requirements 1.1, 1.2, 1.3**

  - [x] 8.4 Write property test: CRLF normalization (Property 4)
    - **Property 4: CRLF normalization replaces all original CRLF pairs**
    - Generate strings with arbitrary `\r\n` placement, verify all original CRLF pairs replaced, bare `\r` preserved
    - **Validates: Requirements 2.1, 2.2**

- [x] 9. Implement parser Stage 1: Front matter extraction and deserialization
  - [x] 9.1 Write unit tests for front matter extraction (TDD)
    - Test valid front matter: `---\nsupersigil:\n  id: test\n---\nbody` → extracts YAML and body
    - Test `---` with trailing whitespace accepted as delimiter
    - Test no `---` first line → returns `None` (NotSupersigil path)
    - Test unclosed front matter (opening `---` but no closing) → `UnclosedFrontMatter` error
    - Test empty YAML between delimiters
    - Test `---` inside YAML content terminates front matter
    - _Requirements: 3.1, 3.2, 3.3_

  - [x] 9.2 Write unit tests for front matter deserialization (TDD)
    - Test valid `supersigil:` with id, doc_type, status → `Frontmatter`
    - Test `supersigil:` with only id → doc_type and status are None
    - Test missing `id` → `MissingId` error
    - Test invalid YAML → `InvalidYaml` error
    - Test no `supersigil:` key → `NotSupersigil`
    - Test extra metadata keys preserved in `extra` HashMap
    - Test front matter with only `supersigil: { id: x }` and extra keys
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6_

  - [x] 9.3 Implement `extract_front_matter(content: &str) -> Result<Option<(&str, &str)>, ParseError>`
    - Detect opening `---` (with optional trailing whitespace) on first line
    - Find closing `---` line (with optional trailing whitespace)
    - Return `(yaml_str, body_str)` or error
    - _Requirements: 3.1, 3.2, 3.3_

  - [x] 9.4 Implement `deserialize_front_matter(yaml: &str) -> Result<FrontMatterResult, ParseError>`
    - Deserialize YAML, check for `supersigil:` key
    - Extract `id` (required), `doc_type`, `status` into `Frontmatter`
    - Preserve non-supersigil keys in `extra` HashMap
    - Return `NotSupersigil` if no `supersigil:` key
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6_

  - [x] 9.5 Write property test: NotSupersigil detection (Property 5)
    - **Property 5: Files without supersigil front matter return NotSupersigil**
    - Generate files without `---` first line, or with YAML but no `supersigil:` key, verify `NotSupersigil`
    - **Validates: Requirements 3.3, 4.6, 10.2**

  - [x] 9.6 Write property test: Unclosed front matter (Property 6)
    - **Property 6: Unclosed front matter produces an error**
    - Generate content starting with `---\n` followed by arbitrary content with no closing `---` line, verify `UnclosedFrontMatter` error
    - **Validates: Requirements 3.2**

  - [x] 9.7 Write property test: Extra metadata preservation (Property 7)
    - **Property 7: Extra metadata preservation**
    - Generate YAML with `supersigil:` key and additional arbitrary keys, verify all non-supersigil keys appear in `extra`
    - **Validates: Requirements 4.4**

- [x] 10. Implement parser Stage 2: MDX AST generation
  - [x] 10.1 Write unit tests for MDX parsing (TDD)
    - Test valid MDX body produces AST
    - Test invalid MDX syntax → `MdxSyntaxError` with position and message
    - Test body with PascalCase components produces `MdxJsxFlowElement` nodes
    - Test body with lowercase HTML elements (div, p, table) in AST
    - _Requirements: 5.1, 5.2_

  - [x] 10.2 Implement `parse_mdx_body(body: &str) -> Result<mdast::Node, ParseError>`
    - Use `markdown-rs` with MDX constructs enabled
    - Map parse errors to `MdxSyntaxError`
    - _Requirements: 5.1, 5.2_

- [x] 11. Implement parser Stage 3: Component extraction
  - [x] 11.1 Write unit tests for component extraction (TDD)
    - Test PascalCase flow element extracted with name and attributes
    - Test lowercase element (div, p) silently ignored
    - Test inline JSX (`MdxJsxTextElement`) ignored
    - Test string literal attributes stored as raw strings
    - Test expression attribute `{...}` → `ExpressionAttribute` error, attribute excluded
    - Test self-closing component → body_text is None
    - Test component with text content → body_text is trimmed concatenation
    - Test component with only child components → body_text is None
    - Test nested components: parent's children list populated, recursive nesting preserved
    - Test source position offset: positions relative to original file (front matter offset applied)
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 9.1, 9.2, 9.3_

  - [x] 11.2 Implement `extract_components(node, component_defs, body_offset, errors) -> Vec<ExtractedComponent>`
    - Walk AST recursively, process only `MdxJsxFlowElement` nodes
    - Skip lowercase element names (HTML), skip `MdxJsxTextElement` (inline)
    - Extract PascalCase component name, string attributes, body text, children
    - Offset positions by `body_offset` for file-relative source positions
    - Record `ExpressionAttribute` errors for `{...}` syntax, exclude those attributes
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 9.1, 9.2, 9.3_

  - [x] 11.3 Write property test: String attribute fidelity (Property 8)
    - **Property 8: String attribute extraction fidelity**
    - Generate components with string literal attributes, verify exact raw string preservation
    - **Validates: Requirements 6.1, 6.4**

  - [x] 11.4 Write property test: Body text (Property 9)
    - **Property 9: Body text is trimmed concatenation of non-component text nodes**
    - Generate components with varying text/child combinations, verify body_text rules
    - **Validates: Requirements 8.3, 8.4, 8.5**

  - [x] 11.5 Write property test: Recursive child collection (Property 10)
    - **Property 10: Recursive child collection preserves nesting structure**
    - Generate nested component structures, verify tree structure matches source
    - **Validates: Requirements 9.1, 9.2, 9.3**

- [x] 12. Implement lint-time validation and `parse_file` public API
  - [x] 12.1 Write unit tests for lint-time validation (TDD)
    - Test unknown PascalCase component → `UnknownComponent` error
    - Test known component → no error
    - Test lowercase element name → no unknown component error (silently ignored)
    - Test missing required attribute → `MissingRequiredAttribute` error with component name, attribute, position
    - Test all required attributes present → no error
    - Test validation uses config component defs when provided, built-in defaults when not
    - _Requirements: 21.1, 21.2, 21.3, 25.1, 25.2_

  - [x] 12.2 Write unit tests for `parse_file` integration (TDD)
    - Test full pipeline: valid MDX file → `ParseResult::Document` with frontmatter, extra, components
    - Test file without front matter → `ParseResult::NotSupersigil`
    - Test file with front matter but no `supersigil:` key → `ParseResult::NotSupersigil`
    - Test error collection: file with multiple errors returns all of them
    - Test stage 1 fatal error prevents stages 2-3
    - Test stage 2 error prevents stage 3
    - Test stage 3 errors (expression attr, unknown component, missing attr) all collected
    - _Requirements: 10.1, 10.2, 10.3, 21.4, 25.3_

  - [x] 12.3 Implement lint-time validation checks
    - Unknown component detection: check PascalCase names against `ComponentDefs`, emit `UnknownComponent` error
    - Missing required attribute detection: check each component's attributes against its definition, emit `MissingRequiredAttribute` error
    - Both checks run after component extraction, errors appended to error list
    - _Requirements: 21.1, 21.2, 21.3, 25.1, 25.2_

  - [x] 12.4 Implement `parse_file(path, &ComponentDefs) -> Result<ParseResult, Vec<ParseError>>`
    - Wire the three-stage pipeline: preprocess → extract/deserialize front matter → parse MDX → extract components → lint-time validation
    - Implement error collection: stage 1 fatal errors stop pipeline, stage 2 errors stop stage 3, stage 3 errors collected
    - Return `ParseResult::Document` or `ParseResult::NotSupersigil` on success, `Vec<ParseError>` on failure
    - _Requirements: 10.1, 10.2, 10.3_

  - [x] 12.5 Write property test: Missing required attributes detected (Property 16)
    - **Property 16: Missing required attributes are detected**
    - Generate component defs with required attributes, generate component instances missing some, verify errors. Also verify no false positives when all present.
    - **Validates: Requirements 21.1, 21.2**

  - [x] 12.6 Write property test: Unknown component names detected (Property 17)
    - **Property 17: Unknown component names are detected**
    - Generate PascalCase names not in component defs, verify `UnknownComponent` error. Verify lowercase names never produce errors.
    - **Validates: Requirements 25.1**

  - [x] 12.7 Write property test: Error collection (Property 18)
    - **Property 18: Parser collects all errors rather than stopping at the first**
    - Generate files with multiple independent error conditions, verify all errors returned
    - **Validates: Requirements 10.3**

- [x] 13. Checkpoint — `supersigil-parser` complete
  - Ensure all tests pass with `cargo nextest run`, ask the user if questions arise.
  - Run `cargo clippy --workspace` and fix any warnings.

- [x] 14. Create MDX test fixtures and final integration validation
  - [x] 14.1 Create MDX test fixture files
    - Create `crates/supersigil-parser/tests/fixtures/valid_simple.mdx`: minimal valid supersigil document
    - Create `crates/supersigil-parser/tests/fixtures/valid_nested.mdx`: document with nested components (AcceptanceCriteria > Criterion)
    - Create `crates/supersigil-parser/tests/fixtures/no_frontmatter.mdx`: plain MDX without front matter
    - Create `crates/supersigil-parser/tests/fixtures/no_supersigil_key.mdx`: front matter without `supersigil:` key
    - Create `crates/supersigil-parser/tests/fixtures/extra_metadata.mdx`: front matter with supersigil + extra keys
    - _Requirements: 3.1, 3.3, 4.4, 4.6, 9.1_

  - [x] 14.2 Write fixture-based integration tests
    - Test each fixture file through `parse_file` and verify expected output
    - Verify round-trip properties hold for fixture front matter
    - _Requirements: 10.1, 10.2, 22.1_

- [x] 15. Final checkpoint — all crates complete
  - Ensure all tests pass with `cargo nextest run --workspace`, ask the user if questions arise.
  - Run `cargo clippy --workspace` and fix any warnings.
  - Verify `cargo doc --workspace --no-deps` builds without warnings.

## Notes

- TDD approach: test sub-tasks come before implementation sub-tasks within each group
- Each task references specific requirements for traceability
- Property tests use `proptest` crate with minimum 100 iterations per property
- Use `cargo nextest run` for all test execution (not `cargo test`)
- Use `cargo clippy` for linting (pedantic + extra lints already configured in workspace)
- Checkpoints ensure incremental validation at crate boundaries
