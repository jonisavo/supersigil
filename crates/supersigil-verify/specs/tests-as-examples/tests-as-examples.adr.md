---
supersigil:
  id: tests-as-examples/adr
  type: adr
  status: accepted
title: "ADR: Tests Are Examples"
---

```supersigil-xml
<References refs="ecosystem-plugins/adr, verification-engine/req" />
```

## Context

Supersigil originally had an "executable examples" feature that allowed
authors to embed runnable code samples directly in spec documents using
`<Example>` and `<Expected>` components. The idea was inspired by Rust
doctests: documentation that proves itself by executing.

The feature included a full test-runner layer: built-in runners for
shell, cargo-test, and HTTP; output matchers (text, JSON with wildcards,
regex, snapshot); snapshot rewriting; progress display; and per-example
timeout and parallelism controls.

In practice, the feature had structural problems:

- **Authoring friction.** Writing examples inside supersigil-xml code
  blocks within Markdown was cumbersome. The `supersigil-ref` binding
  syntax (linking external code fences to Example components) added
  indirection without improving ergonomics.
- **Slow execution broke the feedback loop.** Example execution took
  seconds (6+ in the supersigil repo), which meant the LSP could not
  run them. This created an inconsistency: editor diagnostics and CLI
  verification reported different results.
- **Cascading failures.** A single failing example emitted findings
  against every criterion it targeted, drowning out the structural
  findings that actually mattered.
- **Disproportionate complexity.** The feature touched nearly every
  crate (core, parser, verify, evidence, CLI, LSP) and accounted for
  thousands of lines of runner/matcher/snapshot machinery. This
  complexity was at odds with supersigil's goal of being a focused
  spec verification tool, not a test framework.

Meanwhile, the ecosystem plugin system (Rust `#[verifies(...)]`,
JS/TS `verifies` annotations) already provided a better path: tests
live in the test framework where they belong, and supersigil discovers
the mapping to criteria automatically.

## Decision

```supersigil-xml
<Decision id="tests-are-examples" standalone="Cross-cutting decision about supersigil's verification philosophy; not scoped to a single feature's criteria.">
  Tests are examples. Supersigil does not execute code embedded in specs.
  Verification evidence comes exclusively from ecosystem plugins and
  explicit VerifiedBy tags. Tests that link to criteria via plugin
  annotations serve as both proof and documentation of the specified
  behavior.

  <Rationale>
    The executable examples feature tried to make specs self-proving, but
    this pulled supersigil toward being a test runner — a different product
    with different tradeoffs. Ecosystem plugins achieve the same evidence
    goal without supersigil owning execution: tests run in their native
    framework (faster, better tooling, better debugging), and supersigil
    maps results to criteria via static analysis. The spec browser can
    render linked test source alongside criteria, preserving the
    "documentation by example" value without execution machinery.
  </Rationale>

  <Alternative id="executable-examples" status="rejected">
    Keep executable examples with Example and Expected components, runners,
    matchers, and snapshot support. Rejected because the authoring friction,
    slow execution, cascading failures, and codebase complexity outweighed
    the elegance of self-proving specs. The feature was removed before the
    first public release.
  </Alternative>

  <Alternative id="lightweight-examples" status="rejected">
    Strip executable examples down to shell one-liners with exit-code
    checking only (no output matching, no HTTP runner, no cargo-test
    scaffolding). Rejected because even a minimal version still requires
    supersigil to own execution, creates a second evidence path alongside
    plugins, and does not solve the LSP speed problem.
  </Alternative>

  <Alternative id="non-executable-examples" status="deferred">
    Keep Example components in specs as illustrative documentation (not
    executed) with a lint rule checking that each example has a
    corresponding test. Deferred because plain Markdown code fences
    already serve the illustrative purpose without component overhead,
    and the spec browser can pull in linked test source directly.
  </Alternative>
</Decision>
```

## Consequences

**Positive:**
- Supersigil's codebase is simpler (~12,000 lines removed) and its
  boundary is clearer: spec authoring + evidence mapping, not test
  execution.
- The feedback loop is unified: the LSP and CLI run the same pipeline
  with no skipped phases.
- Evidence comes from real tests that run in real frameworks, avoiding
  the "mocked test passed but production broke" class of problems.

**Negative:**
- Specs can no longer self-prove. Authors must write separate tests and
  link them to criteria, which is more steps than embedding an example.
- The "documentation that executes" narrative is weaker. Illustrative
  code in specs is now just Markdown, not verified.

**Future directions:**
- Convention-based evidence mapping (e.g., `test_auth_session_expiry`
  auto-maps to `auth/req#session-expiry`) to reduce annotation burden.
- Spec browser rendering of linked test source alongside criteria,
  making tests visible as examples without execution machinery.
- Doctest evidence parsing (consuming `cargo test --doc` output) to
  give Rust doctests first-class evidence status.
