# supersigil-evidence

Shared, language-agnostic evidence types for
[supersigil](https://github.com/jonisavo/supersigil) ecosystem plugins.

This crate provides the normalized evidence model consumed by
`supersigil-verify` and implemented by ecosystem plugins such as
`supersigil-rust` and `supersigil-js`. It does not contain any
ecosystem-specific parsing or discovery logic.

## Usage

```toml
[dependencies]
supersigil-evidence = "0.1"
```

Plugin authors depend on this crate to implement the evidence trait for their
language ecosystem.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE)
or [MIT License](../../LICENSE-MIT) at your option.
