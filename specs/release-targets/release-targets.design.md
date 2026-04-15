---
supersigil:
  id: release-targets/design
  type: design
  status: approved
title: "Release Targets"
---

```supersigil-xml
<Implements refs="release-targets/req" />
<TrackedFiles paths="release-targets.json, cliff.editor.toml, scripts/release-targets/index.mjs, scripts/release-targets/package.json, scripts/release-targets/release-targets.test.js, mise.toml, Cargo.toml, crates/**/Cargo.toml, .github/workflows/release.yml, .github/workflows/publish-vscode.yml, .github/workflows/publish-intellij.yml, .github/workflows/publish-crates.yml, editors/intellij/build.gradle.kts, editors/intellij/CHANGELOG.md, editors/intellij/gradle.properties, editors/vscode/CHANGELOG.md, editors/vscode/package.json" />
```

## Overview

Keep the existing release-target layer, but extend it in three directions:

1. Add one aggregate `crates` target so any qualifying crate change bumps and
   publishes all workspace crates together, while crate-only releases can still
   skip editor publishes.
2. Generate target-local changelogs for both editor extensions from a shared
   editor changelog config that drops `ci(...)` commits entirely.
3. Keep editor/server compatibility separate from release-target detection.

The implementation stays intentionally small:

1. A checked-in target registry that now includes `crates`, `vscode`,
   `intellij`, `npm-vitest`, and `npm-eslint-plugin`.
2. A repo-local helper script that detects impacted targets and prepares
   target-local metadata, including aggregate crate version bumps.
3. Workflow wiring so release prep and GitHub Actions consume the same impact
   results, and editor publishes wait for crate publishes when crates are
   being released.

## Architecture

```
repo tag version (vX.Y.Z)
        │
        ├─ root release prep
        │   ├─ root CHANGELOG.md
        │   └─ lockfile refresh only when impacted targets need it
        │
        ├─ release target layer
        │   ├─ release-targets.json
        │   │    ├─ crates          (aggregate)
        │   │    ├─ vscode
        │   │    ├─ intellij
        │   │    ├─ npm-vitest
        │   │    └─ npm-eslint-plugin
        │   ├─ cliff.editor.toml
        │   └─ scripts/release-targets/index.mjs
        │
```

## Target Registry

Keep `release-targets.json` as the single checked-in registry. JSON remains the
right fit because the helper script runs in local release prep and GitHub
Actions without adding extra runtime dependencies.

Each target entry continues to describe:

- `id`
- `enabled`
- `versionFile`
- `versionKind`
- `versionKey`
- `changelogFile`
- `cliffConfig`
- `impactPaths`
- `excludePaths`

This pass adds one more supported shape: an Aggregate_Target with a
multi-file version-edit strategy.

Initial targets become:

- `crates`
- `vscode`
- `intellij`
- `npm-vitest`
- `npm-eslint-plugin`

`vscode` and `intellij` should both point at `cliff.editor.toml` for their
target changelogs. The crates target should not have a target-local changelog;
crate publishing still relies on the root release notes and crates.io metadata.

## Impact Path Modeling

Impact paths remain explicit and reviewable.

For editor targets, keep modeling shipped artifact inputs rather than broad
directories. That means:

- `vscode` continues to include extension sources plus shared preview and
  explorer inputs that affect the packaged `.vsix`
- `intellij` continues to include plugin sources plus shared preview inputs that
  affect the packaged plugin
- editor test files remain in `excludePaths`

For `crates`, impact paths should cover published crate inputs, not generated
release outputs. The aggregate target should include:

- root Cargo metadata that changes workspace publish behavior
- each workspace crate manifest
- crate source trees and build inputs that affect published packages
- source inputs for assets bundled into published crates, such as the CLI's
  checked-in `explore_assets/` files, bundled `skills/`, and the upstream
  sources that regenerate those shipped assets

The helper should keep ignoring generated changelog outputs and version-only
changes in configured version files, just as it does today for the editor and
package targets.

## Helper Script

Keep `scripts/release-targets/index.mjs` with the same two subcommands:
`detect` and `prepare`.

### `detect`

Inputs:

- `--base-ref <ref>` and `--head-ref <ref>`, or
- convenience resolution of the previous reachable release tag

Behavior:

1. Load `release-targets.json`.
2. Resolve changed files in the release range.
3. Mark each target impacted when changed files intersect its
   `impactPaths` minus `excludePaths`.
4. Emit JSON and optional GitHub Actions outputs such as
   `publish_crates`, `publish_vscode`, and `publish_intellij`.

### `prepare`

Inputs:

- `--version <X.Y.Z>`
- the same `--base-ref` / `--head-ref` options as `detect`

Behavior:

1. Reuse the same impact detection logic.
2. For each impacted target, apply its version-edit strategy.
3. For each impacted target with a changelog file, render a target-local
   changelog with its configured `cliffConfig`.
4. Leave unimpacted targets untouched.
5. Emit the impacted target map so the caller can decide what to stage.

This pass adds one new version-edit strategy:

- `cargo-workspace`: rewrite all workspace crate `Cargo.toml` files and the
  root workspace `=version` pins as one coordinated unit when the `crates`
  target is impacted

Existing strategies remain:

- `package-json`
- `gradle-property`

## Release Preparation Flow

