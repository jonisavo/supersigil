# Multi-Project Patterns

Use this reference when working in a monorepo with `[projects.*]` entries
in `supersigil.toml`.

## Project Discovery

Run `supersigil ls --format json` to see which project each document
belongs to. The project assignment comes from the `paths` globs in each
`[projects.<name>]` entry.

## Scoping Commands

Some commands accept `--project <name>` (notably `ls`, `verify`, and
`new`). Others are scoped by document ID or prefix:

```bash
supersigil ls --format json --project backend
supersigil new requirements auth/req --project backend
supersigil plan backend/
supersigil status auth/req
supersigil affected --since HEAD
```

`plan`, `status`, and `affected` do not accept `--project` today.
`plan` and `status` accept an ID or prefix argument to narrow scope.
`affected` has no scoping — it returns all documents whose tracked
files changed, across all projects.

## Cross-Project References

References between documents in different projects are allowed and
verified normally. The graph is workspace-wide even when projects
partition the discovery paths.

When authoring cross-project refs:
- Use the full document ID (e.g., `infra/req#req-1-1`), not a
  project-relative path.
- Run `supersigil verify` after adding cross-project refs to confirm
  the target exists and the reference direction is valid.

## Isolated Projects

A project with `isolated = true` restricts its documents from
referencing documents in other projects. Use this for independently
deployable services or packages that should not have spec-level
coupling.

When working in an isolated project:
- All refs must resolve within the same project.
- `supersigil verify` will flag cross-project refs as errors.
- `supersigil plan <prefix>` narrows output to documents matching
  the prefix, which approximates project scoping.

## Ecosystem Plugin Scoping

The `[ecosystem.rust]` config uses `project_scope` to map Cargo
manifest directories to supersigil projects. This controls which
project a Rust crate's `#[verifies(...)]` annotations are validated
against.

```toml
[ecosystem.rust]
project_scope = [
  { manifest_dir_prefix = "crates/my-crate", project = "foundation" },
]
```

When adding new crates, ensure they have a `project_scope` entry if
the Rust plugin is active and the crate contains test evidence.

## Authoring in Multi-Project Mode

- Use `supersigil new <type> <id> --project <name>` to scaffold
  documents in the correct project directory.
- Keep feature IDs consistent within a project prefix convention.
- When a feature spans projects, create separate spec documents in
  each project and use cross-project `<DependsOn>` or `<References>`
  to express the relationship.
