---
supersigil:
  id: lsp-code-actions/req
  type: requirements
  status: implemented
title: "LSP Code Actions / Quick Fixes"
---

## Introduction

Attach actionable fixes to LSP diagnostics so that spec authors can resolve
warnings and errors with one click instead of manually editing files. The
server responds to `textDocument/codeAction` requests by matching diagnostics
to a set of providers, each responsible for one category of fix.

Scope: the code action handler, diagnostic enrichment, provider trait and
initial provider set, and the "create document" interactive flow. Out of
scope: editor extension UI beyond what the LSP protocol provides natively
(e.g. custom webview-based refactoring dialogs).

## Definitions

- **DiagnosticData**: A strongly-typed struct attached to each LSP diagnostic
  via the `data` field. Contains the source (parse, graph, or verify rule),
  the originating document ID, and an `ActionContext` enum carrying
  fix-specific metadata.
- **CodeActionProvider**: A trait implemented by each category of fix.
  Stateless — receives all context through a `CodeActionContext` bundle
  containing the document graph, config, file parses, and project root.
- **Ambiguous_Project**: In multi-project mode, a situation where the
  target spec directory for a new document cannot be deterministically
  derived from the referring document's project membership.

## Requirement 1: Diagnostic Enrichment

As a code action provider, I need structured metadata on each diagnostic,
so that I can dispatch on rule type without parsing message strings.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN converting a verification Finding to an LSP Diagnostic, THE server
    SHALL attach a serialized DiagnosticData struct to the diagnostic's
    `data` field containing the RuleName enum variant, the originating
    document ID, and an ActionContext enum with fix-specific metadata.
  </Criterion>
  <Criterion id="req-1-2">
    WHEN converting a parse error or warning to an LSP Diagnostic, THE
    server SHALL attach a DiagnosticData with a ParseDiagnosticKind enum
    variant identifying the error category (e.g. MissingRequiredAttribute,
    UnknownComponent).
  </Criterion>
  <Criterion id="req-1-3">
    WHEN converting a graph error to an LSP Diagnostic, THE server SHALL
    attach a DiagnosticData with a GraphDiagnosticKind enum variant
    identifying the error category (e.g. DuplicateDocumentId,
    DuplicateComponentId, BrokenRef).
  </Criterion>
  <Criterion id="req-1-4">
    THE DiagnosticData struct SHALL serialize to JSON for the LSP wire
    format and deserialize back without loss in the code action handler.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Code Action Handler

As a spec author, I want the editor to offer quick fixes on diagnostics,
so that I can resolve issues without manual editing.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE server SHALL advertise `codeActionProvider` in its capabilities
    with `codeActionKinds` set to `["quickfix"]`.
  </Criterion>
  <Criterion id="req-2-2">
    WHEN a `textDocument/codeAction` request arrives, THE server SHALL
    deserialize the `data` field of each diagnostic into DiagnosticData,
    iterate registered providers, and return all matching CodeActions.
  </Criterion>
  <Criterion id="req-2-3">
    WHEN a diagnostic has no `data` field or deserialization fails, THE
    server SHALL skip that diagnostic without error rather than failing
    the entire request.
  </Criterion>
  <Criterion id="req-2-4">
    Each returned CodeAction SHALL have kind `quickfix` and SHALL include
    the originating diagnostic in its `diagnostics` field so the editor
    can associate the fix with the warning/error marker.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Provider Trait

As the LSP maintainer, I want a clean extension point for adding new
quick fixes, so that each fix category is isolated and testable.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    THE server SHALL define a CodeActionProvider trait with a `handles`
    method that inspects DiagnosticData to determine applicability, and
    an `actions` method that returns zero or more CodeActions.
  </Criterion>
  <Criterion id="req-3-2">
    Providers SHALL be stateless. All context (document graph, config,
    file parses, project root, file URI, file content) SHALL be passed
    via a CodeActionContext struct.
  </Criterion>
  <Criterion id="req-3-3">
    Providers SHALL be registered once at server initialization in an
    ordered collection. The handler SHALL iterate all providers for each
    diagnostic.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Initial Provider Set

