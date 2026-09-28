# Review Slice, Plan 0 of 3: Purge

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Archive the spec-driven Supersigil under a tag and reduce the repository to a clean base for the review-first pivot: one CLI crate shell, the workspace lint policy, trimmed CI and release tooling, and the pivot documents.

**Architecture:** Nothing is preserved by keeping code around. Git history and the archive tag preserve everything; the working tree keeps only what plans 1 to 3 build on. The `supersigil` crate becomes a clap shell with the color and JSON output helpers and shell completions. The workspace `Cargo.toml` keeps its `[workspace.lints]` sections verbatim, since they encode the project's Rust guidelines.

**Tech Stack:** git, Rust 2024 workspace with the existing lint policy, clap 4, cocogitto and git-cliff for commits and changelog, mise for tasks, pnpm only for the release-targets helper, GitHub Actions.

**Spec:** `docs/research/multiverse-review-design.md`, decision 6 as revised on 2026-09-28: the author chose a full purge over leaving the spec crates in place.

## Global Constraints

- The two `[workspace.lints.rust]` and `[workspace.lints.clippy]` tables in the root `Cargo.toml` are copied byte for byte. Do not loosen, reorder, or reformat them.
- `rust-version = "1.92"`, edition 2024, `resolver = "3"` stay.
- Nothing is unpublished. The 0.14.0 releases on crates.io, Homebrew, AUR, the VS Code marketplace, and the JetBrains marketplace stay as they are.
- The archive tag is created before any deletion and is never moved.
- Deletions use `git rm -r` so the commit records them; untracked leftovers such as `node_modules` and `dist` output directories are removed with `rm -rf` only if they are already gitignored.
- The `.env` file and the `.idea`, `.junie`, `.claude/settings.local.json`, `.claude/projects`, `.claude/worktrees`, and `.superpowers` entries are not touched.
- After the purge, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features`, and `cargo nextest run` pass with no warnings, and `pnpm install` followed by `pnpm -r test` passes for the one remaining JS package.
- Commits: conventional commit type, no scope, stage files by name, and no `Co-Authored-By` or other attribution trailer; the author keeps AI attribution out of the commit history. The deletion commit uses the `!` breaking marker and a `BREAKING CHANGE:` footer.

## Review Focus

Things a person will hit after this lands, each pinned to a step:

1. `cargo install supersigil` from git or a future release must still produce a binary that prints help and completions. (Task 4, `smoke.rs`.)
2. The release workflow must not reference `supersigil-lsp`, `cli-assets`, `vendored-openssl`, the deleted `prepare` job, or the deleted editor and npm publish workflows, or the next tag push fails. (Task 5, step 2 grep gate.)
3. The Homebrew formula and AUR templates must not reference the LSP archives, or the publish jobs fail on a missing checksum. (Task 5, step 4 grep gate.)
4. `pnpm install` and `mise qa` must run end to end: the release-targets helper must not depend on the deleted vitest package or on removed release targets, or setup and the pre-commit hook block every commit. (Task 5, step 6.)
5. Someone reading `README.md` or the changelog must find the archive tag and the last spec-driven version. (Task 6.)

---

## File Structure

Kept and rewritten:

| Path | After the purge |
|---|---|
| `Cargo.toml` | one member, trimmed dependencies, lints verbatim |
| `Cargo.lock` | regenerated |
| `crates/supersigil-cli/` | clap shell: `Cargo.toml`, `README.md`, `src/{main,lib,commands,error,format}.rs`, `src/commands/completions.rs`, `tests/{common/mod.rs,cmd_completions.rs,smoke.rs}` |
| `.github/workflows/{audit,ci,release,publish-crates,publish-homebrew,publish-aur}.yml` | trimmed to the CLI crate |
| `dist/homebrew/supersigil.rb.tmpl`, `dist/aur/{supersigil,supersigil-bin}/PKGBUILD` | LSP references removed |
| `release-targets.json` | one target, `crates` |
| `mise.toml`, `package.json`, `pnpm-workspace.yaml`, `pnpm-lock.yaml` | Rust tasks plus the release-targets helper |
| `README.md`, `AGENTS.md` (via the `CLAUDE.md` symlink), `CHANGELOG.md` | rewritten for the pivot |
| `LICENSE-APACHE`, `LICENSE-MIT`, `media/`, `cliff.toml`, `cog.toml`, `deny.toml`, `lefthook.yml`, `scripts/release*.{mjs,sh,ps1}`, `scripts/command-invocation.mjs`, `scripts/release-targets/`, `.gitignore`, `.agents/skills/frontend-design/`, `.claude/skills/frontend-design/`, `docs/research/multiverse-review-design.md`, `docs/research/2026-09-28-review-slice-*.md` | unchanged |

Deleted:

| Path | Why |
|---|---|
| `crates/supersigil-{core,parser,import,evidence,js,rust,rust-macros,verify,lsp}/` | spec model, verification, plugins, LSP |
| `crates/supersigil-cli/{skills,specs}/`, `crates/supersigil-cli/src/commands/*` except `completions.rs`, `crates/supersigil-cli/src/{discover,loader,plugins,prompt,scope,skills}.rs`, `crates/supersigil-cli/tests/*` except the three kept | spec commands and their tests |
| `editors/`, `packages/`, `website/` | editor extensions, npm packages, docs site |
| `specs/`, `supersigil.toml`, `tests/` | the project's own specs and empty fixture dir |
| `.agents/skills/ss-*`, `.claude/skills/ss-*` | spec workflow skills |
| `eval/` | spec skill evaluations; plan 3 restores the harness from the archive tag when annotator evaluations exist |
| `docs/research/{competitive-landscape,polish-audit,strategic-direction,specs-in-source,specs-in-source-decision,specs-in-source-implementation-plan}.md`, `docs/research/technical/` | spec-era research |
| `.github/workflows/{verify,publish-vscode,publish-intellij}.yml`, `dist/aur/supersigil-lsp*/`, `cliff.editor.toml`, `scripts/bundle-cli-assets.mjs`, `eslint.config.js`, `ROADMAP.md` | tooling for removed targets |

---

### Task 1: Archive tag and working branch

**Files:** none changed in the tree.

**Interfaces:**
- Produces: annotated tag `archive/spec-driven` on `main`; branch `pivot` from the current `specs-in-source` head with the pivot documents committed.

- [ ] **Step 1: Confirm the starting state**

Run: `git status --short && git branch --show-current && git log --oneline -3`
Expected: branch `specs-in-source`; the only untracked or modified paths are `docs/research/multiverse-review-design.md` and `docs/research/2026-09-28-review-slice-*.md`. Stop and ask if anything else is dirty.

- [ ] **Step 2: Tag the last spec-driven state**

```bash
git tag -a archive/spec-driven main -m "Last spec-driven Supersigil before the review-first pivot.

