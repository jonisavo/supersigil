---
name: spec-driven-development
description: Run the full Supersigil workflow from feature idea to verified specs to implementation. Use when the user explicitly wants an end-to-end guided spec-driven development flow rather than only spec authoring or only implementation.
---

# Spec-Driven Development

Use this skill only when the user explicitly wants the full guided Supersigil workflow. This skill owns the sequence. It does not wait for the user to invent the next phase.

## Current Contract

Treat this as a directive wrapper over the two lower-level Supersigil skills:

- `feature-specification` for requirements, properties, design, and tasks
- `feature-development` for implementation against the finished spec graph

If those skills are not available, fall back to the embedded summaries below instead of failing.

## Workflow

1. Scope the feature before creating artifacts.
   Confirm the user goal, success condition, and the feature boundary.
   Keep the scope small enough that one spec graph can stay coherent.

2. Run the specification phase first.
   Use the `feature-specification` workflow to produce or repair the requirement, property, design, and tasks docs.
   Keep docs at `status: draft` while the graph is still moving.
   Do not start implementation while the spec graph is still structurally broken.

3. Gate the transition to implementation.
   Move on only when the scoped docs are lint-clean, verify-clean enough for honest handoff, and reviewed with the user.
   Make the transition explicit in the conversation:
   `Specs are complete and verified. Switching to implementation.`

4. Run the implementation phase second.
   Use the `feature-development` workflow to select the next criterion or task chain, implement it, add verification evidence, and keep task states current.

5. Close with the graph, not just the code.
   Summarize `supersigil status`, `supersigil plan`, and `supersigil verify` for the scoped feature.
   If the user stops after specs only, suggest `feature-development` for the next session.

## Embedded Fallback Summary

Use this only when the lower-level skills are unavailable.

### Specification Phase

- Run `supersigil schema`, `supersigil ls`, `supersigil context`, and `supersigil plan` to inspect the current graph.
- Use `supersigil new` or `supersigil import --from kiro` as the starting point.
- Keep documents in `status: draft` while editing.
- Run `supersigil lint` after every spec write.
- Run `supersigil verify` before handing the graph to implementation.

### Implementation Phase

- Run `supersigil status`, `supersigil plan`, and `supersigil context` before coding.
- Implement one criterion or task chain at a time.
- Add or repair `VerifiedBy` evidence as part of the change.
- Re-run `supersigil verify` before claiming completion.

## Stage Gates

- Do not implement before the spec phase is genuinely ready.
- Do not keep the user in this wrapper if they only want one phase.
  If they only want spec authoring, use `feature-specification`.
  If they only want implementation against existing specs, use `feature-development`.
  If they want to recover specs from existing code, use `retroactive-specification`.

## Handoff

Use this skill to get through the full flow, then hand future work to the narrower skill that matches the next task:

- `feature-specification` for further spec edits
- `feature-development` for implementation follow-up
- `retroactive-specification` for brownfield capture work
