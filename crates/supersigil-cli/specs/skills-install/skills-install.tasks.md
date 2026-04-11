---
supersigil:
  id: skills-install/tasks
  type: tasks
  status: done
title: "Skills Installation Tasks"
---

## Overview

Implementation follows TDD: each task writes tests first, then the minimum
code to pass them. Tasks are dependency-ordered so each builds on a tested
foundation. The prompt helpers use trait objects for testability without
requiring a real TTY.

```supersigil-xml
<Task id="task-1" status="done" implements="skills-install/req#req-1-1, skills-install/req#req-2-3">
  **Config extension.** Add `SkillsConfig` to `supersigil-core`.

  <Task id="task-1-1" status="done">
    Write unit tests: TOML with `[skills] path = "custom"` deserializes into
    `SkillsConfig { path: Some("custom") }`; absent `[skills]` section
    deserializes to `None`.
  </Task>

  <Task id="task-1-2" status="done" depends="task-1-1">
    Add `SkillsConfig` struct and wire it into `Config`. Make tests pass.
  </Task>
</Task>

<Task id="task-2" status="done" implements="skills-install/req#req-1-1, skills-install/req#req-4-1, skills-install/req#req-4-2" depends="task-1">
  **Skills embedding and write module.** Add `include_dir` dependency, create
  `src/skills.rs` with embedded skill data and `write_skills()`.

  <Task id="task-2-1" status="done">
    Write unit tests: `write_skills` to a temp dir produces the expected
    directory structure with key files present (e.g.,
    `ss-feature-development/SKILL.md`); calling it twice overwrites without
    error; returned count matches the number of embedded skills.
  </Task>

  <Task id="task-2-2" status="done" depends="task-2-1">
    Add `include_dir` to `Cargo.toml`, create `src/skills.rs` with
    `DEFAULT_SKILLS_PATH`, embedded data, and `write_skills`. Make tests pass.
  </Task>
</Task>

<Task id="task-3" status="done" implements="skills-install/req#req-2-1, skills-install/req#req-2-2, skills-install/req#req-3-5">
  **Prompt helpers.** Create `src/prompt.rs` with `confirm()` and
  `input_with_default()` that accept `Read`/`Write` trait objects for
  testability.

  <Task id="task-3-1" status="done">
    Write unit tests: `confirm` returns `true` on empty input when
    `default_yes` is true; returns `false` on "n"; `input_with_default`
    returns default on empty input, user value on non-empty.
  </Task>

  <Task id="task-3-2" status="done" depends="task-3-1">
    Implement `confirm` and `input_with_default`. Make tests pass.
  </Task>
</Task>

<Task id="task-4" status="done" implements="skills-install/req#req-2-1, skills-install/req#req-2-2, skills-install/req#req-2-3, skills-install/req#req-2-4, skills-install/req#req-2-5, skills-install/req#req-3-1, skills-install/req#req-3-2, skills-install/req#req-3-3, skills-install/req#req-3-4, skills-install/req#req-3-5, skills-install/req#req-3-6" depends="task-1, task-2, task-3">
  **Enhanced init command.** Add `InitArgs`, update `Command::Init`, implement
  flag resolution and interactive flow, config file generation with optional
  `[skills]` section.

  <Task id="task-4-1" status="done">
    Write integration tests with `assert_cmd` in temp directories: non-TTY
    (piped stdin) creates `supersigil.toml` AND skills at default path;
    `--no-skills` creates config only; `--skills-path custom/` creates skills
    at custom path with `[skills]` in TOML; `--skills --no-skills` exits with
    error.
  </Task>

  <Task id="task-4-2" status="done" depends="task-4-1">
    Add `InitArgs` struct with `-y`, `--skills`, `--no-skills`,
    `--skills-path` fields. Change `Command::Init` from unit variant to
    `Init(InitArgs)`. Update `main.rs` dispatch.
  </Task>

  <Task id="task-4-3" status="done" depends="task-4-2">
    Implement flag resolution logic, interactive prompts, and config file
    generation with optional `[skills]` section. Make integration tests pass.
  </Task>
</Task>

<Task id="task-5" status="done" implements="skills-install/req#req-4-1, skills-install/req#req-4-2, skills-install/req#req-4-3, skills-install/req#req-4-4, skills-install/req#req-4-5" depends="task-1, task-2">
  **Skills install command.** Add `SkillsArgs`/`SkillsCommand`, implement path
  resolution, wire into `main.rs` dispatch.

  <Task id="task-5-1" status="done">
    Write integration tests with `assert_cmd`: `skills install` in dir with no
    TOML writes to default path; `skills install --path custom/` writes to
    custom path; `skills install` with `[skills] path` in existing TOML uses
    config path; `skills install` over existing skills overwrites.
  </Task>

  <Task id="task-5-2" status="done" depends="task-5-1">
    Add `SkillsArgs`, `SkillsCommand`, `SkillsInstallArgs` to `commands.rs`.
    Add `Command::Skills` variant. Update `main.rs` dispatch as a pre-config
    command.
  </Task>

  <Task id="task-5-3" status="done" depends="task-5-2">
    Implement `commands/skills.rs` with path resolution (flag &gt; config &gt;
    default) and output. Make integration tests pass.
  </Task>
</Task>

<Task id="task-6" status="done" depends="task-4, task-5">
  **Verify and polish.** Run `supersigil verify`, ensure all criteria have
  test coverage via `VerifiedBy` mappings, clean up any gaps. Run
  `cargo fmt`, `cargo clippy`, `cargo nextest run`.
</Task>
```
