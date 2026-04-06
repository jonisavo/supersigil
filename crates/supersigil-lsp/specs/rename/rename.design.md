---
supersigil:
  id: rename/design
  type: design
  status: approved
title: "LSP: Rename"
---

```supersigil-xml
<Implements refs="rename/req" />
```

```supersigil-xml
<TrackedFiles paths="crates/supersigil-lsp/src/rename.rs, crates/supersigil-lsp/src/definition.rs, crates/supersigil-lsp/src/state.rs" />
```

## Overview

Implement `textDocument/rename` and `textDocument/prepareRename` by enriching
the existing `find_ref_at_position()` with cursor-part and span information,
then building a rename-specific target detector and edit collector. The edit
collector reuses the `DocumentGraph` reverse mappings to identify referencing
documents, then scans file contents to produce precise `TextEdit`s at the
byte positions of old identifiers.

## Architecture

### Enriched Cursor Detection

`find_ref_at_position()` in `definition.rs` is refactored from
`Option<String>` to `Option<RefAtPosition>`:

```rust
pub enum RefPart {
    /// Cursor is on the document ID portion (before `#`), or no `#` present.
    DocId,
    /// Cursor is on the fragment portion (after `#`).
    Fragment,
}

pub struct RefAtPosition {
    /// The full ref string (e.g. "auth/req#crit-a").
    pub ref_string: String,
    /// Which part of the ref the cursor is on.
    pub part: RefPart,
    /// Byte offset within the line where the relevant part starts.
    pub part_start: u32,
    /// Byte offset within the line where the relevant part ends (exclusive).
    pub part_end: u32,
}
```

Three existing callers must be updated to use `.ref_string`:

1. `state.rs` definition handler (passes result to `resolve_ref()`)
2. `references.rs` `find_reference_target()` (passes result to
   `parse_ref_target()`)
3. `hover.rs` (passes result to `hover_ref()`)

No behavior change for these callers; they destructure the new type.

### Rename Target Detection

A new `find_rename_target()` function in `rename.rs`:

```rust
pub struct LineRange {
    pub line: u32,
    /// Byte offset of range start within the line.
    pub start: u32,
    /// Byte offset of range end (exclusive) within the line.
    pub end: u32,
}

pub enum RenameTarget {
    ComponentId {
        doc_id: String,
        component_id: String,
        range: LineRange,
    },
    DocumentId {
        doc_id: String,
        range: LineRange,
    },
}

pub fn find_rename_target(
    content: &str, line: u32, character: u32, doc_id: &str,
) -> Option<RenameTarget>;
```

Detection priority (same order as `find_reference_target()` in
`references.rs`):

1. **Ref attribute** — via enriched `find_ref_at_position()`.
   `RefPart::Fragment` yields `ComponentId`; `RefPart::DocId` yields
   `DocumentId`. Range from `part_start`/`part_end`.

2. **Component tag or `id` attribute value** — when the cursor is on a
   component tag name (via `component_name_at_position()`) or directly on
   an `id="..."` attribute value, and the component has an `id` attribute.
   Yields `ComponentId`. Range covers the `id` attribute value.

3. **Frontmatter `id:`** — when the cursor is on the `id:` value line
   within YAML frontmatter. Yields `DocumentId`. Range covers the value.

`find_rename_target` delegates to the same helper functions as
`find_reference_target` (`find_ref_at_position`, `component_name_at_position`,
`extract_id_attribute_on_line`, `is_in_frontmatter`) to avoid logic
duplication.

### New Name Validation

Before producing edits, the rename handler validates `new_name`:

- Must be non-empty.
- Must not contain whitespace, `#`, or `"`.

If validation fails, the handler returns a `ResponseError` with a descriptive
message. The handler does not check for ID collisions; that is left to the
normal verification diagnostics cycle after the edit is applied.

### Edit Collection

```rust
pub fn collect_rename_edits(
    target: &RenameTarget,
    new_name: &str,
    graph: &DocumentGraph,
    open_files: &HashMap<Url, Arc<String>>,
) -> WorkspaceEdit;
```

File contents come from `open_files` when available (keyed by URI), falling
back to disk reads via `graph.document(doc_id).path` for closed files.

**ComponentId rename** — rename `component_id` to `new_name`:

1. **Definition site**: in the owning document, find the `id="old"` attribute
   and replace the value.
2. **Ref attributes**: for every document referencing `doc_id#component_id`
   (via `graph.references()`), scan file content for `refs`, `implements`,
   `depends` attributes containing `doc_id#old` and replace the fragment
   portion after `#`.
