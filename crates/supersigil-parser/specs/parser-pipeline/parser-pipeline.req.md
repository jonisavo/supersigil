---
supersigil:
  id: parser-pipeline/req
  type: requirements
  status: implemented
title: "Parser Pipeline"
---

```supersigil-xml
<References refs="document-format/adr" />
```

## Introduction

This spec defines the single-file parsing behavior implemented by the
`supersigil-parser` crate. It covers the byte-to-document pipeline:
preprocessing, front matter extraction, `supersigil:` namespace
deserialization, Markdown parsing, `supersigil-xml` fence extraction, XML
component extraction, lint-time validation, and `ParseResult` assembly.

The format is standard Markdown (`.md`) with YAML front matter and
`supersigil-xml` fenced code blocks containing an XML subset of structured
components. See `document-format/adr` for the format rationale.

## Definitions

- **Parser**: The `supersigil-parser` crate surface rooted at `parse_file`.
- **Preprocessing**: UTF-8 decoding, BOM stripping, and CRLF normalization.
- **Front_Matter**: YAML delimited by leading and closing `---` lines.
- **Supersigil_Namespace**: The `supersigil:` mapping inside front matter that
  yields `Frontmatter`.
- **Supersigil_Fence**: A Markdown fenced code block with the language
  identifier `supersigil-xml`.
- **PascalCase_Component**: An XML element inside a Supersigil_Fence whose
  name starts with an uppercase ASCII letter.
- **Component_Defs**: The merged runtime component definitions used for
  lint-time validation.

## Requirement 1: Preprocessing

As a parser consumer, I want file bytes normalized before any YAML or XML work,
so that editor-specific encodings and line endings do not change parse
behavior.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE Parser SHALL decode file bytes as UTF-8. IF the bytes are not valid
    UTF-8, THEN preprocessing SHALL return `ParseError::IoError`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/src/preprocess.rs" />
  </Criterion>
  <Criterion id="req-1-2">
    WHEN file content begins with a UTF-8 BOM, THE Parser SHALL strip it before
    front matter detection.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-1-3">
    THE Parser SHALL normalize every `\r\n` sequence to `\n` while preserving
    bare `\r` characters.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Front Matter Detection

As a parser consumer, I want the parser to distinguish supersigil documents
from ordinary Markdown files, so that non-spec files can be skipped cleanly.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    WHEN the first line of the preprocessed file is `---`, optionally followed
    by trailing whitespace, THE Parser SHALL treat the following lines up to
    the next delimiter line as Front_Matter.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-2-2">
    IF the opening delimiter has no matching closing delimiter, THEN front
    matter extraction SHALL return `ParseError::UnclosedFrontMatter`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-2-3">
    WHEN the file does not begin with an opening delimiter, THE Parser SHALL
    treat the file as not supersigil content rather than as malformed front
    matter.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Supersigil Namespace Deserialization

As a parser consumer, I want the `supersigil:` YAML namespace converted into
typed metadata while preserving the rest of the front matter, so that document
identity survives parsing.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN Front_Matter contains a `supersigil:` mapping with an `id`, THE Parser
    SHALL deserialize `id`, optional `type`, and optional `status` into
    `Frontmatter`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/src/frontmatter.rs" />
  </Criterion>
  <Criterion id="req-3-2">
    WHEN the `supersigil:` key is absent, or the front matter is empty, THE
    Parser SHALL return `FrontMatterResult::NotSupersigil`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-3-3">
    IF the `supersigil:` mapping is present but missing `id`, THEN
    deserialization SHALL return `ParseError::MissingId`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-3-4">
    IF the YAML is malformed, or the `supersigil:` value is not a mapping, THEN
    deserialization SHALL return `ParseError::InvalidYaml`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-3-5">
    THE Parser SHALL preserve all non-`supersigil:` front-matter keys as
    opaque extra metadata on the parsed document.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Markdown Parsing and Fence Extraction

