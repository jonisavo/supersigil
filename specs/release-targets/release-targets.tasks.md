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

Implement the release-target layer in four slices: test harness and fixture
repos first, then the registry and helper, then local release integration, and
finally GitHub Actions plus IntelliJ metadata wiring.

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
  implements="release-targets/req#req-5-1, release-targets/req#req-5-2, release-targets/req#req-5-3, release-targets/req#req-5-4"
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

<Task
  id="task-6"
  status="done"
  depends="task-3, task-4, task-5"
  implements="release-targets/req#req-6-1"
>
  Update release-related docs and specs that still describe universal lockstep
  target publishing, including the VS Code version-mismatch design rationale,
  so operator guidance matches the selective target release model.
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
```