The spec verification tool, its LSP, editor extensions, npm packages,
and docs site live at this tag and in the v0.14.0 releases."
git tag --list 'archive/*'
```

Expected: one tag, `archive/spec-driven`. Do not push it; the author pushes tags.

- [ ] **Step 3: Commit the pivot documents and branch**

```bash
git add docs/research/multiverse-review-design.md docs/research/2026-09-28-review-slice-0-purge.md docs/research/2026-09-28-review-slice-1-record.md
git commit -m "docs: add review-first pivot design and plans"
git switch -c pivot
```

Expected: branch `pivot` at the new commit.

---

### Task 2: Delete the spec-driven implementation

**Files:** everything in the "Deleted" table.

**Interfaces:**
- Produces: a tree containing only the kept paths, with `crates/supersigil-cli` not yet compiling (Task 4 fixes it).

- [ ] **Step 1: Remove tracked paths**

```bash
git rm -r -q crates/supersigil-core crates/supersigil-parser crates/supersigil-import crates/supersigil-evidence crates/supersigil-js crates/supersigil-rust crates/supersigil-rust-macros crates/supersigil-verify crates/supersigil-lsp
git rm -r -q editors packages website specs supersigil.toml eval ROADMAP.md cliff.editor.toml eslint.config.js scripts/bundle-cli-assets.mjs
git rm -r -q .agents/skills/ss-ci-review .agents/skills/ss-feature-development .agents/skills/ss-feature-specification .agents/skills/ss-refactoring .agents/skills/ss-retroactive-specification .agents/skills/ss-spec-driven-development
git rm -r -q .claude/skills/ss-ci-review .claude/skills/ss-feature-development .claude/skills/ss-feature-specification .claude/skills/ss-refactoring .claude/skills/ss-retroactive-specification .claude/skills/ss-spec-driven-development
git rm -r -q docs/research/competitive-landscape.md docs/research/polish-audit.md docs/research/strategic-direction.md docs/research/specs-in-source.md docs/research/specs-in-source-decision.md docs/research/specs-in-source-implementation-plan.md docs/research/technical
git rm -r -q .github/workflows/verify.yml .github/workflows/publish-vscode.yml .github/workflows/publish-intellij.yml dist/aur/supersigil-lsp dist/aur/supersigil-lsp-bin
git rm -r -q crates/supersigil-cli/skills crates/supersigil-cli/src/discover.rs crates/supersigil-cli/src/loader.rs crates/supersigil-cli/src/plugins.rs crates/supersigil-cli/src/prompt.rs crates/supersigil-cli/src/scope.rs crates/supersigil-cli/src/skills.rs
```

If `crates/supersigil-cli/specs` or a root `tests` directory is tracked, remove them too: `git rm -r -q crates/supersigil-cli/specs tests` (ignore "did not match" for whichever does not exist).

- [ ] **Step 2: Remove the spec command modules and tests**

```bash
cd crates/supersigil-cli
git rm -q src/commands/affected.rs src/commands/context.rs src/commands/explore.rs src/commands/export.rs src/commands/graph.rs src/commands/import.rs src/commands/init.rs src/commands/ls.rs src/commands/new.rs src/commands/plan.rs src/commands/refs.rs src/commands/schema.rs src/commands/skills.rs src/commands/status.rs src/commands/verify.rs
git rm -r -q src/commands/explore_assets src/commands/explore_template.html src/commands/graph src/commands/verify
git rm -q tests/clap_parse.rs tests/cmd_context.rs tests/cmd_export.rs tests/cmd_import.rs tests/cmd_init.rs tests/cmd_ls.rs tests/cmd_new.rs tests/cmd_plan.rs tests/cmd_schema.rs tests/cmd_skills.rs tests/cmd_status.rs tests/cmd_verify.rs tests/color_resolution.rs tests/discover.rs tests/loader.rs tests/smoke.rs
cd ../..
```

Any path in those lists that does not exist means the tree differs from the plan's snapshot: list the directory, remove what is there that is not in the kept set, and continue.

- [ ] **Step 3: Remove ignored leftovers and check the tree**

```bash
rm -rf node_modules crates/supersigil-cli/src/commands/explore_assets
git status --short | grep -v '^D ' || true
find . -path ./.git -prune -o -path ./target -prune -o -path ./.idea -prune -o -path ./.superpowers -prune -o -path ./.worktrees -prune -o -type f -print | sort
```

Expected: `git status` shows only deletions. The file listing contains, apart from `.claude`, `.junie`, and `.env`: `.github/workflows/{audit,ci,publish-aur,publish-crates,publish-homebrew,release}.yml`, `.agents/skills/frontend-design/**`, `.claude/skills/frontend-design/**`, `.gitignore`, `AGENTS.md`, `CLAUDE.md`, `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`, `LICENSE-*`, `README.md`, `cliff.toml`, `cog.toml`, `deny.toml`, `lefthook.yml`, `mise.toml`, `package.json`, `pnpm-lock.yaml`, `pnpm-workspace.yaml`, `release-targets.json`, `crates/supersigil-cli/{Cargo.toml,README.md,src/main.rs,src/lib.rs,src/commands.rs,src/commands/completions.rs,src/error.rs,src/format.rs,tests/common/mod.rs,tests/cmd_completions.rs}`, `dist/homebrew/supersigil.rb.tmpl`, `dist/aur/{supersigil,supersigil-bin}/PKGBUILD`, `docs/research/multiverse-review-design.md`, `docs/research/2026-09-28-review-slice-*.md`, `media/*`, `scripts/{command-invocation,release,release-command}.mjs`, `scripts/release.{sh,ps1}`, `scripts/release-targets/{index.mjs,package.json,release-command.test.js,release-targets.test.js}`. Anything else present is either kept on purpose (say so in the task summary) or removed now.

---

### Task 3: Workspace manifest

**Files:**
- Modify: `Cargo.toml`

**Interfaces:**
- Produces: a workspace with one member and these workspace dependencies: `serde`, `serde_json`, `thiserror`, `tempfile`, `insta`, `proptest`. Plan 1 appends its own crates and dependencies at the end of `[workspace.dependencies]`.

- [ ] **Step 1: Rewrite the manifest, lints verbatim**

Replace everything above `[profile.release]` with:

```toml
[workspace]
members = ["crates/supersigil-cli"]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.92"
license = "MIT OR Apache-2.0"
repository = "https://github.com/jonisavo/supersigil"
homepage = "https://supersigil.org"

[workspace.dependencies]
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
thiserror = "2.0.18"
tempfile = "3.27.0"
insta = { version = "1.47.2", features = ["yaml"] }
proptest = "1.10.0"
```

Leave `[profile.release]`, `[workspace.lints.rust]`, and `[workspace.lints.clippy]` exactly as they are. Verify with:

```bash
git diff Cargo.toml | grep -E '^[-+]' | grep -vE '^(\+\+\+|---)' | grep -E 'lints|= "warn"|= "allow"|priority' || echo "lints untouched"
```

Expected: `lints untouched`.

- [ ] **Step 2: Commit nothing yet**

The tree does not build until Task 4. Continue.

---

### Task 4: The CLI shell

**Files:**
- Modify: `crates/supersigil-cli/Cargo.toml`, `crates/supersigil-cli/README.md`, `crates/supersigil-cli/src/main.rs`, `crates/supersigil-cli/src/lib.rs`, `crates/supersigil-cli/src/commands.rs`, `crates/supersigil-cli/src/error.rs`, `crates/supersigil-cli/src/format.rs`, `crates/supersigil-cli/tests/common/mod.rs`
- Keep unchanged: `crates/supersigil-cli/src/commands/completions.rs`, `crates/supersigil-cli/tests/cmd_completions.rs`
- Create: `crates/supersigil-cli/tests/smoke.rs`

**Interfaces:**
- Produces, for plan 1: `Cli { color: ColorChoice, command: Command }`, `Command::Completions(CompletionsArgs)`, `CliError::{Io(std::io::Error), CommandFailed(String)}`, `format::{ColorChoice, ColorConfig, ExitStatus, OutputFormat::{Terminal, Json}, Detail, Token, Painted, hint, write_json, write_json_value}`, `main::run(&Cli, ColorConfig) -> Result<ExitStatus, CliError>` with a single `match` over `Command`, and `tests/common::supersigil_cmd()`.

- [ ] **Step 1: Crate manifest**

`crates/supersigil-cli/Cargo.toml`:

```toml
[package]
name = "supersigil"
version = "0.14.0"
description = "Review agent-made changes with the reasoning that produced them"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
readme = "README.md"
keywords = ["code-review", "agents", "cli", "provenance"]
categories = ["command-line-utilities", "development-tools"]
include = ["src/", "tests/", "README.md"]

[[bin]]
name = "supersigil"
path = "src/main.rs"

[lib]
name = "supersigil_cli"
path = "src/lib.rs"

[dependencies]
anstyle = "1.0.14"
clap = { version = "4.6.0", features = ["derive"] }
clap_complete = "4"
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true

[dev-dependencies]
assert_cmd = "2.2.0"
predicates = "3.1.4"
tempfile.workspace = true

[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ version }-{ target }.tar.gz"
pkg-fmt = "tgz"
bin-dir = "{ bin }{ binary-ext }"

[package.metadata.binstall.overrides.'cfg(all(target_os = "linux", target_env = "gnu"))']
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ version }-{ target-arch }-unknown-linux-musl.tar.gz"

[package.metadata.binstall.overrides.'cfg(target_os = "windows")']
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ version }-{ target }.zip"
pkg-fmt = "zip"

[lints]
workspace = true
```

`crates/supersigil-cli/README.md`:

```markdown
# supersigil

Command-line tool for reviewing agent-made changes together with the
recorded reasoning that produced them. This crate is being rebuilt; see the
repository README for the current state.
```

- [ ] **Step 2: Library root, commands, errors**

`crates/supersigil-cli/src/lib.rs`:

```rust
//! CLI interface for supersigil.
//!
//! This crate provides the `supersigil` binary and its command
//! implementations, and exports the argument types for integration tests.

/// CLI command definitions and argument types.
pub mod commands;
/// CLI error types.
pub mod error;
/// Terminal formatting, color configuration, and output helpers.
pub mod format;

pub use commands::{Command, CompletionsArgs};
pub use format::{ColorChoice, ColorConfig, ExitStatus, OutputFormat};

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "supersigil",
    about = "Review agent-made changes with the reasoning that produced them",
    version = env!("CARGO_PKG_VERSION")
)]
/// Top-level CLI entry point parsed by clap.
pub struct Cli {
    /// Color output: always, never, or auto (default)
    #[arg(long, default_value = "auto", global = true)]
    pub color: ColorChoice,

    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}
```

`crates/supersigil-cli/src/commands.rs`:

```rust
//! Command definitions.

/// The `completions` command.
pub mod completions;

/// Available subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Generate shell completions
    Completions(CompletionsArgs),
}

/// Arguments for the `completions` command.
#[derive(Debug, clap::Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    pub shell: clap_complete::Shell,
}
```

`crates/supersigil-cli/src/error.rs`:

```rust
//! CLI error types.

/// Top-level CLI error type.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// I/O error.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Generic command failure with a message.
    #[error("{0}")]
    CommandFailed(String),
}
```

- [ ] **Step 3: Binary entry point**

`crates/supersigil-cli/src/main.rs`:

```rust
//! Supersigil CLI binary entry point.

use std::process::ExitCode;

use clap::Parser;
use supersigil_cli::error::CliError;
use supersigil_cli::{Cli, ColorConfig, Command, ExitStatus};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let color = ColorConfig::resolve(cli.color);

    match run(&cli, color) {
        Ok(ExitStatus::Success) => ExitCode::SUCCESS,
        Ok(ExitStatus::VerifyFailed) => ExitCode::from(1),
        Ok(ExitStatus::VerifyWarnings) => ExitCode::from(2),
        Err(e) => {
            if is_broken_pipe(&e) {
                return ExitCode::SUCCESS;
            }
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn is_broken_pipe(err: &CliError) -> bool {
    matches!(err, CliError::Io(io_err) if io_err.kind() == std::io::ErrorKind::BrokenPipe)
}

fn run(cli: &Cli, _color: ColorConfig) -> Result<ExitStatus, CliError> {
    match cli.command {
        Command::Completions(ref args) => {
            supersigil_cli::commands::completions::run(args)?;
        }
    }
    Ok(ExitStatus::Success)
}
```

If `ExitStatus` in `format.rs` has variants other than `Success`, `VerifyFailed`, and `VerifyWarnings`, match its actual variants; plan 1 keeps `Success` and treats the other two as reserved for later exit codes.

- [ ] **Step 4: Trim the format helpers**

In `crates/supersigil-cli/src/format.rs`: delete the line `use supersigil_core::TaskInfo;`; delete the `write_yaml` function and everything from the `write_tasks` function to the end of the file (the task table, status tokens, verified-by labels, completed summary, dependency graph renderer, and `write_cell`). Keep `Token`, `Painted`, `ColorChoice`, `ColorConfig` and both of its `impl` blocks, `ExitStatus`, `hint`, `OutputFormat`, `Detail`, `write_json`, and `write_json_value`. Remove any `use` line that becomes unused; `cargo clippy` names them.

- [ ] **Step 5: Tests**

`crates/supersigil-cli/tests/common/mod.rs`:

```rust
#![allow(
    dead_code,
    reason = "shared test helpers; not every test file uses every function"
)]

use std::process::Command;

const GIT_ENV_VARS_TO_CLEAR: [&str; 5] = [
    "GIT_COMMON_DIR",
    "GIT_DIR",
    "GIT_INDEX_FILE",
    "GIT_PREFIX",
    "GIT_WORK_TREE",
];

/// Clears git environment variables inherited from a hook or an editor so a
/// test runs against the repository it sets up.
pub fn sanitize_git_env(cmd: &mut Command) -> &mut Command {
    for name in GIT_ENV_VARS_TO_CLEAR {
        cmd.env_remove(name);
    }
    cmd
}

/// The `supersigil` binary under test with a clean git environment.
pub fn supersigil_cmd() -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("supersigil"));
    sanitize_git_env(&mut cmd);
    cmd
}
```

`crates/supersigil-cli/tests/smoke.rs`:

```rust
//! The binary starts, prints help and version, and generates completions.

mod common;

use predicates::prelude::*;

#[test]
fn help_lists_completions() {
    common::supersigil_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("completions"));
}

#[test]
fn version_matches_the_crate() {
    common::supersigil_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn bash_completions_mention_the_binary() {
    common::supersigil_cmd()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("supersigil"));
}

#[test]
fn unknown_command_fails_with_usage() {
    common::supersigil_cmd()
        .arg("verify")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}
```

In `tests/cmd_completions.rs`, delete any `use supersigil_rust::verifies;` line and any `#[verifies(...)]` attribute; leave the tests.

- [ ] **Step 6: Build, lint, test**

```bash
cargo build --workspace
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo nextest run
```

Expected: builds; no warnings; the completions and smoke tests pass. If clippy reports dead code in `format.rs` for helpers only plan 1 will use (`hint`, `write_json_value`, `Detail`, `Token` variants), keep them: they are `pub` and exported, so they are not dead; if a private item is reported, delete it.

- [ ] **Step 7: Commit the purge**

```bash
git add -u
git add crates/supersigil-cli/tests/smoke.rs
git commit -m "refactor!: remove the spec-driven implementation

Supersigil is pivoting to reviewing agent-made changes with the
reasoning that produced them. The spec model, verification engine,
ecosystem plugins, LSP server, editor extensions, npm packages, docs
site, project specs, and spec workflow skills are removed. They remain
at the archive/spec-driven tag and in the v0.14.0 releases.

The supersigil crate is reduced to a clap shell with output helpers and
shell completions. The workspace lint policy is unchanged.

BREAKING CHANGE: every command except completions is gone; the next
release is a different product under the same name."
```

`git add -u` is acceptable here because the commit is the deletion itself; it stages only tracked files, and `git status` was checked in Task 2 step 3.

---
### Task 5: CI, release, and task runner

**Files:**
- Modify: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/workflows/publish-crates.yml`, `.github/workflows/publish-homebrew.yml`, `.github/workflows/publish-aur.yml`, `dist/homebrew/supersigil.rb.tmpl`, `dist/aur/supersigil/PKGBUILD`, `dist/aur/supersigil-bin/PKGBUILD`, `release-targets.json`, `mise.toml`, `package.json`, `pnpm-workspace.yaml`, `pnpm-lock.yaml`

**Interfaces:**
- Produces: workflows and tasks that reference only the `supersigil` crate; `mise qa` = format, lint, build, test.

- [ ] **Step 1: CI workflow**

Replace `.github/workflows/ci.yml` with:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
  workflow_call:

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: Check (${{ matrix.name }})
    strategy:
      matrix:
        include:
          - name: ubuntu
            runner: ubuntu-latest
            clippy-args: ""
            commit-check: true
            fmt-check: true
          - name: windows
            runner: windows-latest
            rust-target: x86_64-pc-windows-msvc
            clippy-args: --target x86_64-pc-windows-msvc
          - name: x86_64-apple-darwin
            runner: macos-26-intel
            rust-target: x86_64-apple-darwin
            clippy-args: --target x86_64-apple-darwin
          - name: aarch64-apple-darwin
            runner: macos-latest
            rust-target: aarch64-apple-darwin
            clippy-args: --target aarch64-apple-darwin
          - name: x86_64-unknown-linux-musl
            runner: ubuntu-latest
            rust-target: x86_64-unknown-linux-musl
            deps: sudo apt-get update && sudo apt-get install -y musl-tools
            clippy-args: --target x86_64-unknown-linux-musl
          - name: aarch64-unknown-linux-musl
            runner: ubuntu-24.04-arm
            rust-target: aarch64-unknown-linux-musl
            deps: sudo apt-get update && sudo apt-get install -y musl-tools
            clippy-args: --target aarch64-unknown-linux-musl
    runs-on: ${{ matrix.runner }}
    steps:
      - name: Checkout
        uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2
        with:
          fetch-depth: 0

      - name: Install Rust
        uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # master, "Add 1.94.1 patch release"
        with:
          toolchain: stable
          components: rustfmt, clippy
          targets: ${{ matrix.rust-target }}

      - name: Cache Rust
        uses: Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4 # v2.9.1

      - name: Install dependencies
        if: matrix.deps
        run: ${{ matrix.deps }}

      - name: Format
        if: matrix.fmt-check
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --workspace --all-targets --all-features ${{ matrix.clippy-args }}

      - name: Conventional commit check
        if: matrix.commit-check
        uses: cocogitto/cocogitto-action@9a9fe03b31c47444290c0d7f9b1ee1b44ee13f20 # v4.1.0
        with:
          command: check

  test:
    name: Test (${{ matrix.name }})
    strategy:
      matrix:
        include:
          - name: ubuntu
            runner: ubuntu-latest
          - name: windows
            runner: windows-latest
    runs-on: ${{ matrix.runner }}
    steps:
      - name: Checkout
        uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2

      - name: Install Rust
        uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # master, "Add 1.94.1 patch release"
        with:
          toolchain: stable

      - name: Cache Rust
        uses: Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4 # v2.9.1

      - name: Install nextest
        uses: taiki-e/install-action@1ed3272338f573e042a2e6bca3893aa19f43b47a # v2.71.3
        with:
          tool: cargo-nextest@0.9.132

      - name: Test
        run: cargo nextest run
```

- [ ] **Step 2: Release workflow**

In `.github/workflows/release.yml`:

1. Delete the whole `prepare` job ("Bundle CLI assets"), and change the `build` job's `needs: prepare` to `needs: ci`.
2. In the `build` job matrix, delete both `extra-args: --features supersigil-verify/vendored-openssl` lines.
3. Delete the "Download CLI assets" step.
4. Change the Build step to `run: cargo build --release --target ${{ matrix.target }} -p supersigil`.
5. Replace the Unix packaging loop with a single binary:

```yaml
      - name: Package (Unix)
        if: runner.os != 'Windows'
        shell: bash
        run: |
          VERSION="${GITHUB_REF_NAME#v}"
          RELEASE_DIR="target/${{ matrix.target }}/release"
          ARCHIVE="supersigil-${VERSION}-${{ matrix.target }}.tar.gz"
          tar -czf "${ARCHIVE}" -C "${RELEASE_DIR}" supersigil
          if command -v sha256sum &>/dev/null; then
            sha256sum "${ARCHIVE}" > "${ARCHIVE}.sha256"
          else
            shasum -a 256 "${ARCHIVE}" > "${ARCHIVE}.sha256"
          fi
```

6. In the Windows packaging step, reduce the `Compress-Archive -Path @(...)` list to `(Join-Path $releaseDir "supersigil.exe")` only.
7. If the `release` job builds a `SHA256SUMS` file from artifact names, leave it; it only lists what was uploaded.
8. In the `target-metadata` job, keep only the `publish_crates` output. Delete the `publish_intellij`, `publish_vscode`, `publish_npm_vitest`, and `publish_npm_eslint_plugin` output lines.
9. Delete the `publish-npm`, `publish-vscode`, and `publish-intellij` jobs entirely; the first is inline and the other two call workflows that Task 2 removed. Delete the `id-token: write # npm OIDC` line under `permissions`, which only npm provenance needed. The remaining jobs are `ci`, `build`, `release`, `target-metadata`, `publish-crates`, `publish-homebrew`, and `publish-aur`.

Gate:

```bash
grep -nE "supersigil-lsp|cli-assets|vendored-openssl|pnpm|bundle|vscode|intellij|npm|prepare" .github/workflows/release.yml || echo "release.yml clean"
```

Expected: `release.yml clean`. A workflow that `uses:` a file that no longer exists fails at the next tag push, so this gate is not optional.

- [ ] **Step 3: Crates publish workflow**

In `.github/workflows/publish-crates.yml`, delete the "Install pnpm", "Setup Node", and "Bundle CLI assets" steps. The remaining steps are Checkout, Install Rust, Install cargo-workspaces, Publish.

- [ ] **Step 4: Homebrew and AUR**

`.github/workflows/publish-homebrew.yml`: delete the four `sed` lines that substitute `@@SHA256_LSP_*@@`, and remove ` | grep -v lsp` from the four remaining `sed` lines.

`dist/homebrew/supersigil.rb.tmpl`: delete every `resource "supersigil-lsp" do ... end` block (four of them) and the `resource("supersigil-lsp").stage do ... end` block in `install`, so the formula installs only `bin.install "supersigil"`.

`.github/workflows/publish-aur.yml`: delete the two matrix entries `pkgname: supersigil-lsp-bin` and `pkgname: supersigil-lsp` with their attributes, delete the `else` branch that computes LSP checksums (the two lines grepping `supersigil-lsp-${VERSION}`), and remove ` | grep -v lsp` from the two remaining checksum lines.

`dist/aur/supersigil/PKGBUILD` and `dist/aur/supersigil-bin/PKGBUILD`: delete the `optdepends=(...)` line that names the LSP package.

Gate:

```bash
grep -rnE "lsp" .github/workflows dist || echo "no lsp references"
```

Expected: `no lsp references`.

- [ ] **Step 5: Release targets**

Replace `release-targets.json` with:

```json
{
  "targets": [
    {
      "id": "crates",
      "enabled": true,
      "versionFile": "Cargo.toml",
      "versionKind": "cargo-workspace",
      "impactPaths": [
        "Cargo.toml",
        "Cargo.lock",
        "crates/*/Cargo.toml",
        "crates/*/README.md",
        "crates/*/src/**",
        "crates/*/tests/**",
        "crates/*/build.rs"
      ],
      "excludePaths": []
    }
  ]
}
```

- [ ] **Step 6: Task runner and JS workspace**

Replace `mise.toml` with:

```toml
[tasks.setup]
description = "Set up the development environment."
run = [
  "pnpm install",
  "cargo build --workspace",
  "lefthook install",
]

