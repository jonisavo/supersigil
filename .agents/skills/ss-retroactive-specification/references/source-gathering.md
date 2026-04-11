# Source Gathering

Use this order when recovering specs from an existing codebase.

## Evidence Order

1. Existing product and architecture docs
2. Public API definitions and type signatures
3. Existing tests
4. Internal implementation details
5. Recent changes that `supersigil affected --since <ref>` or git history suggest are relevant

Prefer higher-order evidence before lower-order evidence when they conflict.

## What to Extract

- User-visible behavior
- Stable API contracts
- Existing invariants or quality properties
- Known gaps between tests and implementation
- Concrete source files worth putting in `TrackedFiles`

## Ambiguity Questions

Ask these when the code and surrounding evidence do not line up:

- Is this behavior intentional or accidental?
- Should the spec capture this behavior as-is, or call it out as a known issue?
- Is the test the intended contract, or is the test outdated?
- Should this be documented as current behavior, or treated as part of an upcoming change?

## Output Expectations

Each bounded area should end with:

- draft Supersigil docs for the observed behavior
- linked evidence where real tests exist
- a short list of uncovered behavior and unresolved intent questions
