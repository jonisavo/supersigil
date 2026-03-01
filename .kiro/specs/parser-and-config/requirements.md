# Requirements Document

## Introduction

This spec covers the foundational parser and configuration subsystem of Supersigil. It is responsible for turning raw `.mdx` files into `ParseResult` values (either `Document(SpecDocument)` or `NotSupersigil(path)`) and parsing `supersigil.toml` into a `Config` struct. No cross-document logic (ref resolution, coverage, graph building) belongs here — only single-file parsing and config deserialization.

The subsystem spans two crates: `supersigil-parser` (front matter extraction, MDX AST generation, component extraction) and the config module of `supersigil-core` (TOML deserialization, structural validation).

## Glossary

- **Parser**: The `supersigil-parser` crate, responsible for reading a single MDX file and producing a `ParseResult`.
- **Config_Loader**: The config module of `supersigil-core`, responsible for reading `supersigil.toml` and producing a `Config` struct.
- **ParseResult**: The return type of the Parser. Either `ParseResult::Document(SpecDocument)` for files with valid `supersigil:` front matter, or `ParseResult::NotSupersigil(path)` for files without `supersigil:` front matter (no front matter, or front matter without a `supersigil:` key).
- **SpecDocument**: A struct containing a file path, parsed front matter (including opaque non-supersigil metadata), and extracted components.
- **Frontmatter**: A struct holding the `id`, optional `doc_type`, and optional `status` fields extracted from the `supersigil:` YAML namespace.
- **Extra_Metadata**: A `HashMap<String, serde_yaml::Value>` field on `SpecDocument` containing all YAML front matter keys outside the `supersigil:` namespace, preserved as opaque data.
- **ExtractedComponent**: A struct representing a single MDX component with its name, attributes, children, body text, and source position.
- **AttributeValue**: A raw `String` as extracted from the MDX source. The parser stores all attribute values as strings. List splitting (for attributes like `refs`, `paths`) is deferred to downstream consumers using component definitions from config.
- **Config**: The output struct of the Config_Loader containing project paths, test paths, document type definitions, component definitions, verification rule overrides, ecosystem plugin declarations, and hook configuration.
- **BOM**: Byte Order Mark (U+FEFF), a Unicode character that may appear at the start of a file.
- **CRLF**: Carriage Return + Line Feed (`\r\n`), a Windows-style line ending.
- **Expression_Attribute**: A JSX expression attribute using `{...}` syntax, which Supersigil rejects.
- **Source_Position**: A struct recording the byte offset, line, and column of a component in the original file (after BOM stripping but including front matter).
- **PascalCase_Element**: An MDX JSX element whose name starts with an uppercase letter (e.g., `<Validates>`, `<Criterion>`). Only PascalCase elements are treated as supersigil components. Lowercase elements (e.g., `<div>`, `<p>`) are standard HTML and are silently ignored.
- **Flow_Element**: A block-level MDX JSX element (`MdxJsxFlowElement` in the AST). Supersigil extracts only flow elements as components.
- **Text_Element**: An inline MDX JSX element (`MdxJsxTextElement` in the AST). Supersigil ignores inline elements.

## Requirements

### Requirement 1: File Decoding and BOM Stripping

**User Story:** As a developer, I want the parser to handle files with different encodings and a leading BOM, so that files saved by Windows editors parse correctly.

#### Acceptance Criteria

1. THE Parser SHALL interpret file bytes as UTF-8. WHEN the file is not valid UTF-8, THE Parser SHALL emit an `IoError` indicating invalid encoding.
2. WHEN an MDX file begins with a BOM (U+FEFF), THE Parser SHALL strip the BOM before any further processing.
3. WHEN an MDX file does not begin with a BOM, THE Parser SHALL process the file content unchanged.

### Requirement 2: Line Ending Normalization

**User Story:** As a developer, I want the parser to normalize line endings, so that files from any operating system parse identically.

#### Acceptance Criteria