As a parser consumer, I want the document body parsed as standard Markdown so
that `supersigil-xml` fences are identified for downstream extraction.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    AFTER successful front matter handling, THE Parser SHALL parse the
    remaining body as standard Markdown (without MDX constructs) and identify
    all fenced code blocks.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/src/markdown_fences.rs" />
  </Criterion>
  <Criterion id="req-4-2">
    THE Parser SHALL collect all fenced code blocks whose language identifier
    is `supersigil-xml` as Supersigil_Fences. The content of each fence is
    the raw text between the opening and closing fence delimiters.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/src/markdown_fences.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: XML Component Extraction

As a parser consumer, I want structured components extracted from
`supersigil-xml` fences into typed data, so that downstream graph and
verification logic can operate on typed components instead of raw text.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE Parser SHALL parse the content of each Supersigil_Fence as an XML
    subset: PascalCase elements, double-quoted string attributes, nesting,
    and text content. No processing instructions, CDATA, DTD, namespaces,
    or entity references beyond `&amp;`, `&lt;`, `&gt;`, `&quot;`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/src/xml_parser.rs, crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-5-2">
    IF a Supersigil_Fence contains invalid XML syntax, THEN the Parser
    SHALL return a parse error with position information adjusted to the
    fence's location in the source file.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-5-3">
    THE Parser SHALL extract only *known* PascalCase XML elements (those
    matching Component_Defs). Unknown PascalCase elements SHALL be treated
    as transparent content wrappers whose children are still traversed, so
    that known components nested inside unknown parents are extracted.
    Lowercase elements SHALL be ignored.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-5-4">
    THE Parser SHALL record component source positions relative to the
    original file after BOM stripping, offsetting by the front matter length
    and the fence's position within the Markdown body.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/src/xml_extract.rs, crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-5-5">
    THE Parser SHALL store string-literal attributes as raw strings without
    splitting list-like values.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-5-6">
    THE Parser SHALL collect nested child components recursively.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-5-7">
    THE Parser SHALL compute `body_text` from direct non-component text
    nodes within an element, trimming leading and trailing whitespace.
    Self-closing elements, or elements whose content is only child
    elements, SHALL have no body text.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Lint-Time Validation

As a parser consumer, I want per-file structural validation to happen during
parse, so that obvious authoring mistakes are caught before graph building.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE Parser SHALL skip unknown PascalCase element names during extraction
    rather than emitting errors. Only elements matching Component_Defs are
    extracted; all others are treated as transparent content wrappers whose
    children are still traversed.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-6-2">
    THE Parser SHALL emit `ParseError::MissingRequiredAttribute` when a known
    component is missing an attribute marked `required = true`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-6-3">
    THE Parser SHALL perform this validation using the Component_Defs supplied
    by the caller. When the caller uses built-in defaults, validation SHALL be
    limited to the built-in component set.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 7: ParseResult Assembly

As a parser consumer, I want one public entry point that clearly separates
successful documents, non-supersigil files, and parse failures, so that the
CLI and graph loader can handle them deterministically.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-7-1">
    WHEN parsing succeeds for a supersigil document, THE Parser SHALL return
    `ParseResult::Document(SpecDocument)` containing the file path,
    `Frontmatter`, extra metadata, and top-level extracted components.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/fixture_integration_tests.rs" />
  </Criterion>
  <Criterion id="req-7-2">
    WHEN the file has no supersigil front matter, or front matter without the
    `supersigil:` key, THE Parser SHALL return `ParseResult::NotSupersigil`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
  <Criterion id="req-7-3">
    THE Parser SHALL stop the pipeline after fatal front matter errors and
    after XML syntax errors. Within component extraction, code content
    resolution, and lint-time validation, independent parse errors SHALL be
    accumulated into one `Vec&lt;ParseError&gt;`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-parser/tests/unit_tests.rs" />
  </Criterion>
</AcceptanceCriteria>
```
