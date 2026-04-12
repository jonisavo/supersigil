---
supersigil:
  id: spec-rendering/design
  type: design
  status: approved
title: "Spec Rendering"
---

```supersigil-xml
<Implements refs="spec-rendering/req" />
```

## Overview

Three layers: a Rust data endpoint (LSP + CLI) that serves
fence-grouped component trees with verification status, a shared
TypeScript presentation kit that converts JSON to themed HTML, and
thin per-host adapters (VS Code markdown-it plugin, IntelliJ browser
extension, Starlight dynamic pages).

```
Rust LSP/CLI
  supersigil/documentComponents (per-document, fence-grouped)
  supersigil export --format json (all documents, build-time)
        |
@supersigil/preview  (packages/preview/)
  renderComponentTree(fences, edges, linkResolver) -> HTML
  supersigil-preview.css  (CSS custom properties)
  supersigil-preview.js   (collapsibles, tooltips, observers)
        |
  +-----------+-----------+-------------+
  |           |           |             |
VS Code    IntelliJ    Starlight    (future)
md-it      browser     Astro
plugin     extension   pages
```

## LSP Side

### Custom Request: `supersigil/documentComponents`

New module `crates/supersigil-lsp/src/document_components.rs`
alongside the existing `document_list.rs`.

```rust
pub struct DocumentComponentsRequest;

impl Request for DocumentComponentsRequest {
    type Params = DocumentComponentsParams;
    type Result = DocumentComponentsResult;
    const METHOD: &'static str = "supersigil/documentComponents";
}

#[derive(Serialize, Deserialize)]
pub struct DocumentComponentsParams {
    pub uri: String,
}

#[derive(Serialize, Deserialize)]
pub struct DocumentComponentsResult {
    pub document_id: String,
    pub stale: bool,
    pub fences: Vec<FenceData>,
    pub edges: Vec<EdgeData>,
}

#[derive(Serialize, Deserialize)]
pub struct FenceData {
    pub byte_range: [usize; 2],
    pub components: Vec<RenderedComponent>,
}

#[derive(Serialize, Deserialize)]
pub struct RenderedComponent {
    pub kind: String,
    pub id: Option<String>,
    pub attributes: HashMap<String, String>,
    pub body_text: Option<String>,
    pub children: Vec<RenderedComponent>,
    pub verification: Option<VerificationStatus>,
    pub source_range: SourceRange,
}

#[derive(Serialize, Deserialize)]
pub struct VerificationStatus {
    pub state: String,  // "verified" | "unverified" | "partial" | "failing"
    pub evidence: Vec<EvidenceEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct EvidenceEntry {
    pub test_name: String,
    pub test_file: String,
    pub test_kind: String,
    pub evidence_kind: String,
    pub source_line: usize,
    pub provenance: Vec<ProvenanceEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ProvenanceEntry {
    #[serde(rename = "verified-by-tag")]
    VerifiedByTag { tag: String },
    #[serde(rename = "verified-by-file-glob")]
    VerifiedByFileGlob { paths: Vec<String> },
    #[serde(rename = "rust-attribute")]
    RustAttribute { file: String, line: usize },
    #[serde(rename = "js-verifies")]
    JsVerifies { file: String, line: usize },
}

#[derive(Serialize, Deserialize)]
pub struct EdgeData {
    pub from: String,
    pub to: String,
    pub kind: String,  // "Implements" | "References" | "DependsOn"
}

#[derive(Serialize, Deserialize)]
pub struct SourceRange {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}
```

### Request Handler

Registered in `SupersigilLsp::new_router()` alongside the existing
`DocumentListRequest` handler:

```rust
router.request::<DocumentComponentsRequest, _>(
    Self::handle_document_components,
);
```

A `workspace/executeCommand` mirror is also registered as
`supersigil.documentComponents`, following the existing pattern for
`supersigil.documentList`. This is required because IntelliJ's
built-in LSP client does not support custom JSON-RPC requests and
must use `workspace/executeCommand` instead. The execute-command
handler delegates to the same internal logic.

The handler:

1. Resolves the URI to a document ID via the existing path-to-id
   index.