[tasks.format]
description = "Format the workspace."
run = "cargo fmt --all"

[tasks.lint]
description = "Lint the workspace."
run = "cargo clippy --workspace --all-targets --all-features"

[tasks.fix]
description = "Apply fixes in the workspace."
run = [
  "cargo fmt --all",
  "cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged",
]

[tasks.build]
description = "Build the workspace."
run = "cargo build --workspace --release"

[tasks.test]
description = "Run workspace tests."
run = [
  "cargo nextest run",
  "mise test:release-targets",
]

[tasks."test:release-targets"]
description = "Run release-target helper tests."
run = "pnpm test"
dir = "scripts/release-targets"

[tasks.qa]
description = "Run all tooling to ensure the repository is squeaky clean."
run = ["mise format", "mise lint", "mise build", "mise test"]

[tasks.install-cli]
description = "Install the CLI locally"
run = "cargo install --path ."
dir = "crates/supersigil-cli"

[tasks."install-cli:safe"]
description = "Install the CLI locally after QA is OK"
run = ["mise install-cli"]
depends = "qa"

[tasks."release:detect"]
description = "Detect what will be released based on git history and print a summary."
run = "node scripts/release-targets/index.mjs detect"

[tasks.release]
description = "Prepare a release: bump versions, update changelog, commit, and tag."
run = "bash scripts/release.sh"
run_windows = "powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release.ps1"
```

Replace `package.json` with:

```json
{
  "private": true,
  "packageManager": "pnpm@11.5.2",
  "type": "module"
}
```

Replace `pnpm-workspace.yaml` with:

```yaml
packages:
  - scripts/release-targets