As a spec author, I want quick fixes for the most common diagnostics,
so that routine issues are one-click resolvable.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    A BrokenRefProvider SHALL handle broken ref diagnostics and offer:
    (a) "Remove broken ref" — edits the attribute to remove the broken
    ref from the comma-separated list, and (b) "Create document" — when
    the target document ID encodes a recognized type and the target path
    is deterministically resolvable or the user selects a project.
  </Criterion>
  <Criterion id="req-4-2">
    A MissingAttributeProvider SHALL handle missing-required-attribute
    parse errors and offer to insert the attribute with a placeholder
    value at the component's opening tag.
  </Criterion>
  <Criterion id="req-4-3">
    A DuplicateIdProvider SHALL handle duplicate document ID and duplicate
    component ID diagnostics and offer to rename the ID to a unique
    variant by appending a numeric suffix.
  </Criterion>
  <Criterion id="req-4-4">
    An IncompleteDecisionProvider SHALL handle incomplete-decision
    verification findings and offer to insert a stub Rationale or
    Alternative component inside the Decision.
  </Criterion>
  <Criterion id="req-4-5">
    A MissingComponentProvider SHALL handle missing-required-component
    verification findings and offer to insert a skeleton of the required
    component at the appropriate location.
  </Criterion>
  <Criterion id="req-4-6">
    An OrphanDecisionProvider SHALL handle orphan-decision verification
    findings and offer to add a References component with a refs
    attribute pointing to the parent document.
  </Criterion>
  <Criterion id="req-4-7">
    An InvalidPlacementProvider SHALL handle invalid-rationale-placement,
    invalid-alternative-placement, and invalid-expected-placement findings
    and offer to move the component to the correct parent.
  </Criterion>
  <Criterion id="req-4-8">
    A SequentialIdProvider SHALL handle sequential-id-gap and
    sequential-id-order findings and offer to renumber component IDs to
    restore sequential order.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Create Document Flow

As a spec author working in a multi-project workspace, I want the
"create document" action to ask me which project to place the file in
when the target is ambiguous, so that files land in the right location.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    WHEN the target document path is unambiguous (single project mode,
    or the referring document belongs to exactly one project), THE
    BrokenRefProvider SHALL return a CodeAction with a WorkspaceEdit
    containing a CreateFile resource operation and TextEdits populating
    the scaffolded content.
  </Criterion>
  <Criterion id="req-5-2">
    WHEN the target document path is ambiguous (Ambiguous_Project), THE
    BrokenRefProvider SHALL return a CodeAction with a Command
    ("supersigil.createDocument") instead of a direct WorkspaceEdit.
  </Criterion>
  <Criterion id="req-5-3">
    WHEN executing the "supersigil.createDocument" command, THE server
    SHALL send a `window/showMessageRequest` to the client with project
    names as action buttons, resolve the spec directory from the chosen
    project's glob prefix, scaffold the file, and apply the edit via
    `workspace/applyEdit`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/state.rs" />
  </Criterion>
  <Criterion id="req-5-4">
    THE scaffolded document content SHALL reuse the same template logic
    as the `supersigil new` CLI command, extracted into a shared function
    accessible to both CLI and LSP.
  </Criterion>
  <Criterion id="req-5-5">
    WHEN the user dismisses the project selection dialog without choosing,
    THE server SHALL take no action and not produce an error.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-lsp/src/state.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 6: Testing

As the LSP maintainer, I want snapshot-based tests for every provider,
so that code action behavior is captured and reviewable.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-6-1">
    Each provider SHALL have snapshot tests using insta that capture the
    full set of returned CodeActions (titles, kinds, and edit operations)
    in a human-readable text format via a shared format_actions helper.
  </Criterion>
  <Criterion id="req-6-2">
    Integration tests SHALL apply returned WorkspaceEdits to source
    files, re-parse, and verify that the originating diagnostic is
    resolved.
  </Criterion>
</AcceptanceCriteria>
```
