# Design Decisions

Architectural decisions and their rationale. Each entry records the
decision, why it was made, and what alternatives were considered.

## Components carry semantics, not document types

Document types (`requirements`, `design`, `tasks`) are classification
tags for humans and documentation tooling. Supersigil's verification
engine operates on the component graph: `<Criterion>`, `<VerifiedBy>`,
`<Implements>`, etc. This means a single document can contain
any combination of components, and custom workflows can use document
types that supersigil has no built-in knowledge of.

**Implication:** A user who calls their documents "user stories" instead
of "requirements" simply uses `type: user-story` in front matter. As long
as those documents contain `<Criterion>` components, supersigil's coverage
checking works identically. You could, in theory, replace Jira this way.

## Unidirectional references

Design docs point at requirements (via `<Implements>`), not the reverse.
`<VerifiedBy>` links criteria to tests, not the reverse. Supersigil
computes reverse mappings from the forward refs.

**Rationale:** Bidirectional references create a synchronization burden.
Adding a new design doc should never require editing the requirement it
implements. Unidirectional refs make the more abstract artifact (the
requirement) stable while the more concrete artifacts (designs, tests)
evolve around it.

## MDX for structured components in prose

MDX provides actual AST nodes for components (`MdxJsxFlowElement`),
eliminating the need for convention-based parsing of plain markdown.
Components degrade gracefully in non-MDX renderers (the content inside
them is still visible as text). And the ecosystem (Astro, Docusaurus,
Next.js) renders them as actual UI components.

**Tradeoff:** MDX parsing is more complex than plain markdown. The
`markdown` crate (markdown-rs) provides this in Rust, but the MDX
support adds weight. If a future user needs plain-markdown support,
a fallback parser could extract structured data from HTML comments
or fenced code blocks, but this is not planned for v1.

## String-only attributes with comma-separated lists

Supersigil rejects JSX expression attributes (`refs={[...]}`). All
attribute values must be plain string literals. Multi-value attributes
use comma-separated strings (`refs="a, b, c"`).

**Rationale:** JSX expression attributes require either evaluating
JavaScript (heavyweight, unsafe, non-deterministic) or parsing a subset
of JS (underspecified, brittle). Comma-separated strings are trivially
parseable, unambiguous, and work identically across every MDX parser.
The tradeoff is that commas are prohibited in IDs and paths — a
restriction that is enforced by lint and has no practical cost.

## Spec drift: three complementary signals

The most common real-world problem is spec drift: code changes but the
spec that describes it is not updated. Supersigil addresses this with
three mechanisms at different confidence levels:

1. **`<TrackedFiles>` — routing signal.** Declares which source files a
   spec is concerned with. When those files change, `supersigil affected`
   flags the spec as potentially stale. This is a low-cost advisory — it
   says "review this spec", not "this spec is wrong."

2. **Ecosystem annotations — evidence signal.** The Rust plugin discovers
   `#[verifies("doc#criterion")]` attributes on test functions at runtime
   and normalizes them into verification evidence. The `#[verifies]` proc
   macro validates refs against the spec graph at compile time, catching
   stale annotations during `cargo check`. Other ecosystems can provide
   equivalent evidence via the `EcosystemPlugin` trait.

3. **Executable examples — proof signal.** `<Example>` components in specs
   embed runnable code that executes during `supersigil verify`. Passing
   examples with `verifies` produce evidence records that satisfy criterion
   coverage. When the code or spec drifts, the example fails — closing
   the loop from specification to live execution.

Each level trades effort for confidence: `<TrackedFiles>` is cheap and
broad, ecosystem annotations are precise but require annotating tests,
and executable examples are the strongest signal but require maintaining
inline code.

**Alternative considered for TrackedFiles:** Deriving file associations
from test mappings (if a test file changes, the spec it verifies is
potentially stale). Rejected because tests and specs can address
different aspects of the same code, and test file changes don't always
indicate spec drift.

