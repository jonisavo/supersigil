# Guidelines

- Follow Test Driven Development. Keep TDD in mind when creating implementation plans.
- For feature development (i.e. implementation), use the feature-development and test-driven-development skills.
- Use `cargo fmt` for formatting code
- Use `cargo clippy` for linting.
- Use `cargo nextest run` for testing.
- When bootstrapping a new worktree, use `mise trust` and `mise setup'.

Run all three before finalizing work:

```shell
cargo run -p supersigil verify
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo nextest run
```

No warnings or errors should be left.

# Style

- Pragmatic and idiomatic Rust.
- Use the new module syntax (so `module.rs` with `module/` instead of `module/mod.rs`).
