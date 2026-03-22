---
supersigil:
  id: ref-discovery/req
  type: requirements
  status: implemented
title: "Context-Aware Criterion Ref Discovery"
---

## Introduction

When annotating code with `#[verifies("...")]` or writing spec cross-references,
users need to know the exact canonical criterion ref (`doc-id#criterion-id`).
Today the only way to discover refs is to open spec files and manually combine
the document ID with a criterion's `id` attribute, or trial-and-error through
`supersigil verify`.

This feature adds a **Criterion_Ref** query surface that lets users discover
criterion refs from the CLI, with optional context-aware scoping that uses
`TrackedFiles` globs to show only refs relevant to the current working
directory.

### In scope

- A core graph primitive for iterating and searching criterion refs.
- A CLI `refs` command with context-aware default scoping.
- Integration points for verify-time and compile-time suggestions.

### Out of scope

- Changes to the `#[verifies]` proc-macro error messages (separate feature).
- Full-text search over criterion body text.

## Definitions

- **Criterion_Ref**: A canonical reference string in `doc-id#criterion-id` form
  that uniquely identifies a verifiable criterion within a supersigil project.
- **Context_Scope**: The set of spec documents whose `TrackedFiles` globs match
  at least one path under the current working directory, expanded by following
  `Implements` relationships from matched documents, used to narrow results to
  the architecturally relevant neighbourhood.
- **Fragment_Lookup**: A search by bare criterion ID (without the document
  prefix), returning all matching Criterion_Refs across all documents.

## Requirement 1: Core graph primitives

As a developer building query features, I want the `DocumentGraph` to expose
criterion iteration and fragment lookup, so that CLI commands and verification
rules can discover refs without re-walking the component tree.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE `DocumentGraph` SHALL expose a method that iterates all referenceable
    components in the component index, yielding the owning document ID,
    fragment ID, and component metadata for each entry.
    <VerifiedBy
      strategy="file-glob"
      paths="crates/supersigil-core/src/graph/tests/unit.rs"
    />
  </Criterion>
  <Criterion id="req-1-2">
    THE `DocumentGraph` SHALL expose a Fragment_Lookup method that, given a bare
    fragment ID string, returns all `(doc_id, component)` pairs whose fragment
    matches, by scanning the existing component index.
    <VerifiedBy
      strategy="file-glob"
      paths="crates/supersigil-core/src/graph/tests/unit.rs"
    />
  </Criterion>
</AcceptanceCriteria>

## Requirement 2: CLI `refs` command

As a developer annotating test code, I want a CLI command that lists criterion
refs I can copy-paste into `#[verifies("...")]`, so that I don't have to
reverse-engineer the ref format from spec files.

<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE `refs` command SHALL load the graph and list all Criterion_Refs across
    the project, displaying the full `doc-id#criterion-id` string and the
    criterion body text for each entry.
  </Criterion>
  <Criterion id="req-2-2">
    THE `refs` command SHALL accept an optional positional prefix argument. WHEN
    provided, it SHALL filter results to criterion refs whose document ID starts
    with the given prefix.
  </Criterion>
  <Criterion id="req-2-3">
    THE `refs` command SHALL support `--format terminal|json`, with terminal as
    the default format.
  </Criterion>
  <Criterion id="req-2-4">
    In terminal mode, each entry SHALL display the full Criterion_Ref and a
    truncated body-text summary on a single line.
  </Criterion>
  <Criterion id="req-2-5">
    In JSON mode, THE `refs` command SHALL write an array of objects containing
    `ref`, `doc_id`, `criterion_id`, and `body_text` fields.
  </Criterion>
</AcceptanceCriteria>

## Requirement 3: Context-aware scoping

As a developer working in a specific area of the codebase, I want `refs` to
automatically show me the criteria relevant to my current directory, so that I
see a focused list instead of the entire project.

<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN no prefix argument is provided and the current working directory is
    inside the project root, THE `refs` command SHALL compute a Context_Scope by
    matching the cwd against `TrackedFiles` globs from all documents, then
    expanding the matched set by following `Implements` relationships from each
    matched document, and SHALL display only criteria from the expanded set. It
    SHALL print a hint to stderr indicating the matched scope and that `--all`
    shows all refs.
  </Criterion>
  <Criterion id="req-3-2">
    IF Context_Scope matching produces zero documents, THEN THE `refs` command
    SHALL fall back to showing all Criterion_Refs and SHALL print a hint to
    stderr indicating that no TrackedFiles matched the cwd.
  </Criterion>
  <Criterion id="req-3-3">
    THE `refs` command SHALL accept an `--all` flag that disables Context_Scope
    filtering and lists all Criterion_Refs regardless of cwd.
  </Criterion>
</AcceptanceCriteria>

## Requirement 4: Fragment lookup in verify diagnostics

As a developer who used an incorrect criterion ref, I want the verification
engine to suggest the correct full ref when it detects an unresolved target,
so that I can fix the annotation without manual research.

<AcceptanceCriteria>
  <Criterion id="req-4-1">
    WHEN the verification engine detects an evidence record with an unresolved
    target (empty targets set), AND a Fragment_Lookup on any raw attribute value
    from that record yields one or more matching Criterion_Refs, THEN the
    `missing_verification_evidence` finding message SHALL include a "did you
    mean" suggestion listing the matching full refs.
    <VerifiedBy
      strategy="file-glob"
      paths="crates/supersigil-verify/src/rules/coverage.rs"
    />
  </Criterion>
  <Criterion id="req-4-2">
    WHEN Fragment_Lookup yields zero matches for the unresolved value, THEN the
    finding message SHALL remain unchanged from the current behaviour.
    <VerifiedBy
      strategy="file-glob"
      paths="crates/supersigil-verify/src/rules/coverage.rs"
    />
  </Criterion>
</AcceptanceCriteria>
```
