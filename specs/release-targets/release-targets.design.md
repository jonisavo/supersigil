---
supersigil:
  id: release-targets/design
  type: design
  status: approved
title: "Release Targets"
---

```supersigil-xml
<Implements refs="release-targets/req" />
<TrackedFiles paths="release-targets.json, cliff.intellij.toml, scripts/release-targets/index.mjs, scripts/release-targets/package.json, scripts/release-targets/release-targets.test.js, mise.toml, .github/workflows/release.yml, .github/workflows/publish-vscode.yml, .github/workflows/publish-intellij.yml, editors/intellij/build.gradle.kts, editors/intellij/CHANGELOG.md, editors/intellij/gradle.properties, editors/vscode/specs/version-mismatch/version-mismatch.design.md" />
```

## Overview

Introduce a small release-target layer on top of the existing tagged release
flow. The repository continues to cut one tagged release version, but editor
and package artifacts no longer have to adopt that version unless their shipped
artifact changed in the release range.

The implementation has three pieces:

1. A checked-in target registry describing which files make each target
   "changed" and how to edit its version/changelog metadata.
2. A repo-local helper script that can detect impacted targets and prepare
   target metadata for a new tagged release.
3. Workflow wiring so `mise release` and GitHub Actions consume the same
   impact results.

## Architecture

```
repo tag version (vX.Y.Z)
        │
        ├─ root release prep
        │   ├─ bump Rust / root release artifacts as today
        │   └─ generate root CHANGELOG.md
        │
        └─ release target layer
            ├─ release-targets.json
            │    └─ target definitions
            │         ├─ intellij
            │         ├─ vscode
            │         ├─ npm-vitest
            │         └─ npm-eslint-plugin
            │
            └─ scripts/release-targets/
                 ├─ index.mjs             -> detect + prepare commands
                 ├─ package.json
                 └─ release-targets.test.js
```

### Target Registry

Add a checked-in JSON file at the repository root, `release-targets.json`.
JSON keeps the helper script dependency-free and easy to consume from Node in
CI without introducing a TOML parser or custom YAML handling.

Each target entry contains:

- `id`: stable target ID
- `enabled`: whether publish gating should ever allow the target to publish
- `versionFile`: path of the target-local version file
- `versionKind`: edit strategy such as `package-json` or `gradle-property`
- `versionKey`: file-local key to update when the target is impacted
- `changelogFile`: optional target changelog output path
- `cliffConfig`: optional git-cliff config path for target notes
- `impactPaths`: source inputs that affect the target's packaged artifact
- `excludePaths`: optional non-shipping paths ignored during impact detection
- `publishJob`: optional GitHub workflow job key used for CI outputs

Initial targets:

- `intellij`
- `vscode`
- `npm-vitest`
- `npm-eslint-plugin`

The registry does not own Cargo crate versioning in the first pass. The Rust
release flow remains repository-wide.

### Impact Path Modeling

Impact paths are source inputs, not release outputs. Generated changelog files
are always excluded from impact classification, and version-only edits must not
cause a target to look impacted by themselves. Some version files still remain
in `impactPaths` because they also carry shipped metadata or build inputs, such
as `package.json` contribution metadata or IntelliJ Gradle properties. The
helper therefore classifies impact as `matches any impactPath` minus `matches
any excludePath`, with one extra semantic rule: if a changed file is the
configured `versionFile`, the configured `versionKey` is normalized away before
comparing the file contents across the release range.

For IntelliJ, impact rules should model the packaged plugin inputs rather than
whole source trees. On the current main branch, the plugin packages shared
preview assets but does not yet bundle the website graph explorer assets, so
the initial target should include:

- `editors/intellij/build.gradle.kts`
- `editors/intellij/gradle.properties`
- `editors/intellij/settings.gradle.kts`
- `editors/intellij/gradle/libs.versions.toml`
- `editors/intellij/src/main/**`
- `packages/preview/src/**`
- `packages/preview/styles/**`
- `packages/preview/scripts/**`
- `packages/preview/esbuild.mjs`
- `packages/preview/package.json`

