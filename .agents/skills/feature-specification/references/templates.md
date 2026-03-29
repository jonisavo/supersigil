# Draft Templates

Start with `supersigil new <type> <feature>` when creating a new document set.
Use these templates when the scaffold is too minimal, when imported docs need to be normalized, or when you need a richer example while editing by hand.

Keep new or actively edited documents at `status: draft` until `supersigil verify` and human review justify promotion.
Write list attributes as comma-separated string literals like `refs="a, b"` and `paths="x, y"`. Do not use JSX expression attributes like `refs={["a"]}`; `supersigil lint` rejects them.

## Requirement

```mdx
---
supersigil:
  id: auth/req
  type: requirement
  status: draft
title: "User Login"
---

# User Login

Describe the user-facing requirement in plain language.

<AcceptanceCriteria>
  <Criterion id="req-1-1">
    WHEN a user submits valid email and password,
    THE SYSTEM SHALL return a session token.
  </Criterion>

  <Criterion id="req-1-2">
    WHEN a user submits an incorrect password,
    THE SYSTEM SHALL return a 401 response.
  </Criterion>
</AcceptanceCriteria>
```

## Design

```mdx
---
supersigil:
  id: auth/design
  type: design
  status: draft
title: "Login Flow"
---

<Implements refs="auth/req" />
<TrackedFiles paths="src/auth/**/*.rs, tests/auth/**/*.rs" />

Describe the implementation approach, boundaries, and tradeoffs.
```

Use `<DependsOn>` only for document-level ordering.
Use `<TrackedFiles>` only when the source paths are concrete.
If a relation target is not known yet, omit that component until the target exists.

## Tasks

```mdx
---
supersigil:
  id: auth/tasks
  type: tasks
  status: draft
title: "Login Tasks"
---

## Overview

Track the implementation sequence for this feature.

<Task
  id="task-1-1"
  status="ready"
  implements="auth/req#req-1-1"
>
  Implement the adapter layer for credential validation.
</Task>

<Task
  id="task-1-2"
  status="ready"
  depends="task-1-1"
  implements="auth/req#req-1-2"
>
  Handle incorrect password responses and error mapping.
</Task>
```

Use `depends` for task ordering inside the same tasks document. Use `implements` for criterion refs only.

## ADR (Architectural Decision Record)

```mdx
---
supersigil:
  id: infra/adr
  type: adr
  status: draft
title: "Use PostgreSQL"
---

# Use PostgreSQL as the Primary Data Store

<Decision id="use-postgres">
  Use PostgreSQL for all persistent storage.

  <References refs="infra/req#req-1-1" />

  <Rationale>
    Mature ecosystem, strong JSONB support, team expertise.
  </Rationale>
</Decision>

<Decision id="reject-mysql" standalone="Evaluated as part of use-postgres">
  <Alternative id="use-mysql" status="rejected">
    MySQL was considered but lacks some PostgreSQL extensions we rely on.
  </Alternative>
</Decision>
```

ADR statuses are `draft`, `review`, `accepted`, `superseded`.
Use `<References>` inside a `<Decision>` to link to requirement criteria.
Use `standalone="..."` when a decision has no corresponding requirement.
Use `<Alternative>` with `status="rejected"` or `status="deferred"` for considered options.

## Example (Executable)

```mdx
---
supersigil:
  id: auth/examples
  type: documentation
  status: draft
title: "Login Examples"
---

<Example id="login-curl" lang="bash" runner="sh" verifies="auth/req#req-1-1">
```bash
curl -X POST /api/login -d '{"user":"admin","pass":"secret"}'
```
<Expected status="0" format="json">
{"token": "..."}
</Expected>
</Example>
```

Use `verifies` to link an example to criteria it demonstrates.
Use `references` for informational links that do not satisfy coverage.
The `runner` attribute determines how the example is executed. Built-in
runners are `sh`, `cargo-test`, and `http`. Custom runners can be defined
in config.

## Quick Reference

- `<AcceptanceCriteria>`: wrapper for `<Criterion>` entries in requirement docs
- `<Criterion id="...">`: a single acceptance criterion
- `<VerifiedBy strategy="tag" tag="...">`: automated verification via tagged tests
- `<VerifiedBy strategy="file-glob" paths="...">`: automated verification via concrete test files
- `<Implements refs="doc">`: design doc points at the spec it implements
- `<References refs="doc#criterion">`: informational traceability link (no verification semantics)
- `<Task id="..." status="..." depends="..." implements="...">`: task entry in a tasks doc
- `<DependsOn refs="doc">`: document-level dependency
- `<TrackedFiles paths="glob">`: source files related to the doc
- `<Decision id="..." standalone="...">`: architectural decision in an ADR doc
- `<Rationale>`: reasoning behind a decision (child of `<Decision>`)
- `<Alternative id="..." status="...">`: considered alternative (child of `<Decision>`)
- `<Example id="..." runner="..." verifies="...">`: executable code example
- `<Expected status="..." format="...">`: expected output (child of `<Example>`)
- Write list attributes as quoted strings, not JSX expressions
