# Specs in Source: Decision Summary

*April 2026 — distilled from [specs-in-source.md](specs-in-source.md)*

## Context

Supersigil builds a verification graph from Markdown spec files and checks
that tests cover criteria. Source code currently participates via
per-language annotations: `#[verifies("doc-id#criterion-id")]` (Rust proc
macro) and `verifies("doc-id#criterion-id")` (Vitest helper). The question:
should source contribute *more* to the graph — criteria, implements links,
design decisions — and if so, how?

**Motivations:** Reduce spec drift (especially in design docs that
accumulate stale code examples). Improve the AI agent experience (agents
see specs while editing code, fewer tool calls). Fill a gap no competitor
occupies.

**Constraints:** Must preserve language independence. Must preserve the
specify-before-implement workflow for requirements and design intent. Later
implementation-level criteria may live in source, but source should not
replace Markdown as the place where user-facing requirements are reviewed.
Must be discoverable (the "hard to find" concern from regulated-sector
feedback). Must work with existing Markdown specs incrementally.

---

## Key Insight: Structured Comments as Universal Mechanism

A late-stage insight that simplifies the entire picture: **structured
comments work in every language.** Instead of per-language annotation
mechanisms (Rust proc macros, JS function calls, C comment directives),
use one syntax everywhere:

```rust
//supersigil:verifies auth/req#login-works
#[test]
fn test_login_success() { ... }

//supersigil:implements auth/req#login-works
pub fn handle_login(creds: Credentials) -> Result<Session> { ... }

//supersigil:criterion auth/req#token-shape "Session tokens are signed JWTs"
pub fn create_session(user: &User) -> Session { ... }
```

```typescript
//supersigil:verifies auth/req#login-works
test('login succeeds with valid credentials', () => { ... });

//supersigil:implements auth/req#login-works
export async function handleLogin(creds: Credentials): Promise<Session> { ... }
```

```python
# supersigil:verifies auth/req#login-works
def test_login_success(): ...

# supersigil:implements auth/req#login-works
def handle_login(creds: Credentials) -> Session: ...
```

```go
//supersigil:implements auth/req#login-works
func HandleLogin(creds Credentials) (Session, error) { ... }
```

```cpp
//supersigil:implements auth/req#login-works
int handle_login(const credentials_t* creds);
```

**Why this works:**
- One syntax to learn, one syntax to document.
- One directive grammar: tree-sitter can find comment nodes, then the shared
  parser extracts directives. Per-language work remains for comment carriers
  and directive-to-code association.
- New language support should mean adding comment extraction and semantic
  association rules, not inventing a new annotation API for every language.
- Comments are traditionally untrustworthy because they're unverified.
  With LSP diagnostics (real-time red squiggles on broken refs) and
  `supersigil verify` as CI gate, they become as reliable as any annotation.

**What you lose vs. proc macros:**
- No `cargo build` failure on broken refs (compile-time validation).
- LSP diagnostics + CI gate provide equivalent coverage in practice.
  The 5% gap: a developer without the LSP who doesn't run verify locally
  only finds out in CI. Same trade-off as any linter.

**The existing Rust macro and Vitest helper are migration scaffolding.**
Supersigil is currently its own only user, so there is no need to promise an
indefinite compatibility layer. They can run in dual mode while the source
directive path proves itself, then be removed after internal migration unless
there is a separate public-API decision to keep them.

---

## The Three Options

### Option 1: Stay the Course

Keep the current model. Markdown specs are the only graph input. Source
contributes only evidence links.

**What you get:** Simplicity. Language independence. Clean separation.

**What you don't get:** Design docs still drift. Agents still need 4 file
reads per slice. Infrastructure code stays at 0-2% coverage. No
differentiation from competitors.

**Verdict:** Safe, but leaves real problems on the table.

---

### Option 2: The Enrichment Model (Recommended)

Source code *enriches* existing Markdown specs via structured comments.
Markdown documents remain authoritative. Source annotations contribute
additional components — criteria, implements links, tracked-file
declarations — flowing into the same graph nodes.

**The directives:**

| Directive | What it does | Example |
|-|-|-|
| `verifies` | Links a test to a criterion (existing behavior, new syntax) | `//supersigil:verifies auth/req#login-works` |
| `implements` | Declares this code implements a criterion | `//supersigil:implements auth/req#login-works` |
| `criterion` | Adds an implementation-level criterion to a document | `//supersigil:criterion auth/req#token-shape "Signed JWTs"` |
| `tracked-by` | Declares this file is tracked by a spec | `//supersigil:tracked-by auth/design` |

**How it works:**
- Source annotations contribute components to existing Markdown documents.
  `//supersigil:criterion auth/req#token-shape "..."` adds a criterion to
  document `auth/req`.
- Markdown owns the document identity (id, type, status) and narrative.
  Source adds implementation-level atoms.
- The graph builder can keep the same broad shape and `build_graph` signature,
  but `SpecDocument` meaning changes: merged documents may contain components
  whose provenance is a source file rather than the Markdown file.
- Extraction uses one directive grammar plus per-language comment extraction
  and directive-to-code association.

**What you get:**
- **Reduced drift.** Implementation-level criteria live next to the code
  they describe. Design docs shed code examples and stay lean.
- **Better agent experience.** Agents see criteria while editing. Skill
  steps drop from 10 to ~6. Tool calls per slice drop from 4 to 2.
- **Convention-based mapping becomes tractable.** Inline criteria provide
  module-level structural anchors for name-based test matching — a safety
  net for when agents forget annotations.
