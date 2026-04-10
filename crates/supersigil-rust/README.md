# supersigil-rust

Rust ecosystem plugin for the
[supersigil](https://github.com/jonisavo/supersigil) verification framework.

This crate provides the Rust-specific integration for supersigil. It handles:

- Parsing criterion targets from `#[verifies(...)]` attributes
- Discovering evidence in Rust source files via `syn`
- Normalizing Rust test results into verification evidence records
- Resolving single-project and multi-project Cargo workspace layouts

The proc-macro from `supersigil-rust-macros` is re-exported here, so consumers
only need to depend on this crate.

## Usage

```toml
[dependencies]
supersigil-rust = "0.1"
```

```rust,ignore
use supersigil_rust::verifies;

#[verifies("REQ-auth#login")]
#[test]
fn test_login_succeeds() {
    // ...
}
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE)
or [MIT License](../../LICENSE-MIT) at your option.