With exclusions such as:

- `editors/intellij/src/test/**`
- `packages/preview/**/__tests__/**`
- `packages/preview/**/*.test.ts`
- `packages/preview/**/*.snap`

This matches the current plugin build, which copies shared preview assets into
the packaged plugin.

### Helper Script

Add `scripts/release-targets/index.mjs` with two subcommands.

#### `detect`

Inputs:

- `--base-ref <ref>` and `--head-ref <ref>` for explicit release range
- or a convenience mode that resolves the previous reachable release tag

Behavior:

1. Load `release-targets.json`.
2. Resolve changed files in the release range using `git diff --name-only`.
3. Mark a target impacted if any changed file matches any of its
   `impactPaths` and none of its `excludePaths`.
4. Emit JSON to stdout and, when requested, GitHub Actions outputs in
   `key=value` form.

This subcommand is used in GitHub Actions after a tag push. It is also used by
local release prep when deciding which target-local files to update.

#### `prepare`

Inputs:

- `--version <X.Y.Z>`
- the same `--base-ref` / `--head-ref` options as `detect`

Behavior:

1. Reuse the same impact detection logic.
2. For each impacted target, update the target's version file to the tagged
   release version.
3. For each impacted target with a changelog file, invoke `git-cliff` with the
   target's `cliffConfig` and the target's `impactPaths` to render a
   target-specific changelog.
4. Leave unimpacted target-local version and changelog files untouched.
5. Emit the impacted target map so the caller can decide what to stage or
   publish.

The helper remains intentionally narrow: it does not replace the whole release
task, only the selective target portion.

### Release Preparation Flow

Update `mise release` so the target flow happens before staging:

1. Determine the new release version from the user argument.
2. Run the existing repository-wide version bumps and root changelog generation.
3. Run `node scripts/release-targets/index.mjs prepare --version "$VERSION"`.
4. Stage only the target-local files that were actually changed.

This keeps the current release tag as the global repository version while
allowing selected targets to lag if they were unaffected by a release.

### GitHub Actions Flow

Add a small detection step in `.github/workflows/release.yml` after checkout:

1. Set up Node in the release job.
2. Run `node scripts/release-targets/index.mjs detect --base-ref <previous-tag> --head-ref
   "$GITHUB_REF_NAME"`.
3. Expose outputs such as `publish_intellij`, `publish_vscode`,
   `publish_npm_vitest`, and `publish_npm_eslint_plugin`.

Downstream publish jobs use `if:` guards against those outputs. For npm, keep
the existing single inline `publish-npm` job, but gate each package publish
step separately with step-level `if:` conditions so `@supersigil/vitest` and
`@supersigil/eslint-plugin` can publish independently.

Target-specific publish workflows may continue exposing `workflow_dispatch` as
an explicit break-glass path. Tagged release automation remains the normal
target-aware path and still uses impact detection outputs from
`scripts/release-targets/index.mjs`, but maintainers may manually publish a
target when recovery or republish work is needed after a failing or incomplete
release run.

The IntelliJ publish job remains commented out, but the detection output is
still produced so Marketplace enablement later is only a workflow toggle.

### IntelliJ Changelog Integration

Keep JetBrains' Gradle Changelog Plugin as the renderer for `changeNotes`, but
make `editors/intellij/CHANGELOG.md` the generated source of truth.

`build.gradle.kts` keeps rendering:

- the section matching `pluginVersion` from `editors/intellij/CHANGELOG.md`
- in HTML for JetBrains Marketplace
- and fails if that section is missing, instead of falling back to
  `Unreleased`

The publish path stops depending on `patchChangelog`, because target changelog
generation is owned by git-cliff during release prep, not by Gradle during
publish.

If the IntelliJ target is not impacted, `pluginVersion` and the local
changelog remain unchanged. That means no IntelliJ Marketplace publish should
run for that release.

## Key Types

Conceptual target registry shape:

