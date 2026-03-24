---
supersigil:
  id: find-all-references/req
  type: requirements
  status: implemented
title: "LSP: Find All References"
---

## Introduction

Add `textDocument/references` support to the supersigil LSP server. Given a
document ID, component fragment, or ref string under the cursor, return all
locations in the spec graph that reference the same target. Natural complement
to the existing Go to Definition feature (Requirement 2 in `lsp-server/req`).

In scope: the `textDocument/references` handler, cursor detection for four
entry points, reference collection via existing graph reverse mappings, and
new graph accessor methods. Out of scope: editor extension changes (references
is a standard LSP capability that editors handle natively).

## Definitions

- **Reference_Target**: A `(doc_id, Option<fragment>)` pair identifying the
  document or component being referenced. Derived from cursor position.
- **Reverse_Mapping**: The existing `DocumentGraph` indexes that track which
  documents reference, implement, or depend on a given target.

## Requirement 1: Cursor Detection

As a spec author, I want Find All References to work from multiple cursor
positions, so that I can discover incoming references regardless of whether
I'm looking at a definition or a reference site.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN the cursor is on a ref string inside a `refs`, `implements`, or
    `depends` attribute within a `supersigil-xml` fence, THE server SHALL
    resolve the ref to a Reference_Target and find all incoming references
    to that target.
  </Criterion>
  <Criterion id="req-1-2">
    WHEN the cursor is on a `supersigil-ref=&lt;target&gt;` token in a code fence
    info string (outside `supersigil-xml` fences), THE server SHALL parse
    the target, resolve it to the Example component in the same document,
    and find all incoming references to that component.
  </Criterion>
  <Criterion id="req-1-3">
    WHEN the cursor is on a component definition tag
    (e.g. Criterion with id="login")
    inside a `supersigil-xml` fence and the component has an `id` attribute,
    THE server SHALL resolve the component to a Reference_Target using the
    document's ID and the component's `id` attribute, and find all incoming
    references.
  </Criterion>
  <Criterion id="req-1-4">
    WHEN the cursor is anywhere in the YAML frontmatter (between the opening
    and closing `---` delimiters), THE server SHALL resolve the current
    document as the Reference_Target (document-level, no fragment) and find
    all incoming references.
  </Criterion>
  <Criterion id="req-1-5">
    THE server SHALL check cursor positions in priority order: ref string,
    supersigil-ref info string, component definition tag, frontmatter. The
    first match wins.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Reference Collection

As a spec author, I want to see all places that reference a target, regardless
of relationship type, so that I can understand the full dependency surface.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    GIVEN a Reference_Target, THE server SHALL collect all locations where
    a `refs`, `implements`, or `depends` attribute references that target,
    across all documents in the graph.
  </Criterion>
  <Criterion id="req-2-2">
    Each returned location SHALL point to the source component's opening
    tag position, not the individual ref string within a comma-separated
    attribute value.
  </Criterion>
  <Criterion id="req-2-3">
    WHEN the request's `context.includeDeclaration` is true, THE server
    SHALL include the target's own definition location as the first result.
    For fragment targets this is the component's source position; for
    document-level targets this is position (0,0) of the file.
  </Criterion>
  <Criterion id="req-2-4">
    WHEN no incoming references exist, THE server SHALL return an empty
    list (not an error).
  </Criterion>
  <Criterion id="req-2-5">
    WHEN the Reference_Target does not exist in the graph (unknown doc ID
    or fragment), THE server SHALL return an empty list.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Capability Registration

As an editor, I need the server to advertise references support so that the
editor enables the "Find All References" UI.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    THE server SHALL advertise `references_provider` in its
    `ServerCapabilities` when `supersigil.toml` is present.
  </Criterion>
</AcceptanceCriteria>
```
