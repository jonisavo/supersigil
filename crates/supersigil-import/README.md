# supersigil-import

Import and convert external spec formats into
[supersigil](https://github.com/jonisavo/supersigil) documents.

This crate handles discovering, parsing, and converting specification documents
from other formats (e.g. Kiro specs) into supersigil's native Markdown-based
format. It provides a plan-then-execute workflow: first preview what would be
imported, then write the output files.

## Usage

```toml
[dependencies]
supersigil-import = "0.1"
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE)
or [MIT License](../../LICENSE-MIT) at your option.
