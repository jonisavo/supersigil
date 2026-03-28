---
supersigil:
  id: rename/req
  type: requirements
  status: implemented
title: "LSP: Rename"
---

## Introduction

Add `textDocument/rename` and `textDocument/prepareRename` support to the
supersigil LSP server. Given a cursor position on a document ID or component
ID, rename the identifier and update all references across the spec tree in
a single workspace edit. Natural complement to the existing Find All
References feature (`find-all-references/req`).

In scope: the `textDocument/rename` and `textDocument/prepareRename` handlers,
cursor-sensitive rename target detection, text edit collection across all
referencing documents, and new-name validation. Out of scope: file rename/move
on disk, renaming component type names (e.g. `Criterion`), renaming across
projects.

## Definitions

- **Rename_Target**: Either a `ComponentId(doc_id, component_id)` or a
  `DocumentId(doc_id)` identifying what the user intends to rename. Derived
  from cursor position.
- **Ref_Part**: Which portion of a ref string the cursor is on: the document
  ID portion (before `#`) or the fragment portion (after `#`).
- **Line_Range**: A byte-offset range within a single source line, identifying
  the text span of the rename target at the cursor site.

## Requirement 1: Rename Target Detection

As a spec author, I want rename to work from multiple cursor positions, so
that I can rename identifiers regardless of whether I'm at a definition or
a reference site.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN the cursor is on the fragment portion of a ref string inside a
    `refs`, `implements`, `depends`, or `verifies` attribute within a
    `supersigil-xml` fence, THE server SHALL identify the Rename_Target
    as a ComponentId.
  </Criterion>
  <Criterion id="req-1-2">
    WHEN the cursor is on the document ID portion of a ref string (before
    the `#` or the entire value when there is no `#`) inside a ref attribute
    within a `supersigil-xml` fence, THE server SHALL identify the
    Rename_Target as a DocumentId.
  </Criterion>
  <Criterion id="req-1-3">
    WHEN the cursor is on a `supersigil-ref=&lt;target&gt;` token in a code
    fence info string, THE server SHALL identify the Rename_Target as a
    ComponentId using the current document's ID and the target value as the
    component ID.
  </Criterion>
  <Criterion id="req-1-4">
    WHEN the cursor is on a component tag name or on the `id` attribute
    value inside a `supersigil-xml` fence and the component has an `id`
    attribute, THE server SHALL identify the Rename_Target as a ComponentId
    using the document's ID and the component's `id` attribute value.
  </Criterion>
  <Criterion id="req-1-5">
    WHEN the cursor is on the `id:` value in YAML frontmatter, THE server
    SHALL identify the Rename_Target as a DocumentId.
  </Criterion>
  <Criterion id="req-1-6">
    THE server SHALL check cursor positions in priority order: ref attribute,
    supersigil-ref info string, component definition tag / id attribute,
    frontmatter. The first match wins.
  </Criterion>
  <Criterion id="req-1-7">
    WHEN the cursor is not on any renameable position, THE server SHALL
    reject the rename with an error.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Prepare Rename

As a spec author, I want the editor to highlight the renameable text and
pre-fill the current identifier when I invoke rename, so that I can see
exactly what will change and start typing the new name.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE server SHALL respond to `textDocument/prepareRename` with a range
    covering exactly the renameable text span and the current identifier
    as a placeholder.
  </Criterion>
  <Criterion id="req-2-2">
    WHEN the cursor is on the fragment portion of a ref string, THE range
    SHALL cover only the fragment portion (after `#`), not the full ref.
  </Criterion>
  <Criterion id="req-2-3">
    WHEN the cursor is on the document ID portion of a ref string, THE
    range SHALL cover only the document ID portion (before `#`), not the
    full ref.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Edit Collection

As a spec author, I want all references updated in a single operation, so
that renaming an identifier does not leave broken references behind.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN renaming a ComponentId, THE server SHALL produce text edits that
    update the `id` attribute at the definition site, all matching fragment
    portions in ref attributes across all documents, and all matching
    `supersigil-ref=` tokens in code fence info strings.
  </Criterion>
  <Criterion id="req-3-2">
    WHEN renaming a DocumentId, THE server SHALL produce text edits that
    update the frontmatter `id:` value at the definition site and all
    matching document ID portions in ref attributes across all documents.
  </Criterion>
  <Criterion id="req-3-3">
    THE server SHALL scan all four ref attributes (`refs`, `implements`,
    `depends`, `verifies`) for matching references.
  </Criterion>
  <Criterion id="req-3-4">
    THE server SHALL group edits by file URI and return them as a single
    `WorkspaceEdit`.
  </Criterion>
  <Criterion id="req-3-5">
    WHEN no references exist beyond the definition site, THE server SHALL
    still produce the edit at the definition site.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: New Name Validation

As a spec author, I want the server to reject invalid names before applying
edits, so that rename does not produce broken documents.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    THE server SHALL reject a new name that is empty, contains whitespace,
    contains `#`, or contains `"`.
  </Criterion>
  <Criterion id="req-4-2">
    WHEN validation fails, THE server SHALL return a ResponseError with a
    descriptive message.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Capability Registration

As an editor, I need the server to advertise rename support so that the
editor enables the rename UI.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE server SHALL advertise `rename_provider` with `prepare_provider:
    true` in its `ServerCapabilities` when `supersigil.toml` is present.
  </Criterion>
</AcceptanceCriteria>
```