1. THE Parser SHALL normalize all CRLF (`\r\n`) sequences to LF (`\n`) before front matter detection and MDX parsing.
2. WHEN a file contains mixed LF and CRLF line endings, THE Parser SHALL normalize all CRLF sequences to LF. Bare `\r` characters (not followed by `\n`) SHALL be preserved as-is.

### Requirement 3: Front Matter Delimiter Detection

**User Story:** As a developer, I want the parser to detect YAML front matter delimiters, so that metadata is separated from document body.

#### Acceptance Criteria

1. WHEN the first line of the file (after BOM stripping and normalization) is `---` optionally followed by trailing whitespace and then a newline, THE Parser SHALL treat the content between the opening delimiter and the next line that is `---` (optionally followed by trailing whitespace) as YAML front matter. Trailing whitespace on delimiter lines is accepted for compatibility with editor auto-formatting.
2. WHEN the first line of the file matches the opening delimiter but no closing `---` delimiter line exists, THE Parser SHALL emit a parse error indicating an unclosed front matter block. The first `---` on its own line after the opening `---` closes the front matter; multi-document YAML separators are not supported.
3. WHEN the first line of the file is not `---` (after trimming trailing whitespace), THE Parser SHALL return `ParseResult::NotSupersigil(path)`.

### Requirement 4: YAML Front Matter Deserialization

**User Story:** As a developer, I want the parser to extract the `supersigil:` namespace from YAML front matter, so that document identity and metadata are available as typed data.

#### Acceptance Criteria

1. WHEN valid YAML front matter contains a `supersigil:` key, THE Parser SHALL deserialize the `id` field into `Frontmatter.id`, the `type` field into `Frontmatter.doc_type`, and the `status` field into `Frontmatter.status`.
2. WHEN the `supersigil:` key is present but the `id` field is missing, THE Parser SHALL emit a parse error indicating that `id` is required.
3. WHEN the YAML between the front matter delimiters is not valid YAML, THE Parser SHALL emit a parse error with the YAML deserialization error message.
4. WHEN valid YAML front matter contains keys outside the `supersigil:` namespace, THE Parser SHALL preserve those keys as opaque metadata in `SpecDocument.extra` (`HashMap<String, serde_yaml::Value>`) without emitting errors or warnings.
5. WHEN valid YAML front matter contains a `supersigil:` key with `type` and `status` fields absent, THE Parser SHALL set `Frontmatter.doc_type` and `Frontmatter.status` to `None`.
6. WHEN valid YAML front matter does not contain a `supersigil:` key, THE Parser SHALL return `ParseResult::NotSupersigil(path)`.

### Requirement 5: MDX AST Generation

**User Story:** As a developer, I want the parser to produce an MDX AST from the document body, so that components can be extracted from the tree.

#### Acceptance Criteria

1. WHEN front matter has been extracted, THE Parser SHALL pass the remaining body content to the MDX parser with MDX constructs enabled to produce an AST.
2. WHEN the body content contains invalid MDX syntax, THE Parser SHALL emit a parse error with the position and description of the syntax error.

### Requirement 6: String Literal Attribute Extraction

**User Story:** As a developer, I want component attributes to be extracted as string literals, so that attribute values are unambiguous and trivially parseable.

#### Acceptance Criteria

1. WHEN an MDX component has a string literal attribute (e.g., `id="valid-creds"`), THE Parser SHALL store the attribute value as a raw `String`.
2. WHEN an MDX component has an attribute using JSX expression syntax (`{...}`), THE Parser SHALL emit a lint error identifying the attribute name, the component name, and the source position, with a fix suggestion showing the equivalent string attribute syntax.
3. THE Parser SHALL reject Expression_Attribute values and exclude them from the resulting ExtractedComponent attributes.
4. THE Parser SHALL store all attribute values as raw strings without interpreting their contents. List splitting (for attributes like `refs`, `paths`) is deferred to downstream consumers using component definitions from config.