2. Reads the parsed `SpecDocument` from the graph. If the document
   has parse errors, falls back to the last successful parse and sets
   `stale: true`.
3. Groups `ExtractedComponent` entries by their originating
   `supersigil-xml` fence using byte ranges from the parser's fence
   extraction step.
4. If verification results exist in memory (from the last
   `supersigil.verify` run), enriches each verifiable component with
   its `VerificationStatus` by looking up the criterion ref in the
   `ArtifactGraph.evidence_by_target` index.
5. Collects outgoing edges from `DocumentGraph.edges()` for the
   document.

### Fence Grouping

The parser already tracks fence byte ranges during the markdown fence
extraction step. Each `ExtractedComponent` carries `offset` and
`end_offset`. To group components into fences, the handler iterates
the document's fence ranges and assigns each top-level component to
the fence whose byte range contains the component's offset.

### CLI: `supersigil export`

New subcommand in `crates/supersigil-cli/src/commands/export.rs`.
Reuses the same `DocumentComponentsResult` serialization. Iterates
all documents in the graph, builds the same fence-grouped component
tree with verification data, and outputs a JSON array of
`DocumentComponentsResult` objects.

Accepts `--format json` (only format initially). Added to the
website's `prebuild` script:

```json
"prebuild": "supersigil graph --format json > public/explore/graph.json && supersigil export --format json > public/specs/render-data.json"
```

## Shared Presentation Kit

### Package: `packages/preview/`

```
packages/preview/
  src/
    render.ts       # renderComponentTree() pure function
    types.ts        # TypeScript interfaces matching Rust types
  styles/
    supersigil-preview.css
  scripts/
    supersigil-preview.js
  package.json
  tsconfig.json
  esbuild.mjs       # builds dist/supersigil-preview.{css,js}
```

### Render Function

```typescript
interface LinkResolver {
  evidenceLink(file: string, line: number): string;
  documentLink(docId: string): string;
  criterionLink(docId: string, criterionId: string): string;
}

function renderComponentTree(
  fences: FenceData[],
  edges: EdgeData[],
  linkResolver: LinkResolver,
): string;
```

Each fence produces a `<div class="supersigil-block">` container.
Components are rendered as nested elements within it:

- **Criterion**: `.supersigil-criterion` with a
  `.supersigil-badge.supersigil-badge--{state}` and a
  `.supersigil-evidence` collapsible section.
- **Decision**: `.supersigil-decision` with nested
  `.supersigil-rationale` and `.supersigil-alternative` blocks.
- **Unsupported or uncustomized kinds**: `.supersigil-component`
  generic wrapper showing kind, optional ID, body text, and nested
  children.
- **AcceptanceCriteria**: `.supersigil-acceptance-criteria` wrapper
  (pass-through to children).
- **VerifiedBy / References / DependsOn / Implements**: rendered as
  compact link pills using the link resolver.

### CSS Theming

CSS custom properties with sensible defaults, overridable by host:

```css
:root {
  --supersigil-verified: #22c55e;
  --supersigil-unverified: #94a3b8;
  --supersigil-partial: #f59e0b;
  --supersigil-failing: #ef4444;
  --supersigil-bg: transparent;
  --supersigil-border: #e2e8f0;
  --supersigil-text: inherit;
  --supersigil-text-muted: #64748b;
  --supersigil-font-mono: inherit;
}
```

VS Code host maps these to `var(--vscode-*)` tokens. IntelliJ host
injects overrides via a small `<style>` block in the JCEF preview.
Starlight host maps to its theme variables.

### Client-Side JavaScript

The preview script (`supersigil-preview.js`):

1. **Collapsibles**: click on `.supersigil-evidence-toggle` toggles
   the adjacent `.supersigil-evidence-list` via a `hidden` attribute.
2. **Tooltips**: hover on `.supersigil-badge` shows a title-attribute
   tooltip with the verification summary.
3. **MutationObserver**: watches `document.body` for added nodes
   matching `.supersigil-block` and re-initializes event listeners.
   Needed because the Markdown preview re-renders on content change.

### Build

