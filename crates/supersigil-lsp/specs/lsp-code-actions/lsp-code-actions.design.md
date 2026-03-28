---
supersigil:
  id: lsp-code-actions/design
  type: design
  status: approved
title: "LSP Code Actions / Quick Fixes — Design"
---

```supersigil-xml
<Implements refs="lsp-code-actions/req" />
```

```supersigil-xml
<DependsOn refs="lsp-server/req" />
```

```supersigil-xml
<TrackedFiles paths="crates/supersigil-lsp/src/code_actions.rs, crates/supersigil-lsp/src/code_actions/**/*.rs, crates/supersigil-lsp/src/diagnostics.rs" />
```

## Overview

Add a `textDocument/codeAction` handler to the LSP server that matches
diagnostics to a registry of providers, each generating quick-fix
`CodeAction`s. Diagnostics are enriched with a typed `DiagnosticData`
struct in the `data` field so providers dispatch on enum variants
rather than fragile message strings.

## Architecture

All new code lives in `crates/supersigil-lsp/`. The feature touches
three layers:

1. **Diagnostic enrichment** (`diagnostics.rs`): Existing conversion
   functions gain a `DiagnosticData` payload attached to every
   diagnostic.
2. **Provider registry** (`code_actions.rs`): A trait + collection of
   providers, iterated by the handler.
3. **Handler** (`state.rs`): The `textDocument/codeAction` endpoint
   that deserializes data, calls providers, and returns actions.

```
textDocument/codeAction request
        │
        ▼
  ┌─────────────┐     ┌──────────────────┐
  │ Deserialize  │────▶│ DiagnosticData    │
  │ data field   │     │ (source, context) │
  └─────────────┘     └──────────────────┘
        │
        ▼
  ┌─────────────┐     ┌──────────────────┐
  │ Iterate      │────▶│ Provider.handles()│
  │ providers    │     │ Provider.actions()│
  └─────────────┘     └──────────────────┘
        │
        ▼
  ┌─────────────┐
  │ Return       │
  │ CodeActions  │
  └─────────────┘
```

For the "create document" flow when the target project is ambiguous:

```
CodeAction with Command("supersigil.createDocument")
        │
        ▼
  execute_command handler
        │
        ▼
  window/showMessageRequest ──▶ user picks project
        │
        ▼
  resolve spec_dir from project glob prefix
        │
        ▼
  scaffold content (shared with CLI)
        │
        ▼
  workspace/applyEdit (CreateFile + TextEdits)
```

## Key Types

All types live in `crates/supersigil-lsp/src/code_actions.rs` unless
otherwise noted.

