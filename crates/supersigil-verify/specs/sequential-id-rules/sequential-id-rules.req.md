---
supersigil:
  id: sequential-id-rules/req
  type: requirements
  status: implemented
title: "Sequential ID Rules"
---

## Introduction

Supersigil documents use numeric-sequence IDs for criteria and tasks (e.g.
`req-1-1`, `task-3`). When authors add, remove, or reorder entries, the
declaration order can drift from the numeric order, or gaps can appear in the
sequence. These inconsistencies make documents harder to scan and can mask
accidental deletions.

This feature introduces two verify rules that detect declaration-order
violations and sequence gaps in components with numeric-sequence IDs.

**In scope:**

- Detecting out-of-order declaration of sequentially-numbered Criterion and
  Task components within a single document.
- Detecting gaps in contiguous numeric sequences within a prefix group.
- Parsing numeric-sequence IDs of the form `prefix-N` and `prefix-N-M`.

**Out of scope:**

- Descriptive suffixes on IDs (e.g. `task-1-login-success`).
- Dependency-aware ordering (whether task declaration order matches the
  dependency graph).
- Changes to the lint command (remains parse-errors only).
- Changes to the existing `id_pattern` config regex or ID validation.

## Definitions

- **Sequential_ID**: A component ID matching the pattern `prefix-N` or
  `prefix-N-M`, where `prefix` is one or more non-numeric dash-separated
  segments and `N`, `M` are unsigned integers. Only one or two numeric
  segments are recognized; IDs with three or more numeric segments (e.g.
  `req-1-2-3`) are non-sequential. IDs with no prefix (e.g. `123`) are also
  non-sequential. Examples: `req-1-1`, `task-3`.
- **Prefix_Group**: The set of sibling components within one document that
  share the same prefix. Ordering and gap checks operate within each
  prefix group independently.
- **Numeric_Key**: The tuple of unsigned integers extracted from a
  Sequential_ID. For `req-1-2` the key is `(1, 2)`. For `task-3` the key is
  `(3,)`. Keys are compared using lexicographic tuple ordering.

## Requirement 1: Sequential ID Recognition

As a rule author, I want a shared parsing function that extracts prefix and
numeric key from component IDs, so that both rules operate on the same ID
classification.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE parser SHALL accept an ID string and return either a parsed
    Sequential_ID (prefix string and Numeric_Key tuple) or a non-sequential
    classification.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/rules/structural.rs" />
  </Criterion>
  <Criterion id="req-1-2">
    An ID SHALL be classified as sequential only when every segment after the
    prefix consists entirely of ASCII digits. IDs with non-numeric segments
    after the first numeric segment (e.g. `req-1-2-foo`) SHALL be classified
    as non-sequential.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/rules/structural.rs" />
  </Criterion>
  <Criterion id="req-1-3">
    IDs classified as non-sequential SHALL be silently skipped by both rules
    without emitting any finding.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/rules/structural.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Declaration Order Rule

As a spec author, I want verification to warn me when sequentially-numbered
components appear out of numeric order in the source, so that I can keep
documents scannable.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    THE `SequentialIdOrder` rule SHALL group sibling referenceable components
    (Criterion, Task) within each document by Prefix_Group, and SHALL check
    that Numeric_Keys appear in ascending lexicographic tuple order by
    declaration position.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/rules/structural.rs" />
  </Criterion>
  <Criterion id="req-2-2">
    WHEN a component's Numeric_Key is less than or equal to the previous
    component's Numeric_Key in the same Prefix_Group, THE rule SHALL emit a
    finding referencing both the out-of-order component and the component it
    should follow.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/rules/structural.rs" />
  </Criterion>
  <Criterion id="req-2-3">
    THE `SequentialIdOrder` rule SHALL default to `warning` severity and SHALL
    be overridable via the `[verify.rules]` config section.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/report.rs, crates/supersigil-verify/src/lib.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Sequence Gap Rule

As a spec author, I want verification to warn me when a numbered sequence has
missing entries, so that I can catch accidental deletions.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    THE `SequentialIdGap` rule SHALL operate within each Prefix_Group. For
    single-level IDs (`prefix-N`), the rule SHALL check that N values form the
    contiguous sequence `1..=max`. For two-level IDs (`prefix-N-M`), the rule
    SHALL check that the first-level N values form the contiguous sequence
    `1..=max`, and that within each first-level group N, the M values form
    the contiguous sequence `1..=max`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/rules/structural.rs" />
  </Criterion>
  <Criterion id="req-3-2">
    WHEN a gap is detected, THE rule SHALL emit a finding that names the
    missing ID(s). WHEN the gap has both a predecessor and successor, the
    finding SHALL reference both. WHEN the gap is at the start of the
    sequence (e.g. sequence begins at 2 instead of 1), the finding SHALL
    reference only the first present ID as context.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/rules/structural.rs" />
  </Criterion>
  <Criterion id="req-3-3">
    THE `SequentialIdGap` rule SHALL default to `warning` severity and SHALL
    be overridable via the `[verify.rules]` config section.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/report.rs, crates/supersigil-verify/src/lib.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Integration with Verification Pipeline

As a workspace maintainer, I want the new rules to participate in the existing
verification pipeline without special handling, so that severity overrides,
project filtering, and draft gating work automatically.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    THE two new rule names SHALL be registered in the `RuleName` enum and SHALL
    be recognized by the `[verify.rules]` config section for severity
    overrides.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/report.rs, crates/supersigil-core/src/config.rs" />
  </Criterion>
  <Criterion id="req-4-2">
    THE rules SHALL operate on the parsed document set (`&amp;[&amp;SpecDocument]`)
    without requiring the DocumentGraph or ArtifactGraph.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/rules/structural.rs" />
  </Criterion>
  <Criterion id="req-4-3">
    THE rules SHALL be invoked as part of the structural rule group in the
    verification pipeline.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/lib.rs" />
  </Criterion>
</AcceptanceCriteria>
```