esbuild bundles `render.ts` to a single ES module (for import by
VS Code extension and Starlight) and copies `supersigil-preview.css`
and `supersigil-preview.js` to `dist/`. The VS Code and IntelliJ
extensions copy these files into their `media/` directories as a
build step.

## VS Code Integration

### `package.json` Additions

```jsonc
{
  "contributes": {
    "markdown.markdownItPlugins": true,
    "markdown.previewStyles": ["./media/supersigil-preview.css"],
    "markdown.previewScripts": ["./media/supersigil-preview.js"]
  }
}
```

### Extension Host Changes (`extension.ts`)

The `activate()` function returns an `extendMarkdownIt` object:

```typescript
export function activate(context: vscode.ExtensionContext) {
  // ... existing LSP client setup ...

  const cache = new Map<string, DocumentComponentsResult>();

  return {
    extendMarkdownIt(md: any) {
      const defaultFence = md.renderer.rules.fence.bind(
        md.renderer.rules,
      );
      md.renderer.rules.fence = (
        tokens: any, idx: number, options: any, env: any, self: any,
      ) => {
        const token = tokens[idx];
        if (token.info.trim() === "supersigil-xml") {
          return renderFence(token, env, cache);
        }
        return defaultFence(tokens, idx, options, env, self);
      };
      return md;
    },
  };
}
```

### Cache Management

- On `supersigil/documentsChanged` notification: clear the entire
  cache, re-fetch `documentComponents` for any document whose
  Markdown preview is currently open (tracked via
  `vscode.window.visibleTextEditors`), then call
  `vscode.commands.executeCommand('markdown.preview.refresh')`.
- On cache miss during render: return a styled `<div>` placeholder
  with the class `.supersigil-loading`, fire an async fetch, and
  trigger `markdown.preview.refresh` when data arrives.

### Fence Correlation

The markdown-it fence token carries the raw source content and its
position in the document. The VS Code extension matches each fence
token to the `FenceData` entry whose byte range contains the token's
source position. When data is stale (`stale: true`), the extension
falls back to document-order correlation and renders the last-known
components with a visual stale indicator.

### Link Resolver

The VS Code link resolver produces `command:` URIs:

```typescript
const vscodeLinkResolver: LinkResolver = {
  evidenceLink: (file, line) =>
    `command:vscode.open?${encodeURIComponent(JSON.stringify([
      vscode.Uri.file(path.join(workspaceRoot, file)),
      { selection: { startLineNumber: line, startColumn: 1 } },
    ]))}`,
  documentLink: (docId) => {
    // Resolve via documentList cache (doc ID -> relative path)
    const entry = documentListCache.get(docId);
    if (!entry) return "#";
    return `command:vscode.open?${encodeURIComponent(
      JSON.stringify([vscode.Uri.file(
        path.join(workspaceRoot, entry.path),
      )]))}`;
  },
  criterionLink: (docId, criterionId) => {
    // Resolve doc file from documentList cache, then use
    // a custom command that triggers LSP go-to-definition
    // for the criterion ref "docId#criterionId"
    return `command:supersigil.goToCriterion?${
      encodeURIComponent(JSON.stringify([docId, criterionId]))}`;
  },
};
```

### Criterion Navigation Command

The extension registers a `supersigil.goToCriterion` command that:

1. Looks up the target document's file path from the
   `documentList` cache.
2. Sends a `textDocument/definition` request to the LSP with a
   synthetic position pointing to the criterion ref (or uses a
   dedicated `supersigil.resolveCriterion` execute-command if
   definition lookup is impractical).
3. Opens the resolved file at the criterion's source position.

This reuses the LSP's existing ref resolution logic rather than
embedding target locations in the render payload.

## IntelliJ Integration

### `plugin.xml` Additions

```xml
<depends>org.intellij.plugins.markdown</depends>

<extensions defaultExtensionNs="org.intellij.markdown">
  <fenceLanguageProvider
      implementation="org.supersigil.intellij.markdown.SupersigilFenceLanguageProvider"/>
  <browserPreviewExtensionProvider
      implementation="org.supersigil.intellij.markdown.SupersigilPreviewExtension"/>
</extensions>
```

