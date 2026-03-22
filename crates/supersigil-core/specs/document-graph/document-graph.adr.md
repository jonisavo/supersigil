---
supersigil:
  id: document-graph/adr
  type: adr
  status: accepted
title: "ADR: Unidirectional References in the Document Graph"
---

```supersigil-xml
<References refs="document-graph/req" />
```

## Context

Specifications form a directed graph: requirements are implemented by
designs, criteria are verified by tests, documents depend on other
documents. The question is whether links should be declared on one end
(forward only) or both ends (bidirectional).

## Decision

```supersigil-xml
<Decision id="unidirectional-references">
  Design docs point at requirements (via `&lt;Implements&gt;`), not the reverse.
  `&lt;VerifiedBy&gt;` links criteria to tests, not the reverse. Supersigil
  computes reverse mappings from the forward refs.

  <References refs="document-graph/req#req-2-1, document-graph/req#req-4-1" />

  <Rationale>
    Bidirectional references create a synchronization burden. Adding a
    new design doc should never require editing the requirement it
    implements. Unidirectional refs make the more abstract artifact (the
    requirement) stable while the more concrete artifacts (designs, tests)
    evolve around it. Reverse mappings are derived automatically by the
    graph, so queryability is not lost.
  </Rationale>

  <Alternative id="bidirectional-references" status="rejected">
    Requiring both ends to declare the link would catch orphans earlier
    but creates a maintenance burden — every new design doc or test
    requires editing the upstream document. This friction discourages
    linking, which defeats the purpose of traceability.
  </Alternative>
</Decision>
```

## Consequences

Authors only edit the document they are working on. Reverse queries
("which designs implement this requirement?") are derived at graph
construction time. The tradeoff is that orphan detection requires a full
graph scan rather than local validation.
