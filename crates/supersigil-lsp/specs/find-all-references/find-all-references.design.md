---
supersigil:
  id: find-all-references/design
  type: design
  status: approved
title: "LSP: Find All References"
---

```supersigil-xml
<Implements refs="find-all-references/req" />
```

```supersigil-xml
<TrackedFiles paths="crates/supersigil-lsp/src/references.rs, crates/supersigil-core/src/graph.rs" />
```

## Overview

Implement `textDocument/references` by reusing the existing `DocumentGraph`
reverse mappings (`references_reverse`, `implements_reverse`,
`depends_on_reverse`) to identify source documents, then scanning
`resolved_refs` and `task_implements` to recover source component positions.
No new graph indexes are needed; the approach trades O(source_docs * components)
scan time per request for zero changes to the graph construction pipeline.

## Architecture

### Cursor Detection

A new `find_reference_target()` function resolves the cursor position to a
`(doc_id, Option<fragment>)` target. Three detection strategies are tried in
priority order:

1. **Ref string** — reuses `find_ref_at_position()` from `definition.rs`.
   Works inside `supersigil-xml` fences, on `refs`/`implements`/`depends`
   attribute values.

2. **Component definition tag** — extends the pattern from
   `component_name_at_position()` in `hover.rs`. When the cursor is on a
   `<Tag` and the same line contains `id="<value>"`, extracts the ID and
   combines with the document ID. Multi-line tags where `id` is on a
   different line than the tag name are not supported (returns None).

3. **Frontmatter** — detects cursor between the first two `---` lines.
   Uses the document ID passed in by the handler (resolved from `file_parses`
   in `state.rs`).

### Reference Collection

`collect_references()` takes a target and returns `Vec<Location>`:

1. Query reverse mappings for source doc IDs (fast filter).
2. For each source doc, use `resolved_refs_for_doc()` to iterate all
   resolved refs from that document. When a `ResolvedRef` matches the
   target, use `component_at_path()` to get the source component's position.
3. For `task_implements`, use `task_implements_for_doc()` to find matching
   task targets, then `graph.component(src_id, task_id)` to get the Task
   component's position (Tasks are referenceable and indexed in
   `component_index`).
4. Convert each `SourcePosition` + file path to an LSP `Location`.
5. If `include_declaration` is true, prepend the target's own location
   using `resolve_ref()` from `definition.rs`.

### LSP Handler

The `fn references()` handler in `state.rs` follows the same pattern as
`fn definition()`: clone the content Arc and graph Arc, resolve file path
to doc ID via `file_parses`, then `Box::pin(async move { ... })`.

## Key Types

New public methods on `DocumentGraph`:

```rust
/// Walk the component tree by index path to get a component reference.
/// Promotes existing private `resolve_component_path` from `reverse.rs`.
pub fn component_at_path(
    &self, doc_id: &str, path: &[usize]
) -> Option<&ExtractedComponent>;

/// Iterate all resolved refs originating from a document.
pub fn resolved_refs_for_doc(
    &self, doc_id: &str
) -> impl Iterator<Item = (&[usize], &[ResolvedRef])>;

/// Iterate all task implements entries from a document.
pub fn task_implements_for_doc(
    &self, doc_id: &str
) -> impl Iterator<Item = (&str, &[(String, String)])>;
```

New functions in `references.rs`:

```rust
pub fn find_reference_target(
    content: &str, line: u32, character: u32,
    doc_id: &str, graph: &DocumentGraph,
) -> Option<(String, Option<String>)>;

pub fn collect_references(
    target_doc: &str, target_fragment: Option<&str>,
    include_declaration: bool, graph: &DocumentGraph,
) -> Vec<Location>;
```

## Error Handling

- Unknown doc ID or fragment: return empty results (no error).
- File read failures during position conversion: fall back to
  `source_to_lsp()` without UTF-16 adjustment (same as `definition.rs`).
- Cursor outside any detection zone: return `None` from target detection,
  handler returns empty.

## Testing Strategy

Unit tests in `references.rs` for each cursor detection case (ref string,
component tag, frontmatter) and for `collect_references` using constructed
`DocumentGraph` instances. Unit test for
`component_at_path` in graph tests. Edge cases: empty results, unknown
targets, `includeDeclaration` flag.

## Decisions

```supersigil-xml
<Decision id="scan-vs-index" standalone="Reference collection strategy">
  Scan resolved_refs and task_implements at request time to recover source
  component positions, rather than building a dedicated reverse-with-positions
  index during graph construction.

  <Rationale>
    Supersigil graphs are small (tens to low hundreds of documents). The scan
    is O(source_docs * components_per_doc) per request, which is negligible
    at this scale. Scanning at request time requires no changes to the graph
    construction pipeline or core data structures. If profiling ever shows a
    bottleneck, a dedicated index can be added as an optimization without
    changing the LSP handler interface.
  </Rationale>

  <Alternative id="enrich-reverse-mappings" status="rejected">
    Enrich existing reverse mappings with source positions. Rejected because
    it changes the shape of existing BTreeSet-based reverse mappings used by
    context and plan queries, causing cascading changes.
  </Alternative>

  <Alternative id="dedicated-index" status="deferred">
    Build a separate reverse-with-positions index during graph construction.
    Deferred because scan performance is adequate for expected graph sizes.
  </Alternative>
</Decision>

<Decision id="source-position-granularity" standalone="Location precision for reference results">
  Point each result location to the source component's opening tag position
  rather than the exact character range of the individual ref string within
  a comma-separated attribute value.

  <Rationale>
    Pinpointing the exact ref within a comma-separated list (e.g. the
    "auth/req#login" portion of refs="foo/req#bar, auth/req#login") would
    require re-parsing the attribute value at the LSP layer and tracking
    per-ref byte offsets that the parser does not currently emit. Pointing
    to the component tag is precise enough to land on the right line, and
    the ref is visible in context. The resolved_refs data structure stores
    positions at the component level, not per-ref.
  </Rationale>

  <Alternative id="per-ref-positions" status="deferred">
    Track per-ref byte offsets during parsing and return exact character
    ranges. Deferred until user feedback indicates the component-level
    granularity is insufficient.
  </Alternative>
</Decision>

<Decision id="frontmatter-trigger" standalone="Frontmatter as document-level reference trigger">
  Treat the entire YAML frontmatter block (between the opening and closing
  --- delimiters) as the trigger zone for document-level Find All References,
  rather than requiring the cursor to be on the specific id field.

  <Rationale>
    Detecting the frontmatter block is a simple line scan for --- delimiters.
    Detecting the specific id field would require YAML parsing at the LSP
    layer. The frontmatter is short (typically 4-6 lines) and conceptually
    represents the document's identity, so treating it as a single trigger
    zone is both simpler and more discoverable.
  </Rationale>
</Decision>
```
