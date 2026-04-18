# Watch Mode Research

*April 2026*

## Architecture

```
notify (v8.x) + notify-debouncer-mini (500ms)
        |
        v
  Classify changed files:
    - Spec (.md in configured paths) -> re-parse affected docs
    - Source (matched by TrackedFiles globs) -> re-scan evidence
    - Config (supersigil.toml) -> full reload
        |
        v
  Determine affected document IDs
  (reuse existing `affected` module logic)
        |
        v
  Incremental re-verify:
    Merge fresh findings (affected) with cached findings (unaffected)
        |
        v
  Clear screen + print report
  "[12:34:56] Re-verified 3/12 documents (auth/*, api/design)"
```

## Crate Selection

### File Watching

**notify v8.2.0** (stable, 62.7M downloads): The foundational crate. Used
by rust-analyzer, zed, deno, watchexec, alacritty, mdBook. Cross-platform:
inotify (Linux), FSEvents (macOS), ReadDirectoryChangesW (Windows).

**notify-debouncer-mini**: Lightweight debouncer on top of notify. Filters
events, emits one event per file per timeframe. Sufficient for supersigil's
use case (we care about "file X changed," not rename tracking).

**watchexec** (library): Higher-level, adds gitignore parsing, process
supervision, command execution. Overkill for supersigil since we run our own
verification engine in-process rather than spawning subprocesses.

**Recommendation:** `notify` v8.x + `notify-debouncer-mini`. Simple,
well-tested, minimal dependency surface.

### Terminal Management

**clearscreen v3.0.0**: Cross-platform screen clearing (tested with 80+
terminals). Used by watchexec/cargo-watch.

**ratatui v0.30.0**: Full TUI framework (immediate-mode rendering, 60+ FPS).
Used by bacon, Nx 21, gitui. Overkill for v1 watch mode but a future option
for a multi-panel view.

**Recommendation:** Start with `clearscreen`. Graduate to `ratatui` if users
want per-document drill-down or multi-panel views.

## Debouncing

A single editor save triggers multiple FS events within milliseconds (write
temp file, rename, chmod). Atomic saves (write-to-temp-then-rename) generate
even more.

**Recommended debounce: 500ms.** This is the sweet spot for build/verification
triggers. Too low (100ms) and you re-verify on partial writes. Too high
(2000ms) and feedback feels sluggish.

Spec files (.md) change rarely and atomically; source files (.rs, .ts) may
change in bursts during refactoring. A uniform 500ms handles both well.

## Incremental Verification Strategy

### What Other Tools Do

**Salsa framework** (powers rust-analyzer, ruff): Red-green marking algorithm
with function-level memoization. Inputs marked dirty propagate to dependents;
nodes recomputed only if actual inputs changed. Powerful but requires modeling
the entire computation as Salsa queries. Overkill for supersigil's pipeline.

**Turbopack** (powers Next.js): `Vc` (value cell) system with automatic
dependency tracking. Demand-driven: only re-executes if something requests
the result. Also overkill.

**tsc --watch**: Module dependency graph. Marks changed files and all
dependents. TypeScript 7's Go rewrite achieves 8-10x faster compilation.

**vitest --watch**: Vite module graph for dependency tracking. Re-runs only
test files that transitively import the changed file.

### What Supersigil Should Do

Supersigil's verify pipeline has clear phases, but incremental invalidation
is more nuanced than "reuse `affected`." The current `affected` command only
covers TrackedFiles-matched git diffs + one-hop transitive refs. It does not
cover evidence-only changes (test files outside TrackedFiles), spec document
changes, multi-hop transitive deps, or global findings like duplicate IDs
and cycles that span unchanged documents.

**Three invalidation categories** (must be handled separately):

1. **Spec graph invalidation.** A spec file changed → re-parse that
   document, rebuild the graph. This can introduce or resolve cross-document
   issues (broken refs, cycles, duplicate IDs) that affect *other* unchanged
   documents. A naive "only re-verify affected docs" cache merge would hide
   these. **Safe approach:** any spec file change triggers a full graph
   rebuild and full rule pass. Spec files change rarely, so this is
   acceptable.

2. **Implementation-file impact.** A source file matched by TrackedFiles
   changed → mark the owning document(s) as stale. This is what `affected`
   does today. One-hop transitive expansion covers direct references. Cache
   merge is safe here because tracked-file impact is per-document.

3. **Evidence/test-file invalidation.** A test file changed → re-scan
   evidence for documents whose VerifiedBy globs match the file, or whose
   ecosystem plugin would discover it. This is *not* covered by `affected`
   today. The watch mode needs its own mapping from test file paths to
   document IDs via VerifiedBy globs and plugin discovery inputs.

**Practical approach for v1:**
- Spec file change → full re-verify (rebuild graph + all rules). Safe,
  simple, and spec changes are infrequent.
- Source/test file change → re-scan evidence for affected documents, re-run
  coverage rules for those documents, keep other findings cached.
- Config change → full reload and re-verify.

This avoids the unsafe cache merge for global findings while still being
incremental for the common case (editing source/test files).

**Future improvement:** Extend `affected` to cover evidence file changes
(see strategic-direction.md, "Extending affected"). This would unify the
invalidation logic and make watch mode simpler.

## Terminal UX Patterns

Three approaches exist in the ecosystem:

**A. Clear-and-reprint** (cargo-watch, ruff --watch): Clear screen, print
fresh output. Simple, universal. Loses scrollback history.

**B. Delimiter-based** (vitest, jest --watch): Separator line + timestamp
between runs. Preserves scrollback. Terminal fills up over time.

**C. Full TUI** (bacon, Nx 21): Multi-panel layout with keyboard navigation.
Rich but complex.

**Recommendation for supersigil:** Start with Pattern A. Verify output is
typically compact (findings + summary). Clear screen, show timestamp header
with scope, print normal verify output. Add `--no-clear` for CI/piping.

## LSP Coexistence

The LSP already provides live-as-you-type feedback (per-file diagnostics on
`didChange`, cross-document diagnostics on `didSave`). Watch mode serves a
different audience:

- Terminal users who don't use an LSP-capable editor
- Users who want the *full verify report* continuously (LSP shows per-file
  diagnostics, not the project-wide summary)
- Local CI simulation (same output as CI would produce)

Both share the same `supersigil-verify` engine. The LSP's caching strategy
(file_parses, graph, evidence_by_target) is a reference for the watch mode's
own caching.

## Dependencies to Add

```toml
[dependencies]
notify = "8.2"
notify-debouncer-mini = "0.6"
clearscreen = "3.0"
```

Optional future: `ratatui = "0.30"` for TUI mode.
