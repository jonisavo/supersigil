# Status Lifecycle

Use this reference when promoting or checking document statuses.

## Document Type Statuses

| Document Type | Statuses (in order) |
|---------------|---------------------|
| requirements  | `draft` → `review` → `approved` → `implemented` |
| design        | `draft` → `review` → `approved` |
| tasks         | `draft` → `ready` → `in-progress` → `done` |
| adr           | `draft` → `review` → `accepted` → `superseded` |

## Task-Level Statuses

| Status        | Meaning |
|---------------|---------|
| `draft`       | Not yet ready for execution |
| `ready`       | Actionable but not started |
| `in-progress` | Active implementation happening now |
| `done`        | Code, tests, and spec updates all in place |

## Promotion Rules

When all tasks in a tasks doc reach `done`:

1. Set the tasks doc to `status: done`
2. Set the sibling design doc to `status: approved`
3. Set the sibling requirements doc to `status: implemented`

When some tasks are `done` but others remain:

1. Set the tasks doc to `status: in-progress`

Run `supersigil verify` after any status change — the `status_inconsistency`
rule will warn about missed promotions or inconsistent sibling statuses.

## During Authoring

- Keep documents at `status: draft` while actively editing.
- Do not promote based only on `supersigil lint` passing.
- Promote to `review` or `approved` only after `supersigil verify` is clean
  and the user has reviewed the document.
- `draft` suppresses configurable verification warnings, so it is the safe
  working state for iterative authoring.

## Status and Verification Interaction

Supersigil has two layers of error handling that interact with document
status differently:

- **Graph-build errors** (broken refs, cycles, duplicate IDs) are always
  fatal. They prevent the graph from loading and cause `verify` to fail
  regardless of document status. Draft gating does not apply to them.
- **Verification findings** (coverage gaps, staleness, status
  inconsistencies, etc.) go through severity resolution. When a document
  is `status: draft`, all its verification findings are unconditionally
  downgraded to `info`. This makes `draft` the safe working state for
  iterative authoring — you see the findings but they do not block.

On non-draft documents, finding severity follows a 4-level precedence:
per-rule config override > global strictness > built-in default.

- `status_inconsistency` checks that sibling documents have compatible
  statuses (e.g., tasks `done` but requirements still `draft` is flagged).
- `supersigil status <id>` shows the current health including coverage
  and staleness for a specific document.