### Requirement 7: Attribute Type Definitions in Config

**User Story:** As a developer, I want component definitions in config to declare attribute types, so that downstream consumers know which attributes are lists and can split them correctly.

#### Acceptance Criteria

1. WHEN a component definition in config declares an attribute with `list = true`, THE Config_Loader SHALL mark that attribute as list-typed. WHEN `list` is absent or `false`, THE Config_Loader SHALL treat the attribute as string-typed. Each attribute also has a `required` boolean (`true` or `false`).
2. THE `supersigil-core` crate SHALL provide a `split_list_attribute(raw: &str) -> Result<Vec<&str>, ListSplitError>` utility function that splits the raw string value on `,`, trims leading and trailing whitespace from each item, and rejects empty items (e.g., trailing comma or consecutive commas). This function is tested in this spec as a shared utility, though it is consumed by downstream specs (graph, verification).
3. THE built-in default component definitions SHALL declare `refs` (on `Validates`, `Implements`, `Illustrates`, `DependsOn`), `paths` (on `VerifiedBy`, `TrackedFiles`), `implements` (on `Task`), and `depends` (on `Task`) as list-typed attributes.

### Requirement 8: Component Extraction from AST

**User Story:** As a developer, I want the parser to walk the MDX AST and collect all MDX components with their attributes and positions, so that downstream verification has structured component data.

#### Acceptance Criteria

1. WHEN the MDX AST contains `MdxJsxFlowElement` nodes with PascalCase names (first character is uppercase), THE Parser SHALL extract each node's component name from the element name field. Lowercase element names (standard HTML like `<div>`, `<p>`, `<table>`) SHALL be silently ignored.
2. THE Parser SHALL record the source position (byte offset, line, and column) of each extracted component in `ExtractedComponent.position`. Positions SHALL be relative to the original file content (after BOM stripping but including front matter), so that they are usable for editor integration. The parser SHALL offset positions from `markdown-rs` (which are relative to the MDX body) by the byte length of the front matter block.
3. WHEN an MDX component contains body text (non-component text nodes between opening and closing tags), THE Parser SHALL collect all non-component text nodes, concatenate them, trim leading and trailing whitespace from the concatenated result, and store it in `ExtractedComponent.body_text`. Child components are excluded from body text and extracted separately into `ExtractedComponent.children`.
4. WHEN an MDX component is self-closing (e.g., `<Validates refs="..." />`), THE Parser SHALL set `ExtractedComponent.body_text` to `None`.
5. WHEN an MDX component has child components but no non-component text nodes, THE Parser SHALL set `ExtractedComponent.body_text` to `None`.
6. THE Parser SHALL ignore `MdxJsxTextElement` nodes (inline JSX elements). Only block-level `MdxJsxFlowElement` nodes are extracted as components.

### Requirement 9: Recursive Child Collection

**User Story:** As a developer, I want nested components to be collected recursively, so that structures like `<Criterion>` inside `<AcceptanceCriteria>` are preserved.

#### Acceptance Criteria

1. WHEN an MDX component contains child MDX components (e.g., `<Criterion>` inside `<AcceptanceCriteria>`), THE Parser SHALL extract the child components and store them in the parent `ExtractedComponent.children` list.
2. WHEN child components themselves contain nested children, THE Parser SHALL extract all levels of nesting recursively.
3. WHEN an MDX component has no child components, THE Parser SHALL set `ExtractedComponent.children` to an empty list.

### Requirement 10: ParseResult Assembly

**User Story:** As a developer, I want the parser to produce a typed result that distinguishes supersigil documents from non-supersigil files, so that downstream consumers can filter appropriately.

#### Acceptance Criteria

