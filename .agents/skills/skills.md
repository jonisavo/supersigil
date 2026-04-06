# Supersigil Skills

Agent skills for spec-driven development with supersigil. Each skill is
self-contained and can be installed independently.

## Skills

### `feature-specification`

**Augments:** brainstorming, planning, and requirements-gathering workflows.

**When to use:** The user has figured out (or is figuring out) what to build
and wants to capture decisions as verifiable supersigil documents.

**What it adds:** The supersigil document format, component grammar, ID
conventions, and the lint-based feedback loop. Teaches agents how to author
valid requirement, design, and tasks documents. Does not prescribe
how to brainstorm or plan — it picks up after that and gives the output a
durable, verifiable home.

**Stance:** Augmenting. Works alongside whatever planning or brainstorming
skills the user already has installed.

**Handoff:** When specification is complete (all documents lint-clean, user
has reviewed), suggest the user activate `feature-development` to begin
implementation against the specs, if the skill exists.

---

### `feature-development`

**Augments:** implementation workflows (TDD, code review, etc.).

**When to use:** Specs exist. The user wants to implement against them.

**What it adds:** The supersigil context/plan/verify loop. Teaches agents
how to read `supersigil plan` to find outstanding work, pick up tasks, tag
tests with `supersigil:` annotations, update task statuses, and run
`supersigil verify` to confirm coverage. Does not own the implementation
methodology — if the user has a TDD skill, feature-development layers on
top of it.

**Stance:** Augmenting. Works alongside whatever implementation skills the
user already has installed.

**Handoff:** When the user describes a new feature idea without existing
specs, suggest `feature-specification` (or `spec-driven-development` for
the full guided flow) to create the spec documents first. When the user
wants to specify existing code that has no specs, suggest
`retroactive-specification`.

---

### `spec-driven-development`

**Augments:** nothing — this is the full omakase flow.

**When to use:** Explicitly invoked. The user wants the complete
spec-driven development experience: idea → verified spec → implementation.
This is the replacement for Kiro's spec workflow.

**What it adds:** Owns the entire lifecycle. Walks the agent through
requirements elicitation, criteria authoring, design,
task planning, and implementation — all producing supersigil documents
verified at each step. Opinionated and sequential.

**Stance:** Directive. Meant to be invoked explicitly when the user wants
a guided, end-to-end workflow. Can (and should) delegate to
feature-specification and feature-development internally, composing them
into a single coherent flow.

**Composition:** Uses `feature-specification` for the spec authoring phase
(requirements → design → tasks), then transitions to
`feature-development` for the implementation phase (plan → implement →
verify). The agent should make this transition explicit to the user:
"Specs are complete and verified. Switching to implementation."

---

### `refactoring`

**Augments:** implementation and code-quality workflows.

**When to use:** Specs exist and tests pass. The user wants to restructure
code (extract modules, rename abstractions, reorganize files) without
changing behavior.

**What it adds:** A verification-preserving refactoring loop. Teaches
agents to snapshot the current `verify` state, make structural changes in
small steps, update `TrackedFiles` and `VerifiedBy` paths as code moves,
and confirm the spec graph stays green throughout. Does not change criteria,
requirements, or design intent.

**Stance:** Augmenting. Works alongside whatever refactoring or code-quality
skills the user already has installed.

**Handoff:** If the refactoring reveals missing or wrong specs, suggest
`retroactive-specification` or `feature-specification`. If the refactoring
is preparation for a new feature, suggest `feature-development` or
`spec-driven-development`.

---

### `ci-review`

**Augments:** CI pipeline and code review workflows.

**When to use:** The user wants to integrate Supersigil verification into
CI pipelines, review PRs against spec coverage, or interpret verification
output in automated contexts.

**What it adds:** Patterns for using `supersigil verify`, `affected`, and
`status` in CI gates and PR reviews. Teaches agents how to scope
verification to PR changes, interpret finding severities, generate coverage
reports, and flag spec drift before it reaches the main branch.

**Stance:** Augmenting. Works alongside whatever CI or review skills the
user already has installed.

**Handoff:** If CI reveals broken specs, suggest `feature-specification`
or `retroactive-specification`. If CI reveals implementation gaps, suggest
`feature-development`.

---