Note: only `browserPreviewExtensionProvider` is used, not
`fenceGeneratingProvider`. `CodeFenceGeneratingProvider` is marked
both `@ApiStatus.Obsolete` and `@ApiStatus.Internal` — the
`@Internal` flag causes the JetBrains plugin verifier to reject
plugins that use it. `MarkdownBrowserPreviewExtension` is only
`@Obsolete` (not `@Internal`), making it safe for Marketplace
distribution. No replacement API exists yet; JetBrains is working
on a Compose-based Markdown preview (IJPL-157064) but has not
published a new extension API.

### Client-Side Rendering Approach

Instead of intercepting fence rendering server-side, the plugin
lets the Markdown preview render `supersigil-xml` fences as
standard `<code class="language-supersigil-xml">` elements. The
injected `supersigil-preview.js` finds these elements in the DOM
and replaces them with rendered components. This is the same
pattern used by the Mermaid plugin for IntelliJ.

### Data Access via `workspace/executeCommand`

IntelliJ's built-in LSP client does not support custom JSON-RPC
requests. Following the existing pattern (where `documentList` uses
`workspace/executeCommand("supersigil.documentList")`), the LSP
server registers a `supersigil.documentComponents` execute-command
handler that delegates to the same `handle_document_components`
logic:

```rust
// In commands.rs, alongside existing command handlers
"supersigil.documentComponents" => {
    let params: DocumentComponentsParams =
        serde_json::from_value(args)?;
    let result = self.handle_document_components_inner(params);
    Ok(serde_json::to_value(result)?)
}
```

The IntelliJ plugin fetches data via this command. The
`SupersigilPreviewExtension` injects a bootstrap script that
fetches component data via the JBCefJSQuery bridge and passes it
to `renderComponentTree()`.

### Fence Correlation

Since there is no server-side AST node access (no
`CodeFenceGeneratingProvider`), the injected JS correlates fences
by DOM order. The Markdown preview renders `supersigil-xml` fences
as sequential `<code class="language-supersigil-xml">` elements.
The JS matches the Nth such element to the Nth entry in the
`FenceData` array from the LSP response (which is ordered by byte
range). When data is stale, the JS renders the last-known components
with a visual stale indicator (dimmed opacity + "stale" badge).

### `SupersigilPreviewExtension`

Implements `MarkdownBrowserPreviewExtension.Provider`:

```kotlin
class SupersigilPreviewExtension :
    MarkdownBrowserPreviewExtension.Provider {

    override fun createBrowserExtension(
        panel: MarkdownHtmlPanel,
    ): MarkdownBrowserPreviewExtension {
        return object : MarkdownBrowserPreviewExtension {
            override val scripts = listOf("/supersigil-preview.js",
                                          "/supersigil-bridge.js")
            override val styles = listOf("/supersigil-preview.css")
            override fun dispose() {}
        }
    }
}
```

The bridge script (`supersigil-bridge.js`):

1. Registers a `JBCefJSQuery` handler for navigation actions.
2. On page load and on MutationObserver updates, fetches
   `documentComponents` data via the JBCefJSQuery bridge (calling
   `workspace/executeCommand("supersigil.documentComponents")`
   through the plugin).
3. Finds `<code class="language-supersigil-xml">` elements and
   replaces each with rendered components using
   `renderComponentTree()` with an IntelliJ link resolver.
4. The link resolver produces `javascript:` URIs that call the
   `JBCefJSQuery` function to open files in the IDE.

The `JBCefJSQuery` handler on the JVM side parses the action:

- `open-file:<path>:<line>` — opens the file at the given line via
  `FileEditorManager.openTextEditor()`.
- `open-criterion:<docId>:<criterionId>` — resolves the target
  document path from the `documentList` cache, then uses the LSP's
  definition resolution (via `workspace/executeCommand`) to find
  the criterion's source position and opens it.

## Starlight Spec Browser

### Dynamic Route: `website/src/pages/specs/[...slug].astro`

A catch-all Astro route that:

