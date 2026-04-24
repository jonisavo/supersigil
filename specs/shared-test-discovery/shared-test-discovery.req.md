---
supersigil:
  id: shared-test-discovery/req
  type: requirements
  status: implemented
title: "Shared Test Discovery"
---

## Introduction

This feature defines how Supersigil resolves configured test-file globs into
the shared test-file baseline used by tag scanning and ecosystem plugin
discovery.

Today the shared resolver expands test globs literally from disk, so broad
patterns such as `packages/**/*.test.ts` can traverse ignored directories like
`node_modules/` or `dist/`. That behavior is noisy for real workspaces and can
surface spurious diagnostics from files the repository already treats as
ignored.

In scope for this feature:

- a workspace-level config surface controlling shared test discovery policy
- ignore-aware shared test-file resolution as the default behavior
- an explicit opt-out for raw glob expansion
- preserving current top-level vs multi-project test-scope rules
- leaving spec/document discovery unchanged

Out of scope for this feature:

- changing `paths` or `[projects.*].paths` discovery semantics
- changing criterion-nested `VerifiedBy strategy="file-glob"` path resolution
- adding plugin-specific ignore settings
- changing plugin-owned widening beyond the shared test-file baseline
- adding an explicit exclude-glob list to `supersigil.toml`

## Definitions

- **Shared_Test_File_Baseline**: The resolved file list produced from
  configured `tests` globs before tag scanning and before plugins receive the
  shared baseline for their own discovery-input planning.
- **Test_Discovery_Policy**: The workspace-level configuration that controls
  how the Shared_Test_File_Baseline is resolved.
- **Standard_Ignore_Mode**: The policy mode where shared test discovery uses
  the `ignore` crate's standard behavior, including repository ignore files,
  `.ignore`, Git exclude files, and hidden-file filtering.
- **Raw_Glob_Mode**: The policy mode where shared test discovery uses literal
  glob expansion without ignore-aware filtering.
- **Discovery_Scope**: The active set of `tests` globs selected from top-level
  `tests` or named `[projects.*].tests` based on the current workspace mode and
  optional project filter.

## Requirement 1: Workspace Test Discovery Policy

As a workspace maintainer, I want shared test discovery configured explicitly,
so that broad test globs have one predictable policy across tag scanning and
shared-baseline plugin discovery.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE config model SHALL expose a workspace-level `test_discovery.ignore`
    setting whose allowed values are `"standard"` and `"off"`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/src/config.rs, crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-1-2">
    IF `test_discovery.ignore` is omitted, THEN shared test discovery SHALL
    default to `"standard"`.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs, crates/supersigil-core/tests/config_property_tests.rs" />
  </Criterion>
  <Criterion id="req-1-3">
    IF `test_discovery.ignore` contains an unknown value, THEN config loading
    SHALL fail before verification or plugin assembly begins.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs" />
  </Criterion>
  <Criterion id="req-1-4">
    THE Test_Discovery_Policy SHALL apply uniformly to top-level `tests` and
    named `[projects.*].tests`. This feature SHALL NOT introduce per-plugin or
    per-project override settings.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-core/tests/config_unit_tests.rs, crates/supersigil-verify/src/lib.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Ignore-Aware Shared Resolution

As an operator, I want shared test discovery to respect standard ignore rules,
so that broad test globs do not traverse ignored build output or vendored
dependency trees.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    WHEN `test_discovery.ignore = "standard"`, THE Shared_Test_File_Baseline
    SHALL be resolved using Standard_Ignore_Mode before matching configured
    test globs.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/lib.rs" />
  </Criterion>
  <Criterion id="req-2-2">
    IN Standard_Ignore_Mode, files and directories excluded by the active
    ignore rules SHALL NOT appear in the Shared_Test_File_Baseline.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/lib.rs" />
  </Criterion>
  <Criterion id="req-2-3">
    IN Standard_Ignore_Mode, ignored files SHALL NOT participate in
    tag scanning and SHALL NOT appear in the Shared_Test_File_Baseline handed
    to ecosystem plugins for discovery-input planning.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/lib.rs, crates/supersigil-js/src/discover.rs" />
  </Criterion>
  <Criterion id="req-2-4">
    FOR any Discovery_Scope, the Shared_Test_File_Baseline SHALL remain sorted
    and deduplicated after resolution.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/lib.rs" />
  </Criterion>
  <Criterion id="req-2-5">
    THE Test_Discovery_Policy SHALL change only how files are resolved from the
    active Discovery_Scope. It SHALL NOT change which top-level or project test
    globs are selected by the current workspace mode or project filter.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/lib.rs" />
  </Criterion>
 </AcceptanceCriteria>
```

## Requirement 3: Raw Glob Opt-Out

As a maintainer, I want an explicit raw mode available, so that repositories
can intentionally bypass ignore-aware filtering when needed.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    WHEN `test_discovery.ignore = "off"`, THE Shared_Test_File_Baseline SHALL
    be resolved using Raw_Glob_Mode rather than Standard_Ignore_Mode.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/lib.rs" />
  </Criterion>
  <Criterion id="req-3-2">
    IN Raw_Glob_Mode, ignored files that match the active Discovery_Scope MAY
    appear in the Shared_Test_File_Baseline.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/lib.rs" />
  </Criterion>
  <Criterion id="req-3-3">
    Raw_Glob_Mode SHALL preserve the same sorted and deduplicated output shape
    as Standard_Ignore_Mode.
    <VerifiedBy strategy="file-glob" paths="crates/supersigil-verify/src/lib.rs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Discovery Boundary

As a maintainer, I want ignore-aware test discovery kept separate from spec
discovery, so that repository ignore files cannot silently remove authoritative
documents from the workspace graph.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    THE Test_Discovery_Policy SHALL apply only to top-level `tests` and named
    `[projects.*].tests`.
    <VerifiedBy strategy="file-glob" paths="README.md, specs/shared-test-discovery/shared-test-discovery.design.md, crates/supersigil-verify/src/lib.rs" />
  </Criterion>
  <Criterion id="req-4-2">
    Spec/document discovery from top-level `paths` and named
    `[projects.*].paths` SHALL continue using raw glob expansion semantics.
    <VerifiedBy strategy="file-glob" paths="README.md, crates/supersigil-lsp/src/state/indexing.rs, crates/supersigil-core/src/glob_util.rs" />
  </Criterion>
  <Criterion id="req-4-3">
    THIS feature SHALL NOT add a separate exclude-glob configuration surface to
    `supersigil.toml`.
    <VerifiedBy strategy="file-glob" paths="README.md, docs/research/polish-audit.md, crates/supersigil-core/src/config.rs" />
  </Criterion>
  <Criterion id="req-4-4">
    Criterion-nested `&lt;VerifiedBy strategy=\"file-glob\"&gt;` resolution SHALL
    continue expanding its declared `paths` relative to the project root and is
    not changed by this feature.
    <VerifiedBy strategy="file-glob" paths="README.md, crates/supersigil-verify/src/rules/tests_rule.rs" />
  </Criterion>
</AcceptanceCriteria>
```
