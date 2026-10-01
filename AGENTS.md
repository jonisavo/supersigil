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
`docs/architecture.md` and `docs/research/multiverse-review-design.md` before
changing the model. The guiding rule: make development history easier to
inspect without making it look more certain than the evidence allows.

Keep plans, specs, and execution notes in the gitignored `.planning/`
directory. Keep durable documentation of behavior and design constraints in
`docs/`, separate from those working documents.

Keep `docs/research/multiverse-review-design.md` tracked for now; it retains
product decisions not yet covered by the architecture document.

# Style

- Pragmatic and idiomatic Rust.
- Use the new module syntax (so `module.rs` with `module/` instead of `module/mod.rs`).
- Every `pub` item has a doc comment; `Result`-returning functions document
  their errors.
