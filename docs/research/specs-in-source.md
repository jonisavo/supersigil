# Specs in Source Code

*April 2026*

Research into a world where supersigil specifications live in source code,
or where as much as possible does. Motivated by reducing spec drift and
improving the AI agent experience.

## Executive Summary

Research across prior art, technical feasibility, architecture, AI agent
experience, industry landscape, trade-offs, dogfooding, discoverability,
regulated-sector feedback, and LSP protocol analysis.

**The opportunity:** No existing tool embeds structured requirement
annotations in source code, links them into a cross-language verification
graph, and checks it for completeness. This gap is real. Inline annotations
would reduce spec drift, cut agent tool calls by ~40%, and simplify skills.

**The risk:** Moving *all* specs into source breaks language independence,
kills prose-rich components, and loses the specify-before-implement workflow.

**The synthesis — the enrichment model:** Rather than source code defining
entire spec documents, source code *enriches* existing Markdown specs.
Markdown documents remain the authoritative documents. Source annotations
contribute additional components — criteria, implements links, tracked-file
declarations — flowing into the same graph nodes. This follows the pattern
`#[verifies]` already established.

**The four-tier model:**

```
Tier 1: Requirements (Markdown)        — what and why
Tier 2: Design (Markdown, lean)         — how things connect, rationale
Tier 3: Implementation specs (source)   — precise verifiable contracts
Tier 4: Tests (source)                  — evidence
```

**The graph-first framing:** The `DocumentGraph` does not care whether a
`SpecDocument` came from Markdown, Rust, JS/TS, or C++. The graph is the
artifact; inputs are pluggable. The graph layer needs zero changes.

**Discoverability is solvable:** The LSP already provides rich spec features
for Markdown. Extending to source files (hover, workspace symbol, code lens,
diagnostics) is technically feasible and works with multi-LSP coexistence in
VS Code and Neovim.

---

## Part 1: Prior Art

### Design-by-Contract

