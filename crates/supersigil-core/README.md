# supersigil-core

Data model, config loader, and built-in component definitions for
[supersigil](https://github.com/jonisavo/supersigil).

This crate provides the foundational types shared across the supersigil
ecosystem: the document graph, component definitions (requirements, criteria,
designs, tasks, etc.), configuration parsing, and cross-reference resolution.

## Usage

```toml
[dependencies]
supersigil-core = "0.1"
```

Most consumers will not depend on this crate directly. It is re-exported by
higher-level crates such as `supersigil-verify` and `supersigil-rust`.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE)
or [MIT License](../../LICENSE-MIT) at your option.
