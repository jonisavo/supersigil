# Draft Templates

Use these templates as the current source of truth for common Supersigil documents until `supersigil schema` exists.

Keep all new documents at `status: draft`.

## Requirement

```mdx
---
supersigil:
  id: auth/req/login
  type: requirement
  status: draft
title: "User Login"
---

# User Login

Describe the user-facing requirement in plain language.

<AcceptanceCriteria>
  <Criterion id="valid-creds">
    WHEN a user submits valid email and password,
    THE SYSTEM SHALL return a session token.
  </Criterion>

  <Criterion id="invalid-password">
    WHEN a user submits an incorrect password,
    THE SYSTEM SHALL return a 401 response.
  </Criterion>
</AcceptanceCriteria>
```

## Property

```mdx
---
supersigil:
  id: auth/prop/token-generation
  type: property
  status: draft
title: "Token Generation"
---

<Validates refs="auth/req/login#valid-creds" />

Explain the invariant or behavior this document validates.

<VerifiedBy strategy="file-glob" paths="tests/auth/login_test.rs" />
```

Add `<VerifiedBy>` only when concrete test paths or tags are already known.

## Design

```mdx
---
supersigil:
  id: auth/design/login-flow
  type: design
  status: draft
title: "Login Flow"
---

<Implements refs="auth/req/login" />
<DependsOn refs="auth/prop/token-generation" />
<TrackedFiles paths="src/auth/**/*.rs" />

Describe the implementation approach, boundaries, and tradeoffs.
```

Use `<DependsOn>` only for document-level ordering. Use `<TrackedFiles>` only when the source paths are concrete.

## Tasks

```mdx
---
supersigil:
  id: auth/tasks/login
  type: tasks
  status: draft
title: "Login Tasks"
---

## Overview

Track the implementation sequence for this feature.

<Task id="type-alignment" status="done">
  Align request and domain types.
</Task>

<Task
  id="adapter-code"
  status="in-progress"
  depends="type-alignment"
  implements="auth/req/login#valid-creds"
>
  Implement the adapter layer for credential validation.
</Task>

<Task
  id="switch-over"
  depends="adapter-code"
>
  Swap the old handler for the new one.
</Task>
```

Use `depends` for task ordering inside the same tasks document. Use `implements` for criterion refs only.

## Quick Reference

- `<AcceptanceCriteria>`: wrapper for `<Criterion>` entries in requirement docs
- `<Criterion id="...">`: a single acceptance criterion
- `<Validates refs="doc#criterion">`: property or design doc points at requirement criteria
- `<Implements refs="doc">`: design doc points at the spec it implements
- `<Illustrates refs="doc#criterion">`: example doc points at criteria without satisfying coverage
- `<Task id="..." status="..." depends="..." implements="...">`: task entry in a tasks doc
- `<DependsOn refs="doc">`: document-level dependency
- `<TrackedFiles paths="glob">`: source files related to the doc
- `<VerifiedBy strategy="file-glob" paths="...">`: provisional test mapping until `verify` exists
