---
supersigil:
  id: decision-components/adr
  type: adr
  status: accepted
title: "ADR: Structured Rationale in the Specification Graph"
---

```supersigil-xml
<References refs="decision-components/req" />
```

```supersigil-xml
<Decision id="structured-rationale">
  Supersigil captures architectural rationale as typed, referenceable
  components in the specification graph rather than as freeform prose in
  markdown files or wiki pages.

  <References refs="decision-components/req#req-1-1, decision-components/req#req-6-1, decision-components/req#req-6-2" />

  <Rationale>
    The existing DECISIONS.md file demonstrates the problem. It contains
    valuable reasoning — why references are unidirectional, why MDX was
    chosen over plain markdown, why IDs are freeform — but it is
    disconnected from the specs it explains. When a design doc changes,
    nothing flags the rationale as potentially stale. When an agent runs
    `supersigil context`, it sees criteria and implementations but not the
    reasoning behind them.

    Structured components solve this because they participate in the same
    graph that already tracks criteria, evidence, and dependencies. A
    Decision with a References edge to a design doc appears in context
    output. A Decision with TrackedFiles gets flagged by `affected`. The
    same infrastructure that keeps specs honest keeps rationale honest.
  </Rationale>

  <Alternative id="convention-only" status="rejected">
    A new `adr` document type with no new components. Authors write
    rationale in prose sections. This costs almost nothing to implement
    but adds no queryability — the graph cannot distinguish rationale from
    any other prose. Agents cannot find "the decisions that explain this
    design" without parsing markdown headings.
  </Alternative>

  <Alternative id="rationale-in-design-prose" status="rejected">
    Several existing design docs already contain "Key Design Decisions" and
    "Design Notes" sections. The problem is that these are invisible to the
    graph. They cannot be referenced, they do not appear in context output,
    and they are not flagged for staleness. They also cannot express
    cross-cutting decisions that span multiple features — the kind of
    reasoning that currently lives in DECISIONS.md.
  </Alternative>
</Decision>

<Decision id="graduated-verification">
  The feature includes configurable rules at different default severities:
  `incomplete_decision` (warning), `orphan_decision` (warning), and
  `missing_decision_coverage` (off).

  <References refs="decision-components/req#req-5-1, decision-components/req#req-5-2, decision-components/req#req-5-3" />

  <Rationale>
    Teams adopt rationale tracking at different speeds. Forcing full
    decision coverage from day one would create friction that discourages
    adoption. The graduated model lets teams start with quality checks on
    decisions they do write (warnings for incomplete or orphaned decisions),
    then opt into coverage checks (`missing_decision_coverage`) when the
    practice is established.

    This mirrors how the existing verification rules work: `isolated_document`
    defaults to `off` and teams enable it when ready. The same pattern
    applied to decisions means zero cost for teams that do not want ADRs
    and incremental value for teams that do.
  </Rationale>

  <Alternative id="all-rules-default-warning" status="rejected">
    Turning on `missing_decision_coverage` by default would produce warnings
    for every design document in every existing project. That penalizes
    projects that have not adopted decision tracking and creates noise that
    trains people to ignore warnings.
  </Alternative>

  <Alternative id="no-verification-rules" status="deferred">
    Pure traceability with no verification would still be useful — decisions
    in the graph are queryable and linkable. But it misses the nudge that
    makes rationale a habit rather than an afterthought. The graduated
    approach delivers traceability immediately and nudges incrementally.
  </Alternative>
</Decision>

<Decision id="components-in-any-doc">
  Decision, Rationale, and Alternative components can appear in any
  document type. The `adr` type is a convention, not a constraint.

  <References refs="decision-components/req#req-1-2" />

  <Rationale>
    This follows the principle that components carry semantics, not document
    types. A Decision in a design doc is just as valid as one in a dedicated
    ADR doc. Restricting placement would add enforcement complexity without
    proportional benefit — the same reasoning that keeps Criterion legal in
    any document type today.

    The practical implication is that small features might embed a single
    Decision directly in their design doc, while cross-cutting architectural
    choices get dedicated ADR documents. Both patterns are first-class.
  </Rationale>

  <Alternative id="decisions-only-in-adr" status="rejected">
    This would force a separate document for every decision, even trivial
    ones. For a feature with one design choice worth recording, requiring
    a second document creates friction that discourages recording the
    decision at all. The component-anywhere model matches how teams
    actually work — rationale lives close to the thing it explains.
  </Alternative>
</Decision>

<Decision id="singular-rationale">
  Each Decision component may contain at most one Rationale child.

  <References refs="decision-components/req#req-2-3" />

  <Rationale>
    A decision has one justification. If the rationale has multiple facets,
    they belong in the same Rationale body as a cohesive argument. Multiple
    Rationale children would invite splitting the justification into
    fragments that are harder to read as a whole.

    This mirrors the Expected/Example pattern: one Expected per Example,
    one Rationale per Decision. The constraint keeps the component model
    simple and the authoring experience predictable.
  </Rationale>

  <Alternative id="multiple-rationale" status="deferred">
    Some decision records separate "justification" from "tradeoff
    acknowledgment." If this proves to be a real need, a future iteration
    could relax the singular constraint or introduce a Tradeoff component.
    The constraint is easy to relax later; adding it later after allowing
    multiple would be a breaking change.
  </Alternative>
</Decision>

<Decision id="referenceable-alternatives">
  Alternative components have an `id` attribute and can be referenced
  from other documents using fragment syntax.

  <References refs="decision-components/req#req-3-6" />

  <Rationale>
    Decisions in different features sometimes relate to each other through
    their alternatives. "We chose X because Alternative Y in decision Z
    was rejected" is a real pattern. Making alternatives referenceable
    enables these cross-links without requiring the author to reference the
    entire parent decision when only a specific rejected option is relevant.

    The cost is one additional referenceable component type, which is
    minimal given that the graph already indexes referenceable components
    generically.
  </Rationale>

  <Alternative id="non-referenceable-alternatives" status="rejected">
    Simpler component model, but loses the ability to create precise
    cross-links between related decisions. Authors would have to reference
    the parent Decision and describe which alternative they mean in prose,
    which is exactly the kind of imprecision that structured components
    are meant to eliminate.
  </Alternative>
</Decision>
```
