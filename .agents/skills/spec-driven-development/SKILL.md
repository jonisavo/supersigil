---
name: spec-driven-development
description: Run the full Supersigil workflow from feature idea to verified specs to implementation. Use when the user explicitly wants an end-to-end guided spec-driven development flow rather than only spec authoring or only implementation.
---

# Spec-Driven Development

Use this skill only when the user explicitly wants the full guided Supersigil workflow. This skill owns the sequence. It does not wait for the user to invent the next phase.

## Current Contract

Treat this as a directive wrapper over a conditional planning round plus the two lower-level Supersigil skills:

- a structured planning round in the conversation when the starting request is underspecified, or whenever the user explicitly asks for planning
- `feature-specification` for requirements, properties, design, and tasks
- `feature-development` for implementation against the finished spec graph

If those skills are not available, fall back to the embedded summaries below instead of failing.
Do not author requirements or design from guessed intent when the planning round is required.

## Workflow

1. Scope the feature before creating artifacts.
   Confirm the user goal, success condition, and the feature boundary.
   Keep the scope small enough that one spec graph can stay coherent.
   Decide whether the request is already specific enough to write requirements honestly.

2. Run a planning phase when needed.
   The planning phase is mandatory when the initial request is underspecified.
   The planning phase is also mandatory whenever the user explicitly asks for it, even if the request is otherwise clear.
   If the request is already well specified and the user did not ask for planning, skip this phase and say briefly why it is safe to proceed.
   Ask only the questions needed to remove product, scope, and quality ambiguity that would weaken requirements or design.
   When a structured question tool is available in the current mode, prefer it.
   Otherwise ask concise direct questions in the conversation.
   Focus questions on user outcomes, main scenarios, non-goals, constraints, integrations, failure modes, and verification expectations.

3. Produce and confirm a planning brief when the planning phase runs.
   Summarize the agreed problem statement, in-scope and out-of-scope behavior, major scenarios, constraints, quality risks, and verification approach in the conversation.
   Treat this brief as the source material for requirements and early design choices.
   Get explicit user confirmation or corrections before authoring requirements.

4. Run the specification phase first.
   Use the `feature-specification` workflow to produce or repair the requirement, property, design, and tasks docs.
   Derive requirement criteria from the approved planning brief when one exists.
   Derive design from the reviewed requirement shape plus the confirmed constraints, risks, and verification strategy.
   Keep docs at `status: draft` while the graph is still moving.
   Do not start implementation while the spec graph is still structurally broken.
   If spec authoring reveals missing intent, pause and return to planning instead of guessing.

5. Gate the transition to implementation.
   Move on only when the scoped docs are lint-clean, verify-clean enough for honest handoff, and reviewed with the user.
   Make the transition explicit in the conversation:
   `Specs are complete and verified. Switching to implementation.`

6. Run the implementation phase second.
   Use the `feature-development` workflow to select the next criterion or task chain, implement it, add verification evidence, and keep task states current.
   Promote document statuses as tasks complete: tasks doc to `done`, design to `approved`, requirements to `implemented`.
   The `status_inconsistency` verify rule will catch any missed promotions.

7. Close with the graph, not just the code.
   Summarize the planning brief when one was used, plus `supersigil status`, `supersigil plan`, and `supersigil verify` for the scoped feature.
   If the user stops after specs only, suggest `feature-development` for the next session.

## Embedded Fallback Summary

Use this only when the lower-level skills are unavailable.

### Planning Phase

- Decide whether planning is required before writing any spec docs.
- Run planning when the initial request is underspecified, or whenever the user explicitly asks for a planning phase.
- Ask only enough questions to remove ambiguity that would lower requirement or design quality.
- Prefer a structured question tool when the current collaboration mode supports it; otherwise ask concise direct questions in the conversation.
- Capture a planning brief in the conversation and get user confirmation before authoring requirements when planning was required.

### Specification Phase

- Run `supersigil schema`, `supersigil ls`, `supersigil context`, and `supersigil plan` to inspect the current graph.
- Use `supersigil new` or `supersigil import --from kiro` as the starting point.
- Base requirements and design on the confirmed planning brief when one exists.
- Keep documents in `status: draft` while editing.
- Run `supersigil lint` after every spec write.
- Run `supersigil verify` before handing the graph to implementation.

### Implementation Phase

- Run `supersigil status`, `supersigil plan`, and `supersigil context` before coding.
- Implement one criterion or task chain at a time.
- Add or repair `VerifiedBy` evidence as part of the change.
- Re-run `supersigil verify` before claiming completion.

## Stage Gates

- Do not author requirements or design before a required planning brief has been confirmed.
- Do not continue spec authoring on guessed intent; return to planning when material ambiguity appears.
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
