---
supersigil:
  id: graph-explorer/adr
  type: adr
  status: draft
title: "Visual Graph Explorer"
---

```supersigil-xml
<References refs="graph-explorer/req, graph-explorer/design" />
```

## Context

The supersigil CLI currently outputs the document graph as static Mermaid or
Graphviz DOT text. Developers need an interactive, browser-based visualization
for orientation, impact analysis, and coverage audit. This requires choosing
a visualization library, a graph layout strategy, and a delivery model.

```supersigil-xml
<Decision id="decision-1">
  Use D3.js with SVG rendering for the graph visualization.

  <References refs="graph-explorer/req#req-4-1, graph-explorer/req#req-10-1, graph-explorer/req#req-10-2" />

  <Rationale>
    D3's d3-force simulation is the established standard for interactive graph
    visualization. SVG rendering allows direct CSS styling with the existing
    Obsidian Glow design tokens — colors, typography, and spacing are applied
    through standard CSS custom properties rather than a library-specific
    styling API. This is critical because the explorer must feel native to the
    supersigil docs site. Canvas-based alternatives would require reimplementing
    the design system in a renderer-specific format.
  </Rationale>

  <Alternative id="alt-1-cytoscape" status="rejected">
    Cytoscape.js offers built-in compound node support (clusters) and a
    CSS-like selector API. However, it renders to Canvas only, meaning all
    styling must go through Cytoscape's own API rather than standard CSS.
    Matching the Obsidian Glow design system would require translating every
    token into Cytoscape style properties. The library is also heavier
    (~400KB) and its visual output is harder to customize at the level of
    polish needed for a flagship docs feature.
  </Alternative>

  <Alternative id="alt-1-visnetwork" status="rejected">
    vis-network provides quick setup with decent defaults and built-in
    clustering. However, the project is less actively maintained, its
    Canvas-based rendering has the same CSS limitation as Cytoscape, and its
    clustering API is more limited. Visual polish would be the hardest to
    achieve of all three options.
  </Alternative>
</Decision>

<Decision id="decision-2">
  Use a clustered force-directed layout with document-level nodes and
  drill-down to components.

  <References refs="graph-explorer/req#req-4-1, graph-explorer/req#req-4-4" />

  <Rationale>
    The spec graph has natural groupings (documents sharing a prefix or
    subsystem) but no strict hierarchy — documents can reference across
    subsystem boundaries. A pure hierarchical/DAG layout would force
    artificial layering and struggle with cross-cutting edges. A pure
    force-directed layout without clustering would produce a hairball at
    scale. The hybrid approach groups related documents visually while
    allowing organic positioning of cross-cluster relationships.

    Document-level as the default view keeps the graph manageable even for
    large spec repositories. Component-level detail (Criteria, Tasks,
    Decisions) is available on demand via drill-down, avoiding information
    overload in the overview.
  </Rationale>

  <Alternative id="alt-2-dag" status="rejected">
    A strict hierarchical DAG layout (requirements at top, designs below,
    tasks at bottom) respects the dependency flow but breaks down with
    cross-cutting References edges and ADRs that don't fit neatly into any
    layer. It also wastes horizontal space and scales poorly when document
    counts per layer are uneven.
  </Alternative>

  <Alternative id="alt-2-force-flat" status="rejected">
    A flat force-directed layout without clustering treats every document
    equally. This works for small graphs but becomes unreadable quickly as
    nodes intermingle without visual grouping. The user loses the ability to
    see subsystem boundaries at a glance.
  </Alternative>
</Decision>

<Decision id="decision-3">
  Deliver the explorer as both an Astro docs page and a standalone CLI
  command, sharing a single visualization module.

  <References refs="graph-explorer/req#req-2-1, graph-explorer/req#req-3-1" />

  <Rationale>
    The Astro docs site already exists with the Obsidian Glow design system
    and Starlight infrastructure. Hosting the explorer at /explore/ makes it
    discoverable alongside documentation with zero extra setup. The standalone
    `supersigil explore` command serves developers who want a quick local view
    without building the docs site.

    Sharing the JS module between both surfaces avoids duplication and ensures
    visual consistency. The module is framework-agnostic vanilla JS, so it
    works in both contexts without adaptation.
  </Rationale>

  <Alternative id="alt-3-embedded-server" status="rejected">
    Embedding an HTTP server in the Rust binary (like cargo doc --open) would
    make the CLI self-contained but adds significant complexity — a web
    server dependency, port management, graceful shutdown. A static HTML file
    achieves the same result with zero runtime dependencies.
  </Alternative>
</Decision>

<Decision id="decision-4">
  Use vanilla JS with JSDoc types instead of TypeScript for the visualization
  module.

  <References refs="graph-explorer/req#req-2-1, graph-explorer/design" />

  <Rationale>
    JSDoc-typed vanilla JS provides full LSP support (autocompletion, type
    checking, go-to-definition) without requiring a TypeScript compilation
    step. The module must work both as an Astro component import and as an
    inline script in the standalone HTML. Avoiding a build step simplifies
    both integration paths. The standalone HTML can embed the JS directly
    without bundling.
  </Rationale>

  <Alternative id="alt-4-typescript" status="rejected">
    TypeScript would provide stronger type guarantees but introduces a build
    step. The standalone HTML embed path would need either bundling or a
    separate compilation target. The added complexity is not justified for a
    module of this size where JSDoc provides adequate type safety.
  </Alternative>
</Decision>
```

## Consequences

- D3.js becomes a frontend dependency. It is loaded via CDN in the standalone
  HTML and as an npm dependency in the Astro site.
- The `graph` command gains a `--format json` flag, establishing a JSON wire
  format that downstream tools can depend on.
- The visualization module is a shared asset that must maintain compatibility
  with both delivery surfaces. Changes to the module must be tested in both
  contexts.
- Coverage data is excluded from v1 JSON. A future `--with-coverage` flag
  will require the graph command to also run the verify pipeline, which is a
  larger architectural change.