- **Broad language support.** The directive grammar works across Rust, JS/TS,
  Python, Go, C/C++, Java, and other languages with comments, while each host
  language still needs extraction and association rules.
- **Simplified annotation model.** One directive grammar instead of
  per-language annotation APIs, with language-specific association rules where
  the host syntax requires them.
- **Differentiation.** No competitor does this.

**Architectural changes:**

| Layer | Change |
|-|-|
| Graph | No required `build_graph` signature change. Component provenance, source-span reporting, and criterion-level `implements` indexes change. |
| Evidence/plugins | New shared directive parser plus Rust/JS source comment extraction and association for evidence. |
| Existing Rust plugin | Dual extraction during internal migration; `#[verifies]` is removable after migration unless a later public-API decision keeps it. |
| Existing JS plugin | Same for the Vitest `verifies()` helper. |
| Verification | Coverage semantics stay unchanged initially. Source-derived criteria use the same checks, with source-aware ordering and diagnostics where needed. |
| LSP | Diagnostics, completion, hover, go-to-definition, code lens, and workspace/symbol support on source annotations. |
| CLI | Loaders need a shared enrichment input set before graph construction; commands then query the enriched graph. |

**Validation without a compiler:**
- LSP: real-time diagnostics on invalid refs, completions for IDs, hover
  for criterion text, go-to-definition from directive to spec. Works in
  VS Code and Neovim via multi-LSP coexistence.
- CI: `supersigil verify` catches anything the LSP misses.
- Code actions: "did you mean X?" for typos in refs.

**Migration path:**
1. Build the shared directive grammar and Rust/JS comment extraction for
   `verifies`.
2. Ship source `verifies` as an experimental, gated dual-mode path.
3. Add LSP diagnostics, completion, and navigation before broad migration.
4. Dogfood Supersigil internally, then remove or deprecate the Rust proc macro
   and Vitest helper once no internal usage remains.
5. Add `tracked-by`, `implements`, and later `criterion` through the
   pre-graph enrichment path after ownership and project scoping are pinned.
6. Add convention-based mapping once source-local criteria provide anchors.

**Risk:** Comments can be accidentally deleted or moved. That is acceptable
only if `supersigil verify` catches stale or broken references and the LSP
surfaces problems during editing.

**Verdict:** The sweet spot. One directive grammar, contained architecture
change, solves both motivations, incremental and reversible.

---

### Option 3: Full Source Spec Documents

Source code can define *entire* spec documents — not just enriching
existing Markdown docs, but creating new graph nodes from source alone.
Uses structured comments for document-level declarations:

```rust
//supersigil:document auth/impl/login type=design status=approved
//supersigil:implements auth/req#login-works
//supersigil:criterion token-validation "JWTs validated against signing key"
//supersigil:criterion rate-limiting "Login attempts rate-limited to 5/min"
pub mod login {
    // ...
}
```

**What you get (beyond Option 2):**
- Full layering pattern: requirement docs (Markdown) → implementation
  docs (source) → tests. Two-layer graph with `Implements` edges.
- Opt-in transitive coverage: a requirement criterion counts as covered
  through its implementation doc's evidence.
- Source-defined specs for cases where no Markdown doc exists yet.

**What it costs (beyond Option 2):**
- New identity model decisions (explicit IDs vs. path-derived).
- LSP must understand source files as spec containers, not just
  annotation carriers.
- More complex mental model for users.
- The `document` directive is a separate authoring act, not metadata on
  something you're already writing.

**Verdict:** Valuable but not urgent. Keep it in an ADR-gated parking lot until
the enrichment model proves itself.

---

## Decision Matrix

| Criterion | Option 1 (status quo) | Option 2 (enrichment) | Option 3 (full source docs) |
|-|-|-|-|
| Spec drift | Unchanged | Improved | Best |
| Agent experience | Unchanged | Improved | Best |
| Language independence | Preserved | **Strengthened** (universal syntax) | Preserved |
| Specify-before-implement | Preserved | Preserved for Markdown requirements; implementation criteria are a later opt-in relaxation | Preserved for Markdown |
| Architectural complexity | None | Medium (shared directive grammar plus provenance, discovery, and index changes) | Medium-high (identity model) |
| Adoption friction | None | Very low (just comments) | Low-medium |
| Competitive differentiation | None | Strong | Strongest |
| Discoverability | N/A | Solvable via LSP | Solvable via LSP |
| Reversibility | N/A | High (delete comments) | Medium |
| New language effort | N/A | Moderate but bounded (comment extraction + association rules) | Moderate |

## Recommendation

**Start with Option 2 (enrichment model with structured comments).**

One directive grammar, every language. Extends the graph input model without
requiring a new spec-authoring surface per language. LSP validation makes
comments trustworthy enough for day-to-day work, with CI as the backstop.

**First experiment:**
1. Build `supersigil-directives` with directive parsing and Rust/JS comment
   extraction for `verifies`.
2. Support `//supersigil:verifies` behind an experimental gate.
3. Add minimal LSP diagnostics, completion, and go-to-definition for source
   directive refs.
4. Migrate one small internal crate or package and verify that coverage and
   diagnostics are equivalent or better.

**Then, if it works:**
- Add `tracked-by` and `implements` after ownership and project scoping are
  pinned.
- Add `criterion` only as implementation-level metadata.
- Add convention-based mapping as a safety net before broader dogfooding.
- Remove or explicitly retain per-language annotation mechanisms based on the
  internal migration outcome.

**Don't build yet:** Full source documents (Option 3), transitive coverage,
C/C++ plugin. Treat full source documents as an ADR-gated parking lot, not the
natural next commitment after the first arc.
