# Component Quick Reference

Start documents with `supersigil new <type> <feature>`. Use this
reference for component placement and attribute syntax when editing.

Write all list attributes as comma-separated string literals:
`refs="a, b"` and `paths="x, y"`. Do not use JSX expression attributes.
Never leave empty placeholders like `refs=""` or `paths=""`.

## Components by Document Type

### Requirement docs

- `<AcceptanceCriteria>` — wrapper for criterion entries
- `<Criterion id="...">` — one acceptance criterion

```mdx
<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN valid credentials, THE SYSTEM SHALL return a session token.
  </Criterion>
</AcceptanceCriteria>
```

### Design docs

- `<Implements refs="feature/req">` — links to the requirement this design implements
- `<TrackedFiles paths="src/**/*.rs, tests/**/*.rs">` — source files owned by this design
- `<DependsOn refs="other/design">` — document-level ordering

```mdx
<Implements refs="auth/req" />
<TrackedFiles paths="src/auth/**/*.rs" />
```

### Tasks docs

- `<Task id="..." status="..." implements="..." depends="...">` — one task entry

```mdx
<Task id="task-1-1" status="ready" implements="auth/req#req-1-1">
  Implement credential validation.
</Task>
```

### ADR docs

- `<Decision id="...">` — wraps rationale, references, and alternatives
- `<Rationale>` — reasoning (child of Decision)
- `<References refs="...">` — links to criteria (child of Decision)
- `<Alternative id="..." status="rejected|deferred">` — considered option (child of Decision)

```mdx
<Decision id="use-postgres">
  Use PostgreSQL for persistent storage.
  <References refs="infra/req#req-1-1" />
  <Rationale>Mature ecosystem, team expertise.</Rationale>
</Decision>
```

Use `standalone="..."` on decisions with no corresponding requirement.

### Evidence (any doc with criteria)

- `<VerifiedBy strategy="tag" tag="...">` — evidence via tagged tests
- `<VerifiedBy strategy="file-glob" paths="...">` — evidence via test file existence

## Statuses

| Document type | Statuses |
|---------------|----------|
| requirement   | draft -> review -> approved -> implemented |
| design        | draft -> review -> approved |
| tasks         | draft -> ready -> in-progress -> done |
| adr           | draft -> review -> accepted -> superseded |
| task (item)   | draft, ready, in-progress, done |