The strongest precedent for specs-in-code. Eiffel (1986) introduced
`require`/`ensure`/`invariant` as first-class syntax. Ada SPARK proved them
statically. Rust's official `#[contracts::requires/ensures]` attributes
(MCP-759, compiler PR #128045) are actively being added to the language,
with ~200 std functions annotated already.

**Key insight:** DbC contracts are *behavioral* specs (boolean predicates
over program state). They express *what* a function promises, not *why* it
exists or *which requirement* it satisfies. Supersigil needs the "which
requirement" link — DbC does not provide it.

### Doc-Comments-as-Specs

Rust doctests (`///` + code blocks) are the gold standard: the example *is*
a test. Python doctest, Elixir doctest follow the same pattern.

**Key insight:** Doctests cover happy-path examples, not edge cases or
non-functional requirements. They demonstrate behavior; they do not assert
traceability.

### Annotation-Driven Traceability (Safety-Critical)

In DO-178C/ISO 26262 workflows, tools like DOORS/Reqtify extract
`@requirement REQ-SAFETY-042` annotations from source code to build
traceability matrices against external requirements databases.

**Key insight:** Only the *link* (requirement ID) lives in code; the
requirement text lives elsewhere. This is exactly supersigil's current
`#[verifies]` pattern — and it is the part that works well.

### BDD Frameworks

RSpec, Spock, Jest `describe/it` blocks read as behavioral specs. But they
lack traceability IDs, rationale, priority, or acceptance criteria metadata.
They describe behavior, not requirements.

### Property-Based Testing

QuickCheck/Hypothesis/proptest properties are executable specifications of
universal invariants. Strong drift resistance (checked on every run). But
they say *what must hold*, not *why it matters* or *which requirement*.

### The Gap in the Landscape

The SDD tools (Kiro, Spec Kit, OpenSpec, Tessl) all use external spec files
with coarse-grained mapping. Rust contracts are inline and verifiable but
behavioral-only. SPDX proves structured source comments can achieve universal
adoption — but only for a narrow domain. **Nobody is building a system that
embeds structured requirement/design annotations in source code at the
function level, links them into a verification graph, and checks that graph
for completeness.**

---

## Part 2: Technical Feasibility

### Rust Mechanisms

Four mechanisms were evaluated. In order of promise:

**1. Declarative macros (`spec!{}`)** — Most expressive.

```rust
supersigil::spec! {
    criterion "login-success" {
        description: "User sees dashboard after valid login",
        verified_by: { strategy: "tag", tag: "test_login_success" },
    }

    implements "auth/req"
    tracked_files ["src/auth/login.rs", "src/auth/session.rs"]
}
```

Expands to nothing (or `const _: () = ();`). The token stream is
freely structured, so the DSL can represent the full component model
including nesting. Extractable via `syn::parse_file()` — supersigil
already uses `syn` for `#[verifies]` discovery. The macro invocation
parser would be a custom `syn::Parse` implementation.

Trade-off: rust-analyzer provides no completions or validation inside
macro invocations. Syntax highlighting is generic.

**2. Proc-macro attributes** — Best for single-criterion annotations.

```rust
#[criterion(id = "login-success", desc = "User sees dashboard after valid login")]
pub fn handle_login(creds: Credentials) -> Result<Dashboard> { ... }
```

Extractable via the existing `syn`-based discovery pipeline. Perfect
tool compatibility (rust-analyzer, clippy, syntax highlighting all
handle custom attributes). Limited to flat key-value pairs — nested
structures require multiple attributes or a mini-DSL inside strings.

**3. Doc comments with structured tags** — Readable but fragile.

```rust
/// @criterion id=login-success
/// User sees the dashboard after entering valid credentials.
/// @verified-by strategy=tag tag=test_login_success
pub fn handle_login(creds: Credentials) -> Result<Dashboard> { ... }
```

Extractable via `syn` (doc comments are `#[doc = "..."]` attributes)
or tree-sitter. No compiler enforcement — typos silently produce no
spec data.

**4. Standalone `specs.rs` files** — Co-located but separate.

A `specs.rs` per module with `#![cfg(supersigil)]` (never compiled).
Essentially reinvents Markdown in Rust syntax. Gains proximity but
loses readability.

**Recommendation for Rust:** `spec!{}` macro for document-level components
+ `#[criterion]` attributes for function-level criteria. Both parsed via
the existing `syn` pipeline.

### JavaScript/TypeScript Mechanisms

**1. Function-call pattern** — Most promising, proven path.

```typescript
import { criterion, decision, spec } from 'supersigil';

criterion("login-success", {
    description: "User sees dashboard after valid login",
    verifiedBy: { strategy: "tag", tag: "test_login_success" },
});
```

Extends the existing `verifies()` extraction. `oxc` already parses
`CallExpression` nodes with `ObjectExpression` arguments. The full
component model (including nesting) is expressible via JS object
literals.

**2. JSDoc/TSDoc structured tags** — Good for function-level.

```typescript
/** @criterion login-success — User sees dashboard after valid login */
export function handleLogin(creds: Credentials): Dashboard { ... }
```

Requires secondary pass to associate comments with AST nodes. IDE
support is excellent. Same fragility concern as Rust doc comments.

**3. TypeScript decorators** — Limited by class-only constraint.

```typescript
@criterion("login-success", "User sees dashboard after valid login")
class AuthService { ... }
```

Cannot decorate standalone functions, `const`, or module-level
expressions. Inapplicable to functional code. TS 5.x support is
solid but the class-only limitation is a deal-breaker for universal
use.

**4. Co-located `.spec.ss.ts` files** — Interesting hybrid.

```typescript
// src/auth/login.spec.ss.ts
export default spec({
    id: "auth/design",
    type: "design",
    status: "approved",
    criteria: [
        criterion("login-success", "User sees dashboard after valid login"),
    ],
});
```

Convention-based discovery. Essentially reinvents Markdown in
TypeScript.

**Recommendation for JS/TS:** Function-call pattern (`criterion()`,
`decision()`, `spec()`) extracted via the existing `oxc` pipeline.

### What to Avoid

- **TypeScript type-level specs** — erased at runtime, awkward syntax,
  no benefit over function calls.
- **Doc-comment-only approaches** — fragile, no enforcement mechanism.
- **Decorators as universal spec carriers** — class-only limitation.

---

## Part 3: Architecture

Three scenarios were analyzed. The verdict is clear.

### Scenario A: Source-Only Specs

All spec information in source annotations. No Markdown.

**Fatal problems:**
- Prose-heavy components (Decision, Rationale, Alternative, multi-paragraph
  criterion descriptions) are painful in annotations.
- Multi-language fragmentation — every language needs its own annotation
  syntax, parser, and LSP integration.
- Loses specify-before-implement workflow — the spec can only exist once
  the source file does.
- Self-hosting paradox — supersigil's own specs would live in parser source
  code, readable only by running the parser.

**Verdict:** Rejected.

### Scenario B: Hybrid (Markdown + Source Annotations)

Two parser frontends feed the same `SpecDocument` type.

**Why it works:**
- The graph layer (`build_graph`) already accepts `Vec<SpecDocument>` — it
  does not care where documents came from.
- Document IDs are unique across the graph; `DuplicateDocumentId` detection
  already prevents conflicts.
- The `doc-id#fragment-id` reference format is string-based and
  language-agnostic — works unchanged.
- Migration is incremental: existing Markdown specs keep working, source
  specs are opt-in.

**Architectural delta:**
- New `SourceSpecExtractor` trait with per-language implementations
  (Rust via `syn`, JS/TS via `oxc`).
- One rule: each document ID has exactly one authoritative source (either
  Markdown or source annotation, never both).
- LSP adds source-file spec awareness incrementally.
- Graph layer: zero changes.

**Verdict:** Recommended.

### Scenario C: Source-Primary, Markdown-Generated

Source annotations are canonical. `supersigil render` generates Markdown.

**Problems:**
- All structured content must be expressible in source annotations.
- Generated Markdown is read-only — edits are lost.
- Adds a rendering layer without verification value.
- Only works for all-developer teams.

**Verdict:** Possible as an optional feature on top of Scenario B, but
should not be the primary model.

---

## Part 4: AI Agent Experience

### The Core Argument

When an agent opens a source file and sees both the implementation and its
acceptance criteria, spec drift becomes harder. Not impossible — agents can
ignore annotations like they ignore TODOs — but the combination of
**proximity** (spec is in the edit buffer) plus **enforcement** (`supersigil
verify` catches drift) is strictly better than either alone.

### Quantified Benefits

| Metric                     | Markdown-only | Hybrid inline |
|----------------------------|---------------|---------------|
| File reads per impl slice  | 4             | 2             |
| Tool calls saved per slice | —             | 2             |
| Tokens saved per slice     | —             | 2,000–4,000   |
| Skill steps (core loop)    | 10            | ~6            |

For Opus-class agents (200k+ context), this is ergonomic. For Haiku-class
agents (8k–32k tokens), it is the difference between fitting the full
context and working blind.

### How the Skill Simplifies

Current `ss-feature-development` skill (10 steps, 6 CLI commands):

1. `supersigil plan` → find outstanding work
2. `supersigil context` → read the requirement
3. Read spec file → see criteria text
4. Read source file → see implementation target
5. Read test file → see existing evidence
6. Implement
7. Annotate tests
8. `supersigil verify`

With inline criteria (steps 2–4 collapse):

1. `supersigil plan` → find outstanding work
2. Read source file → criteria are already there
3. Implement against visible criteria
4. Annotate tests
5. `supersigil verify`

The `context` command becomes less critical. Agents write inline Rust
annotations more reliably than MDX spec files — the format is something
every Rust-trained model handles fluently.

### The Limit

Inline specs absorb the *leaf nodes* (criteria). But structural
relationships — "this design implements that requirement," "this task
depends on that task" — are inherently cross-cutting. They cannot live in
a single source file.

**Optimal split:** Inline the criteria, keep the structure separate. This
is not "inline vs. separate" — it is a two-tier system.

### Code Review

When a PR changes behavior and the inline criterion is in the same file,
the diff shows both together. A reviewer sees "this function changed, and
the criterion it satisfies changed too" — or critically, "this function
changed but the criterion did NOT change." With separate files, the
reviewer must mentally correlate two unrelated-looking diffs.

---

## Part 5: Industry Landscape (2024–2026)

### The SDD Wave

Spec-driven development emerged as a named paradigm. An arXiv paper
(2602.00180) defines three levels: **spec-first** (specs guide then drift),
**spec-anchored** (specs maintained alongside code with automated
enforcement), **spec-as-source** (spec is sole human artifact, code is
generated).

Supersigil is firmly spec-anchored today. The hybrid model keeps it
spec-anchored while making the anchoring tighter.

### Competitors Still Use External Files

- **Kiro** (AWS): `.kiro/specs/` — separate Markdown, no inline annotations
- **Spec Kit** (GitHub): `.specify/` — separate files, coarse traceability
- **OpenSpec** (YC): `openspec/specs/` — separate files, AI-context focus
- **Tessl**: spec-as-source, generates code from spec files (opposite direction)

None embed structured annotations in implementation source code. None build
a verification graph from inline metadata.

### Rust Contracts Are Coming

The official `#[contracts::requires/ensures]` attributes (MCP-759) are
actively being merged into rustc. ~200 std functions annotated. This
establishes a precedent: the Rust ecosystem accepts rich structured
annotations on functions as a normal pattern. Supersigil's
`#[criterion(...)]` would feel native alongside `#[contracts::requires]`.

### The SPDX Precedent

SPDX license identifiers (`// SPDX-License-Identifier: MIT`) prove that
structured source comments can achieve universal adoption for a narrow,
well-defined domain. The key factors: machine-parseable, human-readable,
standardized format, clear value proposition.

### The Gap

No competitor embeds structured annotations in source and builds a
verification graph from them (see Part 1: The Gap in the Landscape).

---

## Part 6: Trade-Offs

### The Case Against (Devil's Advocate)

**Language independence is the fatal risk.** Supersigil already supports
Rust + JS/TS with an explicit plugin architecture for more. Markdown is the
one universal element. In a polyglot project, which language's source is
canonical? Cross-cutting specs (workspace-level requirements, ecosystem
plugin design) have no natural source file to live in.

**Separation of concerns is load-bearing.** Requirements should be writable
and reviewable *before* implementation code exists. If specs live in source,
the spec can only exist once the source file does. You lose the
specify-before-implement workflow that is central to supersigil's value.

**Expressiveness cliff.** A Decision with multi-paragraph Rationale and
multiple Alternatives cannot be compressed into an attribute. There is a
natural line: structural metadata (IDs, references, status) is
annotation-friendly; narrative content (rationale, context, definitions)
is not.

**Tooling complexity multiplier.** Today: one Markdown parser + ecosystem
plugins for evidence. Tomorrow: every plugin must extract spec definitions
too. Testing surface doubles.

**The current split may already be correct.** `#[verifies]` is the bridge
between spec world and code world. It is the only component that naturally
belongs in source because it is fundamentally *about* source. The argument
for moving more into source comes from wanting less distance — but that
distance is a feature.

### The Case For

**Proximity plus enforcement is strictly better.** An agent that sees the
criterion while editing the implementation is less likely to break the
contract. Supersigil's verification catches what proximity misses. Neither
alone is sufficient; together they are.

**The ceremony argument.** Creating a new spec file, choosing an ID,
configuring paths — there is friction. But that friction also ensures
deliberation.

**Differentiation.** No competitor does this (see Part 1: The Gap in
the Landscape).

---

## Part 7: Synthesis and Recommendation

*Note: This initial synthesis was refined by later parts. The enrichment
model (Part 13) and four-tier model (Part 12) supersede the two-tier
model below. The identity model was refined by Part 9 (layering pattern).
Open questions 2 and 4 are addressed by Part 13.*

### The Graph-First Framing

The core architectural insight: **the graph is the artifact; inputs are
pluggable.** The `DocumentGraph` does not care whether a `SpecDocument`
came from Markdown, Rust, JS/TS, or any other source. Each is an input
frontend. The graph is the source of truth for verification, the LSP,
agent skills, and CI.

### Architectural Changes

| Layer | Change required |
|-|-|
| Graph (`build_graph`) | None. Already accepts `Vec<SpecDocument>`. |
| Parser | New `SourceSpecExtractor` trait, per-language implementations. |
| Evidence | Unchanged. `#[verifies]` pipeline already works. |
| Verification rules | Minor: opt-in transitive coverage (Part 9). |
| LSP | Incremental: source-file spec awareness (Part 14). |
| CLI | Unchanged. Commands query the graph. |
| Config | Add source spec discovery to `supersigil.toml`. |

### Migration Path (Revised)

1. **Phase 0 (current):** Markdown specs + `#[verifies]` evidence links.
2. **Phase 1 (enrichment):** Source annotations enrich existing Markdown
   docs — `implements()`, `criterion()`, `tracked_by()`. Extends the
   `#[verifies]` pattern. Existing Markdown specs untouched.
3. **Phase 2 (standalone source docs):** `spec!{}` macro (Rust) and
   `spec()` function (JS/TS) can define entire spec documents in source.
   Layering pattern (Part 9) with opt-in transitive coverage.
4. **Phase 3 (skills + conventions):** Agent skills prefer inline criteria.
   Convention-based mapping as safety net. LSP features on source files.

### Open Questions

1. **Granularity:** Is a `SpecDocument` always a file, or can a single
   source file contain multiple spec documents? (Probably keep one-per-file.)

2. ~~**Partial inline:**~~ Resolved by the enrichment model (Part 13).
   Markdown owns the document; source contributes components.

3. **LSP scope:** Should the LSP provide spec completions inside source-
   language string literals? Technically feasible; the multi-LSP model
   (Part 14) supports it in VS Code and Neovim.

4. ~~**Spec-before-code:**~~ Resolved by the lifecycle-based split
   (Part 13). Requirements stay in Markdown; implementation specs
   come during implementation.

5. **Convention vs. configuration:** Likely configurable, following the
   existing `tests = [...]` pattern in `supersigil.toml`.

---

## Part 8: Dogfooding Analysis

Supersigil has 34 spec documents across 12 feature areas (9 requirements,
9 design, 7 ADRs, 9 task plans). All live under `specs/`. Which are good
candidates for moving inline? Which are not?

### Good Candidates (implementation-close, criterion-heavy)

**ecosystem-plugins/req** (criteria: req-1-1 through req-5-3)
Criteria describe plugin discovery behavior, config model, evidence
extraction. TrackedFiles point directly to `supersigil-rust/src/discover.rs`,
`supersigil-evidence/src/plugin.rs`, etc. These criteria map 1:1 to specific
source modules. Moving `req-1-1` ("plugin discovery SHALL scan configured
test file patterns") inline into `discover.rs` would put the criterion next
to the code it governs.

**js-plugin/req** (criteria: req-1-1 through req-6-5)
Same pattern — criteria about JS test discovery, `verifies()` extraction,
diagnostic reporting. Maps directly to `supersigil-js/`. Each criterion
has a natural home in a specific source file.

**ref-discovery/req** (criteria: req-1-1 through req-4-2)
Criteria about ref listing, filtering, formatting. Maps to
`supersigil-core/src/graph/query.rs` and `supersigil-cli/src/commands/refs.rs`.

**workspace-projects/req** (criteria: req-1-1 through req-4-3)
Criteria about project isolation, config merging, scoped verification.
Maps to `supersigil-core/src/config.rs`.

**Pattern:** Requirements docs whose criteria are tightly coupled to
specific modules — where each criterion describes behavior of a particular
function or subsystem — are strong inline candidates.

### Bad Candidates (cross-cutting, prose-heavy, or planning-oriented)

**All 7 ADRs** — Decision + Rationale + Alternative components are
inherently prose-heavy. `document-format/adr` has 8 decisions with
multi-paragraph rationale. `technology/adr` records the "rust-single-binary"
decision with its justification. These are architectural reasoning artifacts,
not implementation metadata.

**All 9 task documents** — Planning artifacts that exist before (and
independently of) implementation. `decision-components/tasks` has 15 tasks
with status tracking and dependency chains. These are workflow coordination,
not code metadata.

**graph-explorer/req** (criteria: req-1-1 through req-12-3)
Spans multiple languages: Rust CLI (`commands/explore.rs`) + vanilla JS
frontend (`graph-explorer.js`, `detail-panel.js`, `impact-trace.js`) +
Astro pages. No single language owns these criteria. This is exactly the
polyglot problem that makes source-only specs fail.

**onboarding/req** — Spans website + editors + docs. No code anchor.

**spec-rendering/req** — Spans preview package (TS) + editors (TS/Kotlin)
+ website (Astro). Cross-cutting by nature.

### The Interesting Middle Ground: Design Documents

Design docs have `Implements` links to requirements and `TrackedFiles`
pointing to source. They contain both structural metadata (which could go
inline) and architectural prose (which should stay in Markdown).

Example: `ecosystem-plugins/design` implements `ecosystem-plugins/req`,
depends on 7 other design docs, and tracks 11 source files. The structural
links could become inline `Implements` annotations on the relevant modules.
The prose describing *how* the plugin architecture works stays in Markdown.

This suggests a **partial migration** pattern: the design doc keeps its
prose and cross-document links in Markdown, while individual criteria
(if it had any) and `Implements` annotations could move into source.

### Dogfooding Verdict

Of supersigil's 34 spec documents:
- **~4 requirement docs** are strong inline candidates (ecosystem-plugins,
  js-plugin, ref-discovery, workspace-projects)
- **~5 requirement docs** are poor candidates (graph-explorer, onboarding,
  spec-rendering, vscode-extension, decision-components — all cross-cutting)
- **All 7 ADRs** should stay in Markdown
- **All 9 task plans** should stay in Markdown
- **Design docs** could partially migrate (structural links inline, prose stays)

This confirms the two-tier model: roughly half the requirement criteria
have clear code anchors; the other half are inherently cross-cutting.

---

## Part 9: Layering Pattern

### The Idea

Instead of merging inline annotations into existing Markdown documents,
inline annotations create a *separate* document that sits in a layer below
the requirement doc.

```
Markdown requirement doc          Source implementation doc
(auth/req)                        (auth/impl/login)
  Criterion: login-works    <──    Implements: auth/req#login-works
  Criterion: session-mgmt          Criterion: token-validation
                                    Criterion: rate-limiting
                                      │
                                      ▼
                                  Tests: #[verifies("auth/impl/login#token-validation")]
```

### Graph Compatibility: Zero Changes Needed

The graph model was analyzed against actual source code. Key findings:

- `build_graph()` accepts `Vec<SpecDocument>` and indexes by `frontmatter.id`.
  A source-parsed document with `id: "auth/impl/login"` would be indexed
  identically to a Markdown document.
- `resolve_refs()` validates that `Implements refs="auth/req#login-works"`
  resolves: document `auth/req` exists in `doc_index`, fragment `login-works`
  exists in `component_index`. This works regardless of whether either
  document came from Markdown or source.
- `build_reverse_mappings()` stores `implements_reverse: target_doc_id →
  BTreeSet<source_doc_id>`, which would correctly record that
  `auth/impl/login` implements `auth/req`.

**The graph construction pipeline requires zero changes.**

### The Transitive Coverage Question

The critical design decision: does `auth/req#login-works` count as
"covered" when the evidence chain is:

```
auth/req#login-works
    ← Implements ← auth/impl/login#token-validation
        ← #[verifies] ← test_validate_jwt()
```

**Currently: no.** The coverage rule in `coverage::check` does a flat scan:
for each `Criterion`, check `artifact_graph.has_evidence(doc_id, criterion_id)`.
No graph traversal. No transitive resolution.

**The data structures for transitive resolution already exist:**
- `implements_reverse` maps `auth/req` → `{auth/impl/login}`
- `task_implements` maps `(doc_id, task_id)` → `Vec<(target_doc_id, target_fragment)>`

The traversal logic does not exist yet, but the indexes are there.

**Three options for transitive coverage:**

1. **Direct-only (current).** Requirement criteria need their own
   `#[verifies("auth/req#login-works")]`. Safe but requires redundant
   annotations — exactly the problem inline specs aim to solve.

2. **Opt-in transitive.** A `coverage_strategy: transitive` setting on
   the requirement document or per-criterion. Walks `Implements` edges
   to propagate evidence. Gives requirement authors control.

3. **Always transitive.** If an implementation doc `Implements` a
   requirement criterion and the implementation's own criteria have
   evidence, the requirement is covered. Simplest, but risks false
   confidence — the implementation tests may not actually exercise the
   requirement-level behavior.

**Recommendation:** Option 2 (opt-in transitive). This preserves the
current strict-by-default behavior while enabling the layered pattern for
teams that want it. Implementation is bounded: walk `implements_reverse`
for uncovered requirement criteria, check if implementing docs' criteria
have evidence.

### Identity Model

**Explicit IDs are strongly preferred** over path-derived IDs:
- The reference system is string-based (`doc-id#fragment-id`). Derived IDs
  from file paths create tight coupling between file layout and the graph.
- `build_doc_index` rejects duplicates. Path-derived IDs risk collisions
  (e.g., `src/auth/login.rs` and `src/auth/login/mod.rs`).
- Explicit IDs are stable across file moves.

A reasonable compromise: derive a default from the file path but allow
(and recommend) explicit override via `spec!{ document id = "..." }`.

---

## Part 10: Lessons from `#[verifies]` in Practice

### Usage Statistics

460 `#[verifies]` annotations across 1,766 tests (26% coverage).

| Crate | Coverage | Notes |
|-|-|-|
| supersigil-rust | 84% (22/26) | Reference implementation, heavily annotated |
| supersigil-rust-macros | 100% (1/1) | Tiny surface |
| supersigil-lsp | 73% (22/30) | Strong feature-level coverage |
| supersigil-cli | 47% (24/51) | Mixed; some commands well-covered |
| supersigil-verify | 30% (7/23) | Sparse on verification rules |
| supersigil-import | 17% (5/28) | Import logic largely unannotated |
| supersigil-core | 2% (1/45) | Infrastructure, almost no annotations |
| supersigil-parser | 0% (0/11) | Zero annotations |
| supersigil-evidence | 0% (0/6) | Zero annotations |

### Pattern: Adoption Follows Specs

High `#[verifies]` adoption correlates with having detailed requirement
specs. Feature code with requirement docs (rust plugin, lsp, cli) has
47-84% annotation coverage. Infrastructure code without dedicated
requirement docs (core, parser, evidence) has 0-2%.

**Implication for inline specs:** If criteria lived inline in infrastructure
modules, would annotation coverage increase? The hypothesis is yes —
developers (and agents) annotate when the criterion is visible. The
infrastructure code has low coverage not because it is unimportant but
because there is no criterion nearby to reference.

### What Works Well

- **Zero-cost abstraction.** The macro emits the item unchanged. No runtime
  overhead. No resistance from performance-conscious developers.
- **Compile-time validation.** When enabled, the proc macro validates refs
  against the graph at compile time. Broken references are compile errors.
  This is the enforcement mechanism that makes proximity meaningful.
- **Graceful degradation.** Discovery tolerates missing project root;
  compile-time validation skips when config is unreachable. Works in CI,
  in isolation, and in partial checkouts.
- **No visible friction.** Zero TODOs, FIXMEs, or complaints about the
  annotation system in the codebase.

### What Doesn't Work

- **Agents sometimes skip annotations.** This is the user-reported failure
  mode. The `ss-feature-development` skill explicitly includes an "Annotate
  tests" step, and the LSP has code actions for missing attributes — both
  exist because the step is frequently missed.
- **Infrastructure code gets neglected.** 74% of tests have no annotation.
  The gap is not in feature code (where specs drive work) but in
  foundational code (where no spec exists to drive annotations).

### Lessons for Inline Specs

1. **Proximity drives adoption.** The crates with highest annotation coverage
   are the ones where developers think in terms of specs. Inline criteria
   would make more developers think in those terms.
2. **Enforcement matters more than proximity.** Annotations without
   verification are just comments. The compile-time validation is what makes
   `#[verifies]` trustworthy.
3. **The zero-cost pattern is essential.** Any inline spec mechanism must
   expand to nothing at compile time. Performance overhead would kill adoption.
4. **Agents need a safety net.** Explicit annotation is not enough because
   agents sometimes forget. Convention-based mapping is the missing layer.

---

## Part 11: Convention-Based Mapping as a Safety Net

As noted in Part 10, agents sometimes skip `#[verifies]` annotations.
Convention-based mapping is the resilience layer for this failure mode.

### How Inline Specs Enable Module-Scoped Inference

Today, convention-based mapping operates in a global search space: compare
every test name against every criterion ID across the entire project. This
is a combinatorial problem with high false-positive risk.

With inline specs, criteria have a **module address**. If `src/auth/login.rs`
contains `#[criterion(id = "login-success")]` and `tests/auth/login_test.rs`
contains `fn test_login_success()`, the mapping engine can restrict
candidates to the same module path. This transforms the problem from global
fuzzy matching (thousands of candidates) to local fuzzy matching (a handful
per module).

### Three-Tier Evidence Model

| Tier | Mechanism | Confidence | Provenance |
|-|-|-|-|
| 1 (strongest) | Explicit `#[verifies("doc#crit")]` | Certain | `EvidenceKind::RustAttribute` |
| 2 (medium) | Convention match, module-scoped | High | `EvidenceKind::Convention` |
| 3 (weakest) | Convention match, global scope | Lower | `EvidenceKind::Convention` |

Tier 2 only works when inline criteria provide module-level structural
anchors. Without them, only Tier 3 exists — which is why the existing
roadmap correctly marks convention-based mapping as "needs careful design."

### Convention Rules

```
test_login_success  →  normalize  →  login-success  →  match  →  #[criterion(id = "login-success")]
     ↑ same module path ↑                                              ↑ same module path ↑
```

1. Strip prefixes (`test_`, `should_`, `it_`) and suffixes (`_test`, `_spec`)
2. Convert to kebab-case
3. Match against criterion IDs **in the same or parent module**
4. Exact normalized match = 1.0 confidence; substring = 0.9; fuzzy = configurable

### Configuration

```toml
[evidence.conventions]
enabled = true
scope = "module"          # "module" (requires inline criteria) or "global"
confidence_threshold = 0.85
normalize_prefixes = ["test_", "should_", "it_"]
normalize_suffixes = ["_test", "_spec"]
```

### The Synthesis

Inline specs + convention-based mapping form a **defense in depth** for
evidence collection:

- **Layer 1 (inline criteria):** Developer/agent sees the criterion while
  writing code. Proximity nudges correct behavior.
- **Layer 2 (explicit annotation):** `#[verifies]` creates a precise,
  intentional link. Compile-time validated.
- **Layer 3 (convention inference):** Module-scoped name matching catches
  tests that agents forgot to annotate. Clearly labeled as inferred.
- **Layer 4 (verification):** `supersigil verify` checks the graph
  regardless of how evidence arrived.

No single layer is sufficient. Together they cover the realistic failure
modes: developers who forget (Layer 1 reminds), agents who skip annotations
(Layer 3 catches), and drift that slips through (Layer 4 detects).

---

## Part 12: Design Document Drift and the Lean Design Doc

### The Problem

Design documents currently serve a dual purpose:
1. **High-level:** Architecture, rationale, how pieces fit together
2. **Low-level:** Code examples, function signatures, data structures,
   data flow with inline snippets

The low-level content is a copy of what the code says. When the code
changes — a refactor, a renamed function, a restructured data type —
nobody updates the design doc's code examples. These stale examples
become the biggest source of spec drift after a feature is complete.

This is not a discipline problem. It is a structural problem: the design
doc is the only place implementation-level specs can live, so it absorbs
detail that belongs in the source.

### The Solution: Let Source Own Implementation Detail

If source annotations carry implementation-level criteria, `Implements`
links, and structural metadata, design docs can shed their implementation
weight:

| Today (design doc does everything) | With source specs (lean design doc) |
|-|-|
| Architecture + rationale | Architecture + rationale |
| Code examples showing function signatures | *Gone — source IS the signature* |
| Data flow with inline code snippets | *Replaced by graph edges to source specs* |
| "The plugin trait looks like this" | *Lives as criteria on the actual trait* |
| TrackedFiles pointing to 11 source files | *Each source file declares its own identity* |

The design doc becomes a **map** — showing how pieces relate, why this
architecture was chosen, what alternatives were rejected. The **territory**
(the actual implementation contracts) lives in source where it cannot drift.

### Revised Tier Model

The two-tier model from Part 7 becomes a four-tier model with clearer
responsibilities:

```
Tier 1: Requirements (Markdown)        — what and why
Tier 2: Design (Markdown, lean)         — how things connect, architectural rationale
Tier 3: Implementation specs (source)   — precise verifiable contracts
Tier 4: Tests (source)                  — evidence
```

Design docs link to source spec documents via `DependsOn` or `References`
rather than duplicating their content. The graph captures the relationships;
the prose stays high-level and stable.

### Concrete Example: ecosystem-plugins/design Today vs. Tomorrow

**Today** (`ecosystem-plugins/design`): 200+ lines, implements
`ecosystem-plugins/req`, depends on 7 other design docs, tracks 11 source
files. Contains detailed descriptions of how the plugin trait works, how
discovery scans files, how evidence records are produced — all of which
drifts when the code changes.

**Tomorrow:** The design doc keeps:
- The architectural overview (plugin trait design, discovery pipeline concept)
- The rationale for the test-discovery strategy (from the ADR)
- `DependsOn` links to other design docs
- `References` links to source spec docs like `ecosystem-plugins/impl/rust-discover`

The design doc drops:
- Code examples of the plugin trait signature
- Detailed descriptions of discovery logic
- TrackedFiles (each source spec tracks its own files)
- Implementation-level criteria (moved to source)

The implementation detail that currently drifts in Markdown now lives as
`spec()` / `criterion()` annotations in `discover.rs`, `plugin.rs`, etc.
— where changing the code means seeing the spec.

### Impact on Spec Authoring Workflow

The specify-before-implement workflow is preserved but refined:

1. **Before implementation:** Write requirement doc (what/why) and lean
   design doc (architecture, cross-cutting decisions). No code examples.
2. **During implementation:** Add `spec()` and `criterion()` annotations
   to source as you write the code. These become the implementation-level
   specs that `Implements` the requirements.
3. **After implementation:** The design doc remains accurate because it
   never contained implementation detail that could drift. The source
   specs are accurate by construction.

This inverts the current failure mode: instead of design docs accumulating
detail that drifts, they stay lean and stable while source owns what
changes.

---

## Part 13: Regulated Sector Feedback

A colleague working in a heavily regulated sector (robotics/embedded C++)
provided ground-truth feedback on the traceability problem:

### What they said

1. **Test→requirement tracing works:** "In the robot tests it's
   straightforward to add tags to requirements." Their team has solved the
   evidence side (equivalent to `#[verifies]`).

2. **Implementation→requirement tracing doesn't exist:** "The implementation
   doesn't have them at all." The gap supersigil aims to fill.

3. **Isolation is hard:** "It might be hard to isolate the code related to
   different requirements." Requirements often cross-cut multiple modules.

4. **Discoverability concern:** "If it's mixed with the code then it's hard
   to find whatever you are looking for." The in-file navigation problem.

5. **Header/source analogy:** "I could see some separation like how the
   header and the source is separated but still linked together." Specs as
   a parallel file — separate but co-located.

### Revised Lifecycle-Based Split

This feedback suggests a cleaner split than "which components go where."
Split by lifecycle stage instead:

| Stage | Where | Why |
|-|-|-|
| Requirements | Outside (Markdown) | Done first, reviewed before code exists |
| Design + Decisions | In/near source | Owned by implementers, drifts when separate |
| Evidence | In source (tests) | Already works via `#[verifies]` |

Requirements stay external because they're authored by stakeholders before
implementation exists. Design decisions and implementation-level criteria
live in or near source because they're authored by developers during
implementation and drift when maintained separately.

### The Enrichment Model (Revised Phase 1)

Rather than source code defining entire spec documents, source code
*enriches* existing Markdown specs. The Markdown document remains the
document. Source annotations contribute additional components to it:

- `implements("auth/req#login-works")` on a function — adds an Implements
  edge to the existing document's graph node
- `criterion("auth/req#token-validation", "...")` — adds an
  implementation-level criterion to the existing document
- `tracked_by("auth/design")` — declares this file is relevant to a spec
  (reverse of TrackedFiles; source owns the relationship)

This follows the pattern `#[verifies]` already established: the annotation
is metadata on something you're already writing, not a separate authoring
act.

### Additional Feedback: LSP as the Missing Link

After reviewing the LSP-based discoverability approach, the colleague's
reaction was strongly positive:

> "I didn't consider that you can augment the code with LSP to cater for
> the limitations of the language"

> "Just the idea that you click on a code line and wonder why is it there
> and the LSP would provide you the requirement is next level"

> "It would force you to fix the requirements to get to the correct
> outcome in the implementation"

The last point is the strongest validation: if the implementation's
"why" is visible via LSP hover, developers are pushed to keep
requirements accurate — because inaccurate requirements are now visibly
wrong, not silently ignored in a separate file. The specify-before-
implement workflow becomes enforced by tooling, not discipline.

He also noted the idea of a "fully AI native programming language" that
would rely less on LSP/IDE augmentation — suggesting that the gap between
what languages express and what developers need to know is felt acutely
in the regulated sector.

### The Discoverability Problem

The colleague's earlier concern — "hard to find what you're looking for"
— is the main UX challenge. If design decisions are scattered across
source files, how do you get an overview?

This is addressed in Part 14.

---

## Part 14: Discoverability

### The Eight Dimensions

"Hard to find what you're looking for" is not one problem. It is at
least eight distinct activities:

| Dimension | Question | Where metadata lives matters? |
|-|-|-|
| **Overview/Inventory** | "What specs exist?" | No — always needs aggregation |
| **Contextual** | "What spec governs this function?" | Yes — inline wins |
| **Reverse lookup** | "Where is criterion X implemented?" | No — needs an index either way |
| **Neighborhood** | "What specs relate to my current work?" | No — needs graph traversal |
| **Status/Coverage** | "Which criteria are uncovered?" | No — computed from graph |
| **Cross-cutting** | "All auth decisions across all modules?" | Partially — needs search/filter |
| **Temporal** | "What specs changed this sprint?" | Yes — scattered is harder to diff |
| **Authoring** | "Where do I put a new spec?" | Yes — centralized is clearer |

The key insight: **most dimensions don't care where metadata lives.**
They care about the aggregation and navigation layer. Only contextual
discovery (inline wins), temporal tracking (centralized is easier to
diff), and authoring (centralized gives an obvious location) are
meaningfully affected.

### The Universal Pattern from Prior Art

Every successful system with scattered metadata has an aggregation layer:

- **Java annotations + IntelliJ:** PSI index → gutter icons, Find Usages,
  Structure view. Works because the IDE builds a searchable index.
- **C# + Visual Studio CodeLens:** Reference counts, test status, linked
  work items shown above each method. Zero-click contextual discovery.
- **Rust doc comments + rustdoc:** Scattered `///` comments rendered into
  a navigable HTML site. Nobody greps doc comments — they read rustdoc.
- **OpenAPI/Swagger annotations + Swagger UI:** Scattered `@Operation`,
  `@ApiResponse` on controllers → unified interactive API explorer.
  Universally praised despite verbose inline annotations.
- **DOORS + source annotations (safety-critical):** Requirement text in
  DOORS database, lightweight `/* @req REQ-042 */` in code. Reqtify
  builds traceability matrix from both.
- **TODO comments + IDE panels:** Regex-scanned, aggregated in Task List /
  TODO panel. Zero-configuration but unstructured.

**The pattern:** Metadata lives close to code for authoring convenience
and contextual discovery. A tool provides aggregated views for everything
else. No successful system relies solely on "just read the source files."

**Supersigil already has the aggregation layer:** the `DocumentGraph` +
CLI commands + Spec Explorer sidebar + Graph Explorer. The question is
whether the aggregation layer is rich enough for source-embedded specs.

### What the LSP Already Provides

| Feature | Status | Serves |
|-|-|-|
| Document symbols (outline) | Implemented | Contextual (in spec files) |
| Code lenses (ref count + coverage) | Implemented | Status, reverse lookup (in spec files) |
| Hover (component defs, ref targets) | Implemented | Contextual, neighborhood |
| Go-to-definition | Implemented | Reverse lookup |
| Find references | Implemented | Reverse lookup |
| Rename (cross-file) | Implemented | Authoring |
| 7 code action providers | Implemented | Authoring |
| Diagnostics (parse + graph + verify) | Implemented | Status/coverage |
| Spec Explorer tree view | Implemented | Overview/inventory |
| Graph Explorer webview | Implemented | Neighborhood, overview |
| Custom methods (documentList, graphData) | Implemented | Programmatic access |

### What's Missing for Source-Embedded Specs

**workspace/symbol (not implemented):** This is the biggest gap. Typing
"auth" in the workspace symbol picker should return all spec documents and
criteria with "auth" in their ID or description. Currently, only
document-scoped symbols are supported. This would serve overview and
cross-cutting discovery.

**Code lenses on source files (not implemented):** The highest-impact
single improvement. A function with `implements("auth/req#login-works")`
should show a code lens: "implements auth/req#login-works | 3 tests | ✓
verified". Currently, code lenses only appear in `.md` spec files.

**Hover on source annotations (not implemented):** Hovering over
`#[verifies("auth/req#login-works")]` in a `.rs` file should show the
full criterion text, verification status, and related criteria. Currently,
hover only works inside `supersigil-xml` fences in Markdown.

**Inlay hints (not implemented):** Subtle inline text after annotations,
e.g., `#[verifies("auth/req#login-works")]` followed by a dimmed
`// "User sees dashboard after valid login"`. Serves contextual discovery
without requiring hover. Risk: visual noise.

**File decorations (not implemented):** VS Code's `FileDecorationProvider`
could show badges in the file explorer: green for fully verified, yellow
for partial coverage, red for failures. Serves overview/status at the
file-tree level.

**Semantic tokens (not implemented):** Syntax highlighting for spec-
specific constructs: doc IDs, criterion IDs, component names could be
colored distinctly inside `supersigil-xml` blocks and source annotations.

### The Parallel-File Pattern

The regulated-sector colleague suggested a "header/source" model: spec
metadata in a parallel file adjacent to the implementation (`login.rs` +
`login.spec.md`).

**Prior art:**
- C/C++ `.h`/`.cpp` — IDE "Go to Header/Source" is one keypress
- TypeScript `.d.ts` alongside `.js` — pure type declarations
- Storybook `.stories.tsx` — component metadata co-located
- Angular four-file pattern (`.component.ts` + `.html` + `.css` + `.spec.ts`)

**Assessment:** Parallel files occupy a middle ground — more expressive
than inline annotations (full Markdown), more proximate than centralized
specs. But they solve a problem that the LSP already solves better. The
Spec Explorer sidebar, code lenses, hover, and go-to-definition provide
faster, richer discovery than any file-naming convention. A `.spec.md`
next to `login.rs` is marginally easier to find than `specs/auth/req.md`,
but the LSP delivers the spec content without leaving the current file.

**Recommendation:** Invest in LSP features (workspace/symbol, source-file
code lenses, hover on source annotations, file decorations) rather than a
parallel-file convention. The LSP approach scales across all discovery
dimensions; the file convention only helps with overview.

### The Discoverability Roadmap for Source-Embedded Specs

In priority order, what would need to be built:

1. **Code lenses on source files** — Show spec relationships above
   annotated functions. Highest impact, directly addresses "what spec
   governs this function?" This is the CodeLens pattern from Visual
   Studio that developers in regulated sectors already know.

2. **Hover on source annotations** — Show criterion text, status, and
   coverage when hovering over `#[verifies]`, `implements()`,
   `criterion()` in source. Zero-click contextual discovery.

3. **workspace/symbol** — Enable project-wide search for criteria,
   decisions, documents. Addresses the cross-cutting dimension ("show
   me everything about auth").

4. **File decorations** — Badge files in the explorer with spec status.
   Gives overview/status at a glance without opening files.

5. **Inlay hints** — Expand criterion IDs to their descriptions inline.
   Nice-to-have; hover may be sufficient.

Items 1-2 are essential before source-embedded specs would feel
navigable. Items 3-5 improve the experience further but aren't
blockers.

### LSP Protocol and Editor Support Matrix

Research into the full LSP 3.17/3.18 specification and per-editor
implementation status reveals the practical constraints for providing
spec features on source files.

#### Multi-LSP Coexistence (The Critical Question)

Can supersigil's LSP provide code lenses, hover, and inlay hints on
`.rs` files alongside rust-analyzer?

| Editor | Multi-LSP support | How it works |
|-|-|-|
| **VS Code** | Excellent | All providers merged: hover stacked, lenses combined, hints combined, diagnostics namespaced |
| **Neovim** | Good | Multiple clients attach to same buffer. `buf_request_all()` queries all. Most features merge. |
| **IntelliJ** | Limited | LSP API designed for one server per file type. Need native plugin bridging for coexistence with PSI. |
| **Zed** | Partial | Primary/secondary servers. Hover merged. But code lens **not supported at all**. |

**VS Code and Neovim are the viable targets for multi-LSP features.**
IntelliJ needs the existing native plugin approach (already built).
Zed's code lens gap is a hard blocker for the most impactful feature.

#### Per-Feature Editor Support

| LSP Feature | VS Code | IntelliJ | Neovim | Zed | Multi-provider merge? |
|-|-|-|-|-|-|
| **Diagnostics** | ✓ | ✓ | ✓ | ✓ | Yes (all editors) |
| **Hover** | ✓ | ✓ | ✓ | ✓ | Yes (VS Code stacks, others vary) |
| **Code Actions** | ✓ | ✓ | ✓ | ✓ | Yes (VS Code, Neovim) |
| **Completion** | ✓ | ✓ | ✓ | ✓ | Yes (scored/prioritized) |
| **Code Lens** | ✓ | ✓ (2026.1) | ✓ (off by default) | **✗** | Yes (VS Code, Neovim) |
| **Inlay Hints** | ✓ (interactive) | ✓ (2025.2) | ✓ (non-interactive) | ✓ | Yes (VS Code) |
| **Workspace Symbol** | ✓ | ✓ (2025.3) | ✓ | ✓ | Yes |
| **Document Links** | ✓ | ✓ (2025.1) | ✓ (0.12) | **✗** | N/A |
| **Semantic Tokens** | ✓ | ✓ (2024.2) | ✓ (0.12) | ✓ | Custom types supported everywhere |
| **Call/Type Hierarchy** | ✓ | ✓ (2025.3) | ✓ | Unclear | Not useful for specs (wrong semantics) |
| **Custom Requests** | ✓ (via extension) | Difficult | ✓ (buf_request) | Very limited | N/A |

#### The Safe Cross-Editor Feature Set

Features that work across all four editors AND support multi-provider
merging where needed:

1. **Diagnostics** — Universal, always merged, namespaced per server.
   Already implemented. Spec violations on source files would just work.
2. **Hover** — Universal. Supersigil's hover on `#[verifies]` would
   appear alongside rust-analyzer's type info in VS Code.
3. **Code Actions** — Universal. Quick fixes for missing annotations,
   broken refs in source files.
4. **Completion** — Universal. Criterion ID completions inside
   annotation strings.
5. **Workspace Symbol** — Universal. Specs searchable via "Go to Symbol
   in Workspace" (Ctrl+T). High impact for cross-cutting discovery.

Features that work in most editors but have gaps:

6. **Code Lens** — Works in VS Code, IntelliJ (new), Neovim. **Missing
   in Zed.** Highest-impact feature for contextual discovery.
7. **Inlay Hints** — Works everywhere, but interactive label parts
   (clickable navigation) only in VS Code. Display-only elsewhere.
8. **Document Links** — Works in VS Code, IntelliJ, Neovim. Missing
   in Zed.
9. **Semantic Tokens** — Universal. Custom token types (e.g.,
   `supersigilCriterion`, `supersigilRef`) can be defined and themed.

#### Practical Strategy

For source-embedded spec features, implement in this order:

1. **Diagnostics on source files** — Broken refs, uncovered criteria
   visible in Problems panel. Works everywhere. Zero risk.
2. **Hover on source annotations** — Show criterion text when hovering
   `#[verifies("...")]`. Works everywhere. VS Code stacks with
   rust-analyzer hover.
3. **Workspace Symbol** — Register all criteria, decisions, documents
   as searchable symbols. Works everywhere. Addresses the "hard to find"
   concern directly.
4. **Code Lens on source files** — "implements auth/req#login-works |
   verified" above annotated functions. High impact but missing in Zed.
5. **Inlay Hints** — Expand criterion IDs to descriptions. Interactive
   in VS Code, display-only elsewhere.
6. **Semantic Tokens** — Custom token types for spec annotations.
   Visual distinction. Works everywhere.

#### IntelliJ Note

IntelliJ's code lens support only landed in 2026.1. For IntelliJ,
supersigil already has a native plugin (`editors/intellij/`) which can
provide spec features through IntelliJ's extension points directly,
bypassing LSP limitations. The native plugin approach remains the right
strategy for JetBrains IDEs.

#### Zed Note

Zed's missing code lens and document link support, plus its restricted
WASM extension sandbox, make it the weakest target for source-embedded
spec features. Diagnostics, hover, completion, workspace symbol, and
inlay hints all work. Code lens — the highest-impact feature — does not.
This may improve as Zed matures.

---

## Part 15: C/C++ Annotation Mechanisms

Research into how source-embedded specs would work in C/C++, motivated
by a colleague in robotics/embedded C++ who expressed interest.

### The Landscape

C/C++ has no equivalent to Rust's proc macros or JS/TS's function calls
that expand to nothing. The available mechanisms are:

| Mechanism | Compiler interaction | Portable | Extractable | Practical? |
|-|-|-|-|-|
| Structured comments (`// @criterion`) | None | All compilers | tree-sitter, regex | **Yes** |
| Empty-expansion macros (`SS_CRITERION()`) | None (expands to nothing) | All compilers | tree-sitter | **Yes** |
| `__attribute__((annotate("...")))` | Clang only, affects codegen | No | libclang | No |
| `[[supersigil::criterion]]` | Warnings on all compilers | No | tree-sitter | No |
| `#pragma supersigil` | Warnings vary | Partial | regex | Marginal |
| C++26 contracts (`pre`/`post`) | Behavioral only | Future | N/A | Complementary |

### Recommended: Structured Comments (Universal, Not Just C/C++)

A late-stage insight (see Part 16) elevates structured comments from
"C/C++ mechanism" to the **universal mechanism for all languages.** The
same syntax works everywhere:

```c
//supersigil:criterion login-success "User sees dashboard after valid login"
//supersigil:implements auth/req
int handle_login(const credentials_t* creds);
```

This follows the Go directive convention (`//go:generate`, `//go:embed`).
Zero compiler interaction — no warnings, no macros, no headers.

This is what regulated-sector C/C++ developers already do. Every
safety-critical codebase has structured comments for requirement IDs
(`/* @req REQ-SAFETY-042 */`, `/* @satisfy REQ-NAV-103 */`). Each vendor
(LDRA, Reqtify, Parasoft, Polyspace) uses configurable regex extraction.
Supersigil would offer the same pattern with a verification graph behind
it — the missing piece.

### Alternative: Empty-Expansion Macros

```c
#include <supersigil.h>

SS_CRITERION("login-success", "User sees dashboard after valid login")
SS_IMPLEMENTS("auth/req")
int handle_login(const credentials_t* creds);
```

Macros expand to nothing. Greppable, refactorable, natural for C
developers. Requires distributing a `supersigil.h` with empty macro
definitions. Extractable via tree-sitter (`call_expression` nodes).
Precedent: Microsoft SAL (`_In_`, `_Out_`) uses this exact pattern.

### What Doesn't Work

**Custom `[[...]]` attributes:** Both GCC and Clang emit warnings for
unrecognized attributes (`-Wattributes`, `-Wunknown-attributes`).
Regulated codebases treat warnings as errors. P2565 ("Supporting
User-Defined Attributes") proposed standardizing this but was not adopted.

**`__attribute__((annotate(...)))`:** Clang-specific, inserts
`llvm.annotation` intrinsics that affect optimization. A proposed
`annotate_decl` variant (LLVM PR #122431) would fix this but is not
merged.

### The Header/Source Opportunity

C/C++ headers already serve as the API contract. They declare signatures,
types, and constants. Spec annotations in headers mirror this:

```c
// auth.h — the spec contract
//supersigil:criterion login-success "User sees dashboard after valid login"
//supersigil:implements auth/req
int handle_login(const credentials_t* creds);
```

```c
// auth.c — the implementation
#include "auth.h"
int handle_login(const credentials_t* creds) { /* ... */ }
```

This aligns with existing conventions: Doxygen comments go in headers,
SAL annotations go on declarations, C++26 contracts attach to declarations.
In embedded/robotics projects, headers are the stable, reviewed artifact.
Placing spec annotations there means they are visible to every translation
unit and reviewed alongside API changes.

### C++26 Contracts: Complementary, Not Competing

Contracts (`pre`, `post`, `contract_assert`) express behavioral
preconditions/postconditions — *what must hold*. Supersigil criteria
express traceability — *which requirement this satisfies*. They are
orthogonal. A supersigil criterion could reference a contract ("the
precondition on `getWidget` implements criterion `widget-validity`"),
but contracts do not carry traceability IDs.

### Extraction: tree-sitter-cpp

Consistent with planned Python/Go plugins. The `supersigil-treesitter`
shared crate would provide tree-walking utilities. Comments are `comment`
nodes in the tree-sitter extras list; macro calls are `call_expression`
nodes. Association with the next sibling `function_definition` or
`declaration` node gives the spec annotation its code anchor.

### Regulated Sector Prior Art

| Standard | Domain | Tracing mechanism |
|-|-|-|
| DO-178C | Avionics | `/* @req REQ-ID */` comments, LDRA/DOORS extraction |
| ISO 26262 | Automotive | `/* @satisfy SWR-ID */` comments, Parasoft/Polyspace |
| IEC 62304 | Medical devices | Structured comments, vendor-specific tools |
| MISRA C:2025 | Coding rules | Not a tracing standard, but deviation records use structured comments |

The common pattern: **a structured comment with a requirement ID, placed
before the implementing function, extracted by regex.** The requirement
text lives in an external system (DOORS, Jama, Polarion). Only the link
lives in code. This is exactly supersigil's `#[verifies]` pattern — and
the enrichment model would extend it to `implements` and `criterion`.

---

## Part 16: Universal Structured Comments

### The Insight

Structured comments were initially researched as the C/C++ mechanism
(Part 15). But they work in every language — and they simplify the entire
architecture by replacing per-language annotation mechanisms.

Instead of three extraction systems (Rust proc macros via `syn`, JS/TS
function calls via `oxc`, C/C++ comments via tree-sitter), there is one:
tree-sitter comment extraction + directive parsing.

```rust
//supersigil:verifies auth/req#login-works
#[test]
fn test_login_success() { ... }

//supersigil:implements auth/req#login-works
pub fn handle_login(creds: Credentials) -> Result<Session> { ... }
```

```typescript
//supersigil:verifies auth/req#login-works
test('login succeeds', () => { ... });
```

```python
# supersigil:implements auth/req#login-works
def handle_login(creds: Credentials) -> Session: ...
```

Same syntax everywhere. One extraction crate. New language support =
add comment node types to a config table.

### Why Comments Are Not Second-Class

Comments are traditionally untrustworthy because they are unverified. But
with supersigil's validation pipeline, structured comments are functionally
identical to language-specific annotations:

| Capability | Proc macro (`#[verifies]`) | Structured comment (`//supersigil:verifies`) |
|-|-|-|
| Compile-time error on broken ref | Yes (Rust only) | No |
| LSP real-time diagnostic | Not yet | Yes (with LSP extension) |
| CI gate | `supersigil verify` | `supersigil verify` |
| Hover showing criterion text | Not yet | Yes (with LSP extension) |
| Go-to-definition | Not yet | Yes (with LSP extension) |
| Completions for IDs | Not yet | Yes (with LSP extension) |
| Code actions for typos | Not yet | Yes (with LSP extension) |
| Works in all languages | No (Rust + JS only) | Yes |

The proc macro's compile-time validation is the one capability comments
cannot replicate. But it requires finding `supersigil.toml` and parsing
the full graph during compilation — a heavy dependency that only works in
Rust. LSP diagnostics provide equivalent feedback *faster* (while typing,
not at compile time) and universally.

### Why Tree-Sitter (Not Regex)

Tree-sitter provides structural context that regex cannot:

- **Comment association.** Tree-sitter knows that a comment is a sibling
  of a function definition, not just a line in a file. This enables:
  - Validating that `//supersigil:verifies` is on a test function
  - Validating that `//supersigil:implements` is on a function/module
  - Warning when a directive is floating unattached
- **String literal exclusion.** Tree-sitter knows that `//supersigil:`
  inside a string literal is not a comment. Regex would false-positive.
- **Rich diagnostics.** "This `verifies` directive is not attached to a
  test function" — possible with tree-sitter, not with regex.
- **Future extraction.** Tree-sitter opens the door to extracting richer
  information (function signatures, module structure, test framework
  detection) from the same parse.

Comment node types per language:

| Language | Grammar crate | Comment node types |
|-|-|-|
| Rust | `tree-sitter-rust` | `line_comment`, `block_comment` |
| JavaScript/TypeScript | `tree-sitter-javascript`/`typescript` | `comment` |
| Python | `tree-sitter-python` | `comment` |
| C/C++ | `tree-sitter-c`/`cpp` | `comment` |
| Go | `tree-sitter-go` | `comment` |

### Architecture: The `supersigil-directives` Crate

A single shared crate that replaces per-language extraction logic:

```
supersigil-directives
├── tree-sitter comment extraction (universal)
├── directive parser (//supersigil:<directive> <args>)
├── structural association (comment → next sibling node)
├── language config (comment node types per grammar)
└── directive validation (ref format, known directives)
```

This crate would be used by all ecosystem plugins. Eventually, the
existing `syn`-based Rust extraction and `oxc`-based JS extraction could
be migrated to tree-sitter too — one extraction path for all languages.

### Migration Path for Existing Annotations

Since supersigil is currently the only user:

1. Build `supersigil-directives` crate with `verifies` and `implements`.
2. Migrate one Rust crate's `#[verifies]` annotations to
   `//supersigil:verifies` comments. Verify the graph builds correctly.
3. Migrate remaining Rust and JS/TS annotations.
4. Keep the proc macro and Vitest helper as optional backwards-compat
   layers. Deprecate but do not remove.

### Implications for the Enrichment Model

The enrichment model (Part 13) becomes simpler:

- **Before:** Three mechanisms (Rust proc macro, JS function call, C comment)
- **After:** One mechanism (structured comments) for all languages

The directives are:

| Directive | Purpose | Enriches |
|-|-|-|
| `verifies <ref>` | Test evidence link | Adds evidence to graph |
| `implements <ref>` | Implementation traceability | Adds Implements edge |
| `criterion <doc#id> "desc"` | Implementation-level criterion | Adds Criterion to document |
| `tracked-by <doc-id>` | File-to-spec declaration | Adds TrackedFiles (reverse) |

All four follow the same pattern: a comment directive on a source code
element, extracted by tree-sitter, validated by the LSP and verify pipeline,
contributing to the graph.

---

## Appendix: References

### Academic

- "Spec-Driven Development: From Code to Contract" (arXiv:2602.00180)
- "Constitutional Spec-Driven Development" (arXiv:2602.02584)

### Tools & Standards

- Kiro: `.kiro/specs/` — AWS, separate Markdown files
- GitHub Spec Kit: `.specify/` — open source, 84.7k stars
- OpenSpec: `openspec/specs/` — YC-backed, 20k+ stars
- Tessl: spec-as-source, generates code from specs
- Rust contracts (MCP-759, PR #128045): `#[contracts::requires/ensures]`
- SPDX 3.0.1: structured license identifiers in source comments
- MISRA C:2025: recognizes AI-generated code for compliance

### Design-by-Contract Implementations

- Eiffel: `require`/`ensure`/`invariant` (1986)
- Ada SPARK: aspects with GNATprove static verification
- Rust `contracts` crate: proc-macro `#[requires]`/`#[ensures]`
- Microsoft Code Contracts for .NET
- Java JML (Java Modeling Language)
- D language built-in contracts

### Prior Analysis (Martin Fowler)

- "Understanding SDD — Kiro, spec-kit, and Tessl" — characterizes the
  spec-first / spec-anchored / spec-as-source spectrum

### LSP Protocol

- LSP 3.17 Specification (microsoft.github.io)
- LSP 3.18 Specification (microsoft.github.io)
- IntelliJ LSP API: 2023.2–2026.1 feature additions
- Neovim 0.12 built-in LSP client
- Zed language server configuration

### C/C++ Annotations

- Clang Attribute Reference (clang.llvm.org)
- RFC: `annotate_decl` attribute (LLVM Discourse, PR #122431)
- P2565: Supporting User-Defined Attributes (open-std.org)
- C++26 Contracts (P2900, voted March 2026)
- C++26 Reflection (P2996, voted June 2025)
- Doxygen Custom Commands (doxygen.nl)
- tree-sitter-cpp (github.com/tree-sitter/tree-sitter-cpp)

### Safety-Critical Traceability

- DO-178C requirements tracing (LDRA)
- ISO 26262 requirements traceability (Parasoft)
- Reqtify traceability (Dassault Systemes)
- MISRA C:2025 / MISRA C++:2023