## Tasks as components within documents, not individual documents

Task tracking is a common need in spec-driven development. Supersigil
models tasks as `<Task>` components within a `type: tasks` document,
following the same pattern as `<Criterion>` components within a
requirements document. One tasks document per feature, containing all
its tasks.

**Rationale:** Making each task a separate document would produce 10-20
documents per feature, which is unwieldy. The component model keeps
task granularity inside the document boundary while still making tasks
individually referenceable via fragment syntax
(`auth/tasks/login#adapter-code`). Task *ordering* is verified by
supersigil (cycle detection, topological sort). Task *execution* is
the agent's responsibility — it reads the plan, picks up the next
task, edits the `status` attribute in the MDX, and commits.

**Two levels of dependency:** `depends` on `<Task>` handles ordering
within a tasks document (the common case). `<DependsOn>` at the
document level handles ordering between documents (rarer, e.g., one
design document depends on another).

## Configurable strictness for CI enforcement

Findings are split into two categories. **Hard errors** (broken refs,
duplicate IDs, missing required attributes, expression attributes,
dependency cycles) are structural integrity failures — always fatal,
never configurable. **Configurable rules** have a built-in default
severity that can be overridden.

Four levels of precedence (highest to lowest):

1. **Draft gating** — `status: draft` suppresses to info.
2. **Per-rule overrides** (`[verify.rules]`).
3. **Global strictness** (`[verify] strictness`).
4. **Built-in defaults**.

Unknown rule keys in `[verify.rules]` are config errors to catch typos.

**Rationale:** Hard errors are not negotiable — suppressing them invites
real problems. Making them unconfigurable eliminates the temptation.
For everything else, one vocabulary across all scopes means zero
translation overhead.

## Status-gating: draft documents are not blocked

Documents with `status: draft` have all configurable rules suppressed to
`info` level. Findings still appear in the output (as "would be error if
not draft") but don't fail the build. Hard errors are never suppressed.

**Rationale:** This solves the tension between strict defaults and
iterative authoring. `status: draft` is the mechanism that makes
strictness humane — you write the spec incrementally, and supersigil
tells you what's missing without blocking you. When you promote the
status, the full rule set applies.

## Freeform IDs with optional validation

IDs are declared in front matter and are freeform strings. This is
resistant to AI agent hallucination (agents can use any string) while
remaining correctable (supersigil verify catches broken refs). An
optional `id_pattern` in config lets teams enforce conventions via
warnings.

**Alternative considered:** Deriving IDs from file paths. Rejected
because it couples identity to filesystem layout, making reorganization
a breaking change.

## Test discovery: hardcoded format, ecosystem plugins for depth

The v1 test mapping strategy is explicit file globs: the `<VerifiedBy>`
component declares which files contain relevant tests. This is
language-agnostic and requires no pattern matching.

Tag scanning uses a hardcoded format (`supersigil: {tag}`) that is not
configurable. A single universal convention avoids per-project
bikeshedding and makes tags greppable across any codebase.

For language-native test discovery (AST-level, not comment-level),
supersigil uses ecosystem plugins. The built-in Rust plugin uses `syn`
to find annotated test items and understands proptest. Future plugins
can extend this to other languages.

Test *execution* and pass/fail reporting is handled by consuming
existing test result formats (JUnit XML), not by running tests.
Supersigil is a verification tool, not a test runner.

## Advisory status, not enforced state machines

Statuses (`draft`, `approved`, `verified`, etc.) are informational.
Supersigil reports inconsistencies (e.g., `status: verified` with no
tests) but does not prevent status transitions. Enforcing state machines
in a CLI tool creates friction without proportional value.

## Rust with single-binary distribution

Supersigil is implemented in Rust for single-binary distribution, fast
filesystem traversal, and native MDX parsing via the `markdown` crate.
Pluggability is handled via external process hooks (stdin/stdout JSON),
avoiding the need for a plugin runtime.
