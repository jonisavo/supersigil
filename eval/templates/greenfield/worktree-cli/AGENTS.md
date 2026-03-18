# Git Worktree CLI — Spec-Driven Development

Build a Rust CLI tool called `wt` that provides a friendly interface to git
worktrees. Use supersigil for spec-driven development throughout.

## Target Application

A CLI with subcommands:

- `wt list` — list all worktrees with status (branch, clean/dirty, path)
- `wt create <name> [--branch <branch>]` — create a new worktree (defaults to
  creating a new branch matching the name)
- `wt remove <name>` — remove a worktree (refuse if dirty unless `--force`)
- `wt switch <name>` — print the `cd` command or shell snippet to switch to a
  worktree

## Development Workflow

Follow this order strictly:

1. Run `supersigil init` to initialize the project
2. Write requirement documents under `specs/` using `supersigil new requirement`
   - Each requirement should have specific, testable criteria
   - Cover all four subcommands plus error handling
3. Write a tasks document using `supersigil new tasks` to order implementation
4. Run `supersigil lint` to ensure specs are valid
5. Initialize the Rust project with `cargo init`
6. Add `[ecosystem]` configuration to `supersigil.toml`:
   ```toml
   [ecosystem.rust]
   ```
7. Implement the CLI feature by feature, following task order
8. Write tests annotated with `#[verifies("doc#criterion")]` using
   `supersigil_rust::verifies`
9. Run `supersigil verify --format json` after each feature to track progress
10. Reach `supersigil verify` clean and `supersigil lint` clean

## Constraints

- Use `clap` for argument parsing (add with `cargo add clap --features derive`)
- Use `std::process::Command` to invoke git (no git library dependency)
- Write integration tests that exercise the CLI against a temporary git repo
- Do not ask for user input — work autonomously
