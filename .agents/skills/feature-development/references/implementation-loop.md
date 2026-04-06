# Implementation Loop

Use this loop when implementing against existing Supersigil specs.

## Preflight

1. Pick one feature prefix or one main document ID.
2. Run:

```bash
supersigil status <main-id>
supersigil plan <feature-prefix>
supersigil context <main-id>
```

3. Choose one outstanding criterion or one task chain to finish next.

If `plan` is empty but the user still expects missing behavior, stop and treat that as a spec problem, not an implementation shortcut.

## Execute One Slice

1. Implement the chosen slice with the user's normal coding skill.
2. If tests change, add or repair the matching `VerifiedBy` evidence.
3. If spec docs change, run:

```bash
supersigil verify
```

4. Re-run:

```bash
supersigil verify
```

5. If `verify` reports scoped errors, fix them before moving on.

## Task Status Rules

- `draft`: the task is not yet ready for execution
- `ready`: the task is actionable but not started
- `in-progress`: active implementation is happening now
- `done`: code, tests, and required spec updates for the task are all in place

Do not skip directly to `done` if verification evidence is still missing.

## Example Command Loop

```bash
supersigil status auth/req
supersigil plan auth
supersigil context auth/req
# implement one slice
supersigil verify
supersigil verify
supersigil status auth/req
```

Use `supersigil affected --since <ref>` when recent source changes may have made nearby tracked docs stale.
