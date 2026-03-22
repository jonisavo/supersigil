---
supersigil:
  id: ecosystem-plugins/adr
  type: adr
  status: accepted
title: "ADR: Test Discovery Strategy"
---

```supersigil-xml
<References refs="ecosystem-plugins/req" />
```

## Context

Supersigil needs to discover which tests verify which criteria. This
must work across languages without requiring supersigil to run tests
itself.

## Decision

```supersigil-xml
<Decision id="test-discovery-strategy">
  The v1 test mapping strategy is explicit file globs via `&lt;VerifiedBy&gt;`.
  Tag scanning uses a hardcoded format (`supersigil: {tag}`) that is not
  configurable. For language-native test discovery, supersigil uses
  ecosystem plugins. Test execution and pass/fail reporting is handled by
  consuming existing test result formats (JUnit XML), not by running
  tests.

  <References refs="ecosystem-plugins/req#req-4-1, verification-engine/req#req-4-2, verification-engine/req#req-4-3" />

  <Rationale>
    A single universal tag convention avoids per-project bikeshedding and
    makes tags greppable across any codebase. Language-native discovery
    (AST-level, not comment-level) requires ecosystem-specific knowledge:
    the built-in Rust plugin uses `syn` to find annotated test items
    and understands proptest. Future plugins extend this to other
    languages. Supersigil is a verification tool, not a test runner, so
    it consumes results rather than producing them.
  </Rationale>

  <Alternative id="configurable-tag-format" status="rejected">
    Allowing per-project tag formats would require configuring and
    documenting the format in every project. A single convention is
    simpler and makes cross-project tooling (grep, CI scripts) work
    without configuration.
  </Alternative>
</Decision>
```

## Consequences

Test discovery is language-agnostic at the base level (file globs + tag
comments) and language-native at the plugin level (AST scanning). Adding
support for a new language means writing an ecosystem plugin, not
changing the core verification engine.
