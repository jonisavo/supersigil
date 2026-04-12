---
supersigil:
  id: graph-explorer/tasks
  type: tasks
  status: done
title: "Visual Graph Explorer"
---

## Overview

Implementation proceeds in three phases: Rust-side JSON export, JS
visualization module, and delivery integration (Astro page + standalone
command). Tasks are dependency-ordered so each builds on the previous.

```supersigil-xml
<Task id="task-1" status="done" implements="graph-explorer/req#req-1-1, graph-explorer/req#req-1-5">
  Add `Json` variant to `GraphFormat` enum and wire it through clap argument
  parsing. Extend `clap_parse.rs` tests for `graph --format json`.
</Task>

<Task id="task-2" status="done" depends="task-1" implements="graph-explorer/req#req-1-2, graph-explorer/req#req-1-3, graph-explorer/req#req-1-4">
  Implement JSON serialization of the document graph. Create serde-serializable
  structs matching the Graph_JSON schema (DocumentNode, Component, Edge).
  Source edges from DocumentGraph reverse mappings. Source title from
  frontmatter extra with fallback to document ID. Unit tests for round-trip
  serialization, empty graph, null fields, and nested Decision children.
</Task>

<Task id="task-3" status="done" depends="task-2">
  Set up the JS project structure under `website/src/components/explore/`.
  Create stub files with JSDoc type definitions matching the Graph_JSON
  schema. Add D3.js as an npm dependency in the website package. Configure
  vitest for the JS test suite.
</Task>

<Task id="task-4" status="done" depends="task-3" implements="graph-explorer/req#req-4-1, graph-explorer/req#req-4-2, graph-explorer/req#req-4-3">
  Implement the core D3 force simulation in `graph-explorer.js`. Mount
  function accepts a container element and Graph_JSON data. Render document
  nodes as circles with type-colored strokes, cluster borders as dashed
  rectangles, and edges as lines. Wire up d3-zoom (scroll to zoom, drag to
  pan) and d3-drag (reposition nodes).
</Task>

<Task id="task-5" status="done" depends="task-4" implements="graph-explorer/req#req-10-1, graph-explorer/req#req-10-2">
  Create `styles.css` using only CSS custom property references from the
  Obsidian Glow design tokens. Style nodes, edges, clusters, labels, and the
  top bar. Verify both dark and light themes render correctly by toggling
  Starlight's theme switch.
</Task>

<Task id="task-6" status="done" depends="task-4" implements="graph-explorer/req#req-4-4">
  Implement cluster drill-down. Double-click a document node to expand its
  components (Criteria, Tasks, Decisions with children) as child nodes inside
  the cluster border. Add internal edges for Task implements and Decision
  children. Double-click again to collapse. Animate expansion/collapse.
</Task>

<Task id="task-7" status="done" depends="task-4" implements="graph-explorer/req#req-5-1, graph-explorer/req#req-5-2">
  Implement the detail sidebar in `detail-panel.js`. Render document ID,
  type/status badges, edge list, and component list. Slide-in animation from
  the right. Gold highlight ring on selected node. Close on Escape or
  re-click.
</Task>

<Task id="task-8" status="done" depends="task-4" implements="graph-explorer/req#req-6-1, graph-explorer/req#req-6-2, graph-explorer/req#req-6-3">
  Implement filtering in `graph-data.js`. Type toggle chips derived from
  unique doc_type values in the data. Status dropdown. Filter state produces
  a visible node set. Filtered-out nodes fade to near-invisible. Vitest
  coverage for filter combinations.
</Task>

<Task id="task-9" status="done" depends="task-4" implements="graph-explorer/req#req-7-1, graph-explorer/req#req-7-2">
  Implement impact trace in `impact-trace.js`. Compute transitive closure of
  downstream documents. Traced subgraph at full opacity with gold edges,
  everything else dimmed. Clear on background click. Vitest coverage for
  linear chains, diamonds, and disconnected subgraphs.
</Task>

<Task id="task-10" status="done" depends="task-4" implements="graph-explorer/req#req-8-1, graph-explorer/req#req-8-2">
  Implement search. Fuzzy match on document ID and title. Dropdown results
  list, keyboard-navigable. Select zooms to node and opens detail panel.
  `/` key focuses search input.
</Task>

<Task id="task-11" status="done" depends="task-7, task-8, task-9, task-10" implements="graph-explorer/req#req-9-1, graph-explorer/req#req-9-2">
  Implement URL deep-linking in `url-router.js`. Parse hash on load, update
  hash on interaction. Support `#/doc/{id}`, `#/doc/{id}/trace`, and
  `#/filter/type:{csv},status:{value}`. Vitest coverage for round-trip
  parsing.
</Task>

<Task id="task-12" status="done" depends="task-5, task-6, task-7, task-8, task-9, task-10, task-11" implements="graph-explorer/req#req-3-1, graph-explorer/req#req-3-2">
  Create the Astro page at `website/src/pages/explore.astro`. Import the JS
  module and styles. Add prebuild script to `package.json` that runs
  `supersigil graph --format json &gt; public/explore/graph.json`. Verify the
  page renders correctly in the Astro dev server.
</Task>

<Task id="task-13" status="done" depends="task-2, task-12" implements="graph-explorer/req#req-2-1, graph-explorer/req#req-2-2">
  Implement the `explore` CLI command. Embed the HTML template and JS module
  via `include_str!`. Inline Graph_JSON into the template. Write to temp file
  or `--output` path. Open with `open::that()`. Add clap parsing and tests.
</Task>

<Task id="task-14" status="done" implements="graph-explorer/req#req-11-1">
  Update `build_html` to accept optional `RepositoryInfo`, serialize it as
  JSON, and inject it via a `REPOSITORY_INFO` placeholder in the HTML
  template. Update the template to pass repository info to the `mount()` call.
  Update the `explore` command `run` function to resolve repository info from
  config and plugins before calling `build_html`. Tests for HTML output with
  and without repository info.
</Task>

<Task id="task-15" status="done" depends="task-14" implements="graph-explorer/req#req-11-1, graph-explorer/req#req-11-2, graph-explorer/req#req-11-3">
  Update `mount()` in `graph-explorer.js` to accept `repositoryInfo` as a
  fourth parameter and pass it to `renderDetail`. Update
  `createExplorerLinkResolver` in `detail-panel.js` to use provider-specific
  URL templates when `repositoryInfo` is present. When `null`, render evidence
  locations as plain text. Remove hardcoded repository URL from
  `graph-explorer.js`. Rebuild `explore_standalone.js`.
</Task>

<Task id="task-16" status="done" depends="task-15" implements="graph-explorer/req#req-11-1">
  Update the Astro page at `website/src/pages/explore.astro` to pass
  repository info to `mount()`.
</Task>

<Task id="task-17" status="done" implements="graph-explorer/req#req-2-3">
  Handle open::that() failure gracefully. Instead of propagating the error,
  catch it and print a diagnostic message to stderr suggesting --output.
  Still print the temp file path. Exit successfully.
</Task>
```
