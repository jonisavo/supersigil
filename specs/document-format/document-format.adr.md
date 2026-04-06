---
supersigil:
  id: document-format/adr
  type: adr
  status: accepted
title: "ADR: Markdown with Supersigil-XML Fences"
---

```supersigil-xml
<References refs="document-graph/req, config/req, document-format/mdx-adr" />
```

## Context

Supersigil originally used MDX as its document format (see
`document-format/mdx-adr`). MDX provided real AST nodes for components
and graceful degradation in plain markdown renderers.

In practice, MDX introduced significant friction:

- **Editor support is poor.** JetBrains' MDX parser does not work
  correctly — components enclosed in backticks are still treated as live
  JSX. VSCode's MDX extension provides no preview capability; a separate
  preview extension is required.
- **Format complexity is disproportionate.** MDX is "JSX in Markdown",
  carrying the full conceptual weight of JavaScript. Supersigil uses
  none of that — only PascalCase elements with string attributes.
- **The format bleeds into tooling.** Files must use the `.mdx`
  extension, which lacks the universal editor support that `.md` enjoys
  (native preview, TOC, spellcheck, folding).

Supersigil needs a format that retains structured components but uses
standard Markdown as the carrier, with universal editor support and a
grammar no more complex than what the tool actually uses.

## Decisions

```supersigil-xml
<Decision id="markdown-with-fences">
  Supersigil documents are standard Markdown (`.md`) files with YAML
  front matter. Structured components live inside fenced code blocks
  with the `supersigil-xml` language identifier.

  <References refs="config/req#req-2-2" />

  <Rationale>
    Standard Markdown is universally supported: every editor provides
    preview, syntax highlighting, folding, and spellcheck for `.md`
    files without extensions. Fenced code blocks provide an explicit,
    unambiguous boundary between prose and structured content. There is
    no risk of a parser treating prose as components or vice versa. The
    front matter detection (`supersigil:` namespace) is unchanged.
  </Rationale>

  <Alternative id="keep-mdx" status="superseded">
    Continue using MDX as the document format. Rejected because editor
    support is inadequate (JetBrains parser bugs, no VSCode preview) and
    the format carries JavaScript complexity that supersigil does not
    use. See `document-format/mdx-adr`.
  </Alternative>

  <Alternative id="markdown-directives" status="rejected">
    Use CommonMark directive syntax (`:::Component`) for structured
    content. Rejected because the directive proposal is still a draft
    with competing syntaxes (remark-directive vs MyST), nesting degrades
    at depth (increasing colon counts), and no Rust parser supports it.
  </Alternative>
</Decision>

<Decision id="supersigil-xml-grammar">
  The content inside `supersigil-xml` fences is an XML subset:
  PascalCase elements, double-quoted string attributes, nesting, and
  text content. No processing instructions, CDATA, DTD, namespaces,
  comments, or entity references beyond `&amp;`, `&lt;`, `&gt;`,
  `&quot;`.

  <References refs="parser-pipeline/req#req-5-1" />

  <Rationale>
    Every component invocation in supersigil is already valid XML. The
    full MDX/JSX grammar — boolean attributes, expression attributes,
    fragments, spread syntax — was never used and was actively rejected
    by the parser. An XML subset is fully specified, trivially parseable
    (e.g. with the `quick-xml` crate), and eliminates the `markdown` crate's
    MDX mode as a dependency. AI agents produce XML with high reliability.
  </Rationale>

  <Alternative id="kdl-grammar" status="deferred">
    Use KDL (kdl.dev) as the inner grammar. KDL is more concise (no
    closing tags), has a mature Rust crate (`kdl` v6) with
    format-preserving edits, and growing editor support. Deferred because
    XML is more universally familiar and body text is first-class in XML
    but a positional argument in KDL. The fence language scheme
    (`supersigil-xml` / `supersigil-kdl`) allows adding KDL support
    later without breaking changes.
  </Alternative>

  <Alternative id="custom-dsl" status="rejected">
    Design a purpose-built DSL (e.g. `@Component(attr="val")`). Maximum
    design freedom but zero existing editor support, no standard, and
    requires building TextMate/tree-sitter grammars from scratch. The
    benefit over XML does not justify the cost.
  </Alternative>
</Decision>

<Decision id="fence-language-naming">
  The fenced code block language identifier is `supersigil-xml`, not
  `xml` or `supersigil`. Future inner grammars use the pattern
  `supersigil-{grammar}` (e.g. `supersigil-kdl`).

  <References refs="parser-pipeline/req#req-4-2" />

  <Rationale>
    The `supersigil-` prefix makes blocks unambiguously identifiable as
    supersigil content, distinct from unrelated XML blocks. The grammar
    suffix (`-xml`) is explicit about what parser to use, avoiding
    auto-detection heuristics. The naming scheme allows adding new inner
    grammars without ambiguity or breaking changes. The VSCode extension
    provides syntax highlighting via grammar injection (injecting
    `source.xml` into `supersigil-xml` fences).
  </Rationale>

  <Alternative id="bare-xml-fence" status="rejected">
    Use `xml` as the fence language. Provides free syntax highlighting
    everywhere but cannot distinguish supersigil blocks from unrelated
    XML, and precludes grammar-specific LSP behavior.
  </Alternative>

  <Alternative id="bare-supersigil-fence" status="rejected">
    Use `supersigil` as the fence language. Unambiguous but does not
    encode the inner grammar, making it difficult to support multiple
    grammars in the future without a configuration-based dispatch.
  </Alternative>
</Decision>

<Decision id="grouped-fences">
  Related components are grouped together in a single fence. A document
  typically contains multiple fences interspersed with prose.

  <References refs="parser-pipeline/req#req-4-2" />

  <Rationale>
    Grouping related components (e.g. an `AcceptanceCriteria` block with
    its `Criterion` children and a `VerifiedBy`) reduces fence delimiter
    noise while preserving the ability to interleave prose between
    logical sections. One-component-per-fence is too verbose; one fence
    per document defeats the purpose of mixing prose and structure.
  </Rationale>
</Decision>

<Decision id="components-carry-semantics">
  Unchanged from `document-format/mdx-adr`. Document types are
  classification tags; the verification engine operates on the component
  graph.

  <References refs="document-graph/req#req-1-2, config/req#req-2-1, config/req#req-2-3" />

  <Rationale>
    Coupling semantics to document types would mean supersigil needs
    built-in knowledge of every workflow. Instead, a user who calls
    their documents "user stories" simply uses `type: user-story` in
    front matter. As long as those documents contain `Criterion`
    components, coverage checking works identically. This makes the
    tool workflow-agnostic while keeping the component graph precise.
  </Rationale>
</Decision>

<Decision id="string-only-attributes">
  Unchanged from `document-format/mdx-adr`. All attribute values are
  double-quoted string literals. Multi-value attributes use
  comma-separated strings.

  <References refs="config/req#req-2-5" />

  <Rationale>
    Expression attributes require either evaluating a scripting
    language or parsing a subset of one — both disproportionately
    complex. Comma-separated strings are trivially parseable,
    unambiguous, and cover every real use case. The tradeoff is that
    commas are prohibited in IDs and paths, enforced by lint with no
    practical cost.
  </Rationale>
</Decision>

<Decision id="tasks-as-components">
  Unchanged from `document-format/mdx-adr`. Task tracking is modeled as
  `Task` components within documents.

  <References refs="document-graph/req#req-2-3, document-graph/req#req-3-1" />

  <Rationale>
    Making each task a separate document would produce many documents
    per feature. The component model keeps task granularity inside the
    document boundary while making tasks individually referenceable via
    fragment syntax. Task ordering is verified by supersigil (cycle
    detection, topological sort).
  </Rationale>
</Decision>

<Decision id="freeform-ids">
  Unchanged from `document-format/mdx-adr`. IDs are freeform strings
  declared in front matter.

  <References refs="document-graph/req#req-1-1, config/req#req-4-1" />

  <Rationale>
    Freeform IDs are resistant to AI agent hallucination and decouple
    identity from filesystem layout, so reorganizing files is never a
    breaking change. An optional `id_pattern` in config lets teams
    enforce conventions via warnings.
  </Rationale>
</Decision>
```