1. WHEN parsing produces a valid `supersigil:` front matter with a required `id`, THE Parser SHALL return `ParseResult::Document(SpecDocument)` containing the file path, the parsed `Frontmatter`, the opaque `extra` metadata, and the list of top-level `ExtractedComponent` structs.
2. WHEN the file has no front matter or has front matter without a `supersigil:` key, THE Parser SHALL return `ParseResult::NotSupersigil(path)`.
3. WHEN parsing encounters one or more errors, THE Parser SHALL return all collected errors from all pipeline stages that executed, rather than stopping at the first error. Stage 1 fatal errors (unclosed front matter, invalid YAML, missing `id`) prevent stages 2 and 3 from running; stage 2 errors (MDX syntax) prevent stage 3. Within each stage, all independent errors are collected.

### Requirement 11: Config File Deserialization

**User Story:** As a developer, I want `supersigil.toml` to be deserialized into a typed `Config` struct, so that all configuration is available as structured data.

#### Acceptance Criteria

1. WHEN a valid `supersigil.toml` file is provided, THE Config_Loader SHALL deserialize the file into a `Config` struct.
2. WHEN the TOML file contains syntax errors, THE Config_Loader SHALL emit a parse error with the TOML deserialization error message.
3. WHEN the TOML file contains unknown keys at any nesting level (top-level, inside `[documents]`, inside `[components.<Name>]`, inside `[projects.<name>]`), THE Config_Loader SHALL emit a config error identifying the unknown key and its location.

### Requirement 12: Single-Project vs. Multi-Project Mutual Exclusivity

**User Story:** As a developer, I want the config loader to enforce that single-project keys and multi-project keys are mutually exclusive, so that the project structure is unambiguous.

#### Acceptance Criteria

1. WHEN the config file contains a top-level `paths` key and no `projects` table, THE Config_Loader SHALL configure a single-project setup using the `paths` value.
2. WHEN the config file contains a `projects` table and no top-level `paths` or `tests` keys, THE Config_Loader SHALL configure a multi-project setup with one project per entry in the `projects` table.
3. WHEN the config file contains both a top-level `paths` key and a `projects` table, THE Config_Loader SHALL emit a config error stating that `paths` and `projects` are mutually exclusive.
4. WHEN the config file contains both a top-level `tests` key and a `projects` table, THE Config_Loader SHALL emit a config error stating that `tests` and `projects` are mutually exclusive.
5. WHEN the config file contains neither a top-level `paths` key nor a `projects` table, THE Config_Loader SHALL emit a config error stating that one of `paths` or `projects` is required.
6. WHEN the config file contains a top-level `tests` key in single-project mode, THE Config_Loader SHALL store the test path globs for the project.
7. WHEN the config file omits the top-level `tests` key in single-project mode, THE Config_Loader SHALL default `tests` to an empty list.

### Requirement 13: Document Type Definitions

**User Story:** As a developer, I want to define document types with valid status lists and required components, so that the verification engine can check documents against their type constraints.

#### Acceptance Criteria

1. WHEN the config file contains `[documents.types.<name>]` sections, THE Config_Loader SHALL parse each section into a document type definition with a `status` list and an optional `required_components` list.
2. WHEN a document type definition includes a `status` field, THE Config_Loader SHALL store the list of valid status strings for that type.
3. WHEN a document type definition includes a `required_components` field, THE Config_Loader SHALL store the list of required component names for that type.
4. WHEN no `[documents.types]` section is present, THE Config_Loader SHALL use an empty set of document type definitions.

### Requirement 14: Component Definitions

**User Story:** As a developer, I want to define components with attribute requirements in config, so that the verification engine can check component usage against declared schemas.

#### Acceptance Criteria