Update `mise release` so the selective target helper owns all target-local
version edits, including crates:

1. Determine the release version from the user argument.
2. Run `node scripts/release-targets/index.mjs prepare --version "$VERSION"`.
3. If `crates` is impacted, regenerate `Cargo.lock`.
4. If any pnpm-managed target is impacted, regenerate `pnpm-lock.yaml`.
5. Generate the root `CHANGELOG.md`.
6. Stage only the files that actually changed.

The main behavior shift is that `mise release` no longer unconditionally bumps
every crate. Crate versions now move only when the `crates` target is
impacted.

## GitHub Actions Flow

`target-metadata` should continue to run after the GitHub release is created,
but now it exposes `publish_crates` alongside the existing target outputs.

Publish sequencing becomes:

1. `publish-crates` depends on `release` and `target-metadata`.
2. `publish-crates` runs only when `publish_crates == true`.
3. `publish-homebrew` and `publish-aur` depend on `release` and
   `target-metadata`, and they reuse `publish_crates` as their gate because
   they distribute the same Rust-built CLI artifact through other channels.
4. `publish-vscode` and `publish-intellij` depend on `publish-crates` as an
   ordering barrier in addition to their own target metadata.
5. Their `if:` guards must accept both `needs.publish-crates.result == 'success'`
   and `needs.publish-crates.result == 'skipped'`, so a skipped crates publish
   does not suppress an otherwise-needed editor publish.

This preserves the intended rule:

- if crates are not publishing, editor jobs run according to their own target
  booleans
- if crates are publishing, editor jobs wait for that publish to finish first

Manual `workflow_dispatch` publish workflows remain available as break-glass
recovery paths.

## Editor Changelog Integration

Replace the IntelliJ-only changelog config with one shared
`cliff.editor.toml`.

That config should:

- keep editor-local commit grouping
- skip `chore(release): prepare ...` commits
- skip all `ci(...)` commits entirely

`vscode` should gain `editors/vscode/CHANGELOG.md` at the extension root.
That file is both the checked-in target changelog and the root-level changelog
file for the packaged extension.

`intellij` keeps using `editors/intellij/CHANGELOG.md` as the source of truth
for `changeNotes`, and Gradle should continue to fail if the changelog section
for `pluginVersion` is missing.

If an editor target is not impacted, release prep should leave both its version
file and its changelog untouched.

## Key Types

Conceptual registry examples:

```json
{
  "targets": [
    {
      "id": "crates",
      "enabled": true,
      "versionFile": "Cargo.toml",
      "versionKind": "cargo-workspace",
      "versionKey": "workspace-version",
      "impactPaths": [
        "Cargo.toml",
        "crates/**/Cargo.toml",
        "crates/**/src/**",
        "crates/**/build.rs"
      ],
      "excludePaths": []
    },
    {
      "id": "vscode",
      "enabled": true,
      "versionFile": "editors/vscode/package.json",
      "versionKind": "package-json",
      "versionKey": "version",
      "changelogFile": "editors/vscode/CHANGELOG.md",
      "cliffConfig": "cliff.editor.toml",
      "impactPaths": [
        "editors/vscode/src/**",
        "packages/preview/src/**",
        "website/src/components/explore/**"
      ],
      "excludePaths": [
        "editors/vscode/src/**/*.test.ts"
      ]
    }
  ]
}
```

## Error Handling

- If the registry is malformed, the helper exits non-zero with a file and
  field-specific error.
- If a target references an unsupported `versionKind`, the helper exits
  non-zero before touching files.
- If `git-cliff` is unavailable during `prepare`, the helper exits non-zero and
  leaves the release task incomplete.
- If the `cargo-workspace` strategy cannot update one of the coordinated Cargo
  manifests, the helper exits non-zero instead of partially bumping crates.
- If the IntelliJ build cannot find the changelog section matching
  `pluginVersion`, Gradle fails immediately instead of rendering fallback
  notes.
- If `publish-crates` is skipped, editor jobs should still be able to run when
  their own target booleans are true.

## Testing Strategy

Extend the existing `scripts/release-targets` test harness rather than adding
new release-detection machinery.

Cover at least:

- crate-source changes -> `crates` impacted
- crate version-only changes -> `crates` not impacted
- no crate changes -> `publish_crates=false`
- shared editor-source changes -> only the relevant editor target impacted
- VS Code target prepare -> rewrites `editors/vscode/package.json` and
  `editors/vscode/CHANGELOG.md`
- editor changelog generation -> omits `ci(...)` commits entirely
- crates impacted + editor impacted -> release workflow outputs require editors
  to wait for crates publishing

Manual checks still matter for packaging:

- package the VS Code extension and confirm `CHANGELOG.md` is present at the
  extension root
- build the IntelliJ plugin and confirm `changeNotes` renders from the
  generated changelog

## Alternatives Considered

### Keep crates outside the release-target registry

Rejected. It preserves the current unconditional crate publish path and creates
two sources of truth for impact detection.

### Force editor publishes whenever crates publish

Rejected. It restores package-version lockstep through workflow policy, which
undercuts selective editor releases and creates empty editor publishes.

### Put compatibility ranges into the release-target design

Rejected. Release-targets should answer "what gets bumped and published," not
"which editor/server versions are compatible." That policy belongs in the
shared editor/server compatibility spec.
