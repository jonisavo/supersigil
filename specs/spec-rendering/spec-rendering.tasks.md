---
supersigil:
  id: spec-rendering/tasks
  type: tasks
  status: done
title: "Spec Rendering"
---

```supersigil-xml
<DependsOn refs="spec-rendering/design" />
```

## Overview

Eight tasks in dependency order: LSP data types, LSP request handler
with execute-command mirror, presentation kit package, VS Code
markdown-it integration, IntelliJ browser extension, CLI render
command, Starlight spec browser, and a final verification pass.
TDD applies to all Rust and TypeScript tasks — write tests first
for each criterion.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="spec-rendering/req#req-1-2, spec-rendering/req#req-1-3"
>
  Define LSP data types in a new
  `crates/supersigil-lsp/src/document_components.rs` module:
  `DocumentComponentsRequest`, `DocumentComponentsParams`,
  `DocumentComponentsResult`, `FenceData`, `RenderedComponent`,
  `VerificationStatus`, `EvidenceEntry`, `ProvenanceEntry`,
  `EdgeData`, `SourceRange`. Write serde round-trip tests for each
  type. Write a builder/test-helper function that constructs a
  representative `DocumentComponentsResult` for use in downstream
  tests.
</Task>

<Task
  id="task-2"
  status="done"
  implements="spec-rendering/req#req-1-1, spec-rendering/req#req-1-4"
  depends="task-1"
>
  Implement the `handle_document_components` handler in the LSP.
  Register it on the Router as both a custom request
  (`supersigil/documentComponents`) and a `workspace/executeCommand`
  mirror (`supersigil.documentComponents`). The handler resolves URI
  to document ID, reads the parsed SpecDocument, groups components
  by fence byte range, enriches verifiable components with
  verification status from the ArtifactGraph, collects outgoing
  edges, and sets `stale: true` when falling back to a previous
  parse.

  TDD: write integration tests first using fixture `.req.md` files:
  (a) single-fence document with verified criterion,
  (b) multi-fence document with mixed verification states,
  (c) document with parse errors returning stale data,
  (d) unknown URI returning empty fence list.
</Task>

<Task
  id="task-3"
  status="done"
  implements="spec-rendering/req#req-2-1, spec-rendering/req#req-2-2, spec-rendering/req#req-2-3, spec-rendering/req#req-2-4, spec-rendering/req#req-2-5, spec-rendering/req#req-2-6, spec-rendering/req#req-2-7"
  depends="task-1"
>
  Create the shared presentation kit at `packages/preview/`.
  Set up `package.json`, `tsconfig.json`, and `esbuild.mjs`.

  Implement `renderComponentTree(fences, edges, linkResolver)` in
  `src/render.ts` with TypeScript interfaces in `src/types.ts`.
  Render Criterion (badge + collapsible evidence), Decision
  (rationale + alternatives), AcceptanceCriteria (wrapper),
  unsupported or uncustomized kinds via a generic wrapper, and link
  components (VerifiedBy, References, DependsOn, Implements as link
  pills).

  Create `styles/supersigil-preview.css` with CSS custom properties
  for theming and all component styles.

  Create `scripts/supersigil-preview.js` with collapsible toggles,
  badge tooltips, and MutationObserver re-initialization.

  TDD: write unit tests first for `renderComponentTree()` — given
  JSON fixtures, assert output contains expected CSS classes, badge
  states, evidence details, and link resolver URIs. Add snapshot
  tests for a representative multi-fence component tree.

  Build produces `dist/supersigil-preview.css`,
  `dist/supersigil-preview.js`, and `dist/render.js` (ES module).
</Task>

<Task
  id="task-4"
  status="done"
  implements="spec-rendering/req#req-3-1, spec-rendering/req#req-3-2, spec-rendering/req#req-3-3, spec-rendering/req#req-3-4, spec-rendering/req#req-3-5"
  depends="task-2, task-3"
>
  Integrate the presentation kit into the VS Code extension.

  Copy `dist/supersigil-preview.css` and `dist/supersigil-preview.js`
  into `editors/vscode/media/` as a build step.

  Add `markdown.markdownItPlugins`, `markdown.previewStyles`, and
  `markdown.previewScripts` contribution points to `package.json`.

  In `extension.ts`: return `extendMarkdownIt` from `activate()`
  that overrides `md.renderer.rules.fence` for `supersigil-xml`
  fences. Implement per-document cache of `documentComponents`
  responses. On `supersigil/documentsChanged`: invalidate cache,
  re-fetch for open previews, trigger `markdown.preview.refresh`.
  On cache miss: return loading placeholder, fetch async, refresh on
  arrival. On stale data: render with stale indicator, use
  document-order correlation.

  Implement VS Code link resolver with `command:vscode.open` for
  evidence and document links. Register `supersigil.goToCriterion`
  command for cross-document criterion navigation via LSP
  definition resolution.

  Fence correlation: match markdown-it token source position to
  `FenceData.byte_range`; fall back to document order when stale.

  TDD: write integration test that the markdown-it plugin transforms
  a `supersigil-xml` fence into HTML containing `.supersigil-block`
  elements. Test cache invalidation with mock LSP responses.
