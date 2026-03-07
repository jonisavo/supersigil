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

## Property

```mdx
---
supersigil:
  id: auth/property
  type: property
  status: draft
title: "Login Verification"
---

<Validates refs="auth/req#req-1-1, auth/req#req-1-2" />

Explain the invariant or behavior this document validates.

<VerifiedBy strategy="tag" tag="auth-login" />
```

Use `strategy="tag"` when tests are annotated with `supersigil: auth-login`.
Use `strategy="file-glob"` when concrete test paths are known but tags are not:

```mdx
<VerifiedBy strategy="file-glob" paths="tests/auth/login_test.rs" />
```

Omit `<VerifiedBy>` entirely until you have a real tag or path. Do not leave empty placeholder attributes behind.

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
<DependsOn refs="auth/property" />
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

## Quick Reference

- `<AcceptanceCriteria>`: wrapper for `<Criterion>` entries in requirement docs
- `<Criterion id="...">`: a single acceptance criterion
- `<Validates refs="doc#criterion">`: property or design doc points at requirement criteria
- `<VerifiedBy strategy="tag" tag="...">`: automated verification via tagged tests
- `<VerifiedBy strategy="file-glob" paths="...">`: automated verification via concrete test files
- `<Implements refs="doc">`: design doc points at the spec it implements
- `<Illustrates refs="doc#criterion">`: example doc points at criteria without satisfying coverage
- `<Task id="..." status="..." depends="..." implements="...">`: task entry in a tasks doc
- `<DependsOn refs="doc">`: document-level dependency
- `<TrackedFiles paths="glob">`: source files related to the doc
- Write list attributes as quoted strings, not JSX expressions