### `retroactive-specification`

**Augments:** code exploration, documentation, and architecture-recovery
workflows.

**When to use:** A brownfield project exists with working code but no
supersigil specs. The user wants to create verified specifications that
capture the current behavior — either to document what exists, to
establish a baseline before refactoring, or to bring an existing codebase
under spec-driven governance. Also use when existing specs have gone stale
and need to be reconciled with the current codebase.

**What it adds:** A structured, incremental approach to reverse-engineering
specifications from existing code. The agent:

1. **Scopes the work.** Asks the user which area of the codebase to
   specify (a module, a feature, a service boundary). Does not attempt
   to specify the entire project at once.

2. **Gathers sources of truth.** Asks the user for existing documentation
   the agent can consult: READMEs, ADRs, API docs, OpenAPI specs, inline
   doc comments, wiki pages, issue trackers. Uses these as primary input
   before reading code.

3. **Reads code incrementally.** Explores the scoped area: public APIs,
   type signatures, test suites (existing tests are a gold mine for
   inferring intended behavior). Works module-by-module, not all at once.

4. **Asks clarifying questions.** When intent is ambiguous from code and
   docs alone, asks the user: "Is this behavior intentional or accidental?
   Should the spec capture it as-is or flag it as a known issue?" This is
   critical — brownfield code often has behavior that nobody intended.

5. **Drafts specs iteratively.** Produces requirement documents with
   criteria derived from observed behavior, using `status: draft`. Each
   document covers one bounded area. The user reviews before the agent
   moves to the next area.

6. **Links to existing tests.** Uses `<VerifiedBy>` with `file-glob` or
   `tag` strategy to connect criteria to tests that already exist. Runs
   `supersigil verify` to surface coverage gaps — criteria with no
   backing tests become visible immediately.

7. **Identifies gaps.** Reports what the code does that has no tests, and
   what the tests cover that has no spec. These gaps are the value
   proposition: the user now sees exactly where the project's
   specification debt lives.

**Scaling strategy:** Works one bounded area at a time. The agent should
propose a traversal order (e.g., "start with the public API surface, then
move to internal services, then infrastructure") and get user agreement
before proceeding. Each area produces its own set of documents. Cross-area
refs are added as the graph grows.

**Stance:** Guided but collaborative. The agent drives the process but
defers to the user on intent. It never assumes behavior is correct just
because it exists in code.

**Handoff:** Once a set of specs is drafted and reviewed, suggest
`feature-development` for ongoing work against those specs. If the user
wants to refactor or fix issues discovered during specification, suggest
`spec-driven-development` for the fix/refactor cycle, or `refactoring`
if the change is purely structural.

---

## Design Principles

- **Self-contained.** Each skill folder contains everything an agent needs.
  No shared folders or cross-skill dependencies at the file level.

- **Augmenting by default.** feature-specification and feature-development
  layer on top of existing skills (brainstorming, TDD, etc.) rather than
  replacing them. They add the supersigil dimension without hijacking the
  user's preferred workflow.

- **Omakase when asked.** spec-driven-development is the exception — it
  owns the direction because the user explicitly asked for it.

- **Pit of success.** Skills should make it harder to produce invalid
  documents than valid ones. Key mechanisms:
  - Always start documents as `status: draft` (suppresses configurable
    verification rules while iterating)
  - Run `supersigil verify` after every write (immediate structural feedback)
  - Run `supersigil verify` before promoting status (full graph check)
  - Use `supersigil schema` to get current component definitions (never
    guess component names or attributes)

- **CLI as the tool.** Skills refer to the `supersigil` CLI for feedback
  and verification rather than bundling scripts. The CLI is the single
  source of truth for what's valid.

## Structure

```
skills/
├── skills.md                        # This file
├── feature-specification/
│   └── SKILL.md
├── feature-development/
│   └── SKILL.md
├── spec-driven-development/
│   └── SKILL.md
├── retroactive-specification/
│   └── SKILL.md
├── refactoring/
│   └── SKILL.md
└── ci-review/
    └── SKILL.md
```

Each skill may include additional reference files alongside SKILL.md if
needed (e.g., a component quick-reference). The SKILL.md is the entry
point and can refer to companion files with `@filename.md` syntax.