## Examples

A requirements document in the new format:

````md
---
supersigil:
  id: auth/req
  type: requirements
title: "Authentication Requirements"
---

## Overview

The authentication system must support email/password login with
session tokens.

```supersigil-xml
<References refs="auth/design" />
```

## Acceptance Criteria

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="login-success">
    User can log in with valid email and password credentials.
  </Criterion>
  <Criterion id="login-failure">
    Invalid credentials produce a 401 response with no session token.
  </Criterion>
</AcceptanceCriteria>
```

Session tokens must expire after the configured TTL.

```supersigil-xml
<VerifiedBy strategy="tag" tag="login-success" />
<VerifiedBy strategy="tag" tag="login-failure" />
```
````

A document can still include ordinary Markdown code fences for prose,
design notes, or illustrative snippets. They are not interpreted as
structured supersigil content:

````md
## Notes

Use the current session token for follow-up API calls:

```sh
curl -H "Authorization: Bearer $TOKEN" https://api.example.test/me
```
````

## Consequences

**Gains.** Documents are standard Markdown files that render correctly
in any editor or documentation platform without special tooling.
Structured components are explicitly delimited, eliminating ambiguity
between prose and specification content. The parser pipeline simplifies:
standard Markdown parsing to locate fences, then XML subset parsing
within each fence. The `supersigil-{grammar}` naming scheme allows
future inner grammars (notably KDL) without format-level breaking
changes.

**Costs.** Each group of components requires fence delimiters
(` ```supersigil-xml ` / ` ``` `), adding verbosity. This is offset by
the elimination of MDX parsing complexity and the gain in editor
support.

**Lost: MDX ecosystem rendering.** The superseded format allowed specs
to render as React components in Astro, Docusaurus, and Next.js. The
new format loses this — `supersigil-xml` fences render as plain code
blocks in documentation systems. If publishable documentation output is
needed, a custom remark/rehype plugin would transform the fences. This
tradeoff is accepted: editor authoring experience is prioritized over
rendered documentation output.

**Rollout.** This is a flag-day change. Supersigil is unreleased with
no external users, so there is no dual-format transition period.
Existing specs, config defaults, parser, LSP, and VSCode extension will
be updated to the new format as part of implementation. Specifically,
the LSP specification (`lsp/req`), VSCode extension specification
(`vscode-extension/req`), parser pipeline specification
(`parser-pipeline/req`), config specification (`config/req`), and
authoring commands specification (`authoring-commands/req`) all contain
requirements that assume `.mdx` files and MDX parsing — these will be
revised to target `.md` files with `supersigil-xml` fences.
