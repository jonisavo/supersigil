# Roadmap / Open Questions

- **LSP support**: A language server providing autocomplete on refs,
  go-to-definition, and diagnostics would significantly improve the
  authoring experience. This is a v2 concern but the parser architecture
  should be designed to support incremental re-parsing.

- **Watch mode**: `supersigil verify --watch` for continuous feedback
  during authoring. Requires file watching and incremental verification
  (re-verify only documents whose files or dependencies changed).

- **WASM plugins**: For verification rules that need more than
  stdin/stdout hooks, WASM plugins (via Extism or similar) could
  provide sandboxed, cross-language extensibility. Not planned for v1.

- **Spec generation**: Should `supersigil new` be purely structural
  (template files) or optionally agent-powered (call an LLM to scaffold
  from a prompt)? Likely both, with a `--scaffold` flag.
