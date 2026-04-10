# supersigil-verify

Verification engine for [supersigil](https://github.com/jonisavo/supersigil)
spec documents.

This crate orchestrates the full verification pipeline: loading the document
graph, discovering evidence from ecosystem plugins (Rust, JS/TS), matching
evidence to criteria, and producing a structured verification report with
coverage, staleness, and traceability information.

## Usage

```toml
[dependencies]
supersigil-verify = "0.1"
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE)
or [MIT License](../../LICENSE-MIT) at your option.
