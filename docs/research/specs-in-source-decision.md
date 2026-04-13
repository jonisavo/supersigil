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
specify-before-implement workflow. Must be discoverable (the "hard to find"
concern from regulated-sector feedback). Must work with existing Markdown
specs incrementally.

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
- One extraction mechanism: tree-sitter finds comment nodes, regex
  extracts directives. Per-language overhead is minimal (comment prefix).
- New language support = add comment prefix to config. No plugin needed.
- Comments are traditionally untrustworthy because they're unverified.
  With LSP diagnostics (real-time red squiggles on broken refs) and
  `supersigil verify` as CI gate, they become as reliable as any annotation.

**What you lose vs. proc macros:**
- No `cargo build` failure on broken refs (compile-time validation).
- LSP diagnostics + CI gate provide equivalent coverage in practice.
  The 5% gap: a developer without the LSP who doesn't run verify locally
  only finds out in CI. Same trade-off as any linter.

**The existing Rust macro and Vitest helper can remain** as optional
convenience layers for teams that want compile-time errors or test-
framework integration. But they become opt-in extras, not the primary
mechanism. Since supersigil's only user is itself, migration is mechanical.

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
- The graph builder merges both sources. No changes to `build_graph`.
- Extraction: tree-sitter finds comments, regex extracts directives.
  One shared extraction crate for all languages.

**What you get:**
- **Reduced drift.** Implementation-level criteria live next to the code
  they describe. Design docs shed code examples and stay lean.
- **Better agent experience.** Agents see criteria while editing. Skill
  steps drop from 10 to ~6. Tool calls per slice drop from 4 to 2.
- **Convention-based mapping becomes tractable.** Inline criteria provide
  module-level structural anchors for name-based test matching — a safety
  net for when agents forget annotations.
- **Universal language support.** Same syntax works for Rust, JS/TS,
  Python, Go, C/C++, Java, anything with comments.
- **Simplified architecture.** One extraction mechanism instead of
  per-language parsers.
- **Differentiation.** No competitor does this.

**Architectural changes:**

| Layer | Change |
|-|-|
| Graph | None. |
| Evidence/plugins | New shared `supersigil-directives` crate: tree-sitter comment extraction + directive parsing. |
| Existing Rust plugin | Unchanged (still extracts `#[verifies]` for backwards compat). New directives extracted via shared crate. |
| Existing JS plugin | Same. |
| Verification | None (new criteria are verified like any other). |
| LSP | Hover, diagnostics, code lens, workspace/symbol on source annotations. |
| CLI | None. Commands query the graph. |

**Validation without a compiler:**
- LSP: real-time diagnostics on invalid refs, completions for IDs, hover
  for criterion text, go-to-definition from directive to spec. Works in
  VS Code and Neovim via multi-LSP coexistence.
- CI: `supersigil verify` catches anything the LSP misses.
- Code actions: "did you mean X?" for typos in refs.

**Migration path:**
1. Build the `supersigil-directives` crate (tree-sitter comment extraction
   + directive parsing). Start with `verifies` and `implements`.
2. Dogfood: migrate supersigil's own `#[verifies]` annotations to
   `//supersigil:verifies` comments. Move 2-3 `Implements` links from
   `ecosystem-plugins/design` into source.
3. Add `criterion` and `tracked-by` directives.
4. Extend LSP: hover/diagnostics/code lens on directives in source files.
5. Convention-based mapping with module-scoped inference.
6. Deprecate (but don't remove) the Rust proc macro and Vitest helper.

**Risk:** Comments can be accidentally deleted or moved. But the same is
true of any annotation — and `supersigil verify` catches it.

**Verdict:** The sweet spot. One universal mechanism, minimal architecture
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

**Verdict:** Valuable but not urgent. Build it as Phase 2 after the
enrichment model proves itself. The graph layer already supports it.

---

## Decision Matrix

| Criterion | Option 1 (status quo) | Option 2 (enrichment) | Option 3 (full source docs) |
|-|-|-|-|
| Spec drift | Unchanged | Improved | Best |
| Agent experience | Unchanged | Improved | Best |
| Language independence | Preserved | **Strengthened** (universal syntax) | Preserved |
| Specify-before-implement | Preserved | Preserved | Preserved for Markdown |
| Architectural complexity | None | Low (one shared crate) | Medium (identity model) |
| Adoption friction | None | Very low (just comments) | Low-medium |
| Competitive differentiation | None | Strong | Strongest |
| Discoverability | N/A | Solvable via LSP | Solvable via LSP |
| Reversibility | N/A | High (delete comments) | Medium |
| New language effort | N/A | Minimal (comment prefix) | Minimal |

## Recommendation

**Start with Option 2 (enrichment model with structured comments).**

One syntax, every language. Extends the graph without changing the graph.
LSP validation makes comments trustworthy. Fully incremental, fully
reversible.

**First experiment:**
1. Build `supersigil-directives` crate with tree-sitter comment extraction.
2. Support `//supersigil:verifies` and `//supersigil:implements` directives.
3. Migrate supersigil's own `#[verifies]` annotations in one crate as a
   test. Verify the graph builds correctly.
4. Move 2-3 `Implements` links from `ecosystem-plugins/design` into source.

**Then, if it works:**
- Add `criterion` and `tracked-by` directives.
- Extend LSP to source files (hover, diagnostics, workspace/symbol).
- Convention-based mapping as safety net.
- Deprecate per-language annotation mechanisms.

**Don't build yet:** Full source documents (Option 3), transitive coverage,
C/C++ plugin. These are Phase 2 — valuable but dependent on the enrichment
model proving itself.
