---
supersigil:
  id: release-targets/req
  type: requirements
  status: implemented
title: "Release Targets"
---

## Introduction

The current release-target layer covers IntelliJ, VS Code, and the npm
packages, but Rust crates still bump and publish on every tagged release.
That leaves two gaps in the workflow. First, the VS Code extension still lacks
its own target changelog even though the IntelliJ plugin already relies on one.
Second, crate publishing is still unconditional and is not sequenced ahead of
the editor publishes that may depend on the latest crate release.

This next pass keeps the selective-release model, but broadens it in three
ways:

1. Add an aggregate `crates` target so any qualifying crate change bumps and
   publishes all workspace crates together, while crate-only releases can still
   skip editor publishes.
2. Generate target-local changelogs for both editor extensions, with CI commits
   omitted entirely from those editor changelogs.
3. Keep editor/server compatibility as a separate concern from release-target
   detection and publish ordering.

### Scope

- **In scope:** a checked-in release-target registry, impact detection from git
  history, aggregate crate release detection, selective target version bumps,
  target-specific editor changelog generation, GitHub Actions publish gating,
  and publish ordering so crates are released before editor extensions when the
  crate target publishes.
- **Out of scope:** automatic compatibility negotiation between editors and the
  server, semver range handling for compatibility, and publishing the IntelliJ
  plugin before Marketplace acceptance.

## Definitions

- **Release_Target**: A releasable artifact whose version, changelog, or
  publish step can be managed independently from other artifacts within the
  same tagged repository release.
- **Aggregate_Target**: A Release_Target whose impact drives one coordinated
  version bump and publish decision for multiple artifacts, such as all
  workspace crates.
- **Impact_Paths**: The set of source files and build inputs that determine
  whether a Release_Target's shipped artifact changed in a release range.
- **Impact_Exclusions**: Optional path filters that remove known non-shipping
  files, such as tests or snapshots, from a target's impact classification.
- **Release_Range**: The commit range between the previous reachable release
  tag and the commit being prepared or published.
- **Target_Changelog**: A changelog file rendered from only the commits that
  affect one Release_Target.
- **Disabled_Target**: A Release_Target whose metadata is prepared and
  evaluated in CI even though its publish job is intentionally inactive.

## Requirement 1: Release Target Registry

As a maintainer, I want one checked-in release target registry, so that
local release prep and GitHub Actions evaluate the same target boundaries.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    THE repository SHALL define one checked-in registry of release targets.
    <VerifiedBy strategy="file-glob" paths="release-targets.json" />
  </Criterion>
  <Criterion id="req-1-2">
    EACH release target definition SHALL include a stable target ID, the
    target's version-edit metadata, and the target's Impact_Paths.
    <VerifiedBy strategy="file-glob" paths="release-targets.json" />
  </Criterion>
  <Criterion id="req-1-3">
    Impact_Paths MAY include shared sources outside the target's own
    directory when those sources can change the shipped artifact.
    <VerifiedBy strategy="file-glob" paths="release-targets.json" />
  </Criterion>
  <Criterion id="req-1-4">
    Impact_Paths SHALL describe shipped artifact inputs and SHALL NOT rely on
    version-file or changelog-file outputs to classify a target as impacted.
    <VerifiedBy strategy="file-glob" paths="release-targets.json, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-1-5">
    Adding a new target that uses an existing version-editing or changelog
    strategy SHALL require registry data changes rather than a new
    target-specific detection code path.
    <VerifiedBy strategy="file-glob" paths="release-targets.json, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-1-6">
    THE registry SHALL support an Aggregate_Target for the Cargo workspace so
    one publish decision can represent all crates.io releases.
    <VerifiedBy strategy="file-glob" paths="release-targets.json, scripts/release-targets/index.mjs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 2: Impact Detection

As a maintainer, I want release tooling to compute impacted targets from git
history, so that release prep and publishing only touch changed artifacts.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-2-1">
    Release tooling SHALL compute impacted targets for a Release_Range by
    comparing changed files against each target's Impact_Paths.
    <VerifiedBy strategy="file-glob" paths="scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-2-2">
    Impact detection SHALL produce machine-readable per-target outputs that
    can be consumed by local release prep and GitHub Actions.
    <VerifiedBy strategy="file-glob" paths="scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-2-3">
    Target-specific changelog generation SHALL exclude release-preparation
    commits such as `chore(release): prepare vX.Y.Z` from rendered notes.
    <VerifiedBy strategy="file-glob" paths="cliff.editor.toml, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-2-4">
    Impact detection SHALL support Impact_Exclusions so shared source trees can
    be counted as target inputs without treating test-only or snapshot-only
    changes as shipped artifact changes.
    <VerifiedBy strategy="file-glob" paths="release-targets.json, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-2-5">
    Automated tests for target-specific changelog generation SHALL NOT require
    a globally installed `git-cliff` binary.
  </Criterion>
  <Criterion id="req-2-6">
    Editor target changelog generation SHALL omit `ci` commits entirely rather
    than rendering them under a `CI/CD` section or any fallback group.
    <VerifiedBy strategy="file-glob" paths="cliff.editor.toml, scripts/release-targets/index.mjs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 3: Selective Target Preparation

