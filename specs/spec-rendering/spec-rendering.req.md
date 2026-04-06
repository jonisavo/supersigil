---
supersigil:
  id: spec-rendering/req
  type: requirements
  status: implemented
title: "Spec Rendering"
---

## Introduction

Render supersigil-xml blocks as rich, interactive components in editor
Markdown previews and as a dedicated spec browser on the documentation
site. Primary audience is spec authors who want to see verification
status, evidence details, and navigation links inline while editing.
Secondary audience is documentation site readers who want to browse
specs with full context.

Scope: an LSP data endpoint for component trees with verification
status, a shared presentation kit (HTML generator + CSS + JS), editor
Markdown preview integrations (VS Code and IntelliJ), and a Starlight
spec browser section.

Out of scope: custom editor webview panels, Neovim/Zed preview
support, real-time test execution from previews.

## Definitions

- **Component tree**: The parsed hierarchy of supersigil-xml
  components (Criterion, Decision, Task, etc.) from a spec
  document, enriched with verification status.
- **Presentation kit**: A shared TypeScript package that converts
  component tree JSON into styled HTML fragments.
- **Spec browser**: A dedicated section of the Starlight site that
  renders spec documents with verification data and graph
  relationships.
- **Evidence entry**: A record linking a verifiable criterion to its
  test evidence, including provenance (how the evidence was
  discovered).
- **Fence**: A single `supersigil-xml` fenced code block within a
  Markdown document. A document may contain multiple fences, each
  containing a subset of the document's components.
- **Link resolver**: A host-provided callback that converts a
  navigation target (file path + line, document ID, criterion ref)
  into a host-appropriate URI (VS Code `command:` URI, IntelliJ
  `JBCefJSQuery` call, Starlight page link).

## Requirement 1: Component Data Endpoint

As an editor extension, I need the LSP to provide per-document
component trees with verification status, so that the Markdown
preview can render rich spec blocks without shelling out to the CLI.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE LSP server SHALL handle a custom `supersigil/documentComponents`
    request that accepts a document URI and returns a JSON payload
    containing the document ID, a list of fences ordered by source
    position (each with its source byte range and its subset of the
    component tree), and outgoing graph edges. Editor integrations
    SHALL use a deterministic correlation strategy (byte range matching
    or document-order matching) to map each preview fence to its
    corresponding component subset.
  </Criterion>
  <Criterion id="req-1-2">
    Each component in the response SHALL include its kind, optional ID,
    attributes, body text, child components, source range (for
    navigation), and verification status (for verifiable components).
  </Criterion>
  <Criterion id="req-1-3">
    Verification status SHALL include the state (verified, unverified,
    partial, failing) and a list of evidence entries. Each evidence
    entry SHALL include the test name, test file, test kind (unit,
    async, property, snapshot, unknown), evidence kind (tag, file-glob,
    rust-attribute, js-verifies), source line, and provenance chain.
  </Criterion>
  <Criterion id="req-1-4">
    WHEN the requested document has not been indexed or has parse
    errors, THE server SHALL return the most recent successful parse
    result, or an empty fence list if no successful parse exists. THE
    response SHALL include a `stale` boolean that is true when the
    returned data does not reflect the current document content. WHEN
    data is stale, editor integrations SHALL render the components
    with a visual stale indicator (e.g. dimmed opacity or a "stale"
    badge) and MAY fall back to document-order correlation if byte
    range matching is unreliable due to shifted offsets.
  </Criterion>
  <Criterion id="req-1-5">
    THE CLI SHALL provide a `supersigil render --format json` command
    that outputs the same component-tree-with-verification JSON for all
    documents in the project, for use by the Starlight build pipeline.
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Shared Presentation Kit

As a maintainer of multiple editor integrations and the documentation
site, I need a single rendering implementation so that spec blocks
look and behave consistently across all surfaces.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE presentation kit SHALL provide a pure function that accepts a
    component tree JSON array, graph edges, and a host-provided link
    resolver, and returns an HTML string rendering all components with
    their verification status. THE link resolver SHALL translate
    navigation targets (evidence source locations, document refs,
    criterion refs) into host-appropriate URIs.
    <VerifiedBy strategy="file-glob" paths="packages/preview/__tests__/render.test.ts" />
  </Criterion>
  <Criterion id="req-2-2">
    THE presentation kit SHALL render Criterion components with a
    visual badge indicating verification state (verified, unverified,
    partial, failing) and an expandable evidence list showing test
    details and provenance.
    <VerifiedBy strategy="file-glob" paths="packages/preview/__tests__/render.test.ts" />
  </Criterion>
  <Criterion id="req-2-3">
    THE presentation kit SHALL render Decision components with their
    rationale and alternatives as structured, readable blocks.
    <VerifiedBy strategy="file-glob" paths="packages/preview/__tests__/render.test.ts" />
  </Criterion>
  <Criterion id="req-2-4">
    THE presentation kit SHALL render unsupported or otherwise
    uncustomized component kinds using a generic wrapper that shows
    the component kind, optional ID, body text, and child components.
    <VerifiedBy strategy="file-glob" paths="packages/preview/__tests__/render.test.ts" />
  </Criterion>
  <Criterion id="req-2-5">
    THE presentation kit SHALL use CSS custom properties for theming
    so that colors adapt to the host environment (VS Code theme,
    IntelliJ theme, Starlight theme).
    <VerifiedBy strategy="file-glob" paths="packages/preview/styles/supersigil-preview.css" />
  </Criterion>
  <Criterion id="req-2-6">
    THE presentation kit SHALL provide client-side JavaScript for
    interactive behaviors: collapsible evidence lists, tooltips on
    badges, and MutationObserver-based re-initialization when the DOM
    updates.
    <VerifiedBy strategy="file-glob" paths="packages/preview/scripts/supersigil-preview.js" />
  </Criterion>
  <Criterion id="req-2-7">
    THE presentation kit SHALL have zero framework dependencies
    (vanilla JS + CSS) and be distributable as a single CSS file and
    single JS file.
    <VerifiedBy strategy="file-glob" paths="packages/preview/package.json" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: VS Code Markdown Preview

