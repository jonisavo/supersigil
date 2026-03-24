---
supersigil:
  id: document-symbols/req
  type: requirements
  status: implemented
title: "LSP Document Symbols"
---

## Introduction

Expose the component structure of Supersigil spec files via
`textDocument/documentSymbol` so that editors can render an outline panel,
breadcrumbs, and "Go to Symbol in Document" (`Ctrl+Shift+O`) for spec
navigation.

Scope: the `documentSymbol` request handler in the LSP server and the
corresponding capability advertisement. Out of scope: workspace-wide
symbol search (`workspace/symbol`), which is a separate feature.

## Definitions

- **Document_Symbol**: An LSP `DocumentSymbol` representing a named,
  hierarchical element in a file. Has `name`, `kind`, `range`,
  `selectionRange`, `detail`, and optional `children`.

## Requirement 1: Symbol Hierarchy

As a spec author, I want the outline panel to show the component hierarchy
of my spec file, so that I can navigate to specific criteria, tasks,
decisions, and other components without scrolling.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN `textDocument/documentSymbol` is requested, THE server SHALL
    return a hierarchical `DocumentSymbol[]` response (not flat
    `SymbolInformation[]`) representing the components in the file.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/document_symbols.rs, crates/supersigil-lsp/src/state.rs" />
  </Criterion>
  <Criterion id="req-1-2">
    Each top-level `ExtractedComponent` in the parsed `SpecDocument` SHALL
    produce a Document_Symbol. Nested components (children) SHALL appear
    as `children` of their parent symbol, preserving the document tree.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/document_symbols.rs" />
  </Criterion>
  <Criterion id="req-1-3">
    The symbol `name` SHALL be the component's `id` attribute if present,
    otherwise the component name (e.g. `AcceptanceCriteria`, `Rationale`).
    The `detail` field SHALL show the component name when `id` is used as
    the name, so both the ID and the type are visible.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/document_symbols.rs" />
  </Criterion>
  <Criterion id="req-1-4">
    The symbol `kind` SHALL be mapped as follows:
    - `Criterion` → `SymbolKind::PROPERTY`
    - `Task` → `SymbolKind::EVENT`
    - `Decision` → `SymbolKind::INTERFACE`
    - `Alternative` → `SymbolKind::ENUM_MEMBER`
    - `VerifiedBy`, `TrackedFiles`, `References`, `Implements`,
      `DependsOn` → `SymbolKind::STRUCT`
    - All other components → `SymbolKind::OBJECT`
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/document_symbols.rs" />
  </Criterion>
  <Criterion id="req-1-5">
    The symbol `range` SHALL span the full component (from opening tag to
    closing tag or self-closing tag end). The `selectionRange` SHALL be a
    zero-width range at the component's start position (the opening tag
    location).
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/src/types.rs, crates/supersigil-parser/src/xml_parser.rs, crates/supersigil-parser/src/xml_extract.rs, crates/supersigil-lsp/src/document_symbols.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Capability Advertisement

As an editor extension, I need the server to advertise document symbol
support, so that the editor enables the outline panel and breadcrumbs.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    ON `initialize`, THE server SHALL include
    `documentSymbolProvider: true` in the returned `ServerCapabilities`
    when `supersigil.toml` is present.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/state.rs" />
  </Criterion>
  <Criterion id="req-2-2">
    WHEN no `supersigil.toml` is found, THE server SHALL NOT advertise
    document symbol support (consistent with the existing minimal
    capabilities behavior).
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/state.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Empty and Error Cases

As a spec author, I want symbol requests to be safe and predictable even
for unusual files.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN the file has no components (e.g. frontmatter-only or empty body),
    THE server SHALL return an empty symbol list.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/document_symbols.rs" />
  </Criterion>
  <Criterion id="req-3-2">
    WHEN the file cannot be parsed (no `SpecDocument` in the parse cache),
    THE server SHALL return no symbols (`None`) rather than an error
    response. WHEN the file parses successfully but some components have
    issues, THE server SHALL return symbols for all parsed components.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/state.rs, crates/supersigil-parser/src/lib.rs" />
  </Criterion>
</AcceptanceCriteria>
```