1. WHEN the config file contains `[components.<Name>]` sections, THE Config_Loader SHALL parse each section into a component definition with an `attributes` map where each attribute has a `required` boolean and an optional `list` boolean.
2. WHEN a component definition includes a `referenceable` field set to `true`, THE Config_Loader SHALL mark that component as referenceable (its `id` attribute can be targeted by fragment refs).
3. WHEN a component definition includes a `target_component` field, THE Config_Loader SHALL store the target component name for fragment type checking.
4. WHEN no `[components]` section is present, THE Config_Loader SHALL use the built-in default component definitions for `AcceptanceCriteria`, `Criterion`, `Validates`, `VerifiedBy`, `Implements`, `Illustrates`, `Task`, `TrackedFiles`, and `DependsOn`.
5. WHEN a `[components]` section is present, THE Config_Loader SHALL merge user-defined component definitions over the built-in defaults. User-defined components with the same name as a built-in SHALL override the built-in definition. User-defined components with new names SHALL be added to the set. Built-in components not overridden SHALL remain in the set.

### Requirement 15: Verification Rule Severity Overrides

**User Story:** As a developer, I want to override the severity of individual verification rules, so that I can tune strictness for my project.

#### Acceptance Criteria

1. WHEN the config file contains a `[verify]` section with a `strictness` field, THE Config_Loader SHALL store the global strictness value (one of `"off"`, `"warning"`, or `"error"`).
2. WHEN the config file contains `[verify.rules]` entries, THE Config_Loader SHALL store each rule name and its severity override (one of `"off"`, `"warning"`, or `"error"`).
3. WHEN a `[verify.rules]` entry contains an unknown rule name, THE Config_Loader SHALL emit a config error identifying the unknown rule name.
4. WHEN a severity value is not one of `"off"`, `"warning"`, or `"error"`, THE Config_Loader SHALL emit a config error identifying the invalid value.
5. WHEN no `[verify]` section is present, THE Config_Loader SHALL use the built-in default severity for each rule.

### Requirement 16: Ecosystem Plugin Declarations

**User Story:** As a developer, I want to declare ecosystem plugins in config, so that language-native test discovery is enabled.

#### Acceptance Criteria

1. WHEN the config file contains an `[ecosystem]` section with a `plugins` list, THE Config_Loader SHALL store the list of plugin identifiers.
2. WHEN no `[ecosystem]` section is present, THE Config_Loader SHALL default to `plugins = ["rust"]` (the built-in Rust plugin is active by default).
3. WHEN the config file explicitly sets `plugins = []`, THE Config_Loader SHALL store an empty plugin list, disabling all plugins including the built-in Rust plugin.

### Requirement 17: Hook Configuration

**User Story:** As a developer, I want to configure external process hooks in config, so that custom verification logic can run after built-in checks.

#### Acceptance Criteria

1. WHEN the config file contains a `[hooks]` section, THE Config_Loader SHALL parse the `post_verify`, `post_lint`, and `export` fields as ordered lists of command strings.
2. WHEN the `[hooks]` section includes a `timeout_seconds` field, THE Config_Loader SHALL store the timeout value.
3. WHEN the `[hooks]` section omits `timeout_seconds`, THE Config_Loader SHALL default the timeout to 30 seconds.
4. WHEN no `[hooks]` section is present, THE Config_Loader SHALL use empty hook lists and the default timeout.

### Requirement 18: Test Results Configuration

**User Story:** As a developer, I want to configure test result file formats and paths, so that Supersigil can consume pass/fail data from existing test runners.

#### Acceptance Criteria

1. WHEN the config file contains a `[test_results]` section with `formats` and `paths` fields, THE Config_Loader SHALL store the list of result formats and the list of result file path globs.
2. WHEN no `[test_results]` section is present, THE Config_Loader SHALL use empty format and path lists.

### Requirement 19: Multi-Project Configuration

**User Story:** As a developer working in a monorepo, I want each project entry to have its own `paths`, `tests`, and optional `isolated` flag, so that verification can be scoped per project.

#### Acceptance Criteria