As a maintainer, I want release prep to update only impacted targets, so that
unchanged artifacts keep their previous version and changelog state.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-3-1">
    Repository release prep SHALL continue to generate the root
    `CHANGELOG.md` for the full tagged release.
    <VerifiedBy strategy="file-glob" paths="mise.toml" />
  </Criterion>
  <Criterion id="req-3-2">
    Repository release prep SHALL bump a target's version to the tagged
    release version only when that target is impacted.
    <VerifiedBy strategy="file-glob" paths="mise.toml, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-3-3">
    Repository release prep SHALL regenerate a target's Target_Changelog only
    when that target is impacted.
    <VerifiedBy strategy="file-glob" paths="mise.toml, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-3-4">
    WHEN a target is not impacted, release prep SHALL leave that target's
    version file and changelog file unchanged.
    <VerifiedBy strategy="file-glob" paths="scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-3-5">
    WHEN the `crates` Aggregate_Target is impacted, repository release prep
    SHALL bump all workspace crate versions and the workspace `=version` pins
    as one coordinated unit.
    <VerifiedBy strategy="file-glob" paths="mise.toml, Cargo.toml, crates/**/Cargo.toml, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-3-6">
    WHEN the `crates` Aggregate_Target is not impacted, repository release prep
    SHALL leave the workspace crate versions and workspace `=version` pins
    unchanged.
    <VerifiedBy strategy="file-glob" paths="mise.toml, Cargo.toml, crates/**/Cargo.toml, scripts/release-targets/index.mjs" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 4: Selective Publishing

As a maintainer, I want publish jobs to key off impacted targets, so that CI
only publishes artifacts whose shipped contents changed.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-4-1">
    GitHub Actions release automation SHALL expose per-target publish booleans
    derived from impact detection, including the aggregate `crates` target.
    <VerifiedBy strategy="file-glob" paths=".github/workflows/release.yml, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-4-2">
    Publish jobs for targets with dedicated release workflows, and binary
    distribution jobs that reuse the aggregate `crates` decision, SHALL run
    only when their relevant publish boolean is true.
    <VerifiedBy strategy="file-glob" paths=".github/workflows/release.yml" />
  </Criterion>
  <Criterion id="req-4-3">
    Release automation SHALL compute metadata for Disabled_Targets even while
    their publish job remains intentionally disabled.
    <VerifiedBy strategy="file-glob" paths=".github/workflows/release.yml, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-4-4">
    Manually triggered publish workflows MAY remain available as an explicit
    break-glass path for recovery or republish scenarios, even when they bypass
    impact detection used by tagged release automation.
    <VerifiedBy strategy="file-glob" paths=".github/workflows/publish-vscode.yml, .github/workflows/publish-intellij.yml, .github/workflows/publish-crates.yml" />
  </Criterion>
  <Criterion id="req-4-5">
    WHEN the `crates` Aggregate_Target is being published, the VS Code and
    IntelliJ publish jobs SHALL wait for crate publishing to finish before
    starting.
    <VerifiedBy strategy="file-glob" paths=".github/workflows/release.yml" />
  </Criterion>
  <Criterion id="req-4-6">
    WHEN the `crates` Aggregate_Target is not impacted, the crates.io publish
    workflow SHALL be skipped rather than running as a no-op publish attempt.
    <VerifiedBy strategy="file-glob" paths=".github/workflows/release.yml" />
  </Criterion>
</AcceptanceCriteria>
```

## Requirement 5: Editor Release Metadata

As a maintainer, I want the editor extensions to publish target-specific change
notes, so that Marketplace metadata reflects only editor changes.

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="req-5-1">
    WHEN the VS Code target is impacted, release prep SHALL generate
    `editors/vscode/CHANGELOG.md` from only the commits that affect the
    VS Code target, including shared explorer and preview sources that change
    the packaged extension.
    <VerifiedBy strategy="file-glob" paths="release-targets.json, cliff.editor.toml, scripts/release-targets/index.mjs, editors/vscode/CHANGELOG.md" />
  </Criterion>
  <Criterion id="req-5-2">
    THE VS Code target changelog SHALL live at `editors/vscode/CHANGELOG.md`,
    the extension root.
    <VerifiedBy strategy="file-glob" paths="editors/vscode/CHANGELOG.md" />
  </Criterion>
  <Criterion id="req-5-3">
    WHEN the IntelliJ target is impacted, release prep SHALL generate
    `editors/intellij/CHANGELOG.md` from only the commits that affect the
    IntelliJ target, including shared embedded asset sources that change the
    packaged plugin.
    <VerifiedBy strategy="file-glob" paths="release-targets.json, cliff.editor.toml, scripts/release-targets/index.mjs, editors/intellij/CHANGELOG.md" />
  </Criterion>
  <Criterion id="req-5-4">
    THE IntelliJ Gradle build SHALL render JetBrains `changeNotes` from the
    generated `editors/intellij/CHANGELOG.md`.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/build.gradle.kts" />
  </Criterion>
  <Criterion id="req-5-5">
    WHEN an editor target is not impacted, release prep SHALL NOT bump its
    version file and SHALL NOT rewrite its changelog file.
    <VerifiedBy strategy="file-glob" paths="mise.toml, scripts/release-targets/index.mjs" />
  </Criterion>
  <Criterion id="req-5-6">
    WHEN the IntelliJ Gradle build cannot find a changelog section matching
    `pluginVersion`, it SHALL fail rather than render fallback change notes
    such as `Unreleased`.
    <VerifiedBy strategy="file-glob" paths="editors/intellij/build.gradle.kts" />
  </Criterion>
</AcceptanceCriteria>
```
