---
supersigil:
  id: release-targets/tasks
  type: tasks
  status: done
title: "Release Targets"
---

```supersigil-xml
<DependsOn refs="release-targets/design" />
```

## Overview

Tasks `task-1` through `task-7` capture the first release-target pass that is
already in place. The next pass starts at `task-8` and adds aggregate crate
publishing, shared editor changelog generation, and publish ordering between
crates and editors. The follow-up Windows pass starts at `task-13`.

```supersigil-xml
<Task
  id="task-1"
  status="done"
  implements="release-targets/req#req-1-1, release-targets/req#req-1-2, release-targets/req#req-1-3, release-targets/req#req-1-4, release-targets/req#req-1-5, release-targets/req#req-2-1, release-targets/req#req-2-2, release-targets/req#req-2-4, release-targets/req#req-2-5"
>
  Add a release-target test harness and fixture repositories that exercise
  impact detection through a repo-local helper script. Cover IntelliJ,
  VS Code, npm package, no-impact, and test-only shared-asset cases before
  implementing the helper. Provide a fixture changelog-renderer seam so tests
  do not depend on a globally installed `git-cliff`.
</Task>

<Task
  id="task-2"
  status="done"
  depends="task-1"
  implements="release-targets/req#req-1-1, release-targets/req#req-1-2, release-targets/req#req-1-3, release-targets/req#req-1-4, release-targets/req#req-1-5, release-targets/req#req-2-1, release-targets/req#req-2-2, release-targets/req#req-2-3, release-targets/req#req-2-4, release-targets/req#req-2-5"
>
  Add `release-targets.json`, `cliff.intellij.toml`, and
  `scripts/release-targets/index.mjs`. Implement target loading, release-range file
  discovery, include/exclude path matching, machine-readable detect output,
  injected `git-cliff` executable selection, and target changelog generation
  that excludes `chore(release)` commits.
</Task>

<Task
  id="task-3"
  status="done"
  depends="task-2"
  implements="release-targets/req#req-3-1, release-targets/req#req-3-2, release-targets/req#req-3-3, release-targets/req#req-3-4"
>
  Integrate the helper into `mise release` so target-local versions and
  changelogs are updated only for impacted targets, while the root
  `CHANGELOG.md` continues to be generated for every tagged repository release.
</Task>

<Task
  id="task-4"
  status="done"
  depends="task-2"
  implements="release-targets/req#req-5-3, release-targets/req#req-5-4, release-targets/req#req-5-5, release-targets/req#req-5-6"
>
  Update the IntelliJ plugin release metadata flow: generate
  `editors/intellij/CHANGELOG.md` only when the IntelliJ target is impacted,
  render `changeNotes` from that file in Gradle, hard-fail if the matching
  changelog section is missing, and remove publish-time changelog mutation.
</Task>

<Task
  id="task-5"
  status="done"
  depends="task-2"
  implements="release-targets/req#req-4-1, release-targets/req#req-4-2, release-targets/req#req-4-3, release-targets/req#req-4-4"
>
  Wire release-target detection into `.github/workflows/release.yml` and the
  target publish workflows. Expose per-target outputs, gate the two npm
  publish steps independently inside the existing inline job, preserve manual
  `workflow_dispatch` entry points as explicit break-glass publish paths, and
  keep IntelliJ metadata detection active even though
  Marketplace publishing remains disabled.
</Task>

<Task id="task-6" status="done">
  Historical first-pass cleanup: align release-related docs and design notes
  with the initial selective-release model.
</Task>

<Task
  id="task-7"
  status="done"
  depends="task-3, task-4, task-5, task-6"
>
  Run the full verification loop: `cargo run -p supersigil verify`,
  `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features`,
  and `cargo nextest run`. Perform one manual IntelliJ metadata smoke check by
  building the plugin and inspecting the generated change notes source.
</Task>

<Task
  id="task-8"
  status="done"
  depends="task-2"
  implements="release-targets/req#req-1-3, release-targets/req#req-1-4, release-targets/req#req-1-6, release-targets/req#req-2-1, release-targets/req#req-2-2, release-targets/req#req-2-4, release-targets/req#req-3-5, release-targets/req#req-3-6"
>
  Extend the release-target registry, helper, and fixture tests for an
  aggregate `crates` target. Add a `cargo-workspace` version strategy that
  bumps all workspace crates and root `=version` pins together when the crates
  target is impacted, and leaves them untouched otherwise. Model the crates
  target from shipped crate inputs, including bundled CLI asset sources and any
  needed exclusions for non-shipping files.
</Task>

<Task
  id="task-9"
  status="done"
  depends="task-2"
  implements="release-targets/req#req-2-3, release-targets/req#req-2-5, release-targets/req#req-2-6, release-targets/req#req-5-1, release-targets/req#req-5-2, release-targets/req#req-5-3, release-targets/req#req-5-5"
>
  Replace `cliff.intellij.toml` with a shared `cliff.editor.toml`, add
  `editors/vscode/CHANGELOG.md`, and update release-target preparation so both
  editor changelogs are generated from target-local commits while omitting
  `ci(...)` commits entirely.
</Task>

<Task
  id="task-10"
  status="done"
  depends="task-8, task-9"
  implements="release-targets/req#req-3-1, release-targets/req#req-3-2, release-targets/req#req-3-3, release-targets/req#req-3-4, release-targets/req#req-3-5, release-targets/req#req-3-6"
>
  Update `mise release` to delegate all target-local version edits to the
  release-target helper, regenerate `Cargo.lock` only when crates are bumped,
  and stage only the files changed by impacted targets.
</Task>

<Task
  id="task-11"
  status="done"
  depends="task-8, task-9"
  implements="release-targets/req#req-4-1, release-targets/req#req-4-2, release-targets/req#req-4-3, release-targets/req#req-4-4, release-targets/req#req-4-5, release-targets/req#req-4-6"
>
  Update `.github/workflows/release.yml` so `publish-crates` is gated by the
  aggregate crates target, Homebrew and AUR reuse that same gate for the CLI
  distribution jobs, editor publish jobs wait for crate publishing when
  needed, and skipped crate publishes do not suppress editor releases that are
  otherwise required.
</Task>

<Task
  id="task-12"
  status="done"
  depends="task-10, task-11"
>
  Run the full verification loop again: `cargo run -p supersigil verify`,
  `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features`,
  and `cargo nextest run`. Perform manual packaging checks for VS Code
  changelog inclusion and IntelliJ change-notes rendering.
</Task>

<Task
  id="task-13"
  status="done"
  depends="task-11"
  implements="release-targets/req#req-6-1, release-targets/req#req-6-2"
>
  Extend `.github/workflows/release.yml` so the binary build matrix includes
  `x86_64-pc-windows-msvc`, packages `supersigil.exe` and
  `supersigil-lsp.exe` into a Windows-appropriate archive, and uploads matching
  checksum files with the rest of the GitHub release assets.
</Task>

<Task
  id="task-14"
  status="done"
  depends="task-10"
  implements="release-targets/req#req-6-3"
>
  Replace Unix-only bundled-asset shell commands in `mise.toml`,
  `package.json`, and `website/package.json` with host-compatible build steps
  so shipped CLI assets can be regenerated on native Windows hosts.
</Task>

<Task
  id="task-15"
  status="done"
  depends="task-13, task-14"
  implements="release-targets/req#req-6-1, release-targets/req#req-6-2, release-targets/req#req-6-3"
>
  Run the full verification loop again and perform native Windows manual
  checks: regenerate bundled CLI assets, produce at least one Windows release
  archive containing both `.exe` binaries, and confirm release checksums and
  uploaded artifact names are correct.
</Task>
```