1. At build time, reads `public/specs/render-data.json`.
2. Implements `getStaticPaths()` returning one path per document.
3. For each document, calls `renderComponentTree()` (imported from
   `@supersigil/preview`) with the Starlight link resolver.
4. Renders the HTML string inside a Starlight-compatible layout.

### Index Page: `website/src/pages/specs/index.astro`

Reads the render data and document list to produce:

- A sidebar tree grouped by project and document type.
- Per-project coverage statistics (ratio of verified criteria).
- Links to individual spec pages.

### Starlight Link Resolver

The link resolver derives URLs from the Astro site configuration
(`base` from `astro.config.mjs` via `import.meta.env.BASE_URL`)
and a `SUPERSIGIL_REPO_URL` build-time environment variable for
evidence source links:

```typescript
function createStarlightLinkResolver(
  base: string,          // from import.meta.env.BASE_URL
  repositoryUrl: string, // e.g. "https://github.com/org/repo"
): LinkResolver {
  return {
    evidenceLink: (file, line) =>
      `${repositoryUrl}/blob/main/${file}#L${line}`,
    documentLink: (docId) =>
      `${base}specs/${docId}`,
    criterionLink: (docId, criterionId) =>
      `${base}specs/${docId}#${criterionId}`,
  };
}
```

Evidence links point to the source repository. Document and
criterion links point to other spec browser pages with anchor
fragments. All paths are derived from configuration, not
hard-coded.

## Testing Strategy

**LSP endpoint:** Integration tests in `supersigil-lsp` that parse
fixture `.req.md` files, optionally run verification, call the
handler, and assert the JSON response shape. Key cases: multi-fence
document, verified/unverified criteria, stale fallback, empty
document.

**Presentation kit:** Unit tests for `renderComponentTree()` — given
JSON fixtures, assert HTML output contains expected CSS classes, data
attributes, badge states, and link resolver output. Snapshot tests
for visual regression on representative component trees.

**VS Code:** Integration test that the markdown-it plugin transforms
`supersigil-xml` fences into `.supersigil-block` elements. Cache
management tested with mock LSP responses.

**IntelliJ:** Unit test for `SupersigilPreviewExtension` asserting it
provides the correct script and style resource paths. JBCefJSQuery
handler tested with mock bridge calls. The rendering itself is tested
via the shared presentation kit tests (same JS runs in all hosts).

**Starlight:** Build smoke test — `supersigil export --format json`
produces valid JSON, and `astro build` completes without errors.

## Decisions

```supersigil-xml
<Decision id="shared-js-not-rust-html">
  Use a shared TypeScript presentation kit rather than Rust-emitted
  HTML.

  <References refs="spec-rendering/req#req-2-1" />

  <Rationale>
  VS Code's markdown-it plugin runs in a JS sandbox. IntelliJ's JCEF
  preview renders HTML in Chromium. Starlight is an Astro/JS site.
  All three consumers are JavaScript environments. A TypeScript
  rendering function is native to all of them, while Rust-emitted
  HTML would need a process call from the synchronous markdown-it
  renderer (infeasible) and would produce static HTML without
  interactive features.
  </Rationale>

  <Alternative id="rust-emitted-html" status="rejected">
    A new Rust crate emits self-contained HTML fragments. Avoids a
    JS dependency but cannot serve the VS Code markdown-it plugin's
    synchronous rendering constraint, and cannot provide client-side
    interactivity without JS anyway.
  </Alternative>

  <Alternative id="web-components" status="rejected">
    A Lit-based web component library. Maximum code sharing but
    heavier dependency, potential CSP issues in VS Code's preview
    sandbox, and shadow DOM complicates theming with host CSS
    variables.
  </Alternative>
</Decision>

<Decision id="fence-grouped-response">
  Group components by fence in the LSP response rather than returning
  a flat document-level tree.

  <References refs="spec-rendering/req#req-1-1" />

  <Rationale>
  A document can contain multiple supersigil-xml fences. Editor
  preview plugins render per-fence (each fence token is a separate
  renderer call). Grouping by fence with byte ranges lets editors
  correlate each fence token to its component subset without
  re-parsing the XML client-side.
  </Rationale>

  <Alternative id="flat-tree" status="rejected">
    Return a flat component tree. Simpler response but forces each
    editor to re-derive which components belong to which fence,
    duplicating parsing logic.
  </Alternative>