3. **Task implements**: for documents in `graph.implements(doc_id)`, scan
   `task_implements_for_doc` entries matching the fragment and replace.

**DocumentId rename** — rename `doc_id` to `new_name`:

1. **Frontmatter**: in the owning document, edit the `id:` value.
2. **Ref attributes**: for every document referencing `doc_id` (via
   `graph.references()`, `graph.implements()`, `graph.depends_on()`),
   scan for the doc ID portion in ref strings and replace.
3. **Task implements**: for documents in `graph.implements(doc_id)`, scan
   `task_implements_for_doc` entries where the target doc ID matches and
   replace the doc ID portion.

All `TextEdit` ranges use UTF-16 column offsets, converted from byte offsets
using the existing `position::source_to_lsp_utf16` helpers. The server
advertises `PositionEncodingKind::UTF16`.

### LSP Handlers

The `fn prepare_rename()` handler in `state.rs` follows the same pattern as
existing handlers: clone content Arc and graph Arc, resolve file path to doc
ID via `file_parses`, call `find_rename_target()`, then return
`PrepareRenameResponse::RangeWithPlaceholder` with the range (converted to
UTF-16) and current ID as placeholder. Returns an error if the cursor is not
on a renameable target.

The `fn rename()` handler calls `find_rename_target()`, validates `new_name`,
then calls `collect_rename_edits()` to build and return the `WorkspaceEdit`.

Capability registration adds `rename_provider: Some(OneOf::Right(RenameOptions
{ prepare_provider: Some(true), work_done_progress_options:
WorkDoneProgressOptions::default() }))` to `ServerCapabilities`.

## Error Handling

- Cursor outside any detection zone: `find_rename_target` returns `None`,
  `prepareRename` returns an error, `rename` returns an error.
- Invalid new name: `rename` returns a `ResponseError`.
- File read failures during edit collection: skip the file (edits for
  unreachable files are omitted).
- File read failures during position conversion: fall back to
  `source_to_lsp()` without UTF-16 adjustment (same as `definition.rs`).

## Testing Strategy

Unit tests in `rename.rs` for each `find_rename_target` cursor detection
case and for `collect_rename_edits` using constructed `DocumentGraph`
instances (same `test_graph` helper pattern as `references.rs`). Edge cases:
no references, unknown targets, validation failures, multi-file edits.

## Decisions

```supersigil-xml
<Decision id="enrich-find-ref" standalone="Enrich find_ref_at_position rather than duplicate cursor detection">
  Refactor `find_ref_at_position()` to return a richer `RefAtPosition` struct
  with cursor-part and span information, rather than building a parallel
  detection function for rename.

  <Rationale>
    Cursor detection for ref attributes is already implemented and tested in
    `find_ref_at_position()`. Duplicating that logic for rename would create
    two code paths that could drift. The richer return type benefits all
    consumers (definition, references, hover, rename) and keeps the detection
    logic in a single place. The three existing callers need only trivial
    updates to destructure `.ref_string`.
  </Rationale>

  <Alternative id="parallel-detection" status="rejected">
    Build a separate rename-specific cursor detector that re-implements ref
    attribute scanning. Rejected because it duplicates tested logic and
    creates maintenance risk.
  </Alternative>
</Decision>

<Decision id="text-only-rename" standalone="No file rename/move on disk for document ID renames">
  Document ID renames update the frontmatter `id:` value and all reference
  strings, but do not rename or move the file on disk.

  <Rationale>
    Document IDs and file paths are not tightly coupled. The mapping depends
    on the project's prefix configuration, so deriving the correct new file
    path from a new document ID would require reverse-mapping through the
    config. Keeping rename text-only avoids this complexity and the risk of
    moving files to wrong locations. File moves can be offered as a separate
    code action in a future iteration.
  </Rationale>

  <Alternative id="file-rename" status="deferred">
    Include a `RenameFile` resource operation in the `WorkspaceEdit` for
    document ID renames. Deferred until a reliable ID-to-path reverse mapping
    is available.
  </Alternative>
</Decision>

<Decision id="no-collision-check" standalone="No ID collision validation during rename">
  The rename handler does not check whether the new name collides with an
  existing document ID or component ID.

  <Rationale>
    Collision detection would require querying the graph for the new name
    before applying edits. Since the LSP server already produces diagnostics
    on save (via `supersigil verify`), any collisions will surface as
    diagnostics immediately after the rename is applied. Adding a pre-check
    duplicates validation logic and may produce false positives when the
    graph is stale.
  </Rationale>
</Decision>
```
