---
supersigil:
  id: document-format/mdx-adr
  type: adr
  status: superseded
title: "ADR: MDX Document Format (Superseded)"
---

```supersigil-xml
<References refs="document-graph/req, config/req" />
```

## Context

Supersigil needs a document format that is expressive enough to capture
structured specifications (criteria, implementations, evidence) while
remaining readable as plain documentation. The format must be parseable
in Rust without a JavaScript runtime, work with existing documentation
ecosystems, and stay simple enough for AI agents to author reliably.

## Decisions

```supersigil-xml
<Decision id="mdx-for-components">
  Supersigil uses MDX for structured components in prose. MDX provides
  actual AST nodes for components, eliminating the need for convention-based
  parsing of plain markdown.

  <References refs="config/req#req-2-2" />

  <Rationale>
    Components degrade gracefully in non-MDX renderers — the content
    inside them is still visible as text. The ecosystem (Astro,
    Docusaurus, Next.js) can render them as actual UI components, so specs
    can double as publishable documentation. And MDX parsing via the
    `markdown` crate (markdown-rs) keeps everything in Rust without
    shelling out to a JavaScript runtime.
  </Rationale>

  <Alternative id="plain-markdown-conventions" status="rejected">
    Extracting structured data from HTML comments or fenced code blocks
    in plain markdown. This avoids the MDX parsing weight but loses the
    AST — supersigil would need convention-based parsing that is fragile
    and underspecified. If a future user needs plain-markdown support, a
    fallback parser could be added, but this is not planned for v1.
  </Alternative>
</Decision>

<Decision id="components-carry-semantics">
  Document types (`requirements`, `design`, `tasks`) are classification
  tags for humans and documentation tooling. Supersigil's verification
  engine operates on the component graph: `&lt;Criterion&gt;`, `&lt;VerifiedBy&gt;`,
  `&lt;Implements&gt;`, etc. A single document can contain any combination of
  components, and custom workflows can use document types that supersigil
  has no built-in knowledge of.

  <References refs="document-graph/req#req-1-2, config/req#req-2-1, config/req#req-2-3" />

  <Rationale>
    Coupling semantics to document types would mean supersigil needs
    built-in knowledge of every workflow. Instead, a user who calls their
    documents "user stories" simply uses `type: user-story` in front
    matter. As long as those documents contain `&lt;Criterion&gt;` components,
    coverage checking works identically. This makes the tool
    workflow-agnostic while keeping the component graph precise.
  </Rationale>

  <Alternative id="type-bound-components" status="rejected">
    Restricting which components are valid per document type (e.g.
    `&lt;Criterion&gt;` only in `type: requirements`) would add enforcement
    complexity without proportional benefit. It would also prevent
    emergent patterns like embedding a single Decision in a design doc
    instead of creating a separate ADR document.
  </Alternative>
</Decision>

<Decision id="string-only-attributes">
  All attribute values must be plain string literals. Multi-value
  attributes use comma-separated strings (`refs="a, b, c"`). JSX
  expression attributes (`refs={[...]}`) are rejected.

  <References refs="config/req#req-2-5" />

  <Rationale>
    JSX expression attributes require either evaluating JavaScript
    (heavyweight, unsafe, non-deterministic) or parsing a subset of JS
    (underspecified, brittle). Comma-separated strings are trivially
    parseable, unambiguous, and work identically across every MDX parser.
    The tradeoff is that commas are prohibited in IDs and paths — a
    restriction enforced by lint with no practical cost.
  </Rationale>

  <Alternative id="jsx-expression-attributes" status="rejected">
    Supporting JSX expressions like `refs={["a", "b"]}` would be more
    familiar to JavaScript developers but would require either a JS
    runtime or a partial JS parser. The complexity is disproportionate
    to the benefit. Comma-separated strings cover every real use case.
  </Alternative>
</Decision>

<Decision id="tasks-as-components">
  Task tracking is modeled as `&lt;Task&gt;` components.

  <References refs="document-graph/req#req-2-3, document-graph/req#req-3-1" />

  <Rationale>
    Making each task a separate document would produce 10-20 documents
    per feature, which is unwieldy. The component model keeps task
    granularity inside the document boundary while still making tasks
    individually referenceable via fragment syntax
    (`auth/tasks/login#adapter-code`). Task ordering is verified by
    supersigil (cycle detection, topological sort). Task execution is
    the agent's responsibility. It reads the plan, picks up the next
    task, edits the `status` attribute in the MDX, and commits.
  </Rationale>

  <Alternative id="tasks-as-documents" status="rejected">
    One document per task would explode the file count and lose the
    natural grouping that a tasks document provides. It would also
    require a different dependency mechanism — `depends` on `&lt;Task&gt;`
    handles ordering within a document (the common case), while
    `&lt;DependsOn&gt;` at the document level handles the rarer cross-document
    case.
  </Alternative>
</Decision>

<Decision id="freeform-ids">
  IDs are declared in front matter and are freeform strings. An optional
  `id_pattern` in config lets teams enforce conventions via warnings.

  <References refs="document-graph/req#req-1-1, config/req#req-4-1" />

  <Rationale>
    Freeform IDs are resistant to AI agent hallucination. Agents can use
    any string while remaining correctable (verification catches
    broken refs). This decouples identity from filesystem layout, so
    reorganizing files is never a breaking change.
  </Rationale>

  <Alternative id="path-derived-ids" status="rejected">
    Deriving IDs from file paths couples identity to filesystem layout,
    making reorganization a breaking change. It also makes IDs long and
    unwieldy for deeply nested files.
  </Alternative>
</Decision>
```

## Consequences

These decisions produce a format that is simultaneously machine-parseable
(typed AST nodes), human-readable (degrades to prose in any markdown
renderer), and ecosystem-compatible (renders as React components in Astro,
Docusaurus, Next.js). The main cost is the MDX parsing complexity, which
is contained within the `markdown` crate dependency.
