---
supersigil:
  id: skills-install/design
  type: design
  status: approved
title: "Skills Installation Design"
---

```supersigil-xml
<Implements refs="skills-install/req" />

<TrackedFiles paths="crates/supersigil-cli/src/skills.rs, crates/supersigil-cli/src/prompt.rs, crates/supersigil-cli/src/commands/init.rs, crates/supersigil-cli/src/commands/skills.rs, crates/supersigil-core/src/config.rs" />
```

## Overview

Skills are embedded at compile time via `include_dir` and written to disk by a
shared `skills` module used by both `init` and `skills install`. Interactive
prompting uses minimal custom code (two simple prompt helpers), no external
prompt library.

## Architecture

Three areas of change:

### 1. Config extension (`supersigil-core`)

Add an optional `skills` section to `Config`:

```toml
[skills]
path = "custom/skills/path"
```

Deserialized into a `SkillsConfig` struct. The `path` field is `Option<String>`
— absent means use the default.

### 2. Skills module (`supersigil-cli/src/skills.rs`)

Owns the embedded data and write logic:

- Uses the `include_dir` crate to embed the skill directories from
  `.agents/skills/` at compile time.
- Exposes `write_skills(dir: &Path) -> Result<usize, io::Error>` that
  recursively creates directories and writes all embedded files, returning
  the count of skills written.
- Exposes `DEFAULT_SKILLS_PATH` as `.agents/skills/`.

### 3. CLI commands

**`init` enhanced** — The `Command::Init` variant changes from a unit variant
to `Init(InitArgs)`. New flags: `-y`, `--skills`, `--no-skills`,
`--skills-path`. After creating `supersigil.toml`, the command resolves
whether to install skills based on flags and TTY state, then delegates to
`skills::write_skills`.

**`skills install` added** — New `Command::Skills(SkillsArgs)` variant with
nested `SkillsCommand::Install(SkillsInstallArgs)`. The `--path` flag
overrides the resolved skills directory. This is a pre-config command
(dispatched before config loading in `main.rs`, alongside `init` and
`import`).

### Prompt module (`supersigil-cli/src/prompt.rs`)

Two helper functions:

- `confirm(message, default_yes) -> Result<bool>` — writes prompt to stderr,
  reads a y/n line from stdin. Returns default on empty input.
- `input_with_default(message, default) -> Result<String>` — writes prompt to
  stderr showing the default in brackets, reads a line, returns default on
  empty.

Both check `stdin().is_terminal()`. If not a TTY, return the default without
reading.

## Key Types

```rust
// supersigil-core/src/config.rs
pub struct SkillsConfig {
    pub path: Option<String>,
}

// supersigil-cli/src/commands.rs
pub struct InitArgs {
    #[arg(short = 'y')]
    pub yes: bool,
    #[arg(long, conflicts_with = "no_skills")]
    pub skills: bool,
    #[arg(long, conflicts_with = "skills")]
    pub no_skills: bool,
    #[arg(long)]
    pub skills_path: Option<PathBuf>,
}

pub struct SkillsArgs {
    #[command(subcommand)]
    pub command: SkillsCommand,
}

pub enum SkillsCommand {
    Install(SkillsInstallArgs),
}

pub struct SkillsInstallArgs {
    #[arg(long)]
    pub path: Option<PathBuf>,
}
```

## Flag Resolution

The `init` command resolves skills behavior through this decision table:

| Flags                           | Skills? | Path          | Prompts?       |
|---------------------------------|---------|---------------|----------------|
| `--no-skills`                   | no      | —             | none           |
| `--skills-path <p>`             | yes     | `<p>`         | none           |
| `--skills --skills-path <p>`    | yes     | `<p>`         | none           |
| `--skills -y`                   | yes     | default       | none           |
| `--skills` (TTY, no `-y`)       | yes     | —             | path prompt    |
| `-y`                            | yes     | default       | none           |
| non-TTY (no flags)              | yes     | default       | none           |
| TTY (no flags)                  | —       | —             | both prompts   |

## Config File Generation

When skills path is default or skills are skipped, `init` writes the existing
scaffold:

```toml
paths = ["specs/**/*.md"]
```

When skills path is non-default, `init` appends:

```toml
paths = ["specs/**/*.md"]

[skills]
path = "custom/path"
```

## Path Resolution for `skills install`

Resolution order: `--path` flag > `skills.path` from `supersigil.toml` >
`DEFAULT_SKILLS_PATH`.

Config loading is best-effort. If `supersigil.toml` is absent or does not
parse, fall through to the default path.

## Error Handling

- `init` with `--skills` and `--no-skills`: rejected by clap via
  `conflicts_with`.
- File write errors during skill installation from `init`: reported to stderr
  but do not fail the overall init (config file was already created
  successfully).
- `skills install` write errors: fatal, propagated as `CliError::Io`.

## Testing Strategy

Integration tests with `assert_cmd` in temporary directories (existing
pattern):

- `init` without flags in non-TTY: verify `supersigil.toml` AND skills at
  default path.
- `init --no-skills`: verify `supersigil.toml` only, no skills directory.
- `init --skills-path custom/`: verify skills at custom path, `skills.path`
  present in generated TOML.
- `skills install`: verify file tree matches embedded skills (spot-check key
  files).
- `skills install` with existing skills: verify overwrite.
- `skills install --path custom/`: verify custom path used.
- `skills install` with `skills.path` in existing TOML: verify config path
  used.
- Flag conflict (`--skills --no-skills`): verify clap error.

## Alternatives Considered

- **`dialoguer` for prompts**: Only two simple prompts; a dependency is not
  warranted. Custom prompt helpers are ~15 lines each.
- **Network-based skill download**: Embedding avoids network dependency and
  versioning complexity. Skills are small markdown files, so binary size
  impact is negligible.
- **Build script instead of `include_dir`**: Viable but more maintenance.
  `include_dir` handles recursive inclusion cleanly.
- **Separate `supersigil-skills` crate**: YAGNI. Only the CLI crate needs the
  embedded data today.
