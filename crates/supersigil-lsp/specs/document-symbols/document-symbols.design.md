---
supersigil:
  id: document-symbols/design
  type: design
  status: approved
title: "LSP Document Symbols"
---

```supersigil-xml
<Implements refs="document-symbols/req" />
<TrackedFiles paths="crates/supersigil-lsp/src/**/*.rs, crates/supersigil-core/src/types.rs, crates/supersigil-parser/src/xml_parser.rs, crates/supersigil-parser/src/xml_extract.rs" />
```

## Overview

Add `textDocument/documentSymbol` support to the LSP server by mapping
each `ExtractedComponent` in a parsed `SpecDocument` to an LSP
`DocumentSymbol`. The response is hierarchical — nested components
produce nested symbols.

A prerequisite parser change adds `end_position` to
`ExtractedComponent` so that symbol ranges can span the full component.

## Prerequisite: End Positions in Parser

`ExtractedComponent` currently has only a start `position`. The LSP
`DocumentSymbol` requires both `range` (full span) and `selectionRange`
(name/ID within the opening tag). Without end positions, `range` would
be zero-width, breaking outline folding and selection highlighting.

### Change

Add `end_position: SourcePosition` to `ExtractedComponent`. The parser
already receives both `Event::Start` and `Event::End` from quick-xml,
so the end offset is available but not stored.

**`XmlNode::Element`**: Add `end_offset: usize` field, set from
`reader.buffer_position()` in the `Event::End` handler of
`parse_children`. For self-closing elements (`Event::Empty`), compute
end offset as `start_offset + event_bytes.len()`.

**`xml_extract.rs`**: Compute `end_position` from `end_offset` using
the existing `line_col()` helper, same pattern as `position`.

**`ExtractedComponent`**: Add `pub end_position: SourcePosition`. Not
optional — every component has an end.

This follows the existing pattern used by `body_text_offset` /
`body_text_end_offset`.

## Module: `document_symbols.rs`

A new module in `supersigil-lsp` with a single public function:

```rust
pub fn document_symbols(
    doc: &SpecDocument,
    content: &str,
) -> Vec<DocumentSymbol>;
```

**Parameters:**
- `doc`: The parsed spec document (from `file_parses`)
- `content`: The raw file content (for UTF-16 position conversion)

**Returns:** A `Vec<DocumentSymbol>` — one per top-level component, with
children nested recursively.

### Symbol Mapping

For each `ExtractedComponent`:

| Field | Value |
|---|---|
| `name` | `id` attribute if present, otherwise component name |
| `detail` | Component name when `id` is used as `name`; `None` otherwise |
| `kind` | See kind mapping below |
| `range` | `position` → `end_position`, converted to LSP positions |
| `selectionRange` | Start of component name in the opening tag (zero-width at `position`) |
| `children` | Recursive mapping of `component.children` |
| `tags` | `None` |
| `deprecated` | `None` |

### Kind Mapping

| Component | `SymbolKind` | Rationale |
|---|---|---|
| `Criterion` | `PROPERTY` | A verifiable property of the spec |
| `Task` | `EVENT` | A trackable work event |
| `Decision` | `INTERFACE` | A contract/interface choice |
| `Alternative` | `ENUM_MEMBER` | One option among several |
| `VerifiedBy` | `STRUCT` | Structural link component |
| `TrackedFiles` | `STRUCT` | Structural link component |
| `References` | `STRUCT` | Structural link component |
| `Implements` | `STRUCT` | Structural link component |
| `DependsOn` | `STRUCT` | Structural link component |
| All others | `OBJECT` | Generic container |

### Selection Range

The `selectionRange` must be contained within `range`. Use a zero-width
range at the component's start position. This is the simplest correct
approach — the selection highlights the opening tag location, which is
sufficient for navigation.

## Integration in `state.rs`

### Capability Advertisement

In `initialize`, when `supersigil.toml` is present, add
`document_symbol_provider: Some(OneOf::Left(true))` to
`ServerCapabilities`.

### Request Handler

Add a `textDocument/documentSymbol` handler that:

1. Resolves the URI to a file path
2. Looks up the `SpecDocument` in `file_parses`
3. Looks up the file content in `open_files` (or reads from disk)
4. Calls `document_symbols::document_symbols(doc, content)`
5. Returns `DocumentSymbolResponse::Nested(symbols)`

The handler uses the `&mut self` → `Arc` snapshot pattern: capture the
parse and content in the `&mut self` phase, then return a future.

## Testing Strategy

- **Unit tests** in `document_symbols.rs`: construct `SpecDocument`
  with known components (criteria, tasks, nested components) and assert
  the returned symbol names, kinds, detail, and nesting structure.
- **Position tests**: verify `range` and `selectionRange` are correctly
  converted from `SourcePosition` to LSP positions.
- **Edge cases**: empty document (no components), self-closing
  components, deeply nested components.
- **Parser tests**: verify `end_position` is correctly set for both
  regular and self-closing elements.