1. WHEN a `[projects.<name>]` entry contains `paths` and `tests` fields, THE Config_Loader SHALL store the path globs and test globs for that project.
2. WHEN a `[projects.<name>]` entry is missing the `paths` field, THE Config_Loader SHALL emit a deserialization error (serde rejects the entry because `paths` is a required field on `ProjectConfig`).
3. WHEN a `[projects.<name>]` entry omits the `tests` field, THE Config_Loader SHALL default `tests` to an empty list for that project.
4. WHEN a `[projects.<name>]` entry contains an `isolated` field set to `true`, THE Config_Loader SHALL mark that project as isolated (cross-project refs are errors).
5. WHEN a `[projects.<name>]` entry omits the `isolated` field, THE Config_Loader SHALL default `isolated` to `false`.

### Requirement 20: ID Pattern Configuration

**User Story:** As a developer, I want to configure an optional regex pattern for document IDs, so that my team can enforce naming conventions.

#### Acceptance Criteria

1. WHEN the config file contains an `id_pattern` field, THE Config_Loader SHALL compile the value as a regex and store it for ID validation.
2. WHEN the `id_pattern` value is not a valid regex, THE Config_Loader SHALL emit a config error identifying the invalid pattern.
3. WHEN no `id_pattern` field is present, THE Config_Loader SHALL skip ID pattern validation.

### Requirement 21: Lint-Time Attribute Validation

**User Story:** As a developer, I want the parser to validate that components have their required attributes at parse time, so that structural errors are caught immediately without needing the full verification pipeline.

#### Acceptance Criteria

1. WHEN a parsed component is missing a required attribute as defined by the component definitions in config, THE Parser SHALL emit a hard error identifying the component name, the missing attribute, and the source position.
2. WHEN a parsed component has all required attributes, THE Parser SHALL not emit attribute validation errors for that component.
3. THE Parser SHALL load component definitions from the Config to determine which attributes are required. If no config is available, the built-in default component definitions SHALL be used.
4. Attribute validation SHALL be a per-file check (lint-time) requiring no cross-document logic.

### Requirement 22: Parser Round-Trip Property

**User Story:** As a developer, I want confidence that parsing and re-serializing front matter preserves the `supersigil:` namespace data, so that tooling that reads and writes spec files does not corrupt metadata.

#### Acceptance Criteria

1. FOR ALL valid Frontmatter values, serializing a Frontmatter to YAML and then deserializing the YAML back into a Frontmatter SHALL produce an equivalent Frontmatter. This requires `Serialize` and `Deserialize` implementations on `Frontmatter`, with correct field mapping (`type` ↔ `doc_type`).

### Requirement 23: Config Round-Trip Property

**User Story:** As a developer, I want confidence that serializing and deserializing config preserves all configuration data, so that programmatic config generation is reliable.

#### Acceptance Criteria

1. FOR ALL valid Config values, serializing a Config to TOML and then deserializing the TOML back into a Config SHALL produce an equivalent Config. This requires `Serialize` and `Deserialize` implementations on `Config` and all nested types. Fields that are not directly serializable (e.g., compiled `id_pattern` regex) SHALL be serialized as their source representation (the pattern string) and recompiled on deserialization.

### Requirement 24: Minimal Config

**User Story:** As a developer, I want a one-line config file to be sufficient, so that getting started with Supersigil has minimal friction.

#### Acceptance Criteria

1. WHEN the config file contains only `paths = ["specs/**/*.mdx"]` and no other keys, THE Config_Loader SHALL produce a valid Config with default values for all optional fields.

### Requirement 25: Lint-Time Unknown Component Detection

**User Story:** As a developer, I want the parser to detect unknown component names at parse time, so that hallucinated or misspelled components are caught immediately without needing the full verification pipeline.

#### Acceptance Criteria

1. WHEN a parsed component's name does not match any component in the built-in defaults or the config's `[components]` definitions, THE Parser SHALL emit a hard error identifying the unknown component name and the source position. Only PascalCase element names (first character is uppercase) are checked; lowercase element names (standard HTML) are silently ignored and never produce unknown component errors.
2. THE Parser SHALL load component definitions from the Config to determine the set of known component names. If no config is available, the built-in default component definitions SHALL be used.
3. Unknown component detection SHALL be a per-file check (lint-time) requiring no cross-document logic.