</Task>

<Task
  id="task-5"
  status="done"
  implements="spec-rendering/req#req-4-1, spec-rendering/req#req-4-2, spec-rendering/req#req-4-3"
  depends="task-2, task-3"
>
  Integrate the presentation kit into the IntelliJ plugin.

  Copy `dist/supersigil-preview.css` and `dist/supersigil-preview.js`
  into the plugin's resources.

  Add `<depends>org.intellij.plugins.markdown</depends>` to
  `plugin.xml`. Register `fenceLanguageProvider` for
  `supersigil-xml` autocompletion. Register
  `browserPreviewExtensionProvider` implementing
  `MarkdownBrowserPreviewExtension.Provider`.

  The browser extension injects `supersigil-preview.css`,
  `supersigil-preview.js`, and a bridge script
  (`supersigil-bridge.js`). The bridge script:
  (a) registers `JBCefJSQuery` handler for navigation actions,
  (b) on page load + MutationObserver, fetches `documentComponents`
      via JBCefJSQuery bridge calling
      `workspace/executeCommand("supersigil.documentComponents")`,
  (c) finds elements with class language-supersigil-xml and
      replaces them with rendered components using
      `renderComponentTree()` with IntelliJ link resolver,
  (d) link resolver produces `javascript:` URIs calling JBCefJSQuery
      for `open-file` and `open-criterion` actions.

  JVM-side handler: parse action protocol, open files via
  `FileEditorManager`, resolve criterion refs via LSP
  `workspace/executeCommand`.

  TDD: unit test for `SupersigilPreviewExtension` resource paths.
  Test JBCefJSQuery handler with mock bridge calls.
</Task>

<Task
  id="task-6"
  status="done"
  implements="spec-rendering/req#req-1-5"
  depends="task-2"
>
  Add `supersigil render --format json` CLI subcommand in
  `crates/supersigil-cli/src/commands/render.rs`. Iterate all
  documents in the graph, build fence-grouped component trees with
  verification data using the same logic as the LSP handler, and
  output a JSON array of `DocumentComponentsResult` objects.

  TDD: write integration tests first:
  (a) render output for a multi-document project matches expected
      JSON structure,
  (b) render output includes verification status when verify data
      is available,
  (c) render with `--format json` flag produces valid JSON to stdout.

  Register the subcommand in the CLI's command dispatch.
</Task>

<Task
  id="task-7"
  status="done"
  implements="spec-rendering/req#req-5-1, spec-rendering/req#req-5-2, spec-rendering/req#req-5-3, spec-rendering/req#req-5-4"
  depends="task-3, task-6"
>
  Build the Starlight spec browser.

  Add `supersigil render --format json > public/specs/render-data.json`
  to the website's `prebuild` script.

  Create `website/src/pages/specs/index.astro`: read render data,
  display document tree grouped by project and doc_type, show
  per-project coverage stats, link to individual spec pages.

  Create `website/src/pages/specs/[...slug].astro`: dynamic route
  implementing `getStaticPaths()` from render data, render each
  document's components using `renderComponentTree()` from
  `@supersigil/preview` with the Starlight link resolver. Derive
  URLs from `import.meta.env.BASE_URL` and `SUPERSIGIL_REPO_URL`
  env var.

  Import `supersigil-preview.css` and include
  `supersigil-preview.js` as a client-side script for interactivity.

  Test: build smoke test — `supersigil render --format json` produces
  valid JSON for the project's own specs, and `astro build` completes
  without errors.
</Task>

<Task
  id="task-8"
  status="done"
  depends="task-4, task-5, task-6, task-7"
>
  Final verification pass. Run:
  - `supersigil verify` — all spec-rendering criteria covered
  - `cargo fmt --all` — no formatting issues
  - `cargo clippy --workspace --all-targets --all-features` — no warnings
  - `cargo nextest run` — all Rust tests pass
  - VS Code extension `pnpm run build` — no TypeScript errors
  - IntelliJ plugin `./gradlew build` — no Kotlin errors
  - Website `pnpm run build` — Astro builds successfully

  Manually verify: open a `.req.md` file in VS Code, open Markdown
  Preview, confirm supersigil-xml fences render as rich components
  with verification badges.
</Task>
```
