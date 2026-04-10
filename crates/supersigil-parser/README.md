# supersigil-parser

Parsing pipeline for [supersigil](https://github.com/jonisavo/supersigil)
spec documents.

Documents use standard Markdown with `supersigil-xml` fenced code blocks for
component markup. This crate turns those documents into a structured
representation that the rest of the supersigil toolchain can analyze and verify.

## Usage

```toml
[dependencies]
supersigil-parser = "0.1"
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE)
or [MIT License](../../LICENSE-MIT) at your option.
