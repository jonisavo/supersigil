---
supersigil:
  id: lsp/req
  type: requirements
  status: draft
title: "Language Server Protocol Support"
---

## Introduction

A language server for Supersigil Markdown spec files that provides diagnostics,
go-to-definition, autocomplete, and hover documentation inside editors. The
server registers for both `markdown` and `mdx` language IDs, providing
spec-specific intelligence only inside `supersigil-xml` fences and frontmatter
so it does not interfere with general Markdown editing.

Scope: the LSP server itself and necessary prerequisite changes to existing
crates (in-memory parsing API, config field, position conversion). Out of
scope: editor extensions (VS Code, Neovim, etc.) — those are a separate
feature that depends on this one.

## Definitions

- **Diagnostic_Tier**: One of `lint` or `verify`, controlling how much
  analysis runs on save. `lint` is parse + structural rules. `verify`
  adds evidence discovery, tag scanning, and coverage checks.
- **Hybrid_Reindexing**: The strategy of re-parsing a single file on
  `didChange` (for fast local feedback) and rebuilding the full
  `DocumentGraph` on `didSave` (for cross-document analysis).
- **Last_Good_Graph**: The most recent successfully built `DocumentGraph`,
  retained when a graph rebuild fails so that cross-document features
  (go-to-definition, autocomplete, hover on refs) continue working against
  stale-but-valid data rather than losing all cross-document intelligence.

## Requirement 1: Diagnostics

As a spec author, I want to see verification findings directly in my editor,
so that I can fix problems without switching to a terminal.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN a spec file is opened or changed, THE server SHALL publish
    per-file diagnostics derived from parse errors and parser-level
    structural rules (required attributes, unknown components).
  </Criterion>
  <Criterion id="req-1-2">
    WHEN a spec file is saved, THE server SHALL rebuild the
    DocumentGraph and publish cross-document diagnostics (broken refs,
    invalid refs, cycles, missing criteria). THE server SHALL also run
    verification on initial indexing (not just on save) so that
    diagnostics appear immediately when the workspace opens.
  </Criterion>
  <Criterion id="req-1-3">
    WHEN the configured Diagnostic_Tier is `verify`, THE server SHALL run
    the verification pipeline with real evidence from both authored
    `VerifiedBy` components and ecosystem plugins (e.g. Rust `#[verifies]`
    macros), and SHALL include coverage findings in the published
    diagnostics.
  </Criterion>
  <Criterion id="req-1-4">
    THE server SHALL map `ReportSeverity::Error` to `DiagnosticSeverity::ERROR`,
    `Warning` to `WARNING`, and `Info` to `HINT`. Findings with
    `ReportSeverity::Off` SHALL be excluded from diagnostics.
  </Criterion>
  <Criterion id="req-1-6">
    WHEN a `missing_verification_evidence` finding has `example_coverable`
    set to `true`, THE server SHALL downgrade it to
    `DiagnosticSeverity::HINT` regardless of its effective severity, since
    the LSP does not execute examples. THE server SHALL also emit a single
    `INFORMATION`-level diagnostic per affected document summarizing the
    count of example-coverable criteria.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/diagnostics.rs, crates/supersigil-lsp/src/state.rs" />
  </Criterion>
  <Criterion id="req-1-5">
    WHEN publishing diagnostics, THE server SHALL merge per-file and
    cross-document findings into one set per URI, so that a later publish
    does not silently clear earlier diagnostics.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Go-to-Definition

As a spec author, I want to jump from a ref to the referenced document or
criterion, so that I can navigate the spec graph without manual file
searching.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    WHEN the cursor is on a ref string inside a `refs`, `implements`, or
    `depends` attribute and the ref contains `#`, THE server SHALL resolve
    it to the target file and component source position using
    `split_criterion_ref` and the DocumentGraph.
  </Criterion>
  <Criterion id="req-2-2">
    WHEN the ref contains no `#` (document-level ref), THE server SHALL
    resolve it to the top of the target document file.
  </Criterion>
  <Criterion id="req-2-3">
    WHEN the ref target does not exist in the graph, THE server SHALL
    return no locations rather than an error.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Autocomplete

As a spec author, I want completions for document IDs, criterion IDs, and
component names, so that I can write specs without memorizing the full ID
namespace.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN the cursor is inside a ref-accepting attribute (`refs`,
    `implements`, `depends`) and before `#`, THE server SHALL offer
    completions for all document IDs matching the typed prefix.
  </Criterion>
  <Criterion id="req-3-2">
    WHEN the cursor is inside a ref-accepting attribute after `#`, THE
    server SHALL offer completions for referenceable component IDs
    (criteria, tasks) within the specified document.
  </Criterion>
  <Criterion id="req-3-3">
    WHEN the cursor follows `&lt;` inside a `supersigil-xml` fence, THE server SHALL offer
    completions for all known components (built-in and user-defined) with
    attribute signature snippets.
  </Criterion>
  <Criterion id="req-3-4">
    WHEN the cursor is inside a known attribute value (e.g. `strategy`,
    `status`), THE server SHALL offer completions for valid values
    scoped to the enclosing context:
    - For `status` in YAML frontmatter: values from the document type
      definition in the config.
    - For `status` on `Task`: task lifecycle values (`draft`, `ready`,
      `in-progress`, `done`).
    - For `status` on `Alternative`: recognized alternative statuses
      (`rejected`, `deferred`, `superseded`).
    - For `strategy` on `VerifiedBy`: `tag`, `file-glob`.
    - For `status` on `Expected` or other free-form contexts: no
      completions.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Hover

