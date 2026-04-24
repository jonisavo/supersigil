# supersigil-js

JavaScript/TypeScript ecosystem plugin for the
[supersigil](https://github.com/jonisavo/supersigil) verification framework.

This crate provides the JS/TS integration for supersigil. It handles:

- Filtering JS/TS files from the shared test-file baseline
- Relying on the verification engine's shared resolver for `.gitignore`
  and `test_discovery.ignore` behavior
- Parsing `verifies()` calls from test files via `oxc`
- Normalizing JS/TS test results into verification evidence records

## Usage

```toml
[dependencies]
supersigil-js = "0.1"
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE)
or [MIT License](../../LICENSE-MIT) at your option.
