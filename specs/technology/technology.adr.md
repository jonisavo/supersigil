---
supersigil:
  id: technology/adr
  type: adr
  status: accepted
title: "ADR: Technology Choice"
---

## Context

Supersigil needs to parse Markdown with XML-based component fences,
traverse large file trees, and distribute as a single artifact with no
runtime dependencies.

## Decision

```supersigil-xml
<Decision id="rust-single-binary" standalone="Project-level technology choice with no corresponding requirement">
  Supersigil is implemented in Rust for single-binary distribution, fast
  filesystem traversal, native Markdown parsing via the `markdown` crate,
  and XML parsing via `quick-xml`.
  Pluggability is handled via external process hooks (stdin/stdout JSON),
  avoiding the need for a plugin runtime.

  <Rationale>
    A single binary with no runtime dependencies simplifies installation
    and CI integration — `curl | tar` or a GitHub release asset, no
    package manager required. Rust's performance means verification of
    large workspaces completes in seconds. Native Markdown parsing via
    `markdown-rs` extracts fenced component blocks, while `quick-xml`
    parses the XML component structure within them — both are pure Rust,
    avoiding any Node.js runtime dependency. External process
    hooks for plugins keep the core binary lean while allowing ecosystem
    extensions in any language.
  </Rationale>

  <Alternative id="typescript-implementation" status="rejected">
    TypeScript would leverage its rich ecosystem for Markdown and XML
    processing but requires a JavaScript runtime, complicating
    distribution and CI setup. It also makes filesystem-heavy operations
    (scanning hundreds of spec files) slower and harder to parallelize.
  </Alternative>

  <Alternative id="go-implementation" status="rejected">
    Go produces single binaries and has good filesystem performance, but
    its Markdown and XML library ecosystem is less mature than Rust's.
    Achieving the same level of parsing fidelity would require more
    custom code or less well-maintained dependencies.
  </Alternative>
</Decision>
```

## Consequences

Distribution is a single binary per platform. The Rust ecosystem provides
the Markdown parser (`markdown-rs`), the XML parser (`quick-xml`), and
the proc-macro infrastructure for `#[verifies]`. The cost is a steeper
contribution barrier for non-Rust developers, mitigated by the external
process hook interface for plugins.
