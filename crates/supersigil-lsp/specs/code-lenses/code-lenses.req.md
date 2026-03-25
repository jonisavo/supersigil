---
supersigil:
  id: code-lenses/req
  type: requirements
  status: implemented
title: "LSP: Code Lenses"
---

## Introduction

Add `textDocument/codeLens` support to the supersigil LSP server, showing
inline metadata above components in spec documents: reference counts,
verification status, and coverage percentages. This makes verification
status visible in the editor without running the CLI.

In scope: Code Lenses for Document (frontmatter), AcceptanceCriteria, and
Criterion components. Out of scope: Task lenses, Example lenses, per-lens
configuration toggle, `codeLens/resolve`.

## Definitions

- **Code_Lens**: An LSP `CodeLens` object — a line of non-editable text
  displayed above a source line, optionally with a click action (Command).
- **Evidence**: A verification evidence record from the artifact graph,
  linking a test to a verifiable criterion.
- **Coverage_Percentage**: The ratio of criteria with at least one evidence
  record to total criteria, expressed as an integer percentage.

## Requirement 1: Document Lens

As a spec author, I want to see a summary of reference count and coverage
percentage above the document frontmatter, so that I can gauge document
health at a glance.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN a document contains criteria and has incoming references, THE
    server SHALL display a Code_Lens on the frontmatter `id:` line showing
    both the reference count and the Coverage_Percentage in the format
    "{N} references | {M}/{T} criteria verified ({P}%)".
  </Criterion>
  <Criterion id="req-1-2">
    WHEN a document has no criteria (T=0), THE server SHALL display a
    Code_Lens showing only the reference count in the format
    "{N} references".
  </Criterion>
  <Criterion id="req-1-3">
    WHEN a document has criteria but no incoming references (N=0), THE
    server SHALL display a Code_Lens showing only the coverage in the
    format "{M}/{T} criteria verified ({P}%)".
  </Criterion>
  <Criterion id="req-1-4">
    WHEN a document has no incoming references and no criteria, THE server
    SHALL NOT emit a document-level Code_Lens.
  </Criterion>
  <Criterion id="req-1-5">
    The document-level reference count SHALL be the number of unique
    documents that reference this document or any of its components,
    aggregated across References, Implements, and DependsOn relationship
    types and deduplicated into a single set.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: AcceptanceCriteria Lens

As a spec author, I want to see aggregate coverage above each
AcceptanceCriteria block, so that I can assess verification progress
per criteria group.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    WHEN an AcceptanceCriteria block contains Criterion children, THE
    server SHALL display a Code_Lens on the AcceptanceCriteria component
    line showing "{M}/{T} criteria verified ({P}%)" scoped to the child
    criteria of that specific block.
  </Criterion>
  <Criterion id="req-2-2">
    WHEN verification data is unavailable (diagnostics tier below Verify),
    THE server SHALL NOT emit an AcceptanceCriteria Code_Lens.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Criterion Lens

As a spec author, I want to see reference count and verification status
above each Criterion, so that I can identify unverified or unreferenced
criteria.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN a Criterion has incoming references and Evidence, THE server SHALL
    display a Code_Lens in the format "{N} references | verified ({E} tests)".
  </Criterion>
  <Criterion id="req-3-2">
    WHEN a Criterion has incoming references but no Evidence, THE server
    SHALL display a Code_Lens in the format "{N} references | not verified".
  </Criterion>
  <Criterion id="req-3-3">
    WHEN a Criterion has no incoming references but has Evidence, THE server
    SHALL display a Code_Lens in the format "verified ({E} tests)".
  </Criterion>
  <Criterion id="req-3-4">
    WHEN a Criterion has no incoming references and no Evidence, THE server
    SHALL display a Code_Lens with the text "not verified".
  </Criterion>
  <Criterion id="req-3-5">
    WHEN verification data is unavailable (diagnostics tier below Verify),
    THE server SHALL omit the verification portion and show only reference
    counts. If there are also no references, no lens is emitted.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Click Actions

As a spec author, I want to click a reference count to find all references,
so that I can navigate to referencing documents.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    Code_Lenses that include a reference count SHALL have a Command that
    triggers Find All References at the lens position.
  </Criterion>
  <Criterion id="req-4-2">
    Code_Lenses that show only coverage or verification status (no
    reference count) SHALL have no Command (informational only).
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Capability Registration

As an editor, I need the server to advertise code lens support so that the
editor enables the Code Lens UI.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE server SHALL advertise `code_lens_provider` in its
    ServerCapabilities when `supersigil.toml` is present, with
    `resolve_provider` set to false.
  </Criterion>
</AcceptanceCriteria>
```
