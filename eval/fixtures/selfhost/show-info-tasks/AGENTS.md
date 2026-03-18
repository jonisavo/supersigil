# Implement: Show Info Findings in Terminal

You are working on the supersigil codebase. Your task is to implement the
"show info findings in terminal" feature.

## Development Workflow

1. Run `supersigil status` to see the project overview
2. Run `supersigil plan show-info` to discover the outstanding tasks
3. Optionally run `supersigil context show-info/req` to understand the requirement
4. Explore the codebase to understand the verify output pipeline:
   - How findings are rendered to terminal
   - How severity levels are handled
   - Where CLI flags are parsed
5. Implement the tasks in order
6. Write tests annotated with `#[verifies("show-info/req#...")]`
7. Run `supersigil verify --format json` to track progress
8. Reach `supersigil verify` clean

## Guidelines

- Follow Test Driven Development
- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting
- Use `cargo nextest run` for testing
- Pragmatic and idiomatic Rust
- Use the new module syntax (`module.rs` with `module/` instead of `module/mod.rs`)

Run all three before finalizing work:

```shell
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo nextest run
```

No warnings or errors should be left.

## Constraints

- Do not ask for user input — work autonomously
- Do not modify existing tests or spec documents outside the show-info feature
- The `--show-info` flag must be off by default (preserve current behavior)