As a spec author, I want to see documentation and context on hover, so that
I can understand components and refs without leaving my current position.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    WHEN the cursor hovers over a component name (e.g. `Criterion`,
    `VerifiedBy`), THE server SHALL display the component definition:
    required/optional attributes, whether referenceable/verifiable, and a
    short description.
  </Criterion>
  <Criterion id="req-4-2">
    WHEN the cursor hovers over a ref string, THE server SHALL display the
    target's context as a clickable Markdown link to the target file:
    - For fragment refs (`doc#criterion`): the document title, component
      kind, fragment ID, and body text. The link opens the file at the
      component's line.
    - For document-level refs: the document title, type, and status. The
      link opens the file at line 1.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Server Lifecycle

As an editor extension, I need the LSP server to initialize correctly, stay
responsive, and handle configuration changes, so that the user experience is
reliable.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    ON `initialize`, THE server SHALL discover `supersigil.toml`, parse all
    matching `.md` files in parallel, build the initial DocumentGraph, and
    register LSP capabilities for completion, hover, definition, text
    document sync, and file watching. THE server SHALL NOT advertise
    `executeCommandProvider` in capabilities (the editor extension
    registers commands itself to avoid conflicts in multi-root workspaces).
  </Criterion>
  <Criterion id="req-5-2">
    IF no `supersigil.toml` is found during initialization, THE server
    SHALL start with minimal capabilities (text document sync only, no
    completion, hover, or definition) and no diagnostics. Reparse and
    verify are skipped until config is present.
  </Criterion>
  <Criterion id="req-5-3">
    THE server SHALL use Hybrid_Reindexing: single-file re-parse on
    `didChange` (from in-memory buffer, not disk), full graph rebuild on
    `didSave`.
  </Criterion>
  <Criterion id="req-5-4">
    WHEN a graph rebuild fails, THE server SHALL retain the Last_Good_Graph
    and publish GraphErrors as diagnostics, rather than losing
    cross-document features.
  </Criterion>
  <Criterion id="req-5-5">
    ON `didClose`, THE server SHALL remove the file from the open buffer
    set and clear diagnostics for that URI.
  </Criterion>
  <Criterion id="req-5-6">
    THE server SHALL report progress during initial indexing via
    `window/workDoneProgress`.
  </Criterion>
  <Criterion id="req-5-7">
    THE Diagnostic_Tier SHALL be configurable via `[lsp].diagnostics` in
    `supersigil.toml` with `verify` as the default, and overridable at
    runtime via `workspace/didChangeConfiguration`.
  </Criterion>
  <Criterion id="req-5-8">
    WHEN a file URI is inside a subdirectory that contains its own
    `supersigil.toml` (a nested supersigil root), THE server SHALL
    ignore `didOpen`, `didChange`, hover, completion, and definition
    requests for that file, returning empty results. This prevents
    cross-root interference when multiple LSP instances serve nested
    projects.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Custom Commands

As an editor extension, I want to trigger verification on demand, so
that users can run the verify pipeline without leaving the editor.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    THE server SHALL handle a `supersigil.verify` command via
    `workspace/executeCommand` that runs verification at the configured
    tier (or an explicit tier passed as argument) and publishes results
    as diagnostics. THE server SHALL NOT advertise this command in
    `executeCommandProvider` capabilities; the editor extension registers
    it and routes to the appropriate server instance.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 7: Markdown Integration

As a spec author editing Markdown files, I want the Supersigil language server
to provide features only inside `supersigil-xml` fences and frontmatter, so
that it does not interfere with general Markdown editing.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-7-1">
    THE server SHALL register for both the `markdown` and `mdx` language IDs.
  </Criterion>
  <Criterion id="req-7-2">
    THE server SHALL only activate when `supersigil.toml` is found in the
    workspace.
  </Criterion>
  <Criterion id="req-7-3">
    THE server SHALL use fence-aware context detection
    (`is_in_supersigil_fence`) to scope completions, hover, and definition
    features to `supersigil-xml` fenced blocks and YAML frontmatter. Outside
    these regions the server SHALL return empty results. Completions SHALL
    use distinct `CompletionItemKind` or label detail to distinguish
    Supersigil items from other completions.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 8: Prerequisite Crate Changes

As the LSP implementer, I need certain APIs in existing crates, so that the
LSP can function correctly.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-8-1">
    `supersigil-parser` SHALL expose a `parse_content(path, content, defs)`
    entry point that accepts in-memory content, with `parse_file` becoming
    a thin wrapper that reads from disk then calls `parse_content`.
  </Criterion>
  <Criterion id="req-8-2">
    `supersigil-core` Config SHALL accept an optional `[lsp]` section
    (via `lsp: Option&lt;LspConfig&gt;` with `#[serde(default)]`) without
    breaking existing CLI consumers that use `deny_unknown_fields`.
  </Criterion>
  <Criterion id="req-8-3">
    THE server SHALL convert between `SourcePosition` (1-based line/column,
    byte offset) and LSP `Position` (0-based line, 0-based UTF-16 character
    offset), advertising `PositionEncodingKind::UTF16`.
  </Criterion>
</AcceptanceCriteria>
```