### DiagnosticData (in `diagnostics.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticData {
    pub source: DiagnosticSource,
    pub doc_id: Option<String>,
    pub context: ActionContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiagnosticSource {
    Parse(ParseDiagnosticKind),
    Graph(GraphDiagnosticKind),
    Verify(RuleName),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParseDiagnosticKind {
    MissingRequiredAttribute,
    UnknownComponent,
    XmlSyntaxError,
    UnclosedFrontmatter,
    DuplicateCodeRef,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphDiagnosticKind {
    DuplicateDocumentId,
    DuplicateComponentId,
    BrokenRef,
    DependencyCycle,
    InvalidComponent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionContext {
    None,
    BrokenRef {
        target_ref: String,
    },
    MissingAttribute {
        component: String,
        attribute: String,
    },
    DuplicateId {
        id: String,
        other_path: String,
    },
    IncompleteDecision {
        decision_id: String,
        missing: Vec<String>,
    },
    MissingComponent {
        component: String,
        parent_id: String,
    },
    OrphanDecision {
        decision_id: String,
    },
    InvalidPlacement {
        component: String,
        expected_parent: String,
    },
    SequentialIdGap {
        component_type: String,
        ids: Vec<String>,
    },
}
```

### Provider trait and context

```rust
pub trait CodeActionProvider: Send + Sync {
    fn handles(&self, data: &DiagnosticData) -> bool;

    fn actions(
        &self,
        diagnostic: &Diagnostic,
        data: &DiagnosticData,
        ctx: &ActionRequestContext,
    ) -> Vec<CodeAction>;
}

pub struct ActionRequestContext<'a> {
    pub graph: &'a DocumentGraph,
    pub config: &'a Config,
    pub file_parses: &'a HashMap<PathBuf, SpecDocument>,
    pub project_root: &'a Path,
    pub file_uri: &'a Url,
    pub file_content: &'a str,
}
```

### Provider implementations

Eight provider structs, each a unit struct implementing
`CodeActionProvider`:

| Struct | Dispatches on |
|---|---|
| `BrokenRefProvider` | `Graph(BrokenRef)`, `Verify(BrokenRef)` |
| `MissingAttributeProvider` | `Parse(MissingRequiredAttribute)` |
| `DuplicateIdProvider` | `Graph(DuplicateDocumentId)`, `Graph(DuplicateComponentId)` |
| `IncompleteDecisionProvider` | `Verify(IncompleteDecision)` |
| `MissingComponentProvider` | `Verify(MissingRequiredComponent)` |
| `OrphanDecisionProvider` | `Verify(OrphanDecision)` |
| `InvalidPlacementProvider` | `Verify(InvalidRationalePlacement)`, `Verify(InvalidAlternativePlacement)`, `Verify(InvalidExpectedPlacement)` |
| `SequentialIdProvider` | `Verify(SequentialIdGap)`, `Verify(SequentialIdOrder)` |

## Shared Scaffolding

The `supersigil new` CLI command's template logic (frontmatter
generation, default content per document type) will be extracted into
a function in `supersigil-core` or `supersigil-cli` (whichever avoids
a circular dependency) that both the CLI command and the LSP's
`BrokenRefProvider` / `supersigil.createDocument` handler can call.

The spec dir resolution logic (`glob_prefix` + project lookup) already
lives in `supersigil-core::graph::index` and is reusable.

## Error Handling

- **Deserialization failure**: If a diagnostic's `data` field is missing
  or cannot be deserialized into `DiagnosticData`, the handler skips
  it silently. No error response to the client.
- **Provider failure**: If a provider panics (shouldn't happen, but
  defensive), `catch_unwind` or equivalent prevents one provider from
  breaking the entire response. Log the error, continue to next
  provider.
- **showMessageRequest dismissal**: If the user cancels the project
  selection dialog, `show_message_request` returns `None`. The command
  handler returns `Ok(None)` — no edit applied, no error.
- **CreateFile conflict**: If the target file already exists at the
  resolved path, the `CreateFile` operation uses `overwrite: false` and
  `ignoreIfExists: false`, causing the client to report the conflict
  rather than silently overwriting.

## Testing Strategy

**Unit tests (per provider)**:
- Construct `DiagnosticData` + minimal `ActionRequestContext` with
  in-memory graph and config.
- Call `provider.actions()` and snapshot the result with `insta`.
- A shared `format_actions()` helper renders `Vec<CodeAction>` as
  readable text: titles, kinds, file paths, and edit content.

**Integration tests**:
- Set up a temp directory with `supersigil.toml` + spec files
  containing known issues.
- Parse, build graph, run verify to produce real `Finding`s.
- Convert to diagnostics with `DiagnosticData`.
- Feed into the code action handler.
- Apply returned `WorkspaceEdit`s to the files.
- Re-parse and re-verify to confirm the diagnostic is resolved.

**Not tested at this level**: LSP transport, `showMessageRequest`
interaction, VS Code rendering.

## Decisions

```supersigil-xml
<Decision id="typed-diagnostic-data">
  Use a strongly-typed DiagnosticData struct with enum variants for
  dispatch rather than unstructured JSON or message string matching.

  <References refs="lsp-code-actions/req#req-1-1, lsp-code-actions/req#req-1-2, lsp-code-actions/req#req-1-3" />

  <Rationale>
  Enum-based dispatch is exhaustive (the compiler catches missing
  arms), refactor-safe (renames propagate), and self-documenting.
  Message string matching is fragile across i18n, rewording, and
  multi-version clients.
  </Rationale>

  <Alternative id="json-value-data" status="rejected">
  Use serde_json::Value for the data field and pattern-match on
  string keys at runtime. Lower upfront cost but no compile-time
  safety and easy to drift out of sync.
  </Alternative>

  <Alternative id="message-matching" status="rejected">
  Match on diagnostic message text or code strings. Zero upfront cost
  but extremely fragile — any message rewording breaks all providers.
  </Alternative>
</Decision>
```

```supersigil-xml
<Decision id="provider-trait-registry">
  Use a trait-based provider registry rather than a monolithic match
  or a metadata-driven code generation approach.

  <References refs="lsp-code-actions/req#req-3-1, lsp-code-actions/req#req-3-2, lsp-code-actions/req#req-3-3" />

  <Rationale>
  Each provider is a small, focused unit with its own test file.
  Adding a new provider is additive (implement trait, register) with
  no changes to the handler. The trait boundary enforces statelessness
  and makes the context dependencies explicit.
  </Rationale>

  <Alternative id="monolithic-match" status="rejected">
  One large match block in the handler. Works initially but becomes
  unwieldy past 5-6 action types and is harder to test in isolation.
  </Alternative>
</Decision>
```

```supersigil-xml
<Decision id="create-doc-interactive">
  Use window/showMessageRequest for project selection when the target
  project is ambiguous, keeping the flow entirely server-side.

  <References refs="lsp-code-actions/req#req-5-2, lsp-code-actions/req#req-5-3" />

  <Rationale>
  showMessageRequest is part of the LSP base protocol and supported
  by all conforming clients. No VS Code extension changes needed.
  The number of projects in practice is small enough for button-style
  selection. A richer Command-based flow with showQuickPick can be
  added later if needed.
  </Rationale>

  <Alternative id="command-plus-extension" status="deferred">
  Return a Command and handle it in the VS Code extension with
  showQuickPick/showInputBox. Richer UX but requires extension
  changes and breaks editor-agnosticism. Viable as a future
  enhancement if showMessageRequest proves inadequate.
  </Alternative>
</Decision>
```
