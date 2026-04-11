# Git Worktree CLI — Spec-Driven Development

Build a Rust CLI tool called `wt` that provides a friendly interface to git
worktrees. Use supersigil for spec-driven development throughout.

## Getting Started

1. Run `supersigil init` to initialize the project (this also installs skills)
2. Use the `ss-spec-driven-development` skill to guide you through the full
   spec → implement → verify workflow

## Target Application

A CLI with subcommands:

- `wt list` — list all worktrees with status (branch, clean/dirty, path)
- `wt create <name> [--branch <branch>]` — create a new worktree (defaults to
  creating a new branch matching the name)
- `wt remove <name>` — remove a worktree (refuse if dirty unless `--force`)
- `wt switch <name>` — print the `cd` command or shell snippet to switch to a
  worktree

## Technical Constraints

- Use `clap` for argument parsing (add with `cargo add clap --features derive`)
- Use `std::process::Command` to invoke git (no git library dependency)
- Write integration tests that exercise the CLI against a temporary git repo
- Do not ask for user input — work autonomously
