# supersigil-rust-macros

Proc-macro crate for the [supersigil](https://github.com/jonisavo/supersigil)
Rust ecosystem plugin.

Provides the `#[verifies(...)]` attribute macro that links Rust test functions
to supersigil specification criteria. This crate is not intended to be depended
on directly -- consumers should use
[`supersigil-rust`](https://crates.io/crates/supersigil-rust), which re-exports
the macro.

## Example

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