```json
{
  "targets": [
    {
      "id": "intellij",
      "enabled": false,
      "versionFile": "editors/intellij/gradle.properties",
      "versionKind": "gradle-property",
      "versionKey": "pluginVersion",
      "changelogFile": "editors/intellij/CHANGELOG.md",
      "cliffConfig": "cliff.intellij.toml",
      "impactPaths": [
        "editors/intellij/build.gradle.kts",
        "editors/intellij/gradle.properties",
        "editors/intellij/settings.gradle.kts",
        "editors/intellij/gradle/libs.versions.toml",
        "editors/intellij/src/main/**",
        "packages/preview/src/**",
        "packages/preview/styles/**",
        "packages/preview/scripts/**",
        "packages/preview/esbuild.mjs",
        "packages/preview/package.json"
      ],
      "excludePaths": [
        "editors/intellij/src/test/**",
        "packages/preview/**/__tests__/**",
        "packages/preview/**/*.test.ts",
        "packages/preview/**/*.snap"
      ],
      "publishJob": "intellij"
    }
  ]
}
```

Helper-script concepts:

- `ReleaseTarget`: one parsed registry entry
- `ImpactResult`: `{ targetId, impacted, changedFiles[] }`
- `PrepareResult`: impacted map plus the list of files rewritten

Supported version-editing strategies in the first pass:

- `package-json`: parse JSON and replace the top-level `version`
- `gradle-property`: replace one `key = value` line in `gradle.properties`

Helper execution configuration in the first pass:

- `gitCliffBin`: optional CLI flag or environment override for the
  `git-cliff` executable path

## Error Handling

- If the registry is malformed, the helper exits non-zero with a file and field
  specific error.
- If a target references an unsupported `versionKind`, the helper exits
  non-zero before touching files.
- If `git-cliff` is unavailable during `prepare`, the helper exits non-zero and
  leaves the release task incomplete.
- If the injected `gitCliffBin` path is invalid, the helper exits non-zero
  before any target files are rewritten.
- If a target is marked impacted but its `versionFile` or `versionKey` cannot
  be updated, the helper exits non-zero instead of silently skipping it.
- If the IntelliJ build cannot find the changelog section matching
  `pluginVersion`, Gradle should fail immediately instead of rendering fallback
  notes.
- If the release range has no previous tag, detection falls back to the
  repository root range so the first tagged release still works.

## Testing Strategy

Drive the feature through Rust integration tests that invoke the helper script
against fixture git repositories. This keeps the coverage inside
`cargo nextest run`, which is already required by the repository. The tests
should inject a fixture `git-cliff` binary path so they do not depend on a
globally installed tool.

Test cases:

- only CLI changes -> no editor or npm targets impacted
- `editors/intellij/**` changes -> `intellij` impacted
- shared preview asset changes -> `intellij` impacted
- preview test-only changes -> `intellij` not impacted
- version-only changes in target version files -> target not impacted
- `editors/vscode/**` changes -> only `vscode` impacted
- `packages/vitest/**` changes -> only `npm-vitest` impacted
- `prepare` rewrites only impacted target version files
- `prepare` rewrites only impacted target changelogs
- invalid `git-cliff` path fails before target files are rewritten
- target changelog output omits `chore(release)` commits

Manual verification still matters for the IntelliJ side:

- build the plugin after generating a target changelog
- confirm `changeNotes` in the patched plugin metadata contains only the
  IntelliJ-target release notes

## Alternatives Considered

### Ad hoc per-target release logic

Rejected. It is the fastest short-term change for IntelliJ alone, but it would
duplicate target impact logic across `mise`, release workflows, and future
publish jobs.

### Full build-graph inference

Rejected. Automatically deriving every target's artifact inputs from build
graphs would be more general, but the repository does not need that complexity
yet. Explicit `impactPaths` are easier to review and maintain.

### Rust-only helper

Rejected for the first pass. A Rust helper would fit the repository's main
language, but the release workflow job that needs target detection does not
currently install Rust. A small Node script is simpler to run in both local
release prep and GitHub Actions, while a local Vitest suite next to the helper
keeps the tests in the same ecosystem and feeds `@supersigil/vitest` evidence
into the graph directly.
