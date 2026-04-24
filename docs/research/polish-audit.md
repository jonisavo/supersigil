# Polish Audit

*April 2026 — v0.13.0*

UX gaps, rough edges, and improvement opportunities across the CLI, editors,
documentation, and onboarding experience.

## Config Editing Experience

No JSON Schema exists for `supersigil.toml`. Users editing config in VS Code
or IntelliJ get no autocomplete, no validation, no hover docs. A published
JSON Schema (or TOML-compatible equivalent) would make config authoring much
smoother. The `schema` command exists for component schemas but not the
config file itself. A `supersigil schema --config` could emit this.

