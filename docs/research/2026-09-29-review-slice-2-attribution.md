# Review Slice, Plan 2 of 4: Snapshots and Attribution

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Review a change between a git base and a working-tree or commit target, attributing every changed line to the recorded agent edits that produced it (or saying precisely why not), through `supersigil review` and `supersigil why`.

**Architecture:** Three new library crates. `supersigil-git` runs the git CLI: snapshots (the working-tree target through a temporary index), changed paths, blob and worktree-form bytes, worktrees, and reflog origins. `supersigil-anchor` is pure: it walks the recorded edits backward from the target bytes, validating every step by forward execution and recorded hashes, classifies each chain (exact from the base, exact from its start, consistent), and combines chains into per-line provenance. `supersigil-review` is pure: it diffs blob bytes with `similar`, maps anchor's provenance onto the diff through ordered outcome rules, and builds the JSON model and terminal summary. The CLI reconciles every involved record with bounded locks, pins snapshots, maps edits to worktrees, and prints. The record gains one field, the edit's `operation`.

**Tech Stack:** Rust 2024 edition (workspace rust-version 1.92), the git CLI (2.36 or newer) through `std::process::Command`, `similar` 2 (Apache-2.0) for line diffs, serde + serde_json, thiserror 2, proptest (property tests), insta (yaml) and tempfile for tests, assert_cmd and predicates for CLI tests, cargo nextest.

**Spec:** `docs/research/2026-09-29-review-slice-2-design.md` (the approved plan 2 design, sections 1 to 5), which amends `docs/research/multiverse-review-design.md`. Every rule this plan implements is argued there; when this plan and the design disagree, the design wins and the plan is wrong. The **Deviations and Decisions** section below lists where the tasks refine the design and what the author decided.

## Global Constraints

- Edition 2024, `rust-version = "1.92"`, workspace lints apply to every new crate (`[lints] workspace = true`): `missing_docs = "warn"` means every `pub` item gets a doc comment; clippy pedantic is on, so pure functions get `#[must_use]`, every `pub fn` returning `Result` has a `# Errors` section, and every `pub fn` that can panic has a `# Panics` section. The lint policy is not to be relaxed; no `#[allow]` without `reason = "..."`.
- New module syntax only: `module.rs` beside `module/`, never `module/mod.rs`.
- New crates are ordinary publishable workspace members at version `0.14.0`, pinned from `[workspace.dependencies]` with `version = "=0.14.0"` like plan 1's crates.
- No new dependency outside the license allow list in `deny.toml` (Apache-2.0, Apache-2.0 WITH LLVM-exception, BSL-1.0, MIT, Unicode-3.0, Unlicense). `similar` is Apache-2.0.
- No `unsafe`. Tests never call `std::env::set_var` (unsafe in edition 2024): environment for git and for the CLI binary is passed to each spawned `Command`.
- Git is run only through `supersigil_git::run::Git`, which removes `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, and `GIT_COMMON_DIR` from the child environment and sets `GIT_OPTIONAL_LOCKS=0`. Output carrying paths is read in NUL-delimited (`-z`) formats. The minimum git version is 2.36.0.
- The product honors the user's git configuration. Only tests isolate it: every test that runs git sets `GIT_CONFIG_NOSYSTEM=1`, `HOME` and `XDG_CONFIG_HOME` to its temporary directory, `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`, `GIT_COMMITTER_EMAIL`, and `core.autocrlf` explicitly per repository (false unless the test is about conversion).
- Nothing in plan 2 writes the real index, the worktree, or a ref. The working-tree snapshot writes objects only.
- Anchor never uses `structuredPatch` hunk text as bytes; hunk line numbers only order candidate positions. Every accepted reversal passes forward execution with the recorder's semantics (`supersigil-session`'s `claude_code/content.rs`: first occurrence; every occurrence for `replace_all`; a Write's result is its content) and every known hash.
- Time never decides lineage. Ordinals order edits only within one transcript; equal ordinals are unordered.
- Line numbers are 0-based inside `supersigil-anchor` and `supersigil-review` code and 1-based in JSON and terminal output. Lines are split after `\n`; a line keeps its terminator.
- Every untrusted string reaching a terminal goes through plan 1's `supersigil_cli::format::Untrusted`.
- Before finishing any task: `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features` with no warnings, and `cargo nextest run`.
- Commits: conventional commit type (`feat:`, `fix:`, `test:`, `ci:`, `docs:`), no scope unless needed to disambiguate, stage files by name (never `git add -A`), and no `Co-Authored-By` or other attribution trailer; the author keeps AI attribution out of the commit history.
- This plan runs on a branch off `pivot` (after plan 1, merged as ed36785). The design docs are committed separately with `docs:`.

## Review Focus

Inputs a person will hit early, each pinned to a test in the owning task:

1. An edit to a file over 10 KB, whose transcript carries no `originalFile` and so no hashes: one session is still exact from the base; a manual change makes it consistent (Task 10 property tests and `large_file_single_session_is_exact_from_base`; Task 13 `review_large_file_single_session`).
2. A tab-indented file, whose `structuredPatch` expands tabs: attribution never reads hunk text (Task 8 `tab_indented_hunks_are_never_bytes`).
3. A deletion that jsdiff displays a line late: the true position is still a candidate (Task 8 `deletion_displayed_late_is_recovered`).
4. A manual change between two edits of one session: a gap, never exactness across it (Task 13 `review_slice_fixture`).
5. A file the agent created but never staged: listed first with the recorded-edit mark, attributed with `--include-untracked` (Task 13 `review_slice_fixture`, `review_slice_with_include_untracked`).
6. A file deleted with `rm`: unattributed, with the command as a mention (Task 13 `review_slice_fixture`).
7. The stop hook holding a record's writer lock while `review` runs: the command finishes and reports the record as not reconciled (Task 12 `writer_lock_contention_is_reported`, Task 13 `review_reports_locked_record`).
8. `why` on an assume-unchanged file: *not captured*, with the evidence context still present (Task 13 `why_on_assume_unchanged_file_is_not_captured`).
9. A commit made in a sibling worktree: that worktree's record and reflog are used (Task 13 `review_commit_from_sibling_worktree`).
10. A session started in a subdirectory of the repository: its record is involved (Task 12 `subdirectory_record_is_involved`).
11. A SHA-256 repository (Task 4 `sha256_repository_resolves`).
12. Git 2.36 (Task 4 adds the CI job that runs the git crate's tests with git 2.36.0).

---

## File Structure

Every file the plan creates or modifies, with the tasks that touch it (from each task's **Files** block).

**Workspace**

| File | Tasks |
|---|---|
| `.github/workflows/ci.yml` | 3 (modify), 4 (modify) |
| `Cargo.lock` | 2 (modify) |
| `Cargo.toml` | 2 (modify) |
| `commands/why.rs` | 13 (test) |
| `src/mapping.rs` | 11 (create) |
| `src/model.rs` | 11 (create) |
| `src/outcome.rs` | 11 (create) |
| `src/summary.rs` | 11 (create) |
| `src/why.rs` | 11 (create) |
| `tests/file_review.rs` | 11 (test) |
| `tests/outcome_rules.rs` | 11 (test) |
| `tests/snapshots.rs` | 11 (test) |
| `tests/snapshots/` | 11 (test) |

**crates/supersigil-record**

| File | Tasks |
|---|---|
| `crates/supersigil-record/src/derive.rs` | 1 (modify) |
| `crates/supersigil-record/src/observations.rs` | 1 (modify) |
| `crates/supersigil-record/tests/model_roundtrip.rs` | 1 (modify) |
| `crates/supersigil-record/tests/snapshots/model_roundtrip__observation_json_shape_is_stable.snap` | 1 (modify) |

**crates/supersigil-session**

| File | Tasks |
|---|---|
| `crates/supersigil-session/src/claude_code.rs` | 1 (modify) |
| `crates/supersigil-session/src/discover.rs` | 12 (modify) |
| `crates/supersigil-session/tests/fixtures/slice.jsonl` | 13 (modify) |
| `crates/supersigil-session/tests/parse_fixture.rs` | 1 (test) |

**crates/supersigil-git**

| File | Tasks |
|---|---|
| `crates/supersigil-git/Cargo.toml` | 2 (create), 4 (modify), 5 (modify) |
| `crates/supersigil-git/src/bytes.rs` | 6 (create) |
| `crates/supersigil-git/src/changes.rs` | 6 (create) |
| `crates/supersigil-git/src/error.rs` | 4 (create) |
| `crates/supersigil-git/src/lib.rs` | 2 (create), 4 (modify), 5 (modify), 6 (modify), 7 (modify) |
| `crates/supersigil-git/src/oid.rs` | 4 (create) |
| `crates/supersigil-git/src/origin.rs` | 7 (create) |
| `crates/supersigil-git/src/path.rs` | 4 (create), 5 (modify) |
| `crates/supersigil-git/src/repo.rs` | 4 (create) |
| `crates/supersigil-git/src/run.rs` | 4 (create) |
| `crates/supersigil-git/src/snapshot.rs` | 5 (create) |
| `crates/supersigil-git/src/worktree.rs` | 4 (create) |
| `crates/supersigil-git/tests/changes.rs` | 6 (create) |
| `crates/supersigil-git/tests/common/mod.rs` | 4 (create) |
| `crates/supersigil-git/tests/origin.rs` | 7 (create) |
| `crates/supersigil-git/tests/repo.rs` | 4 (create) |
| `crates/supersigil-git/tests/snapshot.rs` | 5 (create) |
| `crates/supersigil-git/tests/worktree.rs` | 4 (create) |

**crates/supersigil-anchor**

| File | Tasks |
|---|---|
| `crates/supersigil-anchor/Cargo.toml` | 2 (create), 8 (modify) |
| `crates/supersigil-anchor/src/combine.rs` | 10 (create) |
| `crates/supersigil-anchor/src/content.rs` | 10 (create) |
| `crates/supersigil-anchor/src/input.rs` | 8 (create) |
| `crates/supersigil-anchor/src/lib.rs` | 2 (create), 8 (replace), 9 (replace), 10 (replace) |
| `crates/supersigil-anchor/src/lines.rs` | 8 (create) |
| `crates/supersigil-anchor/src/provenance.rs` | 10 (create) |
| `crates/supersigil-anchor/src/result.rs` | 8 (create), 9 (replace), 10 (replace) |
| `crates/supersigil-anchor/src/walk.rs` | 9 (create) |
| `crates/supersigil-anchor/tests/align.rs` | 8 (create) |
| `crates/supersigil-anchor/tests/attribute.rs` | 10 (create) |
| `crates/supersigil-anchor/tests/common/mod.rs` | 8 (create), 9 (modify), 10 (modify) |
| `crates/supersigil-anchor/tests/properties.rs` | 10 (create) |
| `crates/supersigil-anchor/tests/step.rs` | 8 (create) |
| `crates/supersigil-anchor/tests/walk.rs` | 9 (create) |

**crates/supersigil-review**

| File | Tasks |
|---|---|
| `crates/supersigil-review/Cargo.toml` | 2 (create), 11 (modify) |
| `crates/supersigil-review/src/diff.rs` | 11 (create) |
| `crates/supersigil-review/src/lib.rs` | 2 (create), 11 (modify) |
| `crates/supersigil-review/tests/common/mod.rs` | 11 (test) |

**crates/supersigil-cli**

| File | Tasks |
|---|---|
| `crates/supersigil-cli/Cargo.toml` | 13 (modify) |
| `crates/supersigil-cli/src/commands.rs` | 13 (modify) |
| `crates/supersigil-cli/src/commands/review.rs` | 13 (create) |
| `crates/supersigil-cli/src/commands/why.rs` | 13 (create) |
| `crates/supersigil-cli/src/error.rs` | 13 (modify) |
| `crates/supersigil-cli/src/evidence.rs` | 13 (create) |
| `crates/supersigil-cli/src/format.rs` | 13 (modify) |
| `crates/supersigil-cli/src/lib.rs` | 12 (modify), 13 (modify) |
| `crates/supersigil-cli/src/main.rs` | 13 (modify) |
| `crates/supersigil-cli/src/mapping.rs` | 13 (create) |
| `crates/supersigil-cli/src/pipeline.rs` | 13 (create) |
| `crates/supersigil-cli/src/reconcile.rs` | 12 (create) |
| `crates/supersigil-cli/src/record_dir.rs` | 12 (modify) |
| `crates/supersigil-cli/tests/cmd_review.rs` | 13 (test) |
| `crates/supersigil-cli/tests/reconcile.rs` | 12 (test) |

## Deviations and Decisions

The tasks were written in parallel against the interfaces contract (appendix), each verified in its own worktree, and then integrated in one worktree where all fourteen tasks build and pass together (fmt clean, clippy clean, 430 tests). Where a task's own **Interfaces** block differs from the appendix, the task is authoritative. An adversarial Codex review of the first assembly found eleven problems (a Task 13 compile error, false provenance, suppressed ambiguity, unbounded work, and five CLI gaps); all eleven are fixed in the tasks below, each with its counterexample as a regression test, and the design was amended to match (plan 2 design, sections 1 to 4).

Collected deviations:

- **Task 1:** `EditOperation::Unknown`'s doc names `` `MultiEdit` `` in backticks (clippy `doc_markdown`).
- **Task 2:** `similar` is declared without default features; each crate declares only the dependencies it uses so far, later tasks add the rest.
- **Tasks 4 to 7:** `tempfile` becomes a regular dependency of `supersigil-git` in Task 5 (the snapshot needs a temporary directory). `TestRepo` gains `home()`, `run_in`, `run_input`, `tree_files`; `commit_all` allows empty commits. `SnapshotOptions.pathspecs` narrows only the listings: the snapshot tree always covers the whole worktree, so its identity does not depend on scope options. `head_reflog` returns no entries only when the worktree's own HEAD reflog does not exist or is empty; any other failure to read it is unavailable origin evidence (changed after the whole-branch review, which also found that `git log -g HEAD` falls back to the branch's reflog when HEAD's own has no entries); `find_origins` treats a missing directory like a prunable worktree and cannot see record associations (Task 13 appends those). A missing object is `GitError::Parse`. An `ls-files -v` tag `s` (both flags) counts as assume-unchanged. The git 2.36 CI job uses `actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0`.
- **Tasks 8 to 10:** a retained patch is a consistency check (the author's decision of 2026-09-30, design section 3): a candidate contradicting a hunk's old side is rejected, and a step whose current bytes contradict a hunk's new side stops with the new `StopReason::AfterPatchMismatch`; the test helper `with_patch(edit, before, after)` builds the jsdiff-style hunk and replaces the contract's `with_hunk_hint`. `align::align` returns `align::Alignments`, a lazy iterator, because alignment choices are made independently per replacement pair and enumerated under the budget. `ContentMatches` gains `incomplete: bool`, and `content::content_matches` takes the combined outcomes (to match only the lines that need the fallback) and a budget; `attribute` calls it only when the search completed. Every generated branch is charged 1 KiB before it is explored. Agreement compares the full provenance (origins, whitespace-only and earlier contributors) and never merges contributors across chains; provenance travels as character runs through whitespace joins and splits. Test helpers add `state(Option<&str>) -> State`, `line_count(&State) -> usize`, and `finishes_within`. `State::matches` is `Some(false)` when the record says present-with-unknown-content but the state is absent. A content-match block covers a line only when it contains the whole line apart from its terminator; a Write's `old_text` is never used. `ContentMatches` is not re-exported at the crate root (import it from `supersigil_anchor::result`).
- **Task 11:** `render_summary` and `render_why` take `escape: fn(&str) -> String` (the CLI passes `escape_control`), so the review crate need not depend on the CLI. `AttributionInfo.status` and `WhyLine.status` are `Option<PathStatus>` (none when attribution is unavailable). Additions: `FileStatus::letter()`, `FileKindInfo::as_str()`, `SpanSide`; `AttributionInfo.conflicting_edits`; `ScopeInfo.path_note`; `WhyLine.differs_from_head_reason` (and `why_line`'s last parameter); `UnattributedReason::ContentMatchIncomplete` with its own `FileCounts.content_match_incomplete`. Hunk starts use unified-diff numbering. `why_line` takes target lines before base lines while the outcome functions take base before target (as the contract wrote them).
- **Task 12:** `supersigil_session::discover::transcripts_in(project_dir)` is split out of `discover_transcripts`; `record_dir::RETRY` (50 ms, crate-private). `reconcile` takes the main worktree and also reads the project directories of each candidate worktree's ancestors up to it (`ancestors_within`). The writer-lock retry also retries `StoreError::Conflict`.
- **Task 13:** edits are deduplicated once across all paths before partitioning (`MappedAccepted`, `PathConflict`, `conflicts_for`, `all_conflicts`). `why` on an untracked path takes a second snapshot including that path, only when the path is absent from the first tree and present on disk (going through `--include-untracked` would also change what `add -u` captures). Typed paths for `--include-untracked` and `-- <path>` resolve relative to the current directory, as git does; a root selector reviews everything (`path_filters`, `untracked_paths`). For commit targets, mentions search every candidate worktree. The on-disk check compares the disk with the converted bytes (`PathBytes::target_worktree_form`).

Decisions the author made on 2026-09-30:

1. **Set-aside refinement (Task 9): accepted.** A chain that does not end at the base is set aside when a base-reaching chain entered, in the same order, every group the first one entered (possibly followed by more, see G2 below) and used, in each, every edit the first one used; a chain holding a recorded edit no base-reaching chain holds is never set aside, so no recorded evidence is dropped, and the set-aside chains stay listed as alternatives that assume an unrecorded change. The design's "exactly the same set of edits" would have contradicted its own completeness property: a wrong deletion guess on a hashless file is typically stopped early by a recorded check and uses fewer edits. The plan 2 design and the main design doc are amended; `a_sibling_stopped_early_by_a_recorded_check_is_set_aside` and `a_sibling_stopped_early_by_a_creation_is_set_aside` pin it.
2. **Reconcile on a vanished transcript (Task 12): left to plan 3.** A transcript deleted between discovery and read aborts reconcile in plan 2; plan 3's deleted-transcript availability item reports it as unavailable evidence instead (see *What Plans 3 and 4 Build on This*).
3. **Hashless deletions and the budget (Task 8): resolved by the patch-consistency check.** Without it, a pure deletion in a large file without hashes makes every line start a candidate and exhausts the 64 MiB budget (a 2,000-line file costs about 160 MB for the candidates alone). Claude Code's hunks show the before-file exactly under a measured display rule (every tab as two spaces, 274 of 274 hunks), so the true candidate always passes the check and wrong positions almost never do; `a_hashless_deletion_in_a_large_file_ends_exact_from_the_base` was search incomplete before the check. The check only rejects, but rejecting a true alternative can make a survivor look unambiguous, so G1 below narrows it to edits whose relevant hash is unknown and ignores hunks that contradict the rule.

### Second review (G1 to G6), applied during execution

A second adversarial Codex review (2026-09-30), run on this plan after the fixes above, found six problems. They were confirmed against the plan and the design and are **not** yet in the task code below: the implementer of each named task applies them test-first (the regression test is written first and must fail), and each task's review checks them. Where a task's code or test count below disagrees with this list, this list wins; each added test raises that task's expected test count by one.

- **G1 (Task 8, and Task 9 for the walk test): the patch check never overrides a hash, and a hunk that contradicts the display rule is ignored.** `reverse` consults the old side only for a candidate when the edit's before-hash is unknown, and the new side (`AfterPatchMismatch`) only when its after-hash is unknown; a known hash decides alone. An edit whose retained hunks hold a literal tab (impossible under the measured rule) is reversed as if no patch were retained. Update the module and `reverse` doc comments and the Task 8 prose that says the check "can only reject". Tests: `a_known_hash_decides_without_the_patch` and `a_hunk_holding_a_literal_tab_is_ignored` (Task 8, step level); `verified_heads_survive_a_hunk_in_another_display` (Task 9: two heads in different transcripts, both with known hashes, `\tx\n` to `\ty\n`, one hunk showing the raw tab and one showing two spaces; both chains reach the base).
- **G2 (Task 9): keep `covers` as written (a prefix of the base-reaching chain's groups).** The design is amended to match. Update `walk`'s doc comment to the prefix wording. Test: `a_location_choice_that_misses_the_base_is_set_aside_across_a_link` (base `q\na\n`, target `a\na\n`, hashless D `x\n` to `a\n` in one transcript, verified P `q\na\n` to `x\na\n` in another; one exact chain [P, D], one set-aside chain starting at `a\nx\n`).
- **G3 (Task 9): verified links are explored at every state.** `explore` calls `try_links` whenever the chain has a step, also while `remaining` is non-empty; leaving the group then abandons the bucket's other edits, which the design allows ("may leave a group before reaching its first edit"). Test: `a_verified_link_is_explored_mid_bucket` (base `a\nb\n`, target `A\nB\n`, hashless `a` to `A` and `b` to `B` in one bucket of one transcript, verified P `q\nb\n` to `A\nb\n` in another; the chain [P, b-edit] stays next to the exact chains, so line 1 is ambiguous).
- **G4 (Task 10): blank lines keep their history.** `replaced` gathers earlier contributors from `lines[r].attrs()` for both `Line` variants, so a blank line an edit introduced still names it. Test: `a_blank_intermediate_line_keeps_its_contributor` (base `a\n`, target `b\n`; E and G in different transcripts each verified `a\n` to `\n`, F verified `\n` to `b\n`; the two chains disagree on line 1's earlier contributor, so it is ambiguous).
- **G5 (Task 13): deduplicate before filtering (design section 1, step 7; completes F7).** `map_candidates` keeps every sighting (candidate, non-candidate, and unplaced), `deduplicate` runs over all of them, and only accepted edits are then filtered to candidate worktrees; a conflict is excluded from every path and listed on each file any of its sightings touched. Test: `a_conflict_with_a_non_candidate_worktree_attributes_neither`, a variant of `a_conflicting_edit_id_attributes_neither_path` whose second sighting lies in a registered nested worktree that is not a candidate.
- **G6 (Task 10): content matching is charged for what it marks.** `find` collects the line intervals of every match of one block, merges overlaps, charges the union's bytes, and marks each covered line once. Test: `repeated_block_matches_are_charged_once` (a target of 100,000 lines `a\n` and an unknown-operation edit whose `new_text` is 50,000 such lines finishes within `finishes_within`'s bound).

Still open, left to CI: the non-UTF-8 index-entry test on Windows, and `rev-parse --show-object-format` and `sparse-checkout set --no-cone` on git 2.36; the CI jobs from Tasks 3 and 4 exercise them. On older git a recreated sparse file may keep its skip-worktree bit, so that test accepts either cause and always requires "present, not captured".

---

### Task 1: Record the edit operation

**Files:**
- Modify: `crates/supersigil-record/src/observations.rs` (new `EditOperation` enum before `Edit`; new `operation` field on `Edit` after `replace_all`)
- Modify: `crates/supersigil-record/src/derive.rs` (test helper `edit` sets the field)
- Modify: `crates/supersigil-record/tests/model_roundtrip.rs` (sample edit sets the field; new test)
- Modify: `crates/supersigil-record/tests/snapshots/model_roundtrip__observation_json_shape_is_stable.snap` (regenerated)
- Modify: `crates/supersigil-session/src/claude_code.rs` (`build_edit` sets the field)
- Test: `crates/supersigil-session/tests/parse_fixture.rs` (two new tests)

**Interfaces:**
- Consumes: plan 1's `Edit` observation and the Claude Code parser's `build_edit`.
- Produces: `supersigil_record::observations::EditOperation { Replace, Write, Unknown }` (`Copy`, `Default` = `Unknown`, serialized `"replace"`, `"write"`, `"unknown"`) and `Edit::operation: EditOperation` with `#[serde(default)]`. The parser records `Replace` for the Edit tool, `Write` for Write, and `Unknown` for MultiEdit. Anchor (Tasks 8 and 9) reads it to decide how a step is reversed; `Unknown` is never reversible.

The record crate has exactly two `Edit { .. }` struct literals outside the parser: the `edit` helper in `derive.rs`'s tests and `sample()` in `tests/model_roundtrip.rs`. Both get the field; nothing else in the workspace constructs an `Edit` literal.

- [ ] **Step 1: Write the failing tests**

In `crates/supersigil-record/tests/model_roundtrip.rs`, replace the `supersigil_record::observations` import with:

```rust
use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, Command, CommandCategory, Content, Edit, EditOperation,
    EndReason, FileState, Hunk, Material, Observation, Outcome, Role, SessionEnd, SessionStart,
    Source, Turn,
};
```

Add this test after `observations_round_trip_through_json_lines`:

```rust
#[test]
fn edit_without_operation_deserializes_as_unknown() {
    let Observation::Edit(recorded) = &sample()[2] else {
        panic!("sample()[2] is the edit");
    };
    let mut json = serde_json::to_value(&sample()[2]).unwrap();
    assert_eq!(json["operation"], "replace");
    json.as_object_mut().unwrap().remove("operation");
    let parsed: Observation = serde_json::from_value(json).unwrap();
    let Observation::Edit(parsed) = parsed else {
        panic!("still an edit");
    };
    assert_eq!(parsed.operation, EditOperation::Unknown);
    assert_eq!(
        Edit {
            operation: EditOperation::Replace,
            ..parsed
        },
        *recorded
    );
}
```

In `crates/supersigil-session/tests/parse_fixture.rs`, replace the `supersigil_record::observations` import with:

```rust
use supersigil_record::observations::{
    CommandCategory, Content, EditOperation, FileState, Material, Observation, Outcome, Role,
};
```

Add these two tests after `edits_carry_both_sides_and_computed_after_states`:

```rust
#[test]
fn fixture_edits_record_their_operation() {
    let outcome = parse_transcript(&fixture(), 0);
    let operations: Vec<EditOperation> = edits(&outcome).iter().map(|e| e.operation).collect();
    assert_eq!(
        operations,
        [
            EditOperation::Replace,
            EditOperation::Write,
            EditOperation::Replace,
            EditOperation::Replace,
            EditOperation::Replace,
        ]
    );
}

#[test]
fn multi_edit_operation_is_unknown() {
    let path = "/work/repo/a.txt";
    let multi = only_edit(
        "MultiEdit",
        json!({"file_path": path, "edits": [{"old_string": "a", "new_string": "c"}]}),
        json!({"filePath": path, "originalFile": "a b\n", "structuredPatch": []}),
    );
    assert_eq!(multi.after, known("c b\n"));
    assert_eq!(multi.operation, EditOperation::Unknown);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-record -p supersigil-session`
Expected: compile error, `unresolved import` of `supersigil_record::observations::EditOperation` (the type does not exist yet).

- [ ] **Step 3: Add the enum and the field**

In `crates/supersigil-record/src/observations.rs`, insert before the doc comment `/// A file edit made through the agent's editing tools.`:

```rust
/// Editing operation recorded for an edit.
///
/// Reconstructing an earlier file state depends on it: an Edit replaced
/// `old_text` with `new_text`, a Write replaced the whole file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditOperation {
    /// Claude Code's Edit tool: `old_text` replaced by `new_text`.
    Replace,
    /// Claude Code's Write tool: the whole file becomes `new_text`.
    Write,
    /// Not recorded (logs written before this field existed) or a tool
    /// anchor cannot reverse, such as `MultiEdit`.
    #[default]
    Unknown,
}
```

(`MultiEdit` is in backticks because clippy's pedantic `doc_markdown` rejects it bare.)

In `pub struct Edit`, after the `replace_all` field, add:

```rust
    /// Editing operation, `Unknown` for observations recorded before it was kept.
    #[serde(default)]
    pub operation: EditOperation,
```

`#[serde(default)]` is what lets logs written before this plan deserialize: the field is absent there and becomes `Unknown`. Sync never rewrites a log, so those observations keep `Unknown`; anchor treats them as not reversible (Task 8), and deduplication treats an `Unknown` and a known sighting of the same edit as compatible (Task 9).

- [ ] **Step 4: Set the field everywhere an `Edit` is built**

In `crates/supersigil-session/src/claude_code.rs`, replace the `supersigil_record::observations` import with:

```rust
use supersigil_record::observations::{
    CaptureCounts, Command, CommandCategory, Edit, EditOperation, FileState, Hunk, Material,
    Observation, Outcome, Role, Turn,
};
```

In `build_edit`, replace the `let parts = match tool.name.as_str() { .. };` block with:

```rust
    let (parts, operation) = match tool.name.as_str() {
        "Edit" => (edit_parts(tool, result), EditOperation::Replace),
        "Write" => (write_parts(tool, result), EditOperation::Write),
        _ => (multi_edit_parts(tool, result), EditOperation::Unknown),
    };
```

and in the `Edit { .. }` literal it returns, add `operation,` right after `replace_all: parts.replace_all,`.

In `crates/supersigil-record/src/derive.rs`, in the `tests` module, change `use crate::observations::Material;` to:

```rust
    use crate::observations::{EditOperation, Material};
```

and in the `edit` helper's `Edit { .. }` literal add, right after `replace_all: false,`:

```rust
            operation: EditOperation::Replace,
```

In `crates/supersigil-record/tests/model_roundtrip.rs`, in `sample()`'s `Edit { .. }` literal add, right after `replace_all: false,`:

```rust
            operation: EditOperation::Replace,
```

- [ ] **Step 5: Run the tests; the JSON shape snapshot fails as expected**

Run: `cargo nextest run -p supersigil-record -p supersigil-session`
Expected: every test passes except `model_roundtrip observation_json_shape_is_stable`, which fails and writes `crates/supersigil-record/tests/snapshots/model_roundtrip__observation_json_shape_is_stable.snap.new`.

- [ ] **Step 6: Review and accept the snapshot**

Run: `diff crates/supersigil-record/tests/snapshots/model_roundtrip__observation_json_shape_is_stable.snap crates/supersigil-record/tests/snapshots/model_roundtrip__observation_json_shape_is_stable.snap.new`
Expected: exactly one added data line, `>   operation: replace`, after `replace_all: false` in the `kind: edit` entry (plus an `assertion_line: 114` metadata line, which accepting drops). Any other difference is a regression: stop and fix it.

Accept it with cargo-insta (installed on the author's machine; otherwise `cargo binstall cargo-insta`):

Run: `cargo insta accept`
Expected: `accepted: crates/supersigil-record/tests/model_roundtrip.rs (observation_json_shape_is_stable.snap)`; the `.snap.new` file is gone.

Without cargo-insta, `INSTA_UPDATE=always cargo nextest run -p supersigil-record` writes the snapshot directly; review it with `git diff` afterwards the same way.

Run: `git diff crates/supersigil-record/tests/snapshots/`
Expected: the only change is `+  operation: replace` after `replace_all: false`.

- [ ] **Step 7: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; all tests pass (171 at the time of writing).

- [ ] **Step 8: Commit**

```bash
git add crates/supersigil-record/src/observations.rs crates/supersigil-record/src/derive.rs crates/supersigil-record/tests/model_roundtrip.rs crates/supersigil-record/tests/snapshots/model_roundtrip__observation_json_shape_is_stable.snap crates/supersigil-session/src/claude_code.rs crates/supersigil-session/tests/parse_fixture.rs
git commit -m "feat: record the operation of each edit"
```

---

### Task 2: Scaffold the git, anchor, and review crates

**Files:**
- Modify: `Cargo.toml` (workspace members and dependencies)
- Modify: `Cargo.lock` (regenerated by the build)
- Create: `crates/supersigil-git/Cargo.toml`, `crates/supersigil-git/src/lib.rs`
- Create: `crates/supersigil-anchor/Cargo.toml`, `crates/supersigil-anchor/src/lib.rs`
- Create: `crates/supersigil-review/Cargo.toml`, `crates/supersigil-review/src/lib.rs`

**Interfaces:**
- Consumes: the workspace manifest; `supersigil-record` from plan 1.
- Produces: three empty library crates that build and lint clean, and workspace dependency entries `supersigil-git`, `supersigil-anchor`, `supersigil-review` (pinned `=0.14.0`) and `similar` (without default features). Dependencies each crate already declares:
  - `supersigil-git`: `serde`, `thiserror`; dev: `tempfile`.
  - `supersigil-anchor`: `serde`, `supersigil-record`; dev: `proptest`.
  - `supersigil-review`: `serde`, `serde_json`, `similar`, `supersigil-anchor`, `supersigil-record`; dev: `insta`.
  A later task that needs another dependency (for example `serde_json` as a dev-dependency of anchor, or the three crates in the CLI) adds it in that task. Tasks 4 to 11 replace each `lib.rs` body with the module declarations the interfaces contract lists, keeping the crate doc comment.

Dependency direction, which later tasks must keep: `supersigil-git` depends on no crate of ours; `supersigil-anchor` on `supersigil-record`; `supersigil-review` on `supersigil-anchor` and `supersigil-record`, never on `supersigil-git`.

`similar` is Apache-2.0 (its `Cargo.toml` says `license = "Apache-2.0"`), on the `deny.toml` allow list. Its default feature is `text`, which Task 11 does not need: `similar::capture_diff_slices` and `similar::Algorithm` live in its always-compiled `common` and `algorithms` modules, so it is declared with `default-features = false`.

The release helper (`scripts/release-targets/index.mjs`, `replaceCargoWorkspacePins`) rewrites every `[workspace.dependencies]` line that contains `path = "crates/` and a `version = "=..."`; the three new one-line pins follow that shape, and the `crates/*` globs in `release-targets.json` already cover the new crates.

- [ ] **Step 1: Add workspace members and dependencies**

In the root `Cargo.toml`, replace the `members` list with (alphabetical):

```toml
members = [
    "crates/supersigil-anchor",
    "crates/supersigil-cli",
    "crates/supersigil-git",
    "crates/supersigil-record",
    "crates/supersigil-review",
    "crates/supersigil-session",
]
```

Add to the end of `[workspace.dependencies]`, after the `uuid` line:

```toml
supersigil-anchor = { path = "crates/supersigil-anchor", version = "=0.14.0" }
supersigil-git = { path = "crates/supersigil-git", version = "=0.14.0" }
supersigil-review = { path = "crates/supersigil-review", version = "=0.14.0" }
similar = { version = "2.7.0", default-features = false }
```

- [ ] **Step 2: Create the git crate**

`crates/supersigil-git/Cargo.toml`:

```toml
[package]
name = "supersigil-git"
version = "0.14.0"
description = "Git snapshots, changed paths, bytes, and commit origins for supersigil review"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[dependencies]
serde.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true

[lints]
workspace = true
```

`crates/supersigil-git/src/lib.rs` (the crate doc comment is the whole file for now; with `missing_docs` on, a crate without one warns):

```rust
//! Runs the git CLI for supersigil review.
//!
//! Resolves a base and a target, builds the working-tree target as a tree in
//! the object database through a temporary index, lists changed paths,
//! reads blob bytes and their worktree form, and finds which worktree's own
//! HEAD reflog originated a commit. It never writes the real index, the
//! worktree, or a ref.
```

- [ ] **Step 3: Create the anchor crate**

`crates/supersigil-anchor/Cargo.toml`:

```toml
[package]
name = "supersigil-anchor"
version = "0.14.0"
description = "Attribution of reviewed changes to recorded agent edits for supersigil"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[dependencies]
serde.workspace = true
supersigil-record.workspace = true

[dev-dependencies]
proptest.workspace = true

[lints]
workspace = true
```

`crates/supersigil-anchor/src/lib.rs`:

```rust
//! Attributes the lines of a reviewed file to the recorded edits that produced them.
//!
//! Pure over its inputs: given a base state, a target state, and the recorded
//! edits of one path, it walks backward from the target, accepting a step only
//! when executing the edit forward reproduces the current bytes and every
//! recorded hash matches, and reports per line which edits explain it, which
//! readings disagree, and where the evidence stops.
```

- [ ] **Step 4: Create the review crate**

`crates/supersigil-review/Cargo.toml`:

```toml
[package]
name = "supersigil-review"
version = "0.14.0"
description = "Review model: the reviewed diff with per-line attribution for supersigil"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
similar.workspace = true
supersigil-anchor.workspace = true
supersigil-record.workspace = true

[dev-dependencies]
insta.workspace = true

[lints]
workspace = true
```

`crates/supersigil-review/src/lib.rs`:

```rust
//! Builds the review model: the reviewed diff with each changed line's attribution.
//!
//! Pure over its inputs: it diffs blob bytes, maps attribution onto the diff
//! lines through ordered outcome rules, and produces the JSON model and the
//! terminal summary that `supersigil review` and `supersigil why` print.
```

- [ ] **Step 5: Build, lint, and check licenses**

Run: `cargo build -p supersigil-git -p supersigil-anchor -p supersigil-review`
Expected: all three compile; `Cargo.lock` gains `similar` 2.7.0, `proptest` and its dependencies, and the three crates.

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings (unused dependencies are not a lint in this workspace); the existing tests all pass.

Run: `cargo deny check licenses` (cargo-deny 0.19.1, the version `.github/workflows/audit.yml` installs; `cargo binstall cargo-deny@0.19.1` if missing)
Expected: `licenses ok`. The pre-existing "unmatched license allowance" note is not an error.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/supersigil-git/Cargo.toml crates/supersigil-git/src/lib.rs crates/supersigil-anchor/Cargo.toml crates/supersigil-anchor/src/lib.rs crates/supersigil-review/Cargo.toml crates/supersigil-review/src/lib.rs
git commit -m "feat: scaffold git, anchor, and review crates"
```

---

### Task 3: CI coverage

**Files:**
- Modify: `.github/workflows/ci.yml` (the `test` job)

**Interfaces:**
- Consumes: the existing `test` job (nextest on `ubuntu-latest` and `windows-latest`).
- Produces: nextest on both macOS runners the `check` job already uses (`macos-26-intel`, `macos-latest`), and a `Git version` step in every test job so each run's log names the git its tests exercised. Task 4 adds a separate job that runs the `supersigil-git` tests with git 2.36.0; it is not added here because the crate has no tests yet and `cargo nextest run` fails when no test matches.

Until now the macOS jobs only compiled and linted, so nothing ran the file-system and git behavior the later tasks test on a case-insensitive file system (APFS). The test matrix names match the `check` matrix's.

- [ ] **Step 1: Add the macOS runners to the test matrix**

In `.github/workflows/ci.yml`, in `jobs.test.strategy.matrix.include`, after the `windows` entry, add:

```yaml
          - name: x86_64-apple-darwin
            runner: macos-26-intel
          - name: aarch64-apple-darwin
            runner: macos-latest
```

The job's `include` list then reads:

```yaml
        include:
          - name: ubuntu
            runner: ubuntu-latest
          - name: windows
            runner: windows-latest
          - name: x86_64-apple-darwin
            runner: macos-26-intel
          - name: aarch64-apple-darwin
            runner: macos-latest
```

- [ ] **Step 2: Print the git version before the tests**

In the same job, replace the final step

```yaml
      - name: Test
        run: cargo nextest run
```

with:

```yaml
      - name: Git version
        run: git --version

      - name: Test
        run: cargo nextest run
```

- [ ] **Step 3: Validate the workflow**

Run: `actionlint .github/workflows/ci.yml` (install with `go install github.com/rhysd/actionlint/cmd/actionlint@latest` or your package manager if missing)
Expected: no output, exit status 0.

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: unchanged results; the workflow is not part of the Rust build.

The macOS jobs themselves only run on GitHub; the PR's CI shows them. If one fails there, that is a real platform difference for the task that introduced the failing test to fix, not a reason to drop the runner.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: run tests on macOS and print the git version"
```

---

### Task 4: Git runner, revisions, and worktrees

**Files:**
- Modify: `crates/supersigil-git/Cargo.toml` (dependencies)
- Modify: `crates/supersigil-git/src/lib.rs` (replaces the Task 2 scaffold)
- Create: `crates/supersigil-git/src/error.rs`, `crates/supersigil-git/src/run.rs`, `crates/supersigil-git/src/oid.rs`, `crates/supersigil-git/src/path.rs`, `crates/supersigil-git/src/repo.rs`, `crates/supersigil-git/src/worktree.rs`
- Create: `crates/supersigil-git/tests/common/mod.rs`, `crates/supersigil-git/tests/repo.rs`, `crates/supersigil-git/tests/worktree.rs`
- Modify: `.github/workflows/ci.yml` (a job that runs these tests with git 2.36.0)

**Interfaces:**
- Consumes: the `supersigil-git` scaffold from Task 2.
- Produces (every later git task and the CLI use these exact names):
  - `run`: `Git { new(cwd), with_env(key, value), in_dir(cwd), cwd(), command(), raw(args), output(args), output_with_input(args, input), version() }`, `GitVersion { major, minor, patch }` (ordered, `Display` as `2.36.0`), `MIN_VERSION`. Crate-private: `Git::moved_to(cwd)`, `run::io_error(context, source)`.
  - `error::GitError` with the variants of the interfaces contract.
  - `oid`: `ObjectFormat { Sha1, Sha256 }` with `hex_len()`, `ObjectId` with `parse(text, format)`, `as_str()`, `Display`, `Serialize` as the hex string. Crate-private: `ObjectFormat::parse`, `ObjectId::is_null`, `oid::parse_output(bytes, format)`.
  - `path::RepoPath` with `new`, `from_utf8`, `as_bytes`, `to_str`, `display`, `escaped`, `to_path`. Crate-private: `path::path_from_git(bytes)`.
  - `repo`: `Repo { open(git), git(), root(), format(), version(), resolve_commit(rev), commit_tree(commit), head(), first_parent(commit), empty_tree(), is_ancestor(a, d), commits_in_range(base, target), config_bool(key) }`, `TargetSpec`, `Base`, `Ancestry`, `ResolvedTarget`, `ResolvedRange { target_commit(), commits(repo) }`, `resolve_range(repo, base, target)`.
  - `worktree`: `Worktree { path, head, bare, prunable }`, `list_worktrees(repo)`.
  - Test support in `tests/common/mod.rs`: `isolated(git, home)` and `TestRepo { dir, root, new(), new_sha256(), home(), git(), repo(), run(args), run_in(dir, args), run_input(args, input), write(path, bytes), commit_all(message), tree_files(tree) }`. Tasks 5 to 7 add test files that use it unchanged.

Every test here runs real git in a temporary repository isolated from the machine's configuration (Global Constraints): `isolated` adds `GIT_CONFIG_NOSYSTEM=1`, `HOME` and `XDG_CONFIG_HOME` under the temporary directory, and a fixed identity to every `Git`, and `TestRepo` sets `core.autocrlf=false` in each repository. The product never sets any of these. Tests never call `std::env::set_var`.

- [ ] **Step 1: Set the crate's dependencies**

Make the dependency sections of `crates/supersigil-git/Cargo.toml` read:

```toml
[dependencies]
serde.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 2: Write the shared test support**

`crates/supersigil-git/tests/common/mod.rs`:

```rust
//! Temporary git repositories isolated from the machine's git configuration.
//!
//! Every git process here, and every [`Git`] handed to the crate under test,
//! gets `GIT_CONFIG_NOSYSTEM=1`, `HOME` and `XDG_CONFIG_HOME` pointing into the
//! test's temporary directory, and a fixed identity, so a developer's or a
//! CI runner's configuration (Git for Windows sets `core.autocrlf` system-wide)
//! cannot change what a test sees.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use supersigil_git::{Git, Repo};

/// Adds the isolation environment to `git`.
pub fn isolated(git: Git, home: &Path) -> Git {
    git.with_env("GIT_CONFIG_NOSYSTEM", "1")
        .with_env("HOME", home)
        .with_env("XDG_CONFIG_HOME", home)
        .with_env("GIT_AUTHOR_NAME", "Test")
        .with_env("GIT_AUTHOR_EMAIL", "test@example.com")
        .with_env("GIT_COMMITTER_NAME", "Test")
        .with_env("GIT_COMMITTER_EMAIL", "test@example.com")
}

/// A repository in a temporary directory, with `core.autocrlf=false`.
pub struct TestRepo {
    /// Holds the temporary directory: `home/` for configuration, `repo/` for
    /// the worktree, and room for linked worktrees beside it.
    pub dir: tempfile::TempDir,
    /// The worktree root.
    pub root: PathBuf,
}

impl TestRepo {
    /// `git init -b main` with SHA-1 objects.
    pub fn new() -> Self {
        Self::init(&[])
    }

    /// `git init -b main --object-format=sha256`.
    pub fn new_sha256() -> Self {
        Self::init(&["--object-format=sha256"])
    }

    fn init(extra: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("home")).unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let repo = Self { dir, root };
        let mut args = vec!["init", "-q", "-b", "main"];
        args.extend_from_slice(extra);
        repo.run(&args);
        repo.run(&["config", "core.autocrlf", "false"]);
        repo
    }

    /// The directory used as `HOME`.
    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    /// A [`Git`] for the worktree root with the isolation environment.
    pub fn git(&self) -> Git {
        isolated(Git::new(&self.root), &self.home())
    }

    /// Opens the worktree root as a [`Repo`].
    pub fn repo(&self) -> Repo {
        Repo::open(self.git()).unwrap()
    }

    /// Runs git in the worktree root; panics on failure; returns stdout.
    pub fn run(&self, args: &[&str]) -> String {
        self.run_in(&self.root, args)
    }

    /// Runs git in `dir` with the isolation environment; panics on failure.
    pub fn run_in(&self, dir: &Path, args: &[&str]) -> String {
        self.run_with_input(dir, args, None)
    }

    /// Runs git in the worktree root with `input` on stdin; panics on failure.
    pub fn run_input(&self, args: &[&str], input: &[u8]) -> String {
        self.run_with_input(&self.root, args, Some(input))
    }

    fn run_with_input(&self, dir: &Path, args: &[&str], input: Option<&[u8]>) -> String {
        let git = self.git().in_dir(dir);
        let output = match input {
            Some(input) => git.output_with_input(args, input),
            None => git.output(args),
        }
        .unwrap_or_else(|e| panic!("git {args:?} in {}: {e}", dir.display()));
        String::from_utf8(output).unwrap()
    }

    /// Writes `bytes` to `path` under the worktree root, creating directories.
    pub fn write(&self, path: &str, bytes: &[u8]) {
        let full = self.root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, bytes).unwrap();
    }

    /// `add -A` and commit; returns the new commit id.
    pub fn commit_all(&self, message: &str) -> String {
        self.run(&["add", "-A"]);
        self.run(&["commit", "-q", "--allow-empty", "-m", message]);
        self.run(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// Every file in `tree`, by path, with its contents as text.
    pub fn tree_files(&self, tree: &str) -> BTreeMap<String, String> {
        let names = self.run(&["ls-tree", "-r", "-z", "--name-only", tree]);
        names
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(|name| {
                let spec = format!("{tree}:{name}");
                (name.to_owned(), self.run(&["cat-file", "blob", &spec]))
            })
            .collect()
    }
}
```

- [ ] **Step 3: Write the failing integration tests**

`crates/supersigil-git/tests/repo.rs`. The empty tree id in `root_commit_target_has_an_empty_tree_base` is SHA-1's; `sha256_repository_resolves` checks the SHA-256 lengths instead. `revisions_that_are_not_commits_are_unknown` includes `--all` and `-h`, which `--end-of-options` keeps from being read as options:

```rust
//! Opening worktrees and resolving review ranges against real repositories.

mod common;

use std::path::Path;

use common::{TestRepo, isolated};
use supersigil_git::{
    Ancestry, Git, GitError, MIN_VERSION, ObjectFormat, Repo, ResolvedTarget, TargetSpec,
    resolve_range,
};

fn same_dir(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).unwrap() == std::fs::canonicalize(b).unwrap()
}

#[test]
fn installed_git_meets_the_minimum() {
    let repo = TestRepo::new();
    assert!(repo.repo().version() >= MIN_VERSION);
}

#[test]
fn open_finds_the_root_from_a_subdirectory() {
    let repo = TestRepo::new();
    repo.write("a/b/file.txt", b"x\n");
    let opened = Repo::open(repo.git().in_dir(repo.root.join("a/b"))).unwrap();
    assert!(same_dir(opened.root(), &repo.root));
    assert!(same_dir(opened.git().cwd(), &repo.root));
    assert_eq!(opened.format(), ObjectFormat::Sha1);
}

#[test]
fn open_outside_a_worktree_fails() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let git =
        isolated(Git::new(&plain), dir.path()).with_env("GIT_CEILING_DIRECTORIES", dir.path());
    assert!(matches!(Repo::open(git), Err(GitError::NotAWorktree(_))));
}

#[test]
fn open_inside_the_git_directory_fails() {
    let repo = TestRepo::new();
    let result = Repo::open(repo.git().in_dir(repo.root.join(".git")));
    assert!(
        matches!(result, Err(GitError::NotAWorktree(_))),
        "{result:?}"
    );
}

#[test]
fn bare_repository_is_rejected() {
    let repo = TestRepo::new();
    let bare = repo.dir.path().join("bare.git");
    repo.run(&["init", "-q", "--bare", bare.to_str().unwrap()]);
    let result = Repo::open(repo.git().in_dir(&bare));
    assert!(matches!(result, Err(GitError::Bare(_))), "{result:?}");
}

#[test]
fn sha256_repository_resolves() {
    let repo = TestRepo::new_sha256();
    repo.write("a.txt", b"a\n");
    let first = repo.commit_all("first");
    repo.write("a.txt", b"b\n");
    let second = repo.commit_all("second");
    let opened = repo.repo();
    assert_eq!(opened.format(), ObjectFormat::Sha256);
    let head = opened.resolve_commit("HEAD").unwrap();
    assert_eq!(head.as_str(), second);
    assert_eq!(head.as_str().len(), 64);
    assert_eq!(opened.empty_tree().unwrap().as_str().len(), 64);
    let range = resolve_range(&opened, None, &TargetSpec::Commit("HEAD".into())).unwrap();
    assert_eq!(range.base.commit.unwrap().as_str(), first);
    assert_eq!(range.ancestry, Ancestry::Ancestor);
}

#[test]
fn working_tree_defaults_to_a_head_base() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let head = repo.commit_all("first");
    let opened = repo.repo();
    let range = resolve_range(&opened, None, &TargetSpec::WorkingTree).unwrap();
    assert_eq!(range.base.commit.as_ref().unwrap().as_str(), head);
    let tree = repo.run(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(range.base.tree.as_str(), tree.trim());
    assert!(
        matches!(&range.target, ResolvedTarget::WorkingTree { head: Some(h) } if h.as_str() == head)
    );
    assert_eq!(range.ancestry, Ancestry::Ancestor);
    assert!(range.commits(&opened).unwrap().is_empty());
}

#[test]
fn commit_target_defaults_to_its_first_parent() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let first = repo.commit_all("first");
    repo.write("a.txt", b"b\n");
    let second = repo.commit_all("second");
    let opened = repo.repo();
    let range = resolve_range(&opened, None, &TargetSpec::Commit("HEAD".into())).unwrap();
    assert_eq!(range.base.commit.as_ref().unwrap().as_str(), first);
    assert_eq!(range.target_commit().unwrap().as_str(), second);
    let commits: Vec<String> = range
        .commits(&opened)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(commits, [second]);
}

#[test]
fn root_commit_target_has_an_empty_tree_base() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let root = repo.commit_all("root");
    let opened = repo.repo();
    let range = resolve_range(&opened, None, &TargetSpec::Commit(root.clone())).unwrap();
    assert_eq!(range.base.commit, None);
    assert_eq!(
        range.base.tree.as_str(),
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    );
    assert_eq!(range.ancestry, Ancestry::Unavailable);
    let commits: Vec<String> = range
        .commits(&opened)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(commits, [root]);
}

#[test]
fn unborn_head_without_a_base_reviews_against_the_empty_tree() {
    let repo = TestRepo::new();
    let opened = repo.repo();
    assert_eq!(opened.head().unwrap(), None);
    let range = resolve_range(&opened, None, &TargetSpec::WorkingTree).unwrap();
    assert_eq!(range.base.commit, None);
    assert_eq!(range.base.tree, opened.empty_tree().unwrap());
    assert_eq!(range.target, ResolvedTarget::WorkingTree { head: None });
    assert_eq!(range.ancestry, Ancestry::Unavailable);
    assert!(range.commits(&opened).unwrap().is_empty());
}

#[test]
fn unborn_head_with_an_explicit_base_has_no_ancestry() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let main = repo.commit_all("first");
    repo.run(&["switch", "-q", "--orphan", "other"]);
    let opened = repo.repo();
    let range = resolve_range(&opened, Some("main"), &TargetSpec::WorkingTree).unwrap();
    assert_eq!(range.base.commit.unwrap().as_str(), main);
    assert_eq!(range.target, ResolvedTarget::WorkingTree { head: None });
    assert_eq!(range.ancestry, Ancestry::Unavailable);
}

#[test]
fn base_that_is_not_an_ancestor_is_reported() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("s.txt", b"s\n");
    let side = repo.commit_all("side");
    repo.run(&["switch", "-q", "main"]);
    repo.write("m.txt", b"m\n");
    repo.commit_all("main moves on");
    repo.run(&["switch", "-q", "side"]);
    let opened = repo.repo();
    let range = resolve_range(&opened, Some("main"), &TargetSpec::WorkingTree).unwrap();
    assert_eq!(range.ancestry, Ancestry::NotAncestor);
    let commits: Vec<String> = range
        .commits(&opened)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(commits, [side]);
}

#[test]
fn revisions_that_are_not_commits_are_unknown() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    let opened = repo.repo();
    let tree = repo.run(&["rev-parse", "HEAD^{tree}"]);
    for rev in ["nope", tree.trim(), "--all", "-h"] {
        let result = resolve_range(&opened, Some(rev), &TargetSpec::WorkingTree);
        assert!(
            matches!(result, Err(GitError::UnknownRevision(ref r)) if r == rev),
            "{rev}: {result:?}"
        );
    }
}

#[test]
fn boolean_config_is_read_and_unset_is_none() {
    let repo = TestRepo::new();
    let opened = repo.repo();
    assert_eq!(opened.config_bool("core.autocrlf").unwrap(), Some(false));
    assert_eq!(opened.config_bool("supersigil.unset").unwrap(), None);
    repo.run(&["config", "core.ignorecase", "true"]);
    assert_eq!(opened.config_bool("core.ignorecase").unwrap(), Some(true));
}
```

`crates/supersigil-git/tests/worktree.rs`:

```rust
//! Listing worktrees against real repositories.

mod common;

use std::path::Path;

use common::TestRepo;
use supersigil_git::worktree::list_worktrees;

fn same_dir(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).unwrap() == std::fs::canonicalize(b).unwrap()
}

#[test]
fn worktrees_are_listed_main_first_with_their_heads() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let head = repo.commit_all("first");
    let linked = repo.dir.path().join("linked tree");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        linked.to_str().unwrap(),
    ]);
    let worktrees = list_worktrees(&repo.repo()).unwrap();
    assert_eq!(worktrees.len(), 2);
    assert!(same_dir(&worktrees[0].path, &repo.root));
    assert!(same_dir(&worktrees[1].path, &linked));
    for worktree in &worktrees {
        assert_eq!(worktree.head.as_ref().unwrap().as_str(), head);
        assert!(!worktree.bare);
        assert!(!worktree.prunable);
    }
}

#[test]
fn a_worktree_whose_directory_is_gone_is_prunable() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    let linked = repo.dir.path().join("gone");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "gone",
        linked.to_str().unwrap(),
    ]);
    std::fs::remove_dir_all(&linked).unwrap();
    let worktrees = list_worktrees(&repo.repo()).unwrap();
    assert_eq!(worktrees.len(), 2);
    assert!(!worktrees[0].prunable);
    assert!(worktrees[1].prunable);
}

#[test]
fn an_unborn_worktree_has_no_head() {
    let repo = TestRepo::new();
    let worktrees = list_worktrees(&repo.repo()).unwrap();
    assert_eq!(worktrees.len(), 1);
    assert_eq!(worktrees[0].head, None);
}

#[test]
fn a_nested_worktree_is_listed_with_its_own_path() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("first");
    let nested = repo.root.join(".claude/worktrees/feature");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        nested.to_str().unwrap(),
    ]);
    let worktrees = list_worktrees(&repo.repo()).unwrap();
    assert!(worktrees.iter().any(|w| same_dir(&w.path, &nested)));
    assert!(worktrees.iter().any(|w| same_dir(&w.path, &repo.root)));
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-git`
Expected: FAIL to compile with `unresolved import` errors for `supersigil_git::Ancestry`, `supersigil_git::worktree`, and the rest of the API.

- [ ] **Step 5: Write the error type**

`crates/supersigil-git/src/error.rs`:

```rust
//! Errors from running git and reading its output.

use std::path::PathBuf;

use crate::run::GitVersion;

/// Failure to run git, or git output this crate cannot use.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// The `git` program could not be started.
    #[error("git was not found on PATH")]
    NotFound(#[source] std::io::Error),
    /// The installed git is older than [`crate::MIN_VERSION`].
    #[error("git {found} is too old; supersigil needs git {required} or newer")]
    TooOld {
        /// Version reported by `git version`.
        found: GitVersion,
        /// Oldest supported version.
        required: GitVersion,
    },
    /// Git exited unsuccessfully.
    #[error("`git {args}` failed ({}): {stderr}", status_text(*.status))]
    Failed {
        /// Arguments passed to git, joined with spaces.
        args: String,
        /// Exit code, or `None` when git was ended by a signal.
        status: Option<i32>,
        /// Standard error, trimmed.
        stderr: String,
    },
    /// An I/O operation around a git call failed.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Git printed something this crate does not understand.
    #[error("unexpected git output: {0}")]
    Parse(String),
    /// The directory is not inside a git worktree.
    #[error("{} is not inside a git worktree", .0.display())]
    NotAWorktree(PathBuf),
    /// The directory is a bare repository, which has no worktree to review.
    #[error("{} is a bare repository; review needs a worktree", .0.display())]
    Bare(PathBuf),
    /// A revision did not resolve to a commit.
    #[error("unknown revision: {0}")]
    UnknownRevision(String),
    /// A path given to include as untracked could not be included.
    #[error("cannot include untracked path {path}: {reason}")]
    Untracked {
        /// The path as given.
        path: String,
        /// Why it cannot be included.
        reason: String,
    },
}

/// Describes an exit status for an error message.
fn status_text(status: Option<i32>) -> String {
    status.map_or_else(
        || "killed by a signal".to_owned(),
        |code| format!("exit {code}"),
    )
}
```

- [ ] **Step 6: Write the runner**

`crates/supersigil-git/src/run.rs`. `SCRUBBED` is removed from every child before the extra environment is applied, so a test (or Task 5's temporary index) can still set one of those variables deliberately. `output_with_input` writes standard input from a scoped thread so git can fill its output pipe while reading; a broken pipe on standard input is not an error because git's exit status already says what happened:

```rust
//! Runs the git CLI with a controlled environment.
//!
//! Every child process loses the variables that would point git at another
//! repository or index, and gets `GIT_OPTIONAL_LOCKS=0` so read commands never
//! refresh and rewrite the real index. The user's configuration is otherwise
//! honored; only tests add isolation through [`Git::with_env`].

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::error::GitError;

/// Variables removed from every git child process: each could make git read
/// or write a different repository or index than the one in the working
/// directory. Git sets them for hooks, so a review started from a hook would
/// otherwise inherit them.
const SCRUBBED: [&str; 4] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
];

/// Oldest git this crate supports: `worktree list -z` arrived in 2.36.
pub const MIN_VERSION: GitVersion = GitVersion {
    major: 2,
    minor: 36,
    patch: 0,
};

/// A git version, as reported by `git version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitVersion {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch version, 0 when git did not report one.
    pub patch: u32,
}

impl fmt::Display for GitVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl GitVersion {
    /// Parses the output of `git version`, for example `git version 2.39.3
    /// (Apple Git-146)` or `git version 2.44.0.windows.1`.
    fn parse(text: &str) -> Option<Self> {
        let version = text.trim().strip_prefix("git version ")?;
        let mut parts = version
            .split_whitespace()
            .next()?
            .split('.')
            .map(leading_number);
        Some(Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next().flatten().unwrap_or(0),
        })
    }
}

/// Parses the digits at the start of `part`, or `None` if there are none.
fn leading_number(part: &str) -> Option<u32> {
    let end = part
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(part.len());
    part[..end].parse().ok()
}

/// Runs git in one directory with the scrubbed environment plus any extra
/// variables added with [`Git::with_env`].
#[derive(Debug, Clone)]
pub struct Git {
    cwd: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl Git {
    /// Creates a runner for `cwd` with no extra environment.
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            env: Vec::new(),
        }
    }

    /// Adds a variable to every child process, after the scrubbing, so it
    /// can also set one of the scrubbed variables deliberately.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Returns a runner for another directory with the same extra environment.
    #[must_use]
    pub fn in_dir(&self, cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            env: self.env.clone(),
        }
    }

    /// Moves this runner to another directory, keeping its extra environment.
    #[must_use]
    pub(crate) fn moved_to(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    /// Returns the directory git runs in.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Returns a `git` command with the scrubbed environment, the extra
    /// variables, and the working directory set.
    #[must_use]
    pub fn command(&self) -> Command {
        let mut command = Command::new("git");
        for key in SCRUBBED {
            command.env_remove(key);
        }
        command.env("GIT_OPTIONAL_LOCKS", "0");
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command.current_dir(&self.cwd);
        command
    }

    /// Runs git with `args` and returns its output whatever the exit status.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::NotFound`] if git cannot be started or
    /// [`GitError::Io`] if waiting for it fails.
    pub fn raw<I, S>(&self, args: I) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run(args, None)
    }

    /// Runs git with `args` and returns standard output.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`] if git exits unsuccessfully, or the errors
    /// of [`Git::raw`].
    pub fn output<I, S>(&self, args: I) -> Result<Vec<u8>, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = collect(args);
        let output = self.run(&args, None)?;
        succeeded(&args, output)
    }

    /// Runs git with `args`, writes `input` to its standard input, and returns
    /// standard output. Input is written from a second thread, so git can
    /// produce output of any size while it reads.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`] if git exits unsuccessfully, or the errors
    /// of [`Git::raw`].
    pub fn output_with_input<I, S>(&self, args: I, input: &[u8]) -> Result<Vec<u8>, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = collect(args);
        let output = self.run(&args, Some(input))?;
        succeeded(&args, output)
    }

    /// Runs `git version` and parses the result.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Parse`] if the output is not a version, or the
    /// errors of [`Git::output`].
    pub fn version(&self) -> Result<GitVersion, GitError> {
        let output = self.output(["version"])?;
        let text = String::from_utf8_lossy(&output);
        GitVersion::parse(&text).ok_or_else(|| GitError::Parse(format!("git version: {text}")))
    }

    /// Spawns git, optionally feeding `input`, and waits for it.
    fn run<I, S>(&self, args: I, input: Option<&[u8]>) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command();
        command
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::NotFound(e)
            } else {
                io_error("cannot start git", e)
            }
        })?;
        let Some(input) = input else {
            return child
                .wait_with_output()
                .map_err(|e| io_error("waiting for git", e));
        };
        let mut stdin = child.stdin.take().expect("stdin is piped");
        std::thread::scope(|scope| {
            let writer = scope.spawn(move || stdin.write_all(input));
            let output = child
                .wait_with_output()
                .map_err(|e| io_error("waiting for git", e))?;
            match writer.join().expect("the stdin writer does not panic") {
                // Git may exit before reading everything, for example on an
                // error it then reports; its exit status says what happened.
                Ok(()) => Ok(output),
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(output),
                Err(e) => Err(io_error("writing to git", e)),
            }
        })
    }
}

/// Collects arguments so they can be both passed to git and quoted in errors.
fn collect<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter().map(|a| a.as_ref().to_owned()).collect()
}

/// Returns standard output, or [`GitError::Failed`] if git exited unsuccessfully.
fn succeeded(args: &[OsString], output: Output) -> Result<Vec<u8>, GitError> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(GitError::Failed {
        args: args
            .iter()
            .map(|a| a.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" "),
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

/// Wraps an I/O error with what was being done.
pub(crate) fn io_error(context: &str, source: std::io::Error) -> GitError {
    GitError::Io {
        context: context.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_across_platform_suffixes() {
        let v = |major, minor, patch| GitVersion {
            major,
            minor,
            patch,
        };
        assert_eq!(GitVersion::parse("git version 2.55.0\n"), Some(v(2, 55, 0)));
        assert_eq!(
            GitVersion::parse("git version 2.39.3 (Apple Git-146)"),
            Some(v(2, 39, 3))
        );
        assert_eq!(
            GitVersion::parse("git version 2.44.0.windows.1"),
            Some(v(2, 44, 0))
        );
        assert_eq!(GitVersion::parse("git version 2.36"), Some(v(2, 36, 0)));
        assert_eq!(
            GitVersion::parse("git version 2.36.0.rc1"),
            Some(v(2, 36, 0))
        );
        assert_eq!(GitVersion::parse("hello"), None);
    }

    #[test]
    fn minimum_is_ordered_below_newer_versions() {
        assert!(
            MIN_VERSION
                < GitVersion {
                    major: 2,
                    minor: 55,
                    patch: 0
                }
        );
        assert!(
            MIN_VERSION
                > GitVersion {
                    major: 2,
                    minor: 35,
                    patch: 9
                }
        );
        assert_eq!(MIN_VERSION.to_string(), "2.36.0");
    }

    #[test]
    fn command_scrubs_repository_variables() {
        let git = Git::new("/tmp").with_env("EXTRA", "1");
        let command = git.command();
        let envs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        for key in SCRUBBED {
            assert!(envs.contains(&(key.to_owned(), None)), "{key} not removed");
        }
        assert!(envs.contains(&("GIT_OPTIONAL_LOCKS".to_owned(), Some("0".to_owned()))));
        assert!(envs.contains(&("EXTRA".to_owned(), Some("1".to_owned()))));
    }

    #[test]
    fn extra_env_can_set_a_scrubbed_variable_deliberately() {
        let command = Git::new("/tmp")
            .with_env("GIT_INDEX_FILE", "/tmp/index")
            .command();
        let value = command
            .get_envs()
            .filter(|(k, _)| *k == "GIT_INDEX_FILE")
            .last()
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()));
        assert_eq!(value.as_deref(), Some("/tmp/index"));
    }
}
```

- [ ] **Step 7: Write object ids and paths**

`crates/supersigil-git/src/oid.rs`:

```rust
//! Object ids in a repository's hash format.

use std::fmt;

use crate::error::GitError;

/// Hash format of a repository's objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectFormat {
    /// SHA-1, 40 hex digits.
    Sha1,
    /// SHA-256, 64 hex digits.
    Sha256,
}

impl ObjectFormat {
    /// Returns the number of hex digits in an object id of this format.
    #[must_use]
    pub const fn hex_len(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }

    /// Parses the output of `rev-parse --show-object-format`.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Parse`] for any other text.
    pub(crate) fn parse(text: &str) -> Result<Self, GitError> {
        match text.trim() {
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            other => Err(GitError::Parse(format!("object format {other:?}"))),
        }
    }
}

/// A full object id: lowercase hex of the repository's object format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(transparent)]
pub struct ObjectId(String);

impl ObjectId {
    /// Validates `text` as a full object id of `format`.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Parse`] unless `text` is exactly
    /// [`ObjectFormat::hex_len`] lowercase hex digits.
    pub fn parse(text: &str, format: ObjectFormat) -> Result<Self, GitError> {
        let valid = text.len() == format.hex_len()
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(GitError::Parse(format!("object id {text:?}")))
        }
    }

    /// Returns the hex digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether every digit is zero, git's placeholder for "no object".
    #[must_use]
    pub(crate) fn is_null(&self) -> bool {
        self.0.bytes().all(|b| b == b'0')
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Parses git output holding one object id, ignoring the trailing newline.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if the output is not one id of `format`.
pub(crate) fn parse_output(output: &[u8], format: ObjectFormat) -> Result<ObjectId, GitError> {
    let text = std::str::from_utf8(output)
        .map_err(|_not_utf8| GitError::Parse("object id is not UTF-8".to_owned()))?;
    ObjectId::parse(text.trim_end(), format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_checked_against_the_format() {
        let sha1 = "a".repeat(40);
        let sha256 = "0123456789abcdef".repeat(4);
        assert_eq!(
            ObjectId::parse(&sha1, ObjectFormat::Sha1).unwrap().as_str(),
            sha1
        );
        ObjectId::parse(&sha1, ObjectFormat::Sha256).unwrap_err();
        ObjectId::parse(&sha256, ObjectFormat::Sha256).unwrap();
        ObjectId::parse(&"A".repeat(40), ObjectFormat::Sha1).unwrap_err();
        ObjectId::parse(&"g".repeat(40), ObjectFormat::Sha1).unwrap_err();
        ObjectId::parse("", ObjectFormat::Sha1).unwrap_err();
    }

    #[test]
    fn formats_parse_and_know_their_length() {
        assert_eq!(ObjectFormat::parse("sha1\n").unwrap(), ObjectFormat::Sha1);
        assert_eq!(ObjectFormat::parse("sha256").unwrap().hex_len(), 64);
        ObjectFormat::parse("md5").unwrap_err();
    }

    #[test]
    fn the_null_id_is_recognized() {
        assert!(
            ObjectId::parse(&"0".repeat(40), ObjectFormat::Sha1)
                .unwrap()
                .is_null()
        );
        assert!(
            !ObjectId::parse(&"1".repeat(40), ObjectFormat::Sha1)
                .unwrap()
                .is_null()
        );
    }
}
```

`crates/supersigil-git/src/path.rs` (Task 5 adds the pathspec helpers):

```rust
//! Repository-relative paths as git stores them: raw bytes.

use std::path::PathBuf;

/// A path relative to the repository root, `/`-separated, as the raw bytes
/// git prints in `-z` output. Git allows any bytes except NUL, so a path is
/// not necessarily UTF-8; the bytes stay its identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepoPath(Vec<u8>);

impl RepoPath {
    /// Wraps raw path bytes.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Wraps a UTF-8 path such as `src/lib.rs`.
    #[must_use]
    pub fn from_utf8(path: &str) -> Self {
        Self(path.as_bytes().to_vec())
    }

    /// Returns the raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the path as text, or `None` when it is not UTF-8.
    #[must_use]
    pub fn to_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// Returns the path for display, replacing invalid UTF-8 with U+FFFD.
    /// Two different paths can display the same; use [`RepoPath::escaped`]
    /// where identity matters.
    #[must_use]
    pub fn display(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }

    /// Returns a reversible ASCII form: `\` becomes `\\`, and every byte
    /// outside printable ASCII (`0x20..=0x7e`) becomes `\xNN` with two
    /// lowercase hex digits.
    #[must_use]
    pub fn escaped(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(self.0.len());
        for &byte in &self.0 {
            match byte {
                b'\\' => out.push_str("\\\\"),
                0x20..=0x7e => out.push(char::from(byte)),
                _ => {
                    out.push_str("\\x");
                    out.push(char::from(HEX[usize::from(byte >> 4)]));
                    out.push(char::from(HEX[usize::from(byte & 0x0f)]));
                }
            }
        }
        out
    }

    /// Returns the path as a relative [`PathBuf`], or `None` when this
    /// platform cannot represent its bytes (non-UTF-8 on Windows).
    #[must_use]
    pub fn to_path(&self) -> Option<PathBuf> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;
            Some(PathBuf::from(std::ffi::OsString::from_vec(self.0.clone())))
        }
        #[cfg(not(unix))]
        {
            self.to_str().map(PathBuf::from)
        }
    }
}

/// Converts path bytes printed by git into a [`PathBuf`], or `None` when the
/// platform cannot represent them.
pub(crate) fn path_from_git(bytes: &[u8]) -> Option<PathBuf> {
    RepoPath::new(bytes.to_vec()).to_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_is_reversible_and_ascii() {
        let path = RepoPath::new(b"dir\\a b/\xff\x01\xc3\xa9.txt".to_vec());
        assert_eq!(path.escaped(), r"dir\\a b/\xff\x01\xc3\xa9.txt");
        assert!(path.escaped().is_ascii());
        assert_eq!(RepoPath::from_utf8("src/lib.rs").escaped(), "src/lib.rs");
    }

    #[test]
    fn non_utf8_paths_display_lossily_but_keep_their_bytes() {
        let a = RepoPath::new(b"x\xff".to_vec());
        let b = RepoPath::new(b"x\xfe".to_vec());
        assert_eq!(a.display(), b.display());
        assert_ne!(a, b);
        assert_ne!(a.escaped(), b.escaped());
        assert_eq!(a.to_str(), None);
        assert_eq!(RepoPath::from_utf8("é").to_str(), Some("é"));
    }
}
```

- [ ] **Step 8: Write the repository and range resolution**

`crates/supersigil-git/src/repo.rs`. `Repo::open` checks the version before anything else, rejects a bare repository, and treats a failing `--show-toplevel` (for example inside `.git`) as not a worktree. `resolve_commit` uses `--end-of-options` so a revision can never be read as an option. The empty tree is written with `mktree` because its id depends on the object format:

```rust
//! Opens a worktree and resolves the base and target of a review.

use std::path::{Path, PathBuf};

use crate::error::GitError;
use crate::oid::{ObjectFormat, ObjectId, parse_output};
use crate::path::path_from_git;
use crate::run::{Git, GitVersion, MIN_VERSION};

/// A git worktree opened for review: its root, object format, and git version.
#[derive(Debug, Clone)]
pub struct Repo {
    git: Git,
    root: PathBuf,
    format: ObjectFormat,
    version: GitVersion,
}

impl Repo {
    /// Opens the worktree containing `git`'s directory.
    ///
    /// Checks the git version first, then that the directory is inside a
    /// worktree of a non-bare repository.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::NotFound`] or [`GitError::TooOld`] for an unusable
    /// git, [`GitError::Bare`] for a bare repository,
    /// [`GitError::NotAWorktree`] outside any worktree (including inside a
    /// `.git` directory), or the errors of [`Git::output`].
    pub fn open(git: Git) -> Result<Self, GitError> {
        let version = git.version()?;
        if version < MIN_VERSION {
            return Err(GitError::TooOld {
                found: version,
                required: MIN_VERSION,
            });
        }
        let bare = git.raw(["rev-parse", "--is-bare-repository"])?;
        if !bare.status.success() {
            return Err(GitError::NotAWorktree(git.cwd().to_path_buf()));
        }
        if bare.stdout.trim_ascii() == b"true" {
            return Err(GitError::Bare(git.cwd().to_path_buf()));
        }
        let top = git.raw(["rev-parse", "--show-toplevel"])?;
        let root = top
            .status
            .success()
            .then(|| path_from_git(top.stdout.trim_ascii_end()))
            .flatten()
            .filter(|root| !root.as_os_str().is_empty())
            .ok_or_else(|| GitError::NotAWorktree(git.cwd().to_path_buf()))?;
        let format_output = git.output(["rev-parse", "--show-object-format"])?;
        let format = ObjectFormat::parse(&String::from_utf8_lossy(&format_output))?;
        Ok(Self {
            git: git.moved_to(&root),
            root,
            format,
            version,
        })
    }

    /// Returns a runner whose directory is the worktree root.
    #[must_use]
    pub fn git(&self) -> &Git {
        &self.git
    }

    /// Returns the worktree root as git prints it.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the repository's object format.
    #[must_use]
    pub fn format(&self) -> ObjectFormat {
        self.format
    }

    /// Returns the git version found when the repository was opened.
    #[must_use]
    pub fn version(&self) -> GitVersion {
        self.version
    }

    /// Resolves `rev` to a commit id.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::UnknownRevision`] if `rev` does not name a commit,
    /// or the errors of [`Git::raw`].
    pub fn resolve_commit(&self, rev: &str) -> Result<ObjectId, GitError> {
        let spec = format!("{rev}^{{commit}}");
        let output = self.git.raw([
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &spec,
        ])?;
        if !output.status.success() {
            return Err(GitError::UnknownRevision(rev.to_owned()));
        }
        parse_output(&output.stdout, self.format)
    }

    /// Returns the tree of `commit`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Git::output`], or [`GitError::Parse`] for
    /// output that is not an object id.
    pub fn commit_tree(&self, commit: &ObjectId) -> Result<ObjectId, GitError> {
        let spec = format!("{commit}^{{tree}}");
        let output = self.git.output(["rev-parse", "--verify", &spec])?;
        parse_output(&output, self.format)
    }

    /// Returns the commit HEAD points at, or `None` for an unborn branch.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Git::raw`], or [`GitError::Parse`] for output
    /// that is not an object id.
    pub fn head(&self) -> Result<Option<ObjectId>, GitError> {
        let output = self
            .git
            .raw(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])?;
        if !output.status.success() {
            return Ok(None);
        }
        parse_output(&output.stdout, self.format).map(Some)
    }

    /// Returns the first parent of `commit`, or `None` for a root commit.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Git::output`], or [`GitError::Parse`] for
    /// output that is not a list of object ids.
    pub fn first_parent(&self, commit: &ObjectId) -> Result<Option<ObjectId>, GitError> {
        let output = self
            .git
            .output(["rev-list", "--parents", "-n", "1", commit.as_str()])?;
        let text = String::from_utf8_lossy(&output);
        text.split_whitespace()
            .nth(1)
            .map(|parent| ObjectId::parse(parent, self.format))
            .transpose()
    }

    /// Writes the empty tree and returns its id. The id depends on the object
    /// format, so it is asked of git rather than hard-coded.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Git::output_with_input`], or
    /// [`GitError::Parse`] for output that is not an object id.
    pub fn empty_tree(&self) -> Result<ObjectId, GitError> {
        let output = self.git.output_with_input(["mktree"], b"")?;
        parse_output(&output, self.format)
    }

    /// Returns whether `ancestor` is an ancestor of `descendant` (a commit is
    /// its own ancestor).
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`] for an exit status other than 0 or 1, or
    /// the errors of [`Git::raw`].
    pub fn is_ancestor(
        &self,
        ancestor: &ObjectId,
        descendant: &ObjectId,
    ) -> Result<bool, GitError> {
        let output = self.git.raw([
            "merge-base",
            "--is-ancestor",
            ancestor.as_str(),
            descendant.as_str(),
        ])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            status => Err(GitError::Failed {
                args: format!("merge-base --is-ancestor {ancestor} {descendant}"),
                status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            }),
        }
    }

    /// Returns the commits reachable from `target` but not from `base`,
    /// newest first; every commit reachable from `target` when `base` is
    /// `None`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Git::output`], or [`GitError::Parse`] for
    /// output that is not a list of object ids.
    pub fn commits_in_range(
        &self,
        base: Option<&ObjectId>,
        target: &ObjectId,
    ) -> Result<Vec<ObjectId>, GitError> {
        let mut args = vec!["rev-list".to_owned(), target.to_string()];
        if let Some(base) = base {
            args.push(format!("^{base}"));
        }
        let output = self.git.output(&args)?;
        String::from_utf8_lossy(&output)
            .lines()
            .map(|line| ObjectId::parse(line, self.format))
            .collect()
    }

    /// Reads a boolean configuration value, or `None` when it is unset.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::Failed`] when git rejects the key or the value is
    /// not a boolean, or the errors of [`Git::raw`].
    pub fn config_bool(&self, key: &str) -> Result<Option<bool>, GitError> {
        let output = self.git.raw(["config", "--type=bool", "--get", key])?;
        match output.status.code() {
            Some(0) => match output.stdout.trim_ascii() {
                b"true" => Ok(Some(true)),
                b"false" => Ok(Some(false)),
                other => Err(GitError::Parse(format!(
                    "config {key}: {}",
                    String::from_utf8_lossy(other)
                ))),
            },
            Some(1) => Ok(None),
            status => Err(GitError::Failed {
                args: format!("config --type=bool --get {key}"),
                status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            }),
        }
    }
}

/// What the review compares against the base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetSpec {
    /// The worktree's on-disk state, captured by
    /// [`crate::snapshot::snapshot_working_tree`].
    WorkingTree,
    /// A revision naming a commit.
    Commit(String),
}

/// The base of a review: a commit and its tree, or the empty tree with no
/// commit (for a root commit target or an unborn HEAD).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    /// The base commit, `None` for the empty tree.
    pub commit: Option<ObjectId>,
    /// The base tree.
    pub tree: ObjectId,
}

/// Whether the base commit is an ancestor of the target commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ancestry {
    /// The base is an ancestor: the diff shows only changes made on the
    /// target side.
    Ancestor,
    /// The base is not an ancestor: the diff also shows changes made on the
    /// base side.
    NotAncestor,
    /// One endpoint is not a commit (an empty-tree base or an unborn HEAD).
    Unavailable,
}

/// The resolved target of a review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTarget {
    /// The working tree; its tree is written later by
    /// [`crate::snapshot::snapshot_working_tree`].
    WorkingTree {
        /// The commit HEAD points at, `None` when unborn.
        head: Option<ObjectId>,
    },
    /// A commit.
    Commit {
        /// The target commit.
        commit: ObjectId,
        /// Its tree.
        tree: ObjectId,
    },
}

/// Base, target, and their ancestry for one review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRange {
    /// The base.
    pub base: Base,
    /// The target.
    pub target: ResolvedTarget,
    /// Whether the base commit is an ancestor of the target commit.
    pub ancestry: Ancestry,
}

impl ResolvedRange {
    /// Returns the target commit, or HEAD for a working-tree target; `None`
    /// when HEAD is unborn.
    #[must_use]
    pub fn target_commit(&self) -> Option<&ObjectId> {
        match &self.target {
            ResolvedTarget::WorkingTree { head } => head.as_ref(),
            ResolvedTarget::Commit { commit, .. } => Some(commit),
        }
    }

    /// Returns the commits in base..target, newest first: every commit
    /// reachable from the target commit when the base is the empty tree, and
    /// none when HEAD is unborn.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Repo::commits_in_range`].
    pub fn commits(&self, repo: &Repo) -> Result<Vec<ObjectId>, GitError> {
        match self.target_commit() {
            Some(target) => repo.commits_in_range(self.base.commit.as_ref(), target),
            None => Ok(Vec::new()),
        }
    }
}

/// Resolves the base and target of a review.
///
/// The base defaults to HEAD for a working-tree target and to the first
/// parent for a commit target; it is the empty tree for a root commit target
/// or an unborn HEAD. An explicit base is used as given (two-dot, no
/// merge-base). Ancestry is checked only when both endpoints are commits.
///
/// # Errors
///
/// Returns [`GitError::UnknownRevision`] if the base or target does not name
/// a commit, or the errors of the [`Repo`] calls it makes.
pub fn resolve_range(
    repo: &Repo,
    base: Option<&str>,
    target: &TargetSpec,
) -> Result<ResolvedRange, GitError> {
    let target = match target {
        TargetSpec::WorkingTree => ResolvedTarget::WorkingTree { head: repo.head()? },
        TargetSpec::Commit(rev) => {
            let commit = repo.resolve_commit(rev)?;
            let tree = repo.commit_tree(&commit)?;
            ResolvedTarget::Commit { commit, tree }
        }
    };
    let base_commit = match (base, &target) {
        (Some(rev), _) => Some(repo.resolve_commit(rev)?),
        (None, ResolvedTarget::WorkingTree { head }) => head.clone(),
        (None, ResolvedTarget::Commit { commit, .. }) => repo.first_parent(commit)?,
    };
    let base = match base_commit {
        Some(commit) => Base {
            tree: repo.commit_tree(&commit)?,
            commit: Some(commit),
        },
        None => Base {
            commit: None,
            tree: repo.empty_tree()?,
        },
    };
    let mut range = ResolvedRange {
        base,
        target,
        ancestry: Ancestry::Unavailable,
    };
    if let (Some(base), Some(target)) = (&range.base.commit, range.target_commit()) {
        range.ancestry = if repo.is_ancestor(base, target)? {
            Ancestry::Ancestor
        } else {
            Ancestry::NotAncestor
        };
    }
    Ok(range)
}
```

- [ ] **Step 9: Write the worktree listing**

`crates/supersigil-git/src/worktree.rs`. With `-z`, every porcelain attribute ends in NUL and an empty attribute ends a worktree; an all-zero `HEAD` is an unborn branch:

```rust
//! Lists a repository's worktrees.

use std::path::PathBuf;

use crate::error::GitError;
use crate::oid::ObjectId;
use crate::path::path_from_git;
use crate::repo::Repo;

/// One worktree registered with the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// The worktree's directory, as git prints it.
    pub path: PathBuf,
    /// The commit its HEAD points at, `None` when unborn.
    pub head: Option<ObjectId>,
    /// Whether this entry is the bare repository itself.
    pub bare: bool,
    /// Whether git reports the worktree prunable (its directory is missing).
    pub prunable: bool,
}

/// Lists every worktree of the repository, the main one first, from
/// `worktree list --porcelain -z`.
///
/// # Errors
///
/// Returns the errors of [`crate::Git::output`], or [`GitError::Parse`] for
/// output this function does not understand.
pub fn list_worktrees(repo: &Repo) -> Result<Vec<Worktree>, GitError> {
    let output = repo
        .git()
        .output(["worktree", "list", "--porcelain", "-z"])?;
    parse_porcelain(&output, repo)
}

/// Parses NUL-terminated porcelain attributes; an empty attribute ends a
/// worktree.
fn parse_porcelain(output: &[u8], repo: &Repo) -> Result<Vec<Worktree>, GitError> {
    let mut worktrees = Vec::new();
    let mut current: Option<Worktree> = None;
    for attribute in output.split(|&b| b == 0) {
        if attribute.is_empty() {
            worktrees.extend(current.take());
            continue;
        }
        let (key, value) = match attribute.iter().position(|&b| b == b' ') {
            Some(space) => (&attribute[..space], &attribute[space + 1..]),
            None => (attribute, &b""[..]),
        };
        if key == b"worktree" {
            worktrees.extend(current.take());
            let path = path_from_git(value).ok_or_else(|| {
                GitError::Parse("worktree path not representable on this platform".to_owned())
            })?;
            current = Some(Worktree {
                path,
                head: None,
                bare: false,
                prunable: false,
            });
            continue;
        }
        let Some(worktree) = current.as_mut() else {
            return Err(GitError::Parse(format!(
                "worktree attribute before a worktree: {}",
                String::from_utf8_lossy(attribute)
            )));
        };
        match key {
            b"HEAD" => {
                let id = ObjectId::parse(&String::from_utf8_lossy(value), repo.format())?;
                worktree.head = (!id.is_null()).then_some(id);
            }
            b"bare" => worktree.bare = true,
            b"prunable" => worktree.prunable = true,
            _ => {}
        }
    }
    worktrees.extend(current);
    Ok(worktrees)
}
```

- [ ] **Step 10: Replace the crate root**

`crates/supersigil-git/src/lib.rs`:

```rust
//! Git access for supersigil review, through the git CLI.
//!
//! [`Repo`] opens a worktree and [`resolve_range`] resolves the base and
//! target of a review; [`worktree`] lists worktrees. Nothing here ever
//! writes the real index, the worktree, or a ref.
//!
//! Git runs only through [`Git`], which scrubs the variables that could point
//! it at another repository or index and honors the user's configuration.

pub mod error;
pub mod oid;
pub mod path;
pub mod repo;
pub mod run;
pub mod worktree;

pub use error::GitError;
pub use oid::{ObjectFormat, ObjectId};
pub use path::RepoPath;
pub use repo::{Ancestry, Base, Repo, ResolvedRange, ResolvedTarget, TargetSpec, resolve_range};
pub use run::{Git, GitVersion, MIN_VERSION};
```

- [ ] **Step 11: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-git`
Expected: PASS, 27 tests (14 in `tests/repo.rs`, 4 in `tests/worktree.rs`, 9 unit tests in `run`, `oid`, and `path`).

- [ ] **Step 12: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; all tests pass.

- [ ] **Step 13: Commit**

```bash
git add Cargo.lock crates/supersigil-git/Cargo.toml crates/supersigil-git/src/lib.rs crates/supersigil-git/src/error.rs crates/supersigil-git/src/run.rs crates/supersigil-git/src/oid.rs crates/supersigil-git/src/path.rs crates/supersigil-git/src/repo.rs crates/supersigil-git/src/worktree.rs crates/supersigil-git/tests/common/mod.rs crates/supersigil-git/tests/repo.rs crates/supersigil-git/tests/worktree.rs
git commit -m "feat: run git and resolve review ranges"
```

- [ ] **Step 14: Add the minimum-git CI job**

`worktree list -z` arrived in git 2.36, which is the stated minimum; this job exercises it rather than assuming it. Append to the `jobs:` map at the end of `.github/workflows/ci.yml` (the `actions/cache` pin is the commit of its `v6.1.0` tag, a lightweight tag):

```yaml
  test-git-minimum:
    name: Test (git 2.36.0)
    runs-on: ubuntu-latest
    env:
      GIT_MINIMUM: 2.36.0
    steps:
      - name: Checkout
        uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2

      - name: Install Rust
        uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # master, "Add 1.94.1 patch release"
        with:
          toolchain: stable

      - name: Cache Rust
        uses: Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4 # v2.9.1

      - name: Cache git ${{ env.GIT_MINIMUM }}
        id: git-cache
        uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
        with:
          path: ~/git-${{ env.GIT_MINIMUM }}
          key: git-${{ env.GIT_MINIMUM }}-${{ runner.os }}-${{ runner.arch }}

      - name: Build git ${{ env.GIT_MINIMUM }}
        if: steps.git-cache.outputs.cache-hit != 'true'
        run: |
          sudo apt-get update && sudo apt-get install -y zlib1g-dev
          curl -fsSL "https://mirrors.edge.kernel.org/pub/software/scm/git/git-${GIT_MINIMUM}.tar.xz" | tar -xJ
          make -C "git-${GIT_MINIMUM}" -j"$(nproc)" prefix="$HOME/git-${GIT_MINIMUM}" \
            NO_CURL=1 NO_EXPAT=1 NO_GETTEXT=1 NO_TCLTK=1 NO_PERL=1 NO_PYTHON=1 install

      - name: Install nextest
        uses: taiki-e/install-action@1ed3272338f573e042a2e6bca3893aa19f43b47a # v2.71.3
        with:
          tool: cargo-nextest@0.9.132

      - name: Test the git crate with git ${{ env.GIT_MINIMUM }}
        run: |
          export PATH="$HOME/git-${GIT_MINIMUM}/bin:$PATH"
          git --version
          test "$(git version)" = "git version ${GIT_MINIMUM}"
          cargo nextest run -p supersigil-git
```

The build installs only zlib headers and disables curl, expat, gettext, Tcl/Tk, Perl, and Python: the tests need local operations, `submodule`, `am`, and `pull` from a local path, nothing else. The cache key includes the version, so bumping `GIT_MINIMUM` rebuilds.

- [ ] **Step 15: Commit the CI job**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: test the git crate with git 2.36.0"
```

---

### Task 5: Working-tree snapshot

**Files:**
- Modify: `crates/supersigil-git/Cargo.toml` (`tempfile` becomes a regular dependency)
- Modify: `crates/supersigil-git/src/lib.rs` (`pub mod snapshot;`)
- Modify: `crates/supersigil-git/src/path.rs` (pathspec helpers)
- Create: `crates/supersigil-git/src/snapshot.rs`
- Create: `crates/supersigil-git/tests/snapshot.rs`

**Interfaces:**
- Consumes: Task 4's `Repo`, `Git`, `ObjectId`, `RepoPath`, `GitError`, `oid::parse_output`, `path::path_from_git`, `run::io_error`, and the test support.
- Produces: `snapshot::{SnapshotOptions { include_untracked, pathspecs }, OnDisk { Present, Missing }, NotCapturedCause { AssumeUnchanged, SkipWorktree, NotStaged }, NotCaptured { path, on_disk, cause }, WorkingTreeSnapshot { tree, not_captured, skip_worktree_absent, unmerged, untracked }, snapshot_working_tree(repo, options)}`. `pathspecs` limit the listings only; the tree always holds the whole worktree, so its identity does not depend on the scope options. Crate-private for Tasks 6 and 7: `snapshot::nul_fields(output)` (the non-empty NUL-terminated fields), and `RepoPath::to_os_string()` and `RepoPath::literal_pathspec()` (`:(literal)<path>`).

The procedure is design section 2's, step for step: the unmerged listing and the real index path are read first without the override; the real index is copied with its modification time taken from the same open handle (so racily clean entries stay racy); every command on the copy runs with `GIT_INDEX_FILE` pointing at it and `-c core.splitIndex=false -c core.fsmonitor=false -c core.hooksPath=<empty temporary directory>`; `add -u`, then each listed untracked path, then `diff-files -z --name-status --ignore-submodules=dirty` as the postcondition, then `ls-files -v -z` for the flags, then `ls-files --others --exclude-standard -z`, then `write-tree`. A missing real index leaves the temporary path nonexistent, which git reads as empty.

- [ ] **Step 1: Make `tempfile` a regular dependency**

The snapshot needs a temporary directory at run time. Make the dependency sections of `crates/supersigil-git/Cargo.toml` read (a regular dependency is also available to tests, so the `[dev-dependencies]` section goes away):

```toml
[dependencies]
serde.workspace = true
tempfile.workspace = true
thiserror.workspace = true
```

- [ ] **Step 2: Write the failing tests**

`crates/supersigil-git/tests/snapshot.rs`. `a_sparse_file_recreated_outside_the_patterns_is_listed_not_captured` accepts either cause: git 2.55 clears the skip-worktree bit of a file that reappears and `diff-files` reports it (`NotStaged`), while older git keeps the bit and the flag listing reports it (`SkipWorktree`); either way the path is listed as present and not captured. `snapshot_never_runs_hooks` finishes by running a normal `git add` to prove the hook itself works:

```rust
//! The working-tree snapshot against real repositories.

mod common;

use std::collections::BTreeMap;

use common::TestRepo;
use supersigil_git::snapshot::{
    NotCaptured, NotCapturedCause, OnDisk, SnapshotOptions, snapshot_working_tree,
};
use supersigil_git::{GitError, RepoPath};

fn files(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn paths(list: &[RepoPath]) -> Vec<String> {
    list.iter().map(RepoPath::display).collect()
}

fn listed(entries: &[NotCaptured]) -> Vec<(String, OnDisk, NotCapturedCause)> {
    entries
        .iter()
        .map(|e| (e.path.display(), e.on_disk, e.cause))
        .collect()
}

/// A repository with four committed files.
fn committed() -> TestRepo {
    let repo = TestRepo::new();
    for name in ["a", "b", "c", "d"] {
        repo.write(&format!("{name}.txt"), format!("{name}\n").as_bytes());
    }
    repo.commit_all("four files");
    repo
}

#[test]
fn snapshot_takes_the_on_disk_state_of_tracked_files() {
    let repo = committed();
    repo.write("a.txt", b"a changed\n");
    std::fs::remove_file(repo.root.join("b.txt")).unwrap();
    repo.write("c.txt", b"c staged\n");
    repo.run(&["add", "c.txt"]);
    repo.write("c.txt", b"c staged then changed\n");
    repo.write("new.txt", b"untracked\n");
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str()),
        files(&[
            ("a.txt", "a changed\n"),
            ("c.txt", "c staged then changed\n"),
            ("d.txt", "d\n"),
        ])
    );
    assert_eq!(paths(&snapshot.untracked), ["new.txt"]);
    assert!(snapshot.not_captured.is_empty());
    assert!(snapshot.unmerged.is_empty());
    assert_eq!(snapshot.skip_worktree_absent, 0);
}

#[test]
fn the_real_index_is_byte_identical_and_keeps_its_mtime() {
    let repo = committed();
    repo.write("a.txt", b"a changed\n");
    repo.write("c.txt", b"c staged\n");
    repo.run(&["add", "c.txt"]);
    let index = repo.root.join(".git/index");
    let bytes = std::fs::read(&index).unwrap();
    let modified = std::fs::metadata(&index).unwrap().modified().unwrap();
    snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(std::fs::read(&index).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&index).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(repo.run(&["diff", "--cached", "--name-only"]), "c.txt\n");
}

#[cfg(unix)]
#[test]
fn snapshot_never_runs_hooks() {
    use std::os::unix::fs::PermissionsExt as _;
    let repo = committed();
    let marker = repo.dir.path().join("hook-ran");
    let hook = repo.root.join(".git/hooks/post-index-change");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    repo.write("a.txt", b"a changed\n");
    snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert!(!marker.exists(), "a hook ran during the snapshot");
    // The hook itself works: a normal `git add` runs it.
    repo.run(&["add", "a.txt"]);
    assert!(marker.exists());
}

#[test]
fn a_fresh_repository_without_an_index_reads_as_empty() {
    let repo = TestRepo::new();
    repo.write("first.txt", b"hello\n");
    assert!(!repo.root.join(".git/index").exists());
    let opened = repo.repo();
    let snapshot = snapshot_working_tree(&opened, &SnapshotOptions::default()).unwrap();
    assert_eq!(snapshot.tree, opened.empty_tree().unwrap());
    assert_eq!(paths(&snapshot.untracked), ["first.txt"]);
    let options = SnapshotOptions {
        include_untracked: vec![RepoPath::from_utf8("first.txt")],
        ..SnapshotOptions::default()
    };
    let included = snapshot_working_tree(&opened, &options).unwrap();
    assert_eq!(
        repo.tree_files(included.tree.as_str()),
        files(&[("first.txt", "hello\n")])
    );
    assert!(included.untracked.is_empty());
    assert!(!repo.root.join(".git/index").exists());
}

#[test]
fn listed_untracked_paths_are_included_and_bad_ones_refused() {
    let repo = committed();
    repo.write("new.txt", b"new\n");
    repo.write("other.txt", b"other\n");
    repo.write(".gitignore", b"ignored.txt\n");
    repo.write("ignored.txt", b"secret\n");
    let opened = repo.repo();
    let options = SnapshotOptions {
        include_untracked: vec![RepoPath::from_utf8("new.txt")],
        ..SnapshotOptions::default()
    };
    let snapshot = snapshot_working_tree(&opened, &options).unwrap();
    let tree = repo.tree_files(snapshot.tree.as_str());
    assert_eq!(tree.get("new.txt").map(String::as_str), Some("new\n"));
    assert!(!tree.contains_key("other.txt"));
    assert_eq!(paths(&snapshot.untracked), [".gitignore", "other.txt"]);

    for (path, reason) in [("missing.txt", "no such file"), ("ignored.txt", "ignored")] {
        let options = SnapshotOptions {
            include_untracked: vec![RepoPath::from_utf8(path)],
            ..SnapshotOptions::default()
        };
        let result = snapshot_working_tree(&opened, &options);
        assert!(
            matches!(&result, Err(GitError::Untracked { path: p, reason: r }) if p == path && r == reason),
            "{path}: {result:?}"
        );
    }
}

#[test]
fn a_split_index_is_read_without_writing_shared_indexes() {
    let repo = committed();
    repo.run(&["config", "core.splitIndex", "true"]);
    repo.run(&["update-index", "--split-index"]);
    repo.write("a.txt", b"a changed\n");
    let shared = |repo: &TestRepo| {
        std::fs::read_dir(repo.root.join(".git"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("sharedindex.")
            })
            .count()
    };
    let before = shared(&repo);
    assert!(before >= 1);
    let index = std::fs::read(repo.root.join(".git/index")).unwrap();
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(shared(&repo), before);
    assert_eq!(std::fs::read(repo.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("a.txt")
            .unwrap(),
        "a changed\n"
    );
}

#[test]
fn assume_unchanged_entries_are_listed_present_or_missing() {
    let repo = committed();
    repo.run(&["update-index", "--assume-unchanged", "a.txt", "b.txt"]);
    repo.write("a.txt", b"a hidden change\n");
    std::fs::remove_file(repo.root.join("b.txt")).unwrap();
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    let tree = repo.tree_files(snapshot.tree.as_str());
    assert_eq!(tree.get("a.txt").unwrap(), "a\n");
    assert_eq!(tree.get("b.txt").unwrap(), "b\n");
    assert_eq!(
        listed(&snapshot.not_captured),
        [
            (
                "a.txt".to_owned(),
                OnDisk::Present,
                NotCapturedCause::AssumeUnchanged
            ),
            (
                "b.txt".to_owned(),
                OnDisk::Missing,
                NotCapturedCause::AssumeUnchanged
            ),
        ]
    );
}

#[test]
fn skip_worktree_entries_are_listed_when_present_and_counted_when_absent() {
    let repo = committed();
    repo.run(&["update-index", "--skip-worktree", "a.txt", "b.txt"]);
    repo.write("a.txt", b"a local override\n");
    std::fs::remove_file(repo.root.join("b.txt")).unwrap();
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(
        listed(&snapshot.not_captured),
        [(
            "a.txt".to_owned(),
            OnDisk::Present,
            NotCapturedCause::SkipWorktree
        )]
    );
    assert_eq!(snapshot.skip_worktree_absent, 1);
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("a.txt")
            .unwrap(),
        "a\n"
    );
}

#[test]
fn a_sparse_file_recreated_outside_the_patterns_is_listed_not_captured() {
    let repo = TestRepo::new();
    repo.write("in/f.txt", b"inside\n");
    repo.write("out/g.txt", b"outside\n");
    repo.commit_all("two directories");
    repo.run(&["sparse-checkout", "set", "--no-cone", "/in/"]);
    assert!(!repo.root.join("out/g.txt").exists());
    repo.write("out/g.txt", b"recreated with new contents\n");
    repo.write("in/f.txt", b"inside changed\n");
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    let tree = repo.tree_files(snapshot.tree.as_str());
    assert_eq!(tree.get("in/f.txt").unwrap(), "inside changed\n");
    assert_eq!(tree.get("out/g.txt").unwrap(), "outside\n");
    // Newer git clears the skip-worktree bit of a file that reappears, and
    // `diff-files` reports it; older git keeps the bit, and the flag listing
    // reports it. Either way it is listed as present and not captured.
    let entry = snapshot
        .not_captured
        .iter()
        .find(|e| e.path.display() == "out/g.txt")
        .expect("out/g.txt is listed");
    assert_eq!(entry.on_disk, OnDisk::Present);
    assert!(matches!(
        entry.cause,
        NotCapturedCause::NotStaged | NotCapturedCause::SkipWorktree
    ));
}

#[test]
fn a_submodule_configured_ignore_all_is_listed_not_captured() {
    let repo = TestRepo::new();
    let sub = TestRepo::new();
    sub.write("s.txt", b"one\n");
    sub.commit_all("sub one");
    repo.write("a.txt", b"a\n");
    repo.run(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        sub.root.to_str().unwrap(),
        "sm",
    ]);
    repo.run(&["config", "-f", ".gitmodules", "submodule.sm.ignore", "all"]);
    repo.commit_all("with submodule");
    let old = repo.run(&["rev-parse", "HEAD:sm"]);
    let sm = repo.root.join("sm");
    repo.write("sm/s.txt", b"two\n");
    repo.run_in(&sm, &["commit", "-q", "-am", "sub two"]);
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    let spec = format!("{}:sm", snapshot.tree);
    assert_eq!(repo.run(&["rev-parse", &spec]), old);
    assert_eq!(
        listed(&snapshot.not_captured),
        [(
            "sm".to_owned(),
            OnDisk::Present,
            NotCapturedCause::NotStaged
        )]
    );
}

#[test]
fn an_unmerged_path_is_taken_on_disk_and_listed() {
    let repo = TestRepo::new();
    repo.write("f.txt", b"base\n");
    repo.commit_all("base");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("f.txt", b"side\n");
    repo.commit_all("side");
    repo.run(&["switch", "-q", "main"]);
    repo.write("f.txt", b"main\n");
    repo.commit_all("main");
    let merge = repo.git().raw(["merge", "-q", "side"]).unwrap();
    assert!(!merge.status.success(), "the merge conflicts");
    let on_disk = std::fs::read_to_string(repo.root.join("f.txt")).unwrap();
    assert!(on_disk.contains("<<<<<<<"));
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(paths(&snapshot.unmerged), ["f.txt"]);
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("f.txt")
            .unwrap(),
        &on_disk
    );
    assert!(
        repo.run(&["ls-files", "-u"]).contains("f.txt"),
        "the real index stays unmerged"
    );
}

#[test]
fn an_intent_to_add_entry_is_staged() {
    let repo = committed();
    repo.write("planned.txt", b"planned\n");
    repo.run(&["add", "-N", "planned.txt"]);
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("planned.txt")
            .unwrap(),
        "planned\n"
    );
}

#[test]
fn a_changed_modification_time_alone_is_not_listed() {
    let repo = committed();
    let path = repo.root.join("a.txt");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let snapshot = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert!(
        snapshot.not_captured.is_empty(),
        "{:?}",
        snapshot.not_captured
    );
}

#[test]
fn pathspecs_limit_the_listings_but_not_the_tree() {
    let repo = committed();
    repo.write("src/new.rs", b"fn main() {}\n");
    repo.write("docs/new.md", b"# doc\n");
    repo.write("a.txt", b"a changed\n");
    let options = SnapshotOptions {
        pathspecs: vec![RepoPath::from_utf8("src")],
        ..SnapshotOptions::default()
    };
    let snapshot = snapshot_working_tree(&repo.repo(), &options).unwrap();
    assert_eq!(paths(&snapshot.untracked), ["src/new.rs"]);
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("a.txt")
            .unwrap(),
        "a changed\n"
    );
}

#[test]
fn a_linked_worktree_is_snapshotted_from_its_own_index() {
    let repo = committed();
    let linked = repo.dir.path().join("linked");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        linked.to_str().unwrap(),
    ]);
    std::fs::write(linked.join("a.txt"), b"a in linked\n").unwrap();
    let opened = supersigil_git::Repo::open(repo.git().in_dir(&linked)).unwrap();
    let snapshot = snapshot_working_tree(&opened, &SnapshotOptions::default()).unwrap();
    assert_eq!(
        repo.tree_files(snapshot.tree.as_str())
            .get("a.txt")
            .unwrap(),
        "a in linked\n"
    );
    let main = snapshot_working_tree(&repo.repo(), &SnapshotOptions::default()).unwrap();
    assert_eq!(
        repo.tree_files(main.tree.as_str()).get("a.txt").unwrap(),
        "a\n"
    );
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-git`
Expected: FAIL to compile with `could not find snapshot in supersigil_git`.

- [ ] **Step 4: Add the pathspec helpers**

Replace `crates/supersigil-git/src/path.rs` with:

```rust
//! Repository-relative paths as git stores them: raw bytes.

use std::ffi::OsString;
use std::path::PathBuf;

/// A path relative to the repository root, `/`-separated, as the raw bytes
/// git prints in `-z` output. Git allows any bytes except NUL, so a path is
/// not necessarily UTF-8; the bytes stay its identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepoPath(Vec<u8>);

impl RepoPath {
    /// Wraps raw path bytes.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Wraps a UTF-8 path such as `src/lib.rs`.
    #[must_use]
    pub fn from_utf8(path: &str) -> Self {
        Self(path.as_bytes().to_vec())
    }

    /// Returns the raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the path as text, or `None` when it is not UTF-8.
    #[must_use]
    pub fn to_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// Returns the path for display, replacing invalid UTF-8 with U+FFFD.
    /// Two different paths can display the same; use [`RepoPath::escaped`]
    /// where identity matters.
    #[must_use]
    pub fn display(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }

    /// Returns a reversible ASCII form: `\` becomes `\\`, and every byte
    /// outside printable ASCII (`0x20..=0x7e`) becomes `\xNN` with two
    /// lowercase hex digits.
    #[must_use]
    pub fn escaped(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(self.0.len());
        for &byte in &self.0 {
            match byte {
                b'\\' => out.push_str("\\\\"),
                0x20..=0x7e => out.push(char::from(byte)),
                _ => {
                    out.push_str("\\x");
                    out.push(char::from(HEX[usize::from(byte >> 4)]));
                    out.push(char::from(HEX[usize::from(byte & 0x0f)]));
                }
            }
        }
        out
    }

    /// Returns the path as a relative [`PathBuf`], or `None` when this
    /// platform cannot represent its bytes (non-UTF-8 on Windows).
    #[must_use]
    pub fn to_path(&self) -> Option<PathBuf> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;
            Some(PathBuf::from(OsString::from_vec(self.0.clone())))
        }
        #[cfg(not(unix))]
        {
            self.to_str().map(PathBuf::from)
        }
    }

    /// Returns the path as an argument for git, or `None` when this platform
    /// cannot represent its bytes.
    pub(crate) fn to_os_string(&self) -> Option<OsString> {
        self.to_path().map(PathBuf::into_os_string)
    }

    /// Returns `:(literal)<path>`, a pathspec that matches this path and
    /// everything below it without interpreting wildcards.
    pub(crate) fn literal_pathspec(&self) -> Option<OsString> {
        let mut spec = OsString::from(":(literal)");
        spec.push(self.to_os_string()?);
        Some(spec)
    }
}

/// Converts path bytes printed by git into a [`PathBuf`], or `None` when the
/// platform cannot represent them.
pub(crate) fn path_from_git(bytes: &[u8]) -> Option<PathBuf> {
    RepoPath::new(bytes.to_vec()).to_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_is_reversible_and_ascii() {
        let path = RepoPath::new(b"dir\\a b/\xff\x01\xc3\xa9.txt".to_vec());
        assert_eq!(path.escaped(), r"dir\\a b/\xff\x01\xc3\xa9.txt");
        assert!(path.escaped().is_ascii());
        assert_eq!(RepoPath::from_utf8("src/lib.rs").escaped(), "src/lib.rs");
    }

    #[test]
    fn non_utf8_paths_display_lossily_but_keep_their_bytes() {
        let a = RepoPath::new(b"x\xff".to_vec());
        let b = RepoPath::new(b"x\xfe".to_vec());
        assert_eq!(a.display(), b.display());
        assert_ne!(a, b);
        assert_ne!(a.escaped(), b.escaped());
        assert_eq!(a.to_str(), None);
        assert_eq!(RepoPath::from_utf8("é").to_str(), Some("é"));
    }

    #[test]
    fn literal_pathspecs_keep_wildcards_literal() {
        let spec = RepoPath::from_utf8("src/*.rs").literal_pathspec().unwrap();
        assert_eq!(spec, OsString::from(":(literal)src/*.rs"));
    }
}
```

- [ ] **Step 5: Write the snapshot**

`crates/supersigil-git/src/snapshot.rs`:

```rust
//! Captures the working tree as a git tree through a temporary index.
//!
//! The real index is copied (keeping its modification time), `git add -u`
//! and the listed untracked paths are applied to the copy, and `write-tree`
//! writes the result. The real index, the worktree, and every ref stay
//! untouched; the only writes are new objects, a refreshed modification time
//! on a split index's shared file (git does that on every read), and
//! whatever configured clean filters do on their own, exactly as `git add`
//! would. Paths whose on-disk state this procedure does not capture are
//! listed, never silently dropped.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::error::GitError;
use crate::oid::{ObjectId, parse_output};
use crate::path::{RepoPath, path_from_git};
use crate::repo::Repo;
use crate::run::{Git, io_error};

/// What to include beyond tracked changes, and which paths the listings cover.
#[derive(Debug, Clone, Default)]
pub struct SnapshotOptions {
    /// Untracked paths to include, as given with `--include-untracked`.
    pub include_untracked: Vec<RepoPath>,
    /// Literal path prefixes that limit the listings; empty for everything.
    /// The tree itself always covers the whole worktree.
    pub pathspecs: Vec<RepoPath>,
}

/// Whether a path whose contents were not captured exists on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnDisk {
    /// The file exists; its contents were not captured.
    Present,
    /// The file is missing; its deletion was not captured.
    Missing,
}

/// Why a tracked path's on-disk state is not in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotCapturedCause {
    /// The index entry is flagged assume-unchanged, so git does not look at
    /// the file.
    AssumeUnchanged,
    /// The index entry is flagged skip-worktree and the file exists.
    SkipWorktree,
    /// `git add -u` left it out for another reason, reported by `diff-files`
    /// (for example a file outside the sparse-checkout patterns whose
    /// skip-worktree bit git cleared, or a submodule configured
    /// `ignore = all`).
    NotStaged,
}

/// A tracked path whose on-disk state the tree does not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCaptured {
    /// The path.
    pub path: RepoPath,
    /// Whether the file exists.
    pub on_disk: OnDisk,
    /// Why it was not captured.
    pub cause: NotCapturedCause,
}

/// The working tree written as a tree, with everything the capture left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingTreeSnapshot {
    /// The tree holding the captured state.
    pub tree: ObjectId,
    /// Tracked paths whose on-disk state is not in the tree.
    pub not_captured: Vec<NotCaptured>,
    /// Skip-worktree entries whose file is absent; they keep their index
    /// state, which is the normal state in a sparse checkout.
    pub skip_worktree_absent: usize,
    /// Paths unmerged in the real index; the tree holds their on-disk form,
    /// conflict markers included.
    pub unmerged: Vec<RepoPath>,
    /// Untracked, non-ignored paths that were not included.
    pub untracked: Vec<RepoPath>,
}

/// Writes the working tree of `repo` as a tree, as `git add -u` plus
/// `options.include_untracked` would stage it, without touching the real
/// index.
///
/// # Errors
///
/// Returns [`GitError::Untracked`] if an included path is missing or
/// ignored, [`GitError::Io`] if the temporary index cannot be prepared, or
/// the errors of the git commands it runs.
pub fn snapshot_working_tree(
    repo: &Repo,
    options: &SnapshotOptions,
) -> Result<WorkingTreeSnapshot, GitError> {
    let pathspecs: Vec<OsString> = options
        .pathspecs
        .iter()
        .filter_map(RepoPath::literal_pathspec)
        .collect();
    // Read from the real index before anything else, without the override.
    let unmerged = unmerged_paths(repo, &pathspecs)?;
    let index = real_index_path(repo)?;

    let temp = tempfile::Builder::new()
        .prefix("supersigil-snapshot-")
        .tempdir()
        .map_err(|e| io_error("cannot create a temporary directory", e))?;
    let temp_index = temp.path().join("index");
    let hooks = temp.path().join("hooks");
    std::fs::create_dir(&hooks)
        .map_err(|e| io_error("cannot create an empty hooks directory", e))?;
    if index.exists() {
        copy_index(&index, &temp_index)?;
    }
    // A missing temporary index reads as an empty one: a fresh repository.
    let staging = Staging {
        git: repo.git().clone().with_env("GIT_INDEX_FILE", &temp_index),
        config: isolation_config(&hooks),
    };

    staging.run(&["add".into(), "-u".into()])?;
    for path in &options.include_untracked {
        include(repo, &staging, path)?;
    }
    let mut not_captured = not_staged(repo, &staging, &pathspecs)?;
    let (flagged, skip_worktree_absent) = flagged_entries(repo, &staging, &pathspecs)?;
    not_captured.extend(flagged);
    let untracked = untracked_paths(&staging, &pathspecs)?;
    let tree = parse_output(&staging.run(&["write-tree".into()])?, repo.format())?;
    Ok(WorkingTreeSnapshot {
        tree,
        not_captured,
        skip_worktree_absent,
        unmerged,
        untracked,
    })
}

/// Git pointed at the temporary index, with the configuration that keeps it
/// from writing anything but the temporary index and objects.
struct Staging {
    git: Git,
    config: Vec<OsString>,
}

impl Staging {
    /// Runs git with the isolation configuration before `args`.
    fn run(&self, args: &[OsString]) -> Result<Vec<u8>, GitError> {
        self.git.output(self.config.iter().chain(args))
    }

    /// Like [`Staging::run`], but returns the raw output whatever the status.
    fn raw(&self, args: &[OsString]) -> Result<std::process::Output, GitError> {
        self.git.raw(self.config.iter().chain(args))
    }
}

/// `-c` options for every command on the temporary index: no split index (so
/// no shared-index files are written into the git directory), no fsmonitor
/// daemon state, and an empty hooks directory, because writing any index,
/// including an alternate one, runs the `post-index-change` hook.
fn isolation_config(hooks: &Path) -> Vec<OsString> {
    let mut hooks_path = OsString::from("core.hooksPath=");
    hooks_path.push(hooks);
    vec![
        "-c".into(),
        "core.splitIndex=false".into(),
        "-c".into(),
        "core.fsmonitor=false".into(),
        "-c".into(),
        hooks_path,
    ]
}

/// Resolves the real index path. `rev-parse --git-path` honors
/// `GIT_INDEX_FILE`, which [`Git::command`] removes, so this is the real one.
fn real_index_path(repo: &Repo) -> Result<PathBuf, GitError> {
    let output = repo.git().output(["rev-parse", "--git-path", "index"])?;
    let path = path_from_git(output.trim_ascii_end())
        .ok_or_else(|| GitError::Parse("index path not representable".to_owned()))?;
    Ok(if path.is_absolute() {
        path
    } else {
        repo.root().join(path)
    })
}

/// Copies the index and gives the copy the source's modification time, read
/// from the same open file. Git decides whether a cached stat entry is racily
/// clean by comparing it with the index file's own mtime; a newer copy would
/// let a file changed in the same instant as the last index write pass as
/// clean.
pub(crate) fn copy_index(source: &Path, target: &Path) -> Result<(), GitError> {
    let mut from = File::open(source).map_err(|e| io_error("cannot open the index", e))?;
    let modified = from
        .metadata()
        .and_then(|m| m.modified())
        .map_err(|e| io_error("cannot read the index's modification time", e))?;
    let mut to =
        File::create(target).map_err(|e| io_error("cannot create the temporary index", e))?;
    std::io::copy(&mut from, &mut to).map_err(|e| io_error("cannot copy the index", e))?;
    to.set_modified(modified)
        .map_err(|e| io_error("cannot set the temporary index's modification time", e))
}

/// Adds one listed untracked path to the temporary index.
fn include(repo: &Repo, staging: &Staging, path: &RepoPath) -> Result<(), GitError> {
    let refuse = |reason: &str| GitError::Untracked {
        path: path.display(),
        reason: reason.to_owned(),
    };
    let (Some(relative), Some(spec)) = (path.to_path(), path.literal_pathspec()) else {
        return Err(refuse("not representable on this platform"));
    };
    if repo.root().join(&relative).symlink_metadata().is_err() {
        return Err(refuse("no such file"));
    }
    let ignored = staging.raw(&[
        "check-ignore".into(),
        "-q".into(),
        "--".into(),
        relative.into(),
    ])?;
    if ignored.status.success() {
        return Err(refuse("ignored"));
    }
    staging.run(&["add".into(), "--".into(), spec])?;
    Ok(())
}

/// Paths `diff-files` still reports after staging: tracked content whose
/// on-disk state `add -u` did not capture, whatever the reason. The flag
/// matters for submodules configured `ignore = all`, whose changed gitlink
/// plain `diff-files` would hide.
fn not_staged(
    repo: &Repo,
    staging: &Staging,
    pathspecs: &[OsString],
) -> Result<Vec<NotCaptured>, GitError> {
    let mut args: Vec<OsString> = vec![
        "diff-files".into(),
        "-z".into(),
        "--name-status".into(),
        "--ignore-submodules=dirty".into(),
        "--".into(),
    ];
    args.extend(pathspecs.iter().cloned());
    let output = staging.run(&args)?;
    let fields: Vec<&[u8]> = nul_fields(&output).collect();
    let mut listed = Vec::new();
    for pair in fields.chunks(2) {
        let [status, path] = pair else {
            return Err(GitError::Parse(
                "diff-files output ends mid-record".to_owned(),
            ));
        };
        let path = RepoPath::new(path.to_vec());
        let on_disk = if status.first() == Some(&b'D') {
            OnDisk::Missing
        } else {
            on_disk(repo, &path)
        };
        listed.push(NotCaptured {
            path,
            on_disk,
            cause: NotCapturedCause::NotStaged,
        });
    }
    Ok(listed)
}

/// Entries flagged assume-unchanged or skip-worktree, from `ls-files -v`:
/// `diff-files` trusts both flags just as `add -u` does, so they are listed
/// from the flags. Returns the listed entries and the number of
/// skip-worktree entries whose file is absent.
fn flagged_entries(
    repo: &Repo,
    staging: &Staging,
    pathspecs: &[OsString],
) -> Result<(Vec<NotCaptured>, usize), GitError> {
    let mut args: Vec<OsString> = vec!["ls-files".into(), "-v".into(), "-z".into(), "--".into()];
    args.extend(pathspecs.iter().cloned());
    let output = staging.run(&args)?;
    let mut listed = Vec::new();
    let mut absent = 0;
    for entry in nul_fields(&output) {
        let [tag, b' ', path @ ..] = entry else {
            return Err(GitError::Parse(format!(
                "ls-files -v entry {:?}",
                String::from_utf8_lossy(entry)
            )));
        };
        let path = RepoPath::new(path.to_vec());
        if tag.is_ascii_lowercase() {
            listed.push(NotCaptured {
                on_disk: on_disk(repo, &path),
                path,
                cause: NotCapturedCause::AssumeUnchanged,
            });
        } else if *tag == b'S' {
            match on_disk(repo, &path) {
                OnDisk::Present => listed.push(NotCaptured {
                    path,
                    on_disk: OnDisk::Present,
                    cause: NotCapturedCause::SkipWorktree,
                }),
                OnDisk::Missing => absent += 1,
            }
        }
    }
    Ok((listed, absent))
}

/// Paths with an unmerged entry in the real index.
fn unmerged_paths(repo: &Repo, pathspecs: &[OsString]) -> Result<Vec<RepoPath>, GitError> {
    let mut args: Vec<OsString> = vec!["ls-files".into(), "-u".into(), "-z".into(), "--".into()];
    args.extend(pathspecs.iter().cloned());
    let output = repo.git().output(&args)?;
    let mut paths = BTreeSet::new();
    for entry in nul_fields(&output) {
        let tab = entry
            .iter()
            .position(|&b| b == b'\t')
            .ok_or_else(|| GitError::Parse("ls-files -u entry without a tab".to_owned()))?;
        paths.insert(RepoPath::new(entry[tab + 1..].to_vec()));
    }
    Ok(paths.into_iter().collect())
}

/// Untracked, non-ignored paths still untracked after staging.
fn untracked_paths(staging: &Staging, pathspecs: &[OsString]) -> Result<Vec<RepoPath>, GitError> {
    let mut args: Vec<OsString> = vec![
        "ls-files".into(),
        "--others".into(),
        "--exclude-standard".into(),
        "-z".into(),
        "--".into(),
    ];
    args.extend(pathspecs.iter().cloned());
    let output = staging.run(&args)?;
    Ok(nul_fields(&output)
        .map(|path| RepoPath::new(path.to_vec()))
        .collect())
}

/// Whether `path` exists under the worktree root (a dangling symlink exists).
fn on_disk(repo: &Repo, path: &RepoPath) -> OnDisk {
    let exists = path
        .to_path()
        .is_some_and(|relative| repo.root().join(relative).symlink_metadata().is_ok());
    if exists {
        OnDisk::Present
    } else {
        OnDisk::Missing
    }
}

/// Splits NUL-terminated output into its non-empty fields.
pub(crate) fn nul_fields(output: &[u8]) -> impl Iterator<Item = &[u8]> {
    output.split(|&b| b == 0).filter(|field| !field.is_empty())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn copied_index_keeps_the_source_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("index");
        std::fs::write(&source, b"DIRC").unwrap();
        let past = SystemTime::now() - Duration::from_secs(3600);
        File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_modified(past)
            .unwrap();
        let target = dir.path().join("copy");
        copy_index(&source, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"DIRC");
        let modified = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        assert_eq!(modified(&target), modified(&source));
    }

    #[test]
    fn nul_fields_skip_the_terminator() {
        let fields: Vec<&[u8]> = nul_fields(b"a\0b c\0").collect();
        assert_eq!(fields, [&b"a"[..], &b"b c"[..]]);
        assert_eq!(nul_fields(b"").count(), 0);
    }
}
```

- [ ] **Step 6: Declare the module**

In `crates/supersigil-git/src/lib.rs`, add `pub mod snapshot;` after `pub mod run;`.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-git`
Expected: PASS, 45 tests in the crate (15 new in `tests/snapshot.rs`, 2 new unit tests in `snapshot`, 1 new in `path`); 44 on Windows, where `snapshot_never_runs_hooks` is `cfg(unix)`.

- [ ] **Step 8: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; all tests pass.

- [ ] **Step 9: Commit**

```bash
git add Cargo.lock crates/supersigil-git/Cargo.toml crates/supersigil-git/src/lib.rs crates/supersigil-git/src/path.rs crates/supersigil-git/src/snapshot.rs crates/supersigil-git/tests/snapshot.rs
git commit -m "feat: snapshot the working tree through a temporary index"
```

---

### Task 6: Changed paths, classification, and bytes

**Files:**
- Modify: `crates/supersigil-git/src/lib.rs` (`pub mod bytes;`, `pub mod changes;`)
- Create: `crates/supersigil-git/src/changes.rs`, `crates/supersigil-git/src/bytes.rs`
- Create: `crates/supersigil-git/tests/changes.rs`

**Interfaces:**
- Consumes: Task 4's `Repo`, `ObjectId`, `RepoPath`, `GitError`; Task 5's `snapshot::nul_fields`, `RepoPath::to_os_string`, `RepoPath::literal_pathspec`.
- Produces:
  - `changes::{Mode (ABSENT, REGULAR, EXECUTABLE, SYMLINK, GITLINK, is_file()), ChangeStatus { Added, Deleted, Modified, TypeChanged }, Change { path, status, old_mode, new_mode, old_blob, new_blob }, changed_paths(repo, base, target, pathspecs), FileKind { Text, Binary, TooLarge, ModeOnly, Symlink, Gitlink, TypeChange, UnsupportedPath }, MAX_DIFF_BYTES, classify_change(change), classify_bytes(old, new)}`. `classify_change` returns `None` when the kind depends on the bytes; the caller then compares `blob_sizes` with `MAX_DIFF_BYTES` (`TooLarge`) and calls `classify_bytes` (`Text` or `Binary`).
  - `bytes::{blob_sizes(repo, ids), read_blobs(repo, ids), Conversion { Identical, Converted(bytes), Failed { status, stderr_tail } }, worktree_form(repo, path, blob, blob_bytes)}`. A missing object is `GitError::Parse` in plan 2; plan 3 turns it into a not-reproducible target when it reopens an old review.

`changed_paths` runs `diff-tree -r -z --raw --no-renames --no-abbrev --ignore-submodules=dirty`: renames are a deletion plus an addition whatever `diff.renames` says, and the flag keeps a gitlink change visible for a submodule configured `ignore = all` (the test shows plain `diff-tree` hiding it). Blob bytes come from one `cat-file --batch` process framed by the header size; attribution bytes come from one `cat-file --filters --path=<path> <blob>` process per blob, because `--batch --filters` reports the unfiltered size in its header on git 2.55.

- [ ] **Step 1: Write the failing tests**

`crates/supersigil-git/tests/changes.rs`. The non-UTF-8 path is created only as an index entry through `update-index -z --index-info` on a separate index file and written with `write-tree`, never as a file on disk (APFS rejects such names):

```rust
//! Changed paths, classification, and bytes against real repositories.

mod common;

use common::TestRepo;
use supersigil_git::bytes::{Conversion, blob_sizes, read_blobs, worktree_form};
use supersigil_git::changes::{
    Change, ChangeStatus, FileKind, Mode, changed_paths, classify_change,
};
use supersigil_git::{GitError, ObjectFormat, ObjectId, RepoPath};

fn id(text: &str) -> ObjectId {
    ObjectId::parse(text.trim(), ObjectFormat::Sha1).unwrap()
}

fn tree(repo: &TestRepo, rev: &str) -> ObjectId {
    id(&repo.run(&["rev-parse", &format!("{rev}^{{tree}}")]))
}

fn summary(changes: &[Change]) -> Vec<(String, ChangeStatus)> {
    changes
        .iter()
        .map(|c| (c.path.display(), c.status))
        .collect()
}

#[test]
fn changed_paths_report_status_modes_and_blobs() {
    let repo = TestRepo::new();
    for name in ["a", "b", "c"] {
        repo.write(&format!("{name}.txt"), format!("{name}\n").as_bytes());
    }
    repo.commit_all("base");
    repo.write("a.txt", b"a changed\n");
    std::fs::remove_file(repo.root.join("b.txt")).unwrap();
    repo.write("d.txt", b"d\n");
    repo.run(&["add", "-A"]);
    repo.run(&["update-index", "--chmod=+x", "c.txt"]);
    repo.run(&["commit", "-q", "-m", "target"]);
    let changes = changed_paths(
        &repo.repo(),
        &tree(&repo, "HEAD~1"),
        &tree(&repo, "HEAD"),
        &[],
    )
    .unwrap();
    assert_eq!(
        summary(&changes),
        [
            ("a.txt".to_owned(), ChangeStatus::Modified),
            ("b.txt".to_owned(), ChangeStatus::Deleted),
            ("c.txt".to_owned(), ChangeStatus::Modified),
            ("d.txt".to_owned(), ChangeStatus::Added),
        ]
    );
    let [a, b, c, d] = changes.as_slice() else {
        unreachable!()
    };
    assert_eq!(
        a.old_blob.as_ref().unwrap(),
        &id(&repo.run(&["rev-parse", "HEAD~1:a.txt"]))
    );
    assert_eq!(
        a.new_blob.as_ref().unwrap(),
        &id(&repo.run(&["rev-parse", "HEAD:a.txt"]))
    );
    assert_eq!((b.new_mode, b.new_blob.as_ref()), (Mode::ABSENT, None));
    assert_eq!((c.old_mode, c.new_mode), (Mode::REGULAR, Mode::EXECUTABLE));
    assert_eq!(classify_change(c), Some(FileKind::ModeOnly));
    assert_eq!((d.old_mode, d.old_blob.as_ref()), (Mode::ABSENT, None));
    assert_eq!(classify_change(a), None);
}

#[test]
fn a_rename_is_a_deletion_plus_an_addition_even_with_rename_config() {
    let repo = TestRepo::new();
    repo.write("old.txt", b"same content\n");
    repo.commit_all("base");
    repo.run(&["config", "diff.renames", "true"]);
    repo.run(&["mv", "old.txt", "new.txt"]);
    repo.run(&["commit", "-q", "-m", "rename"]);
    let changes = changed_paths(
        &repo.repo(),
        &tree(&repo, "HEAD~1"),
        &tree(&repo, "HEAD"),
        &[],
    )
    .unwrap();
    assert_eq!(
        summary(&changes),
        [
            ("new.txt".to_owned(), ChangeStatus::Added),
            ("old.txt".to_owned(), ChangeStatus::Deleted),
        ]
    );
}

#[test]
fn pathspecs_are_literal_prefixes() {
    let repo = TestRepo::new();
    repo.write("src/a.rs", b"a\n");
    repo.write("src*/b.rs", b"b\n");
    repo.write("docs/c.md", b"c\n");
    repo.commit_all("base");
    for path in ["src/a.rs", "src*/b.rs", "docs/c.md"] {
        repo.write(path, b"changed\n");
    }
    repo.commit_all("target");
    let only = |spec: &str| {
        let changes = changed_paths(
            &repo.repo(),
            &tree(&repo, "HEAD~1"),
            &tree(&repo, "HEAD"),
            &[RepoPath::from_utf8(spec)],
        )
        .unwrap();
        summary(&changes)
            .into_iter()
            .map(|(p, _)| p)
            .collect::<Vec<_>>()
    };
    assert_eq!(only("src"), ["src/a.rs"]);
    assert_eq!(only("src*"), ["src*/b.rs"]);
}

#[test]
fn a_gitlink_change_is_listed_for_a_submodule_configured_ignore_all() {
    let repo = TestRepo::new();
    repo.write(
        ".gitmodules",
        b"[submodule \"sm\"]\n\tpath = sm\n\turl = ./sm\n\tignore = all\n",
    );
    let old = "1".repeat(40);
    let new = "2".repeat(40);
    repo.run(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{old},sm"),
    ]);
    repo.run(&["add", ".gitmodules"]);
    repo.run(&["commit", "-q", "-m", "old gitlink"]);
    repo.run(&["update-index", "--cacheinfo", &format!("160000,{new},sm")]);
    repo.run(&["commit", "-q", "-m", "new gitlink"]);
    let (old_tree, new_tree) = (tree(&repo, "HEAD~1"), tree(&repo, "HEAD"));
    // Plain diff-tree hides the change; this is why the flag is passed.
    let plain = repo.run(&[
        "diff-tree",
        "-r",
        "--raw",
        old_tree.as_str(),
        new_tree.as_str(),
    ]);
    assert_eq!(plain, "");
    let changes = changed_paths(&repo.repo(), &old_tree, &new_tree, &[]).unwrap();
    assert_eq!(
        summary(&changes),
        [("sm".to_owned(), ChangeStatus::Modified)]
    );
    assert_eq!(changes[0].old_mode, Mode::GITLINK);
    assert_eq!(classify_change(&changes[0]), Some(FileKind::Gitlink));
}

#[cfg(unix)]
#[test]
fn symlinks_and_type_changes_are_classified() {
    let repo = TestRepo::new();
    repo.write("target.txt", b"t\n");
    repo.write("was-file", b"f\n");
    repo.commit_all("base");
    std::os::unix::fs::symlink("target.txt", repo.root.join("link")).unwrap();
    std::fs::remove_file(repo.root.join("was-file")).unwrap();
    std::os::unix::fs::symlink("target.txt", repo.root.join("was-file")).unwrap();
    repo.commit_all("target");
    let changes = changed_paths(
        &repo.repo(),
        &tree(&repo, "HEAD~1"),
        &tree(&repo, "HEAD"),
        &[],
    )
    .unwrap();
    let kinds: Vec<(String, ChangeStatus, Option<FileKind>)> = changes
        .iter()
        .map(|c| (c.path.display(), c.status, classify_change(c)))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                "link".to_owned(),
                ChangeStatus::Added,
                Some(FileKind::Symlink)
            ),
            (
                "was-file".to_owned(),
                ChangeStatus::TypeChanged,
                Some(FileKind::TypeChange)
            ),
        ]
    );
}

#[test]
fn a_non_utf8_path_in_a_tree_is_parsed_and_unsupported() {
    let repo = TestRepo::new();
    let blob = repo.run_input(&["hash-object", "-w", "--stdin"], b"odd\n");
    let index = repo.dir.path().join("odd-index");
    let mut record = format!("100644 {}\t", blob.trim()).into_bytes();
    record.extend_from_slice(b"odd\xffname.txt\0");
    let git = repo.git().with_env("GIT_INDEX_FILE", &index);
    git.output_with_input(["update-index", "--add", "-z", "--index-info"], &record)
        .unwrap();
    let written = id(&String::from_utf8(git.output(["write-tree"]).unwrap()).unwrap());
    let opened = repo.repo();
    let changes = changed_paths(&opened, &opened.empty_tree().unwrap(), &written, &[]).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path.as_bytes(), b"odd\xffname.txt");
    assert_eq!(changes[0].path.escaped(), r"odd\xffname.txt");
    assert_eq!(
        classify_change(&changes[0]),
        Some(FileKind::UnsupportedPath)
    );
}

#[test]
fn blobs_are_read_in_one_batch_with_exact_bytes_and_sizes() {
    let repo = TestRepo::new();
    let contents: [&[u8]; 3] = [b"line\nwith\nnewlines\n", b"bin\0ary\n\0", b""];
    let ids: Vec<ObjectId> = contents
        .iter()
        .map(|c| id(&repo.run_input(&["hash-object", "-w", "--stdin"], c)))
        .collect();
    let opened = repo.repo();
    let blobs = read_blobs(&opened, &ids).unwrap();
    let sizes = blob_sizes(&opened, &ids).unwrap();
    for (id, content) in ids.iter().zip(contents) {
        assert_eq!(blobs[id], content);
        assert_eq!(sizes[id], content.len() as u64);
    }
}

#[test]
fn a_missing_object_is_an_error() {
    let repo = TestRepo::new();
    let missing = id(&"3".repeat(40));
    let opened = repo.repo();
    assert!(matches!(
        read_blobs(&opened, std::slice::from_ref(&missing)),
        Err(GitError::Parse(_))
    ));
    assert!(matches!(
        blob_sizes(&opened, &[missing]),
        Err(GitError::Parse(_))
    ));
}

#[test]
fn the_worktree_form_is_identical_without_conversion() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\nb\n");
    repo.commit_all("base");
    let blob = id(&repo.run(&["rev-parse", "HEAD:a.txt"]));
    let opened = repo.repo();
    let bytes = read_blobs(&opened, std::slice::from_ref(&blob))
        .unwrap()
        .remove(&blob)
        .unwrap();
    assert_eq!(
        worktree_form(&opened, &RepoPath::from_utf8("a.txt"), &blob, &bytes).unwrap(),
        Conversion::Identical
    );
}

#[test]
fn the_worktree_form_converts_line_endings_under_autocrlf() {
    let repo = TestRepo::new();
    repo.run(&["config", "core.autocrlf", "true"]);
    repo.write("a.txt", b"a\r\nb\r\n");
    repo.commit_all("crlf on disk");
    let blob = id(&repo.run(&["rev-parse", "HEAD:a.txt"]));
    let opened = repo.repo();
    let bytes = read_blobs(&opened, std::slice::from_ref(&blob))
        .unwrap()
        .remove(&blob)
        .unwrap();
    assert_eq!(bytes, b"a\nb\n");
    assert_eq!(
        worktree_form(&opened, &RepoPath::from_utf8("a.txt"), &blob, &bytes).unwrap(),
        Conversion::Converted(b"a\r\nb\r\n".to_vec())
    );
}

#[test]
fn the_worktree_form_follows_an_eol_attribute() {
    let repo = TestRepo::new();
    repo.write(".gitattributes", b"*.txt eol=crlf\n");
    repo.write("a.txt", b"a\nb\n");
    repo.commit_all("eol attribute");
    let blob = id(&repo.run(&["rev-parse", "HEAD:a.txt"]));
    let opened = repo.repo();
    assert_eq!(
        worktree_form(&opened, &RepoPath::from_utf8("a.txt"), &blob, b"a\nb\n").unwrap(),
        Conversion::Converted(b"a\r\nb\r\n".to_vec())
    );
}

#[cfg(unix)]
#[test]
fn a_failed_required_filter_is_reported_not_raised() {
    let repo = TestRepo::new();
    repo.write(".gitattributes", b"*.bad filter=boom\n");
    repo.run(&["config", "filter.boom.clean", "cat"]);
    repo.run(&["config", "filter.boom.smudge", "false"]);
    repo.run(&["config", "filter.boom.required", "true"]);
    let blob = id(&repo.run_input(&["hash-object", "-w", "--stdin"], b"x\n"));
    let result = worktree_form(&repo.repo(), &RepoPath::from_utf8("x.bad"), &blob, b"x\n").unwrap();
    assert!(
        matches!(&result, Conversion::Failed { status: Some(code), stderr_tail } if *code != 0 && !stderr_tail.is_empty()),
        "{result:?}"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-git`
Expected: FAIL to compile with `could not find bytes in supersigil_git` and `could not find changes in supersigil_git`.

- [ ] **Step 3: Write changed paths and classification**

`crates/supersigil-git/src/changes.rs`:

```rust
//! Changed paths between two trees, and what kind of change each one is.

use std::ffi::OsString;

use crate::error::GitError;
use crate::oid::ObjectId;
use crate::path::RepoPath;
use crate::repo::Repo;
use crate::snapshot::nul_fields;

/// A file mode as git records it in a tree, `Mode(0)` for an absent side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct Mode(pub u32);

impl Mode {
    /// No entry on this side.
    pub const ABSENT: Self = Self(0);
    /// A regular file.
    pub const REGULAR: Self = Self(0o100_644);
    /// An executable file.
    pub const EXECUTABLE: Self = Self(0o100_755);
    /// A symbolic link; the blob holds the link target.
    pub const SYMLINK: Self = Self(0o120_000);
    /// A submodule commit.
    pub const GITLINK: Self = Self(0o160_000);

    /// Returns whether this is a regular or executable file.
    #[must_use]
    pub fn is_file(self) -> bool {
        self == Self::REGULAR || self == Self::EXECUTABLE
    }
}

/// How an entry changed between the two trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    /// Only in the target.
    Added,
    /// Only in the base.
    Deleted,
    /// In both, with different contents or mode.
    Modified,
    /// In both, as different kinds of entry (for example a file that became
    /// a symbolic link).
    TypeChanged,
}

/// One changed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The path, relative to the repository root.
    pub path: RepoPath,
    /// How it changed.
    pub status: ChangeStatus,
    /// Mode in the base.
    pub old_mode: Mode,
    /// Mode in the target.
    pub new_mode: Mode,
    /// Blob in the base, `None` when absent.
    pub old_blob: Option<ObjectId>,
    /// Blob in the target, `None` when absent.
    pub new_blob: Option<ObjectId>,
}

/// Lists the entries that differ between the trees `base` and `target`, with
/// `diff-tree -r -z --raw --no-renames --no-abbrev --ignore-submodules=dirty`.
///
/// Renames appear as a deletion plus an addition: rename detection is
/// derived lineage, which plan 2 does not produce. `--ignore-submodules=dirty`
/// keeps a gitlink change visible for a submodule configured `ignore = all`,
/// while submodule contents stay outside the review. `pathspecs` are literal
/// path prefixes; empty means everything.
///
/// # Errors
///
/// Returns the errors of [`crate::Git::output`], or [`GitError::Parse`] for
/// output this function does not understand.
pub fn changed_paths(
    repo: &Repo,
    base: &ObjectId,
    target: &ObjectId,
    pathspecs: &[RepoPath],
) -> Result<Vec<Change>, GitError> {
    let mut args: Vec<OsString> = [
        "diff-tree",
        "-r",
        "-z",
        "--raw",
        "--no-renames",
        "--no-abbrev",
        "--ignore-submodules=dirty",
        base.as_str(),
        target.as_str(),
        "--",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    args.extend(pathspecs.iter().filter_map(RepoPath::literal_pathspec));
    let output = repo.git().output(&args)?;
    parse_raw(&output, repo)
}

/// Parses `--raw -z` records: `:<old mode> <new mode> <old id> <new id> <status>`
/// then the path, each NUL-terminated.
fn parse_raw(output: &[u8], repo: &Repo) -> Result<Vec<Change>, GitError> {
    let fields: Vec<&[u8]> = nul_fields(output).collect();
    let mut changes = Vec::with_capacity(fields.len() / 2);
    for record in fields.chunks(2) {
        let [meta, path] = record else {
            return Err(GitError::Parse(
                "diff-tree output ends mid-record".to_owned(),
            ));
        };
        let meta = std::str::from_utf8(meta)
            .map_err(|_not_utf8| GitError::Parse("diff-tree record is not UTF-8".to_owned()))?;
        let parts: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
        let [old_mode, new_mode, old_id, new_id, status] = parts.as_slice() else {
            return Err(GitError::Parse(format!("diff-tree record {meta:?}")));
        };
        let id = |text: &str| -> Result<Option<ObjectId>, GitError> {
            let id = ObjectId::parse(text, repo.format())?;
            Ok((!id.is_null()).then_some(id))
        };
        changes.push(Change {
            path: RepoPath::new(path.to_vec()),
            status: match *status {
                "A" => ChangeStatus::Added,
                "D" => ChangeStatus::Deleted,
                "M" => ChangeStatus::Modified,
                "T" => ChangeStatus::TypeChanged,
                other => return Err(GitError::Parse(format!("diff-tree status {other:?}"))),
            },
            old_mode: parse_mode(old_mode)?,
            new_mode: parse_mode(new_mode)?,
            old_blob: id(old_id)?,
            new_blob: id(new_id)?,
        });
    }
    Ok(changes)
}

/// Parses an octal mode.
fn parse_mode(text: &str) -> Result<Mode, GitError> {
    u32::from_str_radix(text, 8)
        .map(Mode)
        .map_err(|_not_octal| GitError::Parse(format!("file mode {text:?}")))
}

/// What the review can do with a changed entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// UTF-8 text on every present side: diffed and attributed.
    Text,
    /// A NUL byte in the first 8000 bytes, or not UTF-8: listed, not diffed.
    Binary,
    /// A side larger than [`MAX_DIFF_BYTES`]: listed as too large to diff.
    TooLarge,
    /// Same blob, different mode: listed.
    ModeOnly,
    /// A symbolic link on either side: listed, not diffed, not attributed.
    Symlink,
    /// A submodule commit on either side: listed, not diffed, not attributed.
    Gitlink,
    /// The entry changed kind: listed, not diffed, not attributed.
    TypeChange,
    /// The path is not UTF-8: listed as unsupported.
    UnsupportedPath,
}

/// Largest blob the review diffs.
pub const MAX_DIFF_BYTES: u64 = 8 * 1024 * 1024;

/// Classifies what can be decided without reading blobs: an unsupported
/// path, a type change, a symbolic link, a gitlink, or a mode-only change.
/// Returns `None` when the kind depends on the bytes.
#[must_use]
pub fn classify_change(change: &Change) -> Option<FileKind> {
    let either = |mode: Mode| change.old_mode == mode || change.new_mode == mode;
    if change.path.to_str().is_none() {
        Some(FileKind::UnsupportedPath)
    } else if change.status == ChangeStatus::TypeChanged {
        Some(FileKind::TypeChange)
    } else if either(Mode::SYMLINK) {
        Some(FileKind::Symlink)
    } else if either(Mode::GITLINK) {
        Some(FileKind::Gitlink)
    } else if change.status == ChangeStatus::Modified && change.old_blob == change.new_blob {
        Some(FileKind::ModeOnly)
    } else {
        None
    }
}

/// Bytes git's binary heuristic inspects.
const SNIFF_BYTES: usize = 8000;

/// Classifies the present sides by content: [`FileKind::Binary`] when either
/// has a NUL byte in its first 8000 bytes (git's heuristic) or is not UTF-8,
/// otherwise [`FileKind::Text`].
#[must_use]
pub fn classify_bytes(old: Option<&[u8]>, new: Option<&[u8]>) -> FileKind {
    let binary = |bytes: &[u8]| {
        bytes[..bytes.len().min(SNIFF_BYTES)].contains(&0) || std::str::from_utf8(bytes).is_err()
    };
    if old.into_iter().chain(new).any(binary) {
        FileKind::Binary
    } else {
        FileKind::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(status: ChangeStatus, old_mode: Mode, new_mode: Mode, same_blob: bool) -> Change {
        let id =
            |c: char| ObjectId::parse(&c.to_string().repeat(40), crate::ObjectFormat::Sha1).ok();
        Change {
            path: RepoPath::from_utf8("f"),
            status,
            old_mode,
            new_mode,
            old_blob: id('a'),
            new_blob: if same_blob { id('a') } else { id('b') },
        }
    }

    #[test]
    fn kinds_decided_without_bytes() {
        use ChangeStatus::{Modified, TypeChanged};
        let cases = [
            (
                change(TypeChanged, Mode::REGULAR, Mode::SYMLINK, false),
                Some(FileKind::TypeChange),
            ),
            (
                change(Modified, Mode::SYMLINK, Mode::SYMLINK, false),
                Some(FileKind::Symlink),
            ),
            (
                change(Modified, Mode::GITLINK, Mode::GITLINK, false),
                Some(FileKind::Gitlink),
            ),
            (
                change(Modified, Mode::REGULAR, Mode::EXECUTABLE, true),
                Some(FileKind::ModeOnly),
            ),
            (change(Modified, Mode::REGULAR, Mode::REGULAR, false), None),
        ];
        for (change, expected) in cases {
            assert_eq!(classify_change(&change), expected, "{change:?}");
        }
        let mut odd = change(Modified, Mode::REGULAR, Mode::REGULAR, false);
        odd.path = RepoPath::new(b"odd\xff".to_vec());
        assert_eq!(classify_change(&odd), Some(FileKind::UnsupportedPath));
    }

    #[test]
    fn binary_follows_gits_heuristic_and_utf8() {
        assert_eq!(
            classify_bytes(Some(b"text\n"), Some(b"more\n")),
            FileKind::Text
        );
        assert_eq!(classify_bytes(None, Some(b"new\n")), FileKind::Text);
        assert_eq!(classify_bytes(Some(b"a\0b"), Some(b"x")), FileKind::Binary);
        assert_eq!(
            classify_bytes(Some(b"x"), Some(b"\xff\xfe")),
            FileKind::Binary
        );
        let mut late_nul = vec![b'a'; SNIFF_BYTES];
        late_nul.push(0);
        // Past the sniffed prefix a NUL is still not UTF-8-invalid, so the
        // file is text by git's heuristic.
        assert_eq!(classify_bytes(Some(&late_nul), None), FileKind::Text);
    }

    #[test]
    fn file_modes_are_recognized() {
        assert!(Mode::REGULAR.is_file());
        assert!(Mode::EXECUTABLE.is_file());
        assert!(!Mode::SYMLINK.is_file());
        assert!(!Mode::ABSENT.is_file());
        assert_eq!(parse_mode("100644").unwrap(), Mode::REGULAR);
        assert_eq!(parse_mode("000000").unwrap(), Mode::ABSENT);
        parse_mode("9").unwrap_err();
    }
}
```

- [ ] **Step 4: Write blob bytes and the worktree form**

`crates/supersigil-git/src/bytes.rs`:

```rust
//! Blob contents, and their worktree form for attribution.
//!
//! The reviewed diff uses blob bytes: the immutable object contents, so the
//! change depends only on the two trees. Attribution uses the worktree form,
//! because transcript hashes were computed over files on disk; converting
//! depends on the current attributes and configuration, so the result is
//! evidence of this review run, not a property of the tree.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;

use crate::error::GitError;
use crate::oid::ObjectId;
use crate::path::RepoPath;
use crate::repo::Repo;

/// Returns the size of each blob with `cat-file --batch-check`.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if an object is missing or the output is not
/// understood, or the errors of [`crate::Git::output_with_input`].
pub fn blob_sizes(repo: &Repo, ids: &[ObjectId]) -> Result<BTreeMap<ObjectId, u64>, GitError> {
    let unique: BTreeSet<&ObjectId> = ids.iter().collect();
    let output = repo
        .git()
        .output_with_input(["cat-file", "--batch-check"], &request(&unique))?;
    let text = String::from_utf8_lossy(&output);
    let mut sizes = BTreeMap::new();
    for line in text.lines() {
        let (id, size) = parse_header(line, repo)?;
        sizes.insert(id, size);
    }
    Ok(sizes)
}

/// Reads each blob's bytes with one `cat-file --batch` process.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if an object is missing or the output is not
/// understood, or the errors of [`crate::Git::output_with_input`].
pub fn read_blobs(repo: &Repo, ids: &[ObjectId]) -> Result<BTreeMap<ObjectId, Vec<u8>>, GitError> {
    let unique: BTreeSet<&ObjectId> = ids.iter().collect();
    let output = repo
        .git()
        .output_with_input(["cat-file", "--batch"], &request(&unique))?;
    let mut blobs = BTreeMap::new();
    let mut rest: &[u8] = &output;
    while !rest.is_empty() {
        let newline = rest
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| GitError::Parse("cat-file header without a newline".to_owned()))?;
        let header = String::from_utf8_lossy(&rest[..newline]);
        let (id, size) = parse_header(&header, repo)?;
        let start = newline + 1;
        let end = usize::try_from(size)
            .ok()
            .and_then(|size| start.checked_add(size))
            .filter(|&end| end < rest.len() && rest[end] == b'\n')
            .ok_or_else(|| GitError::Parse(format!("cat-file contents of {id} are truncated")))?;
        blobs.insert(id, rest[start..end].to_vec());
        rest = &rest[end + 1..];
    }
    Ok(blobs)
}

/// One object id per line.
fn request(ids: &BTreeSet<&ObjectId>) -> Vec<u8> {
    let mut input = Vec::new();
    for id in ids {
        input.extend_from_slice(id.as_str().as_bytes());
        input.push(b'\n');
    }
    input
}

/// Parses `<id> blob <size>`; anything else (a missing object, another
/// type) is an error.
fn parse_header(line: &str, repo: &Repo) -> Result<(ObjectId, u64), GitError> {
    let parts: Vec<&str> = line.split(' ').collect();
    match parts.as_slice() {
        [id, "blob", size] => {
            let size = size
                .parse()
                .map_err(|_not_a_size| GitError::Parse(format!("cat-file size in {line:?}")))?;
            Ok((ObjectId::parse(id, repo.format())?, size))
        }
        _ => Err(GitError::Parse(format!("cat-file header {line:?}"))),
    }
}

/// The worktree form of a blob, compared with its blob bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conversion {
    /// The worktree form is byte-identical to the blob.
    Identical,
    /// The worktree form differs, for example in line endings.
    Converted(Vec<u8>),
    /// Git could not convert it, for example because a required filter
    /// failed.
    Failed {
        /// Exit code, `None` when git was ended by a signal.
        status: Option<i32>,
        /// The end of standard error.
        stderr_tail: String,
    },
}

/// Characters of standard error kept in [`Conversion::Failed`].
const STDERR_TAIL_CHARS: usize = 500;

/// Converts `blob` to its worktree form for `path` with
/// `cat-file --filters --path=<path>`, one process per blob: on git 2.55,
/// `--batch --filters` writes the filtered bytes but reports the unfiltered
/// size in its header, so batch framing breaks. Attributes are read from the
/// worktree as it is now.
///
/// # Errors
///
/// Returns [`GitError::Parse`] if `path` cannot be passed to git on this
/// platform, or the errors of [`crate::Git::raw`]. A conversion git refuses is
/// [`Conversion::Failed`], not an error.
pub fn worktree_form(
    repo: &Repo,
    path: &RepoPath,
    blob: &ObjectId,
    blob_bytes: &[u8],
) -> Result<Conversion, GitError> {
    let Some(os_path) = path.to_os_string() else {
        return Err(GitError::Parse(format!(
            "path {} is not representable on this platform",
            path.escaped()
        )));
    };
    let mut path_arg = OsString::from("--path=");
    path_arg.push(os_path);
    let output = repo.git().raw([
        OsString::from("cat-file"),
        OsString::from("--filters"),
        path_arg,
        OsString::from(blob.as_str()),
    ])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let chars: Vec<char> = stderr.trim().chars().collect();
        let tail: String = chars[chars.len().saturating_sub(STDERR_TAIL_CHARS)..]
            .iter()
            .collect();
        return Ok(Conversion::Failed {
            status: output.status.code(),
            stderr_tail: tail,
        });
    }
    if output.stdout == blob_bytes {
        Ok(Conversion::Identical)
    } else {
        Ok(Conversion::Converted(output.stdout))
    }
}
```

- [ ] **Step 5: Declare the modules**

In `crates/supersigil-git/src/lib.rs`, add `pub mod bytes;` and `pub mod changes;` before `pub mod error;`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-git`
Expected: PASS, 60 tests in the crate (12 new in `tests/changes.rs`, 3 new unit tests in `changes`); two of the new ones are `cfg(unix)`.

- [ ] **Step 7: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; all tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/supersigil-git/src/lib.rs crates/supersigil-git/src/changes.rs crates/supersigil-git/src/bytes.rs crates/supersigil-git/tests/changes.rs
git commit -m "feat: list changed paths and read blob bytes"
```

---

### Task 7: Commit origins from worktree reflogs

**Files:**
- Modify: `crates/supersigil-git/src/lib.rs` (`pub mod origin;` and the final crate documentation)
- Create: `crates/supersigil-git/src/origin.rs`
- Create: `crates/supersigil-git/tests/origin.rs`

**Interfaces:**
- Consumes: Task 4's `Repo`, `ObjectId`, `GitError`, `worktree::Worktree`, `worktree::list_worktrees`; Task 5's `snapshot::nul_fields`.
- Produces: `origin::{is_origin_subject(subject), ReflogEntry { commit, subject }, head_reflog(repo, worktree), CommitOrigin { commit, worktrees }, UnavailableOrigin { worktree, reason }, Origins { commits, without_origin, unavailable, origin_worktrees() }, find_origins(repo, worktrees, commits)}`. `head_reflog` returns no entries when git cannot read one (logging disabled, an expired reflog, an unborn HEAD). `find_origins` skips bare entries and lists a prunable worktree, or one whose directory is missing, as unavailable with the reason `worktree directory is missing`. Record associations that map to no registered worktree are the CLI's to add to `unavailable` (Task 13), since only it has the records.

The grammar is design section 2's: the action is the text before the first `": "`, the message the rest; `commit`, `commit (…)`, and `am` are origins whatever the message; `cherry-pick`, `revert`, and any action ending in a sequencer suffix (` (pick)`, ` (reword)`, ` (edit)`, ` (squash)`, ` (fixup)`, ` (continue)`, ` (merge)`) are origins unless the message is exactly `fast-forward`; `merge …` and `pull…` are origins only with a message starting `Merge made by`. Reflogs are read with `log -g -z --no-abbrev --no-color --no-show-signature --format=%H%x1f%gs HEAD --`, so user settings such as `log.showSignature` cannot change the output. Every subject form in the unit table was observed on git 2.55 except `rebase (merge)`, which comes from git's source; the integration tests produce the observed ones through real workflows.

- [ ] **Step 1: Write the failing tests**

`crates/supersigil-git/tests/origin.rs`. The fast-forward case makes the commit in a linked worktree and fast-forwards the main worktree onto it, so only the linked worktree's reflog records creating it:

```rust
//! Reflog origins produced by real git workflows.

mod common;

use std::path::{Path, PathBuf};

use common::TestRepo;
use supersigil_git::origin::{Origins, find_origins, head_reflog};
use supersigil_git::worktree::list_worktrees;
use supersigil_git::{ObjectFormat, ObjectId, Repo};

fn id(text: &str) -> ObjectId {
    ObjectId::parse(text.trim(), ObjectFormat::Sha1).unwrap()
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

/// Origins of `commits`, seen from the repository at `root`.
fn origins_of(repo: &TestRepo, commits: &[&str]) -> Origins {
    let opened = repo.repo();
    let worktrees = list_worktrees(&opened).unwrap();
    let commits: Vec<ObjectId> = commits.iter().map(|c| id(c)).collect();
    find_origins(&opened, &worktrees, &commits).unwrap()
}

/// The canonical origin worktrees of `commit`, empty when it has none.
fn origin_dirs(origins: &Origins, commit: &str) -> Vec<PathBuf> {
    origins
        .commits
        .iter()
        .find(|o| o.commit.as_str() == commit.trim())
        .map(|o| o.worktrees.iter().map(|w| canonical(w)).collect())
        .unwrap_or_default()
}

fn head(repo: &TestRepo, dir: &Path) -> String {
    repo.run_in(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

#[test]
fn commit_and_amend_are_origins() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let first = repo.commit_all("first");
    repo.write("a.txt", b"b\n");
    repo.run(&["commit", "-q", "-a", "--amend", "-m", "first, amended"]);
    let amended = head(&repo, &repo.root);
    let origins = origins_of(&repo, &[&first, &amended]);
    assert_eq!(origin_dirs(&origins, &first), [canonical(&repo.root)]);
    assert_eq!(origin_dirs(&origins, &amended), [canonical(&repo.root)]);
    assert!(origins.without_origin.is_empty());
}

#[test]
fn a_commit_titled_fast_forward_is_still_an_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let commit = repo.commit_all("Fast-forward handling fix");
    let origins = origins_of(&repo, &[&commit]);
    assert_eq!(origin_dirs(&origins, &commit), [canonical(&repo.root)]);
}

#[test]
fn a_merge_commit_is_an_origin_but_a_fast_forward_is_not() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    let linked = repo.dir.path().join("linked");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat",
        linked.to_str().unwrap(),
    ]);
    std::fs::write(linked.join("f.txt"), b"f\n").unwrap();
    repo.run_in(&linked, &["add", "f.txt"]);
    repo.run_in(&linked, &["commit", "-q", "-m", "feature"]);
    let feature = head(&repo, &linked);
    // Fast-forward main onto the feature commit made in the linked worktree.
    repo.run(&["merge", "-q", "--ff-only", "feat"]);
    repo.write("m.txt", b"m\n");
    let side = repo.commit_all("main moves on");
    repo.run_in(
        &linked,
        &["commit", "-q", "--allow-empty", "-m", "feature two"],
    );
    let feature_two = head(&repo, &linked);
    repo.run(&["merge", "-q", "--no-edit", "--no-ff", "feat"]);
    let merge = head(&repo, &repo.root);
    let origins = origins_of(&repo, &[&feature, &side, &feature_two, &merge]);
    assert_eq!(origin_dirs(&origins, &feature), [canonical(&linked)]);
    assert_eq!(origin_dirs(&origins, &side), [canonical(&repo.root)]);
    assert_eq!(origin_dirs(&origins, &feature_two), [canonical(&linked)]);
    assert_eq!(origin_dirs(&origins, &merge), [canonical(&repo.root)]);
}

#[test]
fn cherry_pick_and_revert_are_origins() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("s.txt", b"s\n");
    let side = repo.commit_all("side");
    repo.run(&["switch", "-q", "main"]);
    repo.run(&["cherry-pick", side.as_str()]);
    let picked = head(&repo, &repo.root);
    repo.run(&["revert", "--no-edit", "HEAD"]);
    let reverted = head(&repo, &repo.root);
    let origins = origins_of(&repo, &[&picked, &reverted]);
    assert_eq!(origin_dirs(&origins, &picked), [canonical(&repo.root)]);
    assert_eq!(origin_dirs(&origins, &reverted), [canonical(&repo.root)]);
}

#[test]
fn an_applied_patch_is_an_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("s.txt", b"s\n");
    repo.commit_all("side");
    let patches = repo.dir.path().join("patches");
    repo.run(&[
        "format-patch",
        "-q",
        "-1",
        "HEAD",
        "-o",
        patches.to_str().unwrap(),
    ]);
    repo.run(&["switch", "-q", "main"]);
    let patch = std::fs::read_dir(&patches)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    repo.run(&["am", "-q", patch.to_str().unwrap()]);
    let applied = head(&repo, &repo.root);
    let origins = origins_of(&repo, &[&applied]);
    assert_eq!(origin_dirs(&origins, &applied), [canonical(&repo.root)]);
}

#[test]
fn a_rebased_commit_is_an_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    repo.run(&["switch", "-q", "-c", "side"]);
    repo.write("s.txt", b"s\n");
    repo.commit_all("side");
    repo.run(&["switch", "-q", "main"]);
    repo.write("m.txt", b"m\n");
    repo.commit_all("main");
    repo.run(&["switch", "-q", "side"]);
    repo.run(&["rebase", "-q", "main"]);
    let rebased = head(&repo, &repo.root);
    let entries = head_reflog(&repo.repo(), &repo.root).unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e.commit.as_str() == rebased && e.subject.starts_with("rebase (pick)")),
        "{entries:?}"
    );
    let origins = origins_of(&repo, &[&rebased]);
    assert_eq!(origin_dirs(&origins, &rebased), [canonical(&repo.root)]);
}

#[test]
fn pulls_that_create_commits_are_origins_in_the_pulling_worktree() {
    let upstream = TestRepo::new();
    upstream.write("a.txt", b"a\n");
    upstream.commit_all("base");
    let clone = upstream.dir.path().join("clone");
    upstream.run(&[
        "clone",
        "-q",
        upstream.root.to_str().unwrap(),
        clone.to_str().unwrap(),
    ]);
    upstream.run_in(&clone, &["config", "core.autocrlf", "false"]);

    // pull --rebase replays a local commit onto new upstream work.
    std::fs::write(clone.join("local.txt"), b"local\n").unwrap();
    upstream.run_in(&clone, &["add", "local.txt"]);
    upstream.run_in(&clone, &["commit", "-q", "-m", "local"]);
    upstream.write("up.txt", b"up\n");
    upstream.commit_all("upstream");
    upstream.run_in(&clone, &["pull", "-q", "--rebase", "origin", "main"]);
    let rebased = head(&upstream, &clone);

    // pull --no-rebase makes a merge commit.
    std::fs::write(clone.join("local2.txt"), b"local2\n").unwrap();
    upstream.run_in(&clone, &["add", "local2.txt"]);
    upstream.run_in(&clone, &["commit", "-q", "-m", "local two"]);
    upstream.write("up2.txt", b"up2\n");
    upstream.commit_all("upstream two");
    upstream.run_in(
        &clone,
        &["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"],
    );
    let merged = head(&upstream, &clone);

    let opened = Repo::open(upstream.git().in_dir(&clone)).unwrap();
    let worktrees = list_worktrees(&opened).unwrap();
    let origins = find_origins(&opened, &worktrees, &[id(&rebased), id(&merged)]).unwrap();
    assert_eq!(origin_dirs(&origins, &rebased), [canonical(&clone)]);
    assert_eq!(origin_dirs(&origins, &merged), [canonical(&clone)]);
}

#[test]
fn a_linked_worktree_keeps_its_own_reflog() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let base = repo.commit_all("base");
    let linked = repo.root.join(".claude/worktrees/feature");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        linked.to_str().unwrap(),
    ]);
    std::fs::write(linked.join("f.txt"), b"f\n").unwrap();
    repo.run_in(&linked, &["add", "f.txt"]);
    repo.run_in(&linked, &["commit", "-q", "-m", "feature"]);
    let feature = head(&repo, &linked);
    let origins = origins_of(&repo, &[&base, &feature]);
    assert_eq!(origin_dirs(&origins, &base), [canonical(&repo.root)]);
    assert_eq!(origin_dirs(&origins, &feature), [canonical(&linked)]);
    assert_eq!(
        origins
            .origin_worktrees()
            .iter()
            .map(|w| canonical(w))
            .collect::<Vec<_>>()
            .len(),
        2
    );
}

#[test]
fn a_prunable_worktree_is_unavailable_and_its_commits_have_no_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    repo.commit_all("base");
    let linked = repo.dir.path().join("gone");
    repo.run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "gone",
        linked.to_str().unwrap(),
    ]);
    std::fs::write(linked.join("g.txt"), b"g\n").unwrap();
    repo.run_in(&linked, &["add", "g.txt"]);
    repo.run_in(
        &linked,
        &["commit", "-q", "-m", "made in the gone worktree"],
    );
    let made = head(&repo, &linked);
    std::fs::remove_dir_all(&linked).unwrap();
    let origins = origins_of(&repo, &[&made]);
    assert_eq!(origins.without_origin, [id(&made)]);
    assert_eq!(origins.unavailable.len(), 1);
    assert_eq!(
        origins.unavailable[0].reason,
        "worktree directory is missing"
    );
}

#[test]
fn an_expired_reflog_leaves_commits_without_origin() {
    let repo = TestRepo::new();
    repo.write("a.txt", b"a\n");
    let commit = repo.commit_all("first");
    repo.run(&["reflog", "expire", "--expire=now", "--all"]);
    assert!(head_reflog(&repo.repo(), &repo.root).unwrap().is_empty());
    let origins = origins_of(&repo, &[&commit]);
    assert!(origins.commits.is_empty());
    assert_eq!(origins.without_origin, [id(&commit)]);
}

#[test]
fn an_unborn_worktree_has_no_reflog_entries() {
    let repo = TestRepo::new();
    assert!(head_reflog(&repo.repo(), &repo.root).unwrap().is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-git`
Expected: FAIL to compile with `could not find origin in supersigil_git`.

- [ ] **Step 3: Write the origins**

`crates/supersigil-git/src/origin.rs`:

```rust
//! Which worktree created a reviewed commit, from each worktree's HEAD reflog.
//!
//! A worktree originates a commit when its own HEAD reflog has a
//! commit-family entry for it. The association is computed at review time
//! and never stored; an expired reflog or a pruned worktree simply leaves a
//! commit without an origin, which the review reports.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::GitError;
use crate::oid::ObjectId;
use crate::repo::Repo;
use crate::snapshot::nul_fields;
use crate::worktree::Worktree;

/// Actions of the sequencer (rebase, cherry-pick, revert, and pull with
/// rebase) that create a commit, as the parenthesized suffix git appends.
const SEQUENCER_SUFFIXES: [&str; 7] = [
    " (pick)",
    " (reword)",
    " (edit)",
    " (squash)",
    " (fixup)",
    " (continue)",
    " (merge)",
];

/// Returns whether a reflog subject records the creation of its commit.
///
/// The action is the text before the first `": "` (refnames cannot contain a
/// space, so a refspec such as `main:main` stays inside the action) and the
/// message is the rest. An entry is an origin when:
///
/// - its action is `commit` or `commit (…)`, or `am`, whatever the message
///   (a commit may be titled "Fast-forward handling fix");
/// - its action is `cherry-pick` or `revert`, or ends with a sequencer
///   suffix such as ` (pick)` whatever precedes it (`rebase`, `rebase -i`,
///   `pull --rebase origin main`), and its message is not exactly
///   `fast-forward`: the sequencer writes `<action>: fast-forward` when it
///   moves HEAD to an existing commit, and a commit whose whole title is
///   `fast-forward` is indistinguishable, so it is conservatively not an
///   origin;
/// - its action starts with `merge ` or `pull` and its message starts with
///   `Merge made by` (fast-forward merges and pulls never match).
///
/// Any other action, including a bare `rebase`, is not evidence. Git writes
/// these subjects in English regardless of locale.
#[must_use]
pub fn is_origin_subject(subject: &str) -> bool {
    let Some((action, message)) = subject.split_once(": ") else {
        return false;
    };
    if action == "commit"
        || action == "am"
        || (action.starts_with("commit (") && action.ends_with(')'))
    {
        return true;
    }
    let sequencer = action == "cherry-pick"
        || action == "revert"
        || SEQUENCER_SUFFIXES
            .iter()
            .any(|suffix| action.ends_with(suffix));
    if sequencer {
        return message != "fast-forward";
    }
    (action.starts_with("merge ") || action.starts_with("pull"))
        && message.starts_with("Merge made by")
}

/// One entry of a worktree's HEAD reflog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    /// The commit HEAD moved to.
    pub commit: ObjectId,
    /// The reflog subject, for example `commit: add greet`.
    pub subject: String,
}

/// Reads the HEAD reflog of the worktree at `worktree`, newest first. Each
/// linked worktree has its own. A worktree without a reflog (logging
/// disabled, or an unborn HEAD) has no entries.
///
/// # Errors
///
/// Returns the errors of [`crate::Git::raw`], or [`GitError::Parse`] for
/// entries this function does not understand.
pub fn head_reflog(repo: &Repo, worktree: &Path) -> Result<Vec<ReflogEntry>, GitError> {
    let output = repo.git().in_dir(worktree).raw([
        "log",
        "-g",
        "-z",
        "--no-abbrev",
        "--no-color",
        "--no-show-signature",
        "--format=%H%x1f%gs",
        "HEAD",
        "--",
    ])?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for record in nul_fields(&output.stdout) {
        let record = String::from_utf8_lossy(record);
        let record = record.trim_matches('\n');
        let (commit, subject) = record
            .split_once('\u{1f}')
            .ok_or_else(|| GitError::Parse(format!("reflog entry {record:?}")))?;
        entries.push(ReflogEntry {
            commit: ObjectId::parse(commit, repo.format())?,
            subject: subject.to_owned(),
        });
    }
    Ok(entries)
}

/// A reviewed commit and the worktrees that created it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitOrigin {
    /// The commit.
    pub commit: ObjectId,
    /// Worktrees whose HEAD reflog records creating it, sorted.
    pub worktrees: Vec<PathBuf>,
}

/// A worktree whose reflog could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableOrigin {
    /// The worktree's registered path.
    pub worktree: PathBuf,
    /// Why its evidence is unavailable.
    pub reason: String,
}

/// Origins of the reviewed commits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Origins {
    /// Commits with at least one origin, in the order they were given.
    pub commits: Vec<CommitOrigin>,
    /// Commits no readable reflog records creating, in the order given.
    pub without_origin: Vec<ObjectId>,
    /// Registered worktrees whose directory is missing.
    pub unavailable: Vec<UnavailableOrigin>,
}

impl Origins {
    /// Returns every worktree that originated at least one reviewed commit.
    #[must_use]
    pub fn origin_worktrees(&self) -> BTreeSet<PathBuf> {
        self.commits
            .iter()
            .flat_map(|origin| origin.worktrees.iter().cloned())
            .collect()
    }
}

/// Finds, for each of `commits`, the worktrees whose own HEAD reflog has a
/// commit-family entry for it ([`is_origin_subject`]). Bare entries are
/// skipped; a prunable worktree or one whose directory is missing is listed
/// as unavailable.
///
/// # Errors
///
/// Returns the errors of [`head_reflog`].
pub fn find_origins(
    repo: &Repo,
    worktrees: &[Worktree],
    commits: &[ObjectId],
) -> Result<Origins, GitError> {
    let wanted: BTreeSet<&ObjectId> = commits.iter().collect();
    let mut found: BTreeMap<ObjectId, BTreeSet<PathBuf>> = BTreeMap::new();
    let mut unavailable = Vec::new();
    for worktree in worktrees.iter().filter(|w| !w.bare) {
        if worktree.prunable || !worktree.path.is_dir() {
            unavailable.push(UnavailableOrigin {
                worktree: worktree.path.clone(),
                reason: "worktree directory is missing".to_owned(),
            });
            continue;
        }
        for entry in head_reflog(repo, &worktree.path)? {
            if wanted.contains(&entry.commit) && is_origin_subject(&entry.subject) {
                found
                    .entry(entry.commit)
                    .or_default()
                    .insert(worktree.path.clone());
            }
        }
    }
    let mut origins = Origins {
        unavailable,
        ..Origins::default()
    };
    let mut seen = BTreeSet::new();
    for commit in commits.iter().filter(|c| seen.insert(*c)) {
        match found.get(commit) {
            Some(worktrees) => origins.commits.push(CommitOrigin {
                commit: commit.clone(),
                worktrees: worktrees.iter().cloned().collect(),
            }),
            None => origins.without_origin.push(commit.clone()),
        }
    }
    Ok(origins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_subjects_follow_the_grammar() {
        let cases = [
            ("commit: add greet", true),
            ("commit (initial): init", true),
            ("commit (amend): add greet", true),
            ("commit (merge): Merge branch 'x'", true),
            ("commit: Fast-forward handling fix", true),
            ("commit: fast-forward", true),
            ("am: add greet", true),
            ("cherry-pick: add greet", true),
            ("cherry-pick: fast-forward", false),
            ("revert: Revert \"add greet\"", true),
            ("revert: fast-forward", false),
            ("rebase (pick): add greet", true),
            ("rebase -i (pick): add greet", true),
            ("rebase (reword): add greet", true),
            ("rebase (edit): add greet", true),
            ("rebase (squash): add greet", true),
            ("rebase (fixup): add greet", true),
            ("rebase (continue): add greet", true),
            ("rebase (merge): Merge branch 'x'", true),
            ("rebase (pick): fast-forward", false),
            ("pull --rebase origin main (pick): local", true),
            ("pull -q --rebase origin main (pick): local", true),
            ("merge feat: Merge made by the 'ort' strategy.", true),
            ("merge feat: Fast-forward", false),
            (
                "pull -q --no-rebase --no-edit origin main: Merge made by the 'ort' strategy.",
                true,
            ),
            (
                "pull origin main:main: Merge made by the 'ort' strategy.",
                true,
            ),
            ("pull: Fast-forward", false),
            ("pull origin main: Fast-forward", false),
            ("rebase (start): checkout main", false),
            ("rebase (finish): returning to refs/heads/feat", false),
            ("rebase: fast-forward", false),
            ("checkout: moving from main to feat", false),
            ("reset: moving to HEAD~1", false),
            ("branch: Created from HEAD", false),
            ("commit", false),
            ("", false),
        ];
        for (subject, expected) in cases {
            assert_eq!(is_origin_subject(subject), expected, "{subject:?}");
        }
    }
}
```

- [ ] **Step 4: Declare the module and finish the crate documentation**

In `crates/supersigil-git/src/lib.rs`, add `pub mod origin;` after `pub mod oid;`, and replace the crate documentation at the top of the file (every line starting with `//!`) with:

```rust
//! Git access for supersigil review, through the git CLI.
//!
//! [`Repo`] opens a worktree and [`resolve_range`] resolves the base and
//! target of a review. The working-tree target is written by
//! [`snapshot::snapshot_working_tree`] through a temporary index, so it is the
//! tree `git add -u` would produce, and nothing here ever writes the real
//! index, the worktree, or a ref. [`changes`] lists changed paths,
//! [`bytes`] reads blob contents and their worktree form, [`worktree`] lists
//! worktrees, and [`origin`] reads which worktree created a commit.
//!
//! Git runs only through [`Git`], which scrubs the variables that could point
//! it at another repository or index and honors the user's configuration.
```

The finished `crates/supersigil-git/src/lib.rs`:

```rust
//! Git access for supersigil review, through the git CLI.
//!
//! [`Repo`] opens a worktree and [`resolve_range`] resolves the base and
//! target of a review. The working-tree target is written by
//! [`snapshot::snapshot_working_tree`] through a temporary index, so it is the
//! tree `git add -u` would produce, and nothing here ever writes the real
//! index, the worktree, or a ref. [`changes`] lists changed paths,
//! [`bytes`] reads blob contents and their worktree form, [`worktree`] lists
//! worktrees, and [`origin`] reads which worktree created a commit.
//!
//! Git runs only through [`Git`], which scrubs the variables that could point
//! it at another repository or index and honors the user's configuration.

pub mod bytes;
pub mod changes;
pub mod error;
pub mod oid;
pub mod origin;
pub mod path;
pub mod repo;
pub mod run;
pub mod snapshot;
pub mod worktree;

pub use error::GitError;
pub use oid::{ObjectFormat, ObjectId};
pub use path::RepoPath;
pub use repo::{Ancestry, Base, Repo, ResolvedRange, ResolvedTarget, TargetSpec, resolve_range};
pub use run::{Git, GitVersion, MIN_VERSION};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-git`
Expected: PASS, 72 tests in the crate (11 new in `tests/origin.rs`, 1 new unit test in `origin`); 69 on Windows, where three tests need symlinks, hook scripts, or `false` as a filter and are `cfg(unix)`.

- [ ] **Step 6: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; all tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/supersigil-git/src/lib.rs crates/supersigil-git/src/origin.rs crates/supersigil-git/tests/origin.rs
git commit -m "feat: find which worktree created a commit"
```

---

---

### Task 8: Anchor steps: reversal, forward validation, and alignment

This task builds the lowest layer of attribution: what one recorded edit can
tell us on its own. Design section 3 is the spec; the rules that matter most
here:

- Reversal proposes candidate before-states; a candidate is accepted only when
  the current bytes match the edit's after-hash (when known), the candidate
  matches its before-hash (when known), and executing the edit forward on the
  candidate with the recorder's semantics (first occurrence; every
  non-overlapping occurrence for `replace_all`; a Write's result is its
  content, as `supersigil-session`'s `claude_code/content.rs` applies them)
  reproduces the current bytes exactly.
- `structuredPatch` hunk text is never read as bytes: Claude Code shows every
  tab in it as two spaces. Only the byte-faithful `old_text`/`new_text` and
  Write content become bytes. A retained patch is instead a consistency check
  that can only reject; this replaces the earlier rule that hunk line
  numbers never exclude a candidate. The evidence: over every hunk in the
  local corpus that has an `originalFile` (274 of 274), the hunk's old side
  (context and `-` lines) equals the original file's lines from `old_start`
  once each tab is replaced by two spaces; the identity and four-space
  readings fail on the 32 tab-indented hunks. So a candidate before-state is
  rejected when, for any hunk, its lines at `old_start` (1-based; for an
  empty side the line before it, as in a unified diff) differ from the old
  side under that display rule, and the step stops with
  `StopReason::AfterPatchMismatch` when the current bytes' lines at
  `new_start` differ from the new side (context and `+` lines). Line
  terminators and a trailing `\r` are dropped on both sides, and
  `\ No newline at end of file` markers are skipped.
  - It survives displaced displays. jsdiff sometimes marks a different run
    of identical lines as removed than the edit removed: ten `x` lines and
    `UNIQUE` deleted from a file where ten more `x` lines follow shows
    `UNIQUE` and the later ten as removed. Which lines carry `-` is display
    only; the old side as a whole is still the before-file's lines in order,
    and the new side the after-file's, so the check compares whole sides and
    the true candidate passes.
  - It can only lose completeness, never create a false claim. It proposes
    nothing and changes no candidate; it only removes candidates and steps,
    and every accepted step still rests on forward execution and the hashes.
    A patch that misdescribed the file (a display rule not seen in the
    corpus, bytes that are not UTF-8) would reject the true candidate and
    end the chain earlier: less attribution, never wrong attribution.
  - An Edit's candidate is checked through the lines around its splice
    before it is built, so a rejected candidate costs those lines and the
    hunk text, not the whole file. This ends the hashless-deletion blowup,
    where every line start of a large file was a candidate at whole-file
    cost and the budget ran out.
  - Hunk line numbers still order candidate positions, nearest first.
- Alignment comes from the byte ranges forward execution replaced, never from
  a diff. A line is kept only when its whole span lies outside the replaced
  ranges and its image on the other side is a whole line; whole context lines
  common to the start or end of a replacement are kept too. When a
  replacement's prefix and suffix context overlap, its prefix-first and
  suffix-first mappings are two choices, made independently for each
  replacement of the step (a `replace_all` has one per occurrence): `align`
  returns an iterator that generates the combinations lazily, and the walk
  (Task 9) charges each to the budget before using it.
- Every candidate is charged to a byte budget; a refused charge marks the
  budget exhausted, which Task 9 turns into `PathStatus::SearchIncomplete`.

Conservative readings chosen where the design is silent:

- `State::matches` returns `Some(false)` when the record says the file was
  present (content unknown) but the state is absent: presence itself was
  recorded.
- A deletion (empty `new_text`) is reversible only when `old_text` ends with a
  line terminator; its candidates are every line start plus the end of the
  bytes when that is a line boundary (a deleted last line).
- `Reversed::Candidates` is empty only when the budget ran out; callers check
  `Budget::exhausted`.
- A side's line count comes from the hunk's lines, not `old_lines` or
  `new_lines`; an empty side only requires its position to exist; a hunk
  line starting with anything but a space, `-`, or `+` is skipped like the
  marker. A replace-all's joint candidate that contradicts the patch is
  `ReplaceAllUnverified`, and a Write's is `NoAcceptedCandidate`, as when
  their other checks fail.

**Files:**
- Modify: `crates/supersigil-anchor/Cargo.toml` (dependencies; the crate was scaffolded in Task 2)
- Replace: `crates/supersigil-anchor/src/lib.rs`
- Create: `crates/supersigil-anchor/src/lines.rs`, `crates/supersigil-anchor/src/input.rs`, `crates/supersigil-anchor/src/result.rs`,
  `crates/supersigil-anchor/src/step.rs`, `crates/supersigil-anchor/src/align.rs`
- Create: `crates/supersigil-anchor/tests/common/mod.rs`, `crates/supersigil-anchor/tests/step.rs`, `crates/supersigil-anchor/tests/align.rs`

**Interfaces:**
- Consumes: `supersigil_record::observations::{Edit, EditOperation, FileState,
  Content, Material, Hunk}` (Task 1 added `EditOperation` and `Edit::operation`);
  `supersigil_record::{ContentId, EventId, RecordId}`.
- Produces (exactly as the interfaces contract lists them):
  `lines::{line_starts, split_lines, line_of}`;
  `input::{State, CandidateEdit, TargetKind, Request, DEFAULT_BUDGET_BYTES}`
  with `State::{bytes, content_id, matches, byte_len}`;
  `result::StopReason`, with one variant added to the contract's,
  `AfterPatchMismatch { edit }`;
  `step::{Replacement, Forward, execute_forward, Reversal, Reversed, Budget,
  reverse}`; `align::{Region, Alignment, Alignments, align}`, where
  `align(before, after, replacements) -> Alignments` and
  `Alignments: Iterator<Item = Alignment>` (a change from the contract's
  `Vec<Alignment>`, required by the independent per-replacement choices).
  Test support in `tests/common/mod.rs`: the contract's `WT`, `replace`,
  `replace_all`, `create`, `overwrite`, `unknown_op`, `with_hashes`,
  `in_worktree`, `candidate`, `request`, plus `state(Option<&str>) -> State`
  (an addition: a present state or `Absent`). The contract's
  `with_hunk_hint`, which fabricated a hunk without lines, is replaced by
  `with_patch(edit, before, after)`, which builds the hunk jsdiff would show
  for that change: three lines of context, tabs displayed as two spaces.

- [ ] **Step 1: Set the crate's dependencies**

`crates/supersigil-anchor/Cargo.toml` must read:

```toml
[package]
name = "supersigil-anchor"
version = "0.14.0"
description = "Attribution of reviewed changes to recorded agent edits for supersigil review"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[dependencies]
serde.workspace = true
supersigil-record.workspace = true

[dev-dependencies]
proptest.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write the shared test helpers**

`crates/supersigil-anchor/tests/common/mod.rs` (each test binary uses a subset, hence the
reasoned `allow`):

```rust
//! Builders for edits and requests shared by the anchor tests.
#![allow(
    dead_code,
    reason = "each test binary uses a different subset of these helpers"
)]

use std::path::PathBuf;

use supersigil_anchor::{CandidateEdit, DEFAULT_BUDGET_BYTES, Request, State, TargetKind};
use supersigil_record::observations::{Edit, EditOperation, FileState, Hunk, Material};
use supersigil_record::{ContentId, EventId, RecordId, SessionId, Timestamp, TurnId};

/// The reviewed worktree in tests.
pub const WT: &str = "/work/repo";

#[expect(
    clippy::too_many_arguments,
    reason = "a private builder behind the named helpers below"
)]
fn edit(
    id: &str,
    transcript: &str,
    ordinal: u64,
    operation: EditOperation,
    old: Material<String>,
    new: Material<String>,
    replace_all: bool,
    before: FileState,
    after: FileState,
) -> Edit {
    Edit {
        id: EventId::new(id),
        turn: TurnId::new(format!("turn-{id}")),
        session: SessionId::new("s1"),
        path: PathBuf::from("f.txt"),
        before,
        after,
        patch: Material::unavailable("test"),
        old_text: old,
        new_text: new,
        replace_all,
        operation,
        checkout: PathBuf::from(WT),
        time: Timestamp::new("2026-09-29T10:00:00.000Z"),
        source_ordinal: ordinal,
        agent_id: None,
        transcript: Some(transcript.to_owned()),
    }
}

fn text(value: &str) -> Material<String> {
    Material::Retained(value.to_owned())
}

fn known(content: &str) -> FileState {
    FileState::known(ContentId::of(content.as_bytes()))
}

/// An Edit-tool replacement with unknown before- and after-states.
pub fn replace(id: &str, transcript: &str, ordinal: u64, old: &str, new: &str) -> Edit {
    edit(
        id,
        transcript,
        ordinal,
        EditOperation::Replace,
        text(old),
        text(new),
        false,
        FileState::unknown(),
        FileState::unknown(),
    )
}

/// An Edit-tool replacement of every occurrence, with unknown states.
pub fn replace_all(id: &str, transcript: &str, ordinal: u64, old: &str, new: &str) -> Edit {
    let mut e = replace(id, transcript, ordinal, old, new);
    e.replace_all = true;
    e
}

/// A Write that creates the file: before absent, after known.
pub fn create(id: &str, transcript: &str, ordinal: u64, content: &str) -> Edit {
    edit(
        id,
        transcript,
        ordinal,
        EditOperation::Write,
        Material::unavailable("write replaces the whole file"),
        text(content),
        false,
        FileState::Absent,
        known(content),
    )
}

/// A Write that overwrites the file: before unknown, after known.
pub fn overwrite(id: &str, transcript: &str, ordinal: u64, content: &str) -> Edit {
    edit(
        id,
        transcript,
        ordinal,
        EditOperation::Write,
        Material::unavailable("write replaces the whole file"),
        text(content),
        false,
        FileState::unknown(),
        known(content),
    )
}

/// A replacement whose operation was not recorded.
pub fn unknown_op(id: &str, transcript: &str, ordinal: u64, old: &str, new: &str) -> Edit {
    let mut e = replace(id, transcript, ordinal, old, new);
    e.operation = EditOperation::Unknown;
    e
}

/// Sets both recorded hashes from the given contents.
pub fn with_hashes(mut edit: Edit, before: &str, after: &str) -> Edit {
    edit.before = known(before);
    edit.after = known(after);
    edit
}

/// Retains the patch Claude Code records for an edit from `before` to
/// `after`, built as jsdiff builds it: one hunk around the lines between the
/// common leading and trailing lines, with three lines of context, line
/// terminators dropped, a marker after a last line that has none, and every
/// tab displayed as two spaces. Identical texts give no hunk.
pub fn with_patch(mut edit: Edit, before: &str, after: &str) -> Edit {
    edit.patch = Material::Retained(patch(before, after));
    edit
}

fn patch(before: &str, after: &str) -> Vec<Hunk> {
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    let limit = old.len().min(new.len());
    let prefix = (0..limit).take_while(|&k| old[k] == new[k]).count();
    if prefix == old.len() && prefix == new.len() {
        return Vec::new();
    }
    let suffix = (0..limit - prefix)
        .take_while(|&k| old[old.len() - 1 - k] == new[new.len() - 1 - k])
        .count();
    let start = prefix.saturating_sub(3);
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
    let context_end = (old_end + 3).min(old.len());
    let mut lines = Vec::new();
    let mut show = |sign: char, line: &str| {
        let text = line.strip_suffix('\n').unwrap_or(line);
        lines.push(format!("{sign}{}", text.replace('\t', "  ")));
        if !line.ends_with('\n') {
            lines.push("\\ No newline at end of file".to_owned());
        }
    };
    old[start..prefix].iter().for_each(|l| show(' ', l));
    old[prefix..old_end].iter().for_each(|l| show('-', l));
    new[prefix..new_end].iter().for_each(|l| show('+', l));
    old[old_end..context_end].iter().for_each(|l| show(' ', l));
    // Unified-diff convention: an empty range starts at the line before it.
    let range = |count: usize| {
        let first = if count == 0 { start } else { start + 1 };
        (u32::try_from(first).unwrap(), u32::try_from(count).unwrap())
    };
    let (old_start, old_lines) = range(context_end - start);
    let (new_start, new_lines) = range(context_end - old_end + new_end - start);
    vec![Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        lines,
    }]
}

/// Offers `edit` from record `r1` in `worktree`.
pub fn in_worktree(edit: Edit, worktree: &str) -> CandidateEdit {
    CandidateEdit {
        record: RecordId::new("r1"),
        worktree: PathBuf::from(worktree),
        edit,
    }
}

/// Offers `edit` from record `r1` in the reviewed worktree.
pub fn candidate(edit: Edit) -> CandidateEdit {
    in_worktree(edit, WT)
}

/// A present state holding `text`, or absent for `None`.
pub fn state(text: Option<&str>) -> State {
    text.map_or(State::Absent, |t| State::Present(t.as_bytes().to_vec()))
}

/// A request with the default budget, reviewing [`WT`].
pub fn request(
    base: Option<&str>,
    target: Option<&str>,
    kind: TargetKind,
    edits: Vec<CandidateEdit>,
) -> Request {
    Request {
        base: state(base),
        target: state(target),
        target_kind: kind,
        reviewed_worktree: PathBuf::from(WT),
        edits,
        budget_bytes: DEFAULT_BUDGET_BYTES,
    }
}
```

- [ ] **Step 3: Write the failing step tests**

`crates/supersigil-anchor/tests/step.rs`. `tab_indented_hunks_are_never_bytes` and
`deletion_displayed_late_is_recovered` are the Review Focus tests this task
owns; `a_later_occurrence_the_edit_could_not_have_produced_is_rejected` and
`a_joint_inverse_that_fails_its_checks_is_unverified` pin the
forward-execution counterexamples from the design review. The patch check
has its own regressions: the shifted display keeps only the true candidate
(`a_shifted_deletion_display_keeps_only_the_candidate_the_patch_describes`),
a hashless deletion in a 3,000-line file has one candidate within the
default budget, a candidate contradicting the old side is rejected while
without a patch every candidate is kept, current bytes contradicting the
new side stop the step, a tab-indented file's two-space display picks the
true candidate, and markers and carriage returns are not compared.

```rust
//! Reversing one edit: candidates per operation, forward validation, and
//! the retained patch as a consistency check.

mod common;

use common::{create, overwrite, replace, replace_all, state, unknown_op, with_hashes, with_patch};
use supersigil_anchor::result::StopReason;
use supersigil_anchor::step::{Budget, Replacement, Reversed, execute_forward, reverse};
use supersigil_anchor::{DEFAULT_BUDGET_BYTES, State};
use supersigil_record::EventId;
use supersigil_record::observations::{Hunk, Material};

fn befores(reversed: &Reversed) -> Vec<String> {
    match reversed {
        Reversed::Candidates(all) => all
            .iter()
            .map(|r| String::from_utf8(r.before.bytes().unwrap_or_default().to_vec()).unwrap())
            .collect(),
        Reversed::Stop(reason) => panic!("stopped: {reason:?}"),
    }
}

fn run(
    edit: &supersigil_record::observations::Edit,
    current: &str,
    base: Option<&str>,
) -> Reversed {
    reverse(
        edit,
        &state(Some(current)),
        &state(base),
        &mut Budget::new(1 << 20),
    )
}

fn id(name: &str) -> EventId {
    EventId::new(name)
}

fn location_choices(reversed: &Reversed) -> Vec<bool> {
    match reversed {
        Reversed::Candidates(all) => all.iter().map(|r| r.location_choice).collect(),
        Reversed::Stop(reason) => panic!("stopped: {reason:?}"),
    }
}

#[test]
fn forward_replaces_the_first_occurrence() {
    let e = replace("e1", "t", 1, "x", "y");
    let forward = execute_forward(&e, &state(Some("x\nx\n"))).unwrap();
    assert_eq!(forward.after, state(Some("y\nx\n")));
    assert_eq!(
        forward.replacements,
        vec![Replacement {
            before: 0..1,
            after: 0..1
        }]
    );
}

#[test]
fn forward_replace_all_replaces_non_overlapping_occurrences_left_to_right() {
    let e = replace_all("e1", "t", 1, "a", "aa");
    let forward = execute_forward(&e, &state(Some("aba"))).unwrap();
    assert_eq!(forward.after, state(Some("aabaa")));
    assert_eq!(
        forward.replacements,
        vec![
            Replacement {
                before: 0..1,
                after: 0..2
            },
            Replacement {
                before: 2..3,
                after: 3..5
            },
        ]
    );
}

#[test]
fn forward_write_yields_its_content_and_spans_both_files() {
    let e = overwrite("w", "t", 1, "new\n");
    let forward = execute_forward(&e, &state(Some("old!\n"))).unwrap();
    assert_eq!(forward.after, state(Some("new\n")));
    assert_eq!(
        forward.replacements,
        vec![Replacement {
            before: 0..5,
            after: 0..4
        }]
    );
    assert!(execute_forward(&create("c", "t", 1, "a\n"), &State::Absent).is_some());
}

#[test]
fn forward_does_not_execute_unknown_operations_or_missing_text() {
    assert!(execute_forward(&unknown_op("u", "t", 1, "x", "y"), &state(Some("x"))).is_none());
    let mut e = replace("e", "t", 1, "x", "y");
    e.old_text = Material::unavailable("withheld");
    assert!(execute_forward(&e, &state(Some("x"))).is_none());
    assert!(execute_forward(&replace("e", "t", 1, "z", "y"), &state(Some("x"))).is_none());
}

#[test]
fn a_unique_occurrence_reverses_to_one_candidate() {
    let e = replace("e1", "t", 1, "hello {name}", "Hello, {name}!");
    let out = run(&e, "fn a() { Hello, {name}! }\n", None);
    assert_eq!(befores(&out), vec!["fn a() { hello {name} }\n"]);
    let Reversed::Candidates(all) = out else {
        unreachable!()
    };
    assert!(!all[0].location_choice);
}

#[test]
fn a_later_occurrence_the_edit_could_not_have_produced_is_rejected() {
    // Base "x\nx\n", an ordinary edit x -> y, target "x\ny\n": reversing the
    // only "y" gives the base, but the edit replaces the first "x".
    let e = replace("e1", "t", 1, "x", "y");
    assert_eq!(
        run(&e, "x\ny\n", Some("x\nx\n")),
        Reversed::Stop(StopReason::NoAcceptedCandidate { edit: id("e1") })
    );
}

#[test]
fn several_accepted_positions_are_a_location_choice() {
    let e = replace("e1", "t", 1, "x\n", "a\n");
    let out = run(&e, "a\na\n", None);
    assert_eq!(befores(&out), vec!["x\na\n", "a\nx\n"]);
    let Reversed::Candidates(all) = out else {
        unreachable!()
    };
    assert!(all.iter().all(|r| r.location_choice));
}

#[test]
fn a_known_before_hash_picks_among_positions() {
    let e = with_hashes(replace("e1", "t", 1, "x\n", "a\n"), "a\nx\n", "a\na\n");
    let out = run(&e, "a\na\n", None);
    assert_eq!(befores(&out), vec!["a\nx\n"]);
    let Reversed::Candidates(all) = out else {
        unreachable!()
    };
    assert!(!all[0].location_choice);
}

#[test]
fn a_contradicted_after_hash_stops_the_step() {
    let e = with_hashes(replace("e1", "t", 1, "x", "y"), "x\n", "y\n");
    assert_eq!(
        run(&e, "z\n", None),
        Reversed::Stop(StopReason::AfterHashMismatch { edit: id("e1") })
    );
}

#[test]
fn tab_indented_hunks_are_never_bytes() {
    // Claude Code's structuredPatch shows every tab as two spaces; the hunk
    // below is in that display form. Reversal takes bytes from the texts
    // only; the hunk is only checked against them.
    let before = "fn f() {\n\tif x {\n\t\treturn false;\n\t}\n}\n";
    let after = "fn f() {\n\tif x {\n\t\treturn true;\n\t}\n}\n";
    let mut e = replace("e1", "t", 1, "\t\treturn false;\n", "\t\treturn true;\n");
    e.patch = Material::Retained(vec![Hunk {
        old_start: 2,
        old_lines: 3,
        new_start: 2,
        new_lines: 3,
        lines: vec![
            "   if x {".to_owned(),
            "-    return false;".to_owned(),
            "+    return true;".to_owned(),
            "   }".to_owned(),
        ],
    }]);
    assert_eq!(befores(&run(&e, after, None)), vec![before]);
}

#[test]
fn deletion_displayed_late_is_recovered() {
    // The deleted block begins with a line identical to the one before it,
    // so jsdiff may mark the removal a line late, and the texts alone allow
    // the block before or after that line (and at every other line start).
    // Whichever lines the patch marks, its old side is the base's lines.
    let base = "}\n\n// ---\n// ---\n// Exit\n// ---\n\nenum E;\n\n// Symbols\n";
    let block = "// ---\n// Exit\n// ---\n\nenum E;\n\n";
    let target = "}\n\n// ---\n// Symbols\n";
    let unpatched = replace("d", "t", 1, block, "");
    assert_eq!(befores(&run(&unpatched, target, None)).len(), 5);
    let unknown = with_patch(unpatched, base, target);
    assert_eq!(befores(&run(&unknown, target, None)), vec![base]);
    let known = with_patch(
        with_hashes(replace("d", "t", 1, block, ""), base, target),
        base,
        target,
    );
    assert_eq!(befores(&run(&known, target, None)), vec![base]);
}

#[test]
fn a_shifted_deletion_display_keeps_only_the_candidate_the_patch_describes() {
    // Ten "x" lines and UNIQUE are deleted, and ten more "x" lines follow.
    // jsdiff keeps the first ten "x" lines as common and shows UNIQUE and
    // the ten after it as deleted: the display is shifted, but its old side
    // is still the before-file's lines at old_start.
    let x10 = "x\n".repeat(10);
    let block = format!("{x10}UNIQUE\n");
    let before = format!("head\n{block}{x10}tail\n");
    let after = format!("head\n{x10}tail\n");
    let unpatched = replace("d", "t", 1, &block, "");
    assert_eq!(befores(&run(&unpatched, &after, None)).len(), 13);
    let e = with_patch(unpatched, &before, &after);
    let hunk = &e.patch.retained().unwrap()[0];
    assert_eq!((hunk.old_start, hunk.lines[3].as_str()), (9, "-UNIQUE"));
    let out = run(&e, &after, None);
    assert_eq!(befores(&out), vec![before]);
    assert_eq!(location_choices(&out), vec![false]);
}

#[test]
fn a_hashless_deletion_in_a_large_file_has_one_candidate_within_the_budget() {
    // Without the patch every line start of 3,000 lines is a candidate, each
    // costing the whole file, and the default budget runs out.
    let lines: Vec<String> = (0..3_000).map(|i| format!("line {i:04}\n")).collect();
    let before = lines.concat();
    let after = [&lines[..1_500], &lines[1_501..]].concat().concat();
    let e = with_patch(replace("d", "t", 1, &lines[1_500], ""), &before, &after);
    let mut budget = Budget::new(DEFAULT_BUDGET_BYTES);
    let out = reverse(&e, &state(Some(&after)), &State::Absent, &mut budget);
    assert!(!budget.exhausted());
    assert_eq!(befores(&out), vec![before]);
}

#[test]
fn a_candidate_contradicting_the_patch_is_rejected() {
    let e = replace("e1", "t", 1, "x\n", "a\n");
    // Without a retained patch, every candidate is kept.
    assert_eq!(
        befores(&run(&e, "a\nb\na\n", None)),
        vec!["x\nb\na\n", "a\nb\nx\n"]
    );
    let patched = with_patch(e, "a\nb\nx\n", "a\nb\na\n");
    let out = run(&patched, "a\nb\na\n", None);
    assert_eq!(befores(&out), vec!["a\nb\nx\n"]);
    assert_eq!(location_choices(&out), vec![false]);
}

#[test]
fn current_bytes_contradicting_the_patch_stop_the_step() {
    let mut e = with_patch(
        replace("e1", "t", 1, "x\n", "a\n"),
        "a\nb\nx\n",
        "a\nb\na\n",
    );
    assert_eq!(
        run(&e, "a\nc\na\n", None),
        Reversed::Stop(StopReason::AfterPatchMismatch { edit: id("e1") })
    );
    e.patch = Material::unavailable("test");
    assert_eq!(befores(&run(&e, "a\nc\na\n", None)).len(), 2);
}

#[test]
fn a_tab_indented_patch_in_display_form_picks_the_true_candidate() {
    // Both "2;\n" positions reverse; the patch, which shows each tab as two
    // spaces, describes only the second.
    let before = "fn f() {\n\tlet a = 1;\n\tlet b = 2;\n}\n";
    let after = "fn f() {\n\tlet a = 1;\n\tlet b = 1;\n}\n";
    let unpatched = replace("e1", "t", 1, "2;\n", "1;\n");
    assert_eq!(befores(&run(&unpatched, after, None)).len(), 2);
    let e = with_patch(unpatched, before, after);
    assert_eq!(e.patch.retained().unwrap()[0].lines[1], "   let a = 1;");
    assert_eq!(befores(&run(&e, after, None)), vec![before]);
}

#[test]
fn patch_markers_and_carriage_returns_are_not_compared() {
    let before = "a\r\nb";
    let after = "a\r\nc";
    let e = with_patch(replace("e1", "t", 1, "b", "c"), before, after);
    let lines = &e.patch.retained().unwrap()[0].lines;
    assert!(lines.contains(&"\\ No newline at end of file".to_owned()));
    assert_eq!(befores(&run(&e, after, None)), vec![before]);
}

#[test]
fn deletion_candidates_are_every_line_start_and_the_end() {
    let e = replace("d", "t", 1, "c\n", "");
    assert_eq!(
        befores(&run(&e, "a\nb\n", None)),
        vec!["c\na\nb\n", "a\nc\nb\n", "a\nb\nc\n"]
    );
}

#[test]
fn a_deletion_without_a_line_terminator_cannot_be_located() {
    let e = replace("d", "t", 1, "abc", "");
    assert_eq!(
        run(&e, "x\n", None),
        Reversed::Stop(StopReason::NotLocatable { edit: id("d") })
    );
}

#[test]
fn replace_all_reverses_jointly_with_a_known_before_hash() {
    let e = with_hashes(replace_all("e1", "t", 1, "a", "b"), "a a\n", "b b\n");
    assert_eq!(befores(&run(&e, "b b\n", None)), vec!["a a\n"]);
}

#[test]
fn replace_all_without_a_before_hash_is_unverified() {
    let e = replace_all("e1", "t", 1, "a", "b");
    assert_eq!(
        run(&e, "b b\n", None),
        Reversed::Stop(StopReason::ReplaceAllUnverified { edit: id("e1") })
    );
}

#[test]
fn a_joint_inverse_that_fails_its_checks_is_unverified() {
    // "ax" with replace_all(x -> aa) gives "aaa"; the joint inverse of the
    // first "aa" gives "xa", which fails the before-hash.
    let spanning = with_hashes(replace_all("e1", "t", 1, "x", "aa"), "ax", "aaa");
    assert_eq!(
        run(&spanning, "aaa", None),
        Reversed::Stop(StopReason::ReplaceAllUnverified { edit: id("e1") })
    );
    // replace_all(a -> aa) never yields "aaa": the inverse "aa" fails
    // forward execution even with a matching before-hash.
    let growing = with_hashes(replace_all("e2", "t", 1, "a", "aa"), "aa", "aaa");
    assert_eq!(
        run(&growing, "aaa", None),
        Reversed::Stop(StopReason::ReplaceAllUnverified { edit: id("e2") })
    );
}

#[test]
fn a_creation_reverses_to_absent_only_when_its_content_is_the_target() {
    let e = create("c", "t", 1, "a\n");
    let Reversed::Candidates(all) = run(&e, "a\n", None) else {
        panic!("creation must reverse")
    };
    assert_eq!(all[0].before, State::Absent);
    assert_eq!(
        run(&e, "b\n", None),
        Reversed::Stop(StopReason::AfterHashMismatch { edit: id("c") })
    );
}

#[test]
fn an_overwrite_reverses_to_the_base_only_when_its_before_hash_is_the_base() {
    let mut e = overwrite("w", "t", 1, "new\n");
    assert_eq!(
        run(&e, "new\n", Some("old\n")),
        Reversed::Stop(StopReason::WholeFileWrite { edit: id("w") })
    );
    e = with_hashes(e, "old\n", "new\n");
    assert_eq!(befores(&run(&e, "new\n", Some("old\n"))), vec!["old\n"]);
    assert_eq!(
        run(&e, "new\n", Some("other\n")),
        Reversed::Stop(StopReason::WholeFileWrite { edit: id("w") })
    );
}

#[test]
fn an_overwrite_whose_output_is_not_the_current_bytes_is_rejected() {
    let mut e = overwrite("w", "t", 1, "c\n");
    e.after = supersigil_record::observations::FileState::unknown();
    assert_eq!(
        run(&e, "d\n", Some("old\n")),
        Reversed::Stop(StopReason::NoAcceptedCandidate { edit: id("w") })
    );
}

#[test]
fn unknown_operations_and_missing_text_stop() {
    assert_eq!(
        run(&unknown_op("u", "t", 1, "x", "y"), "y", None),
        Reversed::Stop(StopReason::OperationUnknown { edit: id("u") })
    );
    let mut e = replace("e", "t", 1, "x", "y");
    e.new_text = Material::unavailable("withheld");
    assert_eq!(
        run(&e, "y", None),
        Reversed::Stop(StopReason::TextUnavailable { edit: id("e") })
    );
}

#[test]
fn an_exhausted_budget_returns_what_was_accepted() {
    let e = replace("d", "t", 1, "c\n", "");
    let mut budget = Budget::new(1);
    let out = reverse(&e, &state(Some("a\nb\n")), &State::Absent, &mut budget);
    assert_eq!(out, Reversed::Candidates(Vec::new()));
    assert!(budget.exhausted());
}
```

- [ ] **Step 4: Write the failing alignment tests**

`crates/supersigil-anchor/tests/align.rs`, including the duplicate-lines, split, join, `foofoo`
deletion, and `x\nx\n` prefix/suffix regressions, and
`context_choices_are_independent_per_replacement_pair` (a replace-all whose
two pairs each have a choice yields all four combinations, the mixed ones
included):

```rust
//! Aligning lines across one edit from the replacement pairs.

mod common;

use common::{create, replace, replace_all, state};
use supersigil_anchor::State;
use supersigil_anchor::align::{Alignment, Region, align};
use supersigil_anchor::step::execute_forward;
use supersigil_record::observations::Edit;

/// Executes `edit` on `before` and aligns the result.
fn aligned(edit: &Edit, before: Option<&str>) -> Vec<Alignment> {
    let before = state(before);
    let forward = execute_forward(edit, &before).expect("the edit executes");
    align(&before, &forward.after, &forward.replacements).collect()
}

fn region(before: std::ops::Range<usize>, after: std::ops::Range<usize>) -> Region {
    Region { before, after }
}

#[test]
fn duplicate_lines_around_a_replacement_follow_the_bytes() {
    // Replacing "A\nX\n" with "A\n" in A X X B: the surviving X is the
    // original third line, whatever a diff would pair it with.
    let e = replace("e", "t", 1, "A\nX\n", "A\n");
    let all = aligned(&e, Some("A\nX\nX\nB\n"));
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kept, vec![(0, 0), (2, 1), (3, 2)]);
    assert_eq!(all[0].regions, vec![region(1..2, 1..1)]);
}

#[test]
fn a_split_line_is_one_region_and_later_lines_stay_kept() {
    let e = replace("e", "t", 1, "x", "x\n");
    let all = aligned(&e, Some("AxZ\nEND\n"));
    assert_eq!(all[0].kept, vec![(1, 2)]);
    assert_eq!(all[0].regions, vec![region(0..1, 0..2)]);
}

#[test]
fn a_joined_line_is_one_region() {
    let e = replace("e", "t", 1, "x\n", "x");
    let all = aligned(&e, Some("Ax\nZ\nEND\n"));
    assert_eq!(all[0].kept, vec![(2, 1)]);
    assert_eq!(all[0].regions, vec![region(0..2, 0..1)]);
}

#[test]
fn a_deletion_that_joins_two_lines_uses_the_bytes_it_removed() {
    // Forward execution removes the first "foo\n", inside "foofoo\n".
    let e = replace("d", "t", 1, "foo\n", "");
    let all = aligned(&e, Some("foofoo\nfoo\nEND\n"));
    assert_eq!(all[0].kept, vec![(2, 1)]);
    assert_eq!(all[0].regions, vec![region(0..2, 0..1)]);
}

#[test]
fn a_deletion_at_a_line_start_changes_no_target_line() {
    let e = replace("d", "t", 1, "b\n", "");
    let all = aligned(&e, Some("a\nb\nc\n"));
    assert_eq!(all[0].kept, vec![(0, 0), (2, 1)]);
    assert_eq!(all[0].regions, vec![region(1..2, 1..1)]);
}

#[test]
fn whole_line_context_inside_a_replacement_is_kept() {
    let e = replace("e", "t", 1, "a\nb\nc\n", "a\nB\nc\n");
    let all = aligned(&e, Some("a\nb\nc\n"));
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kept, vec![(0, 0), (2, 2)]);
    assert_eq!(all[0].regions, vec![region(1..2, 1..2)]);
}

#[test]
fn partial_line_context_is_not_kept() {
    // The replacement starts inside line 0, so its first line is not whole.
    let e = replace("e", "t", 1, "b\nc\n", "B\nc\n");
    let all = aligned(&e, Some("ab\nc\n"));
    assert_eq!(all[0].kept, vec![(1, 1)]);
    assert_eq!(all[0].regions, vec![region(0..1, 0..1)]);
}

#[test]
fn overlapping_prefix_and_suffix_context_yield_two_alignments() {
    let e = replace("e", "t", 1, "x\nx\n", "x\n");
    let all = aligned(&e, Some("x\nx\n"));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].kept, vec![(0, 0)]);
    assert_eq!(all[0].regions, vec![region(1..2, 1..1)]);
    assert_eq!(all[1].kept, vec![(1, 0)]);
    assert_eq!(all[1].regions, vec![region(0..1, 0..0)]);
}

#[test]
fn a_creation_puts_every_line_in_one_region() {
    let e = create("c", "t", 1, "a\nb\n");
    let forward = execute_forward(&e, &State::Absent).unwrap();
    let all: Vec<Alignment> =
        align(&State::Absent, &forward.after, &forward.replacements).collect();
    assert_eq!(all[0].kept, Vec::<(usize, usize)>::new());
    assert_eq!(all[0].regions, vec![region(0..0, 0..2)]);
}

#[test]
fn context_choices_are_independent_per_replacement_pair() {
    // Two occurrences of "x\nx\n" contracted to "x\n": each pair chooses
    // prefix-first or suffix-first on its own, so the mixed choices are
    // alignments too.
    let e = replace_all("e", "t", 1, "x\nx\n", "x\n");
    let kept: Vec<Vec<(usize, usize)>> = aligned(&e, Some("x\nx\nx\nx\n"))
        .into_iter()
        .map(|a| a.kept)
        .collect();
    assert_eq!(
        kept,
        vec![
            vec![(0, 0), (2, 1)],
            vec![(1, 0), (2, 1)],
            vec![(0, 0), (3, 1)],
            vec![(1, 0), (3, 1)],
        ]
    );
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-anchor`
Expected: compile errors, for example ``unresolved import `supersigil_anchor::step` ``
and ``cannot find type `State` in crate `supersigil_anchor` ``.

- [ ] **Step 6: Write the crate root, lines, input, and the first result type**

`crates/supersigil-anchor/src/lib.rs`:

```rust
//! Attributes the lines of a reviewed change to the recorded edits that
//! produced them.
//!
//! Anchor walks backward from a target state through recorded edits. Each
//! step proposes before-states and accepts one only when the recorded hashes
//! (where known), the retained patch (where kept), and forward execution
//! with the recorder's semantics agree ([`step`]). Lines are aligned across
//! a step from the byte ranges forward execution replaced, never from a
//! diff ([`align`]). Anchor is pure: it reads no files and runs no
//! processes.

pub mod align;
pub mod input;
pub mod lines;
pub mod result;
pub mod step;

pub use input::{CandidateEdit, DEFAULT_BUDGET_BYTES, Request, State, TargetKind};
pub use result::StopReason;
```

`crates/supersigil-anchor/src/lines.rs` (its unit tests pin the line convention every module
shares):

```rust
//! Splits bytes into lines that keep their terminators.
//!
//! A line ends right after `\n`; the last line may lack one. Empty input has
//! no lines. Every other module counts lines this way, so a line index means
//! the same thing to the walk, the alignment, and the review.

/// Returns the byte offset where each line starts.
///
/// `b"a\nb"` has lines starting at `[0, 2]`, `b"a\n"` at `[0]`, and empty
/// input has none.
#[must_use]
pub fn line_starts(bytes: &[u8]) -> Vec<usize> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut starts = vec![0];
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' && i + 1 < bytes.len() {
            starts.push(i + 1);
        }
    }
    starts
}

/// Splits `bytes` into lines, each keeping its terminator.
#[must_use]
pub fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    let starts = line_starts(bytes);
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = starts.get(i + 1).copied().unwrap_or(bytes.len());
            &bytes[start..end]
        })
        .collect()
}

/// Returns the index of the line containing `offset`, given [`line_starts`].
///
/// An offset at or past the end belongs to the last line; with no lines the
/// result is 0.
#[must_use]
pub fn line_of(starts: &[usize], offset: usize) -> usize {
    starts
        .partition_point(|&start| start <= offset)
        .saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_end_after_newlines_and_the_last_may_lack_one() {
        assert_eq!(line_starts(b""), Vec::<usize>::new());
        assert_eq!(line_starts(b"a\n"), vec![0]);
        assert_eq!(line_starts(b"a\nb"), vec![0, 2]);
        assert_eq!(line_starts(b"a\r\nb\n"), vec![0, 3]);
        assert_eq!(
            split_lines(b"a\r\nb\nc"),
            vec![&b"a\r\n"[..], &b"b\n"[..], &b"c"[..]]
        );
    }

    #[test]
    fn offsets_belong_to_the_line_that_contains_them() {
        let starts = line_starts(b"ab\ncd\n");
        assert_eq!(line_of(&starts, 0), 0);
        assert_eq!(line_of(&starts, 2), 0);
        assert_eq!(line_of(&starts, 3), 1);
        assert_eq!(line_of(&starts, 6), 1);
        assert_eq!(line_of(&[], 4), 0);
    }
}
```

`crates/supersigil-anchor/src/input.rs`:

```rust
//! What anchor is asked to explain: a base and a target state of one file,
//! and the recorded edits that may connect them.

use std::path::PathBuf;

use supersigil_record::observations::{Content, Edit, FileState};
use supersigil_record::{ContentId, RecordId};

/// Contents of a file at one point in its history, or its absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The file does not exist.
    Absent,
    /// The file exists with these bytes.
    Present(Vec<u8>),
}

impl State {
    /// Returns the bytes, or `None` when the file is absent.
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Absent => None,
            Self::Present(bytes) => Some(bytes),
        }
    }

    /// Returns the content hash, or `None` when the file is absent.
    #[must_use]
    pub fn content_id(&self) -> Option<ContentId> {
        self.bytes().map(ContentId::of)
    }

    /// Compares this state with a recorded file state.
    ///
    /// Returns `Some(true)` when they agree and `Some(false)` when they
    /// contradict. Returns `None` when both are present and the recorded
    /// content is unknown, since nothing can then be checked. A recorded
    /// present file contradicts an absent state even when its content is
    /// unknown, because presence itself was recorded.
    #[must_use]
    pub fn matches(&self, recorded: &FileState) -> Option<bool> {
        match (self, recorded) {
            (Self::Absent, FileState::Absent) => Some(true),
            (Self::Absent, FileState::Present { .. }) | (Self::Present(_), FileState::Absent) => {
                Some(false)
            }
            (
                Self::Present(bytes),
                FileState::Present {
                    content: Content::Known(id),
                },
            ) => Some(ContentId::of(bytes) == *id),
            (
                Self::Present(_),
                FileState::Present {
                    content: Content::Unknown,
                },
            ) => None,
        }
    }

    /// Returns the number of bytes, 0 when the file is absent.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.bytes().map_or(0, <[u8]>::len)
    }
}

/// A recorded edit offered to anchor, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateEdit {
    /// Record the edit was read from.
    pub record: RecordId,
    /// Root of the worktree the edit's file lies in.
    pub worktree: PathBuf,
    /// The recorded edit.
    pub edit: Edit,
}

/// What kind of snapshot the target state was taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// The working tree, snapshotted after every pinned observation.
    WorkingTree,
    /// A commit, which observations may postdate.
    Commit,
}

/// Default work budget for one path: 64 MiB of bytes processed.
pub const DEFAULT_BUDGET_BYTES: u64 = 64 * 1024 * 1024;

/// One path to attribute.
#[derive(Debug, Clone)]
pub struct Request {
    /// The base state of the path.
    pub base: State,
    /// The target state of the path.
    pub target: State,
    /// Whether the target is a working tree or a commit.
    pub target_kind: TargetKind,
    /// Root of the worktree under review.
    pub reviewed_worktree: PathBuf,
    /// Candidate edits of this path, in record append order.
    pub edits: Vec<CandidateEdit>,
    /// Bytes the search may process before it is marked incomplete.
    pub budget_bytes: u64,
}
```

`crates/supersigil-anchor/src/result.rs` (Tasks 9 and 10 extend it):

```rust
//! Results of attribution: why a reversal or a chain stopped.

use supersigil_record::EventId;

/// Why reversing an edit, or a whole chain, stopped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopReason {
    /// The edit's operation was not recorded.
    OperationUnknown {
        /// The edit.
        edit: EventId,
    },
    /// The replacement texts the reversal needs were not retained.
    TextUnavailable {
        /// The edit.
        edit: EventId,
    },
    /// The edit's replacement text could not be located in the current bytes.
    NotLocatable {
        /// The edit.
        edit: EventId,
    },
    /// A whole-file write whose previous content was not recorded.
    WholeFileWrite {
        /// The edit.
        edit: EventId,
    },
    /// A replace-all edit whose joint inverse could not be verified.
    ReplaceAllUnverified {
        /// The edit.
        edit: EventId,
    },
    /// The current bytes contradict the edit's recorded after-state.
    AfterHashMismatch {
        /// The edit.
        edit: EventId,
    },
    /// The current bytes contradict the lines the edit's retained patch
    /// shows after it.
    AfterPatchMismatch {
        /// The edit.
        edit: EventId,
    },
    /// Every candidate before-state failed its own checks.
    NoAcceptedCandidate {
        /// The edit.
        edit: EventId,
    },
    /// No recorded edit could precede the chain's start.
    NoPredecessor,
}
```

- [ ] **Step 7: Write reversal, forward execution, and the patch check**

`crates/supersigil-anchor/src/step.rs` (its unit tests pin that the lines read around a splice
are the lines of the spliced bytes, and the display rule):

```rust
//! Reverses one recorded edit and validates every candidate by executing
//! the edit forward.
//!
//! Reversal only proposes before-states. A candidate is accepted when the
//! current bytes match the edit's recorded after-hash (when known), the
//! candidate matches its recorded before-hash (when known), and executing
//! the edit forward on the candidate, with the recorder's semantics,
//! reproduces the current bytes exactly.
//!
//! When the edit's `structuredPatch` is retained, it is a consistency check
//! that can only reject. Claude Code shows every tab in it as two spaces, so
//! its text is never bytes; but each hunk's old side (context and `-`
//! lines) is the before-file's lines at `old_start`, and its new side
//! (context and `+` lines) the after-file's lines at `new_start`, once the
//! file's tabs are shown the same way. The current bytes must show every
//! hunk's new side, and a candidate every hunk's old side. Hunk line numbers
//! also order the candidates, nearest first.

use std::ops::Range;

use supersigil_record::observations::{Content, Edit, EditOperation, FileState, Hunk, Material};

use crate::input::State;
use crate::lines::{line_of, line_starts};
use crate::result::StopReason;

/// One replacement an edit made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// Byte range of the replaced text in the before-state.
    pub before: Range<usize>,
    /// Byte range of the replacement text in the after-state.
    pub after: Range<usize>,
}

/// The result of executing an edit forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward {
    /// The after-state.
    pub after: State,
    /// The replacements made, in increasing byte order.
    pub replacements: Vec<Replacement>,
}

/// Executes `edit` on `before` with the recorder's semantics.
///
/// An Edit (`operation: replace`) replaces the first occurrence of its
/// `old_text`, or every non-overlapping occurrence from left to right when
/// `replace_all` is set, as `supersigil-session`'s `claude_code/content.rs`
/// does. It needs both texts retained, a non-empty `old_text`, a present
/// before-state, and at least one occurrence. A Write's result is its
/// retained content whatever the before-state was, and its one replacement
/// spans both whole files. Returns `None` when the edit cannot execute,
/// which includes every `Unknown` operation.
#[must_use]
pub fn execute_forward(edit: &Edit, before: &State) -> Option<Forward> {
    match edit.operation {
        EditOperation::Replace => {
            let old = retained(&edit.old_text)?;
            let new = retained(&edit.new_text)?;
            let bytes = before.bytes()?;
            if old.is_empty() {
                return None;
            }
            let positions = if edit.replace_all {
                non_overlapping(bytes, old)
            } else {
                find(bytes, old, 0).into_iter().collect()
            };
            if positions.is_empty() {
                return None;
            }
            let mut after = Vec::with_capacity(bytes.len() + new.len());
            let mut replacements = Vec::with_capacity(positions.len());
            let mut last = 0;
            for position in positions {
                after.extend_from_slice(&bytes[last..position]);
                let start = after.len();
                after.extend_from_slice(new);
                replacements.push(Replacement {
                    before: position..position + old.len(),
                    after: start..after.len(),
                });
                last = position + old.len();
            }
            after.extend_from_slice(&bytes[last..]);
            Some(Forward {
                after: State::Present(after),
                replacements,
            })
        }
        EditOperation::Write => {
            let content = retained(&edit.new_text)?;
            Some(Forward {
                after: State::Present(content.to_vec()),
                replacements: vec![Replacement {
                    before: 0..before.byte_len(),
                    after: 0..content.len(),
                }],
            })
        }
        EditOperation::Unknown => None,
    }
}

/// An accepted candidate before-state of one edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reversal {
    /// The before-state.
    pub before: State,
    /// The replacements forward execution made from `before`.
    pub replacements: Vec<Replacement>,
    /// Whether this is one of several accepted candidate positions: an
    /// unrecorded choice the walk must branch on.
    pub location_choice: bool,
}

/// What reversing one edit produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reversed {
    /// The accepted candidates, distinct by before-state. Empty only when
    /// the budget ran out; check [`Budget::exhausted`].
    Candidates(Vec<Reversal>),
    /// No candidate could be proposed or accepted, and why.
    Stop(StopReason),
}

/// A work budget measured in bytes processed.
#[derive(Debug, Clone)]
pub struct Budget {
    remaining: u64,
    exhausted: bool,
}

impl Budget {
    /// Creates a budget allowing `bytes` bytes of work.
    #[must_use]
    pub fn new(bytes: u64) -> Self {
        Self {
            remaining: bytes,
            exhausted: false,
        }
    }

    /// Spends `bytes` of the budget. Returns `false`, and marks the budget
    /// exhausted, when not enough remains; nothing is spent then.
    pub fn charge(&mut self, bytes: usize) -> bool {
        let cost = u64::try_from(bytes).unwrap_or(u64::MAX);
        if self.exhausted || cost > self.remaining {
            self.exhausted = true;
            return false;
        }
        self.remaining -= cost;
        true
    }

    /// Returns whether a charge has ever been refused.
    #[must_use]
    pub fn exhausted(&self) -> bool {
        self.exhausted
    }
}

/// Proposes and validates before-states for `edit`, whose after-state is
/// `current`.
///
/// Candidates per operation:
///
/// - Edit: every occurrence of a non-empty `new_text` in `current`. For an
///   empty `new_text` (a deletion), every line start of `current` plus its
///   end when that is a line boundary, and only when `old_text` ends with a
///   line terminator. Hunk line numbers order the candidates, nearest
///   first.
/// - Edit with `replace_all`: one joint candidate replacing every
///   non-overlapping occurrence of `new_text`, only when the before-hash is
///   known.
/// - Write creating a file (recorded before-state absent): the before-state
///   is absent.
/// - Write overwriting a file: the before-state is `base`, only when the
///   recorded before-hash equals `base`'s hash.
///
/// With a retained patch, the step stops when `current` does not show
/// every hunk's new side, and a candidate is rejected when it does not show
/// every hunk's old side, compared as the module documentation describes.
/// An Edit's candidate is checked through the lines around its splice
/// before it is built, so a rejected one costs only those lines and the
/// hunk text.
///
/// Each candidate is charged to `budget`; when a charge is refused, the
/// candidates accepted so far are returned and [`Budget::exhausted`] is set.
#[must_use]
pub fn reverse(edit: &Edit, current: &State, base: &State, budget: &mut Budget) -> Reversed {
    if edit.operation == EditOperation::Unknown {
        return stop(StopReason::OperationUnknown {
            edit: edit.id.clone(),
        });
    }
    if current.matches(&edit.after) == Some(false) {
        return stop(StopReason::AfterHashMismatch {
            edit: edit.id.clone(),
        });
    }
    let patch = edit.patch.retained().map(|hunks| Patch {
        hunks: sides(hunks),
        bytes: hunks.iter().flat_map(|h| &h.lines).map(String::len).sum(),
        current: Whole::new(current.bytes().unwrap_or_default()),
    });
    if let Some(patch) = &patch
        && !patch.shown_after(&patch.current)
    {
        return stop(StopReason::AfterPatchMismatch {
            edit: edit.id.clone(),
        });
    }
    if edit.operation == EditOperation::Write {
        reverse_write(edit, current, base, patch.as_ref(), budget)
    } else {
        reverse_replace(edit, current, patch.as_ref(), budget)
    }
}

/// A retained patch and the lines of the current bytes it is checked with.
struct Patch<'a> {
    hunks: Vec<Sides<'a>>,
    /// Bytes of hunk text, charged for each check of a candidate.
    bytes: usize,
    current: Whole<'a>,
}

/// One hunk's old side (context and `-` lines) and new side (context and
/// `+` lines), without their prefixes.
struct Sides<'a> {
    old_start: u32,
    old: Vec<&'a str>,
    new_start: u32,
    new: Vec<&'a str>,
}

fn sides(hunks: &[Hunk]) -> Vec<Sides<'_>> {
    hunks
        .iter()
        .map(|hunk| {
            let (mut old, mut new) = (Vec::new(), Vec::new());
            for line in &hunk.lines {
                // `\ No newline at end of file` markers are skipped.
                match line.as_bytes().first() {
                    Some(b' ') => {
                        old.push(&line[1..]);
                        new.push(&line[1..]);
                    }
                    Some(b'-') => old.push(&line[1..]),
                    Some(b'+') => new.push(&line[1..]),
                    _ => {}
                }
            }
            Sides {
                old_start: hunk.old_start,
                old,
                new_start: hunk.new_start,
                new,
            }
        })
        .collect()
}

impl Patch<'_> {
    fn shown_before(&self, lines: &impl Lines) -> bool {
        self.hunks
            .iter()
            .all(|hunk| shows(lines, hunk.old_start, &hunk.old))
    }

    fn shown_after(&self, lines: &impl Lines) -> bool {
        self.hunks
            .iter()
            .all(|hunk| shows(lines, hunk.new_start, &hunk.new))
    }
}

/// Whether `lines` shows `shown` from the 1-based line `start`; for an
/// empty side, `start` is the line before it (0 before the first), as in a
/// unified diff. Lines are compared without their terminator and a trailing
/// `\r` on either side, and with every tab of the file shown as two spaces,
/// the way Claude Code displays them.
fn shows(lines: &impl Lines, start: u32, shown: &[&str]) -> bool {
    let Ok(start) = usize::try_from(start) else {
        return false;
    };
    if shown.is_empty() {
        return start <= lines.count();
    }
    let Some(first) = start.checked_sub(1) else {
        return false;
    };
    first + shown.len() <= lines.count()
        && shown
            .iter()
            .enumerate()
            .all(|(k, text)| displays(lines.line(first + k), text))
}

fn displays(line: &[u8], shown: &str) -> bool {
    let mut rest = without_terminator(shown.as_bytes());
    for byte in without_terminator(line) {
        let display: &[u8] = if *byte == b'\t' {
            b"  "
        } else {
            std::slice::from_ref(byte)
        };
        let Some(after) = rest.strip_prefix(display) else {
            return false;
        };
        rest = after;
    }
    rest.is_empty()
}

fn without_terminator(bytes: &[u8]) -> &[u8] {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    bytes.strip_suffix(b"\r").unwrap_or(bytes)
}

/// Lines of a state, each with its terminator.
trait Lines {
    fn count(&self) -> usize;
    fn line(&self, index: usize) -> &[u8];
}

/// The lines of bytes in hand.
struct Whole<'a> {
    bytes: &'a [u8],
    starts: Vec<usize>,
}

impl<'a> Whole<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            starts: line_starts(bytes),
        }
    }
}

impl Lines for Whole<'_> {
    fn count(&self) -> usize {
        self.starts.len()
    }

    fn line(&self, index: usize) -> &[u8] {
        let end = self
            .starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.bytes.len());
        &self.bytes[self.starts[index]..end]
    }
}

/// The lines of `outer` with a byte range replaced, without building them:
/// only the lines the replacement touches are copied.
struct Spliced<'a> {
    outer: &'a Whole<'a>,
    /// Lines of `outer` before the touched ones.
    head: usize,
    /// The touched lines, after the replacement.
    touched: Vec<u8>,
    touched_starts: Vec<usize>,
    /// The first line of `outer` after the touched ones.
    tail: usize,
}

impl<'a> Spliced<'a> {
    fn new(outer: &'a Whole<'a>, at: &Range<usize>, with: &[u8]) -> Self {
        let head = line_of(&outer.starts, at.start);
        let from = outer.starts.get(head).copied().unwrap_or(0);
        // The first line starting after the replaced range: the line holding
        // its end is touched even when the range ends at that line's start.
        let tail = outer.starts.partition_point(|&start| start <= at.end);
        let to = outer.starts.get(tail).copied().unwrap_or(outer.bytes.len());
        let mut touched = Vec::with_capacity(to - from - at.len() + with.len());
        touched.extend_from_slice(&outer.bytes[from..at.start]);
        touched.extend_from_slice(with);
        touched.extend_from_slice(&outer.bytes[at.end..to]);
        let touched_starts = line_starts(&touched);
        Self {
            outer,
            head,
            touched,
            touched_starts,
            tail,
        }
    }
}

impl Lines for Spliced<'_> {
    fn count(&self) -> usize {
        self.head + self.touched_starts.len() + self.outer.count() - self.tail
    }

    fn line(&self, index: usize) -> &[u8] {
        if index < self.head {
            return self.outer.line(index);
        }
        let local = index - self.head;
        match self.touched_starts.get(local) {
            Some(&start) => {
                let end = self
                    .touched_starts
                    .get(local + 1)
                    .copied()
                    .unwrap_or(self.touched.len());
                &self.touched[start..end]
            }
            None => self
                .outer
                .line(local - self.touched_starts.len() + self.tail),
        }
    }
}

fn stop(reason: StopReason) -> Reversed {
    Reversed::Stop(reason)
}

fn retained(material: &Material<String>) -> Option<&[u8]> {
    material.retained().map(String::as_bytes)
}

/// Returns the first position at or after `from` where `needle` occurs.
fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&i| haystack[i..].starts_with(needle))
}

/// Returns the non-overlapping occurrences of `needle`, left to right, as
/// `str::replace` finds them.
fn non_overlapping(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut from = 0;
    while let Some(position) = find(haystack, needle, from) {
        positions.push(position);
        from = position + needle.len();
    }
    positions
}

/// Returns every position where `needle` occurs, overlapping ones included.
fn occurrences(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut from = 0;
    while let Some(position) = find(haystack, needle, from) {
        positions.push(position);
        from = position + 1;
    }
    positions
}

/// Where a deleted block could have been: every line start, plus the end
/// when the bytes are empty or end with a newline.
fn deletion_positions(bytes: &[u8]) -> Vec<usize> {
    let mut positions = line_starts(bytes);
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        positions.push(bytes.len());
    }
    positions
}

/// The 0-based line where the edit's first hunk shows its first change,
/// used only to order candidates.
fn hint_line(edit: &Edit) -> Option<usize> {
    let hunk = edit.patch.retained()?.first()?;
    let context = hunk
        .lines
        .iter()
        .take_while(|line| line.starts_with(' '))
        .count();
    let one_based = usize::try_from(hunk.new_start).ok()? + context;
    Some(one_based.saturating_sub(1))
}

/// Orders positions by line distance from the hint, then by position.
fn order_by_hint(mut positions: Vec<usize>, starts: &[usize], hint: Option<usize>) -> Vec<usize> {
    if let Some(hint) = hint {
        positions.sort_by_key(|&p| (line_of(starts, p).abs_diff(hint), p));
    }
    positions
}

/// Checks a candidate before-state against the recorded before-hash and by
/// forward execution.
fn validate(edit: &Edit, before: State, current: &State) -> Option<Reversal> {
    if before.matches(&edit.before) == Some(false) {
        return None;
    }
    let forward = execute_forward(edit, &before)?;
    (forward.after == *current).then_some(Reversal {
        before,
        replacements: forward.replacements,
        location_choice: false,
    })
}

fn splice(bytes: &[u8], at: Range<usize>, with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() - at.len() + with.len());
    out.extend_from_slice(&bytes[..at.start]);
    out.extend_from_slice(with);
    out.extend_from_slice(&bytes[at.end..]);
    out
}

fn reverse_replace(
    edit: &Edit,
    current: &State,
    patch: Option<&Patch>,
    budget: &mut Budget,
) -> Reversed {
    let id = || edit.id.clone();
    let (Some(old), Some(new)) = (retained(&edit.old_text), retained(&edit.new_text)) else {
        return stop(StopReason::TextUnavailable { edit: id() });
    };
    let Some(bytes) = current.bytes() else {
        return stop(StopReason::NotLocatable { edit: id() });
    };
    if old.is_empty() {
        return stop(StopReason::NotLocatable { edit: id() });
    }
    if edit.replace_all {
        return reverse_replace_all(edit, current, bytes, old, new, patch, budget);
    }
    let positions = if new.is_empty() {
        if !old.ends_with(b"\n") {
            return stop(StopReason::NotLocatable { edit: id() });
        }
        deletion_positions(bytes)
    } else {
        occurrences(bytes, new)
    };
    if positions.is_empty() {
        return stop(StopReason::NotLocatable { edit: id() });
    }
    let positions = match patch {
        Some(patch) => order_by_hint(positions, &patch.current.starts, hint_line(edit)),
        None => positions,
    };
    let mut accepted: Vec<Reversal> = Vec::new();
    for position in positions {
        if let Some(patch) = patch {
            let spliced = Spliced::new(&patch.current, &(position..position + new.len()), old);
            if !budget.charge(spliced.touched.len() + patch.bytes) {
                break;
            }
            if !patch.shown_before(&spliced) {
                continue;
            }
        }
        if !budget.charge(bytes.len() + old.len()) {
            break;
        }
        let before = State::Present(splice(bytes, position..position + new.len(), old));
        if accepted.iter().any(|r| r.before == before) {
            continue;
        }
        if let Some(reversal) = validate(edit, before, current) {
            accepted.push(reversal);
        }
    }
    if accepted.is_empty() && !budget.exhausted() {
        return stop(StopReason::NoAcceptedCandidate { edit: id() });
    }
    let choice = accepted.len() > 1;
    for reversal in &mut accepted {
        reversal.location_choice = choice;
    }
    Reversed::Candidates(accepted)
}

fn reverse_replace_all(
    edit: &Edit,
    current: &State,
    bytes: &[u8],
    old: &[u8],
    new: &[u8],
    patch: Option<&Patch>,
    budget: &mut Budget,
) -> Reversed {
    let unverified = || {
        stop(StopReason::ReplaceAllUnverified {
            edit: edit.id.clone(),
        })
    };
    let known_before = matches!(
        edit.before,
        FileState::Present {
            content: Content::Known(_)
        }
    );
    if !known_before || new.is_empty() {
        return unverified();
    }
    let positions = non_overlapping(bytes, new);
    if positions.is_empty() {
        return stop(StopReason::NotLocatable {
            edit: edit.id.clone(),
        });
    }
    if !budget.charge(2 * bytes.len()) {
        return Reversed::Candidates(Vec::new());
    }
    let mut before = Vec::with_capacity(bytes.len());
    let mut last = 0;
    for position in positions {
        before.extend_from_slice(&bytes[last..position]);
        before.extend_from_slice(old);
        last = position + new.len();
    }
    before.extend_from_slice(&bytes[last..]);
    if patch.is_some_and(|patch| !patch.shown_before(&Whole::new(&before))) {
        return unverified();
    }
    match validate(edit, State::Present(before), current) {
        Some(reversal) => Reversed::Candidates(vec![reversal]),
        None => unverified(),
    }
}

fn reverse_write(
    edit: &Edit,
    current: &State,
    base: &State,
    patch: Option<&Patch>,
    budget: &mut Budget,
) -> Reversed {
    let id = || edit.id.clone();
    let Some(content) = retained(&edit.new_text) else {
        return stop(StopReason::TextUnavailable { edit: id() });
    };
    if !budget.charge(current.byte_len() + base.byte_len()) {
        return Reversed::Candidates(Vec::new());
    }
    if current.bytes() != Some(content) {
        return stop(StopReason::NoAcceptedCandidate { edit: id() });
    }
    let before = match &edit.before {
        FileState::Absent => State::Absent,
        FileState::Present {
            content: Content::Known(recorded),
        } if base.content_id().as_ref() == Some(recorded) => base.clone(),
        FileState::Present { .. } => return stop(StopReason::WholeFileWrite { edit: id() }),
    };
    if patch
        .is_some_and(|patch| !patch.shown_before(&Whole::new(before.bytes().unwrap_or_default())))
    {
        return stop(StopReason::NoAcceptedCandidate { edit: id() });
    }
    match validate(edit, before, current) {
        Some(reversal) => Reversed::Candidates(vec![reversal]),
        None => stop(StopReason::NoAcceptedCandidate { edit: id() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(lines: &impl Lines) -> Vec<&[u8]> {
        (0..lines.count()).map(|i| lines.line(i)).collect()
    }

    #[test]
    fn spliced_lines_are_the_lines_of_the_spliced_bytes() {
        let outers: [&[u8]; 5] = [b"", b"a\n", b"a\nb", b"ab\ncd\nef\n", b"\n\n"];
        let withs: [&[u8]; 5] = [b"", b"x", b"x\n", b"\ny", b"x\ny\n"];
        for outer in outers {
            let whole = Whole::new(outer);
            for start in 0..=outer.len() {
                for end in start..=outer.len() {
                    for with in withs {
                        let spliced = Spliced::new(&whole, &(start..end), with);
                        let built = splice(outer, start..end, with);
                        assert_eq!(
                            all(&spliced),
                            all(&Whole::new(&built)),
                            "{outer:?} {start}..{end} {with:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_line_displays_without_terminators_and_with_tabs_as_two_spaces() {
        assert!(displays(b"\tx\r\n", "  x"));
        assert!(displays(b"a", "a\r"));
        assert!(!displays(b"\tx\n", "\tx"));
        assert!(!displays(b"\tx\n", "    x"));
        assert!(!displays(b"ab\n", "a"));
        assert!(!displays(b"a\n", "ab"));
    }
}
```

- [ ] **Step 8: Write the alignment**

`crates/supersigil-anchor/src/align.rs`:

```rust
//! Aligns the lines of an edit's before- and after-states from the byte
//! ranges forward execution replaced, never from a diff.
//!
//! Bytes outside the replaced ranges are carried unchanged, shifted by the
//! accumulated difference in range lengths. A line is kept when its whole
//! span lies outside the ranges on its side and the image of that span on
//! the other side is a whole line there; kept lines pair with their images.
//! Inside a replacement, whole lines common to the start of the old and new
//! text, and whole lines common to their end, are kept too; when the two
//! overlap, each replacement chooses independently which to prefer. Every
//! other line belongs to a changed region: the lines between two
//! consecutive kept pairs, before lines on one side and after lines on the
//! other.

use std::ops::Range;

use crate::input::State;
use crate::lines::line_starts;
use crate::step::Replacement;

/// Lines of the before-state and after-state that changed together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Line indices in the before-state.
    pub before: Range<usize>,
    /// Line indices in the after-state.
    pub after: Range<usize>,
}

/// A line correspondence between the before- and after-state of one edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment {
    /// Kept lines as (before line, after line), increasing on both sides.
    pub kept: Vec<(usize, usize)>,
    /// Changed regions, in order.
    pub regions: Vec<Region>,
}

/// Aligns `before` and `after` from the replacements forward execution made.
///
/// When the whole-line context common to the start and to the end of a
/// replacement overlaps (for example `x\nx\n` replaced by `x\n`), that
/// replacement has two mappings, prefix-first and suffix-first: equal bytes
/// do not settle which old line survived. Each such replacement chooses
/// independently (a `replace_all` has one replacement per occurrence), so a
/// step with n of them has 2^n alignments. The returned iterator generates
/// them one at a time, every choice prefix-first first; callers charge each
/// to their budget before using it.
#[must_use]
pub fn align(before: &State, after: &State, replacements: &[Replacement]) -> Alignments {
    let old = Side::new(before.bytes().unwrap_or_default());
    let new = Side::new(after.bytes().unwrap_or_default());
    let mut fixed = carried_pairs(&old, &new, replacements);
    let mut choices = Vec::new();
    for replacement in replacements {
        let prefix = context_pairs(&old, &new, replacement, Prefer::Prefix);
        let suffix = context_pairs(&old, &new, replacement, Prefer::Suffix);
        if prefix == suffix {
            fixed.extend(prefix);
        } else {
            choices.push([prefix, suffix]);
        }
    }
    Alignments {
        counter: Some(vec![false; choices.len()]),
        fixed,
        choices,
        old_count: old.count(),
        new_count: new.count(),
    }
}

/// The alignments of one step, generated lazily: one per combination of
/// the independent context choices.
#[derive(Debug, Clone)]
pub struct Alignments {
    /// Kept pairs every alignment shares.
    fixed: Vec<(usize, usize)>,
    /// Per replacement with a choice: its prefix-first and suffix-first pairs.
    choices: Vec<[Vec<(usize, usize)>; 2]>,
    /// The next combination (`true` = suffix-first), `None` when done.
    counter: Option<Vec<bool>>,
    old_count: usize,
    new_count: usize,
}

impl Iterator for Alignments {
    type Item = Alignment;

    fn next(&mut self) -> Option<Alignment> {
        let counter = self.counter.as_mut()?;
        let mut kept = self.fixed.clone();
        for (&suffix, options) in counter.iter().zip(&self.choices) {
            kept.extend(options[usize::from(suffix)].iter().copied());
        }
        kept.sort_unstable();
        // Advance like a binary counter; wrapping around means done.
        let mut carry = true;
        for bit in counter.iter_mut() {
            if !carry {
                break;
            }
            *bit = !*bit;
            carry = !*bit;
        }
        if carry {
            self.counter = None;
        }
        Some(regions(kept, self.old_count, self.new_count))
    }
}

/// The bytes of one state and where its lines start.
struct Side<'a> {
    bytes: &'a [u8],
    starts: Vec<usize>,
}

impl<'a> Side<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            starts: line_starts(bytes),
        }
    }

    fn count(&self) -> usize {
        self.starts.len()
    }

    fn span(&self, line: usize) -> Range<usize> {
        let end = self
            .starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.bytes.len());
        self.starts[line]..end
    }

    fn line_at(&self, offset: usize) -> Option<usize> {
        self.starts.binary_search(&offset).ok()
    }

    /// Whether `range` is exactly one whole line.
    fn whole_line(&self, range: &Range<usize>) -> Option<usize> {
        let line = self.line_at(range.start)?;
        (self.span(line) == *range).then_some(line)
    }
}

/// Maps a before-state offset outside every replaced range to the
/// after-state.
fn carry(offset: usize, replacements: &[Replacement]) -> usize {
    let mut shifted = offset;
    for replacement in replacements {
        if replacement.before.end <= offset {
            shifted = shifted + replacement.after.len() - replacement.before.len();
        }
    }
    shifted
}

fn overlaps(span: &Range<usize>, range: &Range<usize>) -> bool {
    span.start < range.end && range.start < span.end
}

/// Kept pairs of lines lying wholly outside the replaced ranges.
fn carried_pairs(old: &Side, new: &Side, replacements: &[Replacement]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for line in 0..old.count() {
        let span = old.span(line);
        if replacements.iter().any(|r| overlaps(&span, &r.before)) {
            continue;
        }
        let start = carry(span.start, replacements);
        let image = start..start + span.len();
        if let Some(target) = new.whole_line(&image) {
            pairs.push((line, target));
        }
    }
    pairs
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prefer {
    Prefix,
    Suffix,
}

/// The whole lines of `range` in `side`, as (line index, bytes); `None`
/// for a segment that is not a whole line of the file.
fn segments<'a>(side: &Side<'a>, range: &Range<usize>) -> Vec<Option<(usize, &'a [u8])>> {
    let mut out = Vec::new();
    let mut start = range.start;
    while start < range.end {
        let end = side.bytes[start..range.end]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(range.end, |i| start + i + 1);
        let whole = side.whole_line(&(start..end));
        out.push(whole.map(|line| (line, &side.bytes[start..end])));
        start = end;
    }
    out
}

/// Kept pairs from whole lines common to the start and the end of one
/// replacement, taking the preferred side first and the other up to what
/// remains.
fn context_pairs(old: &Side, new: &Side, r: &Replacement, prefer: Prefer) -> Vec<(usize, usize)> {
    let before = segments(old, &r.before);
    let after = segments(new, &r.after);
    let common = |a: &Option<(usize, &[u8])>, b: &Option<(usize, &[u8])>| match (a, b) {
        (Some((x, left)), Some((y, right))) if left == right => Some((*x, *y)),
        _ => None,
    };
    let limit = before.len().min(after.len());
    let prefix_max = (0..limit)
        .take_while(|&k| common(&before[k], &after[k]).is_some())
        .count();
    let suffix_max = (0..limit)
        .take_while(|&k| {
            common(&before[before.len() - 1 - k], &after[after.len() - 1 - k]).is_some()
        })
        .count();
    let (prefix, suffix) = match prefer {
        Prefer::Prefix => (prefix_max, suffix_max.min(limit - prefix_max)),
        Prefer::Suffix => (prefix_max.min(limit - suffix_max), suffix_max),
    };
    let mut pairs: Vec<(usize, usize)> = (0..prefix)
        .filter_map(|k| common(&before[k], &after[k]))
        .collect();
    pairs.extend(
        (0..suffix)
            .rev()
            .filter_map(|k| common(&before[before.len() - 1 - k], &after[after.len() - 1 - k])),
    );
    pairs
}

/// Turns kept pairs into an alignment: the lines between consecutive kept
/// pairs form one changed region.
fn regions(kept: Vec<(usize, usize)>, old_count: usize, new_count: usize) -> Alignment {
    let mut out = Vec::new();
    let (mut before, mut after) = (0, 0);
    for &(b, a) in kept.iter().chain(std::iter::once(&(old_count, new_count))) {
        if b > before || a > after {
            out.push(Region {
                before: before..b,
                after: after..a,
            });
        }
        before = b + 1;
        after = a + 1;
    }
    Alignment { kept, regions: out }
}
```

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-anchor`
Expected: `41 tests run: 41 passed` (2 unit tests in `lines`, 2 in `step`, 27 step tests, 10 alignment tests).

- [ ] **Step 10: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; every test passes.

- [ ] **Step 11: Commit**

```bash
git add crates/supersigil-anchor/Cargo.toml crates/supersigil-anchor/src/lib.rs crates/supersigil-anchor/src/lines.rs crates/supersigil-anchor/src/input.rs crates/supersigil-anchor/src/result.rs crates/supersigil-anchor/src/step.rs crates/supersigil-anchor/src/align.rs crates/supersigil-anchor/tests/common/mod.rs crates/supersigil-anchor/tests/step.rs crates/supersigil-anchor/tests/align.rs Cargo.lock
git commit -m "feat: reverse and align recorded edits"
```

---

### Task 9: Anchor walk: chains from the target back to the base

This task searches for every chain of recorded edits that reproduces the
target, per design section 3 ("The walk", "What a chain establishes",
"Evidence alternatives and unrecorded choices"):

- Deduplication first (the record's event-identity rule): within a record the
  first sighting of an edit id wins; sightings in several records are one edit
  when equal, or when they differ only in `operation: unknown` versus a known
  operation (the known one is kept, every record listed); anything else is a
  `Conflict`, excluded and never resolved by order.
- Edits are grouped by (worktree, transcript); equal ordinals form an
  unordered bucket, and every order of a bucket is tried.
- Heads: for a working-tree target, the last bucket of each group in the
  reviewed worktree, plus any edit of a group in another (originating)
  worktree; for a commit target, any edit.
- At every state both the previous bucket of the current group and every
  verified link (an edit of a group not yet entered whose recorded after-hash
  equals the current bytes' hash) are explored. No links on unknown hashes.
- A chain ends at the first state equal to the base (exact from the base), or
  where no predecessor is accepted (exact from its start when every step has
  both hashes known, consistent otherwise). Time never decides.
- Subsets and orders of a bucket are generated one at a time and never
  materialized. Every generated branch is charged 1 KiB before it is
  explored, every reversal candidate its bytes, and every alignment the
  step's bytes; enumeration stops the moment the budget is exhausted, which
  makes the status `SearchIncomplete`.

**Setting aside alternatives, and a design gap this task resolves.** The
approved rule sets aside, "among siblings of one unrecorded choice that use
exactly the same set of edits", those that do not end at the base while one
does. Implemented literally it contradicts the design's own completeness
property (section 5, property (a)): a wrong deletion position with unknown
hashes is often stopped early by a recorded check further back (an earlier
edit's known after-hash, or a creation's content), so it uses a *subset* of the
edits of the branch that reaches the base, is not set aside, and makes every
line of the file ambiguous; this happens routinely in sessions whose file grew
past 10 KB midway (early edits hashed, later ones not). The "same edits"
clause exists so that a non-base branch holding evidence the base-reaching
branch lacks is never dropped (the review's `[P, D]` case). This task
therefore sets a chain aside when a base-reaching chain entered the same
groups in the same order and used, in each, every edit the first one used:
it holds no recorded edit the other lacks. Every case the design review
constructed keeps its approved outcome (they are tests below and in Task 10),
and the two new tests `a_sibling_stopped_early_by_a_recorded_check_is_set_aside`
and `a_sibling_stopped_early_by_a_creation_is_set_aside` pin the refinement.
The author should confirm it; the literal alternative is to drop creation
Writes and mixed hashes from property (a).

**Files:**
- Modify: `crates/supersigil-anchor/tests/common/mod.rs` (adds `finishes_within`)
- Replace: `crates/supersigil-anchor/src/result.rs` (adds the chain types), `crates/supersigil-anchor/src/lib.rs`
- Create: `crates/supersigil-anchor/src/walk.rs`, `crates/supersigil-anchor/tests/walk.rs`

**Interfaces:**
- Consumes: Task 8's `step::{reverse, Budget, Reversed}`, `align::{align,
  Alignments, Alignment}`, `input::{Request, CandidateEdit, State,
  TargetKind}`, `result::StopReason`.
- Produces: `walk::{AcceptedEdit, dedup, ChainStep, WalkedChain, Walk, walk}`;
  `result::{ChainClass, ChainEnd, Chain, PathStatus, Conflict}`, all exactly as
  the interfaces contract lists them. `Chain::id` is assigned in discovery
  order across `Walk::chains` and `Walk::set_aside`. Test support adds
  `finishes_within(limit, work)`, which turns a hang into a failing test.

- [ ] **Step 1: Add the time-limit helper**

In `crates/supersigil-anchor/tests/common/mod.rs`, add `use std::sync::mpsc;` and
`use std::time::Duration;` after `use std::path::PathBuf;`, and add after
`state`:

```rust
/// Runs `work` on another thread and returns its result, failing the test
/// when it takes longer than `limit`. A hang becomes a failure instead of a
/// stuck test run; the thread is left to the process's exit.
pub fn finishes_within<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(work());
    });
    result
        .recv_timeout(limit)
        .expect("the work finishes within its time limit")
}
```

- [ ] **Step 2: Write the failing walk tests**

`crates/supersigil-anchor/tests/walk.rs`. It pins every walk-level regression the design lists: a
base state inside a segment, a state carried to another worktree
mid-segment, equal ordinals, replayed observations, unknown-versus-known
operations, conflicting sightings, an internal after-hash contradicted by a
manual change, two verified histories, heads that start elsewhere, a verified
step behind an unknown-hash step, the occurrence choice with and without
different edits, the whole sequence versus the deletion alone, an edit
reverted by hand, working-tree versus commit heads, an originating-worktree
head, the budget, and the two set-aside refinements. Three tests pin bounded
work: `a_large_bucket_of_equal_ordinals_ends_search_incomplete_promptly` (25
equal-ordinal edits in a commit target; building the powerset of a bucket
would never finish), `every_alignment_choice_is_charged_to_the_budget`
(2^18 alignment combinations of one step against a 1 MiB budget), and
`a_hashless_deletion_in_a_large_file_ends_exact_from_the_base` (Task 8's
patch check keeps the default budget from running out).

```rust
//! The backward walk: heads, links, buckets, ends, classes, deduplication,
//! the budget, and setting aside alternatives that assume an unrecorded
//! change.

mod common;

use std::fmt::Write as _;
use std::time::Duration;

use common::{
    WT, candidate, create, finishes_within, in_worktree, replace, replace_all, request,
    with_hashes, with_patch,
};
use supersigil_anchor::walk::{Walk, dedup, walk};
use supersigil_anchor::{CandidateEdit, ChainClass, ChainEnd, PathStatus, StopReason, TargetKind};
use supersigil_record::observations::EditOperation;
use supersigil_record::{EventId, RecordId};

fn ids(names: &[&str]) -> Vec<EventId> {
    names.iter().map(|n| EventId::new(*n)).collect()
}

/// (edits oldest first, class, ended at the base) of every chain.
fn summary(walk: &Walk) -> Vec<(Vec<EventId>, ChainClass, bool)> {
    walk.chains
        .iter()
        .map(|w| {
            (
                w.chain.edits.clone(),
                w.chain.class,
                w.chain.end == ChainEnd::Base,
            )
        })
        .collect()
}

fn wt(base: &str, target: &str, edits: Vec<CandidateEdit>) -> Walk {
    walk(&request(
        Some(base),
        Some(target),
        TargetKind::WorkingTree,
        edits,
    ))
}

#[test]
fn a_base_state_inside_a_segment_ends_the_chain() {
    // One session: x -> b (the base) -> t. Reviewing b..t uses only the
    // second edit.
    let out = wt(
        "b\n",
        "t\n",
        vec![
            candidate(replace("e1", "t", 1, "x\n", "b\n")),
            candidate(replace("e2", "t", 2, "b\n", "t\n")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e2"]), ChainClass::ExactFromBase, true)]
    );
    assert_eq!(out.status, PathStatus::Composed);
}

#[test]
fn a_state_carried_to_another_worktree_mid_segment_is_reached_by_a_verified_link() {
    // W1 records A -> S -> X; the reviewed worktree branched at S and
    // records S -> T. W1's later X does not supersede W2's copy of S.
    let w1 = "/work/w1";
    let out = wt(
        "A\n",
        "T\n",
        vec![
            in_worktree(
                with_hashes(replace("a1", "t1", 1, "A\n", "S\n"), "A\n", "S\n"),
                w1,
            ),
            in_worktree(
                with_hashes(replace("a2", "t1", 2, "S\n", "X\n"), "S\n", "X\n"),
                w1,
            ),
            candidate(with_hashes(
                replace("b1", "t2", 1, "S\n", "T\n"),
                "S\n",
                "T\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["a1", "b1"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn equal_ordinals_are_unordered() {
    // One record issued both edits; either could have run last.
    let out = wt(
        "a\nb\n",
        "A\nB\n",
        vec![
            candidate(replace("e1", "t", 5, "a\n", "A\n")),
            candidate(replace("e2", "t", 5, "b\n", "B\n")),
        ],
    );
    let heads: Vec<EventId> = out.chains.iter().map(|w| w.chain.head.clone()).collect();
    assert_eq!(heads, ids(&["e1", "e2"]));
    assert!(
        out.chains
            .iter()
            .all(|w| w.chain.class == ChainClass::ExactFromBase)
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn replayed_observations_are_one_edit() {
    let e = replace("e1", "t", 1, "a\n", "b\n");
    let out = wt("a\n", "b\n", vec![candidate(e.clone()), candidate(e)]);
    assert_eq!(out.accepted.len(), 1);
    assert_eq!(
        summary(&out),
        vec![(ids(&["e1"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn an_unknown_and_a_known_operation_are_one_edit() {
    let known = replace("e1", "t", 1, "a\n", "b\n");
    let mut old_log = known.clone();
    old_log.operation = EditOperation::Unknown;
    let (accepted, conflicts) = dedup(vec![
        CandidateEdit {
            record: RecordId::new("old"),
            ..candidate(old_log)
        },
        CandidateEdit {
            record: RecordId::new("new"),
            ..candidate(known)
        },
    ]);
    assert!(conflicts.is_empty());
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].edit.operation, EditOperation::Replace);
    assert_eq!(
        accepted[0].records,
        vec![RecordId::new("old"), RecordId::new("new")]
    );
}

#[test]
fn contradictory_sightings_are_excluded_as_conflicts() {
    let a = replace("e1", "t", 1, "a\n", "b\n");
    let b = replace("e1", "t", 1, "a\n", "c\n");
    let out = wt(
        "a\n",
        "b\n",
        vec![
            CandidateEdit {
                record: RecordId::new("r1"),
                ..candidate(a)
            },
            CandidateEdit {
                record: RecordId::new("r2"),
                ..candidate(b)
            },
        ],
    );
    assert!(out.accepted.is_empty());
    assert_eq!(out.conflicts.len(), 1);
    assert_eq!(
        out.conflicts[0].records,
        vec![RecordId::new("r1"), RecordId::new("r2")]
    );
    assert!(matches!(out.status, PathStatus::NotComposed { .. }));
}

#[test]
fn a_manual_change_that_contradicts_an_internal_after_hash_prevents_exactness() {
    // E1 recorded x x -> y x; a manual change made it x y; E2 then y -> Y.
    // Reversing E2 and the unique y for E1 would reach the base, but E1's
    // recorded after-state contradicts the bytes.
    let out = wt(
        "x\nx\n",
        "x\nY\n",
        vec![
            candidate(with_hashes(
                replace("e1", "t", 1, "x\n", "y\n"),
                "x\nx\n",
                "y\nx\n",
            )),
            candidate(replace("e2", "t", 2, "y\n", "Y\n")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e2"]), ChainClass::Consistent, false)]
    );
    assert_eq!(
        out.chains[0].chain.end,
        ChainEnd::Stopped {
            reasons: vec![StopReason::AfterHashMismatch {
                edit: EventId::new("e1")
            }]
        }
    );
}

#[test]
fn two_verified_histories_that_differ_in_which_edit_produced_a_state_both_stay() {
    // Transcript A: B -> S -> T. Transcript B: B -> S. Both are exact.
    let out = wt(
        "x\nz\n",
        "y\nZ\n",
        vec![
            candidate(with_hashes(
                replace("a1", "tA", 1, "x\n", "y\n"),
                "x\nz\n",
                "y\nz\n",
            )),
            candidate(with_hashes(
                replace("a2", "tA", 2, "z\n", "Z\n"),
                "y\nz\n",
                "y\nZ\n",
            )),
            candidate(with_hashes(
                replace("b1", "tB", 1, "x\n", "y\n"),
                "x\nz\n",
                "y\nz\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["a1", "a2"]), ChainClass::ExactFromBase, true),
            (ids(&["b1", "a2"]), ChainClass::ExactFromBase, true),
        ]
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn heads_that_each_start_elsewhere_are_both_kept() {
    let out = wt(
        "a\nb\n",
        "A\nB\n",
        vec![
            candidate(with_hashes(
                replace("ea", "tA", 1, "a\n", "A\n"),
                "a\nB\n",
                "A\nB\n",
            )),
            candidate(with_hashes(
                replace("eb", "tB", 1, "b\n", "B\n"),
                "A\nb\n",
                "A\nB\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["ea"]), ChainClass::ExactFromStart, false),
            (ids(&["eb"]), ChainClass::ExactFromStart, false),
        ]
    );
}

#[test]
fn a_verified_step_behind_an_unknown_hash_step_is_not_set_aside() {
    let out = wt(
        "A\n",
        "C\n",
        vec![
            candidate(with_hashes(
                replace("ea", "t1", 1, "A\n", "C\n"),
                "A\n",
                "C\n",
            )),
            candidate(replace("ex", "t2", 1, "X\n", "Y\n")),
            candidate(with_hashes(
                replace("eb", "t2", 2, "Y\n", "C\n"),
                "Y\n",
                "C\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["ea"]), ChainClass::ExactFromBase, true),
            (ids(&["ex", "eb"]), ChainClass::Consistent, false),
        ]
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn a_location_choice_that_misses_the_base_is_set_aside() {
    let out = wt(
        "a\nx\n",
        "a\na\n",
        vec![candidate(replace("d", "t", 1, "x\n", "a\n"))],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["d"]), ChainClass::ExactFromBase, true)]
    );
    assert_eq!(out.set_aside.len(), 1);
    assert_eq!(out.set_aside[0].start, common::state(Some("x\na\n")));
}

#[test]
fn a_location_choice_whose_branches_use_different_edits_is_not_set_aside() {
    // P (verified) is admitted only by the reading that edits line 1.
    let out = wt(
        "a\nx\n",
        "a\na\n",
        vec![
            candidate(with_hashes(
                replace("p", "t", 1, "q\n", "a\n"),
                "x\nq\n",
                "x\na\n",
            )),
            candidate(replace("d", "t", 2, "x\n", "a\n")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["p", "d"]), ChainClass::Consistent, false),
            (ids(&["d"]), ChainClass::ExactFromBase, true),
        ]
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn the_whole_sequence_and_the_deletion_alone_both_replay_from_the_base() {
    // [a, b], a -> x, b -> a, x -> b, delete a, with unknown hashes.
    let out = wt(
        "a\nb\n",
        "b\n",
        vec![
            candidate(replace("e1", "t", 1, "a\n", "x\n")),
            candidate(replace("e2", "t", 2, "b\n", "a\n")),
            candidate(replace("e3", "t", 3, "x\n", "b\n")),
            candidate(replace("e4", "t", 4, "a\n", "")),
        ],
    );
    let exact: Vec<Vec<EventId>> = summary(&out)
        .into_iter()
        .filter(|(_, class, _)| *class == ChainClass::ExactFromBase)
        .map(|(edits, _, _)| edits)
        .collect();
    assert_eq!(exact, vec![ids(&["e4"]), ids(&["e1", "e2", "e3", "e4"])]);
}

#[test]
fn an_edit_reverted_by_hand_still_allows_an_exact_chain() {
    // b -> c was recorded, then reverted by hand; a -> x followed.
    let out = wt(
        "a\nb\n",
        "x\nb\n",
        vec![
            candidate(replace("e1", "t", 1, "b\n", "c\n")),
            candidate(replace("e2", "t", 2, "a\n", "x\n")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e2"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn working_tree_heads_are_only_the_last_bucket_of_the_reviewed_worktree() {
    let edits = || {
        vec![
            candidate(replace("e1", "t", 1, "a\n", "b\n")),
            candidate(replace("e2", "t", 2, "b\n", "c\n")),
        ]
    };
    // The target is e1's result; e2 was later undone by hand. A later
    // observed edit supersedes e1's state in the working tree.
    let working = wt("a\n", "b\n", edits());
    assert!(working.chains.is_empty());
    assert_eq!(
        working.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::NotLocatable {
                edit: EventId::new("e2")
            }]
        }
    );
    // A commit may predate e2, so any edit can be a head.
    let commit = walk(&request(
        Some("a\n"),
        Some("b\n"),
        TargetKind::Commit,
        edits(),
    ));
    assert_eq!(
        summary(&commit),
        vec![(ids(&["e1"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn an_edit_in_an_originating_worktree_can_be_a_working_tree_head() {
    // The reviewed worktree has no edit of the path; the commit came from W1.
    let out = wt(
        "a\n",
        "t\n",
        vec![in_worktree(replace("e1", "t", 1, "a\n", "t\n"), "/work/w1")],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e1"]), ChainClass::ExactFromBase, true)]
    );
    assert_ne!(WT, "/work/w1");
}

#[test]
fn an_exhausted_budget_makes_the_search_incomplete() {
    let mut req = request(
        Some("a\n"),
        Some("b\n"),
        TargetKind::WorkingTree,
        vec![candidate(replace("e1", "t", 1, "a\n", "b\n"))],
    );
    req.budget_bytes = 3;
    assert_eq!(walk(&req).status, PathStatus::SearchIncomplete);
}

#[test]
fn no_candidate_edit_is_not_composed() {
    let out = wt("a\n", "b\n", Vec::new());
    assert_eq!(
        out.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::NoPredecessor]
        }
    );
}

#[test]
fn steps_hold_the_states_between_edits_oldest_first() {
    let out = wt(
        "a\n",
        "c\n",
        vec![
            candidate(replace("e1", "t", 1, "a\n", "b\n")),
            candidate(replace("e2", "t", 2, "b\n", "c\n")),
        ],
    );
    let chain = &out.chains[0];
    assert_eq!(chain.start, common::state(Some("a\n")));
    let states: Vec<_> = chain
        .steps
        .iter()
        .map(|s| (s.edit.clone(), s.before.clone(), s.after.clone()))
        .collect();
    assert_eq!(
        states,
        vec![
            (
                EventId::new("e1"),
                common::state(Some("a\n")),
                common::state(Some("b\n"))
            ),
            (
                EventId::new("e2"),
                common::state(Some("b\n")),
                common::state(Some("c\n"))
            ),
        ]
    );
    assert_eq!(chain.chain.head, EventId::new("e2"));
}

#[test]
fn a_sibling_stopped_early_by_a_recorded_check_is_set_aside() {
    // Only the true deletion position satisfies the earlier edit's
    // recorded after-hash; the other positions stop there, using fewer
    // edits than the chain that reaches the base.
    let out = wt(
        "a\nb\nc\n",
        "a\nB\n",
        vec![
            candidate(with_hashes(
                replace("e0", "t", 1, "b\n", "B\n"),
                "a\nb\nc\n",
                "a\nB\nc\n",
            )),
            candidate(replace("e1", "t", 2, "c\n", "")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e0", "e1"]), ChainClass::ExactFromBase, true)]
    );
    assert_eq!(out.set_aside.len(), 2);
    assert!(out.set_aside.iter().all(|w| w.chain.edits == ids(&["e1"])));
}

#[test]
fn a_sibling_stopped_early_by_a_creation_is_set_aside() {
    let out = walk(&request(
        None,
        Some("b\n"),
        TargetKind::WorkingTree,
        vec![
            candidate(create("c", "t", 1, "a\nb\n")),
            candidate(replace("d", "t", 2, "a\n", "")),
        ],
    ));
    assert_eq!(
        summary(&out),
        vec![(ids(&["c", "d"]), ChainClass::ExactFromBase, true)]
    );
    assert_eq!(out.set_aside.len(), 1);
}

#[test]
fn a_large_bucket_of_equal_ordinals_ends_search_incomplete_promptly() {
    // One record issued 25 edits of the file at once, and a commit target
    // lets any of them be the head: every subset and order of the bucket is
    // a branch. Enumeration must stop at the budget, never build the
    // powerset.
    let (mut base, mut target) = (String::new(), String::new());
    for i in 1..=25 {
        writeln!(base, "x{i:02}").unwrap();
        writeln!(target, "y{i:02}").unwrap();
    }
    let edits = (1..=25)
        .map(|i| {
            candidate(replace(
                &format!("e{i}"),
                "t",
                1,
                &format!("x{i:02}\n"),
                &format!("y{i:02}\n"),
            ))
        })
        .collect();
    let req = request(Some(&base), Some(&target), TargetKind::Commit, edits);
    let out = finishes_within(Duration::from_secs(60), move || walk(&req));
    assert_eq!(out.status, PathStatus::SearchIncomplete);
}

#[test]
fn a_hashless_deletion_in_a_large_file_ends_exact_from_the_base() {
    // Without the patch every line start of 3,000 lines is a candidate and
    // the default budget runs out; the patch's old side leaves the true one.
    let lines: Vec<String> = (0..3_000).map(|i| format!("line {i:04}\n")).collect();
    let base = lines.concat();
    let target = [&lines[..1_500], &lines[1_501..]].concat().concat();
    let edit = with_patch(replace("d", "t", 1, &lines[1_500], ""), &base, &target);
    let out = wt(&base, &target, vec![candidate(edit)]);
    assert_eq!(out.status, PathStatus::Composed);
    assert_eq!(
        summary(&out),
        vec![(ids(&["d"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn every_alignment_choice_is_charged_to_the_budget() {
    // 18 independent prefix/suffix choices make 2^18 alignments of one
    // step; each is charged, so a 1 MiB budget runs out long before.
    let base = "x\nx\n".repeat(18);
    let target = "x\n".repeat(18);
    let edit = with_hashes(replace_all("e", "t", 1, "x\nx\n", "x\n"), &base, &target);
    let mut req = request(
        Some(&base),
        Some(&target),
        TargetKind::WorkingTree,
        vec![candidate(edit)],
    );
    req.budget_bytes = 1 << 20;
    let out = finishes_within(Duration::from_secs(60), move || walk(&req));
    assert_eq!(out.status, PathStatus::SearchIncomplete);
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-anchor --test walk`
Expected: compile errors, for example ``unresolved import `supersigil_anchor::walk` ``
and ``no `ChainClass` in the root``.

- [ ] **Step 4: Add the chain types**

Replace `crates/supersigil-anchor/src/result.rs` with:

```rust
//! Results of attribution: why walks stop and the chains they find.

use supersigil_record::{EventId, RecordId};

/// Why reversing an edit, or a whole chain, stopped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopReason {
    /// The edit's operation was not recorded.
    OperationUnknown {
        /// The edit.
        edit: EventId,
    },
    /// The replacement texts the reversal needs were not retained.
    TextUnavailable {
        /// The edit.
        edit: EventId,
    },
    /// The edit's replacement text could not be located in the current bytes.
    NotLocatable {
        /// The edit.
        edit: EventId,
    },
    /// A whole-file write whose previous content was not recorded.
    WholeFileWrite {
        /// The edit.
        edit: EventId,
    },
    /// A replace-all edit whose joint inverse could not be verified.
    ReplaceAllUnverified {
        /// The edit.
        edit: EventId,
    },
    /// The current bytes contradict the edit's recorded after-state.
    AfterHashMismatch {
        /// The edit.
        edit: EventId,
    },
    /// The current bytes contradict the lines the edit's retained patch
    /// shows after it.
    AfterPatchMismatch {
        /// The edit.
        edit: EventId,
    },
    /// Every candidate before-state failed its own checks.
    NoAcceptedCandidate {
        /// The edit.
        edit: EventId,
    },
    /// No recorded edit could precede the chain's start.
    NoPredecessor,
}

/// What verifies a chain, strongest first: `min()` of several is the
/// strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainClass {
    /// Replaying the chain from the base reproduces the target.
    ExactFromBase,
    /// Every step's recorded hashes verify, but the chain starts elsewhere.
    ExactFromStart,
    /// Every step reversed, but something is unverified and the chain does
    /// not start at the base.
    Consistent,
}

/// Where a chain's walk ended.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChainEnd {
    /// At a state equal to the base.
    Base,
    /// Where no predecessor was accepted, with the reasons.
    Stopped {
        /// Why each candidate predecessor was rejected.
        reasons: Vec<StopReason>,
    },
}

/// One reading of recorded edits that reproduces the target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Chain {
    /// Identifier within one path's result.
    pub id: usize,
    /// What verifies the chain.
    pub class: ChainClass,
    /// The newest edit: the one that produced the target.
    pub head: EventId,
    /// The chain's edits, oldest first.
    pub edits: Vec<EventId>,
    /// Where the walk ended.
    pub end: ChainEnd,
}

/// The outcome of the search for one path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathStatus {
    /// The search completed and found at least one chain.
    Composed,
    /// The search completed and no head could be reversed.
    NotComposed {
        /// Why each head was rejected.
        reasons: Vec<StopReason>,
    },
    /// The work budget ran out; nothing on the path is exact.
    SearchIncomplete,
}

/// An edit whose sightings in several records disagree; it is excluded.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Conflict {
    /// The edit.
    pub edit: EventId,
    /// Every record that holds a sighting of it.
    pub records: Vec<RecordId>,
}
```

Replace `crates/supersigil-anchor/src/lib.rs` with:

```rust
//! Attributes the lines of a reviewed change to the recorded edits that
//! produced them.
//!
//! Anchor walks backward from a target state through recorded edits. Each
//! step proposes before-states and accepts one only when the recorded hashes
//! (where known), the retained patch (where kept), and forward execution
//! with the recorder's semantics agree ([`step`]). Lines are aligned across
//! a step from the byte ranges forward execution replaced, never from a
//! diff ([`align`]). Anchor is pure: it reads no files and runs no
//! processes.

pub mod align;
pub mod input;
pub mod lines;
pub mod result;
pub mod step;
pub mod walk;

pub use input::{CandidateEdit, DEFAULT_BUDGET_BYTES, Request, State, TargetKind};
pub use result::{Chain, ChainClass, ChainEnd, Conflict, PathStatus, StopReason};
```

- [ ] **Step 5: Write the walk**

`crates/supersigil-anchor/src/walk.rs`:

```rust
//! Walks backward from the target through recorded edits and collects every
//! chain the evidence supports.
//!
//! Edits are grouped by (worktree, transcript); within a group, edits with
//! equal ordinals form an unordered bucket and buckets are ordered by
//! ordinal. A chain starts at a head, continues backward through its group
//! without skipping an edit, and may enter another group only through a
//! verified link: an edit whose recorded after-hash equals the hash of the
//! current bytes. It ends at the first state equal to the base, or where no
//! predecessor is accepted. Time never decides.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use supersigil_record::observations::{Content, Edit, EditOperation, FileState};
use supersigil_record::{ContentId, EventId, RecordId};

use crate::align::{Alignment, align};
use crate::input::{CandidateEdit, Request, State, TargetKind};
use crate::result::{Chain, ChainClass, ChainEnd, Conflict, PathStatus, StopReason};
use crate::step::{Budget, Reversed, reverse};

/// An edit that survived deduplication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedEdit {
    /// The edit, with a known operation when any sighting had one.
    pub edit: Edit,
    /// Root of the worktree the edit's file lies in.
    pub worktree: PathBuf,
    /// Every record holding a sighting of it.
    pub records: Vec<RecordId>,
}

/// Applies the record's event-identity rule.
///
/// Within one record the first sighting of an edit id wins, as in derive.
/// Sightings in different records are one edit when their payloads and
/// worktrees are equal, or differ only in that one has `operation: unknown`
/// where the other has a known operation (sync never rewrites an older log);
/// the known operation is kept and every record is listed. Any other
/// difference excludes the edit as a [`Conflict`]; nothing is resolved by
/// order. Accepted edits keep the order of their first sighting.
#[must_use]
pub fn dedup(edits: Vec<CandidateEdit>) -> (Vec<AcceptedEdit>, Vec<Conflict>) {
    struct Merge {
        accepted: AcceptedEdit,
        conflict: bool,
    }
    let mut first_in_record: BTreeSet<(RecordId, EventId)> = BTreeSet::new();
    let mut position: BTreeMap<EventId, usize> = BTreeMap::new();
    let mut merged: Vec<Merge> = Vec::new();
    for candidate in edits {
        let id = candidate.edit.id.clone();
        if !first_in_record.insert((candidate.record.clone(), id.clone())) {
            continue;
        }
        let Some(&at) = position.get(&id) else {
            position.insert(id, merged.len());
            merged.push(Merge {
                accepted: AcceptedEdit {
                    edit: candidate.edit,
                    worktree: candidate.worktree,
                    records: vec![candidate.record],
                },
                conflict: false,
            });
            continue;
        };
        let merge = &mut merged[at];
        merge.accepted.records.push(candidate.record);
        if merge.conflict {
            continue;
        }
        match compatible(
            &merge.accepted.edit,
            &merge.accepted.worktree,
            &candidate.edit,
            &candidate.worktree,
        ) {
            Some(edit) => merge.accepted.edit = edit,
            None => merge.conflict = true,
        }
    }
    let mut accepted = Vec::new();
    let mut conflicts = Vec::new();
    for merge in merged {
        if merge.conflict {
            conflicts.push(Conflict {
                edit: merge.accepted.edit.id,
                records: merge.accepted.records,
            });
        } else {
            accepted.push(merge.accepted);
        }
    }
    (accepted, conflicts)
}

/// Merges two sightings of one edit, or `None` when they conflict.
fn compatible(a: &Edit, a_worktree: &Path, b: &Edit, b_worktree: &Path) -> Option<Edit> {
    if a_worktree != b_worktree {
        return None;
    }
    if a == b {
        return Some(a.clone());
    }
    let (known, unknown) = match (a.operation, b.operation) {
        (EditOperation::Unknown, op) if op != EditOperation::Unknown => (b, a),
        (op, EditOperation::Unknown) if op != EditOperation::Unknown => (a, b),
        _ => return None,
    };
    let mut lifted = unknown.clone();
    lifted.operation = known.operation;
    (lifted == *known).then(|| known.clone())
}

/// One reversed step of a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStep {
    /// The edit.
    pub edit: EventId,
    /// The state before the edit.
    pub before: State,
    /// The state after the edit.
    pub after: State,
    /// The line alignment this branch uses for the step.
    pub alignment: Alignment,
}

/// A chain with the states and alignments its walk established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkedChain {
    /// The chain.
    pub chain: Chain,
    /// The state before its oldest step.
    pub start: State,
    /// Its steps, oldest first.
    pub steps: Vec<ChainStep>,
}

/// Everything the walk of one path found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
    /// Whether the search completed and found anything.
    pub status: PathStatus,
    /// Chains whose readings are combined line by line.
    pub chains: Vec<WalkedChain>,
    /// Alternatives that assume an unrecorded change: chains that do not
    /// end at the base, covered by one that does (see [`walk`]).
    pub set_aside: Vec<WalkedChain>,
    /// The deduplicated edits.
    pub accepted: Vec<AcceptedEdit>,
    /// Edits excluded as conflicting evidence.
    pub conflicts: Vec<Conflict>,
}

/// Walks backward from `request.target` and returns every chain found.
///
/// Heads are the edits whose reversal from the target is accepted: for a
/// working-tree target, the edits of the last bucket of each group in the
/// reviewed worktree (every pinned observation predates the target, so a
/// group's later edits supersede its earlier states) and any edit of a
/// group in another worktree; for a commit target, any edit. Every order of
/// a bucket's edits is tried. At every state both the previous bucket of
/// the current group and every verified link into a group not yet entered
/// are explored. Chains are classed by what verifies them.
///
/// A chain that does not end at the base is set aside when a chain that
/// does end there entered the same groups in the same order and used, in
/// each, every edit the first one used: the two differ only in unrecorded
/// choices (replacement locations, bucket orders, alignments), and the
/// first holds no recorded edit the second lacks. It may use fewer, when a
/// recorded check (an after-hash, a creation's content) stopped it early.
/// A chain holding an edit that no base-reaching chain holds is never set
/// aside, so no recorded evidence is dropped.
///
/// Subsets and orders of a bucket are generated one at a time, never
/// materialized. Every generated branch is charged 1 KiB before it is
/// explored, every reversal candidate its bytes (see [`reverse`]), and
/// every alignment the step's bytes; enumeration stops the moment the
/// budget is exhausted. The status is then
/// [`PathStatus::SearchIncomplete`] and the chains found so far are
/// returned.
#[must_use]
pub fn walk(request: &Request) -> Walk {
    let (accepted, conflicts) = dedup(request.edits.clone());
    let groups = group(&accepted);
    let mut search = Search {
        request,
        accepted: &accepted,
        groups: &groups,
        budget: Budget::new(request.budget_bytes),
        found: Vec::new(),
        head_failures: BTreeSet::new(),
    };
    let mut any_head = false;
    for (g, grp) in groups.iter().enumerate() {
        let last_only = request.target_kind == TargetKind::WorkingTree
            && grp.worktree == request.reviewed_worktree;
        let buckets: Vec<usize> = if last_only {
            (grp.buckets.len() - 1..grp.buckets.len()).collect()
        } else {
            (0..grp.buckets.len()).collect()
        };
        for b in buckets {
            any_head = true;
            let start = Partial::head(request.target.clone(), g, b);
            if last_only {
                let remaining = grp.buckets[b].clone();
                search.explore(Partial { remaining, ..start });
            } else {
                for &idx in &grp.buckets[b] {
                    for before in Subsets::new(without(&grp.buckets[b], idx)) {
                        if search.budget.exhausted() {
                            break;
                        }
                        let mut reasons = BTreeSet::new();
                        search.try_step(&start, idx, &before, &mut reasons);
                        search.head_failures.extend(reasons);
                    }
                }
            }
        }
    }
    let exhausted = search.budget.exhausted();
    let head_failures = search.head_failures;
    let (chains, set_aside) = settle(search.found);
    let status = if exhausted {
        PathStatus::SearchIncomplete
    } else if !chains.is_empty() {
        PathStatus::Composed
    } else if any_head {
        PathStatus::NotComposed {
            reasons: head_failures.into_iter().collect(),
        }
    } else {
        PathStatus::NotComposed {
            reasons: vec![StopReason::NoPredecessor],
        }
    };
    Walk {
        status,
        chains,
        set_aside,
        accepted,
        conflicts,
    }
}

/// The edits of one file from one transcript.
struct Group {
    worktree: PathBuf,
    /// Indices into the accepted edits, one bucket per ordinal, oldest first.
    buckets: Vec<Vec<usize>>,
}

fn group(accepted: &[AcceptedEdit]) -> Vec<Group> {
    let mut by_key: BTreeMap<(PathBuf, Option<String>), Vec<usize>> = BTreeMap::new();
    for (i, a) in accepted.iter().enumerate() {
        by_key
            .entry((a.worktree.clone(), a.edit.transcript.clone()))
            .or_default()
            .push(i);
    }
    by_key
        .into_iter()
        .map(|((worktree, _), mut indices)| {
            indices.sort_by_key(|&i| accepted[i].edit.source_ordinal);
            let buckets = indices
                .chunk_by(|&a, &b| {
                    accepted[a].edit.source_ordinal == accepted[b].edit.source_ordinal
                })
                .map(<[usize]>::to_vec)
                .collect();
            Group { worktree, buckets }
        })
        .collect()
}

fn without(bucket: &[usize], idx: usize) -> Vec<usize> {
    bucket.iter().copied().filter(|&i| i != idx).collect()
}

/// The subsets of a bucket's other edits (which of them came before the
/// entry edit), generated one at a time and never materialized together.
struct Subsets {
    items: Vec<usize>,
    /// Membership of the next subset; `None` when every subset was given.
    next: Option<Vec<bool>>,
}

impl Subsets {
    fn new(items: Vec<usize>) -> Self {
        let next = Some(vec![false; items.len()]);
        Self { items, next }
    }
}

impl Iterator for Subsets {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Vec<usize>> {
        let member = self.next.as_mut()?;
        let subset = self
            .items
            .iter()
            .zip(member.iter())
            .filter(|(_, in_it)| **in_it)
            .map(|(&item, _)| item)
            .collect();
        // Advance like a binary counter; wrapping around means done.
        let mut carry = true;
        for bit in member.iter_mut() {
            if !carry {
                break;
            }
            *bit = !*bit;
            carry = !*bit;
        }
        if carry {
            self.next = None;
        }
        Some(subset)
    }
}

/// What generating one branch costs the budget before it is explored, so
/// that branches failing cheaply still exhaust it.
const BRANCH_COST: usize = 1024;

/// Whether a recorded state can be checked: absent, or present with a known
/// content hash.
fn is_known(state: &FileState) -> bool {
    !matches!(
        state,
        FileState::Present {
            content: Content::Unknown
        }
    )
}

/// A chain under construction, walking backward.
#[derive(Clone)]
struct Partial {
    /// The current state: the before-state of the newest reversed step, or
    /// the target before any.
    state: State,
    /// Reversed steps and their accepted-edit indices, newest first.
    steps: Vec<(usize, ChainStep)>,
    /// Groups entered, in order.
    entered: Vec<usize>,
    /// The current group and bucket.
    group: usize,
    bucket: usize,
    /// Edits of the current bucket still to reverse before leaving it.
    remaining: Vec<usize>,
}

impl Partial {
    fn head(target: State, group: usize, bucket: usize) -> Self {
        Self {
            state: target,
            steps: Vec::new(),
            entered: vec![group],
            group,
            bucket,
            remaining: Vec::new(),
        }
    }
}

/// A finished chain before ids are assigned.
struct Found {
    /// The groups entered with the edits used in each: chains with equal
    /// keys differ only in unrecorded choices.
    key: Vec<(usize, BTreeSet<EventId>)>,
    walked: WalkedChain,
}

struct Search<'a> {
    request: &'a Request,
    accepted: &'a [AcceptedEdit],
    groups: &'a [Group],
    budget: Budget,
    found: Vec<Found>,
    head_failures: BTreeSet<StopReason>,
}

impl Search<'_> {
    /// Extends `partial` backward by every accepted predecessor, or finishes it.
    fn explore(&mut self, partial: Partial) {
        if self.budget.exhausted() {
            return;
        }
        if !partial.steps.is_empty() && partial.state == self.request.base {
            self.finish(partial, ChainEnd::Base);
            return;
        }
        let mut reasons = BTreeSet::new();
        let mut progressed = false;
        if partial.remaining.is_empty() {
            if partial.bucket > 0 && !partial.steps.is_empty() {
                let bucket = &self.groups[partial.group].buckets[partial.bucket - 1];
                let moved = Partial {
                    bucket: partial.bucket - 1,
                    ..partial.clone()
                };
                for &idx in bucket {
                    progressed |= self.try_step(&moved, idx, &without(bucket, idx), &mut reasons);
                }
            }
            if !partial.steps.is_empty() {
                progressed |= self.try_links(&partial, &mut reasons);
            }
        } else {
            for &idx in &partial.remaining.clone() {
                let rest = without(&partial.remaining, idx);
                progressed |= self.try_step(&partial, idx, &rest, &mut reasons);
            }
        }
        if progressed || self.budget.exhausted() {
            return;
        }
        if partial.steps.is_empty() {
            self.head_failures.extend(reasons);
            return;
        }
        if reasons.is_empty() {
            reasons.insert(StopReason::NoPredecessor);
        }
        self.finish(
            partial,
            ChainEnd::Stopped {
                reasons: reasons.into_iter().collect(),
            },
        );
    }

    /// Tries every verified link out of `partial`'s state.
    fn try_links(&mut self, partial: &Partial, reasons: &mut BTreeSet<StopReason>) -> bool {
        let Some(hash) = partial.state.content_id() else {
            return false;
        };
        let mut progressed = false;
        for (g, grp) in self.groups.iter().enumerate() {
            if partial.entered.contains(&g) {
                continue;
            }
            for (b, bucket) in grp.buckets.iter().enumerate() {
                for &idx in bucket {
                    if !links(&self.accepted[idx].edit.after, &hash) {
                        continue;
                    }
                    let mut entered = partial.clone();
                    entered.entered.push(g);
                    entered.group = g;
                    entered.bucket = b;
                    for before in Subsets::new(without(bucket, idx)) {
                        if self.budget.exhausted() {
                            return progressed;
                        }
                        progressed |= self.try_step(&entered, idx, &before, reasons);
                    }
                }
            }
        }
        progressed
    }

    /// Reverses edit `idx` from `partial`'s state and explores every
    /// accepted candidate and alignment, leaving `remaining` of its bucket
    /// to reverse next. The branch is charged [`BRANCH_COST`] first, and
    /// every alignment the step's bytes before it is explored. Returns
    /// whether any candidate was accepted.
    fn try_step(
        &mut self,
        partial: &Partial,
        idx: usize,
        remaining: &[usize],
        reasons: &mut BTreeSet<StopReason>,
    ) -> bool {
        if !self.budget.charge(BRANCH_COST) {
            return false;
        }
        let edit = &self.accepted[idx].edit;
        let candidates = match reverse(edit, &partial.state, &self.request.base, &mut self.budget) {
            Reversed::Stop(reason) => {
                reasons.insert(reason);
                return false;
            }
            Reversed::Candidates(candidates) => candidates,
        };
        let progressed = !candidates.is_empty();
        for reversal in candidates {
            let step_bytes = reversal.before.byte_len() + partial.state.byte_len();
            for alignment in align(&reversal.before, &partial.state, &reversal.replacements) {
                if !self.budget.charge(step_bytes) {
                    return progressed;
                }
                let mut next = partial.clone();
                next.steps.push((
                    idx,
                    ChainStep {
                        edit: edit.id.clone(),
                        before: reversal.before.clone(),
                        after: partial.state.clone(),
                        alignment,
                    },
                ));
                next.state = reversal.before.clone();
                remaining.clone_into(&mut next.remaining);
                self.explore(next);
            }
        }
        progressed
    }

    fn finish(&mut self, partial: Partial, end: ChainEnd) {
        let from_base = end == ChainEnd::Base;
        let verified = partial.steps.iter().all(|(idx, _)| {
            let edit = &self.accepted[*idx].edit;
            is_known(&edit.before) && is_known(&edit.after)
        });
        let class = if from_base {
            ChainClass::ExactFromBase
        } else if verified {
            ChainClass::ExactFromStart
        } else {
            ChainClass::Consistent
        };
        let mut key: Vec<(usize, BTreeSet<EventId>)> = Vec::new();
        let mut steps: Vec<(usize, ChainStep)> = partial.steps;
        for &g in &partial.entered {
            let ids = steps
                .iter()
                .filter(|(idx, _)| self.groups[g].buckets.iter().any(|b| b.contains(idx)))
                .map(|(_, step)| step.edit.clone())
                .collect();
            key.push((g, ids));
        }
        steps.reverse();
        let head = steps
            .last()
            .map(|(_, step)| step.edit.clone())
            .expect("a finished chain has at least one step");
        let steps: Vec<ChainStep> = steps.into_iter().map(|(_, step)| step).collect();
        self.found.push(Found {
            key,
            walked: WalkedChain {
                chain: Chain {
                    id: 0,
                    class,
                    head,
                    edits: steps.iter().map(|s| s.edit.clone()).collect(),
                    end,
                },
                start: partial.state,
                steps,
            },
        });
    }
}

/// Whether an edit's recorded after-state is a known hash equal to `hash`.
fn links(after: &FileState, hash: &ContentId) -> bool {
    matches!(after, FileState::Present { content: Content::Known(id) } if id == hash)
}

/// Assigns ids in discovery order and sets aside every chain not ending at
/// the base whose key is covered by the key of one that does.
fn settle(found: Vec<Found>) -> (Vec<WalkedChain>, Vec<WalkedChain>) {
    let reaching: Vec<Vec<(usize, BTreeSet<EventId>)>> = found
        .iter()
        .filter(|f| f.walked.chain.end == ChainEnd::Base)
        .map(|f| f.key.clone())
        .collect();
    let mut chains = Vec::new();
    let mut set_aside = Vec::new();
    for (id, mut f) in found.into_iter().enumerate() {
        f.walked.chain.id = id;
        let covered =
            f.walked.chain.end != ChainEnd::Base && reaching.iter().any(|by| covers(by, &f.key));
        if covered {
            set_aside.push(f.walked);
        } else {
            chains.push(f.walked);
        }
    }
    (chains, set_aside)
}

/// Whether `by` entered the groups of `key` in the same order and used, in
/// each, every edit `key` used there.
fn covers(by: &[(usize, BTreeSet<EventId>)], key: &[(usize, BTreeSet<EventId>)]) -> bool {
    key.len() <= by.len()
        && key
            .iter()
            .zip(by)
            .all(|((g, edits), (h, all))| g == h && edits.is_subset(all))
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-anchor`
Expected: `65 tests run: 65 passed` (the 24 walk tests plus Task 8's 41).

- [ ] **Step 7: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; every test passes.

- [ ] **Step 8: Commit**

```bash
git add crates/supersigil-anchor/src/result.rs crates/supersigil-anchor/src/lib.rs crates/supersigil-anchor/src/walk.rs crates/supersigil-anchor/tests/common/mod.rs crates/supersigil-anchor/tests/walk.rs
git commit -m "feat: walk recorded edits back to the base"
```

---

### Task 10: Anchor provenance, combination, content match, and attribute

This task turns chains into what the review shows per line (design section 3,
"Combining chains, line by line", "Line provenance within a chain",
"Content-match fallback", "Per-path result"):

- `replay` walks one chain forward: start lines are base lines (chain from the
  base) or unexplained; kept lines keep provenance. Inside the replay a line
  is a sequence of character runs over its non-whitespace characters, each
  run with its own origin, whitespace-only edits, and earlier contributors,
  so a join followed by a split gives each resulting line only the history
  of its own characters. A changed region whose removed and added text are
  equal once all whitespace is deleted maps characters one to one and adds
  the edit as a *whitespace-only* contributor to every run it moves (base
  identity passes through, never *replaced*; a line of only whitespace is
  *whitespace added* or *removed by a whitespace-only change*); any other
  region makes its added lines *introduced* by the edit with *earlier*
  contributors from the removed runs, and each removed base line *replaced*
  by the edit. Runs are aggregated into sets only for the result.
- `combine` treats chains as alternative histories: a target line is agreed
  only when every chain gives it the identical provenance (origins,
  whitespace-only and earlier contributors; unexplained counts as an origin),
  a base line only when every chain gives it the same fates. Contributors
  are never unioned across chains: chains that name different contributors
  make the line ambiguous. `Fate::CarriedTo::via` is the whitespace-only set
  of the runs that carried the base line's characters.
- `content_matches` runs only when the search completed, and only for lines
  that need the fallback: target lines whose agreed origin is unexplained
  and base lines with no agreed fate. It finds each edit's whole `new_text`
  (or written content) in the target and `old_text` in the base by indexing
  lines by content and matching a block's whole interior lines by line
  identity, checking its first and last partial lines by bytes, so the work
  is linear in the file. It spends its own budget of `budget_bytes`; when
  that runs out it stops and sets `incomplete`, so a missing match is not
  read as no match. Conservative reading of "covers the lines it spans": a
  block covers a line only when it contains the whole line apart from its
  terminator, so a short replacement inside part of a line explains nothing;
  a block with no letter or digit never matches.
- `attribute` runs walk, replay, combine, and content matching. Nothing
  claims capture completeness.

The property tests use an oracle written in the test file that shares no code
with the crate: a forward interpreter that re-implements the recorder's
semantics and tags every byte with its source. (a) completeness runs over
histories restricted to what section 3 guarantees (one transcript,
single-line replacements and deletions, creation Writes, globally fresh
tab-indented lines, hashes randomly known or unknown, patches randomly
retained, built by `with_patch` in display form); (b) soundness adds one
observable unrecorded line; (c) justification runs over unrestricted
histories (duplicates, whitespace, a tab-indented line that displays like a
space-indented one, replace-all, overwrites, unknown operations, two
transcripts and worktrees, equal ordinals, commit and working-tree targets,
patches randomly retained). (a) therefore also shows the patch check never
rejects the true history: breaking the display rule (tabs compared as tabs)
makes it fail. (c) checks agreement on the full provenance, contributors
included. Over 5,000 unrestricted cases the generator retained a patch in
46%, and produced chains in 43%, exact chains in 39%, consistent chains in
4%, set-aside alternatives in 0.4%, and at least one ambiguous line in 0.9%
(1.1% of all target lines), so (c) is not vacuous.

**Files:**
- Modify: `crates/supersigil-anchor/tests/common/mod.rs` (adds `line_count`)
- Replace: `crates/supersigil-anchor/src/result.rs` (adds the line outcome types), `crates/supersigil-anchor/src/lib.rs`
- Create: `crates/supersigil-anchor/src/provenance.rs`, `crates/supersigil-anchor/src/combine.rs`, `crates/supersigil-anchor/src/content.rs`,
  `crates/supersigil-anchor/src/attribute.rs`
- Create: `crates/supersigil-anchor/tests/attribute.rs`, `crates/supersigil-anchor/tests/properties.rs`

**Interfaces:**
- Consumes: Task 9's `walk::{walk, WalkedChain, ChainStep, AcceptedEdit}`,
  `result::{Chain, ChainClass, ChainEnd, PathStatus, Conflict}`; Task 8's
  `lines::{line_starts, split_lines}`, `input::{Request, State}`.
- Produces: `result::{Origin, Provenance, Fate, Reading, LineOutcome,
  BaseLineOutcome, ContentMatches, PathAttribution}`;
  `provenance::{ChainProvenance, replay}`; `combine::combine`;
  `content::content_matches`; `attribute::attribute` re-exported as
  `supersigil_anchor::attribute`; all as the interfaces contract lists them,
  with two changes: `ContentMatches` gains `incomplete: bool`, and
  `content_matches(base, target, edits, target_outcomes, base_outcomes,
  budget_bytes)` takes the combined outcomes (to select the lines that need
  the fallback) and its own budget. `attribute(request: Request)` consumes
  the request. Test support adds `line_count(&State) -> usize`.

- [ ] **Step 1: Add the line-count helper**

In `crates/supersigil-anchor/tests/common/mod.rs`, add after `finishes_within`:

```rust
/// Lines of a state, counted the way anchor counts them.
pub fn line_count(state: &State) -> usize {
    supersigil_anchor::lines::line_starts(state.bytes().unwrap_or_default()).len()
}
```

- [ ] **Step 2: Write the failing attribution tests**

`crates/supersigil-anchor/tests/attribute.rs`. It pins the provenance and combination regressions
(duplicate lines, a string-literal whitespace change, a formatter join, a
formatter change to a base line, a split, split-then-rewrite, a whitespace-only
blank-line deletion, the `foofoo` deletion, overlapping context, two heads
that each leave a line unexplained, two verified histories, a verified chain
against a consistent one, the occurrence choice with and without different
edits, the whole sequence versus the deletion alone, unknown-hash deletions
with and without a chain from the base), the positive content-match case
(a gap before the chain, the earlier edit's whole block surviving), the
corpus-shaped large-file cases (`large_file_single_session_is_exact_from_base`
is the Review Focus test this task owns), and the unsupported cases (unknown
operation, overwrite, replace-all, budget). Further tests pin that a join
then a split keeps each character's own history, that chains naming
different whitespace-only contributors make a line ambiguous, that mixed
alignment choices make a joined line ambiguous, and that content matching
skips lines a chain explains, is skipped when the search is incomplete, is
linear on a huge target, and is marked incomplete when its budget runs out.

```rust
//! Attributing a path: provenance, combination across chains, the
//! content-match fallback, and the corpus-shaped and unsupported cases.

mod common;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::time::Duration;

use common::{
    candidate, finishes_within, overwrite, replace, replace_all, request, unknown_op, with_hashes,
};
use supersigil_anchor::{
    BaseLineOutcome, CandidateEdit, ChainClass, Fate, LineOutcome, Origin, PathAttribution,
    PathStatus, Provenance, StopReason, TargetKind, attribute,
};
use supersigil_record::EventId;

fn id(name: &str) -> EventId {
    EventId::new(name)
}

fn wt(base: &str, target: &str, edits: Vec<CandidateEdit>) -> PathAttribution {
    attribute(request(
        Some(base),
        Some(target),
        TargetKind::WorkingTree,
        edits,
    ))
}

/// The agreed provenance of a target line; panics when ambiguous.
fn agreed(out: &PathAttribution, line: usize) -> (&Provenance, Option<ChainClass>) {
    match &out.target[line] {
        LineOutcome::Agreed { provenance, class } => (provenance, *class),
        other @ LineOutcome::Ambiguous { .. } => panic!("line {line} is {other:?}"),
    }
}

fn origins(out: &PathAttribution, line: usize) -> BTreeSet<Origin> {
    agreed(out, line).0.origins.clone()
}

fn base_fates(out: &PathAttribution, line: usize) -> BTreeSet<Fate> {
    match &out.base[line] {
        BaseLineOutcome::Agreed { fates, .. } => fates.clone(),
        other @ BaseLineOutcome::Ambiguous { .. } => panic!("base line {line} is {other:?}"),
    }
}

fn base(line: usize) -> Origin {
    Origin::Base(line)
}

fn introduced(name: &str) -> Origin {
    Origin::Introduced(id(name))
}

fn set<T: Ord>(items: impl IntoIterator<Item = T>) -> BTreeSet<T> {
    items.into_iter().collect()
}

fn ambiguous(out: &PathAttribution, line: usize) -> Vec<BTreeSet<Origin>> {
    match &out.target[line] {
        LineOutcome::Ambiguous { readings } => {
            readings.iter().map(|r| r.value.origins.clone()).collect()
        }
        other @ LineOutcome::Agreed { .. } => panic!("line {line} is {other:?}"),
    }
}

#[test]
fn duplicate_lines_keep_their_own_provenance() {
    let out = wt(
        "A\nX\nX\nB\n",
        "A\nX\nB\n",
        vec![candidate(replace("e", "t", 1, "A\nX\n", "A\n"))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(origins(&out, 1), set([base(2)]));
    assert_eq!(base_fates(&out, 1), set([Fate::Replaced { edit: id("e") }]));
    assert_eq!(base_fates(&out, 2), set([Fate::KeptAs { line: 1 }]));
}

#[test]
fn a_string_literal_whitespace_change_is_whitespace_only_never_replaced() {
    let out = wt(
        "return \"a b\";\n",
        "return \"ab\";\n",
        vec![candidate(replace("e", "t", 1, "\"a b\"", "\"ab\""))],
    );
    let (provenance, class) = agreed(&out, 0);
    assert_eq!(provenance.origins, set([base(0)]));
    assert_eq!(provenance.whitespace_only, set([id("e")]));
    assert_eq!(class, Some(ChainClass::ExactFromBase));
    assert_eq!(
        base_fates(&out, 0),
        set([Fate::CarriedTo {
            line: 0,
            via: set([id("e")])
        }])
    );
}

#[test]
fn a_formatter_join_keeps_both_origins() {
    let out = wt(
        "left\n",
        "left right\n",
        vec![
            candidate(replace("e1", "t", 1, "left\n", "left\nright\n")),
            candidate(replace("e2", "t", 2, "left\nright\n", "left right\n")),
        ],
    );
    let (provenance, _) = agreed(&out, 0);
    assert_eq!(provenance.origins, set([base(0), introduced("e1")]));
    assert_eq!(provenance.whitespace_only, set([id("e2")]));
}

#[test]
fn a_formatter_change_to_a_base_line_carries_it() {
    let out = wt(
        "let x=1;\n",
        "let x = 1;\n",
        vec![candidate(replace(
            "fmt",
            "t",
            1,
            "let x=1;\n",
            "let x = 1;\n",
        ))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(
        base_fates(&out, 0),
        set([Fate::CarriedTo {
            line: 0,
            via: set([id("fmt")])
        }])
    );
}

#[test]
fn a_split_carries_the_base_line_to_both_target_lines() {
    let out = wt(
        "AxZ\nEND\n",
        "Ax\nZ\nEND\n",
        vec![candidate(replace("e", "t", 1, "x", "x\n"))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(origins(&out, 1), set([base(0)]));
    assert_eq!(origins(&out, 2), set([base(1)]));
    assert_eq!(
        base_fates(&out, 0),
        set([
            Fate::CarriedTo {
                line: 0,
                via: set([id("e")])
            },
            Fate::CarriedTo {
                line: 1,
                via: set([id("e")])
            },
        ])
    );
}

#[test]
fn splitting_then_rewriting_one_half_is_both_carried_and_replaced() {
    let out = wt(
        "AxZ\n",
        "Ax\nQ\n",
        vec![
            candidate(replace("split", "t", 1, "x", "x\n")),
            candidate(replace("rewrite", "t", 2, "Z\n", "Q\n")),
        ],
    );
    assert_eq!(
        base_fates(&out, 0),
        set([
            Fate::CarriedTo {
                line: 0,
                via: set([id("split")])
            },
            Fate::Replaced {
                edit: id("rewrite")
            },
        ])
    );
    let (provenance, _) = agreed(&out, 1);
    assert_eq!(provenance.origins, set([introduced("rewrite")]));
    assert_eq!(provenance.earlier, set([id("split")]));
}

#[test]
fn a_whitespace_only_blank_line_deletion_is_explained() {
    let out = wt(
        "\nX\n",
        "X\n",
        vec![candidate(replace("d", "t", 1, "\n", ""))],
    );
    assert_eq!(
        base_fates(&out, 0),
        set([Fate::RemovedByWhitespace { edit: id("d") }])
    );
    assert_eq!(base_fates(&out, 1), set([Fate::KeptAs { line: 0 }]));
    assert_eq!(out.set_aside.len(), 1);
}

#[test]
fn a_deletion_that_joins_two_lines_introduces_the_joined_line() {
    let out = wt(
        "foofoo\nfoo\nEND\n",
        "foofoo\nEND\n",
        vec![candidate(replace("d", "t", 1, "foo\n", ""))],
    );
    assert_eq!(origins(&out, 0), set([introduced("d")]));
    assert_eq!(origins(&out, 1), set([base(2)]));
    assert_eq!(base_fates(&out, 0), set([Fate::Replaced { edit: id("d") }]));
    assert_eq!(base_fates(&out, 1), set([Fate::Replaced { edit: id("d") }]));
}

#[test]
fn overlapping_context_makes_the_surviving_line_ambiguous() {
    let out = wt(
        "x\nx\n",
        "x\n",
        vec![candidate(replace("e", "t", 1, "x\nx\n", "x\n"))],
    );
    assert_eq!(ambiguous(&out, 0), vec![set([base(0)]), set([base(1)])]);
    assert!(matches!(out.base[0], BaseLineOutcome::Ambiguous { .. }));
}

#[test]
fn multi_line_replacement_context_stays_base_lines() {
    let out = wt(
        "a\nb\nc\n",
        "a\nB\nc\n",
        vec![candidate(replace("e", "t", 1, "a\nb\nc\n", "a\nB\nc\n"))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(origins(&out, 1), set([introduced("e")]));
    assert_eq!(origins(&out, 2), set([base(2)]));
    assert_eq!(base_fates(&out, 1), set([Fate::Replaced { edit: id("e") }]));
}

#[test]
fn two_heads_that_each_leave_a_line_unexplained_make_both_lines_ambiguous() {
    let out = wt(
        "a\nb\n",
        "A\nB\n",
        vec![
            candidate(with_hashes(
                replace("ea", "tA", 1, "a\n", "A\n"),
                "a\nB\n",
                "A\nB\n",
            )),
            candidate(with_hashes(
                replace("eb", "tB", 1, "b\n", "B\n"),
                "A\nb\n",
                "A\nB\n",
            )),
        ],
    );
    assert_eq!(
        ambiguous(&out, 0),
        vec![set([introduced("ea")]), set([Origin::Unexplained])]
    );
    assert_eq!(
        ambiguous(&out, 1),
        vec![set([Origin::Unexplained]), set([introduced("eb")])]
    );
}

#[test]
fn two_verified_histories_make_only_the_disputed_line_ambiguous() {
    let out = wt(
        "x\nz\n",
        "y\nZ\n",
        vec![
            candidate(with_hashes(
                replace("a1", "tA", 1, "x\n", "y\n"),
                "x\nz\n",
                "y\nz\n",
            )),
            candidate(with_hashes(
                replace("a2", "tA", 2, "z\n", "Z\n"),
                "y\nz\n",
                "y\nZ\n",
            )),
            candidate(with_hashes(
                replace("b1", "tB", 1, "x\n", "y\n"),
                "x\nz\n",
                "y\nz\n",
            )),
        ],
    );
    assert_eq!(
        ambiguous(&out, 0),
        vec![set([introduced("a1")]), set([introduced("b1")])]
    );
    assert_eq!(
        agreed(&out, 1),
        (
            &Provenance {
                origins: set([introduced("a2")]),
                ..Provenance::default()
            },
            Some(ChainClass::ExactFromBase)
        )
    );
}

#[test]
fn a_verified_chain_and_a_competing_consistent_chain_are_ambiguous() {
    let out = wt(
        "A\n",
        "C\n",
        vec![
            candidate(with_hashes(
                replace("ea", "t1", 1, "A\n", "C\n"),
                "A\n",
                "C\n",
            )),
            candidate(replace("ex", "t2", 1, "X\n", "Y\n")),
            candidate(with_hashes(
                replace("eb", "t2", 2, "Y\n", "C\n"),
                "Y\n",
                "C\n",
            )),
        ],
    );
    assert_eq!(
        ambiguous(&out, 0),
        vec![set([introduced("ea")]), set([introduced("eb")])]
    );
}

#[test]
fn an_occurrence_choice_that_misses_the_base_is_listed_not_combined() {
    let out = wt(
        "a\nx\n",
        "a\na\n",
        vec![candidate(replace("d", "t", 1, "x\n", "a\n"))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(origins(&out, 1), set([introduced("d")]));
    assert_eq!(out.set_aside.len(), 1);
    assert_eq!(agreed(&out, 1).1, Some(ChainClass::ExactFromBase));
}

#[test]
fn an_occurrence_choice_whose_branches_use_different_edits_is_ambiguous() {
    let out = wt(
        "a\nx\n",
        "a\na\n",
        vec![
            candidate(with_hashes(
                replace("p", "t", 1, "q\n", "a\n"),
                "x\nq\n",
                "x\na\n",
            )),
            candidate(replace("d", "t", 2, "x\n", "a\n")),
        ],
    );
    assert_eq!(
        ambiguous(&out, 1),
        vec![set([introduced("p")]), set([introduced("d")])]
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn the_whole_sequence_and_the_deletion_alone_disagree_on_the_remaining_line() {
    let out = wt(
        "a\nb\n",
        "b\n",
        vec![
            candidate(replace("e1", "t", 1, "a\n", "x\n")),
            candidate(replace("e2", "t", 2, "b\n", "a\n")),
            candidate(replace("e3", "t", 3, "x\n", "b\n")),
            candidate(replace("e4", "t", 4, "a\n", "")),
        ],
    );
    let readings = ambiguous(&out, 0);
    assert!(readings.contains(&set([base(1)])));
    assert!(readings.contains(&set([introduced("e3")])));
}

#[test]
fn an_unknown_hash_deletion_without_a_verified_chain_is_ambiguous_across_positions() {
    // The base is never reached, so no candidate position is set aside;
    // the positions disagree on which "y" the earlier edit wrote.
    let out = wt(
        "zz\n",
        "x\ny\n",
        vec![
            candidate(replace("e1", "t", 1, "q\n", "y\n")),
            candidate(replace("e2", "t", 2, "y\n", "")),
        ],
    );
    assert!(out.set_aside.is_empty());
    assert_eq!(out.chains.len(), 4);
    let readings = ambiguous(&out, 1);
    assert!(readings.contains(&set([Origin::Unexplained])));
    assert!(readings.contains(&set([introduced("e1")])));
    assert_eq!(origins(&out, 0), set([Origin::Unexplained]));
}

#[test]
fn an_unknown_hash_deletion_with_a_chain_from_the_base_lists_the_other_positions() {
    let out = wt(
        "c\na\nb\n",
        "a\nb\n",
        vec![candidate(replace("d", "t", 1, "c\n", ""))],
    );
    assert_eq!(out.set_aside.len(), 2);
    assert_eq!(origins(&out, 0), set([base(1)]));
    assert_eq!(origins(&out, 1), set([base(2)]));
    assert_eq!(base_fates(&out, 0), set([Fate::Replaced { edit: id("d") }]));
}

#[test]
fn a_gap_before_the_chain_leaves_inherited_lines_to_content_matching() {
    // E1's recorded after-state is contradicted by a manual b -> q, so the
    // chain stops at E2; E1's whole new_text still occurs in the target.
    let out = wt(
        "a\nb\n",
        "A1\nA2\nQ\n",
        vec![
            candidate(with_hashes(
                replace("e1", "t", 1, "a\n", "A1\nA2\n"),
                "a\nb\n",
                "A1\nA2\nb\n",
            )),
            candidate(replace("e2", "t", 2, "q\n", "Q\n")),
        ],
    );
    assert_eq!(out.chains.len(), 1);
    assert_eq!(out.chains[0].class, ChainClass::Consistent);
    assert_eq!(origins(&out, 0), set([Origin::Unexplained]));
    assert_eq!(origins(&out, 2), set([introduced("e2")]));
    assert_eq!(out.content.target[0], set([id("e1")]));
    assert_eq!(out.content.target[1], set([id("e1")]));
}

#[test]
fn content_matches_cover_whole_lines_and_ignore_blocks_without_letters() {
    let out = wt(
        "old line\nkeep\n",
        "fn a() {\n    let x = 1;\n}\nmid\n}\n",
        vec![
            candidate(unknown_op(
                "whole",
                "t",
                1,
                "old line\n",
                "fn a() {\n    let x = 1;\n}\n",
            )),
            candidate(unknown_op("part", "t", 2, "k", "x")),
            candidate(unknown_op("brace", "t", 3, "z", "}\n")),
        ],
    );
    assert_eq!(out.content.target[0], set([id("whole")]));
    assert_eq!(out.content.target[1], set([id("whole")]));
    assert_eq!(out.content.target[2], set([id("whole")]));
    assert!(out.content.target[3].is_empty());
    assert!(out.content.target[4].is_empty());
    assert_eq!(out.content.base[0], set([id("whole")]));
    assert!(out.content.base[1].is_empty());
}

#[test]
fn large_file_single_session_is_exact_from_base() {
    // Over 10 KB: Claude Code records no originalFile, so no hashes.
    let original = large_file(500);
    assert!(original.len() > 10 * 1024);
    let edits = vec![
        candidate(replace(
            "e1",
            "t",
            1,
            "line 0010 of the file\n",
            "line 0010 renamed\n",
        )),
        candidate(replace(
            "e2",
            "t",
            2,
            "line 0100 of the file\n",
            "line 0100 of the file\nan inserted line\n",
        )),
        candidate(replace("e3", "t", 3, "line 0200 of the file\n", "")),
        candidate(replace(
            "e4",
            "t",
            4,
            "line 0010 renamed\n",
            "line 0010 renamed twice\n",
        )),
    ];
    let mut target = original
        .replace("line 0010 of the file\n", "line 0010 renamed twice\n")
        .replace(
            "line 0100 of the file\n",
            "line 0100 of the file\nan inserted line\n",
        );
    target = target.replace("line 0200 of the file\n", "");
    let out = wt(&original, &target, edits);
    assert_eq!(out.status, PathStatus::Composed);
    assert_eq!(out.chains.len(), 1);
    assert_eq!(out.chains[0].class, ChainClass::ExactFromBase);
    assert_eq!(origins(&out, 10), set([introduced("e4")]));
    assert_eq!(agreed(&out, 10).0.earlier, set([id("e1")]));
    assert_eq!(origins(&out, 101), set([introduced("e2")]));
    assert_eq!(origins(&out, 100), set([base(100)]));
    assert_eq!(
        base_fates(&out, 200),
        set([Fate::Replaced { edit: id("e3") }])
    );
}

#[test]
fn large_file_with_a_manual_change_is_consistent() {
    let original = large_file(500);
    let target = original
        .replace("line 0010 of the file\n", "line 0010 renamed\n")
        .replace("line 0150 of the file\n", "line 0150 edited by hand\n");
    let out = wt(
        &original,
        &target,
        vec![candidate(replace(
            "e1",
            "t",
            1,
            "line 0010 of the file\n",
            "line 0010 renamed\n",
        ))],
    );
    assert!(out.chains.iter().all(|c| c.class == ChainClass::Consistent));
    assert_eq!(
        agreed(&out, 10),
        (
            &Provenance {
                origins: set([introduced("e1")]),
                ..Provenance::default()
            },
            Some(ChainClass::Consistent)
        )
    );
    assert_eq!(origins(&out, 150), set([Origin::Unexplained]));
}

/// `lines` distinct lines of 22 bytes: line `i` reads `line {i:04} of the file`.
fn large_file(lines: usize) -> String {
    let mut text = String::new();
    for i in 0..lines {
        writeln!(text, "line {i:04} of the file").unwrap();
    }
    text
}

#[test]
fn an_unknown_operation_stops_the_walk() {
    let out = wt(
        "a\n",
        "b\n",
        vec![candidate(unknown_op("u", "t", 1, "a\n", "b\n"))],
    );
    assert_eq!(
        out.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::OperationUnknown { edit: id("u") }]
        }
    );
}

#[test]
fn an_overwrite_whose_previous_content_is_not_the_base_stops_but_content_matches() {
    let out = wt(
        "old\n",
        "new content\n",
        vec![candidate(overwrite("w", "t", 1, "new content\n"))],
    );
    assert_eq!(
        out.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::WholeFileWrite { edit: id("w") }]
        }
    );
    assert_eq!(out.content.target[0], set([id("w")]));
}

#[test]
fn a_replace_all_whose_joint_inverse_fails_stops() {
    let out = wt(
        "aa",
        "aaa",
        vec![candidate(with_hashes(
            replace_all("r", "t", 1, "a", "aa"),
            "aa",
            "aaa",
        ))],
    );
    assert_eq!(
        out.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::ReplaceAllUnverified { edit: id("r") }]
        }
    );
}

#[test]
fn an_exhausted_budget_leaves_nothing_exact() {
    let mut req = request(
        Some("c\na\nb\n"),
        Some("a\nb\n"),
        TargetKind::WorkingTree,
        vec![candidate(replace("d", "t", 1, "c\n", ""))],
    );
    req.budget_bytes = 20;
    assert_eq!(attribute(req).status, PathStatus::SearchIncomplete);
}

#[test]
fn a_join_then_a_split_keeps_each_characters_own_history() {
    // From a b: E changes a to A, J joins the lines, S splits them again.
    // Provenance travels with the characters, so the b line never names E.
    let out = wt(
        "a\nb\n",
        " A\n b\n",
        vec![
            candidate(replace("E", "t", 1, "a\n", "A\n")),
            candidate(replace("J", "t", 2, "A\nb\n", "A b\n")),
            candidate(replace("S", "t", 3, "A b\n", " A\n b\n")),
        ],
    );
    let (first, _) = agreed(&out, 0);
    assert_eq!(first.origins, set([introduced("E")]));
    assert_eq!(first.whitespace_only, set([id("J"), id("S")]));
    let (second, _) = agreed(&out, 1);
    assert_eq!(second.origins, set([base(1)]));
    assert_eq!(second.whitespace_only, set([id("J"), id("S")]));
    assert!(!second.earlier.contains(&id("E")));
}

#[test]
fn chains_naming_different_whitespace_contributors_are_ambiguous() {
    // Two transcripts each indent the line; both chains say base line 0,
    // but through different edits, so nothing is agreed.
    let out = wt(
        "a\n",
        " a\n",
        vec![
            candidate(with_hashes(
                replace("F", "t1", 1, "a\n", " a\n"),
                "a\n",
                " a\n",
            )),
            candidate(with_hashes(
                replace("G", "t2", 1, "a\n", " a\n"),
                "a\n",
                " a\n",
            )),
        ],
    );
    let LineOutcome::Ambiguous { readings } = &out.target[0] else {
        panic!("line 0 is {:?}", out.target[0]);
    };
    let contributors: Vec<(BTreeSet<Origin>, BTreeSet<EventId>)> = readings
        .iter()
        .map(|r| (r.value.origins.clone(), r.value.whitespace_only.clone()))
        .collect();
    assert_eq!(
        contributors,
        vec![
            (set([base(0)]), set([id("F")])),
            (set([base(0)]), set([id("G")])),
        ]
    );
    assert!(matches!(out.base[0], BaseLineOutcome::Ambiguous { .. }));
}

#[test]
fn mixed_alignment_choices_make_the_joined_line_ambiguous() {
    // Replace-all a -> x, the y pair replaced by an x pair, each x pair
    // contracted to one x, then the two joined: the contraction's pairs
    // choose their surviving line independently, so the joined line may
    // hold E1 twice, E2 twice, or one of each.
    let states = [
        "a\ny\ny\na\n",
        "x\ny\ny\nx\n",
        "x\nx\nx\nx\n",
        "x\nx\n",
        "x x\n",
    ];
    let out = wt(
        states[0],
        states[4],
        vec![
            candidate(with_hashes(
                replace_all("e1", "t", 1, "a", "x"),
                states[0],
                states[1],
            )),
            candidate(with_hashes(
                replace("e2", "t", 2, "y\ny\n", "x\nx\n"),
                states[1],
                states[2],
            )),
            candidate(with_hashes(
                replace_all("e3", "t", 3, "x\nx\n", "x\n"),
                states[2],
                states[3],
            )),
            candidate(with_hashes(
                replace("e4", "t", 4, "x\nx\n", "x x\n"),
                states[3],
                states[4],
            )),
        ],
    );
    let readings = ambiguous(&out, 0);
    assert!(readings.contains(&set([introduced("e1")])));
    assert!(readings.contains(&set([introduced("e2")])));
    assert!(readings.contains(&set([introduced("e1"), introduced("e2")])));
}

#[test]
fn content_matching_skips_lines_a_chain_explains() {
    // The chain explains line 0, so its whole-block match is not needed.
    let out = wt(
        "a\n",
        "b\n",
        vec![candidate(replace("e1", "t", 1, "a\n", "b\n"))],
    );
    assert_eq!(origins(&out, 0), set([introduced("e1")]));
    assert!(out.content.target[0].is_empty());
}

#[test]
fn content_matching_is_skipped_when_the_search_is_incomplete() {
    let mut req = request(
        Some("a\n"),
        Some("b\n"),
        TargetKind::WorkingTree,
        vec![candidate(unknown_op("u", "t", 1, "a\n", "b\n"))],
    );
    req.budget_bytes = 3;
    let out = attribute(req);
    assert_eq!(out.status, PathStatus::SearchIncomplete);
    assert!(out.content.target.iter().all(BTreeSet::is_empty));
}

#[test]
fn content_matching_a_huge_target_is_linear() {
    // 1 MiB of identical lines and a one-line block: every line is a match,
    // found without comparing every occurrence against every line.
    let target = "a\n".repeat(524_288);
    let req = request(
        Some("b\n"),
        Some(&target),
        TargetKind::WorkingTree,
        vec![candidate(unknown_op("u", "t", 1, "q\n", "a\n"))],
    );
    let out = finishes_within(Duration::from_secs(60), move || attribute(req));
    assert_eq!(out.content.target.len(), 524_288);
    assert!(out.content.target.iter().all(|m| m.contains(&id("u"))));
}

#[test]
fn content_matching_that_runs_out_is_marked_incomplete() {
    // The walk needs 1 KiB of the 4 KiB budget; matching has its own 4 KiB
    // and runs out while indexing 6,000 bytes of target lines.
    let target = "a\n".repeat(3_000);
    let mut req = request(
        Some("b\n"),
        Some(&target),
        TargetKind::WorkingTree,
        vec![candidate(unknown_op("u", "t", 1, "q\n", "a\n"))],
    );
    req.budget_bytes = 4096;
    let out = attribute(req);
    assert!(matches!(out.status, PathStatus::NotComposed { .. }));
    assert!(out.content.incomplete);
    assert!(!out.content.target.iter().all(|m| m.contains(&id("u"))));
}
```

- [ ] **Step 3: Write the failing property tests**

`crates/supersigil-anchor/tests/properties.rs` (failure persistence is off so nothing is written
into the source tree):

```rust
//! Property tests of anchor against an independent forward interpreter.
//!
//! The oracle in this file applies generated operations to bytes and tags
//! every byte with its source (a base offset, the operation that wrote it,
//! or an unrecorded change). It re-implements the recorder's semantics and
//! shares no code with `supersigil-anchor`.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    WT, create, in_worktree, overwrite, replace, replace_all, request, state, unknown_op,
    with_hashes, with_patch,
};
use proptest::prelude::*;
use supersigil_anchor::provenance::replay;
use supersigil_anchor::walk::{WalkedChain, walk};
use supersigil_anchor::{
    CandidateEdit, ChainClass, ChainEnd, LineOutcome, Origin, Provenance, State, TargetKind,
    attribute,
};
use supersigil_record::observations::{Content, Edit, EditOperation, FileState};
use supersigil_record::{ContentId, EventId};

// ---------------------------------------------------------------------------
// The oracle
// ---------------------------------------------------------------------------

/// Where a byte came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    /// This offset of the base.
    Base(usize),
    /// Written by the operation with this index.
    Op(usize),
    /// Written by a change nothing recorded.
    Unrecorded,
}

/// Bytes with the source of each.
#[derive(Clone, Debug)]
struct Doc {
    bytes: Vec<u8>,
    tags: Vec<Tag>,
}

impl Doc {
    fn new(text: &str, tag: impl Fn(usize) -> Tag) -> Self {
        Self {
            bytes: text.as_bytes().to_vec(),
            tags: (0..text.len()).map(tag).collect(),
        }
    }

    fn text(&self) -> String {
        String::from_utf8(self.bytes.clone()).unwrap()
    }

    /// Replaces `len` bytes at `at` with `with`, tagged `tag`.
    fn splice(&self, at: usize, len: usize, with: &str, tag: Tag) -> Self {
        let mut bytes = self.bytes[..at].to_vec();
        bytes.extend_from_slice(with.as_bytes());
        bytes.extend_from_slice(&self.bytes[at + len..]);
        let mut tags = self.tags[..at].to_vec();
        tags.extend(std::iter::repeat_n(tag, with.len()));
        tags.extend_from_slice(&self.tags[at + len..]);
        Self { bytes, tags }
    }

    /// Line spans, each ending after its newline.
    fn lines(&self) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut start = 0;
        for (i, b) in self.bytes.iter().enumerate() {
            if *b == b'\n' {
                spans.push((start, i + 1));
                start = i + 1;
            }
        }
        if start < self.bytes.len() {
            spans.push((start, self.bytes.len()));
        }
        spans
    }
}

/// The recorder's semantics, written again: an Edit replaces the first
/// occurrence (every non-overlapping one for replace-all); a Write's
/// result is its content.
fn oracle_forward(edit: &Edit, before: Option<&[u8]>) -> Option<Vec<u8>> {
    let text = |m: &supersigil_record::observations::Material<String>| {
        m.retained().map(|s| s.as_bytes().to_vec())
    };
    match edit.operation {
        EditOperation::Write => text(&edit.new_text),
        EditOperation::Unknown => None,
        EditOperation::Replace => {
            let (old, new, before) = (text(&edit.old_text)?, text(&edit.new_text)?, before?);
            if old.is_empty() {
                return None;
            }
            let mut out = Vec::new();
            let mut i = 0;
            let mut replaced = false;
            while i < before.len() {
                if (!replaced || edit.replace_all) && before[i..].starts_with(&old) {
                    out.extend_from_slice(&new);
                    i += old.len();
                    replaced = true;
                } else {
                    out.push(before[i]);
                    i += 1;
                }
            }
            replaced.then_some(out)
        }
    }
}

fn hash_matches(recorded: &FileState, bytes: Option<&[u8]>) -> bool {
    match (recorded, bytes) {
        (FileState::Absent, None)
        | (
            FileState::Present {
                content: Content::Unknown,
            },
            Some(_),
        ) => true,
        (
            FileState::Present {
                content: Content::Known(id),
            },
            Some(b),
        ) => ContentId::of(b) == *id,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Restricted histories: one transcript, whole-line operations, fresh lines
// ---------------------------------------------------------------------------

struct Built {
    base: Option<String>,
    target: String,
    edits: Vec<CandidateEdit>,
    /// Target line index of the unrecorded line, when there is one.
    unrecorded: Option<usize>,
    /// The oracle's outcome per target line.
    expected: Vec<Origin>,
}

/// One generated operation: (delete?, pick, hash known?, patch retained?).
type Op = (bool, u16, bool, bool);

/// Builds a history from generated choices. With `interruption =
/// Some((after, pick))`, a fresh line nothing records is inserted after
/// that many operations and never touched again.
fn build(base_present: bool, size: usize, ops: &[Op], interruption: Option<(u16, u16)>) -> Built {
    let mut counter = 0;
    // Tab-indented, so a retained patch shows every line in display form.
    let mut fresh = || {
        counter += 1;
        format!("\tL{counter:04}\n")
    };
    let mut edits = Vec::new();
    let (base, mut doc, mut k) = if base_present {
        let text: String = (0..size).map(|_| fresh()).collect();
        let doc = Doc::new(&text, Tag::Base);
        (Some(text), doc, 0)
    } else {
        let content: String = (0..size).map(|_| fresh()).collect();
        edits.push(candidate(create("op0", "t", 1, &content)));
        (None, Doc::new(&content, |_| Tag::Op(0)), 1)
    };
    let mut pending =
        interruption.map(|(after, pick)| (usize::from(after) % (ops.len() + 1), pick));
    for (step, &(delete, pick, known, patched)) in ops.iter().enumerate() {
        if let Some((_, at)) = pending.filter(|&(after, _)| after == step) {
            doc = interrupt(&doc, at, &mut fresh);
            pending = None;
        }
        let lines: Vec<(usize, usize)> = doc
            .lines()
            .into_iter()
            .filter(|&(s, _)| doc.tags[s] != Tag::Unrecorded)
            .collect();
        if lines.is_empty() {
            break;
        }
        let (s, e) = lines[usize::from(pick) % lines.len()];
        let old = String::from_utf8(doc.bytes[s..e].to_vec()).unwrap();
        let new = if delete { String::new() } else { fresh() };
        let next = doc.splice(s, e - s, &new, Tag::Op(k));
        let ordinal = u64::try_from(k).unwrap() + 1;
        let mut edit = replace(&format!("op{k}"), "t", ordinal, &old, &new);
        if known {
            edit = with_hashes(edit, &doc.text(), &next.text());
        }
        if patched {
            edit = with_patch(edit, &doc.text(), &next.text());
        }
        edits.push(candidate(edit));
        doc = next;
        k += 1;
    }
    // The history may end early when every line is gone; the interruption
    // then comes last.
    if let Some((_, at)) = pending {
        doc = interrupt(&doc, at, &mut fresh);
    }
    let base_starts = base
        .as_deref()
        .map(|b| Doc::new(b, Tag::Base).lines())
        .unwrap_or_default();
    let base_line = |offset: usize| {
        base_starts
            .iter()
            .position(|&(s, e)| s <= offset && offset < e)
            .unwrap()
    };
    let mut unrecorded = None;
    let expected = doc
        .lines()
        .iter()
        .enumerate()
        .map(|(line, &(s, e))| match doc.tags[s] {
            Tag::Base(o) => {
                assert!(
                    doc.tags[s..e]
                        .iter()
                        .all(|t| matches!(t, Tag::Base(x) if base_line(*x) == base_line(o)))
                );
                Origin::Base(base_line(o))
            }
            Tag::Op(k) => {
                assert!(doc.tags[s..e].iter().all(|t| *t == Tag::Op(k)));
                Origin::Introduced(EventId::new(format!("op{k}")))
            }
            Tag::Unrecorded => {
                unrecorded = Some(line);
                Origin::Unexplained
            }
        })
        .collect();
    Built {
        base,
        target: doc.text(),
        edits,
        unrecorded,
        expected,
    }
}

fn candidate(edit: Edit) -> CandidateEdit {
    in_worktree(edit, WT)
}

fn interrupt(doc: &Doc, pick: u16, fresh: &mut impl FnMut() -> String) -> Doc {
    let mut starts: Vec<usize> = doc.lines().iter().map(|&(s, _)| s).collect();
    starts.push(doc.bytes.len());
    let at = starts[usize::from(pick) % starts.len()];
    doc.splice(at, 0, &fresh(), Tag::Unrecorded)
}

fn restricted() -> impl Strategy<Value = (bool, usize, Vec<Op>)> {
    (
        any::<bool>(),
        1usize..=5,
        prop::collection::vec(
            (any::<bool>(), any::<u16>(), any::<bool>(), any::<bool>()),
            1..=4,
        ),
    )
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    /// (a) Over histories section 3 guarantees to invert unambiguously, a
    /// chain exact from the base exists and every line matches the oracle.
    #[test]
    fn completeness((base_present, size, ops) in restricted()) {
        let built = build(base_present, size, &ops, None);
        let out = attribute(request(
            built.base.as_deref(),
            Some(&built.target),
            TargetKind::WorkingTree,
            built.edits,
        ));
        prop_assert!(out.chains.iter().any(|c| c.class == ChainClass::ExactFromBase));
        for (line, expected) in built.expected.iter().enumerate() {
            match &out.target[line] {
                LineOutcome::Agreed { provenance, .. } => {
                    prop_assert_eq!(&provenance.origins, &BTreeSet::from([expected.clone()]));
                }
                other @ LineOutcome::Ambiguous { .. } => {
                    prop_assert!(false, "line {} is {:?}", line, other);
                }
            }
        }
    }

    /// (b) With one observable unrecorded line, nothing is exact from the
    /// base and that line is never attributed to a recorded edit.
    #[test]
    fn soundness(
        (base_present, size, ops) in restricted(),
        after in any::<u16>(),
        pick in any::<u16>(),
    ) {
        let built = build(base_present, size, &ops, Some((after, pick)));
        let line = built.unrecorded.expect("the interruption survives");
        let out = attribute(request(
            built.base.as_deref(),
            Some(&built.target),
            TargetKind::WorkingTree,
            built.edits,
        ));
        prop_assert!(out.chains.iter().all(|c| c.class != ChainClass::ExactFromBase));
        let claims: Vec<&BTreeSet<Origin>> = match &out.target[line] {
            LineOutcome::Agreed { provenance, .. } => vec![&provenance.origins],
            LineOutcome::Ambiguous { readings } => readings.iter().map(|r| &r.value.origins).collect(),
        };
        for origins in claims {
            prop_assert!(!origins.iter().any(|o| matches!(o, Origin::Introduced(_))));
        }
    }
}

// ---------------------------------------------------------------------------
// Unrestricted histories: duplicates, whitespace, replace-all, writes,
// unknown operations, two transcripts and worktrees, equal ordinals
// ---------------------------------------------------------------------------

// "\ta\n" and "  a\n" look the same in a patch.
const ALPHABET: [&str; 7] = ["a\n", "b\n", "a b\n", "ab\n", "\n", "  a\n", "\ta\n"];

#[derive(Clone, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent generated choice"
)]
struct OpChoice {
    kind: u8,
    start: u16,
    len: u8,
    replacement: Vec<u8>,
    transcript: bool,
    same_ordinal: bool,
    known: bool,
    patched: bool,
}

fn op_choice() -> impl Strategy<Value = OpChoice> {
    (
        0u8..5,
        any::<u16>(),
        1u8..6,
        prop::collection::vec(0u8..7, 0..3),
        any::<bool>(),
        prop::bool::weighted(0.2),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(kind, start, len, replacement, transcript, same_ordinal, known, patched)| OpChoice {
                kind,
                start,
                len,
                replacement,
                transcript,
                same_ordinal,
                known,
                patched,
            },
        )
}

struct Unrestricted {
    base: Option<String>,
    target: Option<String>,
    edits: Vec<CandidateEdit>,
    kind: TargetKind,
}

fn build_unrestricted(
    base: Option<Vec<u8>>,
    ops: &[OpChoice],
    second_worktree: bool,
    commit: bool,
) -> Unrestricted {
    let lines =
        |picks: &[u8]| -> String { picks.iter().map(|&i| ALPHABET[usize::from(i)]).collect() };
    let base = base.map(|picks| lines(&picks));
    let mut current: Option<String> = base.clone();
    let mut edits = Vec::new();
    let mut ordinals: BTreeMap<bool, u64> = BTreeMap::new();
    for (k, op) in ops.iter().enumerate() {
        let id = format!("op{k}");
        let transcript = if op.transcript { "t2" } else { "t1" };
        let replacement = lines(&op.replacement);
        let edit = match (op.kind, current.as_deref()) {
            (_, None) => create(&id, transcript, 0, &replacement),
            (0 | 1 | 4, Some(text)) if !text.is_empty() => {
                let start = usize::from(op.start) % text.len();
                let end = (start + usize::from(op.len)).min(text.len());
                let Some(old) = text.get(start..end) else {
                    continue;
                };
                match op.kind {
                    0 => replace(&id, transcript, 0, old, &replacement),
                    1 => replace_all(&id, transcript, 0, old, &replacement),
                    _ => unknown_op(&id, transcript, 0, old, &replacement),
                }
            }
            (2, Some(_)) => overwrite(&id, transcript, 0, &replacement),
            _ => continue,
        };
        let before = current.as_deref().map(str::as_bytes);
        let after = if edit.operation == EditOperation::Unknown {
            // An unknown operation still happened: apply it as a replacement.
            let mut as_replace = edit.clone();
            as_replace.operation = EditOperation::Replace;
            oracle_forward(&as_replace, before)
        } else {
            oracle_forward(&edit, before)
        };
        let Some(after) = after else { continue };
        let after = String::from_utf8(after).unwrap();
        let ordinal = ordinals.entry(op.transcript).or_insert(0);
        if !op.same_ordinal || *ordinal == 0 {
            *ordinal += 1;
        }
        let mut edit = edit;
        edit.source_ordinal = *ordinal;
        if op.known && edit.operation != EditOperation::Write {
            edit.before = current.as_deref().map_or(FileState::Absent, |b| {
                FileState::known(ContentId::of(b.as_bytes()))
            });
            edit.after = FileState::known(ContentId::of(after.as_bytes()));
        }
        // Claude Code records no patch for a file a Write creates.
        if let (true, Some(before)) = (op.patched, current.as_deref()) {
            edit = with_patch(edit, before, &after);
        }
        let worktree = if second_worktree && op.transcript {
            "/work/w1"
        } else {
            WT
        };
        edits.push(in_worktree(edit, worktree));
        current = Some(after);
    }
    Unrestricted {
        base,
        target: current,
        edits,
        kind: if commit {
            TargetKind::Commit
        } else {
            TargetKind::WorkingTree
        },
    }
}

fn unrestricted() -> impl Strategy<Value = (Option<Vec<u8>>, Vec<OpChoice>, bool, bool)> {
    (
        prop::option::of(prop::collection::vec(0u8..7, 0..5)),
        prop::collection::vec(op_choice(), 1..5),
        any::<bool>(),
        any::<bool>(),
    )
}

fn edit_ids(chain: &WalkedChain) -> BTreeSet<EventId> {
    chain.chain.edits.iter().cloned().collect()
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    /// (c) Everything anchor reports is justified by its own evidence.
    #[test]
    fn justification((base, ops, second, commit) in unrestricted()) {
        let built = build_unrestricted(base, &ops, second, commit);
        let req = request(built.base.as_deref(), built.target.as_deref(), built.kind, built.edits);
        let by_id: BTreeMap<EventId, Edit> = req
            .edits
            .iter()
            .map(|c| (c.edit.id.clone(), c.edit.clone()))
            .collect();
        let walked = walk(&req);
        for chain in walked.chains.iter().chain(&walked.set_aside) {
            for step in &chain.steps {
                let edit = &by_id[&step.edit];
                let forward = oracle_forward(edit, step.before.bytes());
                prop_assert_eq!(forward.as_deref(), step.after.bytes());
                prop_assert!(hash_matches(&edit.before, step.before.bytes()));
                prop_assert!(hash_matches(&edit.after, step.after.bytes()));
            }
            let from_base = chain.chain.end == ChainEnd::Base;
            prop_assert_eq!(from_base, chain.chain.class == ChainClass::ExactFromBase);
            if from_base {
                prop_assert_eq!(&chain.start, &req.base);
                let mut bytes = req.base.bytes().map(<[u8]>::to_vec);
                for step in &chain.steps {
                    bytes = oracle_forward(&by_id[&step.edit], bytes.as_deref());
                }
                prop_assert_eq!(bytes.as_deref(), req.target.bytes());
            }
        }
        for aside in &walked.set_aside {
            prop_assert!(aside.chain.end != ChainEnd::Base);
            let covered = walked.chains.iter().any(|c| {
                c.chain.end == ChainEnd::Base && edit_ids(aside).is_subset(&edit_ids(c))
            });
            prop_assert!(covered, "set aside without a covering chain: {:?}", aside.chain);
        }
        let base_lines = common::line_count(&req.base);
        let replayed: Vec<_> = walked.chains.iter().map(|c| replay(c, base_lines)).collect();
        let out = attribute(req);
        prop_assert_eq!(out.chains.len(), walked.chains.len());
        for (line, outcome) in out.target.iter().enumerate() {
            // Agreement is on the full provenance: origins and contributors.
            let all: Vec<&Provenance> = replayed.iter().map(|p| &p.target[line]).collect();
            let agree = all.windows(2).all(|w| w[0] == w[1]);
            match outcome {
                LineOutcome::Agreed { provenance, .. } => {
                    prop_assert!(agree);
                    if let Some(first) = all.first() {
                        prop_assert_eq!(provenance, *first);
                    }
                }
                LineOutcome::Ambiguous { .. } => prop_assert!(!agree),
            }
        }
    }
}

#[test]
fn the_oracle_replaces_the_first_occurrence_like_the_recorder() {
    let e = replace("e", "t", 1, "x", "y");
    assert_eq!(
        oracle_forward(&e, Some(b"x\nx\n")),
        Some(b"y\nx\n".to_vec())
    );
    let all = replace_all("e", "t", 1, "a", "aa");
    assert_eq!(oracle_forward(&all, Some(b"aba")), Some(b"aabaa".to_vec()));
    assert_eq!(state(Some("a")), State::Present(b"a".to_vec()));
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-anchor --test attribute --test properties`
Expected: compile errors, for example ``cannot find function `attribute` in crate `supersigil_anchor` ``
and ``unresolved import `supersigil_anchor::provenance` ``.

- [ ] **Step 5: Add the line outcome types**

Replace `crates/supersigil-anchor/src/result.rs` with:

```rust
//! Results of attribution: why walks stop, the chains they find, and what
//! each line's history is.

use std::collections::BTreeSet;

use supersigil_record::{EventId, RecordId};

use crate::walk::AcceptedEdit;

/// Why reversing an edit, or a whole chain, stopped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopReason {
    /// The edit's operation was not recorded.
    OperationUnknown {
        /// The edit.
        edit: EventId,
    },
    /// The replacement texts the reversal needs were not retained.
    TextUnavailable {
        /// The edit.
        edit: EventId,
    },
    /// The edit's replacement text could not be located in the current bytes.
    NotLocatable {
        /// The edit.
        edit: EventId,
    },
    /// A whole-file write whose previous content was not recorded.
    WholeFileWrite {
        /// The edit.
        edit: EventId,
    },
    /// A replace-all edit whose joint inverse could not be verified.
    ReplaceAllUnverified {
        /// The edit.
        edit: EventId,
    },
    /// The current bytes contradict the edit's recorded after-state.
    AfterHashMismatch {
        /// The edit.
        edit: EventId,
    },
    /// The current bytes contradict the lines the edit's retained patch
    /// shows after it.
    AfterPatchMismatch {
        /// The edit.
        edit: EventId,
    },
    /// Every candidate before-state failed its own checks.
    NoAcceptedCandidate {
        /// The edit.
        edit: EventId,
    },
    /// No recorded edit could precede the chain's start.
    NoPredecessor,
}

/// What verifies a chain, strongest first: `min()` of several is the
/// strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainClass {
    /// Replaying the chain from the base reproduces the target.
    ExactFromBase,
    /// Every step's recorded hashes verify, but the chain starts elsewhere.
    ExactFromStart,
    /// Every step reversed, but something is unverified and the chain does
    /// not start at the base.
    Consistent,
}

/// Where a chain's walk ended.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChainEnd {
    /// At a state equal to the base.
    Base,
    /// Where no predecessor was accepted, with the reasons.
    Stopped {
        /// Why each candidate predecessor was rejected.
        reasons: Vec<StopReason>,
    },
}

/// One reading of recorded edits that reproduces the target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Chain {
    /// Identifier within one path's result.
    pub id: usize,
    /// What verifies the chain.
    pub class: ChainClass,
    /// The newest edit: the one that produced the target.
    pub head: EventId,
    /// The chain's edits, oldest first.
    pub edits: Vec<EventId>,
    /// Where the walk ended.
    pub end: ChainEnd,
}

/// The outcome of the search for one path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathStatus {
    /// The search completed and found at least one chain.
    Composed,
    /// The search completed and no head could be reversed.
    NotComposed {
        /// Why each head was rejected.
        reasons: Vec<StopReason>,
    },
    /// The work budget ran out; nothing on the path is exact.
    SearchIncomplete,
}

/// An edit whose sightings in several records disagree; it is excluded.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Conflict {
    /// The edit.
    pub edit: EventId,
    /// Every record that holds a sighting of it.
    pub records: Vec<RecordId>,
}

/// Where a target line's content came from, in one chain.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Origin {
    /// The content of this base line (0-based).
    Base(usize),
    /// Written by this edit.
    Introduced(EventId),
    /// A line of only whitespace that this whitespace-only change added.
    WhitespaceAdded(EventId),
    /// Inherited from before the chain's start: the chain does not explain it.
    Unexplained,
}

/// A target line's origins and contributors, in one chain or agreed by all.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Provenance {
    /// Where the line's content came from.
    pub origins: BTreeSet<Origin>,
    /// Edits that changed only whitespace on the way. Removing or adding
    /// whitespace can still change behavior inside strings or in
    /// indentation-sensitive code, so these are observations, not a claim
    /// that only presentation changed.
    pub whitespace_only: BTreeSet<EventId>,
    /// Edits that participated earlier in the line's region.
    pub earlier: BTreeSet<EventId>,
}

/// What happened to a base line, in one chain.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fate {
    /// Removed by this edit outside a whitespace-only change.
    Replaced {
        /// The edit.
        edit: EventId,
    },
    /// Its characters reached this target line through whitespace-only edits.
    CarriedTo {
        /// The target line (0-based).
        line: usize,
        /// The whitespace-only edits recorded on that line.
        via: BTreeSet<EventId>,
    },
    /// A line of only whitespace removed by this whitespace-only change.
    RemovedByWhitespace {
        /// The edit.
        edit: EventId,
    },
    /// Unchanged, as this target line (0-based).
    KeptAs {
        /// The target line.
        line: usize,
    },
}

/// One chain's reading of a line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Reading<T> {
    /// The chain's id.
    pub chain: usize,
    /// The chain's class.
    pub class: ChainClass,
    /// What the chain says about the line.
    pub value: T,
}

/// The combined outcome of a target line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineOutcome {
    /// Every chain gives the identical provenance: origins, whitespace-only
    /// and earlier contributors. `class` is the strongest among them, or
    /// `None` when there is no chain (origins then `Unexplained`).
    Agreed {
        /// The provenance every chain gives.
        provenance: Provenance,
        /// The strongest class among the chains.
        class: Option<ChainClass>,
    },
    /// The chains disagree; each reading is listed.
    Ambiguous {
        /// Every chain's reading.
        readings: Vec<Reading<Provenance>>,
    },
}

/// The combined outcome of a base line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BaseLineOutcome {
    /// Every chain gives the same fates; empty fates mean no fate.
    Agreed {
        /// The agreed fates.
        fates: BTreeSet<Fate>,
        /// The strongest class among the chains, `None` without chains.
        class: Option<ChainClass>,
    },
    /// The chains disagree; each reading is listed.
    Ambiguous {
        /// Every chain's reading.
        readings: Vec<Reading<BTreeSet<Fate>>>,
    },
}

/// Edits whose retained text occurs as a whole block over each line, for
/// the lines that need the fallback.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct ContentMatches {
    /// Per target line, edits whose `new_text` (or written content) covers it.
    pub target: Vec<BTreeSet<EventId>>,
    /// Per base line, edits whose `old_text` covers it.
    pub base: Vec<BTreeSet<EventId>>,
    /// Whether matching ran out of budget before finishing: lines without a
    /// match may then have one that was never looked for.
    pub incomplete: bool,
}

/// Everything anchor established about one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathAttribution {
    /// Whether the search completed and found anything.
    pub status: PathStatus,
    /// Chains whose readings were combined.
    pub chains: Vec<Chain>,
    /// Alternatives that assume an unrecorded change.
    pub set_aside: Vec<Chain>,
    /// Edits excluded as conflicting evidence.
    pub conflicts: Vec<Conflict>,
    /// The deduplicated edits, for the review's edit details.
    pub accepted: Vec<AcceptedEdit>,
    /// One outcome per target line.
    pub target: Vec<LineOutcome>,
    /// One outcome per base line.
    pub base: Vec<BaseLineOutcome>,
    /// Whole-block content matches per line.
    pub content: ContentMatches,
}
```

- [ ] **Step 6: Write the replay**

`crates/supersigil-anchor/src/provenance.rs`:

```rust
//! Replays one chain forward to give every target line its origins and
//! every base line its fates.
//!
//! Inside the replay, provenance travels with characters: each line carries
//! runs over its non-whitespace characters, each run with an origin,
//! whitespace-only contributors, and earlier contributors; a line without
//! any non-whitespace character carries its own. Lines of the chain's start
//! are base lines when the chain ends at the base, and unexplained
//! otherwise. Kept lines keep their runs. In a changed region whose removed
//! and added text are equal once all whitespace is deleted, characters map
//! one to one through the whitespace-free text, so each keeps its run's
//! attributes and gains the edit as a whitespace-only contributor; base-line
//! identity passes through. In any other region, every added character is
//! introduced by the edit, with earlier contributors from the removed
//! region's characters, and each removed base line is replaced by the edit.
//! Runs aggregate into per-line sets only in the output.

use std::collections::BTreeSet;

use supersigil_record::EventId;

use crate::lines::split_lines;
use crate::result::{ChainEnd, Fate, Origin, Provenance};
use crate::walk::{ChainStep, WalkedChain};

/// One chain's provenance for every target line and fates for every base
/// line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainProvenance {
    /// One provenance per target line.
    pub target: Vec<Provenance>,
    /// One set of fates per base line; all empty unless the chain ends at
    /// the base.
    pub base: Vec<BTreeSet<Fate>>,
}

/// What a run of characters, or a line without any, carries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Attrs {
    origin: Origin,
    whitespace_only: BTreeSet<EventId>,
    earlier: BTreeSet<EventId>,
}

impl Attrs {
    fn new(origin: Origin) -> Self {
        Self {
            origin,
            whitespace_only: BTreeSet::new(),
            earlier: BTreeSet::new(),
        }
    }
}

/// A line's provenance during the replay.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    /// Runs of (number of non-whitespace characters, their attributes).
    Runs(Vec<(usize, Attrs)>),
    /// A line without non-whitespace characters, and what it carries.
    Bare(Attrs),
}

impl Line {
    /// A line of `count` non-whitespace characters, all with `attrs`.
    fn uniform(count: usize, attrs: Attrs) -> Self {
        if count == 0 {
            Self::Bare(attrs)
        } else {
            Self::Runs(vec![(count, attrs)])
        }
    }

    /// Every attribute set the line carries: its runs', or its own.
    fn attrs(&self) -> Vec<&Attrs> {
        match self {
            Self::Runs(runs) => runs.iter().map(|(_, attrs)| attrs).collect(),
            Self::Bare(attrs) => vec![attrs],
        }
    }

    /// Aggregates the line into sets.
    fn provenance(&self) -> Provenance {
        let mut provenance = Provenance::default();
        for attrs in self.attrs() {
            provenance.origins.insert(attrs.origin.clone());
            provenance
                .whitespace_only
                .extend(attrs.whitespace_only.iter().cloned());
            provenance.earlier.extend(attrs.earlier.iter().cloned());
        }
        provenance
    }
}

/// Replays `chain` forward from its start. `base_lines` is the number of
/// lines of the base state.
#[must_use]
pub fn replay(chain: &WalkedChain, base_lines: usize) -> ChainProvenance {
    let from_base = chain.chain.end == ChainEnd::Base;
    let mut lines: Vec<Line> = split_lines(chain.start.bytes().unwrap_or_default())
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let origin = if from_base {
                Origin::Base(i)
            } else {
                Origin::Unexplained
            };
            Line::uniform(non_whitespace(text), Attrs::new(origin))
        })
        .collect();
    let mut fates: Vec<BTreeSet<Fate>> = vec![BTreeSet::new(); base_lines];
    for step in &chain.steps {
        lines = apply(step, &lines, &mut fates);
    }
    for (line, provenance) in lines.iter().enumerate() {
        for attrs in provenance.attrs() {
            if let Origin::Base(j) = attrs.origin {
                let fate = if attrs.whitespace_only.is_empty() {
                    Fate::KeptAs { line }
                } else {
                    Fate::CarriedTo {
                        line,
                        via: attrs.whitespace_only.clone(),
                    }
                };
                fates[j].insert(fate);
            }
        }
    }
    ChainProvenance {
        target: lines.iter().map(Line::provenance).collect(),
        base: fates,
    }
}

/// Applies one step: returns the after-state's lines and records the base
/// fates the step decides.
fn apply(step: &ChainStep, lines: &[Line], fates: &mut [BTreeSet<Fate>]) -> Vec<Line> {
    let before = split_lines(step.before.bytes().unwrap_or_default());
    let after = split_lines(step.after.bytes().unwrap_or_default());
    let mut next = vec![Line::Bare(Attrs::new(Origin::Unexplained)); after.len()];
    for &(b, a) in &step.alignment.kept {
        next[a] = lines[b].clone();
    }
    for region in &step.alignment.regions {
        let removed: Vec<usize> = region.before.clone().collect();
        let removed_text: Vec<&[u8]> = removed.iter().map(|&r| before[r]).collect();
        let added_text: Vec<&[u8]> = region.after.clone().map(|a| after[a]).collect();
        let edit = &step.edit;
        let new_lines = if squeeze(&removed_text) == squeeze(&added_text) {
            whitespace_only(edit, &removed, &added_text, lines, fates)
        } else {
            replaced(edit, &removed, &added_text, lines, fates)
        };
        for (a, line) in region.after.clone().zip(new_lines) {
            next[a] = line;
        }
    }
    next
}

/// The characters of `text` with every whitespace character deleted.
fn squeeze(text: &[&[u8]]) -> String {
    text.iter()
        .flat_map(|line| {
            String::from_utf8_lossy(line)
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn non_whitespace(line: &[u8]) -> usize {
    String::from_utf8_lossy(line)
        .chars()
        .filter(|c| !c.is_whitespace())
        .count()
}

/// A whitespace-only region: characters keep their attributes, one to one.
fn whitespace_only(
    edit: &EventId,
    removed: &[usize],
    added_text: &[&[u8]],
    lines: &[Line],
    fates: &mut [BTreeSet<Fate>],
) -> Vec<Line> {
    // The removed characters' runs, in order.
    let mut source: Vec<(usize, Attrs)> = Vec::new();
    for &r in removed {
        match &lines[r] {
            Line::Runs(runs) => source.extend(runs.iter().cloned()),
            Line::Bare(attrs) => {
                if let Origin::Base(j) = attrs.origin {
                    fates[j].insert(Fate::RemovedByWhitespace { edit: edit.clone() });
                }
            }
        }
    }
    let mut source = source.into_iter();
    let mut pending: Option<(usize, Attrs)> = None;
    let mut out = Vec::with_capacity(added_text.len());
    for text in added_text {
        let mut needed = non_whitespace(text);
        if needed == 0 {
            let mut attrs = Attrs::new(Origin::WhitespaceAdded(edit.clone()));
            attrs.whitespace_only.insert(edit.clone());
            out.push(Line::Bare(attrs));
            continue;
        }
        let mut runs: Vec<(usize, Attrs)> = Vec::new();
        while needed > 0 {
            let (count, attrs) = pending
                .take()
                .or_else(|| source.next())
                .expect("the whitespace-free texts are equal, so the characters match");
            let taken = count.min(needed);
            if count > taken {
                pending = Some((count - taken, attrs.clone()));
            }
            let mut attrs = attrs;
            attrs.whitespace_only.insert(edit.clone());
            match runs.last_mut() {
                Some((n, last)) if *last == attrs => *n += taken,
                _ => runs.push((taken, attrs)),
            }
            needed -= taken;
        }
        out.push(Line::Runs(runs));
    }
    out
}

/// Any other region: every added character is introduced by the edit.
fn replaced(
    edit: &EventId,
    removed: &[usize],
    added_text: &[&[u8]],
    lines: &[Line],
    fates: &mut [BTreeSet<Fate>],
) -> Vec<Line> {
    let mut earlier = BTreeSet::new();
    for &r in removed {
        if let Line::Runs(runs) = &lines[r] {
            for (_, attrs) in runs {
                if let Origin::Introduced(id) | Origin::WhitespaceAdded(id) = &attrs.origin {
                    earlier.insert(id.clone());
                }
                earlier.extend(attrs.whitespace_only.iter().cloned());
                earlier.extend(attrs.earlier.iter().cloned());
            }
        }
        for attrs in lines[r].attrs() {
            if let Origin::Base(j) = attrs.origin {
                fates[j].insert(Fate::Replaced { edit: edit.clone() });
            }
        }
    }
    added_text
        .iter()
        .map(|text| {
            let mut attrs = Attrs::new(Origin::Introduced(edit.clone()));
            attrs.earlier.clone_from(&earlier);
            Line::uniform(non_whitespace(text), attrs)
        })
        .collect()
}
```

- [ ] **Step 7: Write the combination**

`crates/supersigil-anchor/src/combine.rs`:

```rust
//! Combines the chains of one path line by line.
//!
//! Chains are alternative histories of the same bytes, not complementary
//! pieces. A target line is agreed only when every chain gives it an
//! identical provenance: the same origins, the same whitespace-only
//! contributors, and the same earlier contributors. "Unexplained" is an
//! origin like any other, so a line one chain explains and another does not
//! is ambiguous, and so is a line two chains explain through different
//! edits. A base line is agreed only when every chain gives it the same
//! fates. Nothing is unioned across chains: competing contributors are
//! never presented as a joint one.

use std::collections::BTreeSet;

use crate::provenance::ChainProvenance;
use crate::result::{BaseLineOutcome, Chain, ChainClass, LineOutcome, Origin, Provenance, Reading};

/// Combines the provenance of `chains` into one outcome per line.
///
/// Agreed lines carry the provenance every chain gives them and the
/// strongest class among the chains; otherwise every chain's full
/// provenance is listed. Without chains, every target line is agreed
/// unexplained and every base line has no fate, with no class.
#[must_use]
pub fn combine(
    chains: &[(&Chain, ChainProvenance)],
    target_lines: usize,
    base_lines: usize,
) -> (Vec<LineOutcome>, Vec<BaseLineOutcome>) {
    let strongest = chains.iter().map(|(chain, _)| chain.class).min();
    let target = (0..target_lines)
        .map(|line| target_outcome(chains, line, strongest))
        .collect();
    let base = (0..base_lines)
        .map(|line| base_outcome(chains, line, strongest))
        .collect();
    (target, base)
}

fn target_outcome(
    chains: &[(&Chain, ChainProvenance)],
    line: usize,
    strongest: Option<ChainClass>,
) -> LineOutcome {
    let Some((_, first)) = chains.first() else {
        return LineOutcome::Agreed {
            provenance: Provenance {
                origins: BTreeSet::from([Origin::Unexplained]),
                ..Provenance::default()
            },
            class: None,
        };
    };
    let provenance = &first.target[line];
    if chains.iter().all(|(_, p)| p.target[line] == *provenance) {
        return LineOutcome::Agreed {
            provenance: provenance.clone(),
            class: strongest,
        };
    }
    LineOutcome::Ambiguous {
        readings: chains
            .iter()
            .map(|(chain, p)| Reading {
                chain: chain.id,
                class: chain.class,
                value: p.target[line].clone(),
            })
            .collect(),
    }
}

fn base_outcome(
    chains: &[(&Chain, ChainProvenance)],
    line: usize,
    strongest: Option<ChainClass>,
) -> BaseLineOutcome {
    let Some((_, first)) = chains.first() else {
        return BaseLineOutcome::Agreed {
            fates: BTreeSet::new(),
            class: None,
        };
    };
    let fates = &first.base[line];
    if chains.iter().all(|(_, p)| p.base[line] == *fates) {
        return BaseLineOutcome::Agreed {
            fates: fates.clone(),
            class: strongest,
        };
    }
    BaseLineOutcome::Ambiguous {
        readings: chains
            .iter()
            .map(|(chain, p)| Reading {
                chain: chain.id,
                class: chain.class,
                value: p.base[line].clone(),
            })
            .collect(),
    }
}
```

- [ ] **Step 8: Write the content-match fallback**

`crates/supersigil-anchor/src/content.rs`:

```rust
//! The content-match fallback: whole edit texts found verbatim, for the
//! lines no chain explains.
//!
//! It runs only for the lines whose combined outcome needs it: a target line
//! with an unexplained origin (which includes a path with no chain) and a
//! base line with no fate. An edit's non-empty `new_text` (a Write's
//! content) is looked for in the target, its `old_text` in the base, as a
//! sequence of lines: every distinct line gets an id once, the block's
//! interior lines are found as a run of ids with a linear-time search
//! (KMP), and its partial first and last lines are checked by bytes. A
//! block covers only the whole lines it contains, a line counting as whole
//! apart from its terminator; a block inside part of a line explains
//! nothing, and a block with no letter or digit never matches. Matching has
//! its own byte budget, charged per line indexed or compared; when it runs
//! out, `incomplete` is set and the lines not yet matched stay unmatched.
//! Matches are evidence of a different kind than a chain, and several stay
//! a list.

use std::collections::{BTreeSet, HashMap};

use supersigil_record::EventId;
use supersigil_record::observations::EditOperation;

use crate::input::State;
use crate::lines::split_lines;
use crate::result::{BaseLineOutcome, ContentMatches, LineOutcome, Origin};
use crate::step::Budget;
use crate::walk::AcceptedEdit;

/// Finds whole-block matches of the edits' retained texts for the lines
/// that need the fallback, within `budget_bytes` of matching work.
#[must_use]
pub fn content_matches(
    base: &State,
    target: &State,
    edits: &[AcceptedEdit],
    target_outcomes: &[LineOutcome],
    base_outcomes: &[BaseLineOutcome],
    budget_bytes: u64,
) -> ContentMatches {
    let mut budget = Budget::new(budget_bytes);
    let target_wanted: Vec<bool> = target_outcomes.iter().map(needs_target).collect();
    let base_wanted: Vec<bool> = base_outcomes.iter().map(needs_base).collect();
    let mut matches = ContentMatches {
        target: vec![BTreeSet::new(); target_wanted.len()],
        base: vec![BTreeSet::new(); base_wanted.len()],
        incomplete: false,
    };
    let target_index = target_wanted
        .contains(&true)
        .then(|| Index::build(target.bytes().unwrap_or_default(), &mut budget))
        .flatten();
    let base_index = base_wanted
        .contains(&true)
        .then(|| Index::build(base.bytes().unwrap_or_default(), &mut budget))
        .flatten();
    for accepted in edits {
        if budget.exhausted() {
            break;
        }
        let edit = &accepted.edit;
        if let (Some(index), Some(block)) = (&target_index, edit.new_text.retained()) {
            let mut cover = Cover {
                wanted: &target_wanted,
                lines: &mut matches.target,
                id: &edit.id,
            };
            index.find(block.as_bytes(), &mut cover, &mut budget);
        }
        if edit.operation == EditOperation::Write {
            continue;
        }
        if let (Some(index), Some(block)) = (&base_index, edit.old_text.retained()) {
            let mut cover = Cover {
                wanted: &base_wanted,
                lines: &mut matches.base,
                id: &edit.id,
            };
            index.find(block.as_bytes(), &mut cover, &mut budget);
        }
    }
    matches.incomplete = budget.exhausted();
    matches
}

fn needs_target(outcome: &LineOutcome) -> bool {
    matches!(
        outcome,
        LineOutcome::Agreed { provenance, .. } if provenance.origins.contains(&Origin::Unexplained)
    )
}

fn needs_base(outcome: &BaseLineOutcome) -> bool {
    matches!(outcome, BaseLineOutcome::Agreed { fates, .. } if fates.is_empty())
}

/// Where a block's matches are recorded: only on the wanted lines.
struct Cover<'a> {
    wanted: &'a [bool],
    lines: &'a mut [BTreeSet<EventId>],
    id: &'a EventId,
}

impl Cover<'_> {
    fn mark(&mut self, line: usize) {
        if self.wanted[line] {
            self.lines[line].insert(self.id.clone());
        }
    }
}

/// One side's lines, indexed once.
struct Index<'a> {
    lines: Vec<&'a [u8]>,
    /// The id of each line: equal lines, terminators included, share one.
    ids: Vec<u32>,
    id_of: HashMap<&'a [u8], u32>,
    /// Lines by their whole bytes, and by their content without terminator.
    by_line: HashMap<&'a [u8], Vec<usize>>,
    by_content: HashMap<&'a [u8], Vec<usize>>,
}

impl<'a> Index<'a> {
    /// Indexes `bytes`, charging every line; `None` when the budget runs out.
    fn build(bytes: &'a [u8], budget: &mut Budget) -> Option<Self> {
        let lines = split_lines(bytes);
        let mut index = Self {
            ids: Vec::with_capacity(lines.len()),
            id_of: HashMap::new(),
            by_line: HashMap::new(),
            by_content: HashMap::new(),
            lines: Vec::new(),
        };
        for (i, &line) in lines.iter().enumerate() {
            if !budget.charge(line.len()) {
                return None;
            }
            let next = u32::try_from(index.id_of.len()).ok()?;
            index.ids.push(*index.id_of.entry(line).or_insert(next));
            index.by_line.entry(line).or_default().push(i);
            index.by_content.entry(content(line)).or_default().push(i);
        }
        index.lines = lines;
        Some(index)
    }

    /// Marks every wanted line `block` covers wherever it occurs.
    fn find(&self, block: &[u8], cover: &mut Cover, budget: &mut Budget) {
        let qualifies = String::from_utf8_lossy(block)
            .chars()
            .any(char::is_alphanumeric);
        let segments = split_lines(block);
        let (Some(&first), Some(&last)) = (segments.first(), segments.last()) else {
            return;
        };
        if !qualifies {
            return;
        }
        if segments.len() == 1 {
            let found = if first.ends_with(b"\n") {
                self.by_line.get(first)
            } else {
                self.by_content.get(first)
            };
            for &line in found.into_iter().flatten() {
                if !budget.charge(first.len()) {
                    return;
                }
                cover.mark(line);
            }
            return;
        }
        let partial_last = (!last.ends_with(b"\n")).then_some(last);
        let core_end = segments.len() - usize::from(partial_last.is_some());
        let core = &segments[1..core_end];
        let starts: Vec<usize> = if core.is_empty() {
            // Only a first line and a partial last one: check each pair.
            (1..self.lines.len()).collect()
        } else {
            let Some(pattern) = core
                .iter()
                .map(|s| self.id_of.get(s).copied())
                .collect::<Option<Vec<u32>>>()
            else {
                return;
            };
            match self.kmp(&pattern, budget) {
                Some(starts) => starts,
                None => return,
            }
        };
        for start in starts {
            let end = start + core.len();
            if !budget.charge(first.len() + partial_last.map_or(0, <[u8]>::len)) {
                return;
            }
            let Some(before) = start.checked_sub(1).map(|i| self.lines[i]) else {
                continue;
            };
            if !before.ends_with(first) {
                continue;
            }
            if let Some(tail) = partial_last
                && !self.lines.get(end).is_some_and(|l| l.starts_with(tail))
            {
                continue;
            }
            if before == first {
                cover.mark(start - 1);
            }
            for line in start..end {
                cover.mark(line);
            }
            if let Some(tail) = partial_last
                && content(self.lines[end]) == tail
            {
                cover.mark(end);
            }
        }
    }

    /// Every start of `pattern` in the line ids, charging each line visited;
    /// `None` when the budget runs out.
    fn kmp(&self, pattern: &[u32], budget: &mut Budget) -> Option<Vec<usize>> {
        let mut fallback = vec![0usize; pattern.len()];
        let mut k = 0;
        for i in 1..pattern.len() {
            while k > 0 && pattern[i] != pattern[k] {
                k = fallback[k - 1];
            }
            if pattern[i] == pattern[k] {
                k += 1;
            }
            fallback[i] = k;
        }
        let mut starts = Vec::new();
        let mut matched = 0;
        for (i, &id) in self.ids.iter().enumerate() {
            if !budget.charge(self.lines[i].len()) {
                return None;
            }
            while matched > 0 && id != pattern[matched] {
                matched = fallback[matched - 1];
            }
            if id == pattern[matched] {
                matched += 1;
            }
            if matched == pattern.len() {
                starts.push(i + 1 - pattern.len());
                matched = fallback[matched - 1];
            }
        }
        Some(starts)
    }
}

/// A line's content, before its `\n` or `\r\n` terminator.
fn content(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r\n")
        .or_else(|| line.strip_suffix(b"\n"))
        .unwrap_or(line)
}
```

- [ ] **Step 9: Write the entry point and the final crate root**

`crates/supersigil-anchor/src/attribute.rs`:

```rust
//! The entry point: attributes one path.

use std::collections::BTreeSet;

use crate::combine::combine;
use crate::content::content_matches;
use crate::input::Request;
use crate::lines::line_starts;
use crate::provenance::{ChainProvenance, replay};
use crate::result::{Chain, ContentMatches, PathAttribution, PathStatus};
use crate::walk::walk;

/// Walks the path's recorded edits, replays and combines the chains that
/// were not set aside, and adds the content-match fallback.
///
/// Content matching runs only when the search completed (after an
/// incomplete search every line is unresolved, so the fallback is never
/// consulted, and its matches stay empty) and only for the lines whose
/// combined outcome needs it, with its own budget of `budget_bytes`.
///
/// No outcome claims capture completeness: a chain exact from the base
/// says only that the recorded replacements transform the base into the
/// target.
#[must_use]
pub fn attribute(request: Request) -> PathAttribution {
    let walked = walk(&request);
    let Request {
        base,
        target,
        budget_bytes,
        ..
    } = request;
    let base_lines = line_starts(base.bytes().unwrap_or_default()).len();
    let target_lines = line_starts(target.bytes().unwrap_or_default()).len();
    let provenance: Vec<(&Chain, ChainProvenance)> = walked
        .chains
        .iter()
        .map(|w| (&w.chain, replay(w, base_lines)))
        .collect();
    let (target_outcomes, base_outcomes) = combine(&provenance, target_lines, base_lines);
    let content = if walked.status == PathStatus::SearchIncomplete {
        ContentMatches {
            target: vec![BTreeSet::new(); target_lines],
            base: vec![BTreeSet::new(); base_lines],
            incomplete: false,
        }
    } else {
        content_matches(
            &base,
            &target,
            &walked.accepted,
            &target_outcomes,
            &base_outcomes,
            budget_bytes,
        )
    };
    PathAttribution {
        status: walked.status,
        chains: walked.chains.iter().map(|w| w.chain.clone()).collect(),
        set_aside: walked.set_aside.iter().map(|w| w.chain.clone()).collect(),
        conflicts: walked.conflicts,
        accepted: walked.accepted,
        target: target_outcomes,
        base: base_outcomes,
        content,
    }
}
```

Replace `crates/supersigil-anchor/src/lib.rs` with:

```rust
//! Attributes the lines of a reviewed change to the recorded edits that
//! produced them.
//!
//! Anchor walks backward from a target state through recorded edits. Each
//! step proposes before-states and accepts one only when the recorded hashes
//! (where known), the retained patch (where kept), and forward execution
//! with the recorder's semantics agree ([`step`]). Lines are aligned across a step from the byte ranges forward
//! execution replaced, never from a diff ([`align`]). The walk collects
//! every chain the evidence supports and classes it by what verifies it
//! ([`walk`]); each chain is replayed into per-line provenance
//! ([`provenance`]) and the chains are combined line by line
//! ([`combine`]), with a labeled content-match fallback ([`content`]).
//! [`attribute()`] runs all of it for one path. Anchor is pure: it reads no
//! files and runs no processes.

pub mod align;
pub mod attribute;
pub mod combine;
pub mod content;
pub mod input;
pub mod lines;
pub mod provenance;
pub mod result;
pub mod step;
pub mod walk;

pub use attribute::attribute;
pub use input::{CandidateEdit, DEFAULT_BUDGET_BYTES, Request, State, TargetKind};
pub use result::{
    BaseLineOutcome, Chain, ChainClass, ChainEnd, Conflict, Fate, LineOutcome, Origin,
    PathAttribution, PathStatus, Provenance, Reading, StopReason,
};
```

- [ ] **Step 10: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-anchor`
Expected: `102 tests run: 102 passed` (33 attribution tests, 4 property tests, and Tasks 8 and 9's 65).

Then run the properties harder once, to be sure they hold beyond the default 256 cases:

Run: `PROPTEST_CASES=20000 cargo nextest run -p supersigil-anchor --test properties`
Expected: `4 tests run: 4 passed` in a few seconds.

- [ ] **Step 11: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; every test passes.

- [ ] **Step 12: Commit**

```bash
git add crates/supersigil-anchor/src/result.rs crates/supersigil-anchor/src/lib.rs crates/supersigil-anchor/src/provenance.rs crates/supersigil-anchor/src/combine.rs crates/supersigil-anchor/src/content.rs crates/supersigil-anchor/src/attribute.rs crates/supersigil-anchor/tests/common/mod.rs crates/supersigil-anchor/tests/attribute.rs crates/supersigil-anchor/tests/properties.rs
git commit -m "feat: attribute target lines to recorded edits"
```

---

---

### Task 11: Review model

**Files:**
- Modify: `crates/supersigil-review/Cargo.toml` (dependencies), `crates/supersigil-review/src/lib.rs`
- Create: `crates/supersigil-review/src/diff.rs`, `src/mapping.rs`, `src/outcome.rs`, `src/model.rs`, `src/summary.rs`, `src/why.rs`
- Test: inline `#[cfg(test)]` in `diff.rs` and `mapping.rs`; `crates/supersigil-review/tests/common/mod.rs`, `tests/outcome_rules.rs`, `tests/file_review.rs`, `tests/snapshots.rs`, and the four snapshot files under `tests/snapshots/`

**Interfaces:**
- Consumes:
  - From Task 8 (`supersigil-anchor`): `lines::split_lines(&[u8]) -> Vec<&[u8]>` (lines keep their terminators; empty input has no lines).
  - From Tasks 9 and 10 (`supersigil-anchor`, re-exported at the crate root): `PathAttribution`, `LineOutcome`, `BaseLineOutcome`, `Provenance`, `Origin`, `Fate`, `Reading`, `Chain`, `ChainClass`, `ChainEnd`, `PathStatus`, `StopReason`, `Conflict`, `ContentMatches`, exactly as the interfaces contract declares them, plus `ContentMatches.incomplete: bool` (Task 10: content matching ran out of budget before every edit was examined). This task never runs anchor's walk: its tests build `PathAttribution` values directly.
  - From Task 1 (`supersigil-record`): `observations::EditOperation`; and from plan 1: `EventId`, `SessionId`, `observations::{CaptureLimitation, CaptureCounts, Material, Role}`.
  - From Task 2: workspace dependency entries `similar`, `supersigil-anchor`, `supersigil-record`.
- Produces (the contract's `supersigil-review` section, with the differences named here):
  - `diff::{COARSE_LINES, DiffHunk, FileDiff, diff_lines}`, `mapping::lines_correspond`.
  - `outcome::{Relation, AttributedEdit, UnattributedReason, Outcome, AttributionState, target_line_outcome, base_line_outcome}`.
  - `model::*` as the contract lists, plus `FileStatus::letter(self) -> char`, `FileKindInfo::as_str(self) -> &'static str`, and `SpanSide { Base, Target }`. **Differs from the contract:** `AttributionInfo.status` and `why::WhyLine.status` are `Option<PathStatus>` (null when attribution is unavailable, since there is no anchor status to report).
  - `summary::render_summary(review: &Review, escape: fn(&str) -> String) -> String` and `why::render_why(why: &Why, escape: fn(&str) -> String) -> String`. **Differs from the contract:** both take the escaping function, which the CLI (Task 13) passes as `supersigil_cli::format::escape_control`, so every untrusted string still goes through plan 1's `Untrusted` rules and the review crate does not duplicate them.
  - `why::{Why, WhyTarget, OnDiskCheck, Tristate, WhyLine, why_line}`.
  - Three fields the contract lacks, required by the review fixes F7, F9, and F10: `AttributionInfo.conflicting_edits: Vec<Conflict>` (anchor's `conflicts` for the file, empty when attribution is unavailable; an edit id whose sightings disagree is excluded from, and listed on, every file any sighting touched), `ScopeInfo.path_note: Option<String>` (set when a path selector named the worktree root, so the review has no path filter; the terminal summary prints it as a scope note), and `WhyLine.differs_from_head_reason: Option<String>` (why `differs_from_head` is `unknown`, for example "coarse diff"). Accordingly `why_line` takes a sixth parameter, `differs_from_head_reason: Option<String>`, after `differs_from_head`.
  - One reason and one count the contract lacks, for content matching that ran out of budget: `UnattributedReason::ContentMatchIncomplete` and `FileCounts.content_match_incomplete: usize` (lines with that reason, counted apart from `unattributed`).
  - `REVIEW_SCHEMA = "supersigil.review/1"`, `WHY_SCHEMA = "supersigil.why/1"` re-exported with `Review` at the crate root.

Line numbers are 0-based in every function and 1-based in the JSON: `HunkReview` starts use unified-diff numbering (for an empty range, the line before it, 0 meaning before the first line, the same convention as plan 1's `Hunk`), and `Span.start`, `WhyLine.number`, and the terminal forms are 1-based. `Outcome::Realigned.line` stays 0-based, as the contract says.

The review crate never names a `supersigil-git` type: the CLI converts `ChangeStatus`, `Mode`, `FileKind`, `Ancestry`, and the snapshot listings into this crate's model types (`FileStatus`, octal mode strings, `FileKindInfo`, `AncestryInfo`, `ScopeInfo`, and so on).

- [ ] **Step 1: Set the crate's dependencies**

Replace `crates/supersigil-review/Cargo.toml` with:

```toml
[package]
name = "supersigil-review"
version = "0.14.0"
description = "Review model for supersigil: the reviewed diff with each line's attribution"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
similar.workspace = true
supersigil-anchor.workspace = true
supersigil-record.workspace = true

[dev-dependencies]
insta.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write the failing diff and mapping tests**

Replace `crates/supersigil-review/src/lib.rs` with the crate documentation and the first two modules (later steps add `outcome`, `model`, `summary`, and `why`):

```rust
//! Builds the review model: the reviewed diff between two snapshots, one
//! outcome per changed line, and the JSON and terminal forms of both.
//!
//! Everything here is pure; the CLI gathers the inputs. [`diff::diff_lines`]
//! computes the reviewed diff over blob bytes. [`mapping::lines_correspond`]
//! decides whether anchor's provenance, computed over worktree-form bytes,
//! maps line for line onto that diff. [`outcome`] turns each diff line's
//! provenance into one outer outcome by the design's ordered rules, and
//! [`model::file_review`] assembles hunks and spans. [`summary`] and [`why`]
//! render the terminal forms.

pub mod diff;
pub mod mapping;
```

`crates/supersigil-review/src/diff.rs` (tests first; the implementation goes above them in step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(
        base_start: usize,
        base_count: usize,
        target_start: usize,
        target_count: usize,
    ) -> DiffHunk {
        DiffHunk {
            base_start,
            base_count,
            target_start,
            target_count,
        }
    }

    #[test]
    fn identical_files_have_no_hunks() {
        let diff = diff_lines(b"a\nb\n", b"a\nb\n");
        assert_eq!(diff.hunks, Vec::new());
        assert!(!diff.coarse);
    }

    #[test]
    fn a_changed_line_is_one_hunk() {
        let diff = diff_lines(b"a\nb\nc\n", b"a\nB\nc\n");
        assert_eq!(diff.hunks, vec![hunk(1, 1, 1, 1)]);
    }

    #[test]
    fn an_insertion_has_an_empty_base_range_where_it_goes() {
        let diff = diff_lines(b"a\nc\n", b"a\nb\nc\n");
        assert_eq!(diff.hunks, vec![hunk(1, 0, 1, 1)]);
        let at_start = diff_lines(b"b\n", b"a\nb\n");
        assert_eq!(at_start.hunks, vec![hunk(0, 0, 0, 1)]);
    }

    #[test]
    fn an_added_file_adds_every_line() {
        let diff = diff_lines(b"", b"x\ny\n");
        assert_eq!(diff.hunks, vec![hunk(0, 0, 0, 2)]);
    }

    #[test]
    fn a_deleted_file_removes_every_line() {
        let diff = diff_lines(b"x\ny\n", b"");
        assert_eq!(diff.hunks, vec![hunk(0, 2, 0, 0)]);
    }

    #[test]
    fn terminators_are_part_of_the_line() {
        assert_eq!(diff_lines(b"x\n", b"x").hunks, vec![hunk(0, 1, 0, 1)]);
        assert_eq!(diff_lines(b"x\n", b"x\r\n").hunks, vec![hunk(0, 1, 0, 1)]);
    }

    #[test]
    fn separate_changes_are_separate_hunks() {
        let diff = diff_lines(b"a\nb\nc\nd\ne\n", b"A\nb\nc\nd\nE\n");
        assert_eq!(diff.hunks, vec![hunk(0, 1, 0, 1), hunk(4, 1, 4, 1)]);
    }

    fn numbered(prefix: &str, count: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for i in 0..count {
            bytes.extend_from_slice(format!("{prefix}{i}\n").as_bytes());
        }
        bytes
    }

    #[test]
    fn exactly_coarse_lines_remaining_is_still_aligned() {
        // First and last lines differ, so nothing is trimmed and 5,000
        // lines remain on each side; the equal middle keeps Myers cheap.
        let middle = numbered("x", COARSE_LINES - 2);
        let base = [b"a\n".as_slice(), &middle, b"z\n"].concat();
        let target = [b"A\n".as_slice(), &middle, b"Z\n"].concat();
        let diff = diff_lines(&base, &target);
        assert!(!diff.coarse);
        assert_eq!(
            diff.hunks,
            vec![
                hunk(0, 1, 0, 1),
                hunk(COARSE_LINES - 1, 1, COARSE_LINES - 1, 1)
            ]
        );
    }

    #[test]
    fn more_than_coarse_lines_remaining_is_one_coarse_hunk() {
        let base = [b"same\n".as_slice(), &numbered("b", COARSE_LINES + 1)].concat();
        let target = [b"same\n".as_slice(), &numbered("t", 3)].concat();
        let diff = diff_lines(&base, &target);
        assert!(diff.coarse);
        assert_eq!(diff.hunks, vec![hunk(1, COARSE_LINES + 1, 1, 3)]);
    }
}
```

`crates/supersigil-review/src/mapping.rs` (tests first; the implementation goes above them in step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_bytes_correspond() {
        assert!(lines_correspond(b"a\nb\n", b"a\nb\n"));
        assert!(lines_correspond(b"", b""));
    }

    #[test]
    fn line_ending_conversion_corresponds() {
        assert!(lines_correspond(b"a\nb\n", b"a\r\nb\r\n"));
        assert!(lines_correspond(b"a\r\nb", b"a\nb"));
    }

    #[test]
    fn changed_content_does_not_correspond() {
        assert!(!lines_correspond(b"a\nb\n", b"a\nc\n"));
    }

    #[test]
    fn changed_line_structure_does_not_correspond() {
        let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:ab\nsize 3\n";
        assert!(!lines_correspond(pointer, b"abc"));
        assert!(!lines_correspond(b"a\nb\n", b"a b\n"));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-review`
Expected: compile errors, `cannot find function diff_lines`, `cannot find value COARSE_LINES`, `cannot find function lines_correspond`.

- [ ] **Step 4: Implement the diff and the mapping check**

Put this above the test module in `crates/supersigil-review/src/diff.rs`:

```rust
//! The reviewed diff: a line diff of blob bytes, bounded without a clock.
//!
//! Lines are split after `\n` and compared byte for byte, terminators
//! included, so a change in line ending or in the final newline is a change.
//! Common leading and trailing lines are removed first; when more than
//! [`COARSE_LINES`] remain on either side, the file gets one coarse hunk
//! instead of an aligned diff, so the work never depends on how different the
//! two sides are.

use similar::{Algorithm, DiffTag, capture_diff_slices};
use supersigil_anchor::lines::split_lines;

/// Lines left on either side after trimming common lines, above which a
/// file gets one coarse hunk instead of a Myers alignment.
pub const COARSE_LINES: usize = 5_000;

/// One changed region, as 0-based line ranges on each side.
///
/// An empty range sits where the other side's lines go: `base_start` of an
/// insertion is the index of the base line the added lines precede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    /// First removed base line, 0-based.
    pub base_start: usize,
    /// Number of removed base lines.
    pub base_count: usize,
    /// First added target line, 0-based.
    pub target_start: usize,
    /// Number of added target lines.
    pub target_count: usize,
}

/// The reviewed diff of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// Changed regions in order, without context lines.
    pub hunks: Vec<DiffHunk>,
    /// Whether the file was too large to align and got one coarse hunk
    /// that removes every remaining base line and adds every remaining
    /// target line.
    pub coarse: bool,
}

/// Diffs `base` against `target` line by line with Myers, git's default.
///
/// Consecutive changed lines form one hunk; hunks carry no context.
#[must_use]
pub fn diff_lines(base: &[u8], target: &[u8]) -> FileDiff {
    let old = split_lines(base);
    let new = split_lines(target);
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];
    if old_mid.is_empty() && new_mid.is_empty() {
        return FileDiff {
            hunks: Vec::new(),
            coarse: false,
        };
    }
    if old_mid.len() > COARSE_LINES || new_mid.len() > COARSE_LINES {
        return FileDiff {
            hunks: vec![DiffHunk {
                base_start: prefix,
                base_count: old_mid.len(),
                target_start: prefix,
                target_count: new_mid.len(),
            }],
            coarse: true,
        };
    }
    let mut hunks = Vec::new();
    let mut open: Option<DiffHunk> = None;
    for op in capture_diff_slices(Algorithm::Myers, old_mid, new_mid) {
        let (tag, old_range, new_range) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            hunks.extend(open.take());
            continue;
        }
        let hunk = open.get_or_insert(DiffHunk {
            base_start: prefix + old_range.start,
            base_count: 0,
            target_start: prefix + new_range.start,
            target_count: 0,
        });
        hunk.base_count += old_range.len();
        hunk.target_count += new_range.len();
    }
    hunks.extend(open);
    FileDiff {
        hunks,
        coarse: false,
    }
}
```

Put this above the test module in `crates/supersigil-review/src/mapping.rs`:

```rust
//! Whether anchor's lines, over worktree-form bytes, map one to one onto the
//! reviewed diff's lines, over blob bytes.

use supersigil_anchor::lines::split_lines;

/// Returns whether `attribution` bytes correspond line for line to `blob`
/// bytes: they are identical, or they have the same number of lines and each
/// line is equal once its terminator and a trailing `\r` are removed (a
/// conversion that changed only line endings).
///
/// When this is false the conversion changed line structure (for example an
/// LFS pointer against its content), and the file's attribution is reported
/// unavailable rather than mapped onto the wrong lines.
#[must_use]
pub fn lines_correspond(blob: &[u8], attribution: &[u8]) -> bool {
    if blob == attribution {
        return true;
    }
    let blob_lines = split_lines(blob);
    let attribution_lines = split_lines(attribution);
    blob_lines.len() == attribution_lines.len()
        && blob_lines
            .iter()
            .zip(&attribution_lines)
            .all(|(a, b)| without_line_ending(a) == without_line_ending(b))
}

/// `line` without its `\n` and then without a trailing `\r`.
fn without_line_ending(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}
```

The coarse bound is deterministic: `similar`'s `capture_diff_slices` runs Myers without a deadline, and the bound is decided from line counts before it runs, never from elapsed time. The 5,000-line test keeps Myers cheap by making only the first and last lines differ.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-review`
Expected: 13 tests pass (9 in `diff::tests`, 4 in `mapping::tests`).

- [ ] **Step 6: Write the failing outcome-rule tests**

Add to `crates/supersigil-review/src/lib.rs`, after `pub mod mapping;`:

```rust
pub mod outcome;
```

Create an empty `crates/supersigil-review/src/outcome.rs` containing only its first doc line, `//! One outer outcome per diff line, decided by the design's ordered rules.`, so the crate compiles and the tests fail on the missing items.

`crates/supersigil-review/tests/common/mod.rs` (builders shared by the three test binaries; anchor results are built directly, and `escape` mirrors the CLI's `format::escape_control` so the snapshots show what the terminal will):

```rust
//! Builders for review tests. Anchor's results are constructed directly, so
//! these tests pin the review rules without running the walk.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use supersigil_anchor::result::ContentMatches;
use supersigil_anchor::{
    BaseLineOutcome, Chain, ChainClass, ChainEnd, Fate, LineOutcome, Origin, PathAttribution,
    PathStatus, Provenance, StopReason,
};
use supersigil_record::EventId;

/// An edit id.
pub fn eid(id: &str) -> EventId {
    EventId::new(id)
}

/// A chain over `edits` (oldest first) whose head is the last edit.
pub fn chain(id: usize, class: ChainClass, edits: &[&str], end: ChainEnd) -> Chain {
    Chain {
        id,
        class,
        head: eid(edits.last().copied().unwrap_or_default()),
        edits: edits.iter().map(|edit| eid(edit)).collect(),
        end,
    }
}

/// A target line every chain agrees on.
pub fn agreed(origins: impl IntoIterator<Item = Origin>, class: Option<ChainClass>) -> LineOutcome {
    LineOutcome::Agreed {
        provenance: Provenance {
            origins: origins.into_iter().collect(),
            ..Provenance::default()
        },
        class,
    }
}

/// A target line introduced by `edit` in a chain exact from the base.
pub fn introduced(edit: &str) -> LineOutcome {
    agreed(
        [Origin::Introduced(eid(edit))],
        Some(ChainClass::ExactFromBase),
    )
}

/// A target line that is base line `j` in a chain exact from the base.
pub fn base_line(j: usize) -> LineOutcome {
    agreed([Origin::Base(j)], Some(ChainClass::ExactFromBase))
}

/// A target line no chain explains.
pub fn unexplained() -> LineOutcome {
    agreed([Origin::Unexplained], None)
}

/// A base line with `fates`, agreed by a chain exact from the base.
pub fn fates(fates: impl IntoIterator<Item = Fate>) -> BaseLineOutcome {
    BaseLineOutcome::Agreed {
        fates: fates.into_iter().collect(),
        class: Some(ChainClass::ExactFromBase),
    }
}

/// A base line replaced by `edit`.
pub fn replaced(edit: &str) -> BaseLineOutcome {
    fates([Fate::Replaced { edit: eid(edit) }])
}

/// A base line kept as target line `line`.
pub fn kept(line: usize) -> BaseLineOutcome {
    fates([Fate::KeptAs { line }])
}

/// A base line with no fate.
pub fn no_fate() -> BaseLineOutcome {
    BaseLineOutcome::Agreed {
        fates: BTreeSet::new(),
        class: None,
    }
}

/// An attribution with `chains` and the given per-line outcomes, and no
/// content matches. Without chains the status is "not composed" with
/// `NoPredecessor`.
pub fn attribution(
    chains: Vec<Chain>,
    target: Vec<LineOutcome>,
    base: Vec<BaseLineOutcome>,
) -> PathAttribution {
    let status = if chains.is_empty() {
        PathStatus::NotComposed {
            reasons: vec![StopReason::NoPredecessor],
        }
    } else {
        PathStatus::Composed
    };
    PathAttribution {
        status,
        chains,
        set_aside: Vec::new(),
        conflicts: Vec::new(),
        accepted: Vec::new(),
        content: ContentMatches {
            target: vec![BTreeSet::new(); target.len()],
            base: vec![BTreeSet::new(); base.len()],
            incomplete: false,
        },
        target,
        base,
    }
}

/// The same escaping as the CLI's `format::escape_control`: control
/// characters other than tab become `\xNN`.
pub fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let code = u32::from(c);
        if (code <= 0x1f && c != '\t') || (0x7f..=0x9f).contains(&code) {
            let _ = write!(out, "\\x{code:02x}");
        } else {
            out.push(c);
        }
    }
    out
}
```

`crates/supersigil-review/tests/outcome_rules.rs`, one test per ordered rule for added and removed lines:

```rust
//! One test per ordered outcome rule, for added (target) and removed (base)
//! diff lines.

mod common;

use std::collections::BTreeSet;

use common::{
    agreed, attribution, base_line, chain, eid, fates, introduced, kept, no_fate, replaced,
    unexplained,
};
use serde_json::json;
use supersigil_anchor::{
    ChainClass, ChainEnd, Fate, LineOutcome, Origin, PathStatus, Provenance, Reading,
};
use supersigil_review::outcome::{
    AttributedEdit, AttributionState, Outcome, Relation, UnattributedReason, base_line_outcome,
    target_line_outcome,
};

fn exact_chain() -> Vec<supersigil_anchor::Chain> {
    vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)]
}

#[test]
fn search_incomplete_is_unresolved_even_with_an_attributed_reading() {
    let mut attr = attribution(exact_chain(), vec![introduced("e1")], vec![]);
    attr.status = PathStatus::SearchIncomplete;
    let (outcome, provenance) =
        target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Unresolved {
            candidates: provenance.clone()
        }
    );
    assert_eq!(provenance["kind"], "agreed");
}

#[test]
fn ambiguous_line_is_ambiguous_with_every_reading() {
    let readings = vec![
        Reading {
            chain: 0,
            class: ChainClass::ExactFromBase,
            value: Provenance {
                origins: BTreeSet::from([Origin::Introduced(eid("e1"))]),
                ..Provenance::default()
            },
        },
        Reading {
            chain: 1,
            class: ChainClass::ExactFromBase,
            value: Provenance {
                origins: BTreeSet::from([Origin::Base(1)]),
                ..Provenance::default()
            },
        },
    ];
    let attr = attribution(
        exact_chain(),
        vec![LineOutcome::Ambiguous {
            readings: readings.clone(),
        }],
        vec![],
    );
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"b\n"]);
    assert_eq!(
        outcome,
        Outcome::Ambiguous {
            readings: serde_json::to_value(&readings).unwrap()
        }
    );
}

#[test]
fn unexplained_line_with_covering_edits_is_a_content_match() {
    let mut attr = attribution(exact_chain(), vec![unexplained()], vec![]);
    attr.content.target[0] = BTreeSet::from([eid("e7"), eid("e3")]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::ContentMatch {
            relation: Relation::Introduced,
            edits: vec![eid("e3"), eid("e7")],
        }
    );
}

#[test]
fn unexplained_line_without_chains_is_unattributed_with_anchor_status() {
    let attr = attribution(vec![], vec![unexplained()], vec![]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Unattributed {
            reason: UnattributedReason::NoSurvivingChain {
                status: attr.status.clone()
            }
        }
    );
}

#[test]
fn unexplained_line_inherited_across_a_gap_is_unattributed() {
    let chains = vec![chain(
        0,
        ChainClass::ExactFromStart,
        &["e2"],
        ChainEnd::Stopped {
            reasons: vec![supersigil_anchor::StopReason::NoPredecessor],
        },
    )];
    let attr = attribution(chains, vec![unexplained()], vec![]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Unattributed {
            reason: UnattributedReason::GapBeforeChain
        }
    );
}

#[test]
fn partly_unexplained_line_is_never_attributed() {
    let line = agreed(
        [Origin::Introduced(eid("e1")), Origin::Unexplained],
        Some(ChainClass::ExactFromStart),
    );
    let attr = attribution(exact_chain(), vec![line], vec![]);
    let (outcome, provenance) =
        target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Unattributed {
            reason: UnattributedReason::GapBeforeChain
        }
    );
    // The explained origin stays visible in the provenance.
    assert!(provenance.to_string().contains("\"introduced\""));
}

#[test]
fn introduced_line_is_attributed_with_its_class() {
    let attr = attribution(exact_chain(), vec![introduced("e1")], vec![]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Attributed {
            edits: vec![AttributedEdit {
                edit: eid("e1"),
                relation: Relation::Introduced,
                classes: BTreeSet::from([ChainClass::ExactFromBase]),
            }]
        }
    );
}

#[test]
fn whitespace_contributors_make_a_whitespace_only_change() {
    let line = LineOutcome::Agreed {
        provenance: Provenance {
            origins: BTreeSet::from([Origin::Base(0)]),
            whitespace_only: BTreeSet::from([eid("fmt")]),
            ..Provenance::default()
        },
        class: Some(ChainClass::ExactFromBase),
    };
    let attr = attribution(exact_chain(), vec![line], vec![]);
    let (outcome, _) = target_line_outcome(
        &AttributionState::Available(&attr),
        0,
        &[b"a=1\n"],
        &[b"a = 1\n"],
    );
    assert_eq!(
        outcome,
        Outcome::WhitespaceOnly {
            edits: vec![eid("fmt")]
        }
    );
}

#[test]
fn whitespace_added_line_is_a_whitespace_only_change() {
    let line = agreed(
        [Origin::WhitespaceAdded(eid("fmt"))],
        Some(ChainClass::ExactFromBase),
    );
    let attr = attribution(exact_chain(), vec![line], vec![]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"\n"]);
    assert_eq!(
        outcome,
        Outcome::WhitespaceOnly {
            edits: vec![eid("fmt")]
        }
    );
}

#[test]
fn base_line_with_identical_blob_is_realigned() {
    let attr = attribution(exact_chain(), vec![base_line(2)], vec![]);
    let base: [&[u8]; 3] = [b"a\n", b"b\n", b"x\n"];
    let (outcome, _) =
        target_line_outcome(&AttributionState::Available(&attr), 0, &base, &[b"x\n"]);
    assert_eq!(outcome, Outcome::Realigned { line: 2 });
}

#[test]
fn base_line_whose_blob_differs_is_a_line_ending_change() {
    let attr = attribution(exact_chain(), vec![base_line(0)], vec![]);
    let (outcome, _) = target_line_outcome(
        &AttributionState::Available(&attr),
        0,
        &[b"x\n"],
        &[b"x\r\n"],
    );
    assert_eq!(outcome, Outcome::LineEndingChanged);
}

#[test]
fn unavailable_attribution_is_unattributed_with_null_provenance() {
    let state = AttributionState::Unavailable {
        reason: "conversion changed line structure".to_owned(),
    };
    let expected = Outcome::Unattributed {
        reason: UnattributedReason::AttributionUnavailable {
            reason: "conversion changed line structure".to_owned(),
        },
    };
    assert_eq!(
        target_line_outcome(&state, 0, &[], &[b"x\n"]),
        (expected.clone(), serde_json::Value::Null)
    );
    assert_eq!(
        base_line_outcome(&state, 0, &[b"x\n"], &[]),
        (expected, serde_json::Value::Null)
    );
}

#[test]
fn replaced_base_line_is_attributed_as_replaced() {
    let attr = attribution(exact_chain(), vec![], vec![replaced("e1")]);
    let (outcome, _) = base_line_outcome(&AttributionState::Available(&attr), 0, &[b"x\n"], &[]);
    assert_eq!(
        outcome,
        Outcome::Attributed {
            edits: vec![AttributedEdit {
                edit: eid("e1"),
                relation: Relation::Replaced,
                classes: BTreeSet::from([ChainClass::ExactFromBase]),
            }]
        }
    );
}

#[test]
fn base_line_without_fate_falls_back_to_content_match_then_unattributed() {
    let mut attr = attribution(vec![], vec![], vec![no_fate(), no_fate()]);
    attr.content.base[1] = BTreeSet::from([eid("e4")]);
    let state = AttributionState::Available(&attr);
    let (first, _) = base_line_outcome(&state, 0, &[b"a\n", b"b\n"], &[]);
    let (second, _) = base_line_outcome(&state, 1, &[b"a\n", b"b\n"], &[]);
    assert_eq!(
        first,
        Outcome::Unattributed {
            reason: UnattributedReason::NoSurvivingChain {
                status: attr.status.clone()
            }
        }
    );
    assert_eq!(
        second,
        Outcome::ContentMatch {
            relation: Relation::Replaced,
            edits: vec![eid("e4")],
        }
    );
}

#[test]
fn unmatched_lines_are_unattributed_when_content_matching_ran_out() {
    let mut attr = attribution(
        exact_chain(),
        vec![unexplained(), unexplained()],
        vec![no_fate()],
    );
    attr.content.incomplete = true;
    attr.content.target[1] = BTreeSet::from([eid("e3")]);
    let state = AttributionState::Available(&attr);
    let base: &[&[u8]] = &[b"a\n"];
    let target: &[&[u8]] = &[b"x\n", b"y\n"];
    let incomplete = Outcome::Unattributed {
        reason: UnattributedReason::ContentMatchIncomplete,
    };
    assert_eq!(target_line_outcome(&state, 0, base, target).0, incomplete);
    assert_eq!(base_line_outcome(&state, 0, base, target).0, incomplete);
    // A match found before the budget ran out still counts.
    assert_eq!(
        target_line_outcome(&state, 1, base, target).0,
        Outcome::ContentMatch {
            relation: Relation::Introduced,
            edits: vec![eid("e3")],
        }
    );
}

#[test]
fn carried_base_line_is_a_whitespace_only_change() {
    let base = fates([Fate::CarriedTo {
        line: 0,
        via: BTreeSet::from([eid("fmt")]),
    }]);
    let attr = attribution(exact_chain(), vec![], vec![base]);
    let (outcome, _) = base_line_outcome(
        &AttributionState::Available(&attr),
        0,
        &[b"a=1\n"],
        &[b"a = 1\n"],
    );
    assert_eq!(
        outcome,
        Outcome::WhitespaceOnly {
            edits: vec![eid("fmt")]
        }
    );
}

#[test]
fn base_line_removed_by_whitespace_is_a_whitespace_only_change() {
    let base = fates([Fate::RemovedByWhitespace { edit: eid("fmt") }]);
    let attr = attribution(exact_chain(), vec![], vec![base]);
    let (outcome, _) = base_line_outcome(&AttributionState::Available(&attr), 0, &[b"\n"], &[]);
    assert_eq!(
        outcome,
        Outcome::WhitespaceOnly {
            edits: vec![eid("fmt")]
        }
    );
}

#[test]
fn kept_base_line_is_realigned_or_a_line_ending_change() {
    let attr = attribution(exact_chain(), vec![], vec![kept(1)]);
    let state = AttributionState::Available(&attr);
    let (same, _) = base_line_outcome(&state, 0, &[b"x\n"], &[b"a\n", b"x\n"]);
    assert_eq!(same, Outcome::Realigned { line: 1 });
    let (ending, _) = base_line_outcome(&state, 0, &[b"x\n"], &[b"a\n", b"x\r\n"]);
    assert_eq!(ending, Outcome::LineEndingChanged);
}

#[test]
fn provenance_is_the_anchor_outcome_serialized() {
    let attr = attribution(exact_chain(), vec![introduced("e1")], vec![replaced("e1")]);
    let state = AttributionState::Available(&attr);
    let (_, target) = target_line_outcome(&state, 0, &[], &[b"x\n"]);
    assert_eq!(
        target,
        json!({
            "kind": "agreed",
            "provenance": {
                "origins": [{"kind": "introduced", "value": "e1"}],
                "whitespace_only": [],
                "earlier": []
            },
            "class": "exact_from_base"
        })
    );
    let (_, base) = base_line_outcome(&state, 0, &[b"x\n"], &[]);
    assert_eq!(
        base,
        json!({
            "kind": "agreed",
            "fates": [{"kind": "replaced", "edit": "e1"}],
            "class": "exact_from_base"
        })
    );
}
```

- [ ] **Step 7: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-review --test outcome_rules`
Expected: compile errors, `unresolved imports supersigil_review::outcome::AttributedEdit` and the other outcome items.

- [ ] **Step 8: Implement the outcome rules**

Replace `crates/supersigil-review/src/outcome.rs` with:

```rust
//! One outer outcome per diff line, decided by the design's ordered rules.
//!
//! Anchor gives every target line a combined provenance (a set of origins
//! plus contributors) and every base line a set of fates, or marks the line
//! ambiguous. The rules, in order:
//!
//! 1. the path's search is incomplete: *unresolved*;
//! 2. the line is ambiguous: *ambiguous*;
//! 3. an added line with any unexplained origin, or a removed line with no
//!    fate: *content match* when whole-edit blocks cover it, otherwise
//!    *unattributed* with the reason, which is that content matching ran
//!    out of budget whenever it did, since a match may exist that was never
//!    looked for. A partly explained line lands here, so it is never shown
//!    as attributed;
//! 4. an introducing edit among the origins, or a replacing edit among the
//!    fates: *attributed*;
//! 5. everything is base content: *whitespace-only change* when whitespace
//!    contributors exist, *line ending changed* when the blob lines still
//!    differ, *realigned* when they are byte-identical.
//!
//! Every outcome comes with the line's full provenance, anchor's combined
//! outcome serialized as-is, so nothing the rules summarize is lost.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;
use supersigil_anchor::{
    BaseLineOutcome, ChainClass, Fate, LineOutcome, Origin, PathAttribution, PathStatus, Provenance,
};
use supersigil_record::EventId;

/// How an edit contributed to an attributed line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// The edit wrote the retained content of an added line.
    Introduced,
    /// The edit removed a base line outside a whitespace-only change.
    Replaced,
}

/// One edit an attributed line names, with the classes of the chains that
/// agree on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttributedEdit {
    /// The edit.
    pub edit: EventId,
    /// How it contributed.
    pub relation: Relation,
    /// The strongest class among the chains that agree on the line.
    pub classes: BTreeSet<ChainClass>,
}

/// Why a diff line has no attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnattributedReason {
    /// No chain survived for the path; anchor's status says why.
    NoSurvivingChain {
        /// Anchor's status for the path, with its stop reasons.
        status: PathStatus,
    },
    /// Chains exist, but the line is inherited from before their start.
    GapBeforeChain,
    /// No whole-edit block was found to cover the line, but content
    /// matching ran out of budget first: one may exist.
    ContentMatchIncomplete,
    /// The file's attribution could not be computed.
    AttributionUnavailable {
        /// Why, for example "conversion changed line structure".
        reason: String,
    },
}

/// The one outer outcome of a diff line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// The search ran out of budget: the chains' readings for this line are
    /// candidate evidence, never a result.
    Unresolved {
        /// Anchor's combined outcome for the line, serialized as-is.
        candidates: Value,
    },
    /// The surviving chains disagree about this line.
    Ambiguous {
        /// Every chain's reading with its class, serialized as-is.
        readings: Value,
    },
    /// No chain explains the line, but whole edit blocks cover it.
    ContentMatch {
        /// Introduced for added lines, replaced for removed lines.
        relation: Relation,
        /// Every covering edit; nothing picks among them.
        edits: Vec<EventId>,
    },
    /// Nothing explains the line.
    Unattributed {
        /// Why.
        reason: UnattributedReason,
    },
    /// An edit introduced the added line or replaced the removed one.
    Attributed {
        /// The introducing or replacing edits.
        edits: Vec<AttributedEdit>,
    },
    /// Only whitespace changed, by these edits.
    WhitespaceOnly {
        /// The whitespace-only contributors.
        edits: Vec<EventId>,
    },
    /// The blob lines differ only in line ending, which no edit explains.
    LineEndingChanged,
    /// Every chain carries the line unchanged; the diff aligned it
    /// differently.
    Realigned {
        /// The corresponding line on the other side, 0-based.
        line: usize,
    },
}

/// A file's attribution as the rules see it.
#[derive(Debug, Clone)]
pub enum AttributionState<'a> {
    /// Anchor's result for the path.
    Available(&'a PathAttribution),
    /// Attribution could not be computed, with the reason.
    Unavailable {
        /// Why, for example "conversion changed line structure".
        reason: String,
    },
}

/// The outcome of added diff line `line` (0-based target line) and its full
/// provenance (anchor's `LineOutcome` as JSON, or null when attribution is
/// unavailable).
///
/// `base_blob_lines` and `target_blob_lines` are the blob lines, used only to
/// tell *realigned* from *line ending changed*.
#[must_use]
pub fn target_line_outcome(
    attribution: &AttributionState<'_>,
    line: usize,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> (Outcome, Value) {
    let attr = match attribution {
        AttributionState::Unavailable { reason } => return (unavailable(reason), Value::Null),
        AttributionState::Available(attr) => *attr,
    };
    let missing = LineOutcome::Agreed {
        provenance: Provenance {
            origins: BTreeSet::from([Origin::Unexplained]),
            ..Provenance::default()
        },
        class: None,
    };
    let combined = attr.target.get(line).unwrap_or(&missing);
    let provenance = json(combined);
    if attr.status == PathStatus::SearchIncomplete {
        return (
            Outcome::Unresolved {
                candidates: provenance.clone(),
            },
            provenance,
        );
    }
    let outcome = match combined {
        LineOutcome::Ambiguous { readings } => Outcome::Ambiguous {
            readings: json(readings),
        },
        LineOutcome::Agreed { provenance, class } => agreed_target(
            attr,
            line,
            provenance,
            *class,
            base_blob_lines,
            target_blob_lines,
        ),
    };
    (outcome, provenance)
}

/// The outcome of removed diff line `line` (0-based base line) and its full
/// provenance (anchor's `BaseLineOutcome` as JSON, or null when attribution
/// is unavailable).
#[must_use]
pub fn base_line_outcome(
    attribution: &AttributionState<'_>,
    line: usize,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> (Outcome, Value) {
    let attr = match attribution {
        AttributionState::Unavailable { reason } => return (unavailable(reason), Value::Null),
        AttributionState::Available(attr) => *attr,
    };
    let missing = BaseLineOutcome::Agreed {
        fates: BTreeSet::new(),
        class: None,
    };
    let combined = attr.base.get(line).unwrap_or(&missing);
    let provenance = json(combined);
    if attr.status == PathStatus::SearchIncomplete {
        return (
            Outcome::Unresolved {
                candidates: provenance.clone(),
            },
            provenance,
        );
    }
    let outcome = match combined {
        BaseLineOutcome::Ambiguous { readings } => Outcome::Ambiguous {
            readings: json(readings),
        },
        BaseLineOutcome::Agreed { fates, class } => agreed_base(
            attr,
            line,
            fates,
            *class,
            base_blob_lines,
            target_blob_lines,
        ),
    };
    (outcome, provenance)
}

/// Rules 3 to 5 for an added line every chain agrees on.
fn agreed_target(
    attr: &PathAttribution,
    line: usize,
    provenance: &Provenance,
    class: Option<ChainClass>,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> Outcome {
    if provenance.origins.is_empty() || provenance.origins.contains(&Origin::Unexplained) {
        return fallback(attr, &attr.content.target, line, Relation::Introduced);
    }
    let introduced: Vec<AttributedEdit> = provenance
        .origins
        .iter()
        .filter_map(|origin| match origin {
            Origin::Introduced(edit) => Some(AttributedEdit {
                edit: edit.clone(),
                relation: Relation::Introduced,
                classes: class.into_iter().collect(),
            }),
            _ => None,
        })
        .collect();
    if !introduced.is_empty() {
        return Outcome::Attributed { edits: introduced };
    }
    let mut whitespace = provenance.whitespace_only.clone();
    for origin in &provenance.origins {
        if let Origin::WhitespaceAdded(edit) = origin {
            whitespace.insert(edit.clone());
        }
    }
    if !whitespace.is_empty() {
        return Outcome::WhitespaceOnly {
            edits: whitespace.into_iter().collect(),
        };
    }
    let bases: Vec<usize> = provenance
        .origins
        .iter()
        .filter_map(|origin| match origin {
            Origin::Base(j) => Some(*j),
            _ => None,
        })
        .collect();
    let target_text = target_blob_lines.get(line);
    let identical = bases.iter().all(|j| {
        base_blob_lines
            .get(*j)
            .is_some_and(|b| Some(b) == target_text)
    });
    match bases.first() {
        Some(&first) if identical => Outcome::Realigned { line: first },
        _ => Outcome::LineEndingChanged,
    }
}

/// Rules 3 to 5 for a removed line every chain agrees on.
fn agreed_base(
    attr: &PathAttribution,
    line: usize,
    fates: &BTreeSet<Fate>,
    class: Option<ChainClass>,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> Outcome {
    if fates.is_empty() {
        return fallback(attr, &attr.content.base, line, Relation::Replaced);
    }
    let replaced: Vec<AttributedEdit> = fates
        .iter()
        .filter_map(|fate| match fate {
            Fate::Replaced { edit } => Some(AttributedEdit {
                edit: edit.clone(),
                relation: Relation::Replaced,
                classes: class.into_iter().collect(),
            }),
            _ => None,
        })
        .collect();
    if !replaced.is_empty() {
        return Outcome::Attributed { edits: replaced };
    }
    let mut whitespace = BTreeSet::new();
    let mut kept = None;
    for fate in fates {
        match fate {
            Fate::CarriedTo { via, line } => {
                whitespace.extend(via.iter().cloned());
                kept.get_or_insert(*line);
            }
            Fate::RemovedByWhitespace { edit } => {
                whitespace.insert(edit.clone());
            }
            Fate::KeptAs { line } => {
                kept.get_or_insert(*line);
            }
            Fate::Replaced { .. } => {}
        }
    }
    if !whitespace.is_empty() {
        return Outcome::WhitespaceOnly {
            edits: whitespace.into_iter().collect(),
        };
    }
    match kept {
        Some(k)
            if base_blob_lines.get(line).is_some()
                && base_blob_lines.get(line) == target_blob_lines.get(k) =>
        {
            Outcome::Realigned { line: k }
        }
        _ => Outcome::LineEndingChanged,
    }
}

/// Rule 3's content-match fallback, then the unattributed reason.
fn fallback(
    attr: &PathAttribution,
    matches: &[BTreeSet<EventId>],
    line: usize,
    relation: Relation,
) -> Outcome {
    if let Some(edits) = matches.get(line).filter(|edits| !edits.is_empty()) {
        return Outcome::ContentMatch {
            relation,
            edits: edits.iter().cloned().collect(),
        };
    }
    let reason = if attr.content.incomplete {
        UnattributedReason::ContentMatchIncomplete
    } else if attr.chains.is_empty() {
        UnattributedReason::NoSurvivingChain {
            status: attr.status.clone(),
        }
    } else {
        UnattributedReason::GapBeforeChain
    };
    Outcome::Unattributed { reason }
}

fn unavailable(reason: &str) -> Outcome {
    Outcome::Unattributed {
        reason: UnattributedReason::AttributionUnavailable {
            reason: reason.to_owned(),
        },
    }
}

/// Serializes anchor's outcome types; they contain no map with non-string
/// keys, so this never falls back to null in practice.
fn json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}
```

Rule 3 comes before rule 4 on purpose: a line whose origins include both an introducing edit and `Unexplained` (partly inherited across a gap) is never reported as attributed; its explained origins stay visible in the provenance. Within rule 3, a covering edit found by content matching still counts even when matching ran out of budget; a line with none gets the reason `content_match_incomplete` whenever `attr.content.incomplete` is set, ahead of "no surviving chain" and "gap before the chain's start", because a covering edit may exist that was never looked for. `realigned` is reported only when the corresponding blob lines are byte-identical; otherwise the difference can only be a line ending, which no edit explains.

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-review`
Expected: 32 tests pass (13 unit, 19 in `outcome_rules`).

- [ ] **Step 10: Write the failing file-review tests**

Add to `crates/supersigil-review/src/lib.rs`, after `pub mod mapping;` and before `pub mod outcome;`:

```rust
pub mod model;
```

and at the end:

```rust
pub use model::{REVIEW_SCHEMA, Review, WHY_SCHEMA};
```

Create `crates/supersigil-review/src/model.rs` containing only `//! The JSON review model and the per-file assembly of hunks and spans.` for now.

`crates/supersigil-review/tests/file_review.rs`:

```rust
//! Assembling one file's review: hunks, spans, and the unattributed summary.

mod common;

use common::{
    attribution, base_line, chain, eid, introduced, kept, no_fate, replaced, unexplained,
};
use supersigil_anchor::{ChainClass, ChainEnd, Conflict};
use supersigil_record::RecordId;
use supersigil_review::model::{
    BytesStatus, FileInput, FileKindInfo, FileStatus, Mention, NotCapturedInfo, RepoPathInfo,
    ScopeInfo, SpanSide, UntrackedInfo, file_review, unattributed_summary,
};
use supersigil_review::outcome::{AttributionState, Outcome, UnattributedReason};

fn path(display: &str) -> RepoPathInfo {
    RepoPathInfo {
        display: display.to_owned(),
        escaped: None,
    }
}

fn identical() -> BytesStatus {
    BytesStatus {
        base: "identical".to_owned(),
        target: "identical".to_owned(),
    }
}

fn input<'a>(
    path: &'a RepoPathInfo,
    status: FileStatus,
    base: Option<&'a [u8]>,
    target: Option<&'a [u8]>,
    attribution: AttributionState<'a>,
) -> FileInput<'a> {
    FileInput {
        path,
        status,
        old_mode: if base.is_some() { "100644" } else { "000000" }.to_owned(),
        new_mode: if target.is_some() { "100644" } else { "000000" }.to_owned(),
        old_blob: base.map(|_| "b1".to_owned()),
        new_blob: target.map(|_| "b2".to_owned()),
        kind: FileKindInfo::Text,
        bytes_status: identical(),
        base_blob: base,
        target_blob: target,
        attribution,
        mentions: Vec::new(),
    }
}

#[test]
fn modified_file_groups_lines_into_spans_removed_first() {
    // Base:   keep, old1, old2        Target: keep, new1, new2, extra
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![
            base_line(0),
            introduced("e1"),
            introduced("e1"),
            unexplained(),
        ],
        vec![kept(0), replaced("e1"), replaced("e1")],
    );
    let path = path("src/lib.rs");
    let review = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"keep\nold1\nold2\n"),
        Some(b"keep\nnew1\nnew2\nextra\n"),
        AttributionState::Available(&attr),
    ));
    assert_eq!(review.hunks.len(), 1);
    let hunk = &review.hunks[0];
    assert_eq!(
        (
            hunk.base_start,
            hunk.base_count,
            hunk.target_start,
            hunk.target_count
        ),
        (2, 2, 2, 3)
    );
    assert_eq!(hunk.removed, vec!["old1\n", "old2\n"]);
    assert_eq!(hunk.added, vec!["new1\n", "new2\n", "extra\n"]);
    let shape: Vec<(SpanSide, usize, usize)> = hunk
        .spans
        .iter()
        .map(|span| (span.side, span.start, span.count))
        .collect();
    assert_eq!(
        shape,
        vec![
            (SpanSide::Base, 2, 2),
            (SpanSide::Target, 2, 2),
            (SpanSide::Target, 4, 1),
        ]
    );
    assert!(matches!(hunk.spans[0].outcome, Outcome::Attributed { .. }));
    assert_eq!(hunk.spans[1].provenance.len(), 2);
    assert_eq!(
        hunk.spans[2].outcome,
        Outcome::Unattributed {
            reason: UnattributedReason::GapBeforeChain
        }
    );
    let info = review.attribution.unwrap();
    assert_eq!(info.chains.len(), 1);
    assert_eq!(info.unavailable, None);
}

#[test]
fn hunk_starts_follow_unified_numbering_for_empty_ranges() {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![base_line(0), introduced("e1"), base_line(1)],
        vec![kept(0), kept(2)],
    );
    let path = path("f.txt");
    let review = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"a\nc\n"),
        Some(b"a\nb\nc\n"),
        AttributionState::Available(&attr),
    ));
    let hunk = &review.hunks[0];
    // An insertion after base line 1: the base range is empty and names the
    // line before it; the target range starts at line 2.
    assert_eq!(
        (
            hunk.base_start,
            hunk.base_count,
            hunk.target_start,
            hunk.target_count
        ),
        (1, 0, 2, 1)
    );
    assert_eq!(hunk.spans.len(), 1);
    assert_eq!(hunk.spans[0].side, SpanSide::Target);
}

#[test]
fn hunk_lines_keep_their_terminators() {
    let attr = attribution(vec![], vec![unexplained()], vec![no_fate()]);
    let path = path("f.txt");
    let review = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"x\n"),
        Some(b"x"),
        AttributionState::Available(&attr),
    ));
    let hunk = &review.hunks[0];
    assert_eq!(hunk.removed, vec!["x\n"]);
    assert_eq!(hunk.added, vec!["x"]);
}

#[test]
fn added_and_deleted_files_have_spans_on_one_side() {
    let attr = attribution(vec![], vec![unexplained(), unexplained()], vec![]);
    let added_path = path("src/new.rs");
    let added = file_review(input(
        &added_path,
        FileStatus::Added,
        None,
        Some(b"a\nb\n"),
        AttributionState::Available(&attr),
    ));
    assert_eq!(added.old_mode, "000000");
    let hunk = &added.hunks[0];
    assert_eq!((hunk.base_start, hunk.base_count), (0, 0));
    assert_eq!(hunk.spans.len(), 1);
    assert_eq!(hunk.spans[0].side, SpanSide::Target);

    let gone = attribution(vec![], vec![], vec![no_fate()]);
    let deleted_path = path("old.txt");
    let mut deleted_input = input(
        &deleted_path,
        FileStatus::Deleted,
        Some(b"gone\n"),
        None,
        AttributionState::Available(&gone),
    );
    deleted_input.mentions = vec![Mention {
        command: "command:1".to_owned(),
        session: "s1".to_owned(),
        turn: "a6".to_owned(),
        checkout: "/work/repo".to_owned(),
        text: "rm old.txt".to_owned(),
        result: None,
    }];
    let deleted = file_review(deleted_input);
    let hunk = &deleted.hunks[0];
    assert_eq!((hunk.target_start, hunk.target_count), (0, 0));
    assert_eq!(hunk.spans[0].side, SpanSide::Base);
    assert_eq!(deleted.mentions.len(), 1);
}

#[test]
fn non_text_file_has_no_hunks_or_attribution() {
    let path = path("logo.png");
    let mut binary = input(
        &path,
        FileStatus::Modified,
        Some(b"\x89PNG\0"),
        Some(b"\x89PNG\0\0"),
        AttributionState::Unavailable {
            reason: "binary".to_owned(),
        },
    );
    binary.kind = FileKindInfo::Binary;
    let review = file_review(binary);
    assert!(review.hunks.is_empty());
    assert_eq!(review.attribution, None);
}

#[test]
fn unavailable_attribution_marks_every_line_and_the_file() {
    let path = path("big.lfs");
    let review = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"a\n"),
        Some(b"b\n"),
        AttributionState::Unavailable {
            reason: "conversion changed line structure".to_owned(),
        },
    ));
    let info = review.attribution.as_ref().unwrap();
    assert_eq!(info.status, None);
    assert_eq!(
        info.unavailable.as_deref(),
        Some("conversion changed line structure")
    );
    for span in &review.hunks[0].spans {
        assert!(matches!(
            span.outcome,
            Outcome::Unattributed {
                reason: UnattributedReason::AttributionUnavailable { .. }
            }
        ));
    }
}

#[test]
fn conflicting_edits_are_listed_on_the_file() {
    let conflict = Conflict {
        edit: eid("e9"),
        records: vec![RecordId::new("r1"), RecordId::new("r2")],
    };
    let mut attr = attribution(vec![], vec![unexplained()], vec![no_fate()]);
    attr.conflicts = vec![conflict.clone()];
    let path = path("src/lib.rs");
    let review = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"a\n"),
        Some(b"b\n"),
        AttributionState::Available(&attr),
    ));
    assert_eq!(
        review.attribution.unwrap().conflicting_edits,
        vec![conflict]
    );

    let unavailable = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"a\n"),
        Some(b"b\n"),
        AttributionState::Unavailable {
            reason: "conversion changed line structure".to_owned(),
        },
    ));
    assert!(
        unavailable
            .attribution
            .unwrap()
            .conflicting_edits
            .is_empty()
    );
}

#[test]
fn unattributed_summary_counts_incomplete_content_matching_apart() {
    let mut attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1"), unexplained(), unexplained()],
        vec![replaced("e1")],
    );
    attr.content.incomplete = true;
    let lib = path("src/lib.rs");
    let reviewed = file_review(input(
        &lib,
        FileStatus::Modified,
        Some(b"old\n"),
        Some(b"new\nmore\nmore2\n"),
        AttributionState::Available(&attr),
    ));
    let summary = unattributed_summary(&[reviewed], &ScopeInfo::default());
    assert_eq!(summary.files[0].unattributed, 0);
    assert_eq!(summary.files[0].content_match_incomplete, 2);
}

#[test]
fn unattributed_summary_counts_lines_and_lists_scope_items() {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1"), unexplained(), unexplained()],
        vec![replaced("e1")],
    );
    let lib = path("src/lib.rs");
    let reviewed = file_review(input(
        &lib,
        FileStatus::Modified,
        Some(b"old\n"),
        Some(b"new\nmore\nmore2\n"),
        AttributionState::Available(&attr),
    ));
    let png = path("logo.png");
    let mut binary_input = input(
        &png,
        FileStatus::Added,
        None,
        Some(b"\0"),
        AttributionState::Unavailable {
            reason: "binary".to_owned(),
        },
    );
    binary_input.kind = FileKindInfo::Binary;
    let binary = file_review(binary_input);
    let scope = ScopeInfo {
        untracked_excluded: vec![
            UntrackedInfo {
                path: "src/new.rs".to_owned(),
                recorded_edit: true,
                include_flag: "--include-untracked src/new.rs".to_owned(),
            },
            UntrackedInfo {
                path: "scratch.txt".to_owned(),
                recorded_edit: false,
                include_flag: "--include-untracked scratch.txt".to_owned(),
            },
        ],
        not_captured: vec![NotCapturedInfo {
            path: "config.toml".to_owned(),
            on_disk: "present".to_owned(),
            cause: "assume_unchanged".to_owned(),
        }],
        ..ScopeInfo::default()
    };
    let summary = unattributed_summary(&[reviewed, binary], &scope);
    assert_eq!(summary.files.len(), 1);
    assert_eq!(summary.files[0].path, "src/lib.rs");
    assert_eq!(summary.files[0].unattributed, 2);
    assert_eq!(summary.not_diffed.len(), 1);
    assert_eq!(summary.not_diffed[0].kind, "binary");
    assert_eq!(summary.untracked_with_recorded_edits, vec!["src/new.rs"]);
    assert_eq!(summary.not_captured, vec!["config.toml"]);
    assert!(summary.coarse.is_empty());
}
```

- [ ] **Step 11: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-review --test file_review`
Expected: compile errors for the missing model items (`file_review`, `FileInput`, `RepoPathInfo`, and the rest) and for `REVIEW_SCHEMA` in the crate root.

- [ ] **Step 12: Implement the model**

Replace `crates/supersigil-review/src/model.rs` with:

```rust
//! The JSON review model and the per-file assembly of hunks and spans.
//!
//! Field order in every struct is the order section 4 of the plan 2 design
//! lists, so the JSON reads top down: identity, then the prominent
//! unattributed summary, then scope, origins, records, evidence, files, and
//! the edits every other section refers to by id.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use supersigil_anchor::lines::split_lines;
use supersigil_anchor::{Chain, Conflict, PathStatus};
use supersigil_record::observations::{CaptureLimitation, EditOperation, Material, Role};

use crate::diff::{DiffHunk, diff_lines};
use crate::outcome::{
    AttributionState, Outcome, UnattributedReason, base_line_outcome, target_line_outcome,
};

/// Schema string of the review JSON; a version marker, not a promise, while
/// the pivot is unreleased.
pub const REVIEW_SCHEMA: &str = "supersigil.review/1";
/// Schema string of the `why` JSON.
pub const WHY_SCHEMA: &str = "supersigil.why/1";

/// A review of the change between a base and a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Review {
    /// [`REVIEW_SCHEMA`].
    pub schema: &'static str,
    /// The reviewed worktree.
    pub worktree: String,
    /// The base snapshot.
    pub base: BaseInfo,
    /// The target snapshot.
    pub target: TargetInfo,
    /// Everything no recorded edit explains, first so a reader meets it
    /// before any detail.
    pub unattributed: UnattributedSummary,
    /// The options used and the snapshot's full listing.
    pub scope: ScopeInfo,
    /// Which worktrees originated the reviewed commits.
    pub origins: OriginsInfo,
    /// Every involved record, with its pinned revision.
    pub records: Vec<RecordInfo>,
    /// Transcripts and capture limitations behind the attribution.
    pub evidence: EvidenceInfo,
    /// One entry per changed path in scope, sorted by path bytes.
    pub files: Vec<FileReview>,
    /// Every accepted edit referenced above, keyed by edit id.
    pub edits: BTreeMap<String, EditInfo>,
}

/// The base snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaseInfo {
    /// Base commit, or null for the empty tree.
    pub commit: Option<String>,
    /// Base tree id.
    pub tree: String,
    /// Whether the base is an ancestor of the target commit.
    pub ancestry: AncestryInfo,
}

/// Whether the base is an ancestor of the target commit; the same variants
/// as `supersigil_git::Ancestry`, which the CLI converts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AncestryInfo {
    /// The base is an ancestor of the target commit.
    Ancestor,
    /// It is not: the diff includes changes made on the base side.
    NotAncestor,
    /// Not checked: an empty-tree base or an unborn HEAD.
    Unavailable,
}

/// The target snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetInfo {
    /// Working tree or commit.
    pub kind: TargetKindInfo,
    /// The commit, or for a working tree the HEAD it was built over; null
    /// for a working tree over an unborn HEAD.
    pub commit: Option<String>,
    /// Target tree id.
    pub tree: String,
}

/// Kind of target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKindInfo {
    /// The reviewed worktree's on-disk state.
    WorkingTree,
    /// A commit.
    Commit,
}

/// Git status of a changed path; mirrors `supersigil_git::changes::ChangeStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    /// Only in the target.
    Added,
    /// Only in the base.
    Deleted,
    /// In both, with different contents or mode.
    Modified,
    /// A file became a symlink or gitlink, or the reverse.
    TypeChanged,
}

impl FileStatus {
    /// The one-letter code git uses for the status.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Modified => 'M',
            Self::TypeChanged => 'T',
        }
    }
}

/// How a changed path is handled; mirrors `supersigil_git::changes::FileKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKindInfo {
    /// Diffed and attributed.
    Text,
    /// Listed, not diffed.
    Binary,
    /// Over the size limit; listed, not diffed.
    TooLarge,
    /// Same blob, different mode; listed.
    ModeOnly,
    /// A symlink; listed, not diffed.
    Symlink,
    /// A submodule commit; listed, not diffed.
    Gitlink,
    /// A type change; listed, not diffed.
    TypeChange,
    /// A path that is not valid UTF-8; listed.
    UnsupportedPath,
}

impl FileKindInfo {
    /// The snake-case name the JSON uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Binary => "binary",
            Self::TooLarge => "too_large",
            Self::ModeOnly => "mode_only",
            Self::Symlink => "symlink",
            Self::Gitlink => "gitlink",
            Self::TypeChange => "type_change",
            Self::UnsupportedPath => "unsupported_path",
        }
    }
}

/// The prominent summary of everything no recorded edit explains.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UnattributedSummary {
    /// Per file with any such line, the counts by outcome.
    pub files: Vec<FileCounts>,
    /// Files whose diff is one coarse hunk.
    pub coarse: Vec<String>,
    /// Files whose attribution could not be computed.
    pub attribution_unavailable: Vec<String>,
    /// Files listed but not diffed.
    pub not_diffed: Vec<NotDiffed>,
    /// Untracked files a recorded edit wrote but the scope excluded.
    pub untracked_with_recorded_edits: Vec<String>,
    /// Paths whose on-disk state the snapshot did not capture.
    pub not_captured: Vec<String>,
}

/// Lines of one file by outcome that no chain attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileCounts {
    /// The file.
    pub path: String,
    /// Lines with the *unattributed* outcome, except those counted in
    /// `content_match_incomplete`.
    pub unattributed: usize,
    /// Lines with the *unattributed* outcome because content matching ran
    /// out of budget before finding a covering edit.
    pub content_match_incomplete: usize,
    /// Lines with the *unresolved* outcome.
    pub unresolved: usize,
    /// Lines with the *ambiguous* outcome.
    pub ambiguous: usize,
    /// Lines with the *line ending changed* outcome.
    pub line_ending: usize,
    /// Lines explained only by a content match.
    pub content_match_only: usize,
}

impl FileCounts {
    fn empty(path: String) -> Self {
        Self {
            path,
            unattributed: 0,
            content_match_incomplete: 0,
            unresolved: 0,
            ambiguous: 0,
            line_ending: 0,
            content_match_only: 0,
        }
    }

    const fn total(&self) -> usize {
        self.unattributed
            + self.content_match_incomplete
            + self.unresolved
            + self.ambiguous
            + self.line_ending
            + self.content_match_only
    }
}

/// A file listed but not diffed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotDiffed {
    /// The file.
    pub path: String,
    /// Its kind, as [`FileKindInfo::as_str`] spells it.
    pub kind: String,
}

/// The options used and the snapshot's listing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScopeInfo {
    /// Untracked paths included with `--include-untracked`.
    pub include_untracked: Vec<String>,
    /// Literal path prefixes the review is limited to.
    pub paths: Vec<String>,
    /// Untracked, non-ignored files the scope excluded.
    pub untracked_excluded: Vec<UntrackedInfo>,
    /// Paths whose on-disk state was not captured.
    pub not_captured: Vec<NotCapturedInfo>,
    /// Skip-worktree paths absent from disk that keep their index state.
    pub skip_worktree_absent: usize,
    /// Paths unmerged in the real index, taken in their on-disk form.
    pub unmerged: Vec<String>,
    /// Set when the base is not an ancestor of the target.
    pub ancestry_note: Option<String>,
    /// Set when a path selector named the worktree root, so the review has
    /// no path filter.
    pub path_note: Option<String>,
}

/// An untracked file the scope excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UntrackedInfo {
    /// The file.
    pub path: String,
    /// Whether a candidate edit in the reviewed worktree wrote this path.
    pub recorded_edit: bool,
    /// The flag that includes it, for example `--include-untracked src/new.rs`.
    pub include_flag: String,
}

/// A path whose on-disk state the snapshot did not capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotCapturedInfo {
    /// The path.
    pub path: String,
    /// `present` or `missing`.
    pub on_disk: String,
    /// `assume_unchanged`, `skip_worktree`, or `not_staged`.
    pub cause: String,
}

/// Which worktrees originated the reviewed commits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OriginsInfo {
    /// Per reviewed commit with an origin, its origin worktrees.
    pub commits: Vec<CommitOriginInfo>,
    /// Reviewed commits no worktree's reflog originates.
    pub without_origin: Vec<String>,
    /// Worktrees whose origin evidence is unavailable.
    pub unavailable: Vec<UnavailableInfo>,
}

/// A reviewed commit and its origin worktrees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitOriginInfo {
    /// The commit.
    pub commit: String,
    /// Worktrees whose own HEAD reflog has a commit-family entry for it.
    pub worktrees: Vec<String>,
}

/// A worktree whose origin evidence is unavailable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnavailableInfo {
    /// The worktree.
    pub worktree: String,
    /// Why, for example "directory missing".
    pub reason: String,
}

/// An involved record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordInfo {
    /// Record id.
    pub id: String,
    /// Pinned revision, null when the record could not be read.
    pub revision: Option<u64>,
    /// Whether reconcile ran for it.
    pub reconciled: bool,
    /// Why not, when it did not.
    pub reason: Option<String>,
}

/// Transcripts and capture limitations behind the attribution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EvidenceInfo {
    /// Every candidate transcript, whether or not its edits were attributed.
    pub candidate_transcripts: Vec<TranscriptInfo>,
    /// Edits whose checkout could not be placed in any worktree.
    pub unplaced_edits: usize,
    /// Checkouts whose discovered transcripts were not reconciled.
    pub unreconciled_checkouts: Vec<UnreconciledInfo>,
    /// Edits excluded because their sightings in several records disagree.
    pub conflicting_edits: Vec<Conflict>,
}

/// A candidate transcript and its capture limitations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptInfo {
    /// Canonical transcript path.
    pub transcript: String,
    /// Session id.
    pub session: String,
    /// Every capture limitation the record holds for it.
    pub capture_limitations: Vec<CaptureLimitation>,
    /// Always false: a limitation names a transcript range, not a path.
    pub localized: bool,
}

/// A checkout whose discovered transcripts were not reconciled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnreconciledInfo {
    /// The checkout.
    pub checkout: String,
    /// How many transcripts were discovered for it.
    pub transcripts: usize,
    /// Why they were not reconciled.
    pub reason: String,
}

/// One changed path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileReview {
    /// The path, lossy when not UTF-8.
    pub path: String,
    /// Reversible escaped form, present only when the path is not UTF-8.
    pub path_escaped: Option<String>,
    /// Git status.
    pub status: FileStatus,
    /// Base mode as octal text, `000000` when absent.
    pub old_mode: String,
    /// Target mode as octal text, `000000` when absent.
    pub new_mode: String,
    /// Base blob id.
    pub old_blob: Option<String>,
    /// Target blob id.
    pub new_blob: Option<String>,
    /// How the path is handled.
    pub kind: FileKindInfo,
    /// Attribution-bytes status per side.
    pub attribution_bytes: BytesStatus,
    /// Anchor's status and chains; null when the file is not diffed.
    pub attribution: Option<AttributionInfo>,
    /// Whether the diff is one coarse hunk.
    pub coarse: bool,
    /// Changed regions with their spans.
    pub hunks: Vec<HunkReview>,
    /// Commands whose text mentions the path (textual evidence, never
    /// attribution).
    pub mentions: Vec<Mention>,
}

/// Attribution-bytes status per side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BytesStatus {
    /// `identical`, `converted`, `failed: <stderr tail>`, or `absent`.
    pub base: String,
    /// Same values as `base`.
    pub target: String,
}

/// Anchor's result for a file, without the per-line detail spans carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttributionInfo {
    /// Anchor's status; null when attribution is unavailable.
    pub status: Option<PathStatus>,
    /// The chains not set aside.
    pub chains: Vec<Chain>,
    /// Alternatives that assume an unrecorded change.
    pub set_aside: Vec<Chain>,
    /// Why attribution is unavailable, when it is.
    pub unavailable: Option<String>,
    /// Edits excluded from this file as conflicting evidence: an edit id
    /// whose sightings in different records disagree is excluded from every
    /// file any sighting touched, and listed on each of them.
    pub conflicting_edits: Vec<Conflict>,
}

/// One changed region with its spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HunkReview {
    /// First removed base line, 1-based; for an empty range, the line
    /// before it (0 means before the first line).
    pub base_start: usize,
    /// Number of removed base lines.
    pub base_count: usize,
    /// First added target line, 1-based, with the same rule.
    pub target_start: usize,
    /// Number of added target lines.
    pub target_count: usize,
    /// Removed lines with their terminators.
    pub removed: Vec<String>,
    /// Added lines with their terminators.
    pub added: Vec<String>,
    /// Removed-line spans, then added-line spans, each in line order.
    pub spans: Vec<Span>,
}

/// Which side of the diff a span covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanSide {
    /// Removed base lines.
    Base,
    /// Added target lines.
    Target,
}

/// Consecutive diff lines on one side with the same outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    /// Which side.
    pub side: SpanSide,
    /// First line, 1-based.
    pub start: usize,
    /// Number of lines.
    pub count: usize,
    /// The shared outcome.
    pub outcome: Outcome,
    /// Each line's full provenance, in order.
    pub provenance: Vec<Value>,
}

/// A command whose text mentions a file's path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mention {
    /// Command event id.
    pub command: String,
    /// Session id.
    pub session: String,
    /// Turn that issued the command.
    pub turn: String,
    /// Checkout the command ran in.
    pub checkout: String,
    /// The command text.
    pub text: String,
    /// The recorded result, when one was recorded.
    pub result: Option<MentionResult>,
}

/// A mentioned command's recorded result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MentionResult {
    /// Exit status, when reported.
    pub exit: Option<i32>,
    /// `passed` or `failed`, when inferred.
    pub outcome: Option<String>,
}

/// An accepted edit referenced by the review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditInfo {
    /// Session id.
    pub session: String,
    /// Canonical transcript path.
    pub transcript: Option<String>,
    /// Turn that issued the edit.
    pub turn: String,
    /// Time of the edit.
    pub time: String,
    /// Worktree the edit maps to.
    pub worktree: String,
    /// Recorded editing operation.
    pub operation: EditOperation,
    /// The nearest recorded ancestor message that is Human or Delegation.
    pub prompt: Option<PromptInfo>,
}

/// The message that preceded an edit in the conversation; not a cause or a
/// rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptInfo {
    /// Turn id of the message.
    pub turn: String,
    /// Human or Delegation.
    pub role: Role,
    /// The retained excerpt, or why it is not retained.
    pub excerpt: Material<String>,
}

/// A changed path's display forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPathInfo {
    /// Lossy display name.
    pub display: String,
    /// Reversible escaped form when the path is not UTF-8.
    pub escaped: Option<String>,
}

/// Everything [`file_review`] needs for one changed path.
#[derive(Debug, Clone)]
pub struct FileInput<'a> {
    /// The path.
    pub path: &'a RepoPathInfo,
    /// Git status.
    pub status: FileStatus,
    /// Base mode as octal text.
    pub old_mode: String,
    /// Target mode as octal text.
    pub new_mode: String,
    /// Base blob id.
    pub old_blob: Option<String>,
    /// Target blob id.
    pub new_blob: Option<String>,
    /// How the path is handled; only `Text` is diffed.
    pub kind: FileKindInfo,
    /// Attribution-bytes status per side.
    pub bytes_status: BytesStatus,
    /// Base blob bytes; `None` for an added file.
    pub base_blob: Option<&'a [u8]>,
    /// Target blob bytes; `None` for a deleted file.
    pub target_blob: Option<&'a [u8]>,
    /// Anchor's result, or why there is none.
    pub attribution: AttributionState<'a>,
    /// Commands mentioning the path.
    pub mentions: Vec<Mention>,
}

/// Builds one file's review: the reviewed diff, each changed line's outcome
/// grouped into spans, and anchor's status and chains.
///
/// Only `Text` files are diffed; any other kind gets no hunks and no
/// attribution.
#[must_use]
pub fn file_review(input: FileInput<'_>) -> FileReview {
    let (attribution, coarse, hunks) = if input.kind == FileKindInfo::Text {
        let base = input.base_blob.unwrap_or_default();
        let target = input.target_blob.unwrap_or_default();
        let diff = diff_lines(base, target);
        let base_lines = split_lines(base);
        let target_lines = split_lines(target);
        let hunks = diff
            .hunks
            .iter()
            .map(|hunk| hunk_review(hunk, &base_lines, &target_lines, &input.attribution))
            .collect();
        (
            Some(attribution_info(&input.attribution)),
            diff.coarse,
            hunks,
        )
    } else {
        (None, false, Vec::new())
    };
    FileReview {
        path: input.path.display.clone(),
        path_escaped: input.path.escaped.clone(),
        status: input.status,
        old_mode: input.old_mode,
        new_mode: input.new_mode,
        old_blob: input.old_blob,
        new_blob: input.new_blob,
        kind: input.kind,
        attribution_bytes: input.bytes_status,
        attribution,
        coarse,
        hunks,
        mentions: input.mentions,
    }
}

/// Collects the prominent summary from the files and the scope listing.
#[must_use]
pub fn unattributed_summary(files: &[FileReview], scope: &ScopeInfo) -> UnattributedSummary {
    let mut summary = UnattributedSummary::default();
    for file in files {
        let mut counts = FileCounts::empty(file.path.clone());
        for span in file.hunks.iter().flat_map(|hunk| &hunk.spans) {
            let slot = match span.outcome {
                Outcome::Unattributed {
                    reason: UnattributedReason::ContentMatchIncomplete,
                } => &mut counts.content_match_incomplete,
                Outcome::Unattributed { .. } => &mut counts.unattributed,
                Outcome::Unresolved { .. } => &mut counts.unresolved,
                Outcome::Ambiguous { .. } => &mut counts.ambiguous,
                Outcome::LineEndingChanged => &mut counts.line_ending,
                Outcome::ContentMatch { .. } => &mut counts.content_match_only,
                Outcome::Attributed { .. }
                | Outcome::WhitespaceOnly { .. }
                | Outcome::Realigned { .. } => continue,
            };
            *slot += span.count;
        }
        if counts.total() > 0 {
            summary.files.push(counts);
        }
        if file.coarse {
            summary.coarse.push(file.path.clone());
        }
        if file
            .attribution
            .as_ref()
            .is_some_and(|info| info.unavailable.is_some())
        {
            summary.attribution_unavailable.push(file.path.clone());
        }
        if file.kind != FileKindInfo::Text {
            summary.not_diffed.push(NotDiffed {
                path: file.path.clone(),
                kind: file.kind.as_str().to_owned(),
            });
        }
    }
    summary.untracked_with_recorded_edits = scope
        .untracked_excluded
        .iter()
        .filter(|untracked| untracked.recorded_edit)
        .map(|untracked| untracked.path.clone())
        .collect();
    summary.not_captured = scope
        .not_captured
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    summary
}

fn attribution_info(state: &AttributionState<'_>) -> AttributionInfo {
    match state {
        AttributionState::Available(attr) => AttributionInfo {
            status: Some(attr.status.clone()),
            chains: attr.chains.clone(),
            set_aside: attr.set_aside.clone(),
            unavailable: None,
            conflicting_edits: attr.conflicts.clone(),
        },
        AttributionState::Unavailable { reason } => AttributionInfo {
            status: None,
            chains: Vec::new(),
            set_aside: Vec::new(),
            unavailable: Some(reason.clone()),
            conflicting_edits: Vec::new(),
        },
    }
}

fn hunk_review(
    hunk: &DiffHunk,
    base_lines: &[&[u8]],
    target_lines: &[&[u8]],
    attribution: &AttributionState<'_>,
) -> HunkReview {
    let removed = hunk.base_start..hunk.base_start + hunk.base_count;
    let added = hunk.target_start..hunk.target_start + hunk.target_count;
    let mut spans = Vec::new();
    push_spans(
        &mut spans,
        SpanSide::Base,
        hunk.base_start,
        removed
            .clone()
            .map(|line| base_line_outcome(attribution, line, base_lines, target_lines)),
    );
    push_spans(
        &mut spans,
        SpanSide::Target,
        hunk.target_start,
        added
            .clone()
            .map(|line| target_line_outcome(attribution, line, base_lines, target_lines)),
    );
    HunkReview {
        base_start: one_based_start(hunk.base_start, hunk.base_count),
        base_count: hunk.base_count,
        target_start: one_based_start(hunk.target_start, hunk.target_count),
        target_count: hunk.target_count,
        removed: base_lines[removed].iter().map(|line| text(line)).collect(),
        added: target_lines[added].iter().map(|line| text(line)).collect(),
        spans,
    }
}

/// Groups consecutive lines with equal outcomes, starting at 0-based `first`.
fn push_spans(
    spans: &mut Vec<Span>,
    side: SpanSide,
    first: usize,
    outcomes: impl Iterator<Item = (Outcome, Value)>,
) {
    let mut current: Option<Span> = None;
    for (offset, (outcome, provenance)) in outcomes.enumerate() {
        if let Some(span) = current.as_mut().filter(|span| span.outcome == outcome) {
            span.count += 1;
            span.provenance.push(provenance);
            continue;
        }
        spans.extend(current.take());
        current = Some(Span {
            side,
            start: first + offset + 1,
            count: 1,
            outcome,
            provenance: vec![provenance],
        });
    }
    spans.extend(current);
}

/// Unified-diff numbering: 1-based, and for an empty range the line before.
const fn one_based_start(start: usize, count: usize) -> usize {
    if count == 0 { start } else { start + 1 }
}

/// A line's text with its terminator; text files are UTF-8, so this is
/// lossless for every diffed file.
pub(crate) fn text(line: &[u8]) -> String {
    String::from_utf8_lossy(line).into_owned()
}
```

Removed-line spans come before added-line spans within a hunk, like a unified diff. A non-`Text` file gets no hunks and a null `attribution`; a `Text` file always gets `attribution`, with `unavailable` set when the CLI could not compute it, and with anchor's `conflicts` for the file as `conflicting_edits`. `unattributed_summary` counts every non-attributed outcome except whitespace-only and realigned lines, which an edit or every chain explains; unattributed lines whose reason is `content_match_incomplete` are counted in `content_match_incomplete`, not in `unattributed`.

- [ ] **Step 13: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-review`
Expected: 41 tests pass (13 unit, 19 in `outcome_rules`, 9 in `file_review`).

- [ ] **Step 14: Write the failing snapshot tests**

Add to `crates/supersigil-review/src/lib.rs`, after `pub mod outcome;`:

```rust
pub mod summary;
pub mod why;
```

Create `crates/supersigil-review/src/summary.rs` and `crates/supersigil-review/src/why.rs` containing only their first doc lines for now (`//! The terminal summary of a review, until plan 4's TUI replaces it.` and ``//! `supersigil why <file>:<line>`: one line's outcome, with the evidence context the review header shows.``).

`crates/supersigil-review/tests/snapshots.rs`:

```rust
//! Snapshots of the review JSON, the terminal summary, and `why`.

mod common;

use std::collections::BTreeMap;

use common::{
    attribution, base_line, chain, eid, escape, introduced, kept, no_fate, replaced, unexplained,
};
use supersigil_anchor::lines::split_lines;
use supersigil_anchor::{ChainClass, ChainEnd, PathAttribution, PathStatus, StopReason};
use supersigil_record::SessionId;
use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, EditOperation, Material, Role,
};
use supersigil_review::model::{
    AncestryInfo, BaseInfo, BytesStatus, EditInfo, EvidenceInfo, FileInput, FileKindInfo,
    FileReview, FileStatus, Mention, MentionResult, NotCapturedInfo, OriginsInfo, PromptInfo,
    RecordInfo, RepoPathInfo, ScopeInfo, TargetInfo, TargetKindInfo, TranscriptInfo,
    UnreconciledInfo, UntrackedInfo, file_review, unattributed_summary,
};
use supersigil_review::outcome::AttributionState;
use supersigil_review::summary::render_summary;
use supersigil_review::why::{OnDiskCheck, Tristate, Why, WhyTarget, render_why, why_line};
use supersigil_review::{REVIEW_SCHEMA, Review, WHY_SCHEMA};

const TRANSCRIPT: &str = "/home/u/.claude/projects/-work-repo/s1.jsonl";

fn bytes(base: &str, target: &str) -> BytesStatus {
    BytesStatus {
        base: base.to_owned(),
        target: target.to_owned(),
    }
}

fn records() -> Vec<RecordInfo> {
    vec![
        RecordInfo {
            id: "rec-1".to_owned(),
            revision: Some(4),
            reconciled: true,
            reason: None,
        },
        RecordInfo {
            id: "rec-2".to_owned(),
            revision: Some(2),
            reconciled: false,
            reason: Some("locked by another writer".to_owned()),
        },
    ]
}

fn evidence() -> EvidenceInfo {
    EvidenceInfo {
        candidate_transcripts: vec![TranscriptInfo {
            transcript: TRANSCRIPT.to_owned(),
            session: "s1".to_owned(),
            capture_limitations: vec![CaptureLimitation {
                session: SessionId::new("s1"),
                transcript: TRANSCRIPT.to_owned(),
                from_ordinal: 0,
                to_ordinal: 19,
                counts: CaptureCounts {
                    abandoned_tool_uses: 1,
                    ..CaptureCounts::default()
                },
            }],
            localized: false,
        }],
        unplaced_edits: 0,
        unreconciled_checkouts: vec![UnreconciledInfo {
            checkout: "/work/repo/.claude/worktrees/x".to_owned(),
            transcripts: 2,
            reason: "records directory locked".to_owned(),
        }],
        conflicting_edits: Vec::new(),
    }
}

fn edits() -> BTreeMap<String, EditInfo> {
    BTreeMap::from([(
        "e1".to_owned(),
        EditInfo {
            session: "s1".to_owned(),
            transcript: Some(TRANSCRIPT.to_owned()),
            turn: "a1".to_owned(),
            time: "2026-09-28T10:00:01.000Z".to_owned(),
            worktree: "/work/repo".to_owned(),
            operation: EditOperation::Replace,
            prompt: Some(PromptInfo {
                turn: "u1".to_owned(),
                role: Role::Human,
                excerpt: Material::Retained("Add a greeting function and clean up.".to_owned()),
            }),
        },
    )])
}

/// One text file's review; modes and blob ids follow from which sides exist.
fn text_file(
    path: &str,
    status: FileStatus,
    base: Option<&[u8]>,
    target: Option<&[u8]>,
    attr: &PathAttribution,
    mentions: Vec<Mention>,
) -> FileReview {
    let info = RepoPathInfo {
        display: path.to_owned(),
        escaped: None,
    };
    let mode = |side: Option<&[u8]>| if side.is_some() { "100644" } else { "000000" };
    let presence = |side: Option<&[u8]>| {
        if side.is_some() {
            "identical"
        } else {
            "absent"
        }
    };
    file_review(FileInput {
        path: &info,
        status,
        old_mode: mode(base).to_owned(),
        new_mode: mode(target).to_owned(),
        old_blob: base.map(|_| format!("{path}@base")),
        new_blob: target.map(|_| format!("{path}@target")),
        kind: FileKindInfo::Text,
        bytes_status: bytes(presence(base), presence(target)),
        base_blob: base,
        target_blob: target,
        attribution: AttributionState::Available(attr),
        mentions,
    })
}

/// A binary file, a file no chain explains, a file deleted by a command, and
/// a file one edit changed exactly from the base, sorted by path.
fn sample_files() -> Vec<FileReview> {
    let logo = RepoPathInfo {
        display: "logo.png".to_owned(),
        escaped: None,
    };
    let binary = file_review(FileInput {
        path: &logo,
        status: FileStatus::Modified,
        old_mode: "100644".to_owned(),
        new_mode: "100644".to_owned(),
        old_blob: Some("logo.png@base".to_owned()),
        new_blob: Some("logo.png@target".to_owned()),
        kind: FileKindInfo::Binary,
        bytes_status: bytes("identical", "identical"),
        base_blob: None,
        target_blob: None,
        attribution: AttributionState::Unavailable {
            reason: "binary".to_owned(),
        },
        mentions: Vec::new(),
    });
    let mut notes = attribution(vec![], vec![unexplained()], vec![no_fate()]);
    notes.status = PathStatus::NotComposed {
        reasons: vec![StopReason::NoAcceptedCandidate { edit: eid("e2") }],
    };
    let old = attribution(vec![], vec![], vec![no_fate()]);
    let rm = Mention {
        command: "command:1".to_owned(),
        session: "s1".to_owned(),
        turn: "a6".to_owned(),
        checkout: "/work/repo".to_owned(),
        text: "rm old.txt".to_owned(),
        result: Some(MentionResult {
            exit: Some(0),
            outcome: None,
        }),
    };
    let lib = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![base_line(0), introduced("e1")],
        vec![kept(0), replaced("e1")],
    );
    vec![
        binary,
        text_file(
            "notes.txt",
            FileStatus::Modified,
            Some(b"draft\n"),
            Some(b"final\n"),
            &notes,
            Vec::new(),
        ),
        text_file(
            "old.txt",
            FileStatus::Deleted,
            Some(b"gone\n"),
            None,
            &old,
            vec![rm],
        ),
        text_file(
            "src/lib.rs",
            FileStatus::Modified,
            Some(b"fn a() {}\nfn old() {}\n"),
            Some(b"fn a() {}\nfn new() {}\n"),
            &lib,
            Vec::new(),
        ),
    ]
}

fn sample_scope() -> ScopeInfo {
    ScopeInfo {
        untracked_excluded: vec![
            UntrackedInfo {
                path: "src/new.rs".to_owned(),
                recorded_edit: true,
                include_flag: "--include-untracked src/new.rs".to_owned(),
            },
            UntrackedInfo {
                path: "scratch.txt".to_owned(),
                recorded_edit: false,
                include_flag: "--include-untracked scratch.txt".to_owned(),
            },
        ],
        not_captured: vec![NotCapturedInfo {
            path: "config.toml".to_owned(),
            on_disk: "present".to_owned(),
            cause: "assume_unchanged".to_owned(),
        }],
        ..ScopeInfo::default()
    }
}

fn sample_review() -> Review {
    let files = sample_files();
    let scope = sample_scope();
    Review {
        schema: REVIEW_SCHEMA,
        worktree: "/work/repo".to_owned(),
        base: BaseInfo {
            commit: Some("a".repeat(40)),
            tree: "b".repeat(40),
            ancestry: AncestryInfo::Ancestor,
        },
        target: TargetInfo {
            kind: TargetKindInfo::WorkingTree,
            commit: Some("a".repeat(40)),
            tree: "c".repeat(40),
        },
        unattributed: unattributed_summary(&files, &scope),
        scope,
        origins: OriginsInfo::default(),
        records: records(),
        evidence: evidence(),
        files,
        edits: edits(),
    }
}

#[test]
fn review_json_shape_is_stable() {
    insta::assert_yaml_snapshot!(sample_review());
}

#[test]
fn terminal_summary_is_stable() {
    insta::assert_snapshot!(render_summary(&sample_review(), escape));
}

#[test]
fn terminal_summary_escapes_untrusted_text() {
    let mut review = sample_review();
    review.files[3].path = "src/\u{1b}[2Klib.rs".to_owned();
    let text = render_summary(&review, escape);
    assert!(text.contains(r"src/\x1b[2Klib.rs"));
    assert!(!text.contains('\u{1b}'));
}

fn sample_why(on_disk: OnDiskCheck) -> Why {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1"), unexplained()],
        vec![],
    );
    let target = b"fn greet() {}\nfn other() {}\n";
    let line = match on_disk {
        OnDiskCheck::Captured => Some(why_line(
            &AttributionState::Available(&attr),
            0,
            &split_lines(target),
            &[],
            Tristate::Yes,
            None,
        )),
        OnDiskCheck::NotCaptured { .. } => None,
    };
    Why {
        schema: WHY_SCHEMA,
        path: "src/lib.rs".to_owned(),
        target: WhyTarget {
            worktree: "/work/repo".to_owned(),
            tree: "c".repeat(40),
            blob: Some("src/lib.rs@target".to_owned()),
            attribution_bytes: "identical".to_owned(),
        },
        records: records(),
        evidence: evidence(),
        on_disk,
        line,
        edits: edits(),
        conflicting_edits: Vec::new(),
    }
}

#[test]
fn why_json_and_terminal_are_stable() {
    let why = sample_why(OnDiskCheck::Captured);
    insta::assert_yaml_snapshot!(why);
    insta::assert_snapshot!(render_why(&why, escape));
}

#[test]
fn terminal_summary_shows_the_path_note() {
    let mut review = sample_review();
    review.scope.path_note = Some(
        "a path selector named the worktree root, so the review has no path filter".to_owned(),
    );
    let text = render_summary(&review, escape);
    assert!(text.contains(
        "\nscope: a path selector named the worktree root, so the review has no path filter\n"
    ));
}

#[test]
fn why_says_why_differs_from_head_is_unknown() {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1")],
        vec![],
    );
    let line = why_line(
        &AttributionState::Available(&attr),
        0,
        &split_lines(b"fn greet() {}\n"),
        &[],
        Tristate::Unknown,
        Some("coarse diff".to_owned()),
    );
    assert_eq!(line.differs_from_head, Tristate::Unknown);
    assert_eq!(
        line.differs_from_head_reason.as_deref(),
        Some("coarse diff")
    );
    let mut why = sample_why(OnDiskCheck::Captured);
    why.line = Some(line);
    assert!(render_why(&why, escape).contains("\n  differs from HEAD: unknown (coarse diff)\n"));
}

#[test]
fn why_not_captured_keeps_the_evidence_context() {
    let why = sample_why(OnDiskCheck::NotCaptured {
        reason: "assume-unchanged".to_owned(),
    });
    assert_eq!(why.line, None);
    let text = render_why(&why, escape);
    assert!(text.starts_with("src/lib.rs: not captured: assume-unchanged"));
    assert!(text.contains("record rec-2 not reconciled: locked by another writer"));
    assert!(text.contains("capture limitations in"));
}
```

- [ ] **Step 15: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-review --test snapshots`
Expected: compile errors for `render_summary`, `render_why`, `why_line`, `Why`, and the other `why` items.

- [ ] **Step 16: Implement the terminal summary and `why`**

Replace `crates/supersigil-review/src/summary.rs` with:

```rust
//! The terminal summary of a review, until plan 4's TUI replaces it.
//!
//! Plain, deterministic text in four parts: a header (worktree, base,
//! target, anything not reconciled, and every candidate transcript's capture
//! limitations), the unattributed summary before anything else, one line per
//! file, and scope and origin notes. No diff text: the JSON carries it.
//!
//! Every string that comes from outside the tool (paths, ids, reasons,
//! transcript text) goes through the caller's `escape` function, which the
//! CLI supplies as its `format::escape_control`, so an escape sequence in a
//! path cannot rewrite the terminal.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use supersigil_anchor::ChainClass;
use supersigil_record::observations::{CaptureCounts, CaptureLimitation};

use crate::model::{
    AncestryInfo, EvidenceInfo, FileKindInfo, FileReview, RecordInfo, Review, SpanSide,
    TargetKindInfo,
};
use crate::outcome::{Outcome, Relation, UnattributedReason};

/// Renders `review` as the terminal summary, escaping untrusted text with
/// `escape`.
#[must_use]
pub fn render_summary(review: &Review, escape: fn(&str) -> String) -> String {
    let mut out = String::new();
    push(&mut out, &format!("review of {}", escape(&review.worktree)));
    let base = match &review.base.commit {
        Some(commit) => format!("  base: {commit} (tree {})", review.base.tree),
        None => format!("  base: empty tree {}", review.base.tree),
    };
    push(&mut out, &base);
    let target = match (review.target.kind, &review.target.commit) {
        (TargetKindInfo::Commit, Some(commit)) => format!("  target: commit {commit}"),
        (TargetKindInfo::WorkingTree, Some(commit)) => {
            format!("  target: working tree over {commit}")
        }
        (_, None) => "  target: working tree over an unborn HEAD".to_owned(),
    };
    push(&mut out, &format!("{target} (tree {})", review.target.tree));
    for line in context_lines(&review.records, &review.evidence, escape) {
        push(&mut out, &format!("  {line}"));
    }

    push(&mut out, "");
    let unattributed = unattributed_lines(review, escape);
    if unattributed.is_empty() {
        push(&mut out, "unattributed: nothing");
    } else {
        push(&mut out, "unattributed:");
        for line in unattributed {
            push(&mut out, &format!("  {line}"));
        }
    }

    push(&mut out, "");
    push(&mut out, "files:");
    if review.files.is_empty() {
        push(&mut out, "  no changes in scope");
    }
    for file in &review.files {
        push(&mut out, &format!("  {}", file_line(review, file, escape)));
    }

    let notes = notes(review, escape);
    if !notes.is_empty() {
        push(&mut out, "");
        for note in notes {
            push(&mut out, &note);
        }
    }
    out
}

/// Header lines shared with `why`: records and checkouts not reconciled,
/// then one line per candidate transcript with capture limitations.
pub(crate) fn context_lines(
    records: &[RecordInfo],
    evidence: &EvidenceInfo,
    escape: fn(&str) -> String,
) -> Vec<String> {
    let mut lines = Vec::new();
    for record in records.iter().filter(|record| !record.reconciled) {
        lines.push(format!(
            "record {} not reconciled: {}",
            escape(&record.id),
            escape(record.reason.as_deref().unwrap_or("unknown reason"))
        ));
    }
    for checkout in &evidence.unreconciled_checkouts {
        lines.push(format!(
            "checkout {} not reconciled ({} transcripts): {}",
            escape(&checkout.checkout),
            checkout.transcripts,
            escape(&checkout.reason)
        ));
    }
    for transcript in &evidence.candidate_transcripts {
        let counts = total_counts(&transcript.capture_limitations);
        let words = count_words(&counts);
        if !words.is_empty() {
            lines.push(format!(
                "capture limitations in {} (session {}): {}; not localized to a path",
                escape(&transcript.transcript),
                escape(&transcript.session),
                words.join(", ")
            ));
        }
    }
    lines
}

fn unattributed_lines(review: &Review, escape: fn(&str) -> String) -> Vec<String> {
    let summary = &review.unattributed;
    let mut lines = Vec::new();
    for untracked in review
        .scope
        .untracked_excluded
        .iter()
        .filter(|untracked| untracked.recorded_edit)
    {
        lines.push(format!(
            "{}: untracked, a recorded edit wrote this file; not included ({})",
            escape(&untracked.path),
            escape(&untracked.include_flag)
        ));
    }
    for counts in &summary.files {
        let parts: Vec<String> = [
            (counts.unattributed, "unattributed"),
            (
                counts.content_match_incomplete,
                "unattributed, content matching incomplete",
            ),
            (counts.unresolved, "unresolved"),
            (counts.ambiguous, "ambiguous"),
            (counts.line_ending, "line ending changed"),
            (counts.content_match_only, "content match only"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect();
        lines.push(format!("{}: {}", escape(&counts.path), parts.join(", ")));
    }
    for path in &summary.attribution_unavailable {
        lines.push(format!("{}: attribution unavailable", escape(path)));
    }
    for path in &summary.coarse {
        lines.push(format!("{}: coarse diff, too large to align", escape(path)));
    }
    for entry in &summary.not_diffed {
        lines.push(format!(
            "{}: not diffed: {}",
            escape(&entry.path),
            entry.kind
        ));
    }
    for path in &summary.not_captured {
        lines.push(format!("{}: on-disk state not captured", escape(path)));
    }
    lines
}

/// One line per file: status, path, line counts, and attribution in words.
fn file_line(review: &Review, file: &FileReview, escape: fn(&str) -> String) -> String {
    let path = escape(&file.path);
    let letter = file.status.letter();
    if file.kind != FileKindInfo::Text {
        return format!("{letter} {path}  not diffed: {}", file.kind.as_str());
    }
    let added: usize = file.hunks.iter().map(|hunk| hunk.target_count).sum();
    let removed: usize = file.hunks.iter().map(|hunk| hunk.base_count).sum();
    let mut introduced = 0;
    let mut replaced = 0;
    let mut other = Tally::default();
    let mut classes = BTreeSet::new();
    let mut sessions = BTreeSet::new();
    for span in file.hunks.iter().flat_map(|hunk| &hunk.spans) {
        match &span.outcome {
            Outcome::Attributed { edits } => {
                match span.side {
                    SpanSide::Target => introduced += span.count,
                    SpanSide::Base => replaced += span.count,
                }
                for edit in edits {
                    classes.extend(edit.classes.iter().copied());
                    if let Some(info) = review.edits.get(edit.edit.as_str()) {
                        sessions.insert(info.session.clone());
                    }
                }
            }
            outcome => other.add(outcome, span.count),
        }
    }
    let mut parts = Vec::new();
    if introduced > 0 {
        parts.push(format!("{introduced} introduced"));
    }
    if replaced > 0 {
        parts.push(format!("{replaced} replaced"));
    }
    if let Some(class) = classes.iter().min() {
        parts.push(class_words(*class).to_owned());
        let count = sessions.len();
        parts.push(format!(
            "{count} {}",
            if count == 1 { "session" } else { "sessions" }
        ));
    }
    parts.extend(other.words(escape));
    if let Some(info) = file
        .attribution
        .as_ref()
        .and_then(|a| a.unavailable.as_ref())
    {
        parts.push(format!("attribution unavailable: {}", escape(info)));
    }
    if file.coarse {
        parts.push("coarse diff".to_owned());
    }
    let words = if parts.is_empty() {
        "no line changes".to_owned()
    } else {
        parts.join(", ")
    };
    format!("{letter} {path}  +{added} -{removed}  {words}")
}

/// Counts of non-attributed outcomes on one file, and the first
/// unattributed reason.
#[derive(Default)]
struct Tally {
    whitespace: usize,
    realigned: usize,
    line_ending: usize,
    content_match: usize,
    ambiguous: usize,
    unresolved: usize,
    unattributed: usize,
    reason: Option<UnattributedReason>,
}

impl Tally {
    fn add(&mut self, outcome: &Outcome, count: usize) {
        match outcome {
            Outcome::WhitespaceOnly { .. } => self.whitespace += count,
            Outcome::Realigned { .. } => self.realigned += count,
            Outcome::LineEndingChanged => self.line_ending += count,
            Outcome::ContentMatch { .. } => self.content_match += count,
            Outcome::Ambiguous { .. } => self.ambiguous += count,
            Outcome::Unresolved { .. } => self.unresolved += count,
            Outcome::Unattributed { reason } => {
                self.unattributed += count;
                self.reason.get_or_insert_with(|| reason.clone());
            }
            Outcome::Attributed { .. } => {}
        }
    }

    fn words(&self, escape: fn(&str) -> String) -> Vec<String> {
        let mut words: Vec<String> = [
            (self.whitespace, "whitespace-only"),
            (self.realigned, "realigned"),
            (self.line_ending, "line ending changed"),
            (self.content_match, "content match"),
            (self.ambiguous, "ambiguous"),
            (self.unresolved, "unresolved"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect();
        if self.unattributed > 0 {
            let reason = self.reason.as_ref().map_or_else(String::new, |reason| {
                format!(": {}", reason_words(reason, escape))
            });
            words.push(format!("{} unattributed{reason}", self.unattributed));
        }
        words
    }
}

fn notes(review: &Review, escape: fn(&str) -> String) -> Vec<String> {
    let scope = &review.scope;
    let mut notes = Vec::new();
    if !scope.paths.is_empty() {
        let paths: Vec<String> = scope.paths.iter().map(|p| escape(p)).collect();
        notes.push(format!("scope: limited to {}", paths.join(", ")));
    }
    if let Some(note) = &scope.path_note {
        notes.push(format!("scope: {}", escape(note)));
    }
    for path in &scope.include_untracked {
        notes.push(format!("scope: untracked {} included", escape(path)));
    }
    let excluded = scope.untracked_excluded.len();
    if excluded > 0 {
        notes.push(format!("scope: {excluded} untracked files excluded"));
    }
    for entry in &scope.not_captured {
        notes.push(format!(
            "scope: {} not captured ({}, {} on disk)",
            escape(&entry.path),
            entry.cause,
            entry.on_disk
        ));
    }
    if scope.skip_worktree_absent > 0 {
        notes.push(format!(
            "scope: {} skip-worktree paths absent from disk keep their index state",
            scope.skip_worktree_absent
        ));
    }
    for path in &scope.unmerged {
        notes.push(format!(
            "scope: {} unmerged, taken as on disk",
            escape(path)
        ));
    }
    if let Some(note) = &scope.ancestry_note {
        notes.push(format!("scope: {}", escape(note)));
    } else if review.base.ancestry == AncestryInfo::NotAncestor {
        notes.push(
            "scope: the base is not an ancestor of the target; the diff includes changes made on the base side"
                .to_owned(),
        );
    }
    let origins = &review.origins;
    let reviewed = origins.commits.len() + origins.without_origin.len();
    if reviewed > 0 {
        notes.push(format!(
            "origins: {reviewed} reviewed commits, {} without an origin worktree",
            origins.without_origin.len()
        ));
    }
    for unavailable in &origins.unavailable {
        notes.push(format!(
            "origins: {}: origin evidence unavailable: {}",
            escape(&unavailable.worktree),
            escape(&unavailable.reason)
        ));
    }
    notes
}

pub(crate) const fn class_words(class: ChainClass) -> &'static str {
    match class {
        ChainClass::ExactFromBase => "exact from the base",
        ChainClass::ExactFromStart => "exact from its start",
        ChainClass::Consistent => "consistent with the recorded edits, not verified",
    }
}

pub(crate) fn reason_words(reason: &UnattributedReason, escape: fn(&str) -> String) -> String {
    match reason {
        UnattributedReason::NoSurvivingChain { .. } => "no surviving chain".to_owned(),
        UnattributedReason::GapBeforeChain => "gap before the chain's start".to_owned(),
        UnattributedReason::ContentMatchIncomplete => {
            "content matching ran out of budget; a covering edit may exist".to_owned()
        }
        UnattributedReason::AttributionUnavailable { reason } => {
            format!("attribution unavailable: {}", escape(reason))
        }
    }
}

pub(crate) const fn relation_words(relation: Relation) -> &'static str {
    match relation {
        Relation::Introduced => "introduced",
        Relation::Replaced => "replaced",
    }
}

/// Sums the counts of every capture limitation of one transcript.
fn total_counts(limitations: &[CaptureLimitation]) -> CaptureCounts {
    let mut total = CaptureCounts::default();
    for limitation in limitations {
        let counts = &limitation.counts;
        for (kind, count) in &counts.unknown_records {
            *total.unknown_records.entry(kind.clone()).or_default() += count;
        }
        total.malformed_lines += counts.malformed_lines;
        total.abandoned_tool_uses += counts.abandoned_tool_uses;
        total.failed_tool_uses += counts.failed_tool_uses;
        total.outside_checkout += counts.outside_checkout;
        total.conflicting_tool_results += counts.conflicting_tool_results;
        total.unmatched_tool_results += counts.unmatched_tool_results;
        total.session_mismatch += counts.session_mismatch;
        total.unnamed_tool_uses += counts.unnamed_tool_uses;
        total.unsupported_tool_uses += counts.unsupported_tool_uses;
    }
    total
}

fn count_words(counts: &CaptureCounts) -> Vec<String> {
    let unknown: u64 = counts.unknown_records.values().sum();
    [
        (unknown, "unknown records"),
        (counts.malformed_lines, "malformed lines"),
        (counts.abandoned_tool_uses, "tool uses without a result"),
        (counts.failed_tool_uses, "failed editing tool uses"),
        (
            counts.outside_checkout,
            "edits or commands outside the checkout",
        ),
        (counts.conflicting_tool_results, "conflicting tool results"),
        (counts.unmatched_tool_results, "unmatched tool results"),
        (counts.session_mismatch, "records of another session"),
        (counts.unnamed_tool_uses, "tool uses without an id"),
        (counts.unsupported_tool_uses, "unsupported tool uses"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{count} {label}"))
    .collect()
}

fn push(out: &mut String, line: &str) {
    let _ = writeln!(out, "{line}");
}
```

Replace `crates/supersigil-review/src/why.rs` with:

```rust
//! `supersigil why <file>:<line>`: one line's outcome, with the evidence
//! context the review header shows.
//!
//! The line is read against the target alone: its outcome follows the same
//! ordered rules as an added diff line (a line the chains do not explain gets
//! the content-match fallback). The records and candidate transcripts are
//! always present, whatever the on-disk check found.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;
use serde_json::Value;
use supersigil_anchor::{Chain, Conflict, PathStatus};
use supersigil_record::observations::Material;

use crate::model::{EditInfo, EvidenceInfo, RecordInfo, text};
use crate::outcome::{AttributionState, Outcome, target_line_outcome};
use crate::summary::{class_words, context_lines, reason_words, relation_words};

/// The `why` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Why {
    /// [`crate::WHY_SCHEMA`].
    pub schema: &'static str,
    /// The path as the user named it, relative to the worktree.
    pub path: String,
    /// What was read.
    pub target: WhyTarget,
    /// Every involved record, with reconcile status; always present.
    pub records: Vec<RecordInfo>,
    /// Candidate transcripts and their capture limitations; always present.
    pub evidence: EvidenceInfo,
    /// Whether the line analysis read what is on disk.
    pub on_disk: OnDiskCheck,
    /// The line's analysis; null when the on-disk check failed.
    pub line: Option<WhyLine>,
    /// The edits this path's analysis refers to, keyed by edit id.
    pub edits: BTreeMap<String, EditInfo>,
    /// Edits for this path excluded as conflicting evidence.
    pub conflicting_edits: Vec<Conflict>,
}

/// What `why` read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WhyTarget {
    /// The worktree.
    pub worktree: String,
    /// The captured working-tree tree.
    pub tree: String,
    /// The path's blob in that tree.
    pub blob: Option<String>,
    /// Attribution-bytes status: `identical`, `converted`, or `failed: ...`.
    pub attribution_bytes: String,
}

/// Whether the captured target matches the file on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OnDiskCheck {
    /// The captured bytes are what is on disk, up to line endings.
    Captured,
    /// They differ; no line is explained.
    NotCaptured {
        /// Why, for example "assume-unchanged".
        reason: String,
    },
}

/// Whether the line differs from HEAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tristate {
    /// It differs.
    Yes,
    /// It is the same.
    No,
    /// Unknown: the file is not in HEAD, HEAD is unborn, or the line lies
    /// in a coarse hunk.
    Unknown,
}

/// One line's analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WhyLine {
    /// Line number, 1-based.
    pub number: usize,
    /// Line text with its terminator.
    pub text: String,
    /// Whether the line differs from HEAD.
    pub differs_from_head: Tristate,
    /// Why `differs_from_head` is unknown, for example "coarse diff"; null
    /// otherwise.
    pub differs_from_head_reason: Option<String>,
    /// Anchor's status for the path; null when attribution is unavailable.
    pub status: Option<PathStatus>,
    /// The chains not set aside.
    pub chains: Vec<Chain>,
    /// Alternatives that assume an unrecorded change.
    pub set_aside: Vec<Chain>,
    /// The line's outer outcome.
    pub outcome: Outcome,
    /// The line's full provenance.
    pub provenance: Value,
}

/// Analyzes 0-based target line `line` by the same ordered rules as an added
/// diff line.
#[must_use]
pub fn why_line(
    attribution: &AttributionState<'_>,
    line: usize,
    target_blob_lines: &[&[u8]],
    base_blob_lines: &[&[u8]],
    differs_from_head: Tristate,
    differs_from_head_reason: Option<String>,
) -> WhyLine {
    let (outcome, provenance) =
        target_line_outcome(attribution, line, base_blob_lines, target_blob_lines);
    let (status, chains, set_aside) = match attribution {
        AttributionState::Available(attr) => (
            Some(attr.status.clone()),
            attr.chains.clone(),
            attr.set_aside.clone(),
        ),
        AttributionState::Unavailable { .. } => (None, Vec::new(), Vec::new()),
    };
    WhyLine {
        number: line + 1,
        text: target_blob_lines
            .get(line)
            .map(|line| text(line))
            .unwrap_or_default(),
        differs_from_head,
        differs_from_head_reason,
        status,
        chains,
        set_aside,
        outcome,
        provenance,
    }
}

/// Renders `why` for a terminal, escaping untrusted text with `escape`.
#[must_use]
pub fn render_why(why: &Why, escape: fn(&str) -> String) -> String {
    let mut out = String::new();
    let path = escape(&why.path);
    match (&why.on_disk, &why.line) {
        (OnDiskCheck::NotCaptured { reason }, _) => {
            push(
                &mut out,
                &format!(
                    "{path}: not captured: {}; no line explained",
                    escape(reason)
                ),
            );
        }
        (OnDiskCheck::Captured, None) => push(&mut out, &format!("{path}: no line analyzed")),
        (OnDiskCheck::Captured, Some(line)) => {
            let content = line.text.trim_end_matches(['\n', '\r']);
            push(
                &mut out,
                &format!("{path}:{}: {}", line.number, escape(content)),
            );
            push(
                &mut out,
                &format!("  {}", outcome_sentence(&line.outcome, escape)),
            );
            let differs = match line.differs_from_head {
                Tristate::Yes => "yes",
                Tristate::No => "no",
                Tristate::Unknown => "unknown",
            };
            match &line.differs_from_head_reason {
                Some(reason) => push(
                    &mut out,
                    &format!("  differs from HEAD: {differs} ({})", escape(reason)),
                ),
                None => push(&mut out, &format!("  differs from HEAD: {differs}")),
            }
        }
    }
    let mut edits: Vec<(&String, &EditInfo)> = why.edits.iter().collect();
    edits.sort_by(|a, b| (&a.1.time, a.0).cmp(&(&b.1.time, b.0)));
    for (id, edit) in edits {
        let prompt = edit.prompt.as_ref().map_or_else(
            || "no preceding message recorded".to_owned(),
            |prompt| match &prompt.excerpt {
                Material::Retained(excerpt) => {
                    let first = excerpt.lines().next().unwrap_or_default();
                    let short: String = first.chars().take(80).collect();
                    format!("after \"{}\"", escape(&short))
                }
                Material::Withheld { policy } => {
                    format!("preceding message withheld ({})", escape(policy))
                }
                Material::Unavailable { reason } => {
                    format!("preceding message unavailable ({})", escape(reason))
                }
            },
        );
        push(
            &mut out,
            &format!(
                "  edit {} at {}, session {}: {prompt}",
                escape(id),
                escape(&edit.time),
                escape(&edit.session)
            ),
        );
    }
    for line in context_lines(&why.records, &why.evidence, escape) {
        push(&mut out, &format!("  {line}"));
    }
    out
}

fn outcome_sentence(outcome: &Outcome, escape: fn(&str) -> String) -> String {
    let ids = |edits: &mut dyn Iterator<Item = String>| -> String {
        edits
            .map(|id| escape(&id))
            .collect::<Vec<_>>()
            .join(" and ")
    };
    match outcome {
        Outcome::Attributed { edits } => {
            let parts: Vec<String> = edits
                .iter()
                .map(|edit| {
                    let class = edit
                        .classes
                        .iter()
                        .min()
                        .map_or("", |class| class_words(*class));
                    format!(
                        "{} by edit {} ({class})",
                        relation_words(edit.relation),
                        escape(edit.edit.as_str())
                    )
                })
                .collect();
            parts.join("; ")
        }
        Outcome::ContentMatch { edits, .. } => format!(
            "matches the text of edit {} (content match, not a composed chain)",
            ids(&mut edits.iter().map(|e| e.as_str().to_owned()))
        ),
        Outcome::WhitespaceOnly { edits } => format!(
            "only whitespace changed, by edit {}",
            ids(&mut edits.iter().map(|e| e.as_str().to_owned()))
        ),
        Outcome::Unattributed { reason } => {
            format!("not attributed: {}", reason_words(reason, escape))
        }
        Outcome::Ambiguous { .. } => {
            "ambiguous: the recorded edits allow more than one history for this line".to_owned()
        }
        Outcome::Unresolved { .. } => {
            "unresolved: the search did not complete within its budget".to_owned()
        }
        Outcome::LineEndingChanged => {
            "only the line ending differs; no recorded edit explains it".to_owned()
        }
        Outcome::Realigned { line } => format!("unchanged from line {}", line + 1),
    }
}

fn push(out: &mut String, line: &str) {
    let _ = writeln!(out, "{line}");
}
```

After this step `crates/supersigil-review/src/lib.rs` reads:

```rust
//! Builds the review model: the reviewed diff between two snapshots, one
//! outcome per changed line, and the JSON and terminal forms of both.
//!
//! Everything here is pure; the CLI gathers the inputs. [`diff::diff_lines`]
//! computes the reviewed diff over blob bytes. [`mapping::lines_correspond`]
//! decides whether anchor's provenance, computed over worktree-form bytes,
//! maps line for line onto that diff. [`outcome`] turns each diff line's
//! provenance into one outer outcome by the design's ordered rules, and
//! [`model::file_review`] assembles hunks and spans. [`summary`] and [`why`]
//! render the terminal forms.

pub mod diff;
pub mod mapping;
pub mod model;
pub mod outcome;
pub mod summary;
pub mod why;

pub use model::{REVIEW_SCHEMA, Review, WHY_SCHEMA};
```

- [ ] **Step 17: Create and review the snapshots**

Run: `INSTA_UPDATE=always cargo nextest run -p supersigil-review --test snapshots`
Expected: 7 tests pass; four files appear under `crates/supersigil-review/tests/snapshots/`: `snapshots__review_json_shape_is_stable.snap`, `snapshots__terminal_summary_is_stable.snap`, `snapshots__why_json_and_terminal_are_stable.snap` (the `why` JSON), and `snapshots__why_json_and_terminal_are_stable-2.snap` (the `why` terminal form).

Read each one before committing. The terminal summary snapshot's body must be exactly:

```text
review of /work/repo
  base: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa (tree bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb)
  target: working tree over aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa (tree cccccccccccccccccccccccccccccccccccccccc)
  record rec-2 not reconciled: locked by another writer
  checkout /work/repo/.claude/worktrees/x not reconciled (2 transcripts): records directory locked
  capture limitations in /home/u/.claude/projects/-work-repo/s1.jsonl (session s1): 1 tool uses without a result; not localized to a path

unattributed:
  src/new.rs: untracked, a recorded edit wrote this file; not included (--include-untracked src/new.rs)
  notes.txt: 2 unattributed
  old.txt: 1 unattributed
  logo.png: not diffed: binary
  config.toml: on-disk state not captured

files:
  M logo.png  not diffed: binary
  M notes.txt  +1 -1  2 unattributed: no surviving chain
  D old.txt  +0 -1  1 unattributed: no surviving chain
  M src/lib.rs  +1 -1  1 introduced, 1 replaced, exact from the base, 1 session

scope: 2 untracked files excluded
scope: config.toml not captured (assume_unchanged, present on disk)
```

The `why` terminal snapshot's body must be exactly:

```text
src/lib.rs:1: fn greet() {}
  introduced by edit e1 (exact from the base)
  differs from HEAD: yes
  edit e1 at 2026-09-28T10:00:01.000Z, session s1: after "Add a greeting function and clean up."
  record rec-2 not reconciled: locked by another writer
  checkout /work/repo/.claude/worktrees/x not reconciled (2 transcripts): records directory locked
  capture limitations in /home/u/.claude/projects/-work-repo/s1.jsonl (session s1): 1 tool uses without a result; not localized to a path
```

In the review JSON snapshot, the top-level keys must appear in this order: `schema`, `worktree`, `base`, `target`, `unattributed`, `scope`, `origins`, `records`, `evidence`, `files`, `edits` (the design's section 4 order; serde keeps struct field order, while maps nested inside `provenance` and `readings` are anchor values serialized by `serde_json` and come out with sorted keys). The `src/lib.rs` entry must carry one hunk (`base_start: 2`, `target_start: 2`) whose spans are a `base` span with outcome `attributed` (`relation: replaced`, `classes: [exact_from_base]`) followed by a `target` span with outcome `attributed` (`relation: introduced`); `old.txt` has one `base` span with outcome `unattributed` (`reason.kind: no_surviving_chain`) and the `rm old.txt` mention; `logo.png` has `kind: binary`, `attribution: ~`, and no hunks.

- [ ] **Step 18: Run the full checks**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no formatting changes, no clippy warnings, every test passes (48 in `supersigil-review`).

- [ ] **Step 19: Commit**

```bash
git add Cargo.lock crates/supersigil-review/Cargo.toml crates/supersigil-review/src/lib.rs crates/supersigil-review/src/diff.rs crates/supersigil-review/src/mapping.rs crates/supersigil-review/src/outcome.rs crates/supersigil-review/src/model.rs crates/supersigil-review/src/summary.rs crates/supersigil-review/src/why.rs crates/supersigil-review/tests/common/mod.rs crates/supersigil-review/tests/outcome_rules.rs crates/supersigil-review/tests/file_review.rs crates/supersigil-review/tests/snapshots.rs crates/supersigil-review/tests/snapshots/snapshots__review_json_shape_is_stable.snap crates/supersigil-review/tests/snapshots/snapshots__terminal_summary_is_stable.snap crates/supersigil-review/tests/snapshots/snapshots__why_json_and_terminal_are_stable.snap crates/supersigil-review/tests/snapshots/snapshots__why_json_and_terminal_are_stable-2.snap
git commit -m "feat: build the review model from line attribution"
```

---

### Task 12: Involved records and bounded reconcile

**Files:**
- Modify: `crates/supersigil-session/src/discover.rs` (split `transcripts_in` out of `discover_transcripts`)
- Modify: `crates/supersigil-cli/src/record_dir.rs` (`Acquired`, `open_or_create_record_within`, shared helpers)
- Create: `crates/supersigil-cli/src/reconcile.rs`
- Modify: `crates/supersigil-cli/src/lib.rs`
- Test: `crates/supersigil-cli/tests/reconcile.rs`

**Interfaces:**
- Consumes: plan 1's `supersigil_session::sync::{sync, SyncError}`, `supersigil_session::discover::{discover_transcripts, encode_project_dir}`, `supersigil_session::claude_code::parse_transcript`, `supersigil_session::checkout::{canonical, placement, Placement}`, `supersigil_record::store::{Store, StoreError, RecordSnapshot, Association}`, `supersigil_record::observations::Observation`, and the CLI's `record_dir::{open_or_create_record, find_owner}`.
- Produces (contract): `supersigil_cli::record_dir::{Acquired, open_or_create_record_within}`; `supersigil_cli::reconcile::{LOCK_WAIT, InvolvedRecord, UnreconciledCheckout, Reconciliation, involved_records, transcripts_below, reconcile}`. `reconcile(records_dir, claude_home, worktrees, main_worktree, wait)` takes the root of the repository's main worktree, where ancestor discovery stops (F8).
- Produces (additions to the contract): `supersigil_session::discover::transcripts_in(project_dir: &Path) -> std::io::Result<Vec<PathBuf>>` (the scan of one project directory that `discover_transcripts` already did, now callable for a directory found by name); `pub(crate) const RETRY: Duration` in `record_dir` (50 ms between lock attempts, shared with `reconcile`).

Design section 1, steps 3 and 4, and section 4's lock rules. The records a review reads are found by association, not by the single nearest ancestor `find_record` returns: a subdirectory record (`session sync --checkout <repo>/src`), a sibling worktree's record, and an ancestor's record are all involved. Every lock is bounded: `session sync` keeps its blocking `open_or_create_record`, and review uses the bounded variant.

Discovery also covers the ancestors of a nested worktree (F8, which resolves open decision 2). Claude Code names a session's project directory after the directory it started in, so a session in the main checkout that edited `.claude/worktrees/feature/src/lib.rs` lives under the main checkout's project directory, not under the worktree's. For each candidate worktree, `reconcile` reads the project directory whose encoded name is exactly that of each proper ancestor of the worktree, up to and including `main_worktree`, and admits a transcript found there when the working directory it records lies at or below that ancestor. The admitted transcripts get a record through `open_or_create_record_within` as `session sync` would, so a review of a nested worktree finds such an edit even when the records directory starts empty.

- [ ] **Step 1: Write the failing test for `transcripts_in`**

Append to the `tests` module at the end of `crates/supersigil-session/src/discover.rs`:

```rust
    #[test]
    fn transcripts_in_scans_one_project_directory() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("-work-repo--claude-worktrees-x");
        let subagents = dir.join("s1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        std::fs::write(dir.join("s1.jsonl"), "").unwrap();
        std::fs::write(subagents.join("agent-y.jsonl"), "").unwrap();
        assert_eq!(
            transcripts_in(&dir).unwrap(),
            vec![dir.join("s1.jsonl"), subagents.join("agent-y.jsonl")]
        );
        assert!(
            transcripts_in(&home.path().join("missing"))
                .unwrap()
                .is_empty()
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo nextest run -p supersigil-session discover`
Expected: compile error, `cannot find function 'transcripts_in' in this scope`.

- [ ] **Step 3: Split the scan out of `discover_transcripts`**

In `crates/supersigil-session/src/discover.rs`, replace the whole `discover_transcripts` function (doc comment included) with:

```rust
/// Lists the main and subagent transcripts for `checkout`.
///
/// `claude_home` is normally `~/.claude`. Searches its `projects` directory
/// using [`encode_project_dir`] on the supplied checkout path, as
/// [`transcripts_in`] describes. Returns an empty vector if the project
/// directory is missing.
///
/// # Errors
///
/// Returns an I/O error if an existing project or subagent directory, or an
/// entry needed for the scan, cannot be read.
pub fn discover_transcripts(checkout: &Path, claude_home: &Path) -> std::io::Result<Vec<PathBuf>> {
    transcripts_in(
        &claude_home
            .join("projects")
            .join(encode_project_dir(checkout)),
    )
}

/// Lists the main and subagent transcripts in one Claude Code project directory.
///
/// Returns main `.jsonl` paths first, then `<session id>/subagents/agent-*.jsonl`
/// paths, with each group sorted by path. Ignores other files and directories.
/// Returns an empty vector if `project_dir` is missing.
///
/// # Errors
///
/// Returns an I/O error if an existing project or subagent directory, or an
/// entry needed for the scan, cannot be read.
pub fn transcripts_in(project_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let Some(entries) = read_dir_if_present(project_dir)? else {
        return Ok(Vec::new());
    };
    let mut main = Vec::new();
    let mut subagents = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            subagents.extend(subagent_transcripts(&path.join("subagents"))?);
        } else if is_jsonl(&path) {
            main.push(path);
        }
    }
    main.sort();
    subagents.sort();
    main.append(&mut subagents);
    Ok(main)
}
```

- [ ] **Step 4: Run the discovery tests**

Run: `cargo nextest run -p supersigil-session discover`
Expected: 3 tests pass, including `transcripts_in_scans_one_project_directory` and plan 1's `discovers_sorted_jsonl_files_for_the_checkout_only`.

- [ ] **Step 5: Write the failing reconcile tests**

Create `crates/supersigil-cli/tests/reconcile.rs`. Every lock test holds one lock at a time with the other free, and asserts timing only through a generous watchdog, never against the retry bound (design section 5, layer 5):

```rust
//! Involved records and bounded reconcile, tested against the library.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use supersigil_cli::reconcile::{LOCK_WAIT, involved_records, reconcile, transcripts_below};
use supersigil_cli::record_dir::open_or_create_record;
use supersigil_record::store::{Association, Store};
use supersigil_session::checkout::canonical;
use supersigil_session::discover::encode_project_dir;
use supersigil_session::sync::sync;

/// A records directory, a Claude home, and a worktree directory, all under
/// one canonical temporary directory.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    records: PathBuf,
    home: PathBuf,
    repo: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = canonical(dir.path()).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    Fixture {
        records: root.join("records"),
        home: root.join("claude"),
        repo,
        root,
        _dir: dir,
    }
}

/// A two-record transcript of `session` working in `cwd`.
fn transcript(session: &str, cwd: &Path) -> String {
    let cwd = cwd.to_string_lossy();
    let line = |uuid: &str, parent: Option<&str>, kind: &str, content: serde_json::Value| {
        let mut text = serde_json::json!({
            "type": kind,
            "uuid": uuid,
            "parentUuid": parent,
            "sessionId": session,
            "cwd": cwd,
            "timestamp": format!("2026-09-29T10:00:0{}.000Z", uuid.len()),
            "isSidechain": false,
            "message": {"role": kind, "content": content},
        })
        .to_string();
        text.push('\n');
        text
    };
    line("u1", None, "user", serde_json::json!("hello"))
        + &line(
            "a1",
            Some("u1"),
            "assistant",
            serde_json::json!([{"type": "text", "text": "hi"}]),
        )
}

/// Writes `text` as `<name>` in the Claude Code project directory of `cwd`.
fn project_transcript(home: &Path, cwd: &Path, name: &str, text: &str) -> PathBuf {
    let dir = home.join("projects").join(encode_project_dir(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn sessions(store: &Store) -> Vec<String> {
    store
        .snapshot()
        .unwrap()
        .sessions()
        .iter()
        .map(|s| s.as_str().to_owned())
        .collect()
}

fn create_record(records: &Path, name: &str, checkout: &Path) -> Store {
    Store::create(
        &records.join(name),
        Association {
            checkout: checkout.to_path_buf(),
        },
    )
    .unwrap()
}

#[test]
fn involved_records_follow_associations() {
    let f = fixture();
    let src = f.repo.join("src");
    let other = f.root.join("other");
    let exact = create_record(&f.records, "a", &f.repo);
    let inside = create_record(&f.records, "b", &src);
    let containing = create_record(&f.records, "c", &f.root);
    create_record(&f.records, "d", &other);

    let roots: Vec<PathBuf> = involved_records(&f.records, std::slice::from_ref(&f.repo))
        .unwrap()
        .iter()
        .map(|s| s.root().to_path_buf())
        .collect();
    assert_eq!(
        roots,
        vec![
            exact.root().to_path_buf(),
            inside.root().to_path_buf(),
            containing.root().to_path_buf(),
        ]
    );
    assert!(
        involved_records(&f.root.join("no-records"), std::slice::from_ref(&f.repo))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn subdirectory_record_is_involved() {
    let f = fixture();
    let src = f.repo.join("src");
    std::fs::create_dir_all(&src).unwrap();
    // What `session sync --checkout <repo>/src` leaves behind.
    let store = open_or_create_record(&f.records, &src).unwrap();
    project_transcript(&f.home, &src, "sub.jsonl", &transcript("s-sub", &src));

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    assert_eq!(result.records.len(), 1);
    let involved = &result.records[0];
    assert_eq!(involved.store.root(), store.root());
    assert_eq!(involved.record_id, store.manifest().unwrap().record_id);
    assert_eq!(involved.not_reconciled, None);
    assert_eq!(sessions(&involved.store), vec!["s-sub"]);
    assert!(result.unreconciled.is_empty());
}

#[test]
fn two_records_cover_one_worktree() {
    let f = fixture();
    let a = f.repo.join("a");
    let b = f.repo.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    open_or_create_record(&f.records, &a).unwrap();
    open_or_create_record(&f.records, &b).unwrap();
    project_transcript(&f.home, &a, "a.jsonl", &transcript("s-a", &a));
    project_transcript(&f.home, &b, "b.jsonl", &transcript("s-b", &b));

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    let mut all: Vec<String> = result
        .records
        .iter()
        .flat_map(|r| sessions(&r.store))
        .collect();
    all.sort();
    assert_eq!(result.records.len(), 2);
    assert_eq!(all, vec!["s-a", "s-b"]);
}

#[test]
fn cursor_only_transcript_is_resynced() {
    let f = fixture();
    let elsewhere = f.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let path = elsewhere.join("t.jsonl");
    std::fs::write(&path, transcript("s-cursor", &f.repo)).unwrap();
    let store = open_or_create_record(&f.records, &f.repo).unwrap();
    sync(&store, &f.repo, std::slice::from_ref(&path)).unwrap();
    let before = store.manifest().unwrap();

    let mut more = std::fs::read_to_string(&path).unwrap();
    more.push_str(
        &serde_json::json!({
            "type": "user",
            "uuid": "u2",
            "parentUuid": "a1",
            "sessionId": "s-cursor",
            "cwd": f.repo.to_string_lossy(),
            "timestamp": "2026-09-29T10:01:00.000Z",
            "isSidechain": false,
            "message": {"role": "user", "content": "again"},
        })
        .to_string(),
    );
    more.push('\n');
    std::fs::write(&path, more).unwrap();

    // No Claude home: the transcript is reachable only through the cursor.
    let result = reconcile(
        &f.records,
        None,
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    let after = result.records[0].store.manifest().unwrap();
    let key = canonical(&path).unwrap().display().to_string();
    assert!(after.revision > before.revision);
    assert!(after.cursors[&key].offset > before.cursors[&key].offset);
}

#[test]
fn transcripts_below_finds_the_worktree_and_nested_project_dirs() {
    let f = fixture();
    let nested = f.repo.join(".claude").join("worktrees").join("x");
    let lookalike = f.root.join("repo-other");
    let own = project_transcript(&f.home, &f.repo, "a.jsonl", "");
    let inner = project_transcript(&f.home, &nested, "b.jsonl", "");
    let similar = project_transcript(&f.home, &lookalike, "c.jsonl", "");
    project_transcript(&f.home, &f.root.join("unrelated"), "d.jsonl", "");

    assert_eq!(
        transcripts_below(&f.home, &f.repo).unwrap(),
        vec![own, inner, similar]
    );
    assert!(
        transcripts_below(&f.root.join("no-home"), &f.repo)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unowned_nested_transcript_gets_a_record() {
    let f = fixture();
    let nested = f.repo.join(".claude").join("worktrees").join("x");
    std::fs::create_dir_all(&nested).unwrap();
    project_transcript(
        &f.home,
        &nested,
        "n.jsonl",
        &transcript("s-nested", &nested),
    );

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    assert_eq!(result.records.len(), 1);
    assert_eq!(sessions(&result.records[0].store), vec!["s-nested"]);
    let associations = result.records[0].store.manifest().unwrap().associations;
    assert_eq!(associations.len(), 1);
    assert_eq!(associations[0].checkout, nested);
}

#[test]
fn lookalike_project_dir_is_not_admitted() {
    let f = fixture();
    let lookalike = f.root.join("repo-other");
    std::fs::create_dir_all(&lookalike).unwrap();
    project_transcript(
        &f.home,
        &lookalike,
        "o.jsonl",
        &transcript("s-other", &lookalike),
    );

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    assert!(result.records.is_empty());
    assert!(result.unreconciled.is_empty());
}

#[test]
fn writer_lock_contention_is_reported() {
    let f = fixture();
    let store = open_or_create_record(&f.records, &f.repo).unwrap();
    project_transcript(&f.home, &f.repo, "t.jsonl", &transcript("s-held", &f.repo));
    // The stop hook, say, holding the record's writer lock.
    let held = store.begin().unwrap();

    let started = Instant::now();
    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        Duration::from_millis(200),
    )
    .unwrap();

    // A watchdog only: timing is never asserted against the retry bound.
    assert!(started.elapsed() < Duration::from_secs(30));
    assert_eq!(result.records.len(), 1);
    assert_eq!(
        result.records[0].not_reconciled.as_deref(),
        Some("locked by another writer")
    );
    assert!(sessions(&result.records[0].store).is_empty());
    drop(held);
}

#[test]
fn records_dir_lock_contention_is_reported() {
    let f = fixture();
    project_transcript(&f.home, &f.repo, "t.jsonl", &transcript("s-new", &f.repo));
    std::fs::create_dir_all(&f.records).unwrap();
    let guard = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(f.records.join(".lock"))
        .unwrap();
    guard.lock().unwrap();

    let started = Instant::now();
    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&f.repo),
        &f.repo,
        Duration::from_millis(200),
    )
    .unwrap();

    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(result.records.is_empty());
    assert_eq!(result.unreconciled.len(), 1);
    let unreconciled = &result.unreconciled[0];
    assert_eq!(unreconciled.checkout, f.repo);
    assert_eq!(unreconciled.transcripts, 1);
    assert_eq!(
        unreconciled.reason,
        "records directory locked by another writer"
    );
    drop(guard);
}

#[test]
fn ancestor_project_dir_is_reconciled_for_a_nested_worktree() {
    let f = fixture();
    let nested = f.repo.join(".claude").join("worktrees").join("x");
    std::fs::create_dir_all(&nested).unwrap();
    // A session in the main checkout: its project directory is named after
    // the main checkout, not after the nested worktree under review.
    project_transcript(
        &f.home,
        &f.repo,
        "main.jsonl",
        &transcript("s-main", &f.repo),
    );
    // Above the main worktree is outside the repository: never read.
    project_transcript(
        &f.home,
        &f.root,
        "above.jsonl",
        &transcript("s-above", &f.root),
    );

    let result = reconcile(
        &f.records,
        Some(&f.home),
        std::slice::from_ref(&nested),
        &f.repo,
        LOCK_WAIT,
    )
    .unwrap();

    assert_eq!(result.records.len(), 1);
    assert_eq!(sessions(&result.records[0].store), vec!["s-main"]);
    let associations = result.records[0].store.manifest().unwrap().associations;
    assert_eq!(associations[0].checkout, f.repo);
    assert!(result.unreconciled.is_empty());
}
```

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil --test reconcile`
Expected: compile errors, `unresolved import supersigil_cli::reconcile`.

- [ ] **Step 7: Bound the records-directory lock**

In `crates/supersigil-cli/src/record_dir.rs`, replace the imports at the top:

```rust
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
```

with:

```rust
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
```

and replace the whole `open_or_create_record` function (doc comment included) with:

```rust
/// Finds the record for `checkout` or creates one at `<records_dir>/<id>`.
/// Lookup and creation happen under an exclusive lock on
/// `<records_dir>/.lock`, so two first syncs racing on one checkout cannot
/// both create a record. A record found through an ancestor of `checkout`
/// gains `checkout` itself as an association before it is returned.
///
/// Waits as long as the lock is held; [`open_or_create_record_within`] is the
/// bounded variant.
///
/// # Errors
///
/// Returns a store error if the record cannot be created, read, or written,
/// or an I/O error if the records directory or its lock cannot be created.
pub fn open_or_create_record(records_dir: &Path, checkout: &Path) -> Result<Store, CliError> {
    let guard = records_lock_file(records_dir)?;
    guard.lock()?;
    let store = match find_owner(records_dir, checkout)? {
        Some((store, 0)) => store,
        Some((store, _)) => {
            let mut tx = store.begin()?;
            tx.add_association(Association {
                checkout: checkout.to_path_buf(),
            });
            tx.commit()?;
            store
        }
        None => create_record(records_dir, checkout)?,
    };
    drop(guard);
    Ok(store)
}

/// A lock-protected value, or the report that a lock stayed held.
#[derive(Debug)]
pub enum Acquired<T> {
    /// The locks were taken and the value produced.
    Ready(T),
    /// A lock was still held by another writer when the wait ran out.
    Busy,
}

/// [`open_or_create_record`] with every lock wait bounded by `wait`.
///
/// Tries the records-directory lock, and the found record's writer lock
/// when `checkout` must be added as an association, every 50 ms until
/// `wait` has passed since the call began. Returns [`Acquired::Busy`]
/// instead of blocking when either stays held, so a review never waits on a
/// stop hook indefinitely.
///
/// # Errors
///
/// Returns a store error if the record cannot be created, read, or written
/// for a reason other than a held lock, or an I/O error if the records
/// directory or its lock cannot be created or locked.
pub fn open_or_create_record_within(
    records_dir: &Path,
    checkout: &Path,
    wait: Duration,
) -> Result<Acquired<Store>, CliError> {
    let deadline = Instant::now() + wait;
    let guard = records_lock_file(records_dir)?;
    loop {
        match guard.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(RETRY);
            }
            Err(TryLockError::WouldBlock) => return Ok(Acquired::Busy),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
    }
    let acquired = match find_owner(records_dir, checkout)? {
        Some((store, 0)) => Acquired::Ready(store),
        Some((store, _)) => loop {
            match store.begin() {
                Ok(mut tx) => {
                    tx.add_association(Association {
                        checkout: checkout.to_path_buf(),
                    });
                    tx.commit()?;
                    break Acquired::Ready(store);
                }
                Err(StoreError::Locked(_)) if Instant::now() < deadline => {
                    std::thread::sleep(RETRY);
                }
                Err(StoreError::Locked(_)) => break Acquired::Busy,
                Err(e) => return Err(e.into()),
            }
        },
        None => Acquired::Ready(create_record(records_dir, checkout)?),
    };
    drop(guard);
    Ok(acquired)
}

/// Interval between attempts on a held lock.
pub(crate) const RETRY: Duration = Duration::from_millis(50);

/// Opens `<records_dir>/.lock`, creating the directory and the file if needed.
///
/// # Errors
///
/// Returns an I/O error if either cannot be created or opened.
fn records_lock_file(records_dir: &Path) -> Result<File, CliError> {
    std::fs::create_dir_all(records_dir)?;
    Ok(OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(records_dir.join(".lock"))?)
}

/// Creates a record at `<records_dir>/<new id>` associated with `checkout`.
///
/// # Errors
///
/// Returns a store error if the record cannot be created.
fn create_record(records_dir: &Path, checkout: &Path) -> Result<Store, CliError> {
    let id = RecordId::generate();
    let root = records_dir.join(id.as_str());
    Ok(Store::create_with_id(
        &root,
        id,
        Association {
            checkout: checkout.to_path_buf(),
        },
    )?)
}
```

- [ ] **Step 8: Write the reconcile module**

Create `crates/supersigil-cli/src/reconcile.rs`:

```rust
//! Brings every record a review involves up to date before it is read.
//!
//! One review can involve several records: a sibling worktree outside the
//! main checkout has its own, and `session sync --checkout <subdirectory>`
//! creates one associated only with that subdirectory. [`reconcile`] finds
//! them all, syncs each from the transcripts it can reach, and bounds every
//! lock wait, so a stop hook holding a record never blocks a review; what it
//! could not reconcile is reported, never hidden.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use supersigil_record::RecordId;
use supersigil_record::observations::Observation;
use supersigil_record::store::{RecordSnapshot, Store, StoreError};
use supersigil_session::checkout::{Placement, canonical, placement};
use supersigil_session::claude_code::parse_transcript;
use supersigil_session::discover::{discover_transcripts, encode_project_dir, transcripts_in};
use supersigil_session::sync::{SyncError, sync};

use crate::error::CliError;
use crate::record_dir::{Acquired, RETRY, open_or_create_record_within};

/// How long reconcile waits for each lock before reporting it held.
pub const LOCK_WAIT: Duration = Duration::from_secs(2);

/// Reason given for a record whose writer lock stayed held.
const WRITER_LOCKED: &str = "locked by another writer";
/// Reason given for a checkout whose record could not be created or
/// associated because the records-directory lock stayed held.
const DIRECTORY_LOCKED: &str = "records directory locked by another writer";

/// A record the review reads, and whether reconcile brought it up to date.
#[derive(Debug)]
pub struct InvolvedRecord {
    /// The record.
    pub store: Store,
    /// The record's id, read from its manifest.
    pub record_id: RecordId,
    /// Why the record was not reconciled, or `None` if it was.
    pub not_reconciled: Option<String>,
}

/// A checkout whose discovered transcripts have no record yet and could not
/// get one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreconciledCheckout {
    /// Working directory the transcripts recorded.
    pub checkout: PathBuf,
    /// Number of transcripts discovered for it.
    pub transcripts: usize,
    /// Why no record was created or associated.
    pub reason: String,
}

/// The records a review involves after reconciling them, and what was left out.
#[derive(Debug)]
pub struct Reconciliation {
    /// Every involved record, sorted by record directory.
    pub records: Vec<InvolvedRecord>,
    /// Checkouts with discovered transcripts that no record could take.
    pub unreconciled: Vec<UnreconciledCheckout>,
}

/// Finds every record with an association equal to, inside, or containing
/// one of `worktrees`, sorted by record directory.
///
/// Paths are compared with [`placement`], which normalizes their spelling.
/// A worktree can have several records (a sibling worktree's own, a
/// subdirectory's), and one record can cover several worktrees. Entries of
/// `records_dir` without a manifest are skipped; a missing `records_dir`
/// has no records.
///
/// # Errors
///
/// Returns [`CliError::Io`] if `records_dir` exists but cannot be read, or a
/// store error if a record's manifest cannot be accessed or read.
pub fn involved_records(records_dir: &Path, worktrees: &[PathBuf]) -> Result<Vec<Store>, CliError> {
    let entries = match std::fs::read_dir(records_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        candidates.push(entry?.path());
    }
    candidates.sort();
    let mut involved = Vec::new();
    for candidate in candidates {
        let store = match Store::open(&candidate) {
            Ok(store) => store,
            Err(StoreError::NotARecord(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let touches = store.manifest()?.associations.iter().any(|a| {
            worktrees.iter().any(|w| {
                placement(&a.checkout, w) != Placement::Outside
                    || placement(w, &a.checkout) != Placement::Outside
            })
        });
        if touches {
            involved.push(store);
        }
    }
    Ok(involved)
}

/// Lists the transcripts in the Claude Code project directories at or below
/// `worktree`: the directory whose name is `worktree` encoded, and every one
/// whose name extends it with `-` (a nested worktree such as
/// `.claude/worktrees/<name>`, or a session started in a subdirectory).
///
/// The encoding maps every non-alphanumeric character to `-`, so a sibling
/// such as `repo-other` looks nested too; callers admit transcripts by the
/// working directory they record. Directories are scanned in name order;
/// within each, main transcripts come before subagent ones. A missing
/// `projects` directory has no transcripts.
///
/// # Errors
///
/// Returns an I/O error if an existing directory or entry cannot be read.
pub fn transcripts_below(claude_home: &Path, worktree: &Path) -> std::io::Result<Vec<PathBuf>> {
    let projects = claude_home.join("projects");
    let own = encode_project_dir(worktree);
    let nested = format!("{own}-");
    let entries = match std::fs::read_dir(&projects) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if (name == own || name.starts_with(&nested)) && entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    let mut found = Vec::new();
    for dir in dirs {
        found.extend(transcripts_in(&dir)?);
    }
    Ok(found)
}

/// Syncs every record `worktrees` involve and returns them with what could
/// not be reconciled.
///
/// 1. Each record from [`involved_records`] is synced once per association,
///    with that association as the checkout, from the transcripts
///    discovered for it under `claude_home` plus the transcripts the record
///    already holds cursors for whose recorded checkout lies under it (a
///    cursor whose session never recorded a checkout is fed to every
///    association; sync admits it only where it belongs). A transcript
///    that no longer exists is left for later.
/// 2. Transcripts that step 1 did not read are gathered from two places
///    for each worktree: the project directories at or below it
///    ([`transcripts_below`]), whose transcripts are admitted when the
///    working directory they record lies inside the worktree; and the
///    project directory of each of its ancestors up to and including
///    `main_worktree`, the root of the repository's main worktree, whose
///    transcripts are admitted when their working directory lies at or
///    below that ancestor. The ancestors cover a session in the main
///    checkout that edited files inside a nested worktree: Claude Code
///    names its project directory after the main checkout. Admitted
///    transcripts are grouped by the working directory they record and get
///    a record exactly as `session sync` would create or associate one.
///    Directories whose encoded name only looks like the right one are
///    rejected by the working directory check.
///
/// Every lock is tried for up to `wait`: a record whose writer lock stays
/// held is returned with [`InvolvedRecord::not_reconciled`] set, and a
/// checkout whose record cannot be created or associated because the
/// records-directory lock stays held is returned in
/// [`Reconciliation::unreconciled`] with its transcript count.
///
/// # Errors
///
/// Returns [`CliError::Io`] if a directory or transcript cannot be read, a
/// store error if a record cannot be read or written for a reason other
/// than a held lock, or a sync error if a transcript cannot be synced.
pub fn reconcile(
    records_dir: &Path,
    claude_home: Option<&Path>,
    worktrees: &[PathBuf],
    main_worktree: &Path,
    wait: Duration,
) -> Result<Reconciliation, CliError> {
    let mut reasons: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut fed: BTreeSet<PathBuf> = BTreeSet::new();
    for store in involved_records(records_dir, worktrees)? {
        if let Some(reason) = sync_record(&store, claude_home, wait, &mut fed)? {
            reasons.insert(store.root().to_path_buf(), reason);
        }
    }

    let mut unreconciled = Vec::new();
    if let Some(home) = claude_home {
        for (checkout, transcripts) in unowned_transcripts(home, worktrees, main_worktree, &fed)? {
            match open_or_create_record_within(records_dir, &checkout, wait)? {
                Acquired::Ready(store) => {
                    if let Some(reason) = sync_within(&store, &checkout, &transcripts, wait)? {
                        reasons.insert(store.root().to_path_buf(), reason);
                    }
                }
                Acquired::Busy => unreconciled.push(UnreconciledCheckout {
                    checkout,
                    transcripts: transcripts.len(),
                    reason: DIRECTORY_LOCKED.to_owned(),
                }),
            }
        }
    }

    let mut records = Vec::new();
    for store in involved_records(records_dir, worktrees)? {
        let record_id = store.manifest()?.record_id;
        let not_reconciled = reasons.get(store.root()).cloned();
        records.push(InvolvedRecord {
            store,
            record_id,
            not_reconciled,
        });
    }
    Ok(Reconciliation {
        records,
        unreconciled,
    })
}

/// Syncs one record per association, adding every transcript it reads to
/// `fed`. Returns the reason it stopped early, if a lock stayed held.
///
/// # Errors
///
/// Returns the errors of [`reconcile`].
fn sync_record(
    store: &Store,
    claude_home: Option<&Path>,
    wait: Duration,
    fed: &mut BTreeSet<PathBuf>,
) -> Result<Option<String>, CliError> {
    let snapshot = store.snapshot()?;
    let recorded = recorded_checkouts(&snapshot)?;
    let manifest = snapshot.manifest();
    for association in &manifest.associations {
        let mut transcripts: Vec<PathBuf> = Vec::new();
        if let Some(home) = claude_home {
            transcripts.extend(discover_transcripts(&association.checkout, home)?);
        }
        for key in manifest.cursors.keys() {
            let under = recorded.get(key).is_none_or(|checkout| {
                placement(checkout, &association.checkout) != Placement::Outside
            });
            let path = PathBuf::from(key);
            if under && path.is_file() && !transcripts.contains(&path) {
                transcripts.push(path);
            }
        }
        if transcripts.is_empty() {
            continue;
        }
        fed.extend(transcripts.iter().map(|t| identity(t)));
        if let Some(reason) = sync_within(store, &association.checkout, &transcripts, wait)? {
            return Ok(Some(reason));
        }
    }
    Ok(None)
}

/// Maps each transcript path the record holds a cursor for to the checkout
/// its session start recorded.
///
/// # Errors
///
/// Returns a store error if the snapshot's logs cannot be read.
fn recorded_checkouts(snapshot: &RecordSnapshot) -> Result<BTreeMap<String, PathBuf>, CliError> {
    let mut checkouts = BTreeMap::new();
    for session in snapshot.sessions() {
        for observation in snapshot.observations(&session)? {
            if let Observation::SessionStart(start) = observation
                && let Some(path) = start.source_ids.get("path")
            {
                checkouts.entry(path.clone()).or_insert(start.checkout);
            }
        }
    }
    Ok(checkouts)
}

/// Transcripts that no involved record read, grouped by the canonical
/// working directory they record: those below each worktree whose working
/// directory lies inside it, and those in the project directory of each of
/// the worktree's ancestors up to `main_worktree` whose working directory
/// lies at or below that ancestor.
///
/// # Errors
///
/// Returns [`CliError::Io`] if a directory or transcript cannot be read.
fn unowned_transcripts(
    claude_home: &Path,
    worktrees: &[PathBuf],
    main_worktree: &Path,
    fed: &BTreeSet<PathBuf>,
) -> Result<BTreeMap<PathBuf, Vec<PathBuf>>, CliError> {
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for worktree in worktrees {
        let mut sources: Vec<(PathBuf, PathBuf)> = transcripts_below(claude_home, worktree)?
            .into_iter()
            .map(|transcript| (transcript, worktree.clone()))
            .collect();
        for ancestor in ancestors_within(worktree, main_worktree) {
            let dir = claude_home
                .join("projects")
                .join(encode_project_dir(&ancestor));
            for transcript in transcripts_in(&dir)? {
                sources.push((transcript, ancestor.clone()));
            }
        }
        for (transcript, admit_under) in sources {
            if fed.contains(&identity(&transcript)) {
                continue;
            }
            let bytes = std::fs::read(&transcript)?;
            let Some(cwd) = parse_transcript(&bytes, 0).checkout else {
                continue;
            };
            if placement(&cwd, &admit_under) == Placement::Outside {
                continue;
            }
            let key = canonical(&cwd).unwrap_or(cwd);
            let group = groups.entry(key).or_default();
            if !group.contains(&transcript) {
                group.push(transcript);
            }
        }
    }
    Ok(groups)
}

/// The proper ancestors of `worktree` that lie at or below `main_worktree`,
/// nearest first: none for the main worktree itself or for a sibling
/// worktree outside it.
fn ancestors_within(worktree: &Path, main_worktree: &Path) -> Vec<PathBuf> {
    worktree
        .ancestors()
        .skip(1)
        .take_while(|ancestor| placement(ancestor, main_worktree) != Placement::Outside)
        .map(Path::to_path_buf)
        .collect()
}

/// Syncs `transcripts` into `store` for `checkout`, retrying every 50 ms
/// while the record is locked or another writer commits first, for up to
/// `wait`. Returns the reason when the wait runs out.
///
/// # Errors
///
/// Returns a sync error other than a held lock or a lost race.
fn sync_within(
    store: &Store,
    checkout: &Path,
    transcripts: &[PathBuf],
    wait: Duration,
) -> Result<Option<String>, CliError> {
    let deadline = Instant::now() + wait;
    loop {
        match sync(store, checkout, transcripts) {
            Ok(_) => return Ok(None),
            Err(SyncError::Store(StoreError::Locked(_) | StoreError::Conflict { .. })) => {
                if Instant::now() >= deadline {
                    return Ok(Some(WRITER_LOCKED.to_owned()));
                }
                std::thread::sleep(RETRY);
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// The path sync keys a transcript by: canonical when it resolves.
fn identity(path: &Path) -> PathBuf {
    canonical(path).unwrap_or_else(|_| path.to_path_buf())
}
```

In `crates/supersigil-cli/src/lib.rs`, add before `pub mod record_dir;`:

```rust
/// Syncing every record a review involves, with bounded lock waits.
pub mod reconcile;
```

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil --test reconcile`
Expected: 10 tests pass. The two lock tests each take about 0.2 s (their bounded wait); the others finish in milliseconds.

- [ ] **Step 10: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; every test passes (plan 1's `cmd_session` tests exercise the rewritten `open_or_create_record` unchanged).

- [ ] **Step 11: Commit**

```bash
git add crates/supersigil-session/src/discover.rs crates/supersigil-cli/src/record_dir.rs crates/supersigil-cli/src/reconcile.rs crates/supersigil-cli/src/lib.rs crates/supersigil-cli/tests/reconcile.rs
git commit -m "feat: reconcile every record a review involves"
```

---

### Task 13: `review` and `why` commands

**Files:**
- Modify: `crates/supersigil-cli/Cargo.toml` (depend on the three new crates)
- Modify: `crates/supersigil-cli/src/error.rs` (`CliError::Git`)
- Modify: `crates/supersigil-cli/src/format.rs` (`AutoFormat`)
- Create: `crates/supersigil-cli/src/mapping.rs`
- Create: `crates/supersigil-cli/src/evidence.rs`
- Create: `crates/supersigil-cli/src/pipeline.rs`
- Create: `crates/supersigil-cli/src/commands/review.rs`
- Create: `crates/supersigil-cli/src/commands/why.rs`
- Modify: `crates/supersigil-cli/src/commands.rs`, `crates/supersigil-cli/src/lib.rs`, `crates/supersigil-cli/src/main.rs`
- Modify: `crates/supersigil-session/tests/fixtures/slice.jsonl` (make `toolu_05`'s recorded hunk describe the file)
- Test: unit tests in `mapping.rs`, `evidence.rs`, `pipeline.rs`, `format.rs`, `commands/why.rs`; `crates/supersigil-cli/tests/cmd_review.rs`

**Interfaces:**
- Consumes:
  - Task 1: `supersigil_record::observations::EditOperation` and `Edit::operation`.
  - Tasks 4 to 7 (`supersigil-git`): `Git::new`, `Repo::{open, root, empty_tree, commit_tree, config_bool}`, `resolve_range`, `ResolvedRange::{commits, target_commit}` and its `pub` fields, `ResolvedTarget`, `TargetSpec`, `Base`, `Ancestry`, `ObjectId`, `RepoPath::{from_utf8, as_bytes, to_str, display, escaped, to_path}`, `worktree::{list_worktrees, Worktree}`, `snapshot::{snapshot_working_tree, SnapshotOptions, WorkingTreeSnapshot, NotCaptured, NotCapturedCause, OnDisk}`, `changes::{changed_paths, classify_change, classify_bytes, Change, ChangeStatus, FileKind, Mode, MAX_DIFF_BYTES}`, `bytes::{blob_sizes, read_blobs, worktree_form, Conversion}`, `origin::{find_origins, Origins}`, `GitError`. `SnapshotOptions.pathspecs` narrows only the snapshot's listings; `changed_paths` applies the same pathspecs to the diff; `find_origins` has no record data, so this task adds record associations with no registered worktree to the unavailable origins itself.
  - Tasks 8 to 10 (`supersigil-anchor`): `attribute`, `Request`, `State::{bytes}`, `TargetKind`, `CandidateEdit`, `DEFAULT_BUDGET_BYTES`, `PathAttribution` (`accepted`, `conflicts`), `Conflict`, `walk::{AcceptedEdit, dedup}`, `lines::split_lines`. `attribute` runs `dedup` again over the edits it is given; with one candidate per accepted edit that pass changes nothing.
  - Task 11 (`supersigil-review`): `Review`, `REVIEW_SCHEMA`, `WHY_SCHEMA`, `model::{FileInput, FileReview, RepoPathInfo, BytesStatus, FileStatus, FileKindInfo, ScopeInfo, UntrackedInfo, NotCapturedInfo, OriginsInfo, CommitOriginInfo, UnavailableInfo, RecordInfo, EvidenceInfo, UnreconciledInfo, TranscriptInfo, EditInfo, PromptInfo, Mention, MentionResult, BaseInfo, AncestryInfo, TargetInfo, TargetKindInfo, file_review, unattributed_summary}`, `outcome::{AttributionState, Outcome}`, `mapping::lines_correspond`, `diff::diff_lines`, `summary::render_summary(review, escape)`, `why::{Why, WhyTarget, OnDiskCheck, Tristate, why_line, render_why(why, escape)}`. `why_line` takes the target's blob lines before the base's. Three fields this task fills belong to Task 11's model: `AttributionInfo.conflicting_edits` (F7, filled by `file_review` from `PathAttribution.conflicts`), `ScopeInfo.path_note` (F9, rendered as a `scope:` note), and `WhyLine.differs_from_head_reason` (F10, the last argument of `why_line`, rendered after `differs from HEAD`).
  - Task 12: `reconcile::{reconcile, LOCK_WAIT, InvolvedRecord, UnreconciledCheckout}`.
  - Plan 1: `record_dir::{resolve_record_dir, canonical_checkout}`, `format::{OutputFormat, write_json, escape_control}`, and the test helper `supersigil_cmd` in `crates/supersigil-cli/tests/common/mod.rs`.
- Produces (contract): `supersigil_cli::mapping::{MappedEdit, map_edit}`, `CliError::Git`, `format::AutoFormat` with `resolve`, `Command::Review(ReviewArgs)`, `Command::Why(WhyArgs)`.
- Produces (additions to the contract, all in the CLI crate): `evidence::{Evidence, edit_info}`; `pipeline::{BaseChoice, PipelineArgs, MappedCandidate, MappedAccepted, PathConflict, PinnedRecord, Gathered, gather, path_filters, untracked_paths, repo_path, same_path, claude_home, display_from, PathBytes (with target_worktree_form), path_bytes, attribute_path, conflicts_for, all_conflicts, candidate_transcripts, base_info, target_info, origins_info, records_info, evidence_info, file_status, file_kind, mode_text, on_disk_word, cause_word, not_captured_reason}`; `commands::{ReviewArgs, WhyArgs}`; `commands::review::run`, `commands::why::run`.

Design section 1 (pipeline steps 1 to 9 and `why`), section 2 (edit mapping), and section 4 (commands, JSON, terminal summary, `why`). Typed paths (`--include-untracked`, `-- <path>`, `why <file>`) are relative to the current directory, as git's are. `why` never passes its path through `--include-untracked`: a tracked file must keep `add -u`'s semantics (an assume-unchanged file stays at its index state and is reported not captured). Only when the path is absent from the captured tree and present on disk does `why` take a second snapshot that includes it.

Four fixes from the adversarial review shape this task:

- **F7.** Deduplication runs once, before partitioning by path. `gather` applies `walk::dedup` over every candidate sighting of every path. It then gives each accepted edit its mapped path and each conflict the set of paths its sightings touched. `attribute_path` passes a path's accepted edits to `attribute` as candidate edits carrying their first record. It then replaces the result's conflicts with those that touched the path. A conflicting edit id is thus excluded from every path. Its reference appears in the evidence block and in every file it touched, and the `edits` map, built from `PathAttribution.accepted`, never names it.
- **F9.** A root selector selects everything. When any `-- <path>` names the worktree root, the review has no path filter, and `scope.path_note` says why. `--include-untracked` converts its paths separately and rejects the root.
- **F10.** Inside a coarse hunk, `why` reports `differs_from_head` as `unknown` with the reason `coarse diff`, never `yes`. The reason is also set when HEAD is unborn or lacks the file.
- **F11.** The on-disk check compares the disk with the target in worktree form, which is the converted bytes whenever conversion succeeded, whether or not they map onto the blob lines. When they do not map, the line analysis reports attribution unavailable, and the check still says captured.

- [ ] **Step 1: Depend on the new crates**

In `crates/supersigil-cli/Cargo.toml`, replace:

```toml
supersigil-record.workspace = true
supersigil-session.workspace = true
```

with:

```toml
supersigil-anchor.workspace = true
supersigil-git.workspace = true
supersigil-record.workspace = true
supersigil-review.workspace = true
supersigil-session.workspace = true
```

Run: `cargo build -p supersigil`
Expected: builds (the three crates are workspace members from Task 2).

- [ ] **Step 2: Write the failing end-to-end tests**

First make the slice fixture's last hunk realistic. The patch-consistency rule (Tasks 8 to 10) checks a retained patch against the file, and plan 1's hand-written hunk for `toolu_05` does not describe it. `originalFile` starts with `// manual`, so the changed line is line 7, but the hunk starts at line 4 with a blank context line and shows `pub fn greet` with an extra leading space. Claude Code records three lines of context whose old side matches `originalFile` at `oldStart`. In `crates/supersigil-session/tests/fixtures/slice.jsonl`, replace:

```text
{"oldStart":4,"oldLines":4,"newStart":4,"newLines":4,"lines":[" ","  pub fn greet(name: &str) -> String {",
```

with:

```text
{"oldStart":4,"oldLines":5,"newStart":4,"newLines":5,"lines":[" }"," "," pub fn greet(name: &str) -> String {",
```

The session tests are unaffected: they assert only the first edit's patch length.

Then create `crates/supersigil-cli/tests/cmd_review.rs`. Generated Edit results carry the patch Claude Code would record (`structured_patch`: one jsdiff-style hunk, three lines of context, tabs shown as two spaces), never an empty one: every real Edit result has hunks, even without `originalFile`. The fixture builder takes explicit initial contents for every base file, since a file deleted by a command or a large file whose edits carry no `originalFile` has nothing to derive them from:

```rust
//! `supersigil review` and `supersigil why` end to end, against temporary
//! git repositories and fixture transcripts.

mod common;

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use assert_cmd::assert::OutputAssertExt;
use predicates::prelude::*;
use serde_json::{Value, json};
use supersigil_record::store::Store;
use supersigil_record::{EventId, SessionId};
use supersigil_session::checkout::canonical;
use supersigil_session::discover::encode_project_dir;

/// Session id of the slice fixture.
const SLICE_SESSION: &str = "11111111-1111-4111-8111-111111111111";
/// `src/lib.rs` before the slice session.
const LIB_BASE: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
/// `src/lib.rs` after the session and the manual line it never recorded.
const LIB_FINAL: &str = "// manual\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn greet(name: &str) -> String {\n    format!(\"Hello, {name}!\")\n}\n";
/// `src/new.rs` as the slice session wrote it.
const NEW_RS: &str = "pub const VERSION: &str = \"1\";\n";

/// A temporary repository, Claude home, and records directory, with git
/// isolated from the machine's configuration.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    claude: PathBuf,
    records: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path()).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let fixture = Self {
            claude: root.join("claude"),
            records: root.join("records"),
            repo,
            root,
            _dir: dir,
        };
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.git(&["config", "core.autocrlf", "false"]);
        fixture
    }

    /// Git isolation (design section 5) and the records directory.
    fn isolate(&self, command: &mut Command) {
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("xdg"))
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("SUPERSIGIL_RECORD_DIR", &self.records)
            .env_remove("XDG_DATA_HOME");
    }

    /// Runs git in `dir`; panics with its stderr on failure.
    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        self.isolate(&mut command);
        let output = command.current_dir(dir).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.repo, args)
    }

    /// Stages everything in `dir`, commits, and returns the commit id.
    fn commit(&self, dir: &Path, message: &str) -> String {
        self.git_in(dir, &["add", "-A"]);
        self.git_in(dir, &["commit", "-q", "-m", message]);
        self.git_in(dir, &["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// Writes `text` as transcript `name` in the Claude Code project
    /// directory of `cwd`, with the fixtures' `/work/repo` replaced by `cwd`
    /// (escaped for a JSON string: a Windows path has backslashes).
    fn transcript(&self, cwd: &Path, name: &str, text: &str) {
        let dir = self.claude.join("projects").join(encode_project_dir(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        let quoted = serde_json::to_string(&cwd.to_string_lossy()).unwrap();
        let text = text.replace("/work/repo", &quoted[1..quoted.len() - 1]);
        std::fs::write(dir.join(name), text).unwrap();
    }

    /// `supersigil <args>` in `dir`, isolated, as typed.
    fn plain(&self, dir: &Path, args: &[&str]) -> Command {
        let mut command = common::supersigil_cmd();
        self.isolate(&mut command);
        command.current_dir(dir).args(args);
        command
    }

    /// `supersigil <subcommand> --claude-home <claude> <rest>` in `dir`.
    fn supersigil(&self, dir: &Path, args: &[&str]) -> Command {
        let mut command = common::supersigil_cmd();
        self.isolate(&mut command);
        command
            .current_dir(dir)
            .arg(args[0])
            .arg("--claude-home")
            .arg(&self.claude)
            .args(&args[1..]);
        command
    }

    /// Runs a command expected to succeed and parses its JSON output.
    fn json(&self, dir: &Path, args: &[&str]) -> Value {
        let output = self.supersigil(dir, args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

/// Writes `content` to `path` under `dir`, creating directories.
fn write(dir: &Path, path: &str, content: &str) {
    let path = dir.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Builds the records of one generated session, each chained to the last.
struct Session<'a> {
    id: &'a str,
    cwd: &'a Path,
    text: String,
    count: u32,
    parent: Option<String>,
}

impl<'a> Session<'a> {
    fn new(id: &'a str, cwd: &'a Path) -> Self {
        Self {
            id,
            cwd,
            text: String::new(),
            count: 0,
            parent: None,
        }
    }

    fn push(&mut self, kind: &str, content: &Value, result: Option<Value>) -> &mut Self {
        let uuid = format!("{}-{}", self.id, self.count);
        let mut value = json!({
            "type": kind, "uuid": uuid, "parentUuid": self.parent, "sessionId": self.id,
            "cwd": self.cwd.to_string_lossy(),
            "timestamp": format!("2026-09-29T10:00:{:02}.000Z", self.count),
            "isSidechain": false, "message": {"role": kind, "content": content},
        });
        if let Some(result) = result {
            value["toolUseResult"] = result;
        }
        self.text.push_str(&value.to_string());
        self.text.push('\n');
        self.parent = Some(uuid);
        self.count += 1;
        self
    }

    fn prompt(&mut self, text: &str) -> &mut Self {
        self.push("user", &json!(text), None)
    }

    /// An Edit call on a file over 10 KB and its result, as Claude Code
    /// records it: no `originalFile`, and the patch of replacing `old` with
    /// `new` in `before`, the file as the edit found it.
    fn edit(&mut self, tool: &str, file: &Path, before: &str, old: &str, new: &str) -> &mut Self {
        let file = file.to_string_lossy();
        let call = json!([{"type": "tool_use", "id": tool, "name": "Edit",
            "input": {"file_path": file, "old_string": old, "new_string": new}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": "updated"}]);
        let result = json!({"filePath": file, "oldString": old, "newString": new,
            "originalFile": null, "structuredPatch": structured_patch(before, old, new),
            "userModified": false, "replaceAll": false});
        self.push("assistant", &call, None)
            .push("user", &done, Some(result))
    }

    /// A Write call that creates `file`, and its result.
    fn create(&mut self, tool: &str, file: &Path, content: &str) -> &mut Self {
        let file = file.to_string_lossy();
        let call = json!([{"type": "tool_use", "id": tool, "name": "Write",
            "input": {"file_path": file, "content": content}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": "File created"}]);
        let result =
            json!({"type": "create", "filePath": file, "content": content, "structuredPatch": []});
        self.push("assistant", &call, None)
            .push("user", &done, Some(result))
    }

    /// A `NotebookEdit` call, which the capture cannot read, and its result.
    fn notebook_edit(&mut self, tool: &str) -> &mut Self {
        let notebook = self.cwd.join("nb.ipynb");
        let call = json!([{"type": "tool_use", "id": tool, "name": "NotebookEdit",
            "input": {"notebook_path": notebook.to_string_lossy(), "new_source": "x"}}]);
        let done = json!([{"type": "tool_result", "tool_use_id": tool, "content": "ok"}]);
        self.push("assistant", &call, None)
            .push("user", &done, None)
    }

    fn text(&self) -> String {
        self.text.clone()
    }
}

/// Claude Code's `structuredPatch` for replacing `old` with `new` once in
/// `before`: one hunk with three lines of context, as jsdiff builds it, with
/// tabs shown as two spaces.
fn structured_patch(before: &str, old: &str, new: &str) -> Value {
    let after = before.replacen(old, new, 1);
    let old_lines: Vec<&str> = before.lines().collect();
    let new_lines: Vec<&str> = after.lines().collect();
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old_changed, new_changed) = (old_lines.len() - suffix, new_lines.len() - suffix);
    let start = prefix.saturating_sub(3);
    let old_end = (old_changed + 3).min(old_lines.len());
    let shown = |marker: &str, lines: &[&str]| -> Vec<String> {
        lines
            .iter()
            .map(|line| format!("{marker}{}", line.replace('\t', "  ")))
            .collect()
    };
    let mut lines = shown(" ", &old_lines[start..prefix]);
    lines.extend(shown("-", &old_lines[prefix..old_changed]));
    lines.extend(shown("+", &new_lines[prefix..new_changed]));
    lines.extend(shown(" ", &old_lines[old_changed..old_end]));
    let after_context = old_end - old_changed;
    json!([{"oldStart": start + 1, "oldLines": old_end - start,
        "newStart": start + 1, "newLines": new_changed + after_context - start,
        "lines": lines}])
}

fn slice_text() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../supersigil-session/tests/fixtures/slice.jsonl"),
    )
    .unwrap()
}

/// The design's section 8 slice: the files the session found, committed,
/// then the working tree after the session plus the manual line it never
/// recorded. `old.txt` appears only in a command, so it is seeded here.
fn slice(f: &Fixture) {
    write(&f.repo, "src/lib.rs", LIB_BASE);
    write(&f.repo, "notes.txt", "draft\n");
    write(&f.repo, "old.txt", "obsolete\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "src/lib.rs", LIB_FINAL);
    write(&f.repo, "src/new.rs", NEW_RS);
    std::fs::remove_file(f.repo.join("old.txt")).unwrap();
    f.transcript(&f.repo, "slice.jsonl", &slice_text());
}

/// A limitations-only transcript: one `NotebookEdit` call, no edit recorded.
fn limits(f: &Fixture) {
    let text = Session::new("s-limits", &f.repo)
        .prompt("Edit the notebook.")
        .notebook_edit("toolu_nb")
        .text();
    f.transcript(&f.repo, "limits.jsonl", &text);
}

fn edit_id(session: &str, tool: &str) -> String {
    EventId::derive("edit", &SessionId::new(session), tool)
        .as_str()
        .to_owned()
}

fn file<'v>(review: &'v Value, path: &str) -> &'v Value {
    review["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no file {path} in {review:#}"))
}

/// `(line text, outcome)` for every diff line of `file` on `side`
/// (`"target"` for added lines, `"base"` for removed ones), in order.
fn side_lines(file: &Value, side: &str) -> Vec<(String, Value)> {
    let (start_key, texts_key) = if side == "target" {
        ("target_start", "added")
    } else {
        ("base_start", "removed")
    };
    let mut lines = Vec::new();
    for hunk in file["hunks"].as_array().unwrap() {
        let first = hunk[start_key].as_u64().unwrap();
        for span in hunk["spans"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["side"] == side)
        {
            for offset in 0..span["count"].as_u64().unwrap() {
                let index =
                    usize::try_from(span["start"].as_u64().unwrap() + offset - first).unwrap();
                let text = hunk[texts_key][index].as_str().unwrap().to_owned();
                lines.push((text, span["outcome"].clone()));
            }
        }
    }
    lines
}

/// 400 lines, about 23 KB: over the size above which Claude Code records no
/// `originalFile`, so the edits to it carry no hashes.
fn big_file() -> String {
    (1..=400).map(big_line).collect()
}

fn big_line(i: u32) -> String {
    format!("line {i:03}: the quick brown fox jumps over the lazy dog\n")
}

#[test]
fn review_slice_fixture() {
    let f = Fixture::new();
    slice(&f);
    let review = f.json(&f.repo, &["review", "--format", "json"]);

    let lib = file(&review, "src/lib.rs");
    assert_eq!(lib["status"], "modified");
    let chains = lib["attribution"]["chains"].as_array().unwrap();
    assert_eq!(chains.len(), 1);
    assert_eq!(chains[0]["class"], "exact_from_start");
    assert_eq!(chains[0]["head"], edit_id(SLICE_SESSION, "toolu_05"));
    for (text, outcome) in side_lines(lib, "target") {
        if text.contains("Hello, {name}!") {
            assert_eq!(outcome["kind"], "attributed", "{text}");
            assert_eq!(
                outcome["edits"][0]["edit"],
                edit_id(SLICE_SESSION, "toolu_05")
            );
            assert_eq!(outcome["edits"][0]["relation"], "introduced");
        } else {
            // The manual line, and the earlier edit's surviving lines: the
            // later edit rewrote part of that block, so no whole block of it
            // occurs in the target and the fallback has nothing to match.
            assert_eq!(outcome["kind"], "unattributed", "{text}");
        }
    }
    let prompt = &review["edits"][edit_id(SLICE_SESSION, "toolu_05")]["prompt"];
    assert_eq!(prompt["turn"], "u1");
    assert_eq!(prompt["role"], "human");

    let old = file(&review, "old.txt");
    assert_eq!(old["status"], "deleted");
    let removed = side_lines(old, "base");
    assert!(!removed.is_empty());
    assert!(removed.iter().all(|(_, o)| o["kind"] == "unattributed"));
    let mentions = old["mentions"].as_array().unwrap();
    assert!(
        mentions.iter().any(|m| m["text"] == "rm old.txt"),
        "{mentions:?}"
    );

    let paths: Vec<&Value> = review["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| &f["path"])
        .collect();
    // The restored notes.txt has no net change; the created file is untracked.
    assert!(!paths.contains(&&json!("notes.txt")));
    assert!(!paths.contains(&&json!("src/new.rs")));
    let untracked = &review["scope"]["untracked_excluded"][0];
    assert_eq!(untracked["path"], "src/new.rs");
    assert_eq!(untracked["recorded_edit"], true);
    assert_eq!(untracked["include_flag"], "--include-untracked src/new.rs");
    assert_eq!(
        review["unattributed"]["untracked_with_recorded_edits"],
        json!(["src/new.rs"])
    );
    assert_eq!(review["records"][0]["reconciled"], true);
}

#[test]
fn review_slice_with_include_untracked() {
    let f = Fixture::new();
    slice(&f);
    let review = f.json(
        &f.repo,
        &[
            "review",
            "--format",
            "json",
            "--include-untracked",
            "src/new.rs",
        ],
    );

    let new = file(&review, "src/new.rs");
    assert_eq!(new["status"], "added");
    assert_eq!(new["attribution"]["chains"][0]["class"], "exact_from_base");
    let added = side_lines(new, "target");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id(SLICE_SESSION, "toolu_02")
    );
    assert!(
        review["scope"]["untracked_excluded"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn review_large_file_single_session() {
    let f = Fixture::new();
    write(&f.repo, "big.txt", &big_file());
    f.commit(&f.repo, "base");
    let (old, new) = (big_line(200), "line 200: a changed line\n".to_owned());
    write(&f.repo, "big.txt", &big_file().replacen(&old, &new, 1));
    let text = Session::new("s-big", &f.repo)
        .prompt("Change line 200.")
        .edit(
            "toolu_big",
            &f.repo.join("big.txt"),
            &big_file(),
            &old,
            &new,
        )
        .text();
    f.transcript(&f.repo, "big.jsonl", &text);

    let review = f.json(&f.repo, &["review", "--format", "json"]);

    let big = file(&review, "big.txt");
    assert_eq!(big["attribution"]["chains"][0]["class"], "exact_from_base");
    let added = side_lines(big, "target");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].0, new);
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id("s-big", "toolu_big")
    );
    let removed = side_lines(big, "base");
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].1["kind"], "attributed");
    assert_eq!(removed[0].1["edits"][0]["relation"], "replaced");
}

#[test]
fn review_large_file_two_sessions() {
    let f = Fixture::new();
    write(&f.repo, "big.txt", &big_file());
    f.commit(&f.repo, "base");
    let changed = big_file()
        .replacen(&big_line(100), "line 100: changed by one\n", 1)
        .replacen(&big_line(300), "line 300: changed by two\n", 1);
    write(&f.repo, "big.txt", &changed);
    let big = f.repo.join("big.txt");
    // Lines 100 and 300 are far apart, so each patch is the same whichever
    // session edited first.
    let one = Session::new("s-one", &f.repo)
        .prompt("Change line 100.")
        .edit(
            "toolu_one",
            &big,
            &big_file(),
            &big_line(100),
            "line 100: changed by one\n",
        )
        .text();
    let two = Session::new("s-two", &f.repo)
        .prompt("Change line 300.")
        .edit(
            "toolu_two",
            &big,
            &big_file(),
            &big_line(300),
            "line 300: changed by two\n",
        )
        .text();
    f.transcript(&f.repo, "one.jsonl", &one);
    f.transcript(&f.repo, "two.jsonl", &two);

    let review = f.json(&f.repo, &["review", "--format", "json"]);

    // No recorded hash joins the two transcripts, so each yields one
    // consistent chain and they do not compose (design section 3).
    let big = file(&review, "big.txt");
    let chains = big["attribution"]["chains"].as_array().unwrap();
    assert_eq!(chains.len(), 2);
    assert!(chains.iter().all(|c| c["class"] == "consistent"));
    let added = side_lines(big, "target");
    assert_eq!(added.len(), 2);
    assert!(
        added.iter().all(|(_, o)| o["kind"] == "ambiguous"),
        "{added:?}"
    );
    // Neither chain reaches the base, so no removed line has a fate; each
    // edit's old text is a whole block of the base.
    let removed = side_lines(big, "base");
    assert!(
        removed.iter().all(|(_, o)| o["kind"] == "content_match"),
        "{removed:?}"
    );
}

#[test]
fn review_commit_target_uses_the_reflog_origin() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let text = Session::new("s-create", &f.repo)
        .prompt("Add a version.")
        .create("toolu_create", &f.repo.join("src/new.rs"), NEW_RS)
        .text();
    f.transcript(&f.repo, "create.jsonl", &text);
    write(&f.repo, "src/new.rs", NEW_RS);
    let head = f.commit(&f.repo, "add new.rs");

    let review = f.json(&f.repo, &["review", "--target", "HEAD", "--format", "json"]);

    assert_eq!(review["origins"]["commits"][0]["commit"], head);
    assert_eq!(
        review["origins"]["commits"][0]["worktrees"],
        json!([f.repo.display().to_string()])
    );
    let new = file(&review, "src/new.rs");
    let added = side_lines(new, "target");
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id("s-create", "toolu_create")
    );
}

#[test]
fn review_commit_target_without_reflog_is_unattributed() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let text = Session::new("s-create", &f.repo)
        .prompt("Add a version.")
        .create("toolu_create", &f.repo.join("src/new.rs"), NEW_RS)
        .text();
    f.transcript(&f.repo, "create.jsonl", &text);
    write(&f.repo, "src/new.rs", NEW_RS);
    let head = f.commit(&f.repo, "add new.rs");
    f.git(&["reflog", "expire", "--expire=now", "--all"]);

    let review = f.json(&f.repo, &["review", "--target", "HEAD", "--format", "json"]);

    assert_eq!(review["origins"]["without_origin"], json!([head]));
    assert!(review["records"].as_array().unwrap().is_empty());
    let added = side_lines(file(&review, "src/new.rs"), "target");
    assert_eq!(added[0].1["kind"], "unattributed");
}

#[test]
fn review_commit_from_sibling_worktree() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let sib = f.root.join("sib");
    let sib_arg = sib.to_string_lossy().into_owned();
    f.git(&["worktree", "add", "-q", "-b", "feature", sib_arg.as_str()]);
    let content = "pub fn feature() {}\n";
    let text = Session::new("s-sib", &sib)
        .prompt("Add the feature.")
        .create("toolu_sib", &sib.join("src/feature.rs"), content)
        .text();
    f.transcript(&sib, "sib.jsonl", &text);
    write(&sib, "src/feature.rs", content);
    f.commit(&sib, "add feature");

    let review = f.json(
        &f.repo,
        &[
            "review", "--base", "main", "--target", "feature", "--format", "json",
        ],
    );

    assert_eq!(
        review["origins"]["commits"][0]["worktrees"],
        json!([sib.display().to_string()])
    );
    // Reconcile created the sibling's record from its transcripts.
    assert_eq!(review["records"].as_array().unwrap().len(), 1);
    let added = side_lines(file(&review, "src/feature.rs"), "target");
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id("s-sib", "toolu_sib")
    );
}

#[test]
fn review_session_editing_a_nested_worktree() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    f.git(&["worktree", "add", "-q", "-b", "x", ".claude/worktrees/x"]);
    let nested = f.repo.join(".claude/worktrees/x");
    // A session in the main checkout writes a file inside the nested worktree.
    let content = "pub fn n() {}\n";
    let text = Session::new("s-parent", &f.repo)
        .prompt("Add n to the nested worktree.")
        .create("toolu_parent", &nested.join("src/n.rs"), content)
        .text();
    f.transcript(&f.repo, "parent.jsonl", &text);
    write(&nested, "src/n.rs", content);
    // No record exists yet: reconcile finds the main checkout's project
    // directory as an ancestor of the nested worktree (design section 1).
    assert!(!f.records.exists());

    let review = f.json(
        &nested,
        &[
            "review",
            "--format",
            "json",
            "--include-untracked",
            "src/n.rs",
        ],
    );

    let added = side_lines(file(&review, "src/n.rs"), "target");
    assert_eq!(added[0].1["kind"], "attributed");
    assert_eq!(
        added[0].1["edits"][0]["edit"],
        edit_id("s-parent", "toolu_parent")
    );
}

#[test]
fn review_reports_locked_record() {
    let f = Fixture::new();
    slice(&f);
    f.json(&f.repo, &["review", "--format", "json"]);
    let record = std::fs::read_dir(&f.records)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("manifest.json").exists())
        .unwrap();
    // The stop hook, say, holding the record's writer lock.
    let held = Store::open(&record).unwrap();
    let tx = held.begin().unwrap();

    let started = Instant::now();
    let review = f.json(&f.repo, &["review", "--format", "json"]);

    assert!(started.elapsed() < Duration::from_secs(30));
    assert_eq!(review["records"][0]["reconciled"], false);
    assert_eq!(review["records"][0]["reason"], "locked by another writer");
    // The pinned revision is still read.
    let chains = &file(&review, "src/lib.rs")["attribution"]["chains"];
    assert_eq!(chains[0]["head"], edit_id(SLICE_SESSION, "toolu_05"));
    drop(tx);
}

#[test]
fn review_reports_unreconciled_checkout() {
    let f = Fixture::new();
    slice(&f);
    std::fs::create_dir_all(&f.records).unwrap();
    let guard = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(f.records.join(".lock"))
        .unwrap();
    guard.lock().unwrap();

    let started = Instant::now();
    let review = f.json(&f.repo, &["review", "--format", "json"]);

    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(review["records"].as_array().unwrap().is_empty());
    let unreconciled = &review["evidence"]["unreconciled_checkouts"][0];
    assert_eq!(unreconciled["checkout"], f.repo.display().to_string());
    assert_eq!(unreconciled["transcripts"], 1);
    assert_eq!(
        unreconciled["reason"],
        "records directory locked by another writer"
    );
    drop(guard);
}

#[test]
fn review_keeps_capture_limitations_when_everything_is_attributed() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    let text = Session::new("s-create", &f.repo)
        .prompt("Add a version.")
        .create("toolu_create", &f.repo.join("src/new.rs"), NEW_RS)
        .text();
    f.transcript(&f.repo, "create.jsonl", &text);
    write(&f.repo, "src/new.rs", NEW_RS);
    limits(&f);
    let args = ["review", "--include-untracked", "src/new.rs", "--format"];
    let mut json_args = args.to_vec();
    json_args.push("json");
    let mut terminal_args = args.to_vec();
    terminal_args.push("terminal");

    let review = f.json(&f.repo, &json_args);

    for (text, outcome) in side_lines(file(&review, "src/new.rs"), "target") {
        assert_eq!(outcome["kind"], "attributed", "{text}");
    }
    let transcripts = review["evidence"]["candidate_transcripts"]
        .as_array()
        .unwrap();
    let limited = transcripts
        .iter()
        .find(|t| t["transcript"].as_str().unwrap().ends_with("limits.jsonl"))
        .unwrap();
    assert!(
        !limited["capture_limitations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(limited["localized"], false);
    f.supersigil(&f.repo, &terminal_args)
        .assert()
        .success()
        .stdout(predicate::str::contains("limits.jsonl"))
        .stdout(predicate::str::contains("not localized to a path"));
}

#[test]
fn why_on_a_changed_line() {
    let f = Fixture::new();
    slice(&f);
    let why = f.json(&f.repo, &["why", "src/lib.rs:7", "--format", "json"]);

    assert_eq!(why["on_disk"]["state"], "captured");
    let line = &why["line"];
    assert!(line["text"].as_str().unwrap().contains("Hello, {name}!"));
    assert_eq!(line["differs_from_head"], "yes");
    assert_eq!(line["outcome"]["kind"], "attributed");
    assert_eq!(
        line["outcome"]["edits"][0]["edit"],
        edit_id(SLICE_SESSION, "toolu_05")
    );
}

#[test]
fn why_on_an_unchanged_line_explains_back_to_creation() {
    let f = Fixture::new();
    let text = Session::new("s-create", &f.repo)
        .prompt("Add a version.")
        .create("toolu_create", &f.repo.join("src/new.rs"), NEW_RS)
        .text();
    f.transcript(&f.repo, "create.jsonl", &text);
    write(&f.repo, "src/new.rs", NEW_RS);
    f.commit(&f.repo, "add new.rs");

    let why = f.json(&f.repo, &["why", "src/new.rs:1", "--format", "json"]);

    let line = &why["line"];
    assert_eq!(line["differs_from_head"], "no");
    assert_eq!(line["chains"][0]["class"], "exact_from_base");
    assert_eq!(line["outcome"]["kind"], "attributed");
    assert_eq!(
        line["outcome"]["edits"][0]["edit"],
        edit_id("s-create", "toolu_create")
    );
}

#[test]
fn why_on_assume_unchanged_file_is_not_captured() {
    let f = Fixture::new();
    slice(&f);
    limits(&f);
    write(&f.repo, "notes.txt", "edited on disk\n");
    f.git(&["update-index", "--assume-unchanged", "notes.txt"]);

    let why = f.json(&f.repo, &["why", "notes.txt:1", "--format", "json"]);

    assert_eq!(why["on_disk"]["state"], "not_captured");
    assert!(why["line"].is_null());
    // The evidence context survives the early return.
    assert_eq!(why["records"].as_array().unwrap().len(), 1);
    let transcripts = why["evidence"]["candidate_transcripts"].as_array().unwrap();
    assert!(transcripts.iter().any(|t| {
        t["transcript"].as_str().unwrap().ends_with("limits.jsonl")
            && !t["capture_limitations"].as_array().unwrap().is_empty()
    }));
    f.supersigil(&f.repo, &["why", "notes.txt:1", "--format", "terminal"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not captured"))
        .stdout(predicate::str::contains("limits.jsonl"));
}

#[test]
fn why_beyond_the_end_of_the_file_fails() {
    let f = Fixture::new();
    slice(&f);
    f.supersigil(&f.repo, &["why", "src/lib.rs:99", "--format", "json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("beyond the end"));
}

#[test]
fn review_format_auto_prints_json_when_piped() {
    let f = Fixture::new();
    slice(&f);
    let review = f.json(&f.repo, &["review"]);
    assert_eq!(review["schema"], "supersigil.review/1");
}

#[test]
fn why_inside_a_coarse_hunk_does_not_claim_a_difference() {
    let f = Fixture::new();
    let middle: String = (2..=6001).map(|i| i.to_string() + "\n").collect();
    write(&f.repo, "big.txt", &format!("first\n{middle}last\n"));
    f.commit(&f.repo, "base");
    // Only the first and last lines change: nothing is common at either end,
    // so the 6,002 remaining lines exceed the cutoff and the diff is coarse.
    write(&f.repo, "big.txt", &format!("FIRST\n{middle}LAST\n"));

    let why = f.json(&f.repo, &["why", "big.txt:3000", "--format", "json"]);

    assert_eq!(why["line"]["differs_from_head"], "unknown");
    assert_eq!(why["line"]["differs_from_head_reason"], "coarse diff");
}

#[cfg(unix)]
#[test]
fn why_compares_the_disk_with_converted_bytes_that_do_not_map_line_for_line() {
    let f = Fixture::new();
    // A reversible filter: checkout doubles every line, staging keeps the
    // odd ones. The worktree form therefore has twice the blob's lines.
    f.git(&["config", "filter.dup.smudge", "sed p"]);
    f.git(&["config", "filter.dup.clean", "awk 'NR%2==1'"]);
    write(&f.repo, ".gitattributes", "*.dup filter=dup\n");
    write(&f.repo, "f.dup", "a\na\nb\nb\n");
    f.commit(&f.repo, "base");

    let why = f.json(&f.repo, &["why", "f.dup:1", "--format", "json"]);

    assert_eq!(why["on_disk"]["state"], "captured");
    assert_eq!(why["target"]["attribution_bytes"], "converted");
    let outcome = &why["line"]["outcome"];
    assert_eq!(outcome["kind"], "unattributed");
    assert_eq!(outcome["reason"]["kind"], "attribution_unavailable");
    assert_eq!(
        outcome["reason"]["reason"],
        "conversion changed line structure"
    );
}

#[test]
fn a_root_path_selector_reviews_everything() {
    let f = Fixture::new();
    write(&f.repo, "src/lib.rs", "a\n");
    write(&f.repo, "README.md", "r\n");
    f.commit(&f.repo, "base");
    write(&f.repo, "src/lib.rs", "b\n");
    write(&f.repo, "README.md", "s\n");

    let review = f.json(&f.repo, &["review", "--format", "json", "--", "src", "."]);

    let paths: Vec<&str> = review["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["README.md", "src/lib.rs"]);
    assert!(review["scope"]["paths"].as_array().unwrap().is_empty());
    assert!(
        review["scope"]["path_note"]
            .as_str()
            .unwrap()
            .contains("whole worktree")
    );
    // `--include-untracked` never treats the root as a selector.
    f.supersigil(
        &f.repo,
        &["review", "--format", "json", "--include-untracked", "."],
    )
    .assert()
    .failure()
    .code(1)
    .stderr(predicate::str::contains("names the worktree"));
}

#[test]
fn a_conflicting_edit_id_attributes_neither_path() {
    let f = Fixture::new();
    write(&f.repo, "README.md", "readme\n");
    f.commit(&f.repo, "base");
    // One session and tool-use id recorded with different files in two
    // records: the same edit id with conflicting payloads.
    let transcripts = f.root.join("transcripts");
    std::fs::create_dir_all(&transcripts).unwrap();
    for (dir, name) in [("x", "a.rs"), ("y", "b.rs")] {
        let checkout = f.repo.join(dir);
        std::fs::create_dir_all(&checkout).unwrap();
        let text = Session::new("s-dup", &checkout)
            .prompt("Write it.")
            .create("toolu_dup", &checkout.join(name), "pub fn f() {}\n")
            .text();
        let transcript = transcripts.join(format!("{dir}.jsonl"));
        std::fs::write(&transcript, text).unwrap();
        let checkout_arg = checkout.to_string_lossy().into_owned();
        let transcript_arg = transcript.to_string_lossy().into_owned();
        let args = [
            "session",
            "--checkout",
            checkout_arg.as_str(),
            "sync",
            "--transcript",
            transcript_arg.as_str(),
        ];
        f.plain(&f.repo, &args).assert().success();
        write(&checkout, name, "pub fn f() {}\n");
    }

    let args = [
        "review",
        "--format",
        "json",
        "--include-untracked",
        "x/a.rs",
        "--include-untracked",
        "y/b.rs",
    ];
    let review = f.json(&f.repo, &args);

    let id = edit_id("s-dup", "toolu_dup");
    for path in ["x/a.rs", "y/b.rs"] {
        let file = file(&review, path);
        let added = side_lines(file, "target");
        assert!(
            added.iter().all(|(_, o)| o["kind"] == "unattributed"),
            "{path}: {added:?}"
        );
        assert_eq!(file["attribution"]["conflicting_edits"][0]["edit"], id);
    }
    assert_eq!(review["evidence"]["conflicting_edits"][0]["edit"], id);
    assert!(review["edits"].get(&id).is_none());
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil --test cmd_review`
Expected: every test fails: `error: unrecognized subcommand 'review'` (exit 2) or `'why'`.

- [ ] **Step 4: Add `CliError::Git` and `AutoFormat`**

In `crates/supersigil-cli/src/error.rs`, add after the `Session` variant:

```rust
    /// Git error: git missing or too old, not a worktree, a revision that
    /// does not resolve, or a git command that failed.
    #[error("{0}")]
    Git(#[from] supersigil_git::GitError),
```

In `crates/supersigil-cli/src/format.rs`, add after the `OutputFormat` enum:

```rust
/// Output format for `review` and `why`, which default to `auto`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum AutoFormat {
    /// Terminal text when stdout is a terminal, JSON otherwise.
    Auto,
    /// JSON output.
    Json,
    /// Plain terminal text.
    Terminal,
}

impl AutoFormat {
    /// The concrete format: `auto` is terminal text when stdout is a
    /// terminal and JSON otherwise.
    #[must_use]
    pub fn resolve(self) -> OutputFormat {
        match self {
            Self::Auto if io::stdout().is_terminal() => OutputFormat::Terminal,
            Self::Auto | Self::Json => OutputFormat::Json,
            Self::Terminal => OutputFormat::Terminal,
        }
    }
}
```

and to its `tests` module:

```rust
    #[test]
    fn explicit_formats_resolve_to_themselves() {
        assert!(matches!(AutoFormat::Json.resolve(), OutputFormat::Json));
        assert!(matches!(
            AutoFormat::Terminal.resolve(),
            OutputFormat::Terminal
        ));
    }
```

- [ ] **Step 5: Write the failing mapping tests**

Create `crates/supersigil-cli/src/mapping.rs` with the tests first (the function body comes in Step 6):

```rust
//! Maps a recorded edit onto the git worktree whose file it changed.
//!
//! An edit stores the transcript's working directory (`checkout`: not
//! canonical, possibly a subdirectory) and a path relative to it. The file it
//! changed belongs to the innermost worktree containing `checkout/path`,
//! which is how a session in `/repo` editing
//! `.claude/worktrees/feature/src/lib.rs` lands in the `feature` worktree as
//! `src/lib.rs`.

use std::path::{Path, PathBuf};

use supersigil_record::observations::Edit;
use supersigil_session::checkout::{Placement, canonical, placement};

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use supersigil_record::observations::{Edit, EditOperation, FileState, Material};
    use supersigil_record::{EventId, SessionId, Timestamp, TurnId};
    use supersigil_session::checkout::canonical;

    use super::{MappedEdit, map_edit};

    fn edit(checkout: &Path, path: &str) -> Edit {
        Edit {
            id: EventId::new("e1"),
            turn: TurnId::new("a1"),
            session: SessionId::new("s1"),
            path: PathBuf::from(path),
            before: FileState::unknown(),
            after: FileState::unknown(),
            patch: Material::unavailable("test"),
            old_text: Material::unavailable("test"),
            new_text: Material::unavailable("test"),
            replace_all: false,
            operation: EditOperation::Replace,
            checkout: checkout.to_path_buf(),
            time: Timestamp::new("2026-09-29T10:00:00.000Z"),
            source_ordinal: 0,
            agent_id: None,
            transcript: None,
        }
    }

    /// A repository with a nested worktree, under a canonical temporary directory.
    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path()).unwrap();
        let repo = root.join("repo");
        let nested = repo.join(".claude").join("worktrees").join("feature");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(nested.join("src")).unwrap();
        (dir, repo, nested)
    }

    #[test]
    fn a_parent_session_edit_lands_in_the_nested_worktree() {
        let (_dir, repo, nested) = dirs();
        let roots = [repo.clone(), nested.clone()];
        let mapped = map_edit(
            &edit(&repo, ".claude/worktrees/feature/src/lib.rs"),
            &roots,
            false,
        );
        assert_eq!(
            mapped,
            Some(MappedEdit {
                worktree: nested,
                path: "src/lib.rs".to_owned()
            })
        );
    }

    #[test]
    fn a_subdirectory_checkout_maps_to_a_repository_path() {
        let (_dir, repo, _nested) = dirs();
        let mapped = map_edit(
            &edit(&repo.join("src"), "lib.rs"),
            std::slice::from_ref(&repo),
            false,
        );
        assert_eq!(mapped.map(|m| m.path), Some("src/lib.rs".to_owned()));
    }

    #[test]
    fn an_edit_outside_every_worktree_is_unplaced() {
        let (dir, repo, _nested) = dirs();
        let elsewhere = canonical(dir.path()).unwrap().join("elsewhere");
        assert_eq!(map_edit(&edit(&elsewhere, "a.rs"), &[repo], false), None);
    }

    #[test]
    fn a_missing_checkout_is_compared_as_written() {
        let (_dir, repo, _nested) = dirs();
        let mapped = map_edit(
            &edit(&repo.join("gone"), "a.rs"),
            std::slice::from_ref(&repo),
            false,
        );
        assert_eq!(mapped.map(|m| m.path), Some("gone/a.rs".to_owned()));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_file_keeps_the_written_path() {
        let (_dir, repo, _nested) = dirs();
        std::fs::write(repo.join("src/real.rs"), "").unwrap();
        std::os::unix::fs::symlink(repo.join("src/real.rs"), repo.join("link.rs")).unwrap();
        let mapped = map_edit(&edit(&repo, "link.rs"), std::slice::from_ref(&repo), false);
        assert_eq!(mapped.map(|m| m.path), Some("link.rs".to_owned()));
    }

    #[test]
    fn case_is_ignored_only_when_asked() {
        let roots = [PathBuf::from("/work/repo")];
        let checkout = Path::new("/work/Repo/src");
        assert_eq!(map_edit(&edit(checkout, "a.rs"), &roots, false), None);
        let mapped = map_edit(&edit(checkout, "a.rs"), &roots, true);
        assert_eq!(mapped.map(|m| m.path), Some("src/a.rs".to_owned()));
    }
}
```

Add to `crates/supersigil-cli/src/lib.rs`, before `pub mod reconcile;`:

```rust
/// Mapping recorded edits onto git worktrees.
pub mod mapping;
```

Run: `cargo nextest run -p supersigil mapping`
Expected: compile error, `unresolved imports super::MappedEdit, super::map_edit`.

- [ ] **Step 6: Implement the mapping**

Insert above the `#[cfg(test)]` line in `crates/supersigil-cli/src/mapping.rs`:

```rust
/// The worktree an edit changed a file in, and the file's path inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedEdit {
    /// Root of the innermost worktree containing the edited file, as given in
    /// `worktree_roots`.
    pub worktree: PathBuf,
    /// The file's path relative to that root, components joined with `/`.
    pub path: String,
}

/// Maps `edit` to the innermost of `worktree_roots` that contains the file
/// it edited.
///
/// The edit's checkout directory is canonicalized when it exists and taken
/// as written otherwise, and its already-validated relative path is
/// appended lexically. Only directories are resolved, never the edited file:
/// a symlinked file keeps the path the agent wrote, and a deleted file still
/// maps. Containment uses [`placement`]'s normalization (separators,
/// drive-letter case); with `ignore_case` (the repository sets
/// `core.ignorecase`), a root that does not contain the file as spelled is
/// tried again with both sides ASCII-lowercased. Returns `None` when no root
/// contains the file or its path is not UTF-8.
#[must_use]
pub fn map_edit(edit: &Edit, worktree_roots: &[PathBuf], ignore_case: bool) -> Option<MappedEdit> {
    let checkout = canonical(&edit.checkout).unwrap_or_else(|_| edit.checkout.clone());
    let file = checkout.join(&edit.path);
    let dir = file.parent()?;
    let mut best: Option<(usize, &PathBuf)> = None;
    for root in worktree_roots {
        let Some(depth) = depth_below(dir, root, ignore_case) else {
            continue;
        };
        if best.is_none_or(|(shallowest, _)| depth < shallowest) {
            best = Some((depth, root));
        }
    }
    let (depth, root) = best?;
    // The directory lies `depth` components below the root; the file name is one more.
    let mut parts = file
        .components()
        .rev()
        .take(depth + 1)
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    parts.reverse();
    Some(MappedEdit {
        worktree: root.clone(),
        path: parts.join("/"),
    })
}

/// How many components `dir` lies below `root`, or `None` when it is outside.
fn depth_below(dir: &Path, root: &Path, ignore_case: bool) -> Option<usize> {
    let depth = |place| match place {
        Placement::Same => Some(0),
        Placement::Nested(depth) => Some(depth),
        Placement::Outside => None,
    };
    depth(placement(dir, root)).or_else(|| {
        ignore_case
            .then(|| depth(placement(&lowercase(dir), &lowercase(root))))
            .flatten()
    })
}

/// `path` with ASCII letters lowercased, for `core.ignorecase` comparisons.
fn lowercase(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_ascii_lowercase())
}
```

Run: `cargo nextest run -p supersigil mapping`
Expected: the six mapping tests pass (five on Windows, where the symlink test is compiled out).

- [ ] **Step 7: Write the failing evidence tests**

Create `crates/supersigil-cli/src/evidence.rs` with its module documentation, imports, and tests (the implementation comes in Step 8):

```rust
//! The recorded evidence around attribution: which transcripts a review
//! draws on and what their capture missed, commands whose text mentions a
//! file, and the message that preceded each edit.
//!
//! Everything here is an observation, never attribution: a mention is text
//! that contains a path, and a prompt is the nearest earlier Human or
//! Delegation message in the conversation, not a cause.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use supersigil_anchor::walk::AcceptedEdit;
use supersigil_record::observations::{
    CaptureLimitation, Command, Edit, Observation, Outcome, Role, Turn,
};
use supersigil_record::{EventId, SessionId, TurnId};
use supersigil_review::model::{EditInfo, Mention, MentionResult, PromptInfo, TranscriptInfo};
use supersigil_session::checkout::{Placement, canonical, placement};

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use supersigil_record::observations::{
        Command, CommandCategory, Edit, EditOperation, FileState, Material, Observation, Role, Turn,
    };
    use supersigil_record::{EventId, SessionId, Timestamp, TurnId};

    use super::Evidence;

    fn turn(id: &str, parent: Option<&str>, role: Role) -> Observation {
        Observation::Turn(Turn {
            id: TurnId::new(id),
            session: SessionId::new("s1"),
            parent: parent.map(TurnId::new),
            role,
            time: Timestamp::new("2026-09-29T10:00:00.000Z"),
            sidechain: false,
            agent_id: None,
            excerpt: Material::Retained(format!("text of {id}")),
            source_ordinal: 0,
            transcript: Some("t.jsonl".to_owned()),
        })
    }

    fn edit_in_turn(turn: &str) -> Edit {
        Edit {
            id: EventId::new("e1"),
            turn: TurnId::new(turn),
            session: SessionId::new("s1"),
            path: PathBuf::from("a.rs"),
            before: FileState::unknown(),
            after: FileState::unknown(),
            patch: Material::unavailable("test"),
            old_text: Material::unavailable("test"),
            new_text: Material::unavailable("test"),
            replace_all: false,
            operation: EditOperation::Replace,
            checkout: PathBuf::from("/work/repo"),
            time: Timestamp::new("2026-09-29T10:00:00.000Z"),
            source_ordinal: 0,
            agent_id: None,
            transcript: Some("t.jsonl".to_owned()),
        }
    }

    fn command(id: &str, cmd: &str, checkout: &Path, transcript: &str) -> Observation {
        Observation::Command(Command {
            id: EventId::new(id),
            turn: TurnId::new("a1"),
            session: SessionId::new("s1"),
            cmd: cmd.to_owned(),
            exit: Some(0),
            stdout_tail: Material::unavailable("test"),
            stderr_tail: Material::unavailable("test"),
            category: CommandCategory::Other,
            reported_error: false,
            outcome: None,
            started: Timestamp::new("2026-09-29T10:00:00.000Z"),
            ended: Some(Timestamp::new("2026-09-29T10:00:01.000Z")),
            checkout: checkout.to_path_buf(),
            source_ordinal: 0,
            agent_id: None,
            transcript: Some(transcript.to_owned()),
        })
    }

    #[test]
    fn prompt_is_the_nearest_human_ancestor() {
        let observations = [
            turn("u1", None, Role::Human),
            turn("a1", Some("u1"), Role::Agent),
            turn("u2", Some("a1"), Role::Tool),
            turn("a2", Some("u2"), Role::Agent),
        ];
        let evidence = Evidence::index(&observations);
        let prompt = evidence.prompt(&edit_in_turn("a2")).unwrap();
        assert_eq!(prompt.turn, "u1");
        assert_eq!(prompt.role, Role::Human);
        assert_eq!(prompt.excerpt, Material::Retained("text of u1".to_owned()));
    }

    #[test]
    fn a_broken_parent_chain_has_no_prompt() {
        let observations = [turn("a2", Some("missing"), Role::Agent)];
        let evidence = Evidence::index(&observations);
        assert_eq!(evidence.prompt(&edit_in_turn("a2")), None);
        assert_eq!(evidence.prompt(&edit_in_turn("unknown")), None);
    }

    #[test]
    fn mentions_match_absolute_and_checkout_relative_paths() {
        let repo = PathBuf::from("/work/repo");
        let observations = [
            command("c1", "rm old.txt", &repo, "t.jsonl"),
            command("c2", "cat lib.rs", &repo.join("src"), "t.jsonl"),
            command(
                "c3",
                "cat /work/repo/src/lib.rs",
                Path::new("/elsewhere"),
                "t.jsonl",
            ),
            command("c4", "rm old.txt", &repo, "other.jsonl"),
        ];
        let evidence = Evidence::index(&observations);
        let transcripts = BTreeSet::from(["t.jsonl".to_owned()]);
        let worktrees = [repo];
        let ids = |path: &str| -> Vec<String> {
            evidence
                .mentions(&transcripts, &worktrees, path)
                .into_iter()
                .map(|m| m.command)
                .collect()
        };
        assert_eq!(ids("old.txt"), vec!["c1"]);
        // c3 ran outside the file's worktree; c4 is not a candidate transcript.
        assert_eq!(ids("src/lib.rs"), vec!["c2"]);
    }
}
```

Add to `crates/supersigil-cli/src/lib.rs`, after `pub mod error;`:

```rust
/// Recorded evidence around attribution: candidate transcripts and their
/// capture limitations, command mentions, and prompts.
pub mod evidence;
```

Run: `cargo nextest run -p supersigil evidence`
Expected: compile error, `unresolved import super::Evidence`.

- [ ] **Step 8: Implement the evidence index**

Insert above the `#[cfg(test)]` line in `crates/supersigil-cli/src/evidence.rs`:

```rust
/// Turns, commands, capture limitations, and transcript locations of every
/// pinned record, indexed for the lookups a review makes.
#[derive(Debug, Default)]
pub struct Evidence {
    /// Every turn by session and id; the first occurrence wins.
    turns: BTreeMap<(SessionId, TurnId), Turn>,
    /// Every command by id; the first occurrence wins.
    commands: BTreeMap<EventId, Command>,
    /// Capture limitations by transcript.
    limitations: BTreeMap<String, Vec<CaptureLimitation>>,
    /// The session each transcript belongs to.
    sessions: BTreeMap<String, SessionId>,
    /// Working directories each transcript's observations name.
    checkouts: BTreeMap<String, Vec<PathBuf>>,
}

impl Evidence {
    /// Indexes `observations`, keeping the first occurrence of each turn and
    /// command.
    #[must_use]
    pub fn index<'a>(observations: impl IntoIterator<Item = &'a Observation>) -> Self {
        let mut evidence = Self::default();
        for observation in observations {
            match observation {
                Observation::SessionStart(start) => {
                    if let Some(path) = start.source_ids.get("path") {
                        evidence.locate(path, &start.session, &start.checkout);
                    }
                }
                Observation::Turn(turn) => {
                    evidence
                        .turns
                        .entry((turn.session.clone(), turn.id.clone()))
                        .or_insert_with(|| turn.clone());
                }
                Observation::Edit(edit) => {
                    if let Some(transcript) = &edit.transcript {
                        evidence.locate(transcript, &edit.session, &edit.checkout);
                    }
                }
                Observation::Command(command) => {
                    if let Some(transcript) = &command.transcript {
                        evidence.locate(transcript, &command.session, &command.checkout);
                    }
                    evidence
                        .commands
                        .entry(command.id.clone())
                        .or_insert_with(|| command.clone());
                }
                Observation::CaptureLimitation(limitation) => {
                    evidence
                        .sessions
                        .entry(limitation.transcript.clone())
                        .or_insert_with(|| limitation.session.clone());
                    evidence
                        .limitations
                        .entry(limitation.transcript.clone())
                        .or_default()
                        .push(limitation.clone());
                }
                Observation::SessionEnd(_) => {}
            }
        }
        evidence
    }

    /// Records that `transcript` belongs to `session` and names `checkout`.
    fn locate(&mut self, transcript: &str, session: &SessionId, checkout: &Path) {
        self.sessions
            .entry(transcript.to_owned())
            .or_insert_with(|| session.clone());
        let checkouts = self.checkouts.entry(transcript.to_owned()).or_default();
        if !checkouts.iter().any(|known| known == checkout) {
            checkouts.push(checkout.to_path_buf());
        }
    }

    /// The candidate transcripts (design section 1, step 7): the union of
    /// the transcripts containing one of `edits` and the transcripts with an
    /// observation whose working directory lies in one of `worktrees`,
    /// whether or not any of their edits end up attributed.
    #[must_use]
    pub fn candidate_transcripts<'a>(
        &self,
        edits: impl IntoIterator<Item = &'a Edit>,
        worktrees: &[PathBuf],
    ) -> BTreeSet<String> {
        let mut transcripts: BTreeSet<String> = edits
            .into_iter()
            .filter_map(|e| e.transcript.clone())
            .collect();
        for (transcript, checkouts) in &self.checkouts {
            let inside = checkouts.iter().any(|c| {
                worktrees
                    .iter()
                    .any(|w| placement(c, w) != Placement::Outside)
            });
            if inside {
                transcripts.insert(transcript.clone());
            }
        }
        transcripts
    }

    /// One entry per transcript in `transcripts`, with its session and every
    /// capture limitation recorded for it, never localized to a path.
    #[must_use]
    pub fn transcripts(&self, transcripts: &BTreeSet<String>) -> Vec<TranscriptInfo> {
        transcripts
            .iter()
            .map(|transcript| TranscriptInfo {
                transcript: transcript.clone(),
                session: self
                    .sessions
                    .get(transcript)
                    .map(|s| s.as_str().to_owned())
                    .unwrap_or_default(),
                capture_limitations: self
                    .limitations
                    .get(transcript)
                    .cloned()
                    .unwrap_or_default(),
                localized: false,
            })
            .collect()
    }

    /// Commands from `transcripts`, run in a checkout inside one of
    /// `worktrees`, whose text contains `path` (relative to that worktree)
    /// as seen from the command's checkout or as an absolute path. Textual
    /// evidence only: a mention is never attribution, and commands are
    /// never replayed.
    #[must_use]
    pub fn mentions(
        &self,
        transcripts: &BTreeSet<String>,
        worktrees: &[PathBuf],
        path: &str,
    ) -> Vec<Mention> {
        let mut mentions = Vec::new();
        for command in self.commands.values() {
            if !command
                .transcript
                .as_ref()
                .is_some_and(|t| transcripts.contains(t))
            {
                continue;
            }
            let Some(worktree) = worktrees
                .iter()
                .find(|w| placement(&command.checkout, w) != Placement::Outside)
            else {
                continue;
            };
            let absolute = worktree.join(path).to_string_lossy().into_owned();
            let relative = relative_to_checkout(&command.checkout, worktree, path);
            let text = &command.cmd;
            if text.contains(&absolute) || relative.is_some_and(|r| text.contains(r.as_str())) {
                mentions.push(Mention {
                    command: command.id.as_str().to_owned(),
                    session: command.session.as_str().to_owned(),
                    turn: command.turn.as_str().to_owned(),
                    checkout: command.checkout.display().to_string(),
                    text: text.clone(),
                    result: command.ended.as_ref().map(|_| MentionResult {
                        exit: command.exit,
                        outcome: command.outcome.map(outcome_word),
                    }),
                });
            }
        }
        mentions
    }

    /// The nearest recorded ancestor of `edit`'s turn, following parent
    /// pointers within its session, whose role is Human or Delegation;
    /// `None` when the chain breaks first. It is the message that preceded
    /// the edit in the conversation, not its cause or its rationale.
    #[must_use]
    pub fn prompt(&self, edit: &Edit) -> Option<PromptInfo> {
        let mut parent = self
            .turns
            .get(&(edit.session.clone(), edit.turn.clone()))?
            .parent
            .as_ref();
        let mut steps = 0;
        while let Some(id) = parent {
            steps += 1;
            if steps > self.turns.len() {
                return None;
            }
            let turn = self.turns.get(&(edit.session.clone(), id.clone()))?;
            if matches!(turn.role, Role::Human | Role::Delegation) {
                return Some(PromptInfo {
                    turn: turn.id.as_str().to_owned(),
                    role: turn.role,
                    excerpt: turn.excerpt.clone(),
                });
            }
            parent = turn.parent.as_ref();
        }
        None
    }
}

/// The review's `edits` map entry for `accepted`.
#[must_use]
pub fn edit_info(accepted: &AcceptedEdit, evidence: &Evidence) -> EditInfo {
    let edit = &accepted.edit;
    EditInfo {
        session: edit.session.as_str().to_owned(),
        transcript: edit.transcript.clone(),
        turn: edit.turn.as_str().to_owned(),
        time: edit.time.as_str().to_owned(),
        worktree: accepted.worktree.display().to_string(),
        operation: edit.operation,
        prompt: evidence.prompt(edit),
    }
}

/// `path` (relative to `worktree`) as seen from `checkout`, a directory
/// inside it: `None` when the file is not below `checkout`.
fn relative_to_checkout(checkout: &Path, worktree: &Path, path: &str) -> Option<String> {
    let checkout = canonical(checkout).unwrap_or_else(|_| checkout.to_path_buf());
    let below = checkout.strip_prefix(worktree).ok()?;
    let prefix = below
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    if prefix.is_empty() {
        return Some(path.to_owned());
    }
    path.strip_prefix(&format!("{}/", prefix.join("/")))
        .map(str::to_owned)
}

/// The JSON word for a command outcome.
fn outcome_word(outcome: Outcome) -> String {
    match outcome {
        Outcome::Passed => "passed",
        Outcome::Failed => "failed",
    }
    .to_owned()
}
```

Run: `cargo nextest run -p supersigil evidence`
Expected: the three evidence tests pass.

- [ ] **Step 9: Write the shared pipeline**

Create `crates/supersigil-cli/src/pipeline.rs`:

```rust
//! The steps `review` and `why` share (design section 1, steps 1 to 7):
//! resolve the change, find the originating worktrees, reconcile and pin
//! every involved record, capture the working tree after the pin, and map
//! the recorded edits onto worktrees. Also the conversions from git and
//! record data to the review model's plain types.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use supersigil_anchor::walk::{AcceptedEdit, dedup};
use supersigil_anchor::{
    CandidateEdit, Conflict, DEFAULT_BUDGET_BYTES, PathAttribution, Request, State, TargetKind,
    attribute,
};
use supersigil_git::bytes::{Conversion, worktree_form};
use supersigil_git::changes::{ChangeStatus, FileKind, Mode};
use supersigil_git::origin::{Origins, find_origins};
use supersigil_git::snapshot::{
    NotCaptured, NotCapturedCause, OnDisk, SnapshotOptions, WorkingTreeSnapshot,
    snapshot_working_tree,
};
use supersigil_git::worktree::{Worktree, list_worktrees};
use supersigil_git::{
    Ancestry, Base, Git, ObjectId, Repo, RepoPath, ResolvedRange, ResolvedTarget, TargetSpec,
    resolve_range,
};
use supersigil_record::observations::Observation;
use supersigil_record::{EventId, RecordId, Revision};
use supersigil_review::mapping::lines_correspond;
use supersigil_review::model::{
    AncestryInfo, BaseInfo, BytesStatus, CommitOriginInfo, EvidenceInfo, FileKindInfo, FileStatus,
    OriginsInfo, RecordInfo, TargetInfo, TargetKindInfo, UnavailableInfo, UnreconciledInfo,
};
use supersigil_session::checkout::{Placement, canonical, placement};

use crate::error::CliError;
use crate::evidence::Evidence;
use crate::mapping::map_edit;
use crate::reconcile::{InvolvedRecord, LOCK_WAIT, UnreconciledCheckout, reconcile};

/// The base a pipeline run reviews against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseChoice {
    /// `--base`, or the target's default base when `None`.
    Rev(Option<String>),
    /// The empty tree, with every commit reachable from the target in range:
    /// `why` explains a line back to the file's creation.
    EmptyTree,
}

/// Inputs to [`gather`].
#[derive(Debug, Clone)]
pub struct PipelineArgs {
    /// Directory that selects the repository and the reviewed worktree.
    pub checkout: PathBuf,
    /// Canonical current directory, which typed paths are relative to.
    pub cwd: PathBuf,
    /// Records directory.
    pub records_dir: PathBuf,
    /// Claude home used for discovery, if any.
    pub claude_home: Option<PathBuf>,
    /// The base.
    pub base: BaseChoice,
    /// The target.
    pub target: TargetSpec,
    /// Untracked paths to include, as typed.
    pub include_untracked: Vec<String>,
    /// Path filters, as typed.
    pub paths: Vec<String>,
}

/// An edit whose file lies in a candidate worktree.
#[derive(Debug, Clone)]
pub struct MappedCandidate {
    /// The edit, its record, and its worktree, as anchor takes it.
    pub candidate: CandidateEdit,
    /// The file's path relative to its worktree, components joined with `/`.
    pub path: String,
}

/// An edit that survived deduplication across every involved record, with
/// the path its file has in its worktree.
#[derive(Debug, Clone)]
pub struct MappedAccepted {
    /// The deduplicated edit and every record that holds a sighting of it.
    pub accepted: AcceptedEdit,
    /// The file's path relative to its worktree, components joined with `/`.
    pub path: String,
}

/// An edit id whose sightings disagree, and every path those sightings map to.
#[derive(Debug, Clone)]
pub struct PathConflict {
    /// The conflicting edit and the records that hold its sightings.
    pub conflict: Conflict,
    /// The paths its sightings touched, relative to their worktrees.
    pub paths: BTreeSet<String>,
}

/// One involved record, pinned at the revision the review reads.
#[derive(Debug)]
pub struct PinnedRecord {
    /// The record's id.
    pub record_id: RecordId,
    /// The pinned revision.
    pub revision: Revision,
    /// Why reconcile did not bring the record up to date, if it did not.
    pub not_reconciled: Option<String>,
    /// The record's checkout associations at the pinned revision.
    pub associations: Vec<PathBuf>,
    /// Every session's observations at the pinned revision.
    pub observations: Vec<Observation>,
}

/// Everything `review` and `why` need after the shared steps.
#[derive(Debug)]
pub struct Gathered {
    /// The repository, opened in the reviewed worktree.
    pub repo: Repo,
    /// Canonical root of the reviewed worktree.
    pub worktree: PathBuf,
    /// Canonical current directory.
    pub cwd: PathBuf,
    /// Base, target, and ancestry.
    pub range: ResolvedRange,
    /// The target tree: the commit's, or the captured working tree.
    pub target_tree: ObjectId,
    /// The working-tree snapshot, for a working-tree target.
    pub snapshot: Option<WorkingTreeSnapshot>,
    /// Registered worktrees, with paths canonical when they exist.
    pub worktrees: Vec<Worktree>,
    /// The commits in range and the worktrees that originated them.
    pub origins: Origins,
    /// Origin worktrees, plus the reviewed worktree for a working-tree target.
    pub candidate_worktrees: Vec<PathBuf>,
    /// Every involved record, pinned.
    pub records: Vec<PinnedRecord>,
    /// Checkouts whose transcripts no record could take.
    pub unreconciled: Vec<UnreconciledCheckout>,
    /// Every sighting of an edit whose file lies in a candidate worktree.
    pub candidates: Vec<MappedCandidate>,
    /// The candidate edits after deduplication across every record: the
    /// only edits attribution sees.
    pub accepted: Vec<MappedAccepted>,
    /// Edit ids excluded because their sightings disagree.
    pub conflicts: Vec<PathConflict>,
    /// Edits no registered worktree contains.
    pub unplaced_edits: usize,
    /// Whether the repository sets `core.ignorecase`.
    pub ignore_case: bool,
    /// `--include-untracked`, relative to the worktree.
    pub include_untracked: Vec<RepoPath>,
    /// Path filters, relative to the worktree; empty when none was given or
    /// one named the worktree root.
    pub paths: Vec<RepoPath>,
    /// Whether a path selector named the worktree root, which selects
    /// everything.
    pub whole_worktree: bool,
    /// The pinned records' evidence, indexed.
    pub evidence: Evidence,
}

/// Runs the shared steps and returns what they found.
///
/// # Errors
///
/// Returns [`CliError::Git`] if git fails or is too old, the checkout is not
/// in a worktree, or a revision does not resolve;
/// [`CliError::CommandFailed`] if a typed path lies outside the worktree or
/// is not UTF-8; [`CliError::Io`] if the worktree root cannot be resolved;
/// and the errors of [`reconcile`].
pub fn gather(args: &PipelineArgs) -> Result<Gathered, CliError> {
    // 1. The reviewed worktree, the base, and the target kind.
    let repo = Repo::open(Git::new(&args.checkout))?;
    let worktree = canonical(repo.root())?;
    let include_untracked = untracked_paths(&worktree, &args.cwd, &args.include_untracked)?;
    let (paths, whole_worktree) = path_filters(&worktree, &args.cwd, &args.paths)?;
    let range = resolve(&repo, &args.base, &args.target)?;

    // 2. The commits in range and the worktrees that originated them.
    let commits = range.commits(&repo)?;
    let worktrees = registered_worktrees(&repo)?;
    let origins = find_origins(&repo, &worktrees, &commits)?;
    let candidate_worktrees = candidates_for(&range, &origins, &worktree);

    // 3 and 4. Every involved record, reconciled with bounded lock waits.
    //    `list_worktrees` lists the main worktree first; discovery reads the
    //    project directories of a nested worktree's ancestors up to it.
    let main_worktree = worktrees
        .first()
        .map_or_else(|| worktree.clone(), |main| main.path.clone());
    let reconciliation = reconcile(
        &args.records_dir,
        args.claude_home.as_deref(),
        &candidate_worktrees,
        &main_worktree,
        LOCK_WAIT,
    )?;

    // 5. One pinned snapshot per record.
    let records = pin(reconciliation.records)?;

    // 6. The working tree, captured after the pin, so every pinned
    //    observation predates it.
    let (target_tree, snapshot) = match &range.target {
        ResolvedTarget::WorkingTree { .. } => {
            let options = SnapshotOptions {
                include_untracked: include_untracked.clone(),
                pathspecs: paths.clone(),
            };
            let snapshot = snapshot_working_tree(&repo, &options)?;
            (snapshot.tree.clone(), Some(snapshot))
        }
        ResolvedTarget::Commit { tree, .. } => (tree.clone(), None),
    };

    // 7. The recorded edits, mapped onto worktrees.
    let ignore_case = repo.config_bool("core.ignorecase")?.unwrap_or(false);
    let (candidates, unplaced_edits) =
        map_candidates(&records, &worktrees, &candidate_worktrees, ignore_case);
    //    Deduplicated once across every path, so an edit id whose sightings
    //    map to different paths is excluded from all of them.
    let (accepted, conflicts) = deduplicate(&candidates);
    let evidence = Evidence::index(records.iter().flat_map(|r| &r.observations));
    Ok(Gathered {
        repo,
        worktree,
        cwd: args.cwd.clone(),
        range,
        target_tree,
        snapshot,
        worktrees,
        origins,
        candidate_worktrees,
        records,
        unreconciled: reconciliation.unreconciled,
        candidates,
        accepted,
        conflicts,
        unplaced_edits,
        ignore_case,
        include_untracked,
        paths,
        whole_worktree,
        evidence,
    })
}

/// Resolves the range, replacing the base with the empty tree for `why`.
///
/// # Errors
///
/// Returns the git errors of [`resolve_range`] and [`Repo::empty_tree`].
fn resolve(repo: &Repo, base: &BaseChoice, target: &TargetSpec) -> Result<ResolvedRange, CliError> {
    match base {
        BaseChoice::Rev(rev) => Ok(resolve_range(repo, rev.as_deref(), target)?),
        BaseChoice::EmptyTree => {
            let mut range = resolve_range(repo, None, target)?;
            range.base = Base {
                commit: None,
                tree: repo.empty_tree()?,
            };
            range.ancestry = Ancestry::Unavailable;
            Ok(range)
        }
    }
}

/// Registered worktrees, with the paths of those that exist canonicalized.
///
/// # Errors
///
/// Returns the git errors of [`list_worktrees`].
fn registered_worktrees(repo: &Repo) -> Result<Vec<Worktree>, CliError> {
    let mut worktrees = list_worktrees(repo)?;
    for registered in &mut worktrees {
        if let Ok(path) = canonical(&registered.path) {
            registered.path = path;
        }
    }
    Ok(worktrees)
}

/// Origin worktrees, plus the reviewed worktree for a working-tree target.
fn candidates_for(range: &ResolvedRange, origins: &Origins, reviewed: &Path) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = origins
        .origin_worktrees()
        .into_iter()
        .map(|path| canonical(&path).unwrap_or(path))
        .collect();
    if matches!(range.target, ResolvedTarget::WorkingTree { .. }) {
        candidates.push(reviewed.to_path_buf());
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

/// Pins one snapshot per involved record and reads every session from it.
///
/// # Errors
///
/// Returns a store error if a snapshot cannot be read.
fn pin(involved: Vec<InvolvedRecord>) -> Result<Vec<PinnedRecord>, CliError> {
    let mut records = Vec::new();
    for record in involved {
        let snapshot = record.store.snapshot()?;
        let mut observations = Vec::new();
        for session in snapshot.sessions() {
            observations.extend(snapshot.observations(&session)?);
        }
        records.push(PinnedRecord {
            record_id: record.record_id,
            revision: snapshot.revision(),
            not_reconciled: record.not_reconciled,
            associations: snapshot
                .manifest()
                .associations
                .iter()
                .map(|a| a.checkout.clone())
                .collect(),
            observations,
        });
    }
    Ok(records)
}

/// Maps every recorded edit; returns those in a candidate worktree and the
/// number no registered worktree contains.
fn map_candidates(
    records: &[PinnedRecord],
    worktrees: &[Worktree],
    candidate_worktrees: &[PathBuf],
    ignore_case: bool,
) -> (Vec<MappedCandidate>, usize) {
    let roots: Vec<PathBuf> = worktrees.iter().map(|w| w.path.clone()).collect();
    let mut mapped = Vec::new();
    let mut unplaced = 0;
    for record in records {
        for observation in &record.observations {
            let Observation::Edit(edit) = observation else {
                continue;
            };
            match map_edit(edit, &roots, ignore_case) {
                Some(found) if candidate_worktrees.contains(&found.worktree) => {
                    mapped.push(MappedCandidate {
                        candidate: CandidateEdit {
                            record: record.record_id.clone(),
                            worktree: found.worktree,
                            edit: edit.clone(),
                        },
                        path: found.path,
                    });
                }
                Some(_) => {}
                None => unplaced += 1,
            }
        }
    }
    (mapped, unplaced)
}

/// Applies the record's event-identity rule ([`dedup`]) once over every
/// candidate sighting, then gives each accepted edit its path and each
/// conflict the paths all of its sightings touched.
fn deduplicate(candidates: &[MappedCandidate]) -> (Vec<MappedAccepted>, Vec<PathConflict>) {
    let mut paths: BTreeMap<&EventId, BTreeSet<String>> = BTreeMap::new();
    for mapped in candidates {
        paths
            .entry(&mapped.candidate.edit.id)
            .or_default()
            .insert(mapped.path.clone());
    }
    let (accepted, conflicts) = dedup(candidates.iter().map(|m| m.candidate.clone()).collect());
    let path_of = |id: &EventId| paths.get(id).cloned().unwrap_or_default();
    let accepted = accepted
        .into_iter()
        .map(|accepted| {
            // Accepted sightings agree on their payload, so on their path.
            let path = path_of(&accepted.edit.id)
                .into_iter()
                .next()
                .unwrap_or_default();
            MappedAccepted { accepted, path }
        })
        .collect();
    let conflicts = conflicts
        .into_iter()
        .map(|conflict| PathConflict {
            paths: path_of(&conflict.edit),
            conflict,
        })
        .collect();
    (accepted, conflicts)
}

/// Converts `-- <path>` selectors with [`repo_path`]. A selector that
/// names the worktree root selects everything: the result is then no filter
/// at all, with `true` so the scope listing can say why.
///
/// # Errors
///
/// Returns the errors of [`repo_path`].
pub fn path_filters(
    root: &Path,
    cwd: &Path,
    typed: &[String],
) -> Result<(Vec<RepoPath>, bool), CliError> {
    let mut paths = Vec::new();
    for text in typed {
        match repo_path(root, cwd, text)? {
            Some(path) => paths.push(path),
            None => return Ok((Vec::new(), true)),
        }
    }
    Ok((paths, false))
}

/// Converts `--include-untracked` paths with [`repo_path`]. They name
/// files, so the worktree root is never a selector here.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] for a path that names the worktree
/// root, and the errors of [`repo_path`].
pub fn untracked_paths(
    root: &Path,
    cwd: &Path,
    typed: &[String],
) -> Result<Vec<RepoPath>, CliError> {
    typed
        .iter()
        .map(|text| {
            repo_path(root, cwd, text)?.ok_or_else(|| {
                CliError::CommandFailed(format!(
                    "--include-untracked takes files; {text} names the worktree root"
                ))
            })
        })
        .collect()
}

/// `typed`, relative to `cwd` unless absolute, as a path relative to the
/// worktree `root`; `None` when it names `root` itself. Resolved lexically:
/// `.` and `..` apply to the spelling and nothing is read from disk.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when the path lies outside `root` or
/// is not valid UTF-8.
pub fn repo_path(root: &Path, cwd: &Path, typed: &str) -> Result<Option<RepoPath>, CliError> {
    let mut normal = PathBuf::new();
    for component in cwd.join(typed).components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    let relative = normal.strip_prefix(root).map_err(|_outside| {
        CliError::CommandFailed(format!(
            "{typed} is outside the worktree {}",
            root.display()
        ))
    })?;
    let parts = relative
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| CliError::CommandFailed(format!("{typed} is not valid UTF-8")))?;
    Ok((!parts.is_empty()).then(|| RepoPath::from_utf8(&parts.join("/"))))
}

/// Whether a mapped edit path names `path`: exactly, or ignoring ASCII case
/// when the repository sets `core.ignorecase`.
#[must_use]
pub fn same_path(mapped: &str, path: &RepoPath, ignore_case: bool) -> bool {
    path.to_str()
        .is_some_and(|p| p == mapped || (ignore_case && p.eq_ignore_ascii_case(mapped)))
}

/// The Claude home: the flag, then `$HOME/.claude`.
#[must_use]
pub fn claude_home(flag: Option<&Path>) -> Option<PathBuf> {
    flag.map(Path::to_path_buf)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")))
}

/// `path` as the user would type it from `cwd`: relative when `cwd` is at or
/// above the file inside the worktree, absolute otherwise.
#[must_use]
pub fn display_from(worktree: &Path, cwd: &Path, path: &RepoPath) -> String {
    let display = path.display();
    let absolute = || worktree.join(&display).display().to_string();
    let Ok(below) = cwd.strip_prefix(worktree) else {
        return absolute();
    };
    let prefix: Vec<String> = below
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if prefix.is_empty() {
        return display;
    }
    display
        .strip_prefix(&format!("{}/", prefix.join("/")))
        .map_or_else(absolute, str::to_owned)
}

/// Both sides' attribution bytes for one path, and the status the output
/// shows for each.
#[derive(Debug, Clone)]
pub struct PathBytes {
    /// Status of each side's attribution bytes.
    pub status: BytesStatus,
    /// The base side's state, or why attribution is unavailable.
    pub base: Result<State, String>,
    /// The target side's state, or why attribution is unavailable.
    pub target: Result<State, String>,
    /// The target's converted bytes when they do not map line for line onto
    /// its blob, so `target` holds that reason instead of them.
    pub target_unmapped: Option<Vec<u8>>,
}

impl PathBytes {
    /// The target in worktree form whenever its conversion succeeded,
    /// whether or not it maps onto the blob lines: what the file on disk
    /// holds when the capture is current.
    #[must_use]
    pub fn target_worktree_form(&self) -> Option<&[u8]> {
        match &self.target {
            Ok(state) => state.bytes(),
            Err(_) => self.target_unmapped.as_deref(),
        }
    }
}

/// Reads both sides' attribution bytes for `path`: each blob converted to
/// worktree form, kept only when it maps line for line onto the blob.
///
/// # Errors
///
/// Returns [`CliError::Git`] if a conversion cannot be run.
pub fn path_bytes(
    repo: &Repo,
    path: &RepoPath,
    base: Option<(&ObjectId, &[u8])>,
    target: Option<(&ObjectId, &[u8])>,
) -> Result<PathBytes, CliError> {
    let base = side_bytes(repo, path, base)?;
    let target = side_bytes(repo, path, target)?;
    Ok(PathBytes {
        status: BytesStatus {
            base: base.status,
            target: target.status,
        },
        base: base.state,
        target: target.state,
        target_unmapped: target.unmapped,
    })
}

/// One side of [`PathBytes`].
struct Side {
    status: String,
    state: Result<State, String>,
    unmapped: Option<Vec<u8>>,
}

/// One side of [`path_bytes`].
///
/// # Errors
///
/// Returns [`CliError::Git`] if the conversion cannot be run.
fn side_bytes(
    repo: &Repo,
    path: &RepoPath,
    blob: Option<(&ObjectId, &[u8])>,
) -> Result<Side, CliError> {
    let Some((id, bytes)) = blob else {
        return Ok(Side {
            status: "absent".to_owned(),
            state: Ok(State::Absent),
            unmapped: None,
        });
    };
    Ok(match worktree_form(repo, path, id, bytes)? {
        Conversion::Identical => Side {
            status: "identical".to_owned(),
            state: Ok(State::Present(bytes.to_vec())),
            unmapped: None,
        },
        Conversion::Converted(converted) if lines_correspond(bytes, &converted) => Side {
            status: "converted".to_owned(),
            state: Ok(State::Present(converted)),
            unmapped: None,
        },
        Conversion::Converted(converted) => Side {
            status: "converted".to_owned(),
            state: Err("conversion changed line structure".to_owned()),
            unmapped: Some(converted),
        },
        Conversion::Failed { stderr_tail, .. } => Side {
            status: format!("failed: {stderr_tail}"),
            state: Err(format!("conversion failed: {stderr_tail}")),
            unmapped: None,
        },
    })
}

/// Runs anchor for `path` with the accepted edits mapped to it, each passed
/// as one candidate carrying its first record, and attaches the conflicts
/// whose sightings touched the path.
///
/// # Errors
///
/// Returns why attribution is unavailable when either side's bytes are.
pub fn attribute_path(
    g: &Gathered,
    path: &RepoPath,
    bytes: &PathBytes,
) -> Result<PathAttribution, String> {
    let base = bytes.base.clone()?;
    let target = bytes.target.clone()?;
    let edits = g
        .accepted
        .iter()
        .filter(|m| same_path(&m.path, path, g.ignore_case))
        .filter_map(|m| {
            Some(CandidateEdit {
                record: m.accepted.records.first()?.clone(),
                worktree: m.accepted.worktree.clone(),
                edit: m.accepted.edit.clone(),
            })
        })
        .collect();
    let target_kind = match g.range.target {
        ResolvedTarget::WorkingTree { .. } => TargetKind::WorkingTree,
        ResolvedTarget::Commit { .. } => TargetKind::Commit,
    };
    let mut attribution = attribute(Request {
        base,
        target,
        target_kind,
        reviewed_worktree: g.worktree.clone(),
        edits,
        budget_bytes: DEFAULT_BUDGET_BYTES,
    });
    attribution.conflicts = conflicts_for(g, path);
    Ok(attribution)
}

/// The conflicts any of whose sightings touched `path`.
#[must_use]
pub fn conflicts_for(g: &Gathered, path: &RepoPath) -> Vec<Conflict> {
    g.conflicts
        .iter()
        .filter(|c| c.paths.iter().any(|p| same_path(p, path, g.ignore_case)))
        .map(|c| c.conflict.clone())
        .collect()
}

/// Every conflict, for the evidence block.
#[must_use]
pub fn all_conflicts(g: &Gathered) -> Vec<Conflict> {
    g.conflicts.iter().map(|c| c.conflict.clone()).collect()
}

/// The candidate transcripts of `g` (design section 1, step 7).
#[must_use]
pub fn candidate_transcripts(g: &Gathered) -> BTreeSet<String> {
    g.evidence.candidate_transcripts(
        g.candidates.iter().map(|m| &m.candidate.edit),
        &g.candidate_worktrees,
    )
}

/// The review model's base block.
#[must_use]
pub fn base_info(g: &Gathered) -> BaseInfo {
    BaseInfo {
        commit: g.range.base.commit.as_ref().map(ToString::to_string),
        tree: g.range.base.tree.to_string(),
        ancestry: match g.range.ancestry {
            Ancestry::Ancestor => AncestryInfo::Ancestor,
            Ancestry::NotAncestor => AncestryInfo::NotAncestor,
            Ancestry::Unavailable => AncestryInfo::Unavailable,
        },
    }
}

/// The review model's target block.
#[must_use]
pub fn target_info(g: &Gathered) -> TargetInfo {
    TargetInfo {
        kind: match g.range.target {
            ResolvedTarget::WorkingTree { .. } => TargetKindInfo::WorkingTree,
            ResolvedTarget::Commit { .. } => TargetKindInfo::Commit,
        },
        commit: g.range.target_commit().map(ToString::to_string),
        tree: g.target_tree.to_string(),
    }
}

/// The origins block. `find_origins` sees only git, so record checkouts that
/// no longer exist or lie in no registered worktree (a worktree already
/// pruned) are added here as unavailable origin evidence.
#[must_use]
pub fn origins_info(g: &Gathered) -> OriginsInfo {
    let mut unavailable: Vec<UnavailableInfo> = g
        .origins
        .unavailable
        .iter()
        .map(|u| UnavailableInfo {
            worktree: u.worktree.display().to_string(),
            reason: u.reason.clone(),
        })
        .collect();
    for association in g.records.iter().flat_map(|r| &r.associations) {
        let registered = g.worktrees.iter().any(|w| {
            placement(association, &w.path) != Placement::Outside
                || placement(&w.path, association) != Placement::Outside
        });
        if !association.exists() || !registered {
            unavailable.push(UnavailableInfo {
                worktree: association.display().to_string(),
                reason: "origin evidence unavailable: worktree no longer registered".to_owned(),
            });
        }
    }
    unavailable.sort_by(|a, b| a.worktree.cmp(&b.worktree));
    unavailable.dedup();
    OriginsInfo {
        commits: g
            .origins
            .commits
            .iter()
            .map(|c| CommitOriginInfo {
                commit: c.commit.to_string(),
                worktrees: c
                    .worktrees
                    .iter()
                    .map(|w| w.display().to_string())
                    .collect(),
            })
            .collect(),
        without_origin: g
            .origins
            .without_origin
            .iter()
            .map(ToString::to_string)
            .collect(),
        unavailable,
    }
}

/// The records block: each involved record, its pinned revision, and
/// whether it was reconciled.
#[must_use]
pub fn records_info(g: &Gathered) -> Vec<RecordInfo> {
    g.records
        .iter()
        .map(|r| RecordInfo {
            id: r.record_id.as_str().to_owned(),
            revision: Some(r.revision.get()),
            reconciled: r.not_reconciled.is_none(),
            reason: r.not_reconciled.clone(),
        })
        .collect()
}

/// The evidence block: candidate transcripts with every capture limitation,
/// unplaced edits, unreconciled checkouts, and `conflicting_edits`.
#[must_use]
pub fn evidence_info(g: &Gathered, conflicting_edits: Vec<Conflict>) -> EvidenceInfo {
    EvidenceInfo {
        candidate_transcripts: g.evidence.transcripts(&candidate_transcripts(g)),
        unplaced_edits: g.unplaced_edits,
        unreconciled_checkouts: g
            .unreconciled
            .iter()
            .map(|u| UnreconciledInfo {
                checkout: u.checkout.display().to_string(),
                transcripts: u.transcripts,
                reason: u.reason.clone(),
            })
            .collect(),
        conflicting_edits,
    }
}

/// The review model's file status.
#[must_use]
pub fn file_status(status: ChangeStatus) -> FileStatus {
    match status {
        ChangeStatus::Added => FileStatus::Added,
        ChangeStatus::Deleted => FileStatus::Deleted,
        ChangeStatus::Modified => FileStatus::Modified,
        ChangeStatus::TypeChanged => FileStatus::TypeChanged,
    }
}

/// The review model's file kind.
#[must_use]
pub fn file_kind(kind: FileKind) -> FileKindInfo {
    match kind {
        FileKind::Text => FileKindInfo::Text,
        FileKind::Binary => FileKindInfo::Binary,
        FileKind::TooLarge => FileKindInfo::TooLarge,
        FileKind::ModeOnly => FileKindInfo::ModeOnly,
        FileKind::Symlink => FileKindInfo::Symlink,
        FileKind::Gitlink => FileKindInfo::Gitlink,
        FileKind::TypeChange => FileKindInfo::TypeChange,
        FileKind::UnsupportedPath => FileKindInfo::UnsupportedPath,
    }
}

/// A mode as six octal digits, `000000` when absent.
#[must_use]
pub fn mode_text(mode: Mode) -> String {
    format!("{:06o}", mode.0)
}

/// The JSON word for a path's presence on disk.
#[must_use]
pub fn on_disk_word(on_disk: OnDisk) -> &'static str {
    match on_disk {
        OnDisk::Present => "present",
        OnDisk::Missing => "missing",
    }
}

/// The JSON word for why a path's on-disk state was not captured.
#[must_use]
pub fn cause_word(cause: NotCapturedCause) -> &'static str {
    match cause {
        NotCapturedCause::AssumeUnchanged => "assume_unchanged",
        NotCapturedCause::SkipWorktree => "skip_worktree",
        NotCapturedCause::NotStaged => "not_staged",
    }
}

/// The sentence for a path whose on-disk state was not captured.
#[must_use]
pub fn not_captured_reason(entry: &NotCaptured) -> String {
    let what = match entry.on_disk {
        OnDisk::Present => "present on disk; contents not captured",
        OnDisk::Missing => "missing on disk; deletion not captured",
    };
    format!("{what} ({})", cause_word(entry.cause))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use supersigil_git::RepoPath;
    use supersigil_git::changes::Mode;

    use super::{display_from, mode_text, repo_path, same_path};

    #[test]
    fn typed_paths_resolve_relative_to_the_current_directory() {
        let root = Path::new("/work/repo");
        let path = |cwd: &str, typed: &str| {
            repo_path(root, Path::new(cwd), typed)
                .unwrap()
                .map(|p| p.display())
        };
        assert_eq!(
            path("/work/repo", "src/lib.rs"),
            Some("src/lib.rs".to_owned())
        );
        assert_eq!(
            path("/work/repo/src", "lib.rs"),
            Some("src/lib.rs".to_owned())
        );
        assert_eq!(
            path("/work/repo/src", "../README.md"),
            Some("README.md".to_owned())
        );
        assert_eq!(
            path("/elsewhere", "/work/repo/a.rs"),
            Some("a.rs".to_owned())
        );
        assert_eq!(path("/work/repo", "."), None);
        repo_path(root, Path::new("/work/repo"), "../other/a.rs").unwrap_err();
    }

    #[test]
    fn paths_match_ignoring_case_only_when_asked() {
        let path = RepoPath::from_utf8("src/Lib.rs");
        assert!(same_path("src/Lib.rs", &path, false));
        assert!(!same_path("src/lib.rs", &path, false));
        assert!(same_path("src/lib.rs", &path, true));
    }

    #[test]
    fn displayed_paths_are_relative_to_the_current_directory() {
        let root = Path::new("/work/repo");
        let path = RepoPath::from_utf8("src/new.rs");
        assert_eq!(display_from(root, root, &path), "src/new.rs");
        assert_eq!(display_from(root, &root.join("src"), &path), "new.rs");
        assert_eq!(
            display_from(root, &root.join("docs"), &path),
            root.join("src/new.rs").display().to_string()
        );
    }

    #[test]
    fn modes_print_as_six_octal_digits() {
        assert_eq!(mode_text(Mode::REGULAR), "100644");
        assert_eq!(mode_text(Mode::ABSENT), "000000");
    }
}
```

Add to `crates/supersigil-cli/src/lib.rs`, before `pub mod reconcile;`:

```rust
/// The steps `review` and `why` share, and conversions to the review model.
pub mod pipeline;
```

Run: `cargo nextest run -p supersigil pipeline`
Expected: the four pipeline tests pass.

- [ ] **Step 10: Write the `review` command**

Create `crates/supersigil-cli/src/commands/review.rs`:

```rust
//! `supersigil review`: the change between a base and a target, each changed
//! line attributed to the recorded edits that produced it, or the reason it
//! is not.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write as _};
use std::path::PathBuf;

use supersigil_anchor::PathAttribution;
use supersigil_git::bytes::{blob_sizes, read_blobs};
use supersigil_git::changes::{
    Change, FileKind, MAX_DIFF_BYTES, changed_paths, classify_bytes, classify_change,
};
use supersigil_git::{Ancestry, ObjectId, RepoPath, ResolvedTarget, TargetSpec};
use supersigil_review::model::{
    BytesStatus, EditInfo, FileInput, FileReview, FileStatus, NotCapturedInfo, RepoPathInfo,
    ScopeInfo, UntrackedInfo, file_review, unattributed_summary,
};
use supersigil_review::outcome::{AttributionState, Outcome};
use supersigil_review::summary::render_summary;
use supersigil_review::{REVIEW_SCHEMA, Review};

use crate::commands::ReviewArgs;
use crate::error::CliError;
use crate::evidence::edit_info;
use crate::format::{OutputFormat, escape_control, write_json};
use crate::pipeline::{
    BaseChoice, Gathered, PipelineArgs, all_conflicts, attribute_path, base_info,
    candidate_transcripts, cause_word, claude_home, display_from, evidence_info, file_kind,
    file_status, gather, mode_text, on_disk_word, origins_info, path_bytes, records_info,
    same_path, target_info,
};
use crate::record_dir;

/// Runs `supersigil review`.
///
/// # Errors
///
/// Returns [`CliError`] when the repository, a revision, a blob, or a record
/// cannot be read (the errors of [`gather`]), or when writing the output
/// fails.
pub fn run(args: &ReviewArgs) -> Result<(), CliError> {
    let g = gather(&PipelineArgs {
        checkout: record_dir::canonical_checkout(args.checkout.as_deref())?,
        cwd: record_dir::canonical_checkout(None)?,
        records_dir: record_dir::resolve_record_dir(args.record_dir.as_deref())?,
        claude_home: claude_home(args.claude_home.as_deref()),
        base: BaseChoice::Rev(args.base.clone()),
        target: args
            .target
            .clone()
            .map_or(TargetSpec::WorkingTree, TargetSpec::Commit),
        include_untracked: args.include_untracked.clone(),
        paths: args.paths.clone(),
    })?;
    let review = build(&g)?;
    match args.format.resolve() {
        OutputFormat::Json => write_json(&review)?,
        OutputFormat::Terminal => io::stdout()
            .lock()
            .write_all(render_summary(&review, escape_control).as_bytes())?,
    }
    Ok(())
}

/// Builds the review model from the gathered inputs.
///
/// # Errors
///
/// Returns [`CliError::Git`] if the diff, a blob, or a conversion cannot be
/// read.
fn build(g: &Gathered) -> Result<Review, CliError> {
    let mut changes = changed_paths(&g.repo, &g.range.base.tree, &g.target_tree, &g.paths)?;
    changes.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let blobs = small_blobs(g, &changes)?;
    let transcripts = candidate_transcripts(g);
    let mention_worktrees: Vec<PathBuf> = match g.range.target {
        ResolvedTarget::WorkingTree { .. } => vec![g.worktree.clone()],
        ResolvedTarget::Commit { .. } => g.candidate_worktrees.clone(),
    };
    let mut files = Vec::new();
    let mut edits: BTreeMap<String, EditInfo> = BTreeMap::new();
    for change in &changes {
        let (file, attribution) = review_file(g, change, &blobs, &transcripts, &mention_worktrees)?;
        if let Some(attribution) = attribution {
            for accepted in &attribution.accepted {
                edits
                    .entry(accepted.edit.id.as_str().to_owned())
                    .or_insert_with(|| edit_info(accepted, &g.evidence));
            }
        }
        files.push(file);
    }
    let scope = scope_info(g);
    Ok(Review {
        schema: REVIEW_SCHEMA,
        worktree: g.worktree.display().to_string(),
        base: base_info(g),
        target: target_info(g),
        unattributed: unattributed_summary(&files, &scope),
        scope,
        origins: origins_info(g),
        records: records_info(g),
        evidence: evidence_info(g, all_conflicts(g)),
        files,
        edits,
    })
}

/// Blob bytes of every side that can be diffed: not classified away by mode
/// or path, and within [`MAX_DIFF_BYTES`].
///
/// # Errors
///
/// Returns [`CliError::Git`] if the sizes or the blobs cannot be read.
fn small_blobs(g: &Gathered, changes: &[Change]) -> Result<BTreeMap<ObjectId, Vec<u8>>, CliError> {
    let ids: Vec<ObjectId> = changes
        .iter()
        .filter(|c| classify_change(c).is_none())
        .flat_map(|c| [c.old_blob.clone(), c.new_blob.clone()])
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let sizes = blob_sizes(&g.repo, &ids)?;
    let small: Vec<ObjectId> = ids
        .into_iter()
        .filter(|id| sizes.get(id).is_some_and(|size| *size <= MAX_DIFF_BYTES))
        .collect();
    Ok(read_blobs(&g.repo, &small)?)
}

/// Reviews one changed path; returns anchor's result with it when the path
/// was attributed.
///
/// # Errors
///
/// Returns [`CliError::Git`] if a conversion cannot be run.
fn review_file(
    g: &Gathered,
    change: &Change,
    blobs: &BTreeMap<ObjectId, Vec<u8>>,
    transcripts: &BTreeSet<String>,
    mention_worktrees: &[PathBuf],
) -> Result<(FileReview, Option<PathAttribution>), CliError> {
    // Both references borrow from the map, so they outlive the lookup key.
    let side = |id: Option<&ObjectId>| {
        id.and_then(|id| blobs.get_key_value(id))
            .map(|(id, bytes)| (id, bytes.as_slice()))
    };
    let (old, new) = (
        side(change.old_blob.as_ref()),
        side(change.new_blob.as_ref()),
    );
    let unread = (change.old_blob.is_some() && old.is_none())
        || (change.new_blob.is_some() && new.is_none());
    let kind = classify_change(change).unwrap_or_else(|| {
        if unread {
            FileKind::TooLarge
        } else {
            classify_bytes(old.map(|(_, b)| b), new.map(|(_, b)| b))
        }
    });
    let text = kind == FileKind::Text;
    let (bytes_status, attribution) = if text {
        let bytes = path_bytes(&g.repo, &change.path, old, new)?;
        let attribution = attribute_path(g, &change.path, &bytes);
        (bytes.status, attribution)
    } else {
        let not_read = BytesStatus {
            base: "not read".to_owned(),
            target: "not read".to_owned(),
        };
        (
            not_read,
            Err(format!("not diffed: {}", file_kind(kind).as_str())),
        )
    };
    let state = match &attribution {
        Ok(found) => AttributionState::Available(found),
        Err(reason) => AttributionState::Unavailable {
            reason: reason.clone(),
        },
    };
    let path = RepoPathInfo {
        display: change.path.display(),
        escaped: change
            .path
            .to_str()
            .is_none()
            .then(|| change.path.escaped()),
    };
    let mentions = change
        .path
        .to_str()
        .map(|p| g.evidence.mentions(transcripts, mention_worktrees, p))
        .unwrap_or_default();
    let mut file = file_review(FileInput {
        path: &path,
        status: file_status(change.status),
        old_mode: mode_text(change.old_mode),
        new_mode: mode_text(change.new_mode),
        old_blob: change.old_blob.as_ref().map(ToString::to_string),
        new_blob: change.new_blob.as_ref().map(ToString::to_string),
        kind: file_kind(kind),
        bytes_status,
        base_blob: old.filter(|_| text).map(|(_, b)| b),
        target_blob: new.filter(|_| text).map(|(_, b)| b),
        attribution: state,
        mentions,
    });
    // Mentions matter where nothing else explains the file (design section 4).
    if file.status != FileStatus::Deleted && !has_unattributed(&file) {
        file.mentions.clear();
    }
    Ok((file, attribution.ok()))
}

/// Whether any span of `file` is unattributed.
fn has_unattributed(file: &FileReview) -> bool {
    file.hunks
        .iter()
        .flat_map(|h| &h.spans)
        .any(|s| matches!(s.outcome, Outcome::Unattributed { .. }))
}

/// The scope block: options, untracked files (those a recorded edit wrote
/// first, each with the flag that includes it), paths whose on-disk state
/// was not captured, unmerged paths, and the ancestry note.
fn scope_info(g: &Gathered) -> ScopeInfo {
    let mut info = ScopeInfo {
        include_untracked: g.include_untracked.iter().map(RepoPath::display).collect(),
        paths: g.paths.iter().map(RepoPath::display).collect(),
        untracked_excluded: Vec::new(),
        not_captured: Vec::new(),
        skip_worktree_absent: 0,
        unmerged: Vec::new(),
        path_note: g.whole_worktree.then(|| {
            "a path selector names the worktree root, so the review covers the whole worktree"
                .to_owned()
        }),
        ancestry_note: (g.range.ancestry == Ancestry::NotAncestor).then(|| {
            "the base is not an ancestor of the target; the diff includes changes made on the base side"
                .to_owned()
        }),
    };
    let Some(snapshot) = &g.snapshot else {
        return info;
    };
    for path in &snapshot.untracked {
        let recorded_edit = g
            .candidates
            .iter()
            .any(|m| m.candidate.worktree == g.worktree && same_path(&m.path, path, g.ignore_case));
        info.untracked_excluded.push(UntrackedInfo {
            path: path.display(),
            recorded_edit,
            include_flag: format!(
                "--include-untracked {}",
                display_from(&g.worktree, &g.cwd, path)
            ),
        });
    }
    info.untracked_excluded.sort_by(|a, b| {
        b.recorded_edit
            .cmp(&a.recorded_edit)
            .then_with(|| a.path.cmp(&b.path))
    });
    info.not_captured = snapshot
        .not_captured
        .iter()
        .map(|n| NotCapturedInfo {
            path: n.path.display(),
            on_disk: on_disk_word(n.on_disk).to_owned(),
            cause: cause_word(n.cause).to_owned(),
        })
        .collect();
    info.skip_worktree_absent = snapshot.skip_worktree_absent;
    info.unmerged = snapshot.unmerged.iter().map(RepoPath::display).collect();
    info
}
```

- [ ] **Step 11: Write the `why` command**

Create `crates/supersigil-cli/src/commands/why.rs`:

```rust
//! `supersigil why <file>:<line>`: where one line of a file came from,
//! explained back to the file's creation when the evidence reaches that far.

use std::collections::BTreeMap;
use std::io::{self, Write as _};

use supersigil_anchor::lines::split_lines;
use supersigil_git::bytes::read_blobs;
use supersigil_git::changes::{FileKind, MAX_DIFF_BYTES, changed_paths, classify_bytes};
use supersigil_git::snapshot::{SnapshotOptions, snapshot_working_tree};
use supersigil_git::{ObjectId, RepoPath, ResolvedTarget, TargetSpec};
use supersigil_review::WHY_SCHEMA;
use supersigil_review::diff::diff_lines;
use supersigil_review::mapping::lines_correspond;
use supersigil_review::outcome::AttributionState;
use supersigil_review::why::{OnDiskCheck, Tristate, Why, WhyTarget, render_why, why_line};

use crate::commands::WhyArgs;
use crate::error::CliError;
use crate::evidence::edit_info;
use crate::format::{OutputFormat, escape_control, write_json};
use crate::pipeline::{
    BaseChoice, Gathered, PipelineArgs, attribute_path, claude_home, conflicts_for, evidence_info,
    gather, not_captured_reason, path_bytes, records_info, repo_path,
};
use crate::record_dir;

/// Runs `supersigil why`.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] for a malformed location, a file that
/// is not a text file in the captured working tree, or a line beyond the end
/// of the file; otherwise the errors of [`gather`].
pub fn run(args: &WhyArgs) -> Result<(), CliError> {
    let (typed, line) = parse_location(&args.location)?;
    let cwd = record_dir::canonical_checkout(None)?;
    let g = gather(&PipelineArgs {
        checkout: record_dir::canonical_checkout(args.checkout.as_deref())?,
        cwd: cwd.clone(),
        records_dir: record_dir::resolve_record_dir(args.record_dir.as_deref())?,
        claude_home: claude_home(args.claude_home.as_deref()),
        base: BaseChoice::EmptyTree,
        target: TargetSpec::WorkingTree,
        include_untracked: Vec::new(),
        paths: Vec::new(),
    })?;
    let path = repo_path(&g.worktree, &cwd, &typed)?.ok_or_else(|| {
        CliError::CommandFailed(format!("{typed} names the worktree, not a file"))
    })?;
    let (tree, blob) = target_blob(&g, &path, &typed)?;
    let bytes = read_one(&g, &blob)?;
    let text = u64::try_from(bytes.len()).is_ok_and(|len| len <= MAX_DIFF_BYTES)
        && classify_bytes(None, Some(&bytes)) == FileKind::Text;
    if !text {
        return Err(CliError::CommandFailed(format!(
            "{typed} is not a text file of at most {MAX_DIFF_BYTES} bytes; why explains lines of text"
        )));
    }
    let side = path_bytes(&g.repo, &path, None, Some((&blob, &bytes)))?;
    let attribution = attribute_path(&g, &path, &side);
    let on_disk = on_disk_check(&g, &path, side.target_worktree_form(), &bytes);
    let target_lines = split_lines(&bytes);
    let analysis = if on_disk == OnDiskCheck::Captured {
        let index = line
            .checked_sub(1)
            .filter(|index| *index < target_lines.len())
            .ok_or_else(|| {
                CliError::CommandFailed(format!(
                    "line {line} is beyond the end of {typed} ({} lines)",
                    target_lines.len()
                ))
            })?;
        let head = head_blob(&g, &path)?;
        let (differs, reason) =
            differs_from_head(head.as_deref().map_err(|reason| *reason), &bytes, index);
        let state = match &attribution {
            Ok(found) => AttributionState::Available(found),
            Err(reason) => AttributionState::Unavailable {
                reason: reason.clone(),
            },
        };
        Some(why_line(&state, index, &target_lines, &[], differs, reason))
    } else {
        None
    };
    let edits = match &attribution {
        Ok(found) => found
            .accepted
            .iter()
            .map(|e| (e.edit.id.as_str().to_owned(), edit_info(e, &g.evidence)))
            .collect(),
        Err(_) => BTreeMap::new(),
    };
    // Conflicts touching this path, whether or not attribution ran.
    let conflicts = conflicts_for(&g, &path);
    let why = Why {
        schema: WHY_SCHEMA,
        path: path.display(),
        target: WhyTarget {
            worktree: g.worktree.display().to_string(),
            tree: tree.to_string(),
            blob: Some(blob.to_string()),
            attribution_bytes: side.status.target.clone(),
        },
        records: records_info(&g),
        evidence: evidence_info(&g, conflicts.clone()),
        on_disk,
        line: analysis,
        edits,
        conflicting_edits: conflicts,
    };
    match args.format.resolve() {
        OutputFormat::Json => write_json(&why)?,
        OutputFormat::Terminal => io::stdout()
            .lock()
            .write_all(render_why(&why, escape_control).as_bytes())?,
    }
    Ok(())
}

/// Splits `<file>:<line>` at its last colon; the line is 1-based.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when there is no colon, the file is
/// empty, or the line is not a positive number.
fn parse_location(location: &str) -> Result<(String, usize), CliError> {
    let invalid = || CliError::CommandFailed(format!("expected <file>:<line>, got {location}"));
    let (file, line) = location.rsplit_once(':').ok_or_else(invalid)?;
    let line = line
        .parse::<usize>()
        .ok()
        .filter(|line| *line > 0)
        .ok_or_else(invalid)?;
    if file.is_empty() {
        return Err(invalid());
    }
    Ok((file.to_owned(), line))
}

/// The captured tree and the path's blob in it. A tracked path keeps the
/// snapshot `add -u` produced; only a path absent from it and present on
/// disk (the user named an untracked file) is captured again with that one
/// path included.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when the path is in neither capture,
/// or the git errors of the snapshot and the diff.
fn target_blob(
    g: &Gathered,
    path: &RepoPath,
    typed: &str,
) -> Result<(ObjectId, ObjectId), CliError> {
    if let Some(blob) = blob_at(g, &g.target_tree, path)? {
        return Ok((g.target_tree.clone(), blob));
    }
    let on_disk = path.to_path().is_some_and(|p| g.worktree.join(p).is_file());
    if on_disk {
        let options = SnapshotOptions {
            include_untracked: vec![path.clone()],
            pathspecs: Vec::new(),
        };
        let snapshot = snapshot_working_tree(&g.repo, &options)?;
        if let Some(blob) = blob_at(g, &snapshot.tree, path)? {
            return Ok((snapshot.tree, blob));
        }
    }
    Err(CliError::CommandFailed(format!(
        "{typed} is not in the captured working tree"
    )))
}

/// The blob `path` has in `tree`, found by diffing the empty tree against it.
///
/// # Errors
///
/// Returns the git errors of the diff.
fn blob_at(g: &Gathered, tree: &ObjectId, path: &RepoPath) -> Result<Option<ObjectId>, CliError> {
    let empty = g.repo.empty_tree()?;
    Ok(
        changed_paths(&g.repo, &empty, tree, std::slice::from_ref(path))?
            .into_iter()
            .find(|c| &c.path == path)
            .and_then(|c| c.new_blob),
    )
}

/// Reads one blob.
///
/// # Errors
///
/// Returns the git errors of the read, or [`CliError::CommandFailed`] when
/// git returns nothing for the id.
fn read_one(g: &Gathered, id: &ObjectId) -> Result<Vec<u8>, CliError> {
    read_blobs(&g.repo, std::slice::from_ref(id))?
        .remove(id)
        .ok_or_else(|| CliError::CommandFailed(format!("blob {id} could not be read")))
}

/// The path's bytes at HEAD, or why there are none.
///
/// # Errors
///
/// Returns the git errors of reading HEAD's tree and the blob.
fn head_blob(g: &Gathered, path: &RepoPath) -> Result<Result<Vec<u8>, &'static str>, CliError> {
    let ResolvedTarget::WorkingTree { head: Some(head) } = &g.range.target else {
        return Ok(Err("HEAD is unborn"));
    };
    let tree = g.repo.commit_tree(head)?;
    match blob_at(g, &tree, path)? {
        Some(id) => Ok(Ok(read_one(g, &id)?)),
        None => Ok(Err("the file is not in HEAD")),
    }
}

/// Whether target line `index` lies in a changed hunk against HEAD, with the
/// reason when that is unknown. A coarse hunk is not aligned, so a line
/// inside it may equal a HEAD line: unknown, never yes.
fn differs_from_head(
    head: Result<&[u8], &str>,
    target: &[u8],
    index: usize,
) -> (Tristate, Option<String>) {
    let head = match head {
        Ok(head) => head,
        Err(reason) => return (Tristate::Unknown, Some(reason.to_owned())),
    };
    let diff = diff_lines(head, target);
    let changed = diff
        .hunks
        .iter()
        .any(|h| (h.target_start..h.target_start + h.target_count).contains(&index));
    match (changed, diff.coarse) {
        (true, true) => (Tristate::Unknown, Some("coarse diff".to_owned())),
        (true, false) => (Tristate::Yes, None),
        (false, _) => (Tristate::No, None),
    }
}

/// Checks that the captured line is what is on disk: a path the snapshot
/// listed as not captured never is; otherwise the file on disk must equal
/// the target in worktree form (`worktree_form`, or the blob when conversion
/// failed) up to line endings. Whether those bytes map onto the blob lines
/// is attribution's concern, not this check's.
fn on_disk_check(
    g: &Gathered,
    path: &RepoPath,
    worktree_form: Option<&[u8]>,
    blob: &[u8],
) -> OnDiskCheck {
    let listed = g
        .snapshot
        .as_ref()
        .and_then(|s| s.not_captured.iter().find(|n| &n.path == path));
    if let Some(entry) = listed {
        return OnDiskCheck::NotCaptured {
            reason: not_captured_reason(entry),
        };
    }
    let expected = worktree_form.unwrap_or(blob);
    let Some(file) = path.to_path().map(|p| g.worktree.join(p)) else {
        return OnDiskCheck::NotCaptured {
            reason: "the path cannot be represented on this platform".to_owned(),
        };
    };
    match std::fs::read(&file) {
        Ok(disk) if lines_correspond(&disk, expected) => OnDiskCheck::Captured,
        Ok(_) => OnDiskCheck::NotCaptured {
            reason: "the file on disk differs from the captured target: its on-disk state was not captured, or it changed during the command".to_owned(),
        },
        Err(e) => OnDiskCheck::NotCaptured {
            reason: format!("the file cannot be read from disk: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::parse_location;

    #[test]
    fn locations_split_at_the_last_colon() {
        assert_eq!(
            parse_location("src/lib.rs:7").unwrap(),
            ("src/lib.rs".to_owned(), 7)
        );
        assert_eq!(
            parse_location(r"C:\repo\a.rs:3").unwrap(),
            (r"C:\repo\a.rs".to_owned(), 3)
        );
        parse_location("src/lib.rs").unwrap_err();
        parse_location("src/lib.rs:0").unwrap_err();
        parse_location(":3").unwrap_err();
        parse_location("a.rs:x").unwrap_err();
    }
}
```

- [ ] **Step 12: Wire the commands**

In `crates/supersigil-cli/src/commands.rs`, replace `use crate::format::OutputFormat;` with `use crate::format::{AutoFormat, OutputFormat};`, replace the module declarations and the `Command` enum with:

```rust
/// The `completions` command.
pub mod completions;
/// The `review` command.
pub mod review;
/// The `session` command group.
pub mod session;
/// The `why` command.
pub mod why;

/// Available subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Review a change with the recorded edits behind each line
    Review(ReviewArgs),
    /// Sync and inspect agent sessions recorded for this checkout
    Session(SessionArgs),
    /// Explain where one line of a file came from
    Why(WhyArgs),
}
```

and append:

```rust
/// Arguments for the `review` command.
#[derive(Debug, clap::Args)]
pub struct ReviewArgs {
    /// Base revision (default: HEAD for the working tree, the target's first parent for a commit)
    #[arg(long)]
    pub base: Option<String>,
    /// Target revision (default: the working tree)
    #[arg(long, conflicts_with = "working_tree")]
    pub target: Option<String>,
    /// Review the working tree (the default)
    #[arg(long)]
    pub working_tree: bool,
    /// Untracked file to include, relative to the current directory (repeatable)
    #[arg(long = "include-untracked", value_name = "PATH")]
    pub include_untracked: Vec<String>,
    /// Output format: `auto` prints text on a terminal and JSON otherwise
    #[arg(long, default_value = "auto")]
    pub format: AutoFormat,
    /// Checkout to review (default: current directory)
    #[arg(long)]
    pub checkout: Option<PathBuf>,
    /// Records directory (default: `$SUPERSIGIL_RECORD_DIR`, then `$XDG_DATA_HOME/supersigil/records`)
    #[arg(long)]
    pub record_dir: Option<PathBuf>,
    /// Claude home directory used for discovery (default: ~/.claude)
    #[arg(long)]
    pub claude_home: Option<PathBuf>,
    /// Limit the review to these paths, relative to the current directory
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<String>,
}

/// Arguments for the `why` command.
#[derive(Debug, clap::Args)]
pub struct WhyArgs {
    /// `<file>:<line>`: a file relative to the current directory and a 1-based line
    pub location: String,
    /// Output format: `auto` prints text on a terminal and JSON otherwise
    #[arg(long, default_value = "auto")]
    pub format: AutoFormat,
    /// Checkout the file belongs to (default: current directory)
    #[arg(long)]
    pub checkout: Option<PathBuf>,
    /// Records directory (default: `$SUPERSIGIL_RECORD_DIR`, then `$XDG_DATA_HOME/supersigil/records`)
    #[arg(long)]
    pub record_dir: Option<PathBuf>,
    /// Claude home directory used for discovery (default: ~/.claude)
    #[arg(long)]
    pub claude_home: Option<PathBuf>,
}
```

In `crates/supersigil-cli/src/lib.rs`, change the re-exports to:

```rust
pub use commands::{
    Command, CompletionsArgs, ReviewArgs, SessionArgs, SessionCommand, SessionListArgs,
    SessionShowArgs, SessionSyncArgs, WhyArgs,
};
pub use format::{AutoFormat, ColorChoice, ColorConfig, OutputFormat};
```

In `crates/supersigil-cli/src/main.rs`, replace the body of `run` with:

```rust
    match cli.command {
        Command::Completions(ref args) => supersigil_cli::commands::completions::run(args),
        Command::Review(ref args) => supersigil_cli::commands::review::run(args),
        Command::Session(ref args) => supersigil_cli::commands::session::run(args, color),
        Command::Why(ref args) => supersigil_cli::commands::why::run(args),
    }
```

- [ ] **Step 13: Run the end-to-end tests to verify they pass**

Run: `cargo nextest run -p supersigil --test cmd_review`
Expected: 20 tests pass (19 on Windows, where the clean/smudge filter test is compiled out). `review_reports_locked_record` and `review_reports_unreconciled_checkout` each take a little over two seconds (the bounded lock wait); the rest take well under a second each.

If `review_slice_fixture` fails on an outcome, print the file's JSON (`eprintln!("{lib:#}")`) and compare with design section 3 before changing an expectation: the chain from `toolu_05` is exact from its start (both hashes known, `toolu_01`'s after-hash contradicts the manual line), and every added line except the `Hello` line is inherited from before that chain.

- [ ] **Step 14: Format, lint, and run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features && cargo nextest run`
Expected: no warnings; every test passes, including plan 1's `cmd_session` and `cmd_completions` tests (the completions test covers the two new subcommands through clap).

- [ ] **Step 15: Commit**

```bash
git add crates/supersigil-cli/Cargo.toml Cargo.lock crates/supersigil-cli/src/error.rs crates/supersigil-cli/src/format.rs crates/supersigil-cli/src/mapping.rs crates/supersigil-cli/src/evidence.rs crates/supersigil-cli/src/pipeline.rs crates/supersigil-cli/src/commands.rs crates/supersigil-cli/src/commands/review.rs crates/supersigil-cli/src/commands/why.rs crates/supersigil-cli/src/lib.rs crates/supersigil-cli/src/main.rs crates/supersigil-cli/tests/cmd_review.rs crates/supersigil-session/tests/fixtures/slice.jsonl
git commit -m "feat: add review and why commands"
```

---

### Task 14: Dogfooding

**Files:** none committed. Notes go into the PR description.

**Interfaces:**
- Consumes: the `supersigil` binary from Task 13, this repository's own Claude Code sessions under `~/.claude/projects`, and its branches (`pivot`, the plan 2 branch, any `.claude/worktrees/*` worktrees).
- Produces: a short dogfooding section for the PR description (design section 5, layer 6): per chain class and per outcome, how many files and lines came out, which stops occurred, how long a review took, and every surprise, each surprise with a pointer to the JSON that shows it. Short and labeled as AI-written, per the author's PR conventions.

This pass is by hand, as plan 1's was. It checks the Review Focus list against real history, which no fixture can.

- [ ] **Step 1: Build the release binary**

Run: `cargo build --release -p supersigil`
Expected: `target/release/supersigil` exists.

- [ ] **Step 2: Review the working tree and the branch**

From the repository root, with the real records directory and Claude home:

```bash
SS=$PWD/target/release/supersigil
OUT=$(mktemp -d)
time $SS review --format json > "$OUT/worktree.json"
time $SS review --base pivot --target HEAD --format json > "$OUT/branch.json"
$SS review --base pivot --target HEAD --format terminal
```

Expected: both runs exit 0; record their wall time. The terminal summary prints the header, the unattributed summary first, then one line per file.

- [ ] **Step 3: Count outcomes, classes, and stops**

```bash
for f in "$OUT/worktree.json" "$OUT/branch.json"; do
  echo "== $f"
  jq '[.files[].hunks[].spans[] | {kind: .outcome.kind, count}] | group_by(.kind)
      | map({outcome: .[0].kind, lines: (map(.count) | add)})' "$f"
  jq '[.files[] | .attribution.chains[]?.class] | group_by(.) | map({class: .[0], chains: length})' "$f"
  jq '[.files[] | .attribution.chains[]? | select(.end.kind == "stopped") | .end.reasons[].kind]
      | group_by(.) | map({stop: .[0], count: length})' "$f"
  jq '{records: [.records[] | {id, revision, reconciled, reason}],
       unreconciled: .evidence.unreconciled_checkouts,
       unplaced: .evidence.unplaced_edits,
       limited: [.evidence.candidate_transcripts[] | select(.capture_limitations | length > 0) | .transcript],
       origins: .origins}' "$f"
done
```

Expected: every JSON parses. Write down the numbers.

- [ ] **Step 4: Check the Review Focus items against real history**

For each, find one real instance in the output or say that none occurred:

1. A file over 10 KB edited by one session in the reviewed range: its chain class should be `exact_from_base`, or a stop reason that explains why not.
2. A tab-indented file (for example under `editors/` in older history, or any Makefile): its lines are attributed with hunk text never used as bytes.
3. A deletion (an Edit with an empty `new_string`): attributed, not ambiguous, when one session made it.
4. A manual change between agent edits: a `stopped` chain end and unattributed lines, never an exact claim across it.
5. An agent-created file never staged: listed first in `scope.untracked_excluded` with `recorded_edit: true`.
6. A file removed with `rm`: its `mentions` name the command.
7. Run `$SS review` while a Claude Code session in this repository is running its stop hook, if one is active: the record is reported not reconciled, and the command still finishes.
8. `$SS why <file>:<line>` on a line an agent wrote in this session's branch, and on a line in a file marked assume-unchanged (`git update-index --assume-unchanged`, restored afterwards with `--no-assume-unchanged`).
9. A commit made in a `.claude/worktrees/*` worktree and reviewed from the main checkout: `origins` names that worktree.
10. A session started in the main checkout that edited files inside a `.claude/worktrees/*` worktree, reviewed from inside that worktree: the edit is a candidate. When the main checkout has no record yet, the one reconcile creates is associated with the main checkout (F8).
11. `$SS review -- . <dir>`: the review covers the whole worktree and the terminal summary prints the `scope:` note (F9).

- [ ] **Step 5: Write the dogfooding notes**

Draft the PR section in a scratch file (not in the repository): the counts from Step 3, the timings from Step 2, and each item of Step 4 as found, not found, or surprising, with the jq path that shows it. Every surprise that points at a defect becomes a finding for the whole-branch review, not a silent fix.

Expected: nothing is committed in this task.

---

## What Plans 3 and 4 Build on This

- **Plan 3, annotations, judgments, and the gate.** It consumes `supersigil-review`'s model and `supersigil-anchor`'s per-line outcomes, and inherits these items from plan 2:
  - *The review basis* pins a list of (record id, revision) pairs, since one review can involve several records (Task 13's `records` block already lists them), plus the tree ids, the scope, the origin associations the review used (Task 7, Task 13's `origins` block), and the conversion statuses (Task 6), so change detection can fingerprint evidence that is not in the record.
  - *The retention ref* `refs/supersigil/reviews/<basis id>` is written when the first judgment binds to a basis. Until then the target tree's objects are unreferenced and best effort; a missing object is a not-reproducible target.
  - *Gate items plan 2 already surfaces:* unattributed, unresolved, ambiguous, line-ending, and content-match-incomplete lines (the `unattributed` summary); untracked files a recorded edit wrote but the scope excluded; paths whose on-disk state was not captured; commits without an origin. Whitespace-only contributions must stay visible to decision grouping or the gate; they may not be dropped as formatting.
  - *Derive:* re-key its groups to (session, transcript, normalized absolute path) and re-derive stored sets whose algorithm version is stale during reconcile, before stored discontinuities and restores feed the history perspective and the gate.
  - *A transcript that vanishes between discovery and read* aborts reconcile in plan 2 (Task 12); plan 3's deleted-transcript availability item should report it as unavailable evidence instead.
  - *Plan 1's parked items:* grace-period cleanup of superseded documents, deleted-transcript availability, unknown timestamps.
- **Plan 4, terminal review.** The TUI renders the review model; historical patches come from `old_text` and `new_text`, never from `structuredPatch` hunks (display form). The stop hook and `init` arrive there, backed by plan 2's reconcile-on-open.
- **Later, if dogfooding asks for it:** Claude Code's file-history backups (`~/.claude/file-history`) could supply hashes for large files at checkpoints, which would turn more consistent chains into verified ones and link transcripts that no recorded hash joins today; path lineage and edit-level git associations would let a rename carry its history.

---

## Appendix: Interfaces Contract

This is the contract the tasks were written against in parallel. Each task's own **Interfaces** block is authoritative where it differs; the differences are listed under **Deviations and Decisions**.

### `supersigil-record` (Task 1)

```rust
// crates/supersigil-record/src/observations.rs
/// Editing operation recorded for an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditOperation {
    /// Claude Code's Edit tool: `old_text` replaced by `new_text`.
    Replace,
    /// Claude Code's Write tool: the whole file becomes `new_text`.
    Write,
    /// Not recorded (logs written before this field existed) or a tool
    /// anchor cannot reverse, such as MultiEdit.
    #[default]
    Unknown,
}
// new field on `Edit`, after `replace_all`:
    /// Editing operation, `Unknown` for observations recorded before it was kept.
    #[serde(default)]
    pub operation: EditOperation,
```

### `supersigil-git` (Tasks 4 to 7)

```rust
// lib.rs
pub mod bytes;     // Task 6
pub mod changes;   // Task 6
pub mod error;     // Task 4
pub mod oid;       // Task 4
pub mod origin;    // Task 7
pub mod path;      // Task 4
pub mod repo;      // Task 4
pub mod run;       // Task 4
pub mod snapshot;  // Task 5
pub mod worktree;  // Task 4
pub use error::GitError;
pub use oid::{ObjectFormat, ObjectId};
pub use path::RepoPath;
pub use repo::{Ancestry, Base, Repo, ResolvedRange, ResolvedTarget, TargetSpec, resolve_range};
pub use run::{Git, GitVersion, MIN_VERSION};

// run.rs
#[derive(Debug, Clone)]
pub struct Git { /* cwd: PathBuf, env: Vec<(OsString, OsString)> */ }
impl Git {
    pub fn new(cwd: impl Into<PathBuf>) -> Self;
    pub fn with_env(self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self;
    pub fn in_dir(&self, cwd: impl Into<PathBuf>) -> Self; // same extra env
    pub fn cwd(&self) -> &Path;
    pub fn command(&self) -> std::process::Command;        // scrubbed env + GIT_OPTIONAL_LOCKS=0 + extra env, current_dir(cwd)
    pub fn output<I, S>(&self, args: I) -> Result<Vec<u8>, GitError>
        where I: IntoIterator<Item = S>, S: AsRef<OsStr>;   // stdout; Err(Failed) on non-zero exit
    pub fn output_with_input<I, S>(&self, args: I, input: &[u8]) -> Result<Vec<u8>, GitError>
        where I: IntoIterator<Item = S>, S: AsRef<OsStr>;
    pub fn raw<I, S>(&self, args: I) -> Result<std::process::Output, GitError>
        where I: IntoIterator<Item = S>, S: AsRef<OsStr>;   // exit status is data (merge-base --is-ancestor, cat-file --filters)
    pub fn version(&self) -> Result<GitVersion, GitError>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitVersion { pub major: u32, pub minor: u32, pub patch: u32 }
impl std::fmt::Display for GitVersion {}               // "2.36.0"
pub const MIN_VERSION: GitVersion = GitVersion { major: 2, minor: 36, patch: 0 };

// error.rs
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    NotFound(#[source] std::io::Error),                  // "git was not found on PATH"
    TooOld { found: GitVersion, required: GitVersion },
    Failed { args: String, status: Option<i32>, stderr: String },
    Io { context: String, #[source] source: std::io::Error },
    Parse(String),
    NotAWorktree(PathBuf),
    Bare(PathBuf),
    UnknownRevision(String),
    Untracked { path: String, reason: String },          // --include-untracked path missing or ignored
}

// oid.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectFormat { Sha1, Sha256 }
impl ObjectFormat { pub const fn hex_len(self) -> usize; }   // 40 or 64
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(transparent)]
pub struct ObjectId(String);                              // lowercase hex of the repo's format
impl ObjectId {
    pub fn parse(text: &str, format: ObjectFormat) -> Result<Self, GitError>;
    pub fn as_str(&self) -> &str;
}
impl std::fmt::Display for ObjectId {}

// path.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepoPath(Vec<u8>);                             // repo-relative, '/'-separated, raw bytes
impl RepoPath {
    pub fn new(bytes: Vec<u8>) -> Self;
    pub fn from_utf8(path: &str) -> Self;
    pub fn as_bytes(&self) -> &[u8];
    pub fn to_str(&self) -> Option<&str>;
    pub fn display(&self) -> String;                      // lossy
    pub fn escaped(&self) -> String;                      // `\` -> `\\`, bytes outside 0x20..=0x7e -> `\xNN`; reversible
    pub fn to_path(&self) -> Option<PathBuf>;             // None when not representable on this platform
}

// repo.rs
#[derive(Debug, Clone)]
pub struct Repo { /* git: Git (cwd = root), root: PathBuf, format: ObjectFormat, version: GitVersion */ }
impl Repo {
    pub fn open(git: Git) -> Result<Self, GitError>;      // version >= MIN_VERSION, not bare, --show-toplevel, --show-object-format
    pub fn git(&self) -> &Git;
    pub fn root(&self) -> &Path;                          // as git prints it, converted to PathBuf
    pub fn format(&self) -> ObjectFormat;
    pub fn version(&self) -> GitVersion;
    pub fn resolve_commit(&self, rev: &str) -> Result<ObjectId, GitError>;  // rev-parse --verify --end-of-options <rev>^{commit}; UnknownRevision on failure
    pub fn commit_tree(&self, commit: &ObjectId) -> Result<ObjectId, GitError>;
    pub fn head(&self) -> Result<Option<ObjectId>, GitError>;             // None when unborn
    pub fn first_parent(&self, commit: &ObjectId) -> Result<Option<ObjectId>, GitError>;
    pub fn empty_tree(&self) -> Result<ObjectId, GitError>;               // mktree with empty input
    pub fn is_ancestor(&self, ancestor: &ObjectId, descendant: &ObjectId) -> Result<bool, GitError>;
    pub fn commits_in_range(&self, base: Option<&ObjectId>, target: &ObjectId) -> Result<Vec<ObjectId>, GitError>;
    pub fn config_bool(&self, key: &str) -> Result<Option<bool>, GitError>; // git config --type=bool --get; None when unset
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetSpec { WorkingTree, Commit(String) }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base { pub commit: Option<ObjectId>, pub tree: ObjectId }
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ancestry { Ancestor, NotAncestor, Unavailable }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTarget {
    WorkingTree { head: Option<ObjectId> },               // tree written later by snapshot_working_tree
    Commit { commit: ObjectId, tree: ObjectId },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRange { pub base: Base, pub target: ResolvedTarget, pub ancestry: Ancestry }
impl ResolvedRange {
    pub fn target_commit(&self) -> Option<&ObjectId>;      // the commit, or HEAD for a working tree
    pub fn commits(&self, repo: &Repo) -> Result<Vec<ObjectId>, GitError>; // commits in base..target_commit (empty-tree base: all reachable)
}
pub fn resolve_range(repo: &Repo, base: Option<&str>, target: &TargetSpec) -> Result<ResolvedRange, GitError>;

// worktree.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree { pub path: PathBuf, pub head: Option<ObjectId>, pub bare: bool, pub prunable: bool }
pub fn list_worktrees(repo: &Repo) -> Result<Vec<Worktree>, GitError>;

// snapshot.rs (Task 5)
#[derive(Debug, Clone, Default)]
pub struct SnapshotOptions { pub include_untracked: Vec<RepoPath>, pub pathspecs: Vec<RepoPath> }
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnDisk { Present, Missing }
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotCapturedCause { AssumeUnchanged, SkipWorktree, NotStaged }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCaptured { pub path: RepoPath, pub on_disk: OnDisk, pub cause: NotCapturedCause }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingTreeSnapshot {
    pub tree: ObjectId,
    pub not_captured: Vec<NotCaptured>,
    pub skip_worktree_absent: usize,
    pub unmerged: Vec<RepoPath>,
    pub untracked: Vec<RepoPath>,                         // non-ignored untracked paths NOT included
}
pub fn snapshot_working_tree(repo: &Repo, options: &SnapshotOptions) -> Result<WorkingTreeSnapshot, GitError>;

// changes.rs (Task 6)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct Mode(pub u32);                                  // octal file mode; Mode(0) for absent
impl Mode {
    pub const ABSENT: Self; pub const REGULAR: Self; pub const EXECUTABLE: Self;
    pub const SYMLINK: Self; pub const GITLINK: Self;
    pub fn is_file(self) -> bool;                           // REGULAR or EXECUTABLE
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus { Added, Deleted, Modified, TypeChanged }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: RepoPath, pub status: ChangeStatus,
    pub old_mode: Mode, pub new_mode: Mode,
    pub old_blob: Option<ObjectId>, pub new_blob: Option<ObjectId>,
}
pub fn changed_paths(repo: &Repo, base: &ObjectId, target: &ObjectId, pathspecs: &[RepoPath]) -> Result<Vec<Change>, GitError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind { Text, Binary, TooLarge, ModeOnly, Symlink, Gitlink, TypeChange, UnsupportedPath }
pub const MAX_DIFF_BYTES: u64 = 8 * 1024 * 1024;
pub fn classify_change(change: &Change) -> Option<FileKind>;         // decided without bytes: UnsupportedPath, Symlink, Gitlink, TypeChange, ModeOnly
pub fn classify_bytes(old: Option<&[u8]>, new: Option<&[u8]>) -> FileKind; // Text or Binary (NUL in first 8000 bytes, or not UTF-8)

// bytes.rs (Task 6)
pub fn blob_sizes(repo: &Repo, ids: &[ObjectId]) -> Result<BTreeMap<ObjectId, u64>, GitError>;     // cat-file --batch-check
pub fn read_blobs(repo: &Repo, ids: &[ObjectId]) -> Result<BTreeMap<ObjectId, Vec<u8>>, GitError>; // cat-file --batch
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conversion { Identical, Converted(Vec<u8>), Failed { status: Option<i32>, stderr_tail: String } }
pub fn worktree_form(repo: &Repo, path: &RepoPath, blob: &ObjectId, blob_bytes: &[u8]) -> Result<Conversion, GitError>;

// origin.rs (Task 7)
pub fn is_origin_subject(subject: &str) -> bool;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry { pub commit: ObjectId, pub subject: String }
pub fn head_reflog(repo: &Repo, worktree: &Path) -> Result<Vec<ReflogEntry>, GitError>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitOrigin { pub commit: ObjectId, pub worktrees: Vec<PathBuf> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableOrigin { pub worktree: PathBuf, pub reason: String }
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Origins { pub commits: Vec<CommitOrigin>, pub without_origin: Vec<ObjectId>, pub unavailable: Vec<UnavailableOrigin> }
impl Origins { pub fn origin_worktrees(&self) -> BTreeSet<PathBuf>; }
pub fn find_origins(repo: &Repo, worktrees: &[Worktree], commits: &[ObjectId]) -> Result<Origins, GitError>;
```

Test support shared by the git crate's integration tests: `crates/supersigil-git/tests/common/mod.rs` with `TestRepo` (created in Task 4, extended by Tasks 5 to 7):

```rust
pub struct TestRepo { pub dir: tempfile::TempDir, pub root: PathBuf }
impl TestRepo {
    pub fn new() -> Self;                                    // git init -q -b main, core.autocrlf=false
    pub fn new_sha256() -> Self;
    pub fn git(&self) -> Git;                                // Git::new(root) with the isolation env
    pub fn run(&self, args: &[&str]) -> String;              // runs git in root with the isolation env, panics on failure, returns stdout
    pub fn write(&self, path: &str, bytes: &[u8]);
    pub fn commit_all(&self, message: &str) -> String;       // add -A && commit; returns the commit id
    pub fn repo(&self) -> Repo;                              // Repo::open(self.git()).unwrap()
}
pub fn isolated(git: Git, home: &Path) -> Git;               // adds the isolation env
```

### `supersigil-anchor` (Tasks 8 to 10)

```rust
// lib.rs
pub mod align;     // Task 8
pub mod attribute; // Task 10: the entry point
pub mod combine;   // Task 10
pub mod content;   // Task 10
pub mod input;     // Task 8
pub mod lines;     // Task 8
pub mod provenance;// Task 10
pub mod result;    // Task 8 (StopReason), extended by Tasks 9 and 10
pub mod step;      // Task 8
pub mod walk;      // Task 9
pub use attribute::attribute;
pub use input::{CandidateEdit, DEFAULT_BUDGET_BYTES, Request, State, TargetKind};
pub use result::{
    BaseLineOutcome, Chain, ChainClass, ChainEnd, Conflict, Fate, LineOutcome, Origin,
    PathAttribution, PathStatus, Provenance, Reading, StopReason,
};

// lines.rs (Task 8)
pub fn line_starts(bytes: &[u8]) -> Vec<usize>;              // start offset of each line; empty input -> no lines
pub fn split_lines(bytes: &[u8]) -> Vec<&[u8]>;              // lines keep their terminators
pub fn line_of(starts: &[usize], offset: usize) -> usize;    // index of the line containing offset

// input.rs (Task 8)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State { Absent, Present(Vec<u8>) }
impl State {
    pub fn bytes(&self) -> Option<&[u8]>;
    pub fn content_id(&self) -> Option<ContentId>;
    pub fn matches(&self, recorded: &FileState) -> Option<bool>;  // None when recorded content is unknown
    pub fn byte_len(&self) -> usize;                              // 0 for Absent
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateEdit { pub record: RecordId, pub worktree: PathBuf, pub edit: Edit }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind { WorkingTree, Commit }
pub const DEFAULT_BUDGET_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Debug, Clone)]
pub struct Request {
    pub base: State, pub target: State, pub target_kind: TargetKind,
    pub reviewed_worktree: PathBuf, pub edits: Vec<CandidateEdit>, pub budget_bytes: u64,
}

// result.rs (Task 8 part)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopReason {
    OperationUnknown { edit: EventId },
    TextUnavailable { edit: EventId },
    NotLocatable { edit: EventId },
    WholeFileWrite { edit: EventId },
    ReplaceAllUnverified { edit: EventId },
    AfterHashMismatch { edit: EventId },
    NoAcceptedCandidate { edit: EventId },     // every candidate failed its own checks
    NoPredecessor,
}

// step.rs (Task 8)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement { pub before: Range<usize>, pub after: Range<usize> } // byte ranges
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward { pub after: State, pub replacements: Vec<Replacement> }
pub fn execute_forward(edit: &Edit, before: &State) -> Option<Forward>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reversal { pub before: State, pub replacements: Vec<Replacement>, pub location_choice: bool } // location_choice: one of several candidate positions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reversed { Candidates(Vec<Reversal>), Stop(StopReason) }
#[derive(Debug, Clone)]
pub struct Budget { /* remaining: u64, exhausted: bool */ }
impl Budget { pub fn new(bytes: u64) -> Self; pub fn charge(&mut self, bytes: usize) -> bool; pub fn exhausted(&self) -> bool; }
pub fn reverse(edit: &Edit, current: &State, base: &State, budget: &mut Budget) -> Reversed;

// align.rs (Task 8)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region { pub before: Range<usize>, pub after: Range<usize> } // line index ranges
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment { pub kept: Vec<(usize, usize)>, pub regions: Vec<Region> } // kept: (before line, after line)
pub fn align(before: &State, after: &State, replacements: &[Replacement]) -> Vec<Alignment>; // 1, or 2 when prefix and suffix context overlap

// walk.rs (Task 9)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedEdit { pub edit: Edit, pub worktree: PathBuf, pub records: Vec<RecordId> }
pub fn dedup(edits: Vec<CandidateEdit>) -> (Vec<AcceptedEdit>, Vec<Conflict>);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStep { pub edit: EventId, pub before: State, pub after: State, pub alignment: Alignment } // one alignment per branch
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkedChain { pub chain: Chain, pub start: State, pub steps: Vec<ChainStep> } // steps oldest first
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk { pub status: PathStatus, pub chains: Vec<WalkedChain>, pub set_aside: Vec<WalkedChain>, pub accepted: Vec<AcceptedEdit>, pub conflicts: Vec<Conflict> }
pub fn walk(request: &Request) -> Walk;

// result.rs (Task 9 part)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainClass { ExactFromBase, ExactFromStart, Consistent } // declaration order = strongest first; `min()` is the strongest
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChainEnd { Base, Stopped { reasons: Vec<StopReason> } }
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Chain { pub id: usize, pub class: ChainClass, pub head: EventId, pub edits: Vec<EventId>, pub end: ChainEnd } // edits oldest first
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathStatus { Composed, NotComposed { reasons: Vec<StopReason> }, SearchIncomplete }
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Conflict { pub edit: EventId, pub records: Vec<RecordId> }

// result.rs (Task 10 part)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Origin { Base(usize), Introduced(EventId), WhitespaceAdded(EventId), Unexplained }
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Provenance { pub origins: BTreeSet<Origin>, pub whitespace_only: BTreeSet<EventId>, pub earlier: BTreeSet<EventId> }
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fate { Replaced { edit: EventId }, CarriedTo { line: usize, via: BTreeSet<EventId> }, RemovedByWhitespace { edit: EventId }, KeptAs { line: usize } }
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Reading<T> { pub chain: usize, pub class: ChainClass, pub value: T }
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineOutcome {
    Agreed { provenance: Provenance, class: Option<ChainClass> },   // class None: no chain (provenance = {Unexplained})
    Ambiguous { readings: Vec<Reading<Provenance>> },
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BaseLineOutcome {
    Agreed { fates: BTreeSet<Fate>, class: Option<ChainClass> },    // empty fates: no fate
    Ambiguous { readings: Vec<Reading<BTreeSet<Fate>>> },
}
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct ContentMatches { pub target: Vec<BTreeSet<EventId>>, pub base: Vec<BTreeSet<EventId>> } // indexed by line
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathAttribution {
    pub status: PathStatus,
    pub chains: Vec<Chain>,
    pub set_aside: Vec<Chain>,               // alternatives that assume an unrecorded change
    pub conflicts: Vec<Conflict>,
    pub accepted: Vec<AcceptedEdit>,
    pub target: Vec<LineOutcome>,            // one per target line
    pub base: Vec<BaseLineOutcome>,          // one per base line
    pub content: ContentMatches,
}

// Test support shared by the anchor crate's integration tests:
// crates/supersigil-anchor/tests/common/mod.rs, created in Task 8 and extended by Tasks 9 and 10.
pub const WT: &str = "/work/repo";                                  // reviewed worktree in tests
pub fn replace(id: &str, transcript: &str, ordinal: u64, old: &str, new: &str) -> Edit;   // operation Replace, before/after unknown
pub fn replace_all(id: &str, transcript: &str, ordinal: u64, old: &str, new: &str) -> Edit;
pub fn create(id: &str, transcript: &str, ordinal: u64, content: &str) -> Edit;           // Write, before Absent, after known
pub fn overwrite(id: &str, transcript: &str, ordinal: u64, content: &str) -> Edit;        // Write, before unknown, after known
pub fn unknown_op(id: &str, transcript: &str, ordinal: u64, old: &str, new: &str) -> Edit; // operation Unknown
pub fn with_hashes(edit: Edit, before: &str, after: &str) -> Edit;                        // both hashes known (ContentId::of)
pub fn with_hunk_hint(edit: Edit, new_start: u32, new_lines: u32) -> Edit;                // a retained one-hunk patch used only as a position hint
pub fn in_worktree(edit: Edit, worktree: &str) -> CandidateEdit;                           // record "r1"
pub fn candidate(edit: Edit) -> CandidateEdit;                                             // in_worktree(edit, WT)
pub fn request(base: Option<&str>, target: Option<&str>, kind: TargetKind, edits: Vec<CandidateEdit>) -> Request; // None = Absent; reviewed WT; DEFAULT_BUDGET_BYTES
// Edit ids are EventId::new(id); session "s1"; checkout WT; path "f.txt"; transcript as given.

// provenance.rs (Task 10)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainProvenance { pub target: Vec<Provenance>, pub base: Vec<BTreeSet<Fate>> }
pub fn replay(chain: &WalkedChain, base_lines: usize) -> ChainProvenance;
// combine.rs (Task 10)
pub fn combine(chains: &[(&Chain, ChainProvenance)], target_lines: usize, base_lines: usize) -> (Vec<LineOutcome>, Vec<BaseLineOutcome>);
// content.rs (Task 10)
pub fn content_matches(base: &State, target: &State, edits: &[AcceptedEdit]) -> ContentMatches;
// attribute.rs (Task 10)
pub fn attribute(request: Request) -> PathAttribution;
```

### `supersigil-review` (Task 11)

```rust
// lib.rs
pub mod diff; pub mod mapping; pub mod model; pub mod outcome; pub mod summary; pub mod why;
pub use model::{REVIEW_SCHEMA, Review, WHY_SCHEMA};

// diff.rs
pub const COARSE_LINES: usize = 5_000;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk { pub base_start: usize, pub base_count: usize, pub target_start: usize, pub target_count: usize } // 0-based
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff { pub hunks: Vec<DiffHunk>, pub coarse: bool }
pub fn diff_lines(base: &[u8], target: &[u8]) -> FileDiff;

// mapping.rs
pub fn lines_correspond(blob: &[u8], attribution: &[u8]) -> bool; // identical, or same line count and each line equal after removing a trailing '\r'

// outcome.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation { Introduced, Replaced }
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AttributedEdit { pub edit: EventId, pub relation: Relation, pub classes: BTreeSet<ChainClass> }
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnattributedReason {
    NoSurvivingChain { status: PathStatus },
    GapBeforeChain,
    AttributionUnavailable { reason: String },
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Unresolved { candidates: serde_json::Value },                      // search incomplete: the chains' readings for this line, serialized as-is
    Ambiguous { readings: serde_json::Value },                         // the anchor readings, serialized as-is
    ContentMatch { relation: Relation, edits: Vec<EventId> },
    Unattributed { reason: UnattributedReason },
    Attributed { edits: Vec<AttributedEdit> },
    WhitespaceOnly { edits: Vec<EventId> },
    LineEndingChanged,
    Realigned { line: usize },                                         // the corresponding line on the other side, 0-based
}
#[derive(Debug, Clone)]
pub enum AttributionState<'a> { Available(&'a PathAttribution), Unavailable { reason: String } } // input only, not serialized
pub fn target_line_outcome(attribution: &AttributionState<'_>, line: usize, base_blob_lines: &[&[u8]], target_blob_lines: &[&[u8]]) -> (Outcome, serde_json::Value);
pub fn base_line_outcome(attribution: &AttributionState<'_>, line: usize, base_blob_lines: &[&[u8]], target_blob_lines: &[&[u8]]) -> (Outcome, serde_json::Value);
// the second element is the line's full provenance (anchor's LineOutcome or BaseLineOutcome serialized), or null

// model.rs: every JSON type, Serialize, fields in the order section 4 of the design lists
pub const REVIEW_SCHEMA: &str = "supersigil.review/1";
pub const WHY_SCHEMA: &str = "supersigil.why/1";
pub struct Review { pub schema: &'static str, pub worktree: String, pub base: BaseInfo, pub target: TargetInfo,
    pub unattributed: UnattributedSummary, pub scope: ScopeInfo, pub origins: OriginsInfo,
    pub records: Vec<RecordInfo>, pub evidence: EvidenceInfo, pub files: Vec<FileReview>,
    pub edits: BTreeMap<String, EditInfo> }
pub struct BaseInfo { pub commit: Option<String>, pub tree: String, pub ancestry: AncestryInfo }
pub enum AncestryInfo { Ancestor, NotAncestor, Unavailable }            // same variants as supersigil_git::Ancestry; the CLI converts
pub struct TargetInfo { pub kind: TargetKindInfo, pub commit: Option<String>, pub tree: String }
pub enum TargetKindInfo { WorkingTree, Commit }
pub enum FileStatus { Added, Deleted, Modified, TypeChanged }          // mirrors supersigil_git::changes::ChangeStatus
pub enum FileKindInfo { Text, Binary, TooLarge, ModeOnly, Symlink, Gitlink, TypeChange, UnsupportedPath } // mirrors FileKind
pub struct UnattributedSummary { pub files: Vec<FileCounts>, pub coarse: Vec<String>, pub attribution_unavailable: Vec<String>,
    pub not_diffed: Vec<NotDiffed>, pub untracked_with_recorded_edits: Vec<String>, pub not_captured: Vec<String> }
pub struct FileCounts { pub path: String, pub unattributed: usize, pub unresolved: usize, pub ambiguous: usize, pub line_ending: usize, pub content_match_only: usize }
pub struct NotDiffed { pub path: String, pub kind: String }
pub struct ScopeInfo { pub include_untracked: Vec<String>, pub paths: Vec<String>, pub untracked_excluded: Vec<UntrackedInfo>,
    pub not_captured: Vec<NotCapturedInfo>, pub skip_worktree_absent: usize, pub unmerged: Vec<String>,
    pub ancestry_note: Option<String> }
pub struct UntrackedInfo { pub path: String, pub recorded_edit: bool, pub include_flag: String }
pub struct NotCapturedInfo { pub path: String, pub on_disk: String, pub cause: String }
pub struct OriginsInfo { pub commits: Vec<CommitOriginInfo>, pub without_origin: Vec<String>, pub unavailable: Vec<UnavailableInfo> }
pub struct CommitOriginInfo { pub commit: String, pub worktrees: Vec<String> }
pub struct UnavailableInfo { pub worktree: String, pub reason: String }
pub struct RecordInfo { pub id: String, pub revision: Option<u64>, pub reconciled: bool, pub reason: Option<String> }
pub struct EvidenceInfo { pub candidate_transcripts: Vec<TranscriptInfo>, pub unplaced_edits: usize,
    pub unreconciled_checkouts: Vec<UnreconciledInfo>, pub conflicting_edits: Vec<Conflict> }
pub struct TranscriptInfo { pub transcript: String, pub session: String, pub capture_limitations: Vec<CaptureLimitation>, pub localized: bool /* always false */ }
pub struct UnreconciledInfo { pub checkout: String, pub transcripts: usize, pub reason: String }
pub struct FileReview { pub path: String, pub path_escaped: Option<String>, pub status: FileStatus, pub old_mode: String, pub new_mode: String, // modes as octal text, "000000" when absent
    pub old_blob: Option<String>, pub new_blob: Option<String>, pub kind: FileKindInfo, pub attribution_bytes: BytesStatus,
    pub attribution: Option<AttributionInfo>, pub coarse: bool, pub hunks: Vec<HunkReview>, pub mentions: Vec<Mention> }
pub struct BytesStatus { pub base: String, pub target: String }            // identical | converted | failed: <tail> | absent
pub struct AttributionInfo { pub status: PathStatus, pub chains: Vec<Chain>, pub set_aside: Vec<Chain>, pub unavailable: Option<String> }
pub struct HunkReview { pub base_start: usize, pub base_count: usize, pub target_start: usize, pub target_count: usize, // 1-based in JSON
    pub removed: Vec<String>, pub added: Vec<String>, pub spans: Vec<Span> }                     // line text with terminators
pub struct Span { pub side: SpanSide /* base | target */, pub start: usize /* 1-based */, pub count: usize, pub outcome: Outcome, pub provenance: Vec<serde_json::Value> }
pub struct Mention { pub command: String, pub session: String, pub turn: String, pub checkout: String, pub text: String, pub result: Option<MentionResult> }
pub struct MentionResult { pub exit: Option<i32>, pub outcome: Option<String> }
pub struct EditInfo { pub session: String, pub transcript: Option<String>, pub turn: String, pub time: String, pub worktree: String,
    pub operation: EditOperation, pub prompt: Option<PromptInfo> }
pub struct PromptInfo { pub turn: String, pub role: Role, pub excerpt: Material<String> }
pub struct FileInput<'a> { pub path: &'a RepoPathInfo, pub status: FileStatus, pub old_mode: String, pub new_mode: String,
    pub old_blob: Option<String>, pub new_blob: Option<String>, pub kind: FileKindInfo, pub bytes_status: BytesStatus,
    pub base_blob: Option<&'a [u8]>, pub target_blob: Option<&'a [u8]>, pub attribution: AttributionState<'a>, pub mentions: Vec<Mention> }
pub struct RepoPathInfo { pub display: String, pub escaped: Option<String> }
pub fn file_review(input: FileInput<'_>) -> FileReview;
pub fn unattributed_summary(files: &[FileReview], scope: &ScopeInfo) -> UnattributedSummary;

// summary.rs
pub fn render_summary(review: &Review) -> String;

// why.rs
pub struct Why { pub schema: &'static str, pub path: String, pub target: WhyTarget, pub records: Vec<RecordInfo>, pub evidence: EvidenceInfo,
    pub on_disk: OnDiskCheck, pub line: Option<WhyLine>, pub edits: BTreeMap<String, EditInfo>, pub conflicting_edits: Vec<Conflict> }
pub struct WhyTarget { pub worktree: String, pub tree: String, pub blob: Option<String>, pub attribution_bytes: String }
pub enum OnDiskCheck { Captured, NotCaptured { reason: String } }        // tag = "state"
pub struct WhyLine { pub number: usize /* 1-based */, pub text: String, pub differs_from_head: Tristate, pub status: PathStatus,
    pub chains: Vec<Chain>, pub set_aside: Vec<Chain>, pub outcome: Outcome, pub provenance: serde_json::Value }
pub enum Tristate { Yes, No, Unknown }
pub fn why_line(attribution: &AttributionState<'_>, line: usize, target_blob_lines: &[&[u8]], base_blob_lines: &[&[u8]], differs_from_head: Tristate) -> WhyLine;
pub fn render_why(why: &Why) -> String;
```

All model structs derive `Debug, Clone, PartialEq, Eq, serde::Serialize` with `#[serde(rename_all = "snake_case")]` on enums; the review crate re-declares `Ancestry`-like enums it needs rather than depending on `supersigil-git` (dependency direction: review -> anchor + record only).

### CLI (Tasks 12 and 13)

```rust
// crates/supersigil-cli/src/lib.rs gains
pub mod evidence;   // Task 13
pub mod mapping;    // Task 13
pub mod pipeline;   // Task 13
pub mod reconcile;  // Task 12

// record_dir.rs (Task 12)
pub enum Acquired<T> { Ready(T), Busy }
pub fn open_or_create_record_within(records_dir: &Path, checkout: &Path, wait: Duration) -> Result<Acquired<Store>, CliError>;

// reconcile.rs (Task 12)
pub const LOCK_WAIT: Duration = Duration::from_secs(2);
pub struct InvolvedRecord { pub store: Store, pub record_id: RecordId, pub not_reconciled: Option<String> }
pub struct UnreconciledCheckout { pub checkout: PathBuf, pub transcripts: usize, pub reason: String }
pub struct Reconciliation { pub records: Vec<InvolvedRecord>, pub unreconciled: Vec<UnreconciledCheckout> }
pub fn involved_records(records_dir: &Path, worktrees: &[PathBuf]) -> Result<Vec<Store>, CliError>;
pub fn transcripts_below(claude_home: &Path, worktree: &Path) -> std::io::Result<Vec<PathBuf>>;
pub fn reconcile(records_dir: &Path, claude_home: Option<&Path>, worktrees: &[PathBuf], wait: Duration) -> Result<Reconciliation, CliError>;

// mapping.rs (Task 13)
pub struct MappedEdit { pub worktree: PathBuf, pub path: String }
pub fn map_edit(edit: &Edit, worktree_roots: &[PathBuf], ignore_case: bool) -> Option<MappedEdit>;

// error.rs (Task 13): CliError gains `Git(#[from] supersigil_git::GitError)`
// format.rs (Task 13): `pub enum AutoFormat { Auto, Json, Terminal }` with `resolve(self) -> OutputFormat` (Auto: terminal on a TTY, JSON otherwise)
// commands.rs (Task 13): `Command::Review(ReviewArgs)`, `Command::Why(WhyArgs)`
```