catalog:
  '@types/node': ^22.19.17
  vitest: ^4.1.3
```

If `scripts/release-targets/package.json` references a catalog entry that is not `vitest` or `@types/node`, add that entry back to the catalog with the version from the current `pnpm-workspace.yaml` in git history (`git show archive/spec-driven:pnpm-workspace.yaml`).

The release-targets helper depended on the deleted vitest package for spec tagging, so fix it before installing:

- In `scripts/release-targets/package.json`, delete the line `"@supersigil/vitest": "workspace:*",` from `devDependencies`.
- In `scripts/release-targets/release-command.test.js` and `scripts/release-targets/release-targets.test.js`, delete the line `import { verifies } from "@supersigil/vitest";`. In every `it(` call, delete the `verifies(...)` argument, which sits between the test name and the function and sometimes spans several lines, so that each call reads `it(name, fn)`. The assertions inside stay as they are.
- Delete every test case in `release-targets.test.js` that reads a checked-in target other than `crates`, for example through `getCheckedInTarget("vscode")` or `getCheckedInTarget("intellij")`, and any helper that only those cases used. Those targets no longer exist. Keep every test that builds its own fixture targets.

Then:

```bash
pnpm install
pnpm -r test
mise qa
```

Expected: `pnpm install` rewrites `pnpm-lock.yaml`; the release-targets tests pass. If `release-targets.test.js` asserts on the real `release-targets.json` target list, change that assertion to the single `crates` target and say so in the task summary. `mise qa` runs format, lint, build, test with no errors.

- [ ] **Step 7: Commit**

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml .github/workflows/publish-crates.yml .github/workflows/publish-homebrew.yml .github/workflows/publish-aur.yml dist/homebrew/supersigil.rb.tmpl dist/aur/supersigil/PKGBUILD dist/aur/supersigil-bin/PKGBUILD release-targets.json mise.toml package.json pnpm-workspace.yaml pnpm-lock.yaml
git add scripts/release-targets/package.json scripts/release-targets/release-command.test.js scripts/release-targets/release-targets.test.js
git commit -m "ci: trim workflows and release tooling to the CLI crate"
```

---

### Task 6: README, agent instructions, changelog

**Files:**
- Modify: `README.md`, `AGENTS.md`, `CHANGELOG.md`

**Interfaces:**
- Produces: a README that states the direction and points at the archive; an `AGENTS.md` whose gate is fmt, clippy, nextest; a changelog entry naming the archive tag.

- [ ] **Step 1: README**

Replace `README.md` with:

```markdown
<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="media/logo.png" />
    <source media="(prefers-color-scheme: light)" srcset="media/logo_light.png" />
    <img alt="Supersigil" src="media/logo.png" width="400" />
  </picture>
</p>

<p align="center">
  Review agent-made changes with the reasoning that produced them.
</p>

---

Supersigil is being rebuilt. The new tool records what agent sessions did
to a repository, links every changed span to the recorded edits and the
rationale behind them, keeps what was tried and reverted, and lets you
review a change in the terminal with that history beside the diff. It stays
honest about what the evidence does and does not show. The design is in
[`docs/research/multiverse-review-design.md`](docs/research/multiverse-review-design.md).

The previous Supersigil, a spec-driven development tool with a
verification engine, an LSP server, and editor extensions, lives at the
`archive/spec-driven` tag and in the v0.14.0 releases on crates.io,
Homebrew, AUR, and the VS Code and JetBrains marketplaces. Its docs site is
not maintained.

## Status

Only shell completions exist in this tree today. The first arc lands in
three steps: the local record of agent sessions, the review model with
attribution, and a terminal review.

## Development

```sh
mise setup     # once
mise qa        # format, lint, build, test
```

## License

MIT OR Apache-2.0.
```

- [ ] **Step 2: Agent instructions**

Replace `AGENTS.md` with (the `CLAUDE.md` symlink follows it):

```markdown
# Guidelines

- Follow Test Driven Development: write the failing test first.
- Use `cargo fmt` for formatting code.
- Use `cargo clippy` for linting. The workspace lint policy in `Cargo.toml`
  is strict on purpose and is not to be relaxed.
- Use `cargo nextest run` for testing.
- When bootstrapping a new worktree, use `mise trust` and `mise setup`.

Run all three before finalizing work:

```shell
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo nextest run
```

No warnings or errors should be left.

# Direction

Supersigil is being rebuilt as a tool for reviewing agent-made changes with
the recorded reasoning behind them. Read
`docs/research/multiverse-review-design.md` before changing the model. The
guiding rule: make development history easier to inspect without making it
look more certain than the evidence allows.

# Style

- Pragmatic and idiomatic Rust.
- Use the new module syntax (so `module.rs` with `module/` instead of `module/mod.rs`).
- Every `pub` item has a doc comment; `Result`-returning functions document
  their errors.
```

- [ ] **Step 3: Changelog**

At the top of `CHANGELOG.md`, under the unreleased heading (add `## [Unreleased]` if there is none), insert:

```markdown
### Breaking changes

- Supersigil is pivoting to reviewing agent-made changes with the reasoning
  that produced them. The spec-driven tool, its LSP server, editor
  extensions, npm packages, and docs site are removed from this repository.
  They remain at the `archive/spec-driven` tag and in the v0.14.0 releases.
```

Check the file's existing structure first with `head -30 CHANGELOG.md` and follow its heading style.

- [ ] **Step 4: Commit**

```bash
git add README.md AGENTS.md CHANGELOG.md
git commit -m "docs: describe the review-first direction"
```

---

### Task 7: Final gate

**Files:** none.

- [ ] **Step 1: Run everything from a clean state**

```bash
cargo clean
mise qa
git status --short
git log --oneline -5
```

Expected: `mise qa` passes; `git status` is clean; the last four commits are the docs commit from Task 1, the purge, the ci trim, and the direction docs, in that order.

- [ ] **Step 2: Report**

In the task summary, list: the archive tag name and the commit it points at, the branch name, anything kept that the "Deleted" table said to delete, anything deleted that the "Kept" table said to keep, and the assertion change in `release-targets.test.js` if one was needed. The author decides when to push the tag and merge `pivot` into `main`.

---

## After this plan

Plan 1 starts on the `pivot` branch. Its Task 1 adds two members after `"crates/supersigil-cli"` and appends its dependencies to `[workspace.dependencies]`. Its Task 8 adds the `session` command to the single `match` in `main.rs`.