</Decision>

<Decision id="link-resolver-pattern">
  Use a host-provided link resolver callback rather than baking
  host-specific URIs into the shared renderer.

  <References refs="spec-rendering/req#req-2-1" />

  <Rationale>
  VS Code needs command: URIs, IntelliJ needs JBCefJSQuery bridge
  calls, and Starlight needs site-relative paths. A link resolver
  interface keeps the rendering function host-agnostic while allowing
  each host full control over navigation behavior.
  </Rationale>

  <Alternative id="post-processing" status="rejected">
    Render with placeholder hrefs and post-process the HTML to
    rewrite them. Fragile (depends on stable placeholder format) and
    requires an extra DOM parse step.
  </Alternative>
</Decision>

<Decision id="client-side-render-intellij">
  Use client-side rendering in IntelliJ's JCEF preview via the shared
  JS bundle, using only the browserPreviewExtensionProvider extension
  point.

  <References refs="spec-rendering/req#req-4-1, spec-rendering/req#req-2-1" />

  <Rationale>
  CodeFenceGeneratingProvider is marked both @ApiStatus.Obsolete and
  @ApiStatus.Internal — the @Internal flag causes the JetBrains
  plugin verifier to reject plugins, blocking Marketplace
  distribution. MarkdownBrowserPreviewExtension is only @Obsolete
  (safe for distribution). By injecting the shared
  supersigil-preview.js via the browser extension, the plugin uses
  the same rendering code as VS Code and Starlight. The injected JS
  finds code elements with class language-supersigil-xml and replaces
  them — the same pattern used by the Mermaid plugin. This preserves
  the single-implementation guarantee and avoids Internal APIs.
  </Rationale>

  <Alternative id="fence-generating-provider" status="rejected">
    Use CodeFenceGeneratingProvider to intercept fence rendering
    server-side. Cleaner data flow but the @ApiStatus.Internal flag
    blocks Marketplace distribution and the API has no guaranteed
    stability.
  </Alternative>

  <Alternative id="kotlin-port" status="rejected">
    Port the rendering function to Kotlin. Avoids client-side JS
    complexity but creates a second implementation that must be kept
    in sync with the TypeScript original. Any presentation change
    requires updating two codebases.
  </Alternative>
</Decision>

<Decision id="dynamic-astro-pages">
  Use dynamic Astro pages for the spec browser rather than Starlight
  content pages.

  <References refs="spec-rendering/req#req-5-1" />

  <Rationale>
  Spec .req.md files have non-standard frontmatter (id, doc_type,
  status) that does not match Starlight's content schema. Dynamic
  Astro routes give full control over layout, sidebar, and filtering
  without polluting the docs content collection. The existing graph
  explorer already uses this pattern successfully.
  </Rationale>

  <Alternative id="starlight-content" status="rejected">
    Copy/symlink spec files into Starlight's content directory with a
    remark plugin to transform fences. Requires frontmatter
    translation, risks content collection conflicts, and limits
    layout control.
  </Alternative>
</Decision>

<Decision id="invalidate-all-on-change">
  Invalidate the entire component cache on documentsChanged rather
  than tracking per-document changes.

  <References refs="spec-rendering/req#req-3-2" />

  <Rationale>
  The existing supersigil/documentsChanged notification carries no
  payload. Changing the notification contract to include changed URIs
  is a cross-cutting protocol change affecting all editor extensions.
  Full cache invalidation with lazy re-fetch for open previews is
  simpler and sufficient — the number of simultaneously open previews
  is small, and the documentComponents request is fast (reads from
  in-memory graph).
  </Rationale>

  <Alternative id="notification-with-uris" status="deferred">
    Extend the documentsChanged notification to include changed
    document URIs. More efficient for targeted invalidation but
    requires updating the notification contract, both editor
    extensions, and adds complexity for marginal benefit given the
    small number of open previews.
  </Alternative>
</Decision>
```
