---
supersigil:
  id: document-format/md-conversion-tasks
  type: tasks
  status: done
title: "Markdown + Supersigil-XML Conversion Tasks"
---

```supersigil-xml
<DependsOn refs="document-format/adr" />
<Implements refs="parser-pipeline/req" />
```

## Overview

These tasks implement the migration from MDX to Markdown with
`supersigil-xml` fences, covering Phases 1–3 of the conversion
roadmap: parser rewrite, config update, and spec file conversion.

Each task is TDD: tests before implementation, with `cargo nextest run`
green before moving on. The parser rewrite preserves the existing
public API surface (`parse_file`, `parse_content`, `ParseResult`,
`SpecDocument`, `ExtractedComponent`) so downstream crates compile
throughout.

## Phase 1: Parser

```supersigil-xml
<Task id="task-1" status="done">
  Add Markdown fence extraction. Parse the document body as standard
  Markdown (without MDX constructs) using the `markdown` crate. Identify
  `supersigil-xml` fences by language identifier and return a
  `MarkdownFences` struct containing their content and offsets. Write
  tests for: fence detection, language matching, offset tracking, and
  fences with no language metadata.
</Task>

<Task id="task-2" status="done" depends="task-1">
  Add XML subset parser for `supersigil-xml` fence content. Parse
  PascalCase elements with double-quoted string attributes, nesting,
  text content, and self-closing elements. Reject processing
  instructions, CDATA, DTD, namespaces, and unsupported entity
  references. Return structured XML nodes with source positions offset
  to the fence's location in the file. Write tests for: valid XML
  fragments, nested elements, self-closing elements, text content,
  attribute parsing, position offsetting, and error cases (unclosed
  tags, invalid attributes, unsupported XML features).
</Task>

<Task id="task-3" status="done" depends="task-2">
  Rewrite component extraction to walk XML nodes instead of the MDX
  AST. Extract known PascalCase components from parsed XML nodes using
  the same `ComponentDefs`-based filtering. Preserve existing behavior:
  unknown PascalCase elements are transparent wrappers, lowercase
  elements are ignored, attributes stored as raw strings, nested
  children collected recursively, `body_text` computed from text nodes.
  The output is the same `Vec&lt;ExtractedComponent&gt;` as before. Write
  tests mirroring the existing extraction tests but with XML input.
</Task>

<Task id="task-4" status="done" depends="task-3">
  Wire up the new pipeline in `parse_file` and `parse_content`.
  Replace the MDX pipeline (preprocess → frontmatter → `parse_mdx_body`
  → `extract_components` → `validate_components`) with the new pipeline
  (preprocess → frontmatter → `parse_markdown_body` → `extract_fences`
  → `parse_supersigil_xml` → `extract_components`
  → `validate_components`). The public API (`parse_file`,
  `parse_content`, `ParseResult`, `SpecDocument`) remains unchanged.
  Update `ParseError` variants: replace `MdxSyntaxError` with
  `XmlSyntaxError`. Write integration tests using fixture documents in
  the new format.
</Task>

<Task id="task-5" status="done" depends="task-4">
  Remove the MDX parsing dependency. Remove the `Constructs::mdx()`
  usage from the `markdown` crate configuration. Remove `extract.rs`
  MDX AST walking code that is no longer reachable. Clean up any
  remaining MDX-specific error variants or helper functions. Verify
  that `cargo clippy` reports no dead code warnings in the parser
  crate.
</Task>
```

## Phase 2: Config + file discovery

```supersigil-xml
<Task id="task-6" status="done" depends="task-4">
  Update the default paths glob. Change the default `paths` value in
  `Config` from `specs/**/*.mdx` to `specs/**/*.md`. Update the `init`
  command scaffold to emit `paths = ["specs/**/*.md"]`. Update config
  tests that assert on the default or scaffold value.
</Task>
```

## Phase 3: Convert spec files

```supersigil-xml
<Task id="task-7" status="done" depends="task-4, task-6">
  Write a conversion script. Create a one-shot script (or `supersigil`
  subcommand) that converts `.mdx` files to `.md` with `supersigil-xml`
  fences: extract components from the MDX AST, wrap them in
  `supersigil-xml` fences, preserve prose sections as-is, and rename
  the file.
</Task>

<Task id="task-8" status="done" depends="task-7">
  Convert all spec files. Run the conversion script against all `.mdx`
  files under `specs/` and `crates/*/specs/`. Convert test fixtures in
  `crates/supersigil-parser/tests/fixtures/`. Verify that
  `supersigil verify` passes on the converted files. Update
  `supersigil.toml` paths if needed.
</Task>

<Task id="task-9" status="done" depends="task-8">
  Update parser test fixtures. Replace all `.mdx` test fixtures with
  `.md` equivalents in the new format. Update fixture integration tests
  to reference the new file names. Ensure `cargo nextest run` passes
  for the full workspace.
</Task>
```

## Phase 6: LSP

```supersigil-xml
<Task id="task-10" status="done">
  Add fence-aware context detection to the LSP. Create a helper
  function `is_in_supersigil_fence(content, byte_offset) -&gt; bool` in
  the LSP crate that determines whether a byte position falls inside a
  `supersigil-xml` fenced code block. Use a lightweight scan for fence
  open/close delimiters (no full Markdown parse). Add this guard to
  the completion, hover, and definition handlers so they only trigger
  inside `supersigil-xml` fences or in frontmatter (for status
  completions). Write unit tests for: position inside fence (true),
  position outside fence (false), position in prose between fences
  (false), position in frontmatter (separate check), multiple fences,
  edge cases (on fence delimiter lines). Run full workspace tests.
</Task>

<Task id="task-11" status="done" depends="task-10">
  Update VSCode extension language registration. Change the document
  selector in `editors/vscode/src/extension.ts` from
  `language: "mdx"` to register for both `markdown` and `mdx`
  languages. This ensures the LSP activates for `.md` files (primary
  format) while maintaining backward compatibility with any `.mdx`
  files. Update the file watcher pattern if needed. Test that the
  extension activates for both `.md` and `.mdx` files.
</Task>
```
