---
supersigil:
  id: code-lenses/design
  type: design
  status: approved
title: "LSP: Code Lenses"
---

```supersigil-xml
<Implements refs="code-lenses/req" />
```

```supersigil-xml
<TrackedFiles paths="crates/supersigil-lsp/src/code_lens.rs, crates/supersigil-lsp/src/state.rs" />
```

## Overview

Implement `textDocument/codeLens` by walking a document's parsed component
tree and frontmatter, using the existing `DocumentGraph` reverse mappings
for reference counts and a cached evidence index for verification status.
All lens computation happens in a single pure function; the LSP handler is
thin routing.

## Architecture

### Core Function

A new module `code_lens.rs` with one public function:

```rust
pub fn build_code_lenses(
    doc: &SpecDocument,
    doc_id: &str,
    content: &str,
    graph: &DocumentGraph,
    evidence_by_target: Option<&HashMap<String, HashMap<String, Vec<EvidenceId>>>>,
) -> Vec<CodeLens>
```

The `content` parameter provides the raw file text, needed to locate the
frontmatter `id:` line by scanning for a line starting with `id:` between
the `---` delimiters. Falls back to line 0 if not found.

The function walks `doc.components` top-down in a single pass:

1. **Document lens** — computed from aggregated reference counts and
   overall criteria coverage. Placed on the frontmatter `id:` line.
2. **AcceptanceCriteria lens** — for each `AcceptanceCriteria` component,
   examines `.children` for `Criterion` components to compute scoped
   coverage. Placed on the component's source position line.
3. **Criterion lens** — for each `Criterion` with an `id` attribute,
   queries reference count and evidence. Placed on the component's
   source position line.

Lens positions use `SourcePosition.line` (1-based) converted to 0-based
LSP lines. Column-level UTF-16 precision is not needed since Code Lenses
span the entire line visually.

### Server Integration

In `state.rs`:

1. **Capability**: Add `code_lens_provider: Some(CodeLensOptions {
   resolve_provider: Some(false) })` to `ServerCapabilities`.

2. **Cached evidence index**: Add a field `evidence_by_target:
   HashMap<String, HashMap<String, Vec<EvidenceId>>>` to `SupersigilLsp`.
   Populated in `run_verify_and_publish` by cloning the `ArtifactGraph`'s
   secondary index. This avoids storing the full `ArtifactGraph` (which
   borrows `&DocumentGraph`).

3. **Handler**: `textDocument/codeLens` looks up the document from
   `file_parses` (falling back to `partial_file_parses`), reads buffer
   content from `open_files`, resolves the doc ID, and delegates to
   `build_code_lenses`.

### Reference Count Aggregation

The document-level reference count is the size of the union of all
document IDs across:
- `graph.references(doc_id, None)` (document-level refs)
- `graph.references(doc_id, Some(crit_id))` for each criterion found
  during the component walk
- `graph.implements(doc_id)`
- `graph.depends_on(doc_id)`

These are deduplicated into a `HashSet` before counting. This ensures
fragment-level references (e.g., `refs="doc#crit-a"`) are included in
the document-level count.

Criterion-level reference count uses `graph.references(doc_id,
Some(criterion_id)).len()` directly — this returns a `BTreeSet<String>`
of unique referencing document IDs.

### Click Actions

Lenses that include a reference count carry a `Command`:
- `command`: `"supersigil.findReferences"`
- `arguments`: `[uri, position]` — the document URI (string) and the
  lens position (`{ line, character }` object)

Each editor plugin registers a handler for this command that converts
the raw JSON arguments to native types and delegates to the editor's
built-in Find References action.

Lenses showing only coverage or verification status have `command: None`.

### Update Timing

Lenses update on save only, matching the existing diagnostics refresh
cycle. The client re-requests `codeLens` after receiving updated
diagnostics.

## Error Handling

- Document not found in `file_parses` or `partial_file_parses`: return
  empty list.
- No buffer content in `open_files`: fall back to reading from disk
  (same pattern as other handlers).
- `evidence_by_target` is `None` (verify not configured): omit all
  verification and coverage parts. AcceptanceCriteria lenses are omitted
  entirely since they show only coverage.
- `evidence_by_target` is `Some` but empty (verify ran, no evidence):
  show "0/N" and "not verified" as appropriate.

## Testing Strategy

Unit tests in `code_lens.rs` for the pure `build_code_lenses` function
using in-memory `SpecDocument` + `DocumentGraph` + evidence maps
constructed via `test_helpers`. No LSP transport needed. Tests cover all
formatting variants, position correctness, scoped AcceptanceCriteria
coverage, click action presence, and behavior with/without verify data.

## Decisions

```supersigil-xml
<Decision id="no-resolve">
  <References refs="code-lenses/req#req-5-1" />
  Compute lens content eagerly in the codeLens request rather than using
  the two-phase codeLens + codeLens/resolve pattern.

  <Rationale>
    All required data (graph, evidence index) is already computed and
    cached in server state at save time. There is nothing expensive to
    defer. The resolve round-trip would add latency and complexity with
    no performance benefit.
  </Rationale>

  <Alternative id="use-resolve" status="rejected">
    Return skeleton lenses from codeLens and fill in titles via
    codeLens/resolve. Rejected because data is already available and
    the extra round-trip is pure overhead for small graphs.
  </Alternative>
</Decision>

<Decision id="evidence-cache">
  <References refs="code-lenses/req#req-2-1, code-lenses/req#req-3-1" />
  Store a cloned HashMap of the ArtifactGraph's evidence_by_target
  secondary index rather than the full ArtifactGraph struct.

  <Rationale>
    ArtifactGraph borrows the DocumentGraph, creating lifetime
    entanglement with the mutable server state. Cloning just the
    secondary index is cheap (evidence sets are small) and avoids
    the borrow.
  </Rationale>

  <Alternative id="store-artifact-graph" status="rejected">
    Store the full ArtifactGraph with an owned DocumentGraph clone.
    Rejected because cloning the entire graph is wasteful when only
    the evidence index is needed.
  </Alternative>
</Decision>

<Decision id="vscode-command">
  <References refs="code-lenses/req#req-4-1" />
  Use a custom supersigil.findReferences command as the lens click
  action. Each editor plugin converts the raw JSON arguments (URI
  string, position object) to native types and delegates to the
  editor's built-in Find References action.

  <Rationale>
    The LSP protocol transmits command arguments as raw JSON.
    Editor-specific built-in commands (e.g. VS Code's
    editor.action.findReferences) expect typed objects (Uri, Position),
    not plain JSON. A proxy command in the editor plugin performs the
    conversion, keeping the LSP editor-agnostic while ensuring
    click actions work correctly.
  </Rationale>

  <Alternative id="direct-editor-command" status="rejected">
    Send editor.action.findReferences directly from the LSP with
    raw JSON arguments. Rejected because vscode-languageclient passes
    arguments through as-is, causing "Unexpected type" errors when
    VS Code tries to interpret plain strings/objects as typed Uri and
    Position instances.
  </Alternative>
</Decision>
```