As a spec author using VS Code, I want supersigil-xml fenced code
blocks to render as rich components in the built-in Markdown preview,
so that I can see verification status and spec structure without
leaving the editor.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    THE VS Code extension SHALL contribute a markdown-it plugin via
    `markdown.markdownItPlugins` that overrides the fence renderer for
    `supersigil-xml` blocks. THE plugin SHALL use the presentation kit
    to produce HTML from cached component data.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/extension.ts" />
  </Criterion>
  <Criterion id="req-3-2">
    THE extension SHALL maintain a per-document cache of
    `documentComponents` responses. WHEN the `supersigil/documentsChanged`
    notification fires, THE extension SHALL invalidate all cache entries
    and re-fetch components for any document whose preview is currently
    open, then trigger `markdown.preview.refresh`.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/previewCache.ts" />
  </Criterion>
  <Criterion id="req-3-3">
    WHEN the cache has no data for a document (initial load or not
    yet indexed), THE preview SHALL show a styled loading placeholder.
    WHEN data arrives, THE preview SHALL automatically refresh. WHEN
    the cached data is stale, THE preview SHALL render the last-known
    components with a visual stale indicator (e.g. dimmed opacity or
    a "stale" badge) rather than a blank placeholder.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/previewCache.ts" />
  </Criterion>
  <Criterion id="req-3-4">
    THE extension SHALL inject the presentation kit CSS via
    `markdown.previewStyles` and the JS via `markdown.previewScripts`.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/package.json" />
  </Criterion>
  <Criterion id="req-3-5">
    THE VS Code extension SHALL provide a link resolver that produces
    `command:vscode.open` URIs for evidence source locations and
    document references, and pass it to the presentation kit render
    function.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/src/previewCache.ts" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: IntelliJ Markdown Preview

As a spec author using IntelliJ, I want supersigil-xml fenced code
blocks to render as rich components in the built-in Markdown preview,
so that I get the same spec rendering experience as VS Code.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    THE IntelliJ plugin SHALL register a browser preview extension
    that injects the shared presentation kit CSS and JS into the
    JCEF preview. THE injected JS SHALL find
    `language-supersigil-xml` code elements in the preview DOM and
    replace them with rendered components using component data
    fetched from the LSP via `workspace/executeCommand`.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/kotlin/org/supersigil/intellij/SupersigilPreviewExtensionProvider.kt" />
  </Criterion>
  <Criterion id="req-4-2">
    THE plugin SHALL inject the presentation kit CSS and JS into the
    JCEF preview via a `browserPreviewExtensionProvider`.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/META-INF/plugin.xml" />
  </Criterion>
  <Criterion id="req-4-3">
    THE IntelliJ plugin SHALL provide a link resolver that produces
    `JBCefJSQuery` callback URIs for evidence source locations and
    document references, and pass it to the presentation kit render
    function. THE injected JS SHALL register `JBCefJSQuery` handlers
    that open the corresponding source file at the target line in the
    IDE editor.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/src/main/resources/supersigil-preview/supersigil-bridge.js" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Spec Explorer Integration

As a documentation site reader, I want to browse rendered spec
documents with verification data alongside the component graph.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    THE graph explorer SHALL load render data from the build-time
    `supersigil render --format json` output and display rendered
    spec components in a detail panel when a document node is
    selected.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/detail-panel.js" />
  </Criterion>
  <Criterion id="req-5-2">
    WHEN no document is selected, THE detail panel SHALL display a
    document index grouped by project and prefix, with overall
    coverage statistics.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/detail-panel.js" />
  </Criterion>
  <Criterion id="req-5-3">
    WHEN a document is selected, THE detail panel SHALL render the
    document's components using the shared presentation kit HTML,
    including verification badges, evidence details, and links to
    related specs. Document links SHALL navigate within the graph
    explorer.
    <VerifiedBy strategy="file-glob" paths="website/src/components/explore/detail-panel.js" />
  </Criterion>
  <Criterion id="req-5-4">
    THE website prebuild script SHALL run
    `supersigil render --format json` alongside the existing
    `supersigil graph --format json` to generate static data for the
    spec browser.
    <VerifiedBy strategy="file-glob" paths="website/package.json" />
  </Criterion>
</AcceptanceCriteria>
```
