# Review Slice, Plan 1 of 3: Record Foundation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn a Claude Code transcript into an immutable, revisioned local record of observations plus derived restores and discontinuities, readable through `supersigil session sync|list|show`.

**Architecture:** Two new library crates. `supersigil-record` owns the model types for the observation and derivation layers and a file-backed store whose manifest pins committed log lengths and immutable document names, so readers never mix revisions. `supersigil-session` parses Claude Code JSONL transcripts into observations and runs incremental sync into a store. The CLI gains a `session` command group. Plans 2 (snapshots, attribution, annotations, review model, judgments) and 3 (TUI, end-to-end slice) build on these crates and do not change their public types.

**Tech Stack:** Rust 2024 edition (workspace rust-version 1.92), serde + serde_json, thiserror 2, sha2 0.11 + hex 0.4 for content ids, uuid 1 (v4) for record identity, clap 4 derive in the existing CLI, insta (yaml) and tempfile for tests, cargo nextest.

**Spec:** `docs/research/multiverse-review-design.md`, sections 3 to 5 and 8. This plan covers the observation and derivation layers, the revision protocol, Claude Code ingestion, and sync. It deliberately leaves interpretations, contributions, snapshots, and attribution to plan 2.

## Global Constraints

- Edition 2024, `rust-version = "1.92"`, workspace lints apply to every new crate (`[lints] workspace = true`): `missing_docs = "warn"` means every `pub` item gets a doc comment; clippy pedantic is on, so pure functions get `#[must_use]`, every `pub fn` returning `Result` has a `# Errors` section, and every `pub fn` that can panic has a `# Panics` section.
- New module syntax only: `module.rs` beside `module/`, never `module/mod.rs`.
- New crates are `publish = false` and version `0.14.0`; they are not added to `release-targets.json` or the publish workflow.
- No new dependency outside the license allow list in `deny.toml` (Apache-2.0, MIT, BSL-1.0, Unicode-3.0, Unlicense). sha2, hex, and uuid are MIT OR Apache-2.0.
- Observations are immutable and append-only. Nothing in this plan ever rewrites a line of an events log.
- Derivations carry `observation_revision` and `algorithm_version` and are written as new immutable documents, never edited.
- Readers read only what the manifest pins: committed byte lengths for logs, immutable file names for documents.
- `user` transcript records are not assumed to be human: `isMeta` records are `Meta`, records whose content is only tool results are `Tool`, `summary` records are `Summary`.
- Unknown record types are counted per type and reported; an incomplete final JSONL line is not consumed and is reported as `trailing_partial`.
- Paths in edits are stored relative to the checkout; an edit outside the checkout is counted and not stored.
- Timestamps are the transcript's RFC 3339 UTC strings, compared as strings; no clock is read in this plan.
- This plan runs on the `pivot` branch after plan 0 (the purge). The CLI is a clap shell with one command, `completions`, and no configuration file.
- Before finishing any task: `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features` with no warnings, and `cargo nextest run`.
- Commits: conventional commit type, no scope unless needed to disambiguate, stage files by name (never `git add -A`), and no `Co-Authored-By` or other attribution trailer; the author keeps AI attribution out of the commit history.

## Review Focus

Inputs the spec implies that a person will hit early, each pinned to a test in the owning task:

1. A transcript whose last line is cut mid-object because Claude Code is still writing it: sync must consume every complete line, report `trailing_partial: true`, and pick up the rest on the next run without duplicating events. (Task 7, `sync_resumes_after_partial_line`; Task 6, `trailing_partial_line_is_not_consumed` and `pending_tool_use_at_end_is_not_consumed`.)
2. An Edit whose `old_string` does not occur in `originalFile`, and a Write the harness reported as failed: the first gets an `Unknown` after-state, the second is not an edit at all. Neither may fabricate a hash for bytes that never existed. (Task 6, `edit_with_missing_old_string_has_unknown_after_state`, `failed_write_is_not_an_edit`.)
3. A `user` record that is only tool results, and one with `isMeta: true`: neither may become a `Human` turn or a prompt. (Task 6, `tool_result_and_meta_records_are_not_human`.)
4. Two writers, for example the stop hook and `session sync` started by hand, racing on one record: the second must fail with a lock error, a writer killed mid-transaction must not block the next one, and derivations must be computed from the manifest the transaction started from. (Task 4, `second_writer_is_blocked_while_lock_is_held`, `stale_lock_file_from_a_dead_writer_does_not_block`, `transaction_snapshot_is_pinned_to_its_starting_manifest`; Task 7 takes its input snapshot under the lock.)
5. A crash between appending log bytes and publishing the manifest: the next open must ignore the tail beyond the pinned length and the next writer must truncate it before appending. (Task 4, `tail_beyond_pinned_length_is_ignored_and_truncated`.)
6. A transcript whose session id or edit paths contain `..`: neither may reach outside the record directory or the checkout. (Task 4, `log_names_cannot_escape_the_record`, `odd_session_ids_get_safe_storage_keys`; Task 6, `traversal_path_is_outside_checkout`.)
7. A test run whose output has one passing binary and one failing one must be recorded as failed. (Task 6, `mixed_cargo_test_output_is_failed`.)

## Revisions after adversarial review

A Codex adversarial review on 2026-09-28 found ten defects in the first draft. All were confirmed and are folded in: session ids are encoded into safe storage keys and every log and document name is checked for containment (Task 4); `Material` and `Content` use adjacent tagging and the command's classification is `category`, since serde rejects internally tagged newtype variants with string payloads and the observation tag was colliding (Task 3); the writer lock is an operating-system file lock released on process death instead of a lock file's existence (Task 4); sync takes its input snapshot from the transaction it holds, not before locking (Task 7); tool results flagged as errors no longer produce edits, and a failed shell command carries `reported_error` (Task 6); test outcomes give failure precedence over an earlier success line (Task 6); edit paths are rejected when any component is `..` or a root (Task 6); record lookup errors on duplicate associations and creation happens under a records-directory lock (Task 8). The two plan 0 findings, the retained helper's dependency on a deleted package and the release workflow's calls into deleted workflows, are fixed in plan 0.

---

## File Structure

New crate `crates/supersigil-record/`:

| File | Responsibility |
|---|---|
| `Cargo.toml` | package metadata, deps: serde, serde_json, thiserror, sha2, hex, uuid |
| `src/lib.rs` | crate docs, module declarations, re-exports |
| `src/ids.rs` | `ContentId`, `SessionId`, `TurnId`, `EventId`, `RecordId`, `Revision`, `Timestamp` |
| `src/observations.rs` | `Observation` enum and its payload types, `FileState`, `Content`, `Material`, `Hunk` |
| `src/derivations.rs` | `DerivationSet`, `Restore`, `Discontinuity` |
| `src/derive.rs` | `derive(observations, revision) -> DerivationSet` |
| `src/store.rs` | `Store`, `StoreError`, lock handling, layout constants |
| `src/store/manifest.rs` | `Manifest`, `Association`, `SourceCursor`, read and publish |
| `src/store/write.rs` | `WriteTx`: append logs, put documents, set cursors, commit |
| `src/store/snapshot.rs` | `RecordSnapshot`: pinned reads, typed observation and derivation reads |
| `tests/store_protocol.rs` | revision protocol, crash recovery, concurrent writers |
| `tests/model_roundtrip.rs` | serde snapshots of every observation variant |

New crate `crates/supersigil-session/`:

| File | Responsibility |
|---|---|
| `Cargo.toml` | deps: serde, serde_json, thiserror, supersigil-record |
| `src/lib.rs` | crate docs, module declarations, re-exports |
| `src/claude_code.rs` | `parse_transcript`, `ParseOutcome`, raw record structs |
| `src/claude_code/content.rs` | after-state computation for Edit, Write, MultiEdit |
| `src/discover.rs` | `encode_project_dir`, `discover_transcripts` |
| `src/sync.rs` | `sync`, `SyncReport`, `SyncError` |
| `tests/fixtures/slice.jsonl` | the synthetic session used by every plan |
| `tests/parse_fixture.rs` | parser behavior on the fixture |
| `tests/sync_fixture.rs` | incremental sync and derivations on the fixture |

Modified in `crates/supersigil-cli/`:

| File | Change |
|---|---|
| `Cargo.toml` | add `supersigil-record`, `supersigil-session` |
| `src/commands.rs` | `Command::Session(SessionArgs)`, `SessionCommand`, arg structs |
| `src/commands/session.rs` | `run` for `sync`, `list`, `show` |
| `src/record_dir.rs` | record directory resolution and record lookup by checkout |
| `src/error.rs` | `CliError::Record`, `CliError::Session` |
| `src/lib.rs` | re-exports |
| `src/main.rs` | add the `Session` arm to the single `match` |
| `tests/cmd_session.rs` | end-to-end CLI tests with the fixture |

Workspace `Cargo.toml`: two members, workspace deps for the two crates and for sha2, hex, uuid.

---

### Task 1: Scaffold the two crates

**Files:**
- Modify: `Cargo.toml` (workspace members and dependencies)
- Create: `crates/supersigil-record/Cargo.toml`, `crates/supersigil-record/src/lib.rs`
- Create: `crates/supersigil-session/Cargo.toml`, `crates/supersigil-session/src/lib.rs`

**Interfaces:**
- Produces: two empty library crates that build and lint clean; workspace dependency entries `supersigil-record`, `supersigil-session`, `sha2`, `hex`, `uuid`.

- [ ] **Step 1: Add workspace members and dependencies**

In the root `Cargo.toml`, change `members = ["crates/supersigil-cli"]` to:

```toml
members = [
    "crates/supersigil-cli",
    "crates/supersigil-record",
    "crates/supersigil-session",
]
```

Add to the end of `[workspace.dependencies]`, after the `proptest` line:

```toml
supersigil-record = { path = "crates/supersigil-record", version = "=0.14.0" }
supersigil-session = { path = "crates/supersigil-session", version = "=0.14.0" }
sha2 = "0.11.0"
hex = "0.4.3"
uuid = { version = "1.26.1", features = ["v4"] }
```

- [ ] **Step 2: Create the record crate**

`crates/supersigil-record/Cargo.toml`:

```toml
[package]
name = "supersigil-record"
version = "0.14.0"
description = "Local, revisioned record of development history for supersigil review"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
publish = false

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
sha2.workspace = true
hex.workspace = true
uuid.workspace = true

[dev-dependencies]
insta.workspace = true
tempfile.workspace = true

[lints]
workspace = true
```

`crates/supersigil-record/src/lib.rs`:

```rust
//! Local, revisioned record of development history.
//!
//! The record has layers with different rules. Observations are immutable
//! events appended to per-session logs. Derivations are rebuildable findings
//! computed from observations and written as immutable documents tagged with
//! the observation revision and algorithm version that produced them. Later
//! plans add interpretations and contributions on the same store.
//!
//! Readers only ever see what the manifest pins, so a reader never mixes two
//! revisions even while a writer is appending.
```

- [ ] **Step 3: Create the session crate**

`crates/supersigil-session/Cargo.toml`:

```toml
[package]
name = "supersigil-session"
version = "0.14.0"
description = "Agent session transcript sources for the supersigil record"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
publish = false

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
supersigil-record.workspace = true

[dev-dependencies]
insta.workspace = true
tempfile.workspace = true

[lints]
workspace = true
```

`crates/supersigil-session/src/lib.rs`:

```rust
//! Transcript sources that turn agent sessions into record observations.
//!
//! Claude Code is the first source. A source parses a transcript tolerantly,
//! classifies every record, and emits observations; sync appends them to a
//! record incrementally from a per-transcript cursor.
```

- [ ] **Step 4: Build and lint**

Run: `cargo build -p supersigil-record -p supersigil-session && cargo clippy -p supersigil-record -p supersigil-session --all-targets --all-features`
Expected: both build; no warnings.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/supersigil-record/Cargo.toml crates/supersigil-record/src/lib.rs crates/supersigil-session/Cargo.toml crates/supersigil-session/src/lib.rs
git commit -m "feat: scaffold record and session crates"
```

---

### Task 2: Identifiers, content ids, and timestamps

**Files:**
- Create: `crates/supersigil-record/src/ids.rs`
- Modify: `crates/supersigil-record/src/lib.rs`
- Test: inline `#[cfg(test)]` in `ids.rs`

**Interfaces:**
- Produces:
  - `ContentId::of(bytes: &[u8]) -> ContentId`, string form `sha256:<64 hex>`; `as_str()`.
  - `SessionId(String)`, `TurnId(String)`: `new(impl Into<String>)`, `as_str()`.
  - `EventId::derive(kind: &str, session: &SessionId, source_id: &str) -> EventId`, string form `<kind>:<16 hex>`; `as_str()`.
  - `RecordId::generate() -> RecordId` (uuid v4 string); `as_str()`.
  - `Revision(u64)`: `ZERO`, `next()`, `get()`.
  - `Timestamp(String)`: `new(impl Into<String>)`, `as_str()`, derives `Ord` on the string.

- [ ] **Step 1: Write the failing tests**

Append to a new file `crates/supersigil-record/src/ids.rs` only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_id_is_sha256_hex_with_prefix() {
        let id = ContentId::of(b"hello\n");
        assert_eq!(
            id.as_str(),
            "sha256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    #[test]
    fn content_id_distinguishes_empty_from_absent_by_being_a_value() {
        let empty = ContentId::of(b"");
        assert_eq!(
            empty.as_str(),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn event_id_is_stable_for_same_inputs() {
        let session = SessionId::new("s1");
        let a = EventId::derive("edit", &session, "toolu_01");
        let b = EventId::derive("edit", &session, "toolu_01");
        assert_eq!(a, b);
        assert!(a.as_str().starts_with("edit:"));
        assert_eq!(a.as_str().len(), "edit:".len() + 16);
    }

    #[test]
    fn event_id_differs_by_kind_session_and_source() {
        let s1 = SessionId::new("s1");
        let s2 = SessionId::new("s2");
        let base = EventId::derive("edit", &s1, "toolu_01");
        assert_ne!(base, EventId::derive("command", &s1, "toolu_01"));
        assert_ne!(base, EventId::derive("edit", &s2, "toolu_01"));
        assert_ne!(base, EventId::derive("edit", &s1, "toolu_02"));
    }

    #[test]
    fn revision_increments() {
        assert_eq!(Revision::ZERO.next().get(), 1);
        assert_eq!(Revision::ZERO.next().next().get(), 2);
    }

    #[test]
    fn timestamps_order_lexicographically() {
        let earlier = Timestamp::new("2026-09-28T10:00:00.000Z");
        let later = Timestamp::new("2026-09-28T10:00:05.000Z");
        assert!(earlier < later);
    }

    #[test]
    fn record_id_is_uuid_shaped_and_unique() {
        let a = RecordId::generate();
        let b = RecordId::generate();
        assert_ne!(a, b);
        assert_eq!(a.as_str().len(), 36);
    }
}
```

Add `pub mod ids;` to `lib.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-record`
Expected: compile error, `ContentId` not found.

- [ ] **Step 3: Implement the types**

Prepend to `crates/supersigil-record/src/ids.rs` above the test module:

```rust
//! Identifiers shared by every layer of the record.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Hash of file content. Absence of a file is never represented by a content
/// id; see [`crate::observations::FileState`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentId(String);

impl ContentId {
    /// Hashes `bytes` with SHA-256.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        Self(format!("sha256:{}", hex::encode(digest)))
    }

    /// The `sha256:<hex>` form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wraps an existing identifier string.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// The identifier as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(
    /// Identifier of a session, taken from the source (Claude Code's `sessionId`).
    SessionId
);
string_id!(
    /// Identifier of a turn, taken from the source (Claude Code's record `uuid`).
    TurnId
);
string_id!(
    /// Identifier of an edit or command event, derived from its source id.
    EventId
);
string_id!(
    /// Identifier of a record (one per repository association set).
    RecordId
);
string_id!(
    /// RFC 3339 UTC timestamp string. Ordering is lexicographic, which is
    /// correct for the fixed-width form Claude Code writes.
    Timestamp
);

impl EventId {
    /// Derives a stable event id from the event kind, its session, and the
    /// source's own id for it (for example a `tool_use` id). Re-syncing the
    /// same transcript yields the same ids.
    #[must_use]
    pub fn derive(kind: &str, session: &SessionId, source_id: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(kind.as_bytes());
        hasher.update(b"\0");
        hasher.update(session.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(source_id.as_bytes());
        let digest = hasher.finalize();
        Self(format!("{kind}:{}", hex::encode(&digest[..8])))
    }
}

impl RecordId {
    /// Generates a fresh random record id.
    #[must_use]
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

/// Monotonic revision number of a record manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    /// The revision of an empty record before any commit.
    pub const ZERO: Self = Self(0);

    /// The following revision.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }

    /// The numeric value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
```

In `lib.rs` add re-exports:

```rust
pub mod ids;

pub use ids::{ContentId, EventId, RecordId, Revision, SessionId, Timestamp, TurnId};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-record && cargo clippy -p supersigil-record --all-targets --all-features`
Expected: 7 tests pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/supersigil-record/src/ids.rs crates/supersigil-record/src/lib.rs
git commit -m "feat: add record identifiers and content ids"
```

---
### Task 3: Observation types

**Files:**
- Create: `crates/supersigil-record/src/observations.rs`
- Modify: `crates/supersigil-record/src/lib.rs`
- Test: `crates/supersigil-record/tests/model_roundtrip.rs`

**Interfaces:**
- Consumes: ids from Task 2.
- Produces (all `Serialize + Deserialize + Clone + PartialEq + Eq + Debug`):
  - `Observation` enum, externally tagged by `kind` (`session_start`, `session_end`, `turn`, `edit`, `command`), with `fn session(&self) -> &SessionId`.
  - `SessionStart { session, source: Source, source_ids: BTreeMap<String, String>, checkout: PathBuf, branch: Option<String>, time: Timestamp }`, `Source::ClaudeCode`.
  - `SessionEnd { session, time, reason: EndReason }`, `EndReason::{TranscriptEnded, Unknown}`.
  - `Turn { id: TurnId, session, parent: Option<TurnId>, role: Role, time, sidechain: bool, agent_id: Option<String>, excerpt: Material<String>, source_ordinal: u64 }`, `Role::{Human, Agent, Tool, Meta, Summary}`.
  - `Edit { id: EventId, turn: TurnId, session, path: PathBuf, before: FileState, after: FileState, patch: Material<Vec<Hunk>>, old_text: Material<String>, new_text: Material<String>, replace_all: bool, checkout: PathBuf, time, source_ordinal: u64 }` with `before_content()` and `after_content()` returning `Option<&ContentId>`.
  - `FileState::{Absent, Present { content: Content }}` with `FileState::known(ContentId)` and `FileState::unknown()`; `Content::{Known(ContentId), Unknown}`.
  - `Material<T>::{Retained(T), Withheld { policy: String }, Unavailable { reason: String }}` with `retained(&self) -> Option<&T>`.
  - `Hunk { old_start: u32, old_lines: u32, new_start: u32, new_lines: u32, lines: Vec<String> }`.
  - `Command { id: EventId, turn: TurnId, session, cmd: String, exit: Option<i32>, stdout_tail: Material<String>, stderr_tail: Material<String>, category: CommandCategory, reported_error: bool, outcome: Option<Outcome>, started: Timestamp, ended: Option<Timestamp>, checkout: PathBuf, source_ordinal: u64 }`, `CommandCategory::{TestRun, Build, Git, Other}`, `Outcome::{Passed, Failed}`. `reported_error` is the harness's own error flag on the tool result.
  - JSON shapes: `Observation` is internally tagged by `kind`, so no payload struct may have a field named `kind` (that is why a command has a `category`). `Material` and `Content` are adjacently tagged, because serde cannot serialize an internally tagged newtype variant whose payload is a string or a sequence: `Material::Retained("x")` is `{"state":"retained","value":"x"}`, `Material::Withheld { policy }` is `{"state":"withheld","value":{"policy":"..."}}`, `Content::Known(id)` is `{"kind":"known","id":"sha256:..."}`, and `Content::Unknown` is `{"kind":"unknown"}`.

The design document lists `event_ids` on `Turn`; this plan omits it because an edit already names its turn and the log is append-only, so a turn cannot be updated after its events arrive.

- [ ] **Step 1: Write the failing round-trip test**

`crates/supersigil-record/tests/model_roundtrip.rs`:

```rust
//! Serialization shape of every observation variant is pinned by snapshot.

use std::collections::BTreeMap;
use std::path::PathBuf;

use supersigil_record::observations::{
    Command, CommandCategory, Content, Edit, EndReason, FileState, Hunk, Material, Observation,
    Outcome, Role, SessionEnd, SessionStart, Source, Turn,
};
use supersigil_record::{ContentId, EventId, SessionId, Timestamp, TurnId};

fn sample() -> Vec<Observation> {
    let session = SessionId::new("11111111-1111-4111-8111-111111111111");
    let checkout = PathBuf::from("/work/repo");
    vec![
        Observation::SessionStart(SessionStart {
            session: session.clone(),
            source: Source::ClaudeCode,
            source_ids: BTreeMap::from([("transcript".to_owned(), "slice.jsonl".to_owned())]),
            checkout: checkout.clone(),
            branch: Some("main".to_owned()),
            time: Timestamp::new("2026-09-28T10:00:00.000Z"),
        }),
        Observation::Turn(Turn {
            id: TurnId::new("u1"),
            session: session.clone(),
            parent: None,
            role: Role::Human,
            time: Timestamp::new("2026-09-28T10:00:00.000Z"),
            sidechain: false,
            agent_id: None,
            excerpt: Material::Retained("Add a greeting function and clean up.".to_owned()),
            source_ordinal: 0,
        }),
        Observation::Edit(Edit {
            id: EventId::derive("edit", &session, "toolu_01"),
            turn: TurnId::new("a1"),
            session: session.clone(),
            path: PathBuf::from("src/lib.rs"),
            before: FileState::known(ContentId::of(b"old\n")),
            after: FileState::Present { content: Content::Unknown },
            patch: Material::Retained(vec![Hunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: vec!["-old".to_owned(), "+new".to_owned()],
            }]),
            old_text: Material::Retained("old\n".to_owned()),
            new_text: Material::Withheld { policy: "capture.edit_text".to_owned() },
            replace_all: false,
            checkout: checkout.clone(),
            time: Timestamp::new("2026-09-28T10:00:06.000Z"),
            source_ordinal: 2,
        }),
        Observation::Command(Command {
            id: EventId::derive("command", &session, "toolu_07"),
            turn: TurnId::new("a7"),
            session: session.clone(),
            cmd: "cargo nextest run".to_owned(),
            exit: None,
            stdout_tail: Material::Retained("3 tests run: 3 passed".to_owned()),
            stderr_tail: Material::Unavailable { reason: "not captured".to_owned() },
            category: CommandCategory::TestRun,
            reported_error: false,
            outcome: Some(Outcome::Passed),
            started: Timestamp::new("2026-09-28T10:01:00.000Z"),
            ended: Some(Timestamp::new("2026-09-28T10:01:02.000Z")),
            checkout: checkout.clone(),
            source_ordinal: 14,
        }),
        Observation::SessionEnd(SessionEnd {
            session,
            time: Timestamp::new("2026-09-28T10:02:00.000Z"),
            reason: EndReason::Unknown,
        }),
    ]
}

#[test]
fn observation_json_shape_is_stable() {
    insta::assert_yaml_snapshot!(sample());
}

#[test]
fn observations_round_trip_through_json_lines() {
    let original = sample();
    let lines: Vec<String> = original
        .iter()
        .map(|o| serde_json::to_string(o).unwrap())
        .collect();
    let parsed: Vec<Observation> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(parsed, original);
    assert_eq!(parsed[2].session().as_str(), "11111111-1111-4111-8111-111111111111");
}

#[test]
fn every_material_and_content_shape_serializes() {
    // Internally tagged newtype variants cannot carry strings or sequences;
    // these are the payloads that must keep working.
    let cases: Vec<serde_json::Value> = vec![
        serde_json::to_value(Material::Retained("text".to_owned())).unwrap(),
        serde_json::to_value(Material::Retained(vec![Hunk {
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: 1,
            lines: vec!["+x".to_owned()],
        }]))
        .unwrap(),
        serde_json::to_value(Material::<String>::Withheld { policy: "p".to_owned() }).unwrap(),
        serde_json::to_value(Content::Known(ContentId::of(b"x"))).unwrap(),
        serde_json::to_value(Content::Unknown).unwrap(),
    ];
    assert_eq!(cases[0]["state"], "retained");
    assert_eq!(cases[0]["value"], "text");
    assert_eq!(cases[1]["value"][0]["lines"][0], "+x");
    assert_eq!(cases[2]["value"]["policy"], "p");
    assert_eq!(cases[3]["kind"], "known");
    assert!(cases[3]["id"].as_str().unwrap().starts_with("sha256:"));
    assert_eq!(cases[4]["kind"], "unknown");
    let command_json = serde_json::to_value(&sample()[3]).unwrap();
    assert_eq!(command_json["kind"], "command");
    assert_eq!(command_json["category"], "test_run");
}

#[test]
fn file_state_helpers_distinguish_absent_unknown_and_known() {
    let known = FileState::known(ContentId::of(b"x"));
    assert!(matches!(known, FileState::Present { content: Content::Known(_) }));
    assert!(matches!(FileState::unknown(), FileState::Present { content: Content::Unknown }));
    assert!(matches!(FileState::Absent, FileState::Absent));
    let retained = Material::Retained(1);
    assert_eq!(retained.retained(), Some(&1));
    let withheld: Material<i32> = Material::Withheld { policy: "p".to_owned() };
    assert_eq!(withheld.retained(), None);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-record --test model_roundtrip`
Expected: compile error, module `observations` not found.

- [ ] **Step 3: Implement the types**

`crates/supersigil-record/src/observations.rs`:

```rust
//! Observation layer: immutable events appended to a per-session log.
//!
//! Nothing here is ever rewritten. A session's end is its own event, and a
//! turn does not list its events because they arrive after it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{ContentId, EventId, SessionId, Timestamp, TurnId};

/// One immutable event in a session's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Observation {
    /// A session was first seen.
    SessionStart(SessionStart),
    /// A session ended, as far as the source can tell.
    SessionEnd(SessionEnd),
    /// A conversation turn.
    Turn(Turn),
    /// A file edit made through the agent's editing tools.
    Edit(Edit),
    /// A shell command the agent ran.
    Command(Command),
}

impl Observation {
    /// The session this event belongs to.
    #[must_use]
    pub fn session(&self) -> &SessionId {
        match self {
            Self::SessionStart(s) => &s.session,
            Self::SessionEnd(s) => &s.session,
            Self::Turn(t) => &t.session,
            Self::Edit(e) => &e.session,
            Self::Command(c) => &c.session,
        }
    }
}

/// Which agent harness produced a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Claude Code JSONL transcripts.
    ClaudeCode,
}

/// First observation of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStart {
    /// Session identifier from the source.
    pub session: SessionId,
    /// Producing harness.
    pub source: Source,
    /// Source-native identifiers, for example the transcript file name.
    pub source_ids: BTreeMap<String, String>,
    /// Checkout the session worked in.
    pub checkout: PathBuf,
    /// Branch name reported by the source, if any.
    pub branch: Option<String>,
    /// Time of the first record.
    pub time: Timestamp,
}

/// Why a session is considered ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// The source marked the transcript as finished.
    TranscriptEnded,
    /// The source gave no reason.
    Unknown,
}

/// Last observation of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEnd {
    /// Session identifier.
    pub session: SessionId,
    /// Time of the end.
    pub time: Timestamp,
    /// Why the session ended.
    pub reason: EndReason,
}

/// Who or what authored a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A person typed it.
    Human,
    /// The agent wrote it.
    Agent,
    /// Tool results only; not human intent.
    Tool,
    /// Harness metadata presented as a user message.
    Meta,
    /// A compaction summary.
    Summary,
}

/// Evidence that may or may not have been kept.
///
/// Adjacently tagged: an internally tagged newtype variant cannot carry a
/// string or a sequence, and retained text and patches are exactly that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum Material<T> {
    /// The material is stored.
    Retained(T),
    /// A policy withheld it; the policy name says which.
    Withheld {
        /// Name of the policy that withheld it.
        policy: String,
    },
    /// The source did not provide it.
    Unavailable {
        /// Why it is missing.
        reason: String,
    },
}

impl<T> Material<T> {
    /// The retained value, if any.
    #[must_use]
    pub fn retained(&self) -> Option<&T> {
        match self {
            Self::Retained(value) => Some(value),
            Self::Withheld { .. } | Self::Unavailable { .. } => None,
        }
    }
}

/// A conversation turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    /// Turn identifier from the source.
    pub id: TurnId,
    /// Session identifier.
    pub session: SessionId,
    /// The turn this one descends from in the conversation tree.
    pub parent: Option<TurnId>,
    /// Author classification.
    pub role: Role,
    /// Time of the record.
    pub time: Timestamp,
    /// Whether the source marked this turn as a sidechain (subagent).
    pub sidechain: bool,
    /// Subagent identifier when inside a subagent.
    pub agent_id: Option<String>,
    /// Text of the turn, subject to the capture policy.
    pub excerpt: Material<String>,
    /// Position of the record in the transcript.
    pub source_ordinal: u64,
}

/// Content of a file that exists. Adjacently tagged for the same reason as
/// [`Material`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Content {
    /// The content hash is known.
    Known(ContentId),
    /// The file existed but its content is not known.
    Unknown,
}

/// State of a file at one side of an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FileState {
    /// The file did not exist.
    Absent,
    /// The file existed.
    Present {
        /// What is known about its content.
        content: Content,
    },
}

impl FileState {
    /// A present file with a known content hash.
    #[must_use]
    pub const fn known(content: ContentId) -> Self {
        Self::Present { content: Content::Known(content) }
    }

    /// A present file whose content is not known.
    #[must_use]
    pub const fn unknown() -> Self {
        Self::Present { content: Content::Unknown }
    }

    /// The known content hash, if the file existed and the hash is known.
    #[must_use]
    pub fn content_id(&self) -> Option<&ContentId> {
        match self {
            Self::Present { content: Content::Known(id) } => Some(id),
            Self::Present { content: Content::Unknown } | Self::Absent => None,
        }
    }
}

/// One hunk of a structured patch, in unified-diff terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// First line of the hunk in the old file (1-based).
    pub old_start: u32,
    /// Number of old lines covered.
    pub old_lines: u32,
    /// First line of the hunk in the new file (1-based).
    pub new_start: u32,
    /// Number of new lines covered.
    pub new_lines: u32,
    /// Lines with a leading ` `, `-`, or `+`.
    pub lines: Vec<String>,
}

/// A file edit made through the agent's editing tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    /// Stable event id derived from the source's tool-use id.
    pub id: EventId,
    /// Turn that issued the edit.
    pub turn: TurnId,
    /// Session identifier.
    pub session: SessionId,
    /// Path relative to the checkout.
    pub path: PathBuf,
    /// File state before the edit.
    pub before: FileState,
    /// File state after the edit.
    pub after: FileState,
    /// Structured patch, if retained.
    pub patch: Material<Vec<Hunk>>,
    /// Text that was replaced, if retained.
    pub old_text: Material<String>,
    /// Replacement text, if retained.
    pub new_text: Material<String>,
    /// Whether every occurrence was replaced.
    pub replace_all: bool,
    /// Checkout the edit happened in.
    pub checkout: PathBuf,
    /// Time the result was recorded.
    pub time: Timestamp,
    /// Position of the issuing record in the transcript.
    pub source_ordinal: u64,
}

impl Edit {
    /// Known content hash before the edit.
    #[must_use]
    pub fn before_content(&self) -> Option<&ContentId> {
        self.before.content_id()
    }

    /// Known content hash after the edit.
    #[must_use]
    pub fn after_content(&self) -> Option<&ContentId> {
        self.after.content_id()
    }
}

/// Coarse classification of a shell command. Named `category` on the
/// command because `kind` is the observation's own tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandCategory {
    /// Runs tests.
    TestRun,
    /// Builds or checks.
    Build,
    /// Invokes git.
    Git,
    /// Anything else.
    Other,
}

/// Result of a command as far as the source reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Reported success.
    Passed,
    /// Reported failure.
    Failed,
}

/// A shell command the agent ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    /// Stable event id derived from the source's tool-use id.
    pub id: EventId,
    /// Turn that issued the command.
    pub turn: TurnId,
    /// Session identifier.
    pub session: SessionId,
    /// The command line.
    pub cmd: String,
    /// Exit code when the source reports one.
    pub exit: Option<i32>,
    /// Tail of standard output, if retained.
    pub stdout_tail: Material<String>,
    /// Tail of standard error, if retained.
    pub stderr_tail: Material<String>,
    /// Classification.
    pub category: CommandCategory,
    /// Whether the harness flagged the tool result as an error (for a shell
    /// command, normally a non-zero exit).
    pub reported_error: bool,
    /// Outcome parsed from the output, if determinable.
    pub outcome: Option<Outcome>,
    /// Time the command was issued.
    pub started: Timestamp,
    /// Time the result was recorded, if seen.
    pub ended: Option<Timestamp>,
    /// Checkout the command ran in.
    pub checkout: PathBuf,
    /// Position of the issuing record in the transcript.
    pub source_ordinal: u64,
}
```

In `lib.rs` add `pub mod observations;` and re-export `pub use observations::Observation;`.

- [ ] **Step 4: Run the tests, accept the snapshot, inspect it**

Run: `INSTA_UPDATE=always cargo nextest run -p supersigil-record --test model_roundtrip`
Expected: 3 tests pass; a file `crates/supersigil-record/tests/snapshots/model_roundtrip__observation_json_shape_is_stable.snap` appears.

Open the snapshot and confirm: the first item has `kind: session_start`; the edit's `before` reads `state: present` with a nested `content` of `kind: known` and an `id` starting with `sha256:`; the edit's `after` has `content` of `kind: unknown`; `old_text` reads `state: retained` with `value: "old\n"`; `new_text` reads `state: withheld` with a `value` holding `policy: capture.edit_text`; the command item has `kind: command` and `category: test_run`, and its `stderr_tail` reads `state: unavailable`. If a field is missing or named differently, fix the type, not the snapshot.

Run again without the env var: `cargo nextest run -p supersigil-record && cargo clippy -p supersigil-record --all-targets --all-features`
Expected: 4 tests pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/supersigil-record/src/observations.rs crates/supersigil-record/src/lib.rs crates/supersigil-record/tests/model_roundtrip.rs crates/supersigil-record/tests/snapshots/model_roundtrip__observation_json_shape_is_stable.snap
git commit -m "feat: add observation layer types"
```

---
### Task 4: The store and its revision protocol

**Files:**
- Create: `crates/supersigil-record/src/store.rs`, `crates/supersigil-record/src/store/manifest.rs`, `crates/supersigil-record/src/store/write.rs`, `crates/supersigil-record/src/store/snapshot.rs`
- Modify: `crates/supersigil-record/src/lib.rs`
- Test: `crates/supersigil-record/tests/store_protocol.rs`

**Interfaces:**
- Consumes: `RecordId`, `Revision`, `SessionId`, `Observation`.
- Produces:
  - `Store::create(root: &Path, association: Association) -> Result<Store, StoreError>`, `Store::create_with_id(root, record_id: RecordId, association)`, `Store::open(root) -> Result<Store, StoreError>`, `root()`, `manifest() -> Result<Manifest>`, `snapshot() -> Result<RecordSnapshot>`, `begin() -> Result<WriteTx<'_>>`.
  - `Manifest { record_id, schema_version, revision, associations: Vec<Association>, logs: BTreeMap<String, u64>, documents: BTreeMap<String, String>, cursors: BTreeMap<String, SourceCursor> }`, `Association { checkout: PathBuf }`, `SourceCursor { offset: u64, next_ordinal: u64, session: Option<SessionId> }`, `SCHEMA_VERSION: u32 = 1`.
  - `WriteTx::manifest()`, `snapshot(&self) -> RecordSnapshot` (pinned to the manifest the transaction started from; the transaction's own appends are invisible through it), `append_log(log: &str, lines: &[&[u8]])`, `append_observations(&[Observation])`, `put_document(logical: &str, bytes: &[u8])`, `set_cursor(key: &str, cursor: SourceCursor)`, `add_association(Association)`, `commit(self) -> Result<Revision>`. Dropping without commit releases the lock and leaves the manifest untouched. The lock is an exclusive operating-system lock on `write.lock` taken with `File::try_lock`, so the kernel releases it when a writer dies; the file itself stays.
  - `storage_key(&SessionId) -> String` percent-encodes every byte outside `A-Z a-z 0-9 _ -`, and `decode_storage_key(&str) -> SessionId` inverts it. Every log and document name passes `validate_name`: relative, no `..`, no root, no empty or `.` component.
  - `RecordSnapshot::manifest()`, `revision()`, `read_log(log) -> Result<Vec<Vec<u8>>>`, `read_document(logical) -> Result<Option<Vec<u8>>>`, `sessions() -> Vec<SessionId>`, `observations(&SessionId) -> Result<Vec<Observation>>`.
  - `observations_log(&SessionId) -> String` = `observations/<storage key>/events.jsonl`.
  - `StoreError::{Io, Json, NotARecord, AlreadyExists, Locked, Conflict, Corrupt, InvalidName}`.

Protocol, from the design: the manifest pins a committed byte length per log and an immutable file name per logical document. Readers load one manifest and read only what it pins. A writer holds an exclusive operating-system lock on `write.lock`, released by the kernel if the process dies, truncates any tail beyond the pinned length before appending, writes new documents under revision-specific names, fsyncs every touched file, re-reads the manifest to check the expected revision, then publishes the new manifest by rename. No name that comes from a transcript is ever used as a path component without encoding, and every name is checked for containment before a file is opened.

- [ ] **Step 1: Write the failing protocol tests**

`crates/supersigil-record/tests/store_protocol.rs`:

```rust
//! The manifest pins exactly one revision; readers never mix two.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

use supersigil_record::store::{
    Association, SourceCursor, Store, StoreError, observations_log, storage_key,
};
use supersigil_record::{Revision, SessionId};

fn assoc() -> Association {
    Association { checkout: PathBuf::from("/work/repo") }
}

fn session() -> SessionId {
    SessionId::new("s1")
}

#[test]
fn create_then_open_keeps_identity_at_revision_zero() {
    let dir = tempfile::tempdir().unwrap();
    let created = Store::create(dir.path(), assoc()).unwrap();
    let id = created.manifest().unwrap().record_id;
    let opened = Store::open(dir.path()).unwrap();
    let manifest = opened.manifest().unwrap();
    assert_eq!(manifest.record_id, id);
    assert_eq!(manifest.revision, Revision::ZERO);
    assert_eq!(manifest.associations, vec![assoc()]);
    assert!(matches!(
        Store::create(dir.path(), assoc()),
        Err(StoreError::AlreadyExists(_))
    ));
    assert!(matches!(
        Store::open(&dir.path().join("nope")),
        Err(StoreError::NotARecord(_))
    ));
}

#[test]
fn commit_publishes_new_revision_and_pins_log_length() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());

    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"one", b"two"]).unwrap();
    let rev = tx.commit().unwrap();
    assert_eq!(rev, Revision::ZERO.next());

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.revision(), rev);
    assert_eq!(snapshot.manifest().logs[&log], 8);
    assert_eq!(snapshot.read_log(&log).unwrap(), vec![b"one".to_vec(), b"two".to_vec()]);
    assert_eq!(snapshot.sessions(), vec![session()]);
}

#[test]
fn reader_stays_pinned_to_its_manifest_while_writer_appends() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"one", b"two"]).unwrap();
    tx.commit().unwrap();

    let pinned = store.snapshot().unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"three"]).unwrap();
    // Not yet committed: the pinned reader sees two lines, a fresh reader too.
    assert_eq!(pinned.read_log(&log).unwrap().len(), 2);
    assert_eq!(store.snapshot().unwrap().read_log(&log).unwrap().len(), 2);
    tx.commit().unwrap();
    // Committed: the pinned reader still sees two, a fresh reader sees three.
    assert_eq!(pinned.read_log(&log).unwrap().len(), 2);
    assert_eq!(store.snapshot().unwrap().read_log(&log).unwrap().len(), 3);
}

#[test]
fn tail_beyond_pinned_length_is_ignored_and_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"one"]).unwrap();
    tx.commit().unwrap();

    // Simulate a crash after appending but before publishing the manifest.
    let path = dir.path().join(&log);
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"partial").unwrap();
    drop(file);
    assert_eq!(store.snapshot().unwrap().read_log(&log).unwrap(), vec![b"one".to_vec()]);

    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"two"]).unwrap();
    tx.commit().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"one\ntwo\n");
    assert_eq!(
        store.snapshot().unwrap().read_log(&log).unwrap(),
        vec![b"one".to_vec(), b"two".to_vec()]
    );
}

#[test]
fn second_writer_is_blocked_while_lock_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let first = store.begin().unwrap();
    assert!(matches!(store.begin(), Err(StoreError::Locked(_))));
    drop(first);
    assert!(store.begin().is_ok());
}

#[test]
fn commit_with_stale_expected_revision_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_log(&observations_log(&session()), &[b"one"]).unwrap();

    // Someone broke the lock and published revision 1 behind our back.
    let manifest_path = dir.path().join("manifest.json");
    let text = fs::read_to_string(&manifest_path).unwrap();
    fs::write(&manifest_path, text.replace("\"revision\": 0", "\"revision\": 1")).unwrap();

    assert!(matches!(
        tx.commit(),
        Err(StoreError::Conflict { expected: 0, found: 1 })
    ));
    assert_eq!(store.manifest().unwrap().revision.get(), 1);
}

#[test]
fn documents_are_immutable_per_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    tx.put_document("derivations/s1", b"{\"v\":1}").unwrap();
    tx.commit().unwrap();
    let first = store.snapshot().unwrap();

    let mut tx = store.begin().unwrap();
    tx.put_document("derivations/s1", b"{\"v\":2}").unwrap();
    tx.commit().unwrap();
    let second = store.snapshot().unwrap();

    assert_eq!(first.read_document("derivations/s1").unwrap().unwrap(), b"{\"v\":1}");
    assert_eq!(second.read_document("derivations/s1").unwrap().unwrap(), b"{\"v\":2}");
    assert_eq!(first.manifest().documents["derivations/s1"], "derivations/s1.r1.json");
    assert_eq!(second.manifest().documents["derivations/s1"], "derivations/s1.r2.json");
    assert!(dir.path().join("derivations/s1.r1.json").exists());
    assert_eq!(first.read_document("missing").unwrap(), None);
}

#[test]
fn dropped_transaction_leaves_manifest_unchanged_and_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());
    {
        let mut tx = store.begin().unwrap();
        tx.append_log(&log, &[b"one"]).unwrap();
        tx.set_cursor(
            "t",
            SourceCursor { offset: 4, next_ordinal: 1, session: Some(session()) },
        );
    }
    let manifest = store.manifest().unwrap();
    assert_eq!(manifest.revision, Revision::ZERO);
    assert!(manifest.logs.is_empty());
    assert!(manifest.cursors.is_empty());
    assert!(store.snapshot().unwrap().read_log(&log).unwrap().is_empty());
    // The lock was released with the transaction.
    assert!(store.begin().is_ok());
}

#[test]
fn cursors_and_associations_are_part_of_the_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    tx.set_cursor(
        "/home/x/.claude/projects/-work-repo/s1.jsonl",
        SourceCursor { offset: 120, next_ordinal: 7, session: Some(session()) },
    );
    tx.add_association(Association { checkout: PathBuf::from("/work/repo-wt") });
    tx.add_association(assoc());
    tx.commit().unwrap();
    let manifest = store.manifest().unwrap();
    assert_eq!(manifest.cursors["/home/x/.claude/projects/-work-repo/s1.jsonl"].offset, 120);
    assert_eq!(manifest.associations.len(), 2);
}

#[test]
fn log_lines_may_not_contain_newlines() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let mut tx = store.begin().unwrap();
    assert!(matches!(
        tx.append_log("observations/s1/events.jsonl", &[b"a\nb"]),
        Err(StoreError::Corrupt(_))
    ));
}

#[test]
fn stale_lock_file_from_a_dead_writer_does_not_block() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    // A writer that was killed leaves the file behind. Only the OS lock
    // matters, and the kernel released that with the process.
    fs::write(dir.path().join("write.lock"), b"").unwrap();
    let mut tx = store.begin().unwrap();
    tx.append_log(&observations_log(&session()), &[b"one"]).unwrap();
    assert_eq!(tx.commit().unwrap(), Revision::ZERO.next());
}

#[test]
fn transaction_snapshot_is_pinned_to_its_starting_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let log = observations_log(&session());
    let mut tx = store.begin().unwrap();
    tx.append_log(&log, &[b"one"]).unwrap();
    tx.commit().unwrap();

    let mut tx = store.begin().unwrap();
    let pinned = tx.snapshot();
    assert_eq!(pinned.revision(), Revision::ZERO.next());
    tx.append_log(&log, &[b"two"]).unwrap();
    assert_eq!(pinned.read_log(&log).unwrap(), vec![b"one".to_vec()]);
}

#[test]
fn log_names_cannot_escape_the_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let victim = observations_log(&SessionId::new("victim"));
    let mut tx = store.begin().unwrap();
    tx.append_log(&victim, &[b"keep me"]).unwrap();
    tx.commit().unwrap();

    let mut tx = store.begin().unwrap();
    for bad in [
        "../outside.jsonl",
        "observations/../../x",
        "/abs/path",
        "observations//events.jsonl",
        "./events.jsonl",
        "",
    ] {
        assert!(
            matches!(tx.append_log(bad, &[b"x"]), Err(StoreError::InvalidName(_))),
            "append_log accepted {bad:?}"
        );
        assert!(
            matches!(tx.put_document(bad, b"{}"), Err(StoreError::InvalidName(_))),
            "put_document accepted {bad:?}"
        );
    }
    drop(tx);
    let snapshot = store.snapshot().unwrap();
    assert!(matches!(snapshot.read_log("../x"), Err(StoreError::InvalidName(_))));
    assert_eq!(snapshot.read_log(&victim).unwrap(), vec![b"keep me".to_vec()]);
    assert!(!dir.path().parent().unwrap().join("outside.jsonl").exists());
}

#[test]
fn odd_session_ids_get_safe_storage_keys() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let victim = SessionId::new("victim");
    let attacker = SessionId::new("../observations/victim");
    let mut tx = store.begin().unwrap();
    tx.append_log(&observations_log(&victim), &[b"keep me"]).unwrap();
    tx.commit().unwrap();

    let mut tx = store.begin().unwrap();
    tx.append_log(&observations_log(&attacker), &[b"other"]).unwrap();
    tx.commit().unwrap();

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.read_log(&observations_log(&victim)).unwrap(), vec![b"keep me".to_vec()]);
    assert_eq!(snapshot.read_log(&observations_log(&attacker)).unwrap(), vec![b"other".to_vec()]);
    assert_eq!(storage_key(&attacker), "%2E%2E%2Fobservations%2Fvictim");
    let mut sessions = snapshot.sessions();
    sessions.sort();
    assert_eq!(sessions, vec![attacker, victim]);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-record --test store_protocol`
Expected: compile error, module `store` not found.

- [ ] **Step 3: Implement the store**

`crates/supersigil-record/src/store.rs`:

```rust
//! File-backed store whose manifest pins exactly one revision.
//!
//! Layout inside a record directory:
//!
//! ```text
//! manifest.json                       identity, revision, pins, cursors
//! write.lock                          present only while a writer is active
//! observations/<session>/events.jsonl append-only log, pinned by byte length
//! <logical>.r<revision>.json          immutable documents, pinned by name
//! ```

mod manifest;
mod snapshot;
mod write;

use std::io;
use std::path::{Path, PathBuf};

pub use manifest::{Association, Manifest, SCHEMA_VERSION, SourceCursor};
pub use snapshot::RecordSnapshot;
pub use write::WriteTx;

use crate::ids::{RecordId, SessionId};

/// File name of the manifest inside a record directory.
pub const MANIFEST_FILE: &str = "manifest.json";
/// File name of the writer lock inside a record directory.
pub const LOCK_FILE: &str = "write.lock";

/// Encodes a session id into one safe path component: every byte outside
/// `A-Z a-z 0-9 _ -` becomes `%XX`, so an id taken from a transcript can never
/// contain a separator or spell `..`.
#[must_use]
pub fn storage_key(session: &SessionId) -> String {
    let mut key = String::new();
    for byte in session.as_str().bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' {
            key.push(char::from(byte));
        } else {
            key.push_str(&format!("%{byte:02X}"));
        }
    }
    key
}

/// Inverts [`storage_key`]. A malformed escape is kept as-is.
#[must_use]
pub fn decode_storage_key(key: &str) -> SessionId {
    let bytes = key.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    SessionId::new(String::from_utf8_lossy(&out).into_owned())
}

/// Relative path of a session's observation log.
#[must_use]
pub fn observations_log(session: &SessionId) -> String {
    format!("observations/{}/events.jsonl", storage_key(session))
}

/// Checks that a log or document name is a relative path of plain
/// components: no `..`, no root, no `.`, no empty component.
pub(crate) fn validate_name(name: &str) -> Result<(), StoreError> {
    let plain = !name.is_empty()
        && !name.contains("//")
        && !name.ends_with('/')
        && Path::new(name)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
    if plain {
        Ok(())
    } else {
        Err(StoreError::InvalidName(name.to_owned()))
    }
}

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// An I/O operation failed.
    #[error("i/o error at {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// A file did not contain the expected JSON.
    #[error("invalid json at {path}: {source}")]
    Json {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },
    /// The directory has no manifest.
    #[error("{0} is not a record directory (no manifest.json)")]
    NotARecord(PathBuf),
    /// The directory already holds a record.
    #[error("{0} already contains a record")]
    AlreadyExists(PathBuf),
    /// Another writer holds the lock.
    #[error("record at {0} is locked by another writer")]
    Locked(PathBuf),
    /// The manifest changed under the writer.
    #[error("manifest revision changed under the writer (expected {expected}, found {found})")]
    Conflict {
        /// Revision the writer started from.
        expected: u64,
        /// Revision found on disk at commit.
        found: u64,
    },
    /// The on-disk state violates an invariant.
    #[error("record is corrupt: {0}")]
    Corrupt(String),
    /// A log or document name is not a plain relative path.
    #[error("invalid record file name: {0:?}")]
    InvalidName(String),
}

pub(crate) fn io_error(path: &Path, source: io::Error) -> StoreError {
    StoreError::Io { path: path.to_path_buf(), source }
}

/// Handle to a record directory.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Creates a new record at `root` with a fresh identity, one checkout
    /// association, and revision zero.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if a manifest is already there,
    /// or an I/O error.
    pub fn create(root: &Path, association: Association) -> Result<Self, StoreError> {
        Self::create_with_id(root, RecordId::generate(), association)
    }

    /// Creates a new record at `root` with the given identity. Callers that
    /// name the directory after the id use this.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if a manifest is already there,
    /// or an I/O error.
    pub fn create_with_id(
        root: &Path,
        record_id: RecordId,
        association: Association,
    ) -> Result<Self, StoreError> {
        if root.join(MANIFEST_FILE).exists() {
            return Err(StoreError::AlreadyExists(root.to_path_buf()));
        }
        std::fs::create_dir_all(root).map_err(|e| io_error(root, e))?;
        let manifest = Manifest::new(record_id, association);
        manifest::publish(root, &manifest)?;
        Ok(Self { root: root.to_path_buf() })
    }

    /// Opens an existing record.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotARecord`] if there is no manifest.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        if !root.join(MANIFEST_FILE).is_file() {
            return Err(StoreError::NotARecord(root.to_path_buf()));
        }
        Ok(Self { root: root.to_path_buf() })
    }

    /// The record directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads the current manifest.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be read or parsed.
    pub fn manifest(&self) -> Result<Manifest, StoreError> {
        manifest::read(&self.root)
    }

    /// A read view pinned to the manifest as it is right now.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be read.
    pub fn snapshot(&self) -> Result<RecordSnapshot, StoreError> {
        Ok(RecordSnapshot::new(self.root.clone(), self.manifest()?))
    }

    /// Starts a write transaction, taking the writer lock.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Locked`] if another writer is active.
    pub fn begin(&self) -> Result<WriteTx<'_>, StoreError> {
        WriteTx::begin(self)
    }
}
```

`crates/supersigil-record/src/store/manifest.rs`:

```rust
//! The manifest: identity, revision, and what the revision pins.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{MANIFEST_FILE, StoreError, io_error};
use crate::ids::{RecordId, Revision, SessionId};

/// Schema version written into new manifests.
pub const SCHEMA_VERSION: u32 = 1;

/// A checkout this record is associated with. Associations locate a record;
/// they never merge records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Association {
    /// Canonical path of the checkout (a worktree or the main working tree).
    pub checkout: PathBuf,
}

/// Where sync left off in one transcript file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCursor {
    /// Byte offset of the first unconsumed byte.
    pub offset: u64,
    /// Ordinal to assign to the next record.
    pub next_ordinal: u64,
    /// Session the transcript belongs to, once known.
    pub session: Option<SessionId>,
}

/// One revision of a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Record identity.
    pub record_id: RecordId,
    /// Schema version of the files this manifest describes.
    pub schema_version: u32,
    /// Revision number, incremented on every commit.
    pub revision: Revision,
    /// Checkouts this record is associated with.
    pub associations: Vec<Association>,
    /// Committed byte length per append-only log, keyed by relative path.
    #[serde(default)]
    pub logs: BTreeMap<String, u64>,
    /// Immutable file name per logical document.
    #[serde(default)]
    pub documents: BTreeMap<String, String>,
    /// Sync cursors keyed by transcript path.
    #[serde(default)]
    pub cursors: BTreeMap<String, SourceCursor>,
}

impl Manifest {
    /// A manifest at revision zero with one association.
    #[must_use]
    pub fn new(record_id: RecordId, association: Association) -> Self {
        Self {
            record_id,
            schema_version: SCHEMA_VERSION,
            revision: Revision::ZERO,
            associations: vec![association],
            logs: BTreeMap::new(),
            documents: BTreeMap::new(),
            cursors: BTreeMap::new(),
        }
    }
}

pub(super) fn read(root: &Path) -> Result<Manifest, StoreError> {
    let path = root.join(MANIFEST_FILE);
    let bytes = fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StoreError::NotARecord(root.to_path_buf())
        } else {
            io_error(&path, e)
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|source| StoreError::Json { path, source })
}

/// Writes the manifest to a temporary file, fsyncs it, renames it into
/// place, and fsyncs the directory where the platform allows.
pub(super) fn publish(root: &Path, manifest: &Manifest) -> Result<(), StoreError> {
    let final_path = root.join(MANIFEST_FILE);
    let tmp_path = root.join(format!("{MANIFEST_FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|source| StoreError::Json { path: final_path.clone(), source })?;
    let mut file = File::create(&tmp_path).map_err(|e| io_error(&tmp_path, e))?;
    file.write_all(&bytes).map_err(|e| io_error(&tmp_path, e))?;
    file.sync_all().map_err(|e| io_error(&tmp_path, e))?;
    drop(file);
    fs::rename(&tmp_path, &final_path).map_err(|e| io_error(&final_path, e))?;
    sync_dir(root);
    Ok(())
}

/// Best-effort directory fsync; directories cannot be opened on every
/// platform, and a failure here does not lose data already fsynced.
pub(super) fn sync_dir(dir: &Path) {
    if let Ok(handle) = File::open(dir) {
        let _ = handle.sync_all();
    }
}
```

`crates/supersigil-record/src/store/snapshot.rs`:

```rust
//! A read view pinned to one manifest.

use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use super::{Manifest, StoreError, decode_storage_key, io_error, observations_log, validate_name};
use crate::ids::{Revision, SessionId};
use crate::observations::Observation;

/// Reads only what its manifest pins, so it never mixes revisions.
#[derive(Debug, Clone)]
pub struct RecordSnapshot {
    root: PathBuf,
    manifest: Manifest,
}

impl RecordSnapshot {
    pub(super) fn new(root: PathBuf, manifest: Manifest) -> Self {
        Self { root, manifest }
    }

    /// The pinned manifest.
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The pinned revision.
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.manifest.revision
    }

    /// Lines of a log up to its committed length. An unpinned log is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is shorter than its pinned length or
    /// cannot be read.
    pub fn read_log(&self, log: &str) -> Result<Vec<Vec<u8>>, StoreError> {
        validate_name(log)?;
        let Some(&pinned) = self.manifest.logs.get(log) else {
            return Ok(Vec::new());
        };
        let path = self.root.join(log);
        let bytes = read_prefix(&path, pinned)?;
        Ok(bytes
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(<[u8]>::to_vec)
            .collect())
    }

    /// Bytes of a logical document at this revision, if it exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the pinned file cannot be read.
    pub fn read_document(&self, logical: &str) -> Result<Option<Vec<u8>>, StoreError> {
        validate_name(logical)?;
        let Some(rel) = self.manifest.documents.get(logical) else {
            return Ok(None);
        };
        let path = self.root.join(rel);
        std::fs::read(&path).map(Some).map_err(|e| io_error(&path, e))
    }

    /// Sessions that have an observation log in this revision.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionId> {
        self.manifest
            .logs
            .keys()
            .filter_map(|log| {
                log.strip_prefix("observations/")?
                    .strip_suffix("/events.jsonl")
                    .map(decode_storage_key)
            })
            .collect()
    }

    /// All observations of a session, in log order.
    ///
    /// # Errors
    ///
    /// Returns an error if the log cannot be read or a line is not a valid
    /// observation.
    pub fn observations(&self, session: &SessionId) -> Result<Vec<Observation>, StoreError> {
        let log = observations_log(session);
        let path = self.root.join(&log);
        self.read_log(&log)?
            .iter()
            .map(|line| {
                serde_json::from_slice(line)
                    .map_err(|source| StoreError::Json { path: path.clone(), source })
            })
            .collect()
    }
}

fn read_prefix(path: &Path, len: u64) -> Result<Vec<u8>, StoreError> {
    let file = File::open(path).map_err(|e| io_error(path, e))?;
    let mut bytes = Vec::new();
    file.take(len)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error(path, e))?;
    if bytes.len() as u64 != len {
        return Err(StoreError::Corrupt(format!(
            "{} is shorter ({} bytes) than its pinned length ({len})",
            path.display(),
            bytes.len()
        )));
    }
    Ok(bytes)
}
```

`crates/supersigil-record/src/store/write.rs`:

```rust
//! A write transaction: lock, stage, fsync, publish.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::PathBuf;

use super::manifest::{self, Association, Manifest, SourceCursor};
use super::{
    LOCK_FILE, RecordSnapshot, Store, StoreError, io_error, observations_log, validate_name,
};
use crate::ids::Revision;
use crate::observations::Observation;

/// An open write transaction. Dropping it without [`WriteTx::commit`]
/// releases the lock and leaves the manifest untouched.
///
/// The lock is an exclusive OS lock on `write.lock` held through `_lock`.
/// The kernel releases it when the handle drops or the process dies, so a
/// killed writer never blocks the next one. The file itself is never removed.
#[derive(Debug)]
pub struct WriteTx<'a> {
    store: &'a Store,
    /// The manifest as it will be published, with staged changes applied.
    manifest: Manifest,
    /// The manifest as it was when the lock was taken.
    starting: Manifest,
    expected: Revision,
    _lock: File,
    touched: Vec<PathBuf>,
}

impl<'a> WriteTx<'a> {
    pub(super) fn begin(store: &'a Store) -> Result<Self, StoreError> {
        let lock_path = store.root().join(LOCK_FILE);
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| io_error(&lock_path, e))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(StoreError::Locked(store.root().to_path_buf()));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(io_error(&lock_path, e)),
        }
        let manifest = manifest::read(store.root())?;
        Ok(Self {
            store,
            starting: manifest.clone(),
            expected: manifest.revision,
            manifest,
            _lock: lock,
            touched: Vec::new(),
        })
    }

    /// The manifest as it will be published, including staged changes.
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// A read view pinned to the manifest this transaction started from.
    /// This transaction's own appends are not visible through it, so
    /// derivations computed from it plus the staged observations are
    /// complete and consistent.
    #[must_use]
    pub fn snapshot(&self) -> RecordSnapshot {
        RecordSnapshot::new(self.store.root().to_path_buf(), self.starting.clone())
    }

    /// Appends lines to a log. Any bytes beyond the pinned length (a crash
    /// tail) are truncated first. Lines must not contain newlines.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidName`] for a name that is not a plain
    /// relative path, [`StoreError::Corrupt`] for a line with a newline or a
    /// file shorter than its pinned length, or an I/O error.
    pub fn append_log(&mut self, log: &str, lines: &[&[u8]]) -> Result<(), StoreError> {
        validate_name(log)?;
        if let Some(bad) = lines.iter().find(|l| l.contains(&b'\n')) {
            return Err(StoreError::Corrupt(format!(
                "log line contains a newline: {}",
                String::from_utf8_lossy(bad)
            )));
        }
        let path = self.store.root().join(log);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        let pinned = self.manifest.logs.get(log).copied().unwrap_or(0);
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| io_error(&path, e))?;
        let len = file.metadata().map_err(|e| io_error(&path, e))?.len();
        if len < pinned {
            return Err(StoreError::Corrupt(format!(
                "{} is shorter ({len} bytes) than its pinned length ({pinned})",
                path.display()
            )));
        }
        file.set_len(pinned).map_err(|e| io_error(&path, e))?;
        file.seek(SeekFrom::Start(pinned)).map_err(|e| io_error(&path, e))?;
        let mut written = pinned;
        for line in lines {
            file.write_all(line).map_err(|e| io_error(&path, e))?;
            file.write_all(b"\n").map_err(|e| io_error(&path, e))?;
            written += line.len() as u64 + 1;
        }
        self.manifest.logs.insert(log.to_owned(), written);
        self.touched.push(path);
        Ok(())
    }

    /// Appends observations to their sessions' logs as JSON lines.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or the append fails.
    pub fn append_observations(&mut self, observations: &[Observation]) -> Result<(), StoreError> {
        let mut by_log: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
        for observation in observations {
            let log = observations_log(observation.session());
            let line = serde_json::to_vec(observation).map_err(|source| StoreError::Json {
                path: self.store.root().join(&log),
                source,
            })?;
            by_log.entry(log).or_default().push(line);
        }
        for (log, lines) in &by_log {
            let refs: Vec<&[u8]> = lines.iter().map(Vec::as_slice).collect();
            self.append_log(log, &refs)?;
        }
        Ok(())
    }

    /// Writes an immutable document for this revision and points the logical
    /// name at it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidName`] for a name that is not a plain
    /// relative path, or an I/O error.
    pub fn put_document(&mut self, logical: &str, bytes: &[u8]) -> Result<(), StoreError> {
        validate_name(logical)?;
        let rel = format!("{logical}.r{}.json", self.expected.next().get());
        let path = self.store.root().join(&rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        fs::write(&path, bytes).map_err(|e| io_error(&path, e))?;
        self.manifest.documents.insert(logical.to_owned(), rel);
        self.touched.push(path);
        Ok(())
    }

    /// Records where sync left off in a transcript.
    pub fn set_cursor(&mut self, key: &str, cursor: SourceCursor) {
        self.manifest.cursors.insert(key.to_owned(), cursor);
    }

    /// Adds a checkout association unless one with the same path exists.
    pub fn add_association(&mut self, association: Association) {
        if !self.manifest.associations.iter().any(|a| a.checkout == association.checkout) {
            self.manifest.associations.push(association);
        }
    }

    /// Fsyncs every touched file, checks the manifest has not moved, and
    /// publishes the next revision.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if the on-disk revision is not the one
    /// this transaction started from, or an I/O error.
    pub fn commit(mut self) -> Result<Revision, StoreError> {
        for path in &self.touched {
            File::open(path)
                .and_then(|f| f.sync_all())
                .map_err(|e| io_error(path, e))?;
        }
        let on_disk = manifest::read(self.store.root())?;
        if on_disk.revision != self.expected {
            return Err(StoreError::Conflict {
                expected: self.expected.get(),
                found: on_disk.revision.get(),
            });
        }
        self.manifest.revision = self.expected.next();
        manifest::publish(self.store.root(), &self.manifest)?;
        Ok(self.manifest.revision)
    }
}

// No `Drop` impl: dropping `_lock` releases the OS lock, and the manifest on
// disk is untouched until `commit` publishes it.
```

In `lib.rs` add `pub mod store;` and `pub use store::{RecordSnapshot, Store, StoreError, WriteTx};`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-record && cargo clippy -p supersigil-record --all-targets --all-features`
Expected: all tests pass, no warnings. If clippy flags `cast_possible_truncation` on `bytes.len() as u64`, it will not: widening `usize` to `u64` is allowed; `line.len() as u64` likewise.

- [ ] **Step 5: Commit**

```bash
git add crates/supersigil-record/src/store.rs crates/supersigil-record/src/store crates/supersigil-record/src/lib.rs crates/supersigil-record/tests/store_protocol.rs
git commit -m "feat: add revision-pinned record store"
```

---
### Task 5: Derivations: restores and discontinuities

**Files:**
- Create: `crates/supersigil-record/src/derivations.rs`, `crates/supersigil-record/src/derive.rs`
- Modify: `crates/supersigil-record/src/store/write.rs`, `crates/supersigil-record/src/store/snapshot.rs`, `crates/supersigil-record/src/lib.rs`
- Test: inline `#[cfg(test)]` in `derive.rs`; one store test appended to `tests/store_protocol.rs`

**Interfaces:**
- Consumes: `Observation`, `Edit`, `Revision`, `SessionId`, `EventId`, `WriteTx::put_document`, `RecordSnapshot::read_document`.
- Produces:
  - `derivations::ALGORITHM_VERSION: u32 = 1`.
  - `DerivationSet { session, observation_revision: Revision, algorithm_version: u32, restores: Vec<Restore>, discontinuities: Vec<Discontinuity> }`.
  - `Restore { edit: EventId, restores: Vec<EventId> }`: the edit's after-content equals the before-content of each listed earlier edit on the same checkout and path.
  - `Discontinuity { path: PathBuf, checkout: PathBuf, prev: EventId, next: EventId }`: `next.before` is known and differs from the known `prev.after`.
  - `derive::derive(session: &SessionId, observations: &[Observation], observation_revision: Revision) -> DerivationSet`.
  - `WriteTx::put_derivations(&DerivationSet)` writing logical document `derivations/<session>`; `RecordSnapshot::derivations(&SessionId) -> Result<Option<DerivationSet>>`.

A derivation makes a claim only when both content ids are known. An unknown side is silence, not a discontinuity. This is the design's rule that an unknown preimage never looks like a known one.

- [ ] **Step 1: Write the failing tests**

`crates/supersigil-record/src/derive.rs` (tests first; the implementation goes above them in step 3):

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::ids::{ContentId, Timestamp, TurnId};
    use crate::observations::{Edit, FileState, Material};

    fn session() -> SessionId {
        SessionId::new("s1")
    }

    fn edit(tool: &str, path: &str, ordinal: u64, before: FileState, after: FileState) -> Observation {
        Observation::Edit(Edit {
            id: EventId::derive("edit", &session(), tool),
            turn: TurnId::new(format!("a{ordinal}")),
            session: session(),
            path: PathBuf::from(path),
            before,
            after,
            patch: Material::Unavailable { reason: "test".to_owned() },
            old_text: Material::Unavailable { reason: "test".to_owned() },
            new_text: Material::Unavailable { reason: "test".to_owned() },
            replace_all: false,
            checkout: PathBuf::from("/work/repo"),
            time: Timestamp::new(format!("2026-09-28T10:00:{ordinal:02}.000Z")),
            source_ordinal: ordinal,
        })
    }

    fn known(text: &str) -> FileState {
        FileState::known(ContentId::of(text.as_bytes()))
    }

    fn id(tool: &str) -> EventId {
        EventId::derive("edit", &session(), tool)
    }

    #[test]
    fn edit_back_to_earlier_content_is_a_restore() {
        let obs = vec![
            edit("t1", "notes.txt", 1, known("draft\n"), known("final\n")),
            edit("t2", "notes.txt", 2, known("final\n"), known("draft\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert_eq!(set.restores, vec![Restore { edit: id("t2"), restores: vec![id("t1")] }]);
        assert!(set.discontinuities.is_empty());
        assert_eq!(set.observation_revision, Revision::ZERO.next());
        assert_eq!(set.algorithm_version, ALGORITHM_VERSION);
    }

    #[test]
    fn mismatched_known_states_are_a_discontinuity() {
        let obs = vec![
            edit("t1", "src/lib.rs", 1, known("a\n"), known("b\n")),
            edit("t2", "src/lib.rs", 2, known("// manual\nb\n"), known("// manual\nc\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert_eq!(
            set.discontinuities,
            vec![Discontinuity {
                path: PathBuf::from("src/lib.rs"),
                checkout: PathBuf::from("/work/repo"),
                prev: id("t1"),
                next: id("t2"),
            }]
        );
        assert!(set.restores.is_empty());
    }

    #[test]
    fn unknown_content_makes_no_claim() {
        let obs = vec![
            edit("t1", "src/lib.rs", 1, known("a\n"), FileState::unknown()),
            edit("t2", "src/lib.rs", 2, known("z\n"), known("y\n")),
            edit("t3", "src/lib.rs", 3, FileState::unknown(), known("a\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert!(set.discontinuities.is_empty());
        // t3 lands on "a\n", which was t1's before-state: that is a restore
        // because both of those hashes are known.
        assert_eq!(set.restores, vec![Restore { edit: id("t3"), restores: vec![id("t1")] }]);
    }

    #[test]
    fn different_paths_and_checkouts_do_not_interact() {
        let mut other = edit("t2", "b.txt", 2, known("x\n"), known("y\n"));
        if let Observation::Edit(e) = &mut other {
            e.checkout = PathBuf::from("/work/other");
        }
        let obs = vec![
            edit("t1", "a.txt", 1, known("x\n"), known("y\n")),
            other,
            edit("t3", "a.txt", 3, known("q\n"), known("x\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert_eq!(set.restores, vec![Restore { edit: id("t3"), restores: vec![id("t1")] }]);
        assert_eq!(set.discontinuities.len(), 1);
        assert_eq!(set.discontinuities[0].next, id("t3"));
    }

    #[test]
    fn no_op_edit_is_not_a_restore_and_creation_after_absence_is_continuous() {
        let obs = vec![
            edit("t1", "a.txt", 1, known("x\n"), known("x\n")),
            edit("t2", "new.rs", 2, FileState::Absent, known("v\n")),
        ];
        let set = derive(&session(), &obs, Revision::ZERO.next());
        assert!(set.restores.is_empty());
        assert!(set.discontinuities.is_empty());
    }
}
```

Append to `crates/supersigil-record/tests/store_protocol.rs`:

```rust
#[test]
fn derivations_round_trip_through_the_store() {
    use supersigil_record::derivations::{ALGORITHM_VERSION, DerivationSet};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::create(dir.path(), assoc()).unwrap();
    let set = DerivationSet {
        session: session(),
        observation_revision: Revision::ZERO.next(),
        algorithm_version: ALGORITHM_VERSION,
        restores: Vec::new(),
        discontinuities: Vec::new(),
    };
    let mut tx = store.begin().unwrap();
    tx.put_derivations(&set).unwrap();
    tx.commit().unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.derivations(&session()).unwrap(), Some(set));
    assert_eq!(snapshot.derivations(&SessionId::new("other")).unwrap(), None);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-record`
Expected: compile errors, `derive`, `derivations` not found.

- [ ] **Step 3: Implement**

`crates/supersigil-record/src/derivations.rs`:

```rust
//! Derivation layer: rebuildable findings computed from observations.
//!
//! Every set records the observation revision and algorithm version it was
//! computed from. A newer computation supersedes an older one; nothing here
//! is a timeless fact.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{EventId, Revision, SessionId};

/// Version of the derivation algorithms in [`crate::derive`].
pub const ALGORITHM_VERSION: u32 = 1;

/// An edit whose after-content equals the before-content of earlier edits on
/// the same checkout and path. An observation, not an explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restore {
    /// The restoring edit.
    pub edit: EventId,
    /// Earlier edits whose before-content this edit returned to, oldest first.
    pub restores: Vec<EventId>,
}

/// Two consecutive edits on one path whose known states do not meet: the
/// available observations have a gap between them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discontinuity {
    /// Path relative to the checkout.
    pub path: PathBuf,
    /// Checkout the edits happened in.
    pub checkout: PathBuf,
    /// The earlier edit.
    pub prev: EventId,
    /// The later edit whose before-state did not match.
    pub next: EventId,
}

/// All derivations for one session at one observation revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivationSet {
    /// Session the derivations describe.
    pub session: SessionId,
    /// Observation revision the input was read at.
    pub observation_revision: Revision,
    /// Algorithm version that produced the set.
    pub algorithm_version: u32,
    /// Restores found.
    pub restores: Vec<Restore>,
    /// Discontinuities found.
    pub discontinuities: Vec<Discontinuity>,
}

impl DerivationSet {
    /// Logical document name for a session's derivations.
    #[must_use]
    pub fn document_name(session: &SessionId) -> String {
        format!("derivations/{session}")
    }
}
```

`crates/supersigil-record/src/derive.rs` (above the test module):

```rust
//! Computes derivations from a session's observations.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::derivations::{ALGORITHM_VERSION, DerivationSet, Discontinuity, Restore};
use crate::ids::{EventId, Revision, SessionId};
use crate::observations::{Edit, Observation};

/// Derives restores and discontinuities from `observations`.
///
/// Edits are grouped by checkout and path and ordered by `source_ordinal`.
/// A claim is made only where both content ids involved are known.
#[must_use]
pub fn derive(
    session: &SessionId,
    observations: &[Observation],
    observation_revision: Revision,
) -> DerivationSet {
    let mut groups: BTreeMap<(PathBuf, PathBuf), Vec<&Edit>> = BTreeMap::new();
    for observation in observations {
        if let Observation::Edit(edit) = observation {
            groups
                .entry((edit.checkout.clone(), edit.path.clone()))
                .or_default()
                .push(edit);
        }
    }

    let mut restores = Vec::new();
    let mut discontinuities = Vec::new();
    for ((checkout, path), mut edits) in groups {
        edits.sort_by_key(|e| e.source_ordinal);
        for (index, edit) in edits.iter().enumerate() {
            if let Some(after) = edit.after_content() {
                if edit.before_content() == Some(after) {
                    continue;
                }
                let earlier: Vec<EventId> = edits[..index]
                    .iter()
                    .filter(|e| e.before_content() == Some(after))
                    .map(|e| e.id.clone())
                    .collect();
                if !earlier.is_empty() {
                    restores.push(Restore { edit: edit.id.clone(), restores: earlier });
                }
            }
        }
        for pair in edits.windows(2) {
            let (prev, next) = (pair[0], pair[1]);
            if let (Some(a), Some(b)) = (prev.after_content(), next.before_content())
                && a != b
            {
                discontinuities.push(Discontinuity {
                    path: path.clone(),
                    checkout: checkout.clone(),
                    prev: prev.id.clone(),
                    next: next.id.clone(),
                });
            }
        }
    }
    restores.sort_by(|a, b| a.edit.cmp(&b.edit));

    DerivationSet {
        session: session.clone(),
        observation_revision,
        algorithm_version: ALGORITHM_VERSION,
        restores,
        discontinuities,
    }
}
```

Note: `restores` is sorted by edit id so the output is deterministic across path groups; the tests above have one restore each, so ordering does not affect them.

Add to `crates/supersigil-record/src/store/write.rs` inside `impl<'a> WriteTx<'a>`:

```rust
    /// Writes a session's derivations as this revision's document.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or the write fails.
    pub fn put_derivations(&mut self, set: &DerivationSet) -> Result<(), StoreError> {
        let logical = DerivationSet::document_name(&set.session);
        let bytes = serde_json::to_vec_pretty(set).map_err(|source| StoreError::Json {
            path: self.store.root().join(&logical),
            source,
        })?;
        self.put_document(&logical, &bytes)
    }
```

with `use crate::derivations::DerivationSet;` at the top of `write.rs`.

Add to `crates/supersigil-record/src/store/snapshot.rs` inside `impl RecordSnapshot`:

```rust
    /// A session's derivations at this revision, if any were written.
    ///
    /// # Errors
    ///
    /// Returns an error if the document cannot be read or parsed.
    pub fn derivations(&self, session: &SessionId) -> Result<Option<DerivationSet>, StoreError> {
        let logical = DerivationSet::document_name(session);
        let Some(bytes) = self.read_document(&logical)? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| StoreError::Json { path: self.root.join(&logical), source })
    }
```

with `use crate::derivations::DerivationSet;` at the top of `snapshot.rs`.

In `lib.rs` add `pub mod derivations;`, `pub mod derive;`, and `pub use derivations::DerivationSet;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-record && cargo clippy -p supersigil-record --all-targets --all-features`
Expected: all pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/supersigil-record/src/derivations.rs crates/supersigil-record/src/derive.rs crates/supersigil-record/src/store/write.rs crates/supersigil-record/src/store/snapshot.rs crates/supersigil-record/src/lib.rs crates/supersigil-record/tests/store_protocol.rs
git commit -m "feat: derive restores and discontinuities from observations"
```

---
### Task 6: Claude Code transcript parser

**Files:**
- Create: `crates/supersigil-session/src/claude_code.rs`, `crates/supersigil-session/src/claude_code/content.rs`
- Modify: `crates/supersigil-session/src/lib.rs`
- Create: `crates/supersigil-session/tests/fixtures/slice.jsonl`
- Test: `crates/supersigil-session/tests/parse_fixture.rs`

**Interfaces:**
- Consumes: every observation type from Task 3, `EventId::derive`, `ContentId::of`.
- Produces:
  - `claude_code::parse_transcript(bytes: &[u8], start_ordinal: u64) -> ParseOutcome`.
  - `ParseOutcome { observations: Vec<Observation>, consumed: u64, next_ordinal: u64, session: Option<SessionId>, checkout: Option<PathBuf>, branch: Option<String>, first_time: Option<Timestamp>, ignored_records: BTreeMap<String, u64>, unknown_records: BTreeMap<String, u64>, malformed_lines: u64, trailing_partial: bool, outside_checkout: u64, abandoned_tool_uses: u64, failed_tool_uses: u64 }`.
  - `claude_code::content::{apply_edit(original, old, new, replace_all) -> Option<String>, apply_multi_edit(original, &[(old, new, replace_all)]) -> Option<String>}`.
  - `claude_code::classify_command(cmd: &str) -> CommandCategory` and `claude_code::test_outcome(stdout: &str, stderr: &str) -> Option<Outcome>`.

Rules the parser enforces:

- It never emits `SessionStart`; sync does that once per transcript. It reports the session id, checkout, branch, and first timestamp instead.
- `consumed` is the byte offset up to which every record has been fully turned into observations. A trailing line without a newline is not consumed. An assistant record with a tool use whose result has not arrived yet, and no later `user` or `assistant` record after it, is not consumed either, nor is anything after it, so the next sync re-reads it together with its result. A tool use with no result but with later records is abandoned: counted, and for a shell command emitted with an unavailable result.
- Observations are emitted only for records below `consumed`, so re-parsing from `consumed` never duplicates an event.
- Ordinals are `start_ordinal + index of the line`, counting every line including ignored and unknown ones, so ordinals are stable across syncs.
- A `user` record is `Meta` when `isMeta` is true, `Tool` when its content is an array whose blocks are all `tool_result`, otherwise `Human`. Records of type `summary` are ignored and counted; they are not turns.
- Edit paths are made relative to the record's `cwd`. A path is outside the checkout, counted and dropped, when `cwd` is empty, when the path does not start with `cwd`, or when the remainder contains a `..`, a root, or a prefix component. `strip_prefix` alone does not do this: `/work/repo/../secret` strips to `../secret`.
- The after-state of an `Edit` is computed by applying the replacement to `originalFile`; if `originalFile` is absent or `old_string` does not occur in it, the after-state is `Unknown`.
- A tool result the harness flags with `is_error: true` is not filesystem evidence. For `Edit`, `Write`, and `MultiEdit` it produces no edit and is counted in `failed_tool_uses`; a hash is never computed for bytes that may not exist. For `Bash` the command still ran, so it is emitted with `reported_error: true`, and a test run with no parsable verdict and a reported error is `Failed`.
- A test verdict gives failure precedence: any `test result: FAILED`, any nextest summary that mentions failed tests, or a bare `FAILED` marks the run `Failed` even if an earlier binary printed `test result: ok`.

- [ ] **Step 1: Write the fixture**

`crates/supersigil-session/tests/fixtures/slice.jsonl`, exactly 19 lines, each one JSON object, file ending with a newline. Session id `11111111-1111-4111-8111-111111111111`, checkout `/work/repo`:

```jsonl
{"type":"user","uuid":"u1","parentUuid":null,"sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"isMeta":false,"userType":"external","version":"2.1.283","message":{"role":"user","content":"Add a greeting function and clean up."}}
{"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:05.000Z","isSidechain":false,"agentId":null,"version":"2.1.283","message":{"role":"assistant","content":[{"type":"text","text":"I'll add greet to lib.rs."},{"type":"tool_use","id":"toolu_01","name":"Edit","input":{"file_path":"/work/repo/src/lib.rs","old_string":"pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n","new_string":"pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n"}}]}}
{"type":"user","uuid":"u2","parentUuid":"a1","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:06.000Z","isSidechain":false,"isMeta":false,"userType":"external","version":"2.1.283","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"The file /work/repo/src/lib.rs has been updated."}]},"toolUseResult":{"filePath":"/work/repo/src/lib.rs","oldString":"pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n","newString":"pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n","originalFile":"pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n","structuredPatch":[{"oldStart":1,"oldLines":3,"newStart":1,"newLines":7,"lines":[" pub fn add(a: i32, b: i32) -> i32 {","     a + b"," }","+","+pub fn greet(name: &str) -> String {","+    format!(\"hello {name}\")","+}"]}],"userModified":false,"replaceAll":false}}
{"type":"assistant","uuid":"a2","parentUuid":"u2","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:10.000Z","isSidechain":false,"agentId":null,"version":"2.1.283","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_02","name":"Write","input":{"file_path":"/work/repo/src/new.rs","content":"pub const VERSION: &str = \"1\";\n"}}]}}
{"type":"user","uuid":"u3","parentUuid":"a2","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:11.000Z","isSidechain":false,"isMeta":false,"userType":"external","version":"2.1.283","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_02","content":"File created successfully at: /work/repo/src/new.rs"}]},"toolUseResult":{"type":"create","filePath":"/work/repo/src/new.rs","content":"pub const VERSION: &str = \"1\";\n","structuredPatch":[]}}
{"type":"assistant","uuid":"a3","parentUuid":"u3","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:20.000Z","isSidechain":false,"agentId":null,"version":"2.1.283","message":{"role":"assistant","content":[{"type":"text","text":"Marking the notes final."},{"type":"tool_use","id":"toolu_03","name":"Edit","input":{"file_path":"/work/repo/notes.txt","old_string":"draft\n","new_string":"final\n"}}]}}
{"type":"user","uuid":"u4","parentUuid":"a3","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:21.000Z","isSidechain":false,"isMeta":false,"userType":"external","version":"2.1.283","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_03","content":"The file /work/repo/notes.txt has been updated."}]},"toolUseResult":{"filePath":"/work/repo/notes.txt","oldString":"draft\n","newString":"final\n","originalFile":"draft\n","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-draft","+final"]}],"userModified":false,"replaceAll":false}}
{"type":"assistant","uuid":"a4","parentUuid":"u4","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:30.000Z","isSidechain":false,"agentId":null,"version":"2.1.283","message":{"role":"assistant","content":[{"type":"text","text":"On reflection the notes are still a draft; reverting."},{"type":"tool_use","id":"toolu_04","name":"Edit","input":{"file_path":"/work/repo/notes.txt","old_string":"final\n","new_string":"draft\n"}}]}}
{"type":"user","uuid":"u5","parentUuid":"a4","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:31.000Z","isSidechain":false,"isMeta":false,"userType":"external","version":"2.1.283","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_04","content":"The file /work/repo/notes.txt has been updated."}]},"toolUseResult":{"filePath":"/work/repo/notes.txt","oldString":"final\n","newString":"draft\n","originalFile":"final\n","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-final","+draft"]}],"userModified":false,"replaceAll":false}}
{"type":"file-history-snapshot","messageId":"a4","isSnapshotUpdate":false,"snapshot":{"messageId":"a4","timestamp":"2026-09-28T10:00:31.500Z","trackedFileBackups":{}}}
{"type":"assistant","uuid":"a5","parentUuid":"u5","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:40.000Z","isSidechain":false,"agentId":null,"version":"2.1.283","message":{"role":"assistant","content":[{"type":"text","text":"Capitalising the greeting."},{"type":"tool_use","id":"toolu_05","name":"Edit","input":{"file_path":"/work/repo/src/lib.rs","old_string":"format!(\"hello {name}\")","new_string":"format!(\"Hello, {name}!\")"}}]}}
{"type":"user","uuid":"u6","parentUuid":"a5","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:41.000Z","isSidechain":false,"isMeta":false,"userType":"external","version":"2.1.283","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_05","content":"The file /work/repo/src/lib.rs has been updated."}]},"toolUseResult":{"filePath":"/work/repo/src/lib.rs","oldString":"format!(\"hello {name}\")","newString":"format!(\"Hello, {name}!\")","originalFile":"// manual\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n","structuredPatch":[{"oldStart":4,"oldLines":4,"newStart":4,"newLines":4,"lines":[" ","  pub fn greet(name: &str) -> String {","-    format!(\"hello {name}\")","+    format!(\"Hello, {name}!\")"," }"]}],"userModified":false,"replaceAll":false}}
{"type":"assistant","uuid":"a6","parentUuid":"u6","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:50.000Z","isSidechain":false,"agentId":null,"version":"2.1.283","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_06","name":"Bash","input":{"command":"rm old.txt","description":"Remove the obsolete file"}}]}}
{"type":"user","uuid":"u7","parentUuid":"a6","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:51.000Z","isSidechain":false,"isMeta":false,"userType":"external","version":"2.1.283","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_06","content":""}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false,"isImage":false,"noOutputExpected":false}}
{"type":"assistant","uuid":"a7","parentUuid":"u7","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:01:00.000Z","isSidechain":false,"agentId":null,"version":"2.1.283","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_07","name":"Bash","input":{"command":"cargo nextest run","description":"Run the test suite"}}]}}
{"type":"user","uuid":"u8","parentUuid":"a7","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:01:02.000Z","isSidechain":false,"isMeta":false,"userType":"external","version":"2.1.283","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_07","content":"    Starting 3 tests across 1 binary\n     Summary [   0.512s] 3 tests run: 3 passed, 0 skipped"}]},"toolUseResult":{"stdout":"    Starting 3 tests across 1 binary\n     Summary [   0.512s] 3 tests run: 3 passed, 0 skipped","stderr":"","interrupted":false,"isImage":false,"noOutputExpected":false}}
{"type":"ai-title","sessionId":"11111111-1111-4111-8111-111111111111","title":"Add greeting"}
{"type":"user","uuid":"u9","parentUuid":"u8","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:01:05.000Z","isSidechain":false,"isMeta":true,"userType":"external","version":"2.1.283","message":{"role":"user","content":"<local-command-caveat>Caveat: generated by a local command.</local-command-caveat>"}}
{"type":"assistant","uuid":"a8","parentUuid":"u9","sessionId":"11111111-1111-4111-8111-111111111111","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:01:10.000Z","isSidechain":false,"agentId":null,"version":"2.1.283","message":{"role":"assistant","content":[{"type":"text","text":"Done. Added greet, created new.rs, removed old.txt, and the tests pass."}]}}
```

Check with `jq -c . crates/supersigil-session/tests/fixtures/slice.jsonl | wc -l`; expected 19 with no parse error.

- [ ] **Step 2: Write the failing tests**

`crates/supersigil-session/tests/parse_fixture.rs`:

```rust
//! Parser behavior on the synthetic slice session.

use std::path::{Path, PathBuf};

use supersigil_record::observations::{
    CommandCategory, Content, FileState, Material, Observation, Outcome, Role,
};
use supersigil_record::{ContentId, EventId, SessionId};
use supersigil_session::claude_code::{ParseOutcome, parse_transcript};

const SESSION: &str = "11111111-1111-4111-8111-111111111111";

fn fixture() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slice.jsonl")).unwrap()
}

fn line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && i + 1 < bytes.len() {
            starts.push(i + 1);
        }
    }
    starts
}

fn edits(outcome: &ParseOutcome) -> Vec<&supersigil_record::observations::Edit> {
    outcome
        .observations
        .iter()
        .filter_map(|o| match o {
            Observation::Edit(e) => Some(e),
            _ => None,
        })
        .collect()
}

fn commands(outcome: &ParseOutcome) -> Vec<&supersigil_record::observations::Command> {
    outcome
        .observations
        .iter()
        .filter_map(|o| match o {
            Observation::Command(c) => Some(c),
            _ => None,
        })
        .collect()
}

fn turns(outcome: &ParseOutcome) -> Vec<&supersigil_record::observations::Turn> {
    outcome
        .observations
        .iter()
        .filter_map(|o| match o {
            Observation::Turn(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn known(text: &str) -> FileState {
    FileState::known(ContentId::of(text.as_bytes()))
}

#[test]
fn whole_fixture_is_consumed_with_session_metadata() {
    let bytes = fixture();
    let outcome = parse_transcript(&bytes, 0);
    assert_eq!(outcome.consumed, bytes.len() as u64);
    assert_eq!(outcome.next_ordinal, 19);
    assert_eq!(outcome.session, Some(SessionId::new(SESSION)));
    assert_eq!(outcome.checkout, Some(PathBuf::from("/work/repo")));
    assert_eq!(outcome.branch.as_deref(), Some("main"));
    assert_eq!(outcome.first_time.as_ref().map(|t| t.as_str()), Some("2026-09-28T10:00:00.000Z"));
    assert!(!outcome.trailing_partial);
    assert_eq!(outcome.malformed_lines, 0);
    assert_eq!(outcome.unknown_records.get("ai-title"), Some(&1));
    assert_eq!(outcome.ignored_records.get("file-history-snapshot"), Some(&1));
    assert_eq!(outcome.outside_checkout, 0);
    assert_eq!(outcome.abandoned_tool_uses, 0);
}

#[test]
fn turns_are_classified_and_ordered() {
    let outcome = parse_transcript(&fixture(), 0);
    let turns = turns(&outcome);
    assert_eq!(turns.len(), 17);
    assert_eq!(turns[0].role, Role::Human);
    assert_eq!(turns[0].excerpt.retained().map(String::as_str), Some("Add a greeting function and clean up."));
    assert_eq!(turns[0].source_ordinal, 0);
    assert_eq!(turns[1].role, Role::Agent);
    assert_eq!(turns[1].parent.as_ref().map(|p| p.as_str()), Some("u1"));
    assert_eq!(turns[1].excerpt.retained().map(String::as_str), Some("I'll add greet to lib.rs."));
    assert_eq!(turns[2].role, Role::Tool);
    assert!(turns[2].excerpt.retained().is_none());
    // Line 10 is a file-history record and line 17 an unknown record: the
    // meta turn on line 18 has ordinal 17.
    let meta = turns.iter().find(|t| t.id.as_str() == "u9").unwrap();
    assert_eq!(meta.role, Role::Meta);
    assert_eq!(meta.source_ordinal, 17);
    assert_eq!(turns.last().unwrap().id.as_str(), "a8");
}

#[test]
fn tool_result_and_meta_records_are_not_human() {
    let outcome = parse_transcript(&fixture(), 0);
    let humans: Vec<_> = turns(&outcome).into_iter().filter(|t| t.role == Role::Human).collect();
    assert_eq!(humans.len(), 1);
    assert_eq!(humans[0].id.as_str(), "u1");
}

#[test]
fn edits_carry_both_sides_and_computed_after_states() {
    let outcome = parse_transcript(&fixture(), 0);
    let session = SessionId::new(SESSION);
    let edits = edits(&outcome);
    assert_eq!(edits.len(), 5);

    let original = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
    let with_greet = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n";
    let first = edits[0];
    assert_eq!(first.id, EventId::derive("edit", &session, "toolu_01"));
    assert_eq!(first.turn.as_str(), "a1");
    assert_eq!(first.path, PathBuf::from("src/lib.rs"));
    assert_eq!(first.checkout, PathBuf::from("/work/repo"));
    assert_eq!(first.before, known(original));
    assert_eq!(first.after, known(with_greet));
    assert_eq!(first.patch.retained().map(Vec::len), Some(1));
    assert_eq!(first.old_text.retained().map(String::as_str), Some(original));
    assert_eq!(first.time.as_str(), "2026-09-28T10:00:06.000Z");
    assert_eq!(first.source_ordinal, 1);

    let created = edits[1];
    assert_eq!(created.path, PathBuf::from("src/new.rs"));
    assert_eq!(created.before, FileState::Absent);
    assert_eq!(created.after, known("pub const VERSION: &str = \"1\";\n"));
    assert!(matches!(created.old_text, Material::Unavailable { .. }));

    assert_eq!(edits[2].before, known("draft\n"));
    assert_eq!(edits[2].after, known("final\n"));
    assert_eq!(edits[3].before, known("final\n"));
    assert_eq!(edits[3].after, known("draft\n"));

    let manual = format!("// manual\n{with_greet}");
    let capitalised = manual.replace("format!(\"hello {name}\")", "format!(\"Hello, {name}!\")");
    assert_eq!(edits[4].before, known(&manual));
    assert_eq!(edits[4].after, known(&capitalised));
}

#[test]
fn commands_are_classified_with_outcome_and_end_time() {
    let outcome = parse_transcript(&fixture(), 0);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].cmd, "rm old.txt");
    assert_eq!(commands[0].category, CommandCategory::Other);
    assert!(!commands[0].reported_error);
    assert_eq!(commands[0].outcome, None);
    assert_eq!(commands[0].started.as_str(), "2026-09-28T10:00:50.000Z");
    assert_eq!(commands[0].ended.as_ref().map(|t| t.as_str()), Some("2026-09-28T10:00:51.000Z"));
    assert_eq!(commands[1].cmd, "cargo nextest run");
    assert_eq!(commands[1].category, CommandCategory::TestRun);
    assert_eq!(commands[1].outcome, Some(Outcome::Passed));
    assert!(commands[1].stdout_tail.retained().unwrap().contains("3 passed"));
}

#[test]
fn edit_with_missing_old_string_has_unknown_after_state() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/work/repo/a.txt","old_string":"gone","new_string":"there"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"filePath":"/work/repo/a.txt","oldString":"gone","newString":"there","originalFile":"something else\n","structuredPatch":[],"userModified":true,"replaceAll":false}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let edits = edits(&outcome);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].before, known("something else\n"));
    assert_eq!(edits[0].after, FileState::Present { content: Content::Unknown });
}

#[test]
fn edit_without_original_file_has_unknown_before_state() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/work/repo/a.txt","old_string":"x","new_string":"y"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"filePath":"/work/repo/a.txt","oldString":"x","newString":"y","structuredPatch":[],"userModified":false,"replaceAll":false}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let edits = edits(&outcome);
    assert_eq!(edits[0].before, FileState::unknown());
    assert_eq!(edits[0].after, FileState::unknown());
}

#[test]
fn trailing_partial_line_is_not_consumed() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let cut = &bytes[..starts[18] + 40];
    let outcome = parse_transcript(cut, 0);
    assert!(outcome.trailing_partial);
    assert_eq!(outcome.consumed, starts[18] as u64);
    assert_eq!(outcome.next_ordinal, 18);
    assert_eq!(turns(&outcome).last().unwrap().id.as_str(), "u9");
}

#[test]
fn pending_tool_use_at_end_is_not_consumed() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    // Everything through line 15 (the assistant record issuing `cargo nextest run`).
    let cut = &bytes[..starts[15]];
    let outcome = parse_transcript(cut, 0);
    assert!(!outcome.trailing_partial);
    assert_eq!(outcome.consumed, starts[14] as u64);
    assert_eq!(outcome.next_ordinal, 14);
    assert_eq!(commands(&outcome).len(), 1);
    assert_eq!(turns(&outcome).last().unwrap().id.as_str(), "u7");
    assert_eq!(outcome.abandoned_tool_uses, 0);
}

#[test]
fn abandoned_tool_use_is_counted_and_command_emitted_without_result() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:09.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":"never mind"}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.consumed, lines.len() as u64);
    assert_eq!(outcome.abandoned_tool_uses, 1);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].ended, None);
    assert!(matches!(commands[0].stdout_tail, Material::Unavailable { .. }));
    assert_eq!(commands[0].category, CommandCategory::TestRun);
    assert!(!commands[0].reported_error);
}

#[test]
fn edit_outside_checkout_is_counted_and_dropped() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/etc/hosts","content":"x"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"type":"update","filePath":"/etc/hosts","content":"x","structuredPatch":[]}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert_eq!(outcome.outside_checkout, 1);
    assert!(edits(&outcome).is_empty());
}

#[test]
fn malformed_line_is_counted_and_skipped() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let mut broken = bytes[..starts[1]].to_vec();
    broken.extend_from_slice(b"{not json\n");
    broken.extend_from_slice(&bytes[starts[1]..]);
    let outcome = parse_transcript(&broken, 0);
    assert_eq!(outcome.malformed_lines, 1);
    assert_eq!(outcome.next_ordinal, 20);
    assert_eq!(edits(&outcome).len(), 5);
}

#[test]
fn re_parsing_from_consumed_offset_continues_ordinals() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let first = parse_transcript(&bytes[..starts[15]], 0);
    let rest = parse_transcript(&bytes[first.consumed as usize..], first.next_ordinal);
    assert_eq!(rest.next_ordinal, 19);
    let all_turns: Vec<String> = turns(&first)
        .iter()
        .chain(turns(&rest).iter())
        .map(|t| t.id.as_str().to_owned())
        .collect();
    assert_eq!(all_turns.len(), 17);
    assert_eq!(all_turns[13], "a7");
    assert_eq!(rest.session, Some(SessionId::new(SESSION)));
}

#[test]
fn failed_write_is_not_an_edit() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/work/repo/locked.txt","content":"never written\n"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"EACCES: permission denied, open '/work/repo/locked.txt'"}]}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert!(edits(&outcome).is_empty());
    assert_eq!(outcome.failed_tool_uses, 1);
    assert_eq!(outcome.outside_checkout, 0);
    assert_eq!(turns(&outcome).len(), 2);
}

#[test]
fn errored_bash_records_reported_error_and_failed_tests() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo nextest run"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:09.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"error: could not compile"}]},"toolUseResult":{"stdout":"","stderr":"error: could not compile `supersigil`","interrupted":false}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    let commands = commands(&outcome);
    assert_eq!(commands.len(), 1);
    assert!(commands[0].reported_error);
    assert_eq!(commands[0].outcome, Some(Outcome::Failed));
    assert_eq!(outcome.failed_tool_uses, 0);
}

#[test]
fn traversal_path_is_outside_checkout() {
    let lines = concat!(
        r#"{"type":"assistant","uuid":"a1","parentUuid":null,"sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/work/repo/../private/secret","content":"x"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","parentUuid":"a1","sessionId":"s","cwd":"/work/repo","gitBranch":"main","timestamp":"2026-09-28T10:00:01.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":{"type":"create","filePath":"/work/repo/../private/secret","content":"x","structuredPatch":[]}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a2","parentUuid":"u1","sessionId":"s","cwd":"","gitBranch":"main","timestamp":"2026-09-28T10:00:02.000Z","isSidechain":false,"message":{"role":"assistant","content":[{"type":"tool_use","id":"t2","name":"Write","input":{"file_path":"a.txt","content":"x"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u2","parentUuid":"a2","sessionId":"s","cwd":"","gitBranch":"main","timestamp":"2026-09-28T10:00:03.000Z","isSidechain":false,"isMeta":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"ok"}]},"toolUseResult":{"type":"create","filePath":"a.txt","content":"x","structuredPatch":[]}}"#,
        "\n",
    );
    let outcome = parse_transcript(lines.as_bytes(), 0);
    assert!(edits(&outcome).is_empty());
    assert_eq!(outcome.outside_checkout, 2);
}

#[test]
fn mixed_cargo_test_output_is_failed() {
    use supersigil_session::claude_code::test_outcome;
    let stdout = "running 3 tests\ntest result: ok. 3 passed; 0 failed\n\nrunning 2 tests\ntest a ... FAILED\ntest result: FAILED. 1 passed; 1 failed\n";
    assert_eq!(test_outcome(stdout, ""), Some(Outcome::Failed));
    assert_eq!(test_outcome("test result: ok. 3 passed; 0 failed\n", ""), Some(Outcome::Passed));
    assert_eq!(test_outcome("compiling...", ""), None);
}

#[test]
fn nextest_failures_are_failed() {
    use supersigil_session::claude_code::test_outcome;
    let stdout = "     Summary [   0.512s] 3 tests run: 2 passed, 1 failed, 0 skipped\n        FAIL [   0.010s] crate::tests::a\n";
    assert_eq!(test_outcome(stdout, ""), Some(Outcome::Failed));
    assert_eq!(
        test_outcome("     Summary [   0.512s] 3 tests run: 3 passed, 0 skipped\n", ""),
        Some(Outcome::Passed)
    );
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-session --test parse_fixture`
Expected: compile error, module `claude_code` not found.
- [ ] **Step 4: Implement content application**

`crates/supersigil-session/src/claude_code/content.rs`:

```rust
//! Recomputes a file's content after an editing tool ran, from the retained
//! original and the tool's input. Returns `None` whenever the result cannot
//! be established, which the caller records as an unknown state.

/// Applies one `Edit` replacement. `None` if `old` is empty or absent.
#[must_use]
pub fn apply_edit(original: &str, old: &str, new: &str, replace_all: bool) -> Option<String> {
    if old.is_empty() || !original.contains(old) {
        return None;
    }
    Some(if replace_all {
        original.replace(old, new)
    } else {
        original.replacen(old, new, 1)
    })
}

/// Applies `MultiEdit` replacements in order. `None` if any step fails.
#[must_use]
pub fn apply_multi_edit(original: &str, edits: &[(String, String, bool)]) -> Option<String> {
    edits.iter().try_fold(original.to_owned(), |current, (old, new, all)| {
        apply_edit(&current, old, new, *all)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_occurrence_or_all() {
        assert_eq!(apply_edit("a b a", "a", "c", false).as_deref(), Some("c b a"));
        assert_eq!(apply_edit("a b a", "a", "c", true).as_deref(), Some("c b c"));
    }

    #[test]
    fn missing_or_empty_old_is_unknown() {
        assert_eq!(apply_edit("a", "z", "c", false), None);
        assert_eq!(apply_edit("a", "", "c", false), None);
    }

    #[test]
    fn multi_edit_is_sequential() {
        let edits = vec![
            ("a".to_owned(), "b".to_owned(), false),
            ("b".to_owned(), "c".to_owned(), true),
        ];
        assert_eq!(apply_multi_edit("a b", &edits).as_deref(), Some("c c"));
        let bad = vec![("q".to_owned(), "r".to_owned(), false)];
        assert_eq!(apply_multi_edit("a", &bad), None);
    }
}
```

- [ ] **Step 5: Implement the parser**

`crates/supersigil-session/src/claude_code.rs`:

```rust
//! Claude Code JSONL transcript parser.
//!
//! Every line is one record with a `type`. `user` and `assistant` records
//! are turns; tool uses inside assistant records pair with tool results in
//! later user records. The parser is tolerant: unknown record types and
//! malformed lines are counted, never fatal.

pub mod content;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use supersigil_record::observations::{
    Command, CommandCategory, Edit, FileState, Hunk, Material, Observation, Outcome, Role, Turn,
};
use supersigil_record::{ContentId, EventId, SessionId, Timestamp, TurnId};

/// Characters kept from an agent turn's text.
const AGENT_EXCERPT_CHARS: usize = 240;
/// Characters kept from the end of a command's output streams.
const OUTPUT_TAIL_CHARS: usize = 2000;
/// Record types the parser knows and deliberately does not turn into
/// observations.
const IGNORED_TYPES: &[&str] = &[
    "system",
    "attachment",
    "summary",
    "file-history-snapshot",
    "file-history-delta",
    "last-prompt",
    "mode",
    "permission-mode",
];

/// Result of parsing a byte range of a transcript.
#[derive(Debug, Default)]
pub struct ParseOutcome {
    /// Observations for every record below `consumed`.
    pub observations: Vec<Observation>,
    /// Byte offset up to which records were fully turned into observations.
    pub consumed: u64,
    /// Ordinal for the first record at or after `consumed`.
    pub next_ordinal: u64,
    /// Session id from the first record that carried one.
    pub session: Option<SessionId>,
    /// Working directory from the first record that carried one.
    pub checkout: Option<PathBuf>,
    /// Branch from the first record that carried one.
    pub branch: Option<String>,
    /// Timestamp of the first record that carried one.
    pub first_time: Option<Timestamp>,
    /// Known record types that were skipped, by type.
    pub ignored_records: BTreeMap<String, u64>,
    /// Unknown record types that were skipped, by type.
    pub unknown_records: BTreeMap<String, u64>,
    /// Lines that were not valid JSON objects.
    pub malformed_lines: u64,
    /// Whether the input ended in a line without a newline.
    pub trailing_partial: bool,
    /// Edits whose path was outside the record's working directory.
    pub outside_checkout: u64,
    /// Tool uses the agent moved past without a recorded result.
    pub abandoned_tool_uses: u64,
    /// Editing tool uses whose result the harness flagged as an error; they
    /// produce no edit.
    pub failed_tool_uses: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRecord {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    parent_uuid: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    git_branch: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    is_sidechain: bool,
    #[serde(default)]
    is_meta: bool,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    message: Option<RawMessage>,
    #[serde(default)]
    tool_use_result: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    #[serde(default)]
    content: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawHunk {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    #[serde(default)]
    lines: Vec<String>,
}

#[derive(Debug)]
struct PendingTool {
    id: String,
    name: String,
    input: Value,
    turn: TurnId,
    ordinal: u64,
    time: Timestamp,
    cwd: PathBuf,
    record_index: usize,
}

#[derive(Debug)]
struct Record {
    index: usize,
    ordinal: u64,
    start: usize,
    raw: RawRecord,
}

/// Parses complete lines of a transcript starting at ordinal `start_ordinal`.
///
/// See the module docs and [`ParseOutcome`] for what is and is not consumed.
#[must_use]
pub fn parse_transcript(bytes: &[u8], start_ordinal: u64) -> ParseOutcome {
    let mut outcome = ParseOutcome::default();
    let mut records = Vec::new();
    let mut line_starts = Vec::new();
    let mut cursor = 0usize;
    let mut index = 0usize;

    while cursor < bytes.len() {
        let Some(len) = bytes[cursor..].iter().position(|b| *b == b'\n') else {
            outcome.trailing_partial = true;
            break;
        };
        let line = &bytes[cursor..cursor + len];
        let start = cursor;
        cursor += len + 1;
        line_starts.push(start);
        let ordinal = start_ordinal + index as u64;
        match serde_json::from_slice::<RawRecord>(line) {
            Ok(raw) => records.push(Record { index, ordinal, start, raw }),
            Err(_) => outcome.malformed_lines += 1,
        }
        index += 1;
    }
    let complete_end = cursor;
    let line_count = index;

    let mut staged: Vec<(usize, Observation)> = Vec::new();
    let mut pending: Vec<PendingTool> = Vec::new();
    let mut session: Option<SessionId> = None;

    for record in &records {
        let raw = &record.raw;
        match raw.kind.as_str() {
            "user" | "assistant" => {}
            other if IGNORED_TYPES.contains(&other) => {
                *outcome.ignored_records.entry(other.to_owned()).or_insert(0) += 1;
                continue;
            }
            other => {
                *outcome.unknown_records.entry(other.to_owned()).or_insert(0) += 1;
                continue;
            }
        }

        if session.is_none() {
            if let Some(id) = &raw.session_id {
                session = Some(SessionId::new(id.clone()));
                outcome.session.clone_from(&session);
                outcome.checkout = raw.cwd.as_ref().map(PathBuf::from);
                outcome.branch.clone_from(&raw.git_branch);
                outcome.first_time = raw.timestamp.as_ref().map(|t| Timestamp::new(t.clone()));
            }
        }
        let Some(session_id) = session.clone() else {
            *outcome.unknown_records.entry("no-session".to_owned()).or_insert(0) += 1;
            continue;
        };
        let Some(uuid) = &raw.uuid else {
            *outcome.unknown_records.entry(format!("{}-without-uuid", raw.kind)).or_insert(0) += 1;
            continue;
        };
        let turn_id = TurnId::new(uuid.clone());
        let time = Timestamp::new(raw.timestamp.clone().unwrap_or_default());
        let cwd = PathBuf::from(raw.cwd.clone().unwrap_or_default());
        let content = raw.message.as_ref().map(|m| &m.content);

        if raw.kind == "assistant" {
            for tool in pending.drain(..) {
                outcome.abandoned_tool_uses += 1;
                if let Some(observation) = build_abandoned(&tool, &session_id) {
                    staged.push((tool.record_index, observation));
                }
            }
        }

        let role = classify_role(raw, content);
        let excerpt = excerpt_for(role, content);
        staged.push((
            record.index,
            Observation::Turn(Turn {
                id: turn_id.clone(),
                session: session_id.clone(),
                parent: raw.parent_uuid.clone().map(TurnId::new),
                role,
                time: time.clone(),
                sidechain: raw.is_sidechain,
                agent_id: raw.agent_id.clone(),
                excerpt,
                source_ordinal: record.ordinal,
            }),
        ));

        if raw.kind == "assistant" {
            for block in blocks(content) {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    pending.push(PendingTool {
                        id: block.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        name: block.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        input: block.get("input").cloned().unwrap_or(Value::Null),
                        turn: turn_id.clone(),
                        ordinal: record.ordinal,
                        time: time.clone(),
                        cwd: cwd.clone(),
                        record_index: record.index,
                    });
                }
            }
        } else {
            let results: Vec<&Value> = blocks(content)
                .into_iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
                .collect();
            let structured = if results.len() == 1 { raw.tool_use_result.as_ref() } else { None };
            for result in results {
                let id = result.get("tool_use_id").and_then(Value::as_str).unwrap_or_default();
                let is_error = result.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                let Some(position) = pending.iter().position(|p| p.id == id) else {
                    continue;
                };
                let tool = pending.remove(position);
                if let Some(observation) =
                    build_resolved(&tool, &session_id, structured, is_error, &time, &mut outcome)
                {
                    staged.push((record.index, observation));
                }
            }
        }
    }

    // A tool use still waiting for its result at the end blocks consumption
    // from its record onward, so the next parse sees it with its result.
    let cutoff_index = pending.iter().map(|p| p.record_index).min();
    match cutoff_index {
        Some(cut) => {
            outcome.consumed = line_starts[cut] as u64;
            outcome.next_ordinal = start_ordinal + cut as u64;
            staged.retain(|(index, _)| *index < cut);
        }
        None => {
            outcome.consumed = complete_end as u64;
            outcome.next_ordinal = start_ordinal + line_count as u64;
        }
    }
    outcome.observations = staged.into_iter().map(|(_, o)| o).collect();
    outcome
}

fn blocks(content: Option<&Value>) -> Vec<&Value> {
    match content {
        Some(Value::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    }
}

fn classify_role(raw: &RawRecord, content: Option<&Value>) -> Role {
    if raw.kind == "assistant" {
        return Role::Agent;
    }
    if raw.is_meta {
        return Role::Meta;
    }
    let items = blocks(content);
    if !items.is_empty()
        && items
            .iter()
            .all(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
    {
        return Role::Tool;
    }
    Role::Human
}

fn text_of(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn excerpt_for(role: Role, content: Option<&Value>) -> Material<String> {
    match role {
        Role::Human => Material::Retained(text_of(content)),
        Role::Agent => Material::Retained(truncate_chars(&text_of(content), AGENT_EXCERPT_CHARS)),
        Role::Tool => Material::Unavailable { reason: "tool result".to_owned() },
        Role::Meta => Material::Unavailable { reason: "harness metadata".to_owned() },
        Role::Summary => Material::Unavailable { reason: "compaction summary".to_owned() },
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(max)).collect()
}

fn string_field<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a str> {
    value?.get(key)?.as_str()
}

/// The edit's path relative to the checkout, or `None` when it cannot be
/// shown to lie inside it. Containment is lexical: the checkout must be
/// non-empty, the path must start with it, and the remainder may contain
/// only normal components. `/work/repo/../secret` strips to `../secret` and
/// is rejected here. Symlinks are not resolved in this plan; plan 2 checks
/// paths against the git tree, which never contains one that escapes.
fn relative_path(tool: &PendingTool, result: Option<&Value>) -> Option<PathBuf> {
    if tool.cwd.as_os_str().is_empty() {
        return None;
    }
    let path = string_field(Some(&tool.input), "file_path")
        .or_else(|| string_field(result, "filePath"))?;
    let relative = Path::new(path).strip_prefix(&tool.cwd).ok()?;
    let contained = relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    if contained && relative.as_os_str().len() > 0 {
        Some(relative.to_path_buf())
    } else {
        None
    }
}

fn hunks_of(result: Option<&Value>) -> Material<Vec<Hunk>> {
    match result.and_then(|r| r.get("structuredPatch")) {
        Some(value) => match serde_json::from_value::<Vec<RawHunk>>(value.clone()) {
            Ok(raw) => Material::Retained(
                raw.into_iter()
                    .map(|h| Hunk {
                        old_start: h.old_start,
                        old_lines: h.old_lines,
                        new_start: h.new_start,
                        new_lines: h.new_lines,
                        lines: h.lines,
                    })
                    .collect(),
            ),
            Err(_) => Material::Unavailable { reason: "unreadable structured patch".to_owned() },
        },
        None => Material::Unavailable { reason: "no structured patch".to_owned() },
    }
}

fn state_of(text: Option<&str>) -> FileState {
    text.map_or_else(FileState::unknown, |t| FileState::known(ContentId::of(t.as_bytes())))
}

fn build_resolved(
    tool: &PendingTool,
    session: &SessionId,
    result: Option<&Value>,
    is_error: bool,
    ended: &Timestamp,
    outcome: &mut ParseOutcome,
) -> Option<Observation> {
    match tool.name.as_str() {
        "Edit" | "Write" | "MultiEdit" => {
            if is_error {
                // The harness says the tool failed. Whatever the input
                // asked for did not necessarily reach the disk, so there is
                // no edit and no hash to compute.
                outcome.failed_tool_uses += 1;
                return None;
            }
            let Some(path) = relative_path(tool, result) else {
                outcome.outside_checkout += 1;
                return None;
            };
            let original = string_field(result, "originalFile");
            let (before, after, old_text, new_text, replace_all) = match tool.name.as_str() {
                "Edit" => {
                    let old = string_field(Some(&tool.input), "old_string")
                        .or_else(|| string_field(result, "oldString"))
                        .unwrap_or_default();
                    let new = string_field(Some(&tool.input), "new_string")
                        .or_else(|| string_field(result, "newString"))
                        .unwrap_or_default();
                    let all = tool.input.get("replace_all").and_then(Value::as_bool)
                        .or_else(|| result?.get("replaceAll")?.as_bool())
                        .unwrap_or(false);
                    let after = original.and_then(|o| content::apply_edit(o, old, new, all));
                    (
                        state_of(original),
                        state_of(after.as_deref()),
                        Material::Retained(old.to_owned()),
                        Material::Retained(new.to_owned()),
                        all,
                    )
                }
                "Write" => {
                    let text = string_field(Some(&tool.input), "content")
                        .or_else(|| string_field(result, "content"))
                        .unwrap_or_default();
                    let before = if string_field(result, "type") == Some("create") {
                        FileState::Absent
                    } else {
                        state_of(original)
                    };
                    (
                        before,
                        state_of(Some(text)),
                        Material::Unavailable { reason: "write replaces the whole file".to_owned() },
                        Material::Retained(text.to_owned()),
                        false,
                    )
                }
                _ => {
                    let edits: Vec<(String, String, bool)> = tool
                        .input
                        .get("edits")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .map(|e| {
                                    (
                                        string_field(Some(e), "old_string").unwrap_or_default().to_owned(),
                                        string_field(Some(e), "new_string").unwrap_or_default().to_owned(),
                                        e.get("replace_all").and_then(Value::as_bool).unwrap_or(false),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let after = original.and_then(|o| content::apply_multi_edit(o, &edits));
                    (
                        state_of(original),
                        state_of(after.as_deref()),
                        Material::Unavailable { reason: "multi-edit".to_owned() },
                        Material::Unavailable { reason: "multi-edit".to_owned() },
                        false,
                    )
                }
            };
            Some(Observation::Edit(Edit {
                id: EventId::derive("edit", session, &tool.id),
                turn: tool.turn.clone(),
                session: session.clone(),
                path,
                before,
                after,
                patch: hunks_of(result),
                old_text,
                new_text,
                replace_all,
                checkout: tool.cwd.clone(),
                time: ended.clone(),
                source_ordinal: tool.ordinal,
            }))
        }
        "Bash" => {
            let cmd = string_field(Some(&tool.input), "command").unwrap_or_default().to_owned();
            let category = classify_command(&cmd);
            let stdout = string_field(result, "stdout").unwrap_or_default();
            let stderr = string_field(result, "stderr").unwrap_or_default();
            let interrupted = result
                .and_then(|r| r.get("interrupted"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let outcome_value = if category == CommandCategory::TestRun && !interrupted {
                test_outcome(stdout, stderr).or(if is_error { Some(Outcome::Failed) } else { None })
            } else {
                None
            };
            Some(Observation::Command(Command {
                id: EventId::derive("command", session, &tool.id),
                turn: tool.turn.clone(),
                session: session.clone(),
                cmd,
                exit: None,
                stdout_tail: Material::Retained(tail_chars(stdout, OUTPUT_TAIL_CHARS)),
                stderr_tail: Material::Retained(tail_chars(stderr, OUTPUT_TAIL_CHARS)),
                category,
                reported_error: is_error,
                outcome: outcome_value,
                started: tool.time.clone(),
                ended: Some(ended.clone()),
                checkout: tool.cwd.clone(),
                source_ordinal: tool.ordinal,
            }))
        }
        _ => None,
    }
}

fn build_abandoned(tool: &PendingTool, session: &SessionId) -> Option<Observation> {
    if tool.name != "Bash" {
        return None;
    }
    let cmd = string_field(Some(&tool.input), "command").unwrap_or_default().to_owned();
    let category = classify_command(&cmd);
    let reason = "no result recorded".to_owned();
    Some(Observation::Command(Command {
        id: EventId::derive("command", session, &tool.id),
        turn: tool.turn.clone(),
        session: session.clone(),
        cmd,
        exit: None,
        stdout_tail: Material::Unavailable { reason: reason.clone() },
        stderr_tail: Material::Unavailable { reason },
        category,
        reported_error: false,
        outcome: None,
        started: tool.time.clone(),
        ended: None,
        checkout: tool.cwd.clone(),
        source_ordinal: tool.ordinal,
    }))
}

/// Coarse classification of a shell command line.
#[must_use]
pub fn classify_command(cmd: &str) -> CommandCategory {
    let lower = cmd.to_lowercase();
    const TESTS: &[&str] = &[
        "nextest", "cargo test", "vitest", "pnpm test", "npm test", "pytest", "go test", "jest",
    ];
    const BUILDS: &[&str] = &[
        "cargo build", "cargo check", "cargo clippy", "tsc", "pnpm build", "npm run build",
    ];
    if TESTS.iter().any(|t| lower.contains(t)) {
        CommandCategory::TestRun
    } else if lower.starts_with("git ") || lower.contains("&& git ") || lower.starts_with("gh ") {
        CommandCategory::Git
    } else if BUILDS.iter().any(|b| lower.contains(b)) {
        CommandCategory::Build
    } else {
        CommandCategory::Other
    }
}

/// Reads a pass or fail verdict from cargo test or nextest output.
///
/// Failure takes precedence: a workspace run prints one `test result:` line
/// per binary, and one failing binary after several passing ones is a
/// failed run. `None` means no verdict could be read, not success.
#[must_use]
pub fn test_outcome(stdout: &str, stderr: &str) -> Option<Outcome> {
    let text = format!("{stdout}\n{stderr}");
    let nextest_summary = text
        .lines()
        .find(|l| l.contains("tests run:") || l.contains("test run:"));
    let failed = text.contains("test result: FAILED")
        || nextest_summary.is_some_and(|l| l.contains("failed"))
        || text.contains("FAILED");
    if failed {
        return Some(Outcome::Failed);
    }
    if text.contains("test result: ok") || nextest_summary.is_some() {
        return Some(Outcome::Passed);
    }
    None
}
```

In `crates/supersigil-session/src/lib.rs` add `pub mod claude_code;` and `pub use claude_code::{ParseOutcome, parse_transcript};`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-session && cargo clippy -p supersigil-session --all-targets --all-features`
Expected: 17 fixture tests and 3 content tests pass, no warnings. Likely clippy findings and their fixes: `cast_possible_truncation` does not fire for `usize as u64`; if `too_many_lines` fires on `parse_transcript` or `build_resolved`, split the `Edit`/`Write`/`MultiEdit` arms into three private functions returning the five-tuple rather than adding an allow.

- [ ] **Step 7: Commit**

```bash
git add crates/supersigil-session/src/claude_code.rs crates/supersigil-session/src/claude_code/content.rs crates/supersigil-session/src/lib.rs crates/supersigil-session/tests/fixtures/slice.jsonl crates/supersigil-session/tests/parse_fixture.rs
git commit -m "feat: parse Claude Code transcripts into observations"
```

---
### Task 7: Transcript discovery and incremental sync

**Files:**
- Create: `crates/supersigil-session/src/discover.rs`, `crates/supersigil-session/src/sync.rs`
- Modify: `crates/supersigil-session/src/lib.rs`
- Test: inline `#[cfg(test)]` in `discover.rs`; `crates/supersigil-session/tests/sync_fixture.rs`

**Interfaces:**
- Consumes: `parse_transcript`, `Store`, `WriteTx::{add_association, append_observations, put_derivations, set_cursor, commit}`, `RecordSnapshot::observations`, `derive::derive`, `SourceCursor`, `Association`.
- Produces:
  - `discover::encode_project_dir(checkout: &Path) -> String`: every `/`, `\`, and `.` becomes `-`, so `/home/joni/.local/src/supersigil` becomes `-home-joni--local-src-supersigil`.
  - `discover::discover_transcripts(checkout: &Path, claude_home: &Path) -> Vec<PathBuf>`: sorted `*.jsonl` files under `<claude_home>/projects/<encoded>/`, empty if the directory does not exist.
  - `sync::sync(store: &Store, checkout: &Path, transcripts: &[PathBuf]) -> Result<SyncReport, SyncError>`.
  - `SyncReport { revision: Revision, sessions: Vec<SessionId>, new_observations: usize, transcripts: Vec<TranscriptReport> }`.
  - `TranscriptReport { path: PathBuf, session: Option<SessionId>, new_observations: usize, consumed: u64, trailing_partial: bool, unknown_records: BTreeMap<String, u64>, malformed_lines: u64, abandoned_tool_uses: u64, failed_tool_uses: u64, outside_checkout: u64 }`.
  - `SyncError::{Store(StoreError), Io { path, source }}`.

Sync rules: one transaction per call, and the input snapshot is taken from that transaction after the lock is held, never before, so derivations are computed from exactly the observations the manifest pins plus the ones this call appends; the cursor key is the transcript path as given; a transcript shorter than its cursor is treated as rewritten and read from the start; `SessionStart` is emitted once, when a cursor first learns its session; derivations are recomputed for every session that received observations, from the pinned observations plus the ones appended in this transaction; a call that appends nothing and adds no association commits nothing and reports the current revision.

- [ ] **Step 1: Write the failing tests**

`crates/supersigil-session/src/discover.rs` test module (implementation goes above it in step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_separators_and_dots_as_dashes() {
        assert_eq!(
            encode_project_dir(Path::new("/home/joni/.local/src/supersigil")),
            "-home-joni--local-src-supersigil"
        );
        assert_eq!(encode_project_dir(Path::new("/work/repo")), "-work-repo");
    }

    #[test]
    fn discovers_sorted_jsonl_files_for_the_checkout_only() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("projects").join("-work-repo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.jsonl"), "").unwrap();
        std::fs::write(dir.join("a.jsonl"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        let other = home.path().join("projects").join("-work-other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("c.jsonl"), "").unwrap();

        let found = discover_transcripts(Path::new("/work/repo"), home.path());
        assert_eq!(found, vec![dir.join("a.jsonl"), dir.join("b.jsonl")]);
        assert!(discover_transcripts(Path::new("/nowhere"), home.path()).is_empty());
    }
}
```

`crates/supersigil-session/tests/sync_fixture.rs`:

```rust
//! Incremental sync of the slice fixture into a record.

use std::path::{Path, PathBuf};

use supersigil_record::observations::{Observation, Role, Source};
use supersigil_record::store::{Association, Store};
use supersigil_record::{EventId, SessionId};
use supersigil_session::sync::sync;

const SESSION: &str = "11111111-1111-4111-8111-111111111111";

fn fixture() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slice.jsonl")).unwrap()
}

fn line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && i + 1 < bytes.len() {
            starts.push(i + 1);
        }
    }
    starts
}

struct Setup {
    _dir: tempfile::TempDir,
    store: Store,
    transcript: PathBuf,
    checkout: PathBuf,
}

fn setup(bytes: &[u8]) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let checkout = PathBuf::from("/work/repo");
    let store = Store::create(&dir.path().join("record"), Association { checkout: checkout.clone() }).unwrap();
    let transcript = dir.path().join("slice.jsonl");
    std::fs::write(&transcript, bytes).unwrap();
    Setup { _dir: dir, store, transcript, checkout }
}

fn turn_ids(store: &Store) -> Vec<String> {
    store
        .snapshot()
        .unwrap()
        .observations(&SessionId::new(SESSION))
        .unwrap()
        .iter()
        .filter_map(|o| match o {
            Observation::Turn(t) => Some(t.id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

fn command_count(store: &Store) -> usize {
    store
        .snapshot()
        .unwrap()
        .observations(&SessionId::new(SESSION))
        .unwrap()
        .iter()
        .filter(|o| matches!(o, Observation::Command(_)))
        .count()
}

#[test]
fn sync_writes_session_start_observations_and_derivations() {
    let s = setup(&fixture());
    let report = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(report.revision.get(), 1);
    assert_eq!(report.sessions, vec![SessionId::new(SESSION)]);
    assert_eq!(report.new_observations, 25);
    assert_eq!(report.transcripts.len(), 1);
    assert!(!report.transcripts[0].trailing_partial);
    assert_eq!(report.transcripts[0].unknown_records.get("ai-title"), Some(&1));
    assert_eq!(report.transcripts[0].failed_tool_uses, 0);

    let snapshot = s.store.snapshot().unwrap();
    let observations = snapshot.observations(&SessionId::new(SESSION)).unwrap();
    assert_eq!(observations.len(), 25);
    let Observation::SessionStart(start) = &observations[0] else {
        panic!("first observation must be the session start");
    };
    assert_eq!(start.source, Source::ClaudeCode);
    assert_eq!(start.checkout, PathBuf::from("/work/repo"));
    assert_eq!(start.branch.as_deref(), Some("main"));
    assert_eq!(start.time.as_str(), "2026-09-28T10:00:00.000Z");
    assert_eq!(start.source_ids.get("transcript").map(String::as_str), Some("slice.jsonl"));
    assert!(matches!(&observations[1], Observation::Turn(t) if t.role == Role::Human));

    let derivations = snapshot.derivations(&SessionId::new(SESSION)).unwrap().unwrap();
    let session = SessionId::new(SESSION);
    assert_eq!(derivations.observation_revision.get(), 1);
    assert_eq!(derivations.restores.len(), 1);
    assert_eq!(derivations.restores[0].edit, EventId::derive("edit", &session, "toolu_04"));
    assert_eq!(derivations.restores[0].restores, vec![EventId::derive("edit", &session, "toolu_03")]);
    assert_eq!(derivations.discontinuities.len(), 1);
    assert_eq!(derivations.discontinuities[0].path, PathBuf::from("src/lib.rs"));
    assert_eq!(derivations.discontinuities[0].prev, EventId::derive("edit", &session, "toolu_01"));
    assert_eq!(derivations.discontinuities[0].next, EventId::derive("edit", &session, "toolu_05"));

    let cursor = &snapshot.manifest().cursors[&s.transcript.display().to_string()];
    assert_eq!(cursor.offset, fixture().len() as u64);
    assert_eq!(cursor.next_ordinal, 19);
    assert_eq!(cursor.session, Some(session));
}

#[test]
fn sync_is_idempotent() {
    let s = setup(&fixture());
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    let again = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(again.new_observations, 0);
    assert_eq!(again.revision.get(), 1);
    assert_eq!(s.store.manifest().unwrap().revision.get(), 1);
    assert_eq!(turn_ids(&s.store).len(), 17);
}

#[test]
fn sync_resumes_after_partial_line() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let s = setup(&bytes[..starts[18] + 40]);
    let first = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert!(first.transcripts[0].trailing_partial);
    assert_eq!(first.transcripts[0].consumed, starts[18] as u64);
    assert_eq!(turn_ids(&s.store).len(), 16);

    std::fs::write(&s.transcript, &bytes).unwrap();
    let second = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(second.new_observations, 1);
    assert_eq!(second.revision.get(), 2);
    let ids = turn_ids(&s.store);
    assert_eq!(ids.len(), 17);
    assert_eq!(ids.last().map(String::as_str), Some("a8"));
    let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), 17);
    assert_eq!(
        s.store.snapshot().unwrap().derivations(&SessionId::new(SESSION)).unwrap().unwrap().observation_revision.get(),
        2
    );
}

#[test]
fn sync_waits_for_a_pending_tool_result() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let s = setup(&bytes[..starts[15]]);
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    let cursor_offset = s.store.manifest().unwrap().cursors[&s.transcript.display().to_string()].offset;
    assert_eq!(cursor_offset, starts[14] as u64);
    assert_eq!(command_count(&s.store), 1);

    std::fs::write(&s.transcript, &bytes).unwrap();
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    assert_eq!(command_count(&s.store), 2);
    let ids = turn_ids(&s.store);
    let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(ids.len(), 17);
    assert_eq!(unique.len(), 17);
}

#[test]
fn rewritten_shorter_transcript_is_read_from_the_start() {
    let bytes = fixture();
    let starts = line_starts(&bytes);
    let s = setup(&bytes);
    sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    std::fs::write(&s.transcript, &bytes[..starts[3]]).unwrap();
    let report = sync(&s.store, &s.checkout, std::slice::from_ref(&s.transcript)).unwrap();
    // The three records are appended again under the same ids; the log is
    // append-only, so duplicates are visible and later layers dedupe by id.
    assert_eq!(report.transcripts[0].consumed, starts[3] as u64);
    assert_eq!(s.store.manifest().unwrap().cursors[&s.transcript.display().to_string()].offset, starts[3] as u64);
}

#[test]
fn missing_transcript_is_an_io_error() {
    let s = setup(&fixture());
    let missing = s.transcript.with_file_name("missing.jsonl");
    assert!(matches!(
        sync(&s.store, &s.checkout, &[missing]),
        Err(supersigil_session::sync::SyncError::Io { .. })
    ));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil-session`
Expected: compile errors, `discover` and `sync` not found.

- [ ] **Step 3: Implement discovery**

`crates/supersigil-session/src/discover.rs` (above the test module):

```rust
//! Finds Claude Code transcripts for a checkout without a hook.
//!
//! Claude Code stores transcripts under `~/.claude/projects/<encoded cwd>/`,
//! where the encoding replaces path separators and dots with dashes. Hooks
//! hand over `transcript_path` directly; this module is the fallback for
//! sessions recorded before the hook existed.

use std::path::{Path, PathBuf};

/// Encodes a checkout path the way Claude Code names its project directories.
#[must_use]
pub fn encode_project_dir(checkout: &Path) -> String {
    checkout
        .to_string_lossy()
        .chars()
        .map(|c| if c == '/' || c == '\\' || c == '.' { '-' } else { c })
        .collect()
}

/// Sorted `.jsonl` transcripts for `checkout` under `claude_home`
/// (normally `~/.claude`). Empty when the project directory does not exist.
#[must_use]
pub fn discover_transcripts(checkout: &Path, claude_home: &Path) -> Vec<PathBuf> {
    let dir = claude_home.join("projects").join(encode_project_dir(checkout));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    found.sort();
    found
}
```

- [ ] **Step 4: Implement sync**

`crates/supersigil-session/src/sync.rs`:

```rust
//! Incremental sync of transcripts into a record.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use supersigil_record::derive::derive;
use supersigil_record::observations::{Observation, SessionStart, Source};
use supersigil_record::store::{Association, SourceCursor, Store, StoreError};
use supersigil_record::{Revision, SessionId};

use crate::claude_code::parse_transcript;

/// Errors from sync.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// The store failed.
    #[error("{0}")]
    Store(#[from] StoreError),
    /// A transcript could not be read.
    #[error("cannot read transcript {path}: {source}")]
    Io {
        /// Transcript path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// What one transcript contributed to a sync.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TranscriptReport {
    /// Transcript path as given.
    pub path: PathBuf,
    /// Session the transcript belongs to, once known.
    pub session: Option<SessionId>,
    /// Observations appended from this transcript.
    pub new_observations: usize,
    /// Byte offset the cursor now points at.
    pub consumed: u64,
    /// Whether the file ended in an incomplete line.
    pub trailing_partial: bool,
    /// Unknown record types skipped, by type.
    pub unknown_records: BTreeMap<String, u64>,
    /// Lines that were not valid JSON.
    pub malformed_lines: u64,
    /// Tool uses the agent moved past without a result.
    pub abandoned_tool_uses: u64,
    /// Editing tool uses the harness reported as failed; no edit was recorded.
    pub failed_tool_uses: u64,
    /// Edits outside the checkout that were dropped.
    pub outside_checkout: u64,
}

/// Result of one sync call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SyncReport {
    /// Record revision after the call.
    pub revision: Revision,
    /// Sessions that received observations.
    pub sessions: Vec<SessionId>,
    /// Observations appended in total.
    pub new_observations: usize,
    /// Per-transcript detail.
    pub transcripts: Vec<TranscriptReport>,
}

/// Appends new observations from `transcripts` to `store`, recomputes
/// derivations for every touched session, and commits one revision.
///
/// # Errors
///
/// Returns [`SyncError::Io`] if a transcript cannot be read, or the store's
/// error if the record is locked or cannot be written.
pub fn sync(store: &Store, checkout: &Path, transcripts: &[PathBuf]) -> Result<SyncReport, SyncError> {
    let mut tx = store.begin()?;
    // Taken under the lock. Derivations below are computed from this
    // snapshot plus the observations this transaction appends, so another
    // writer's commit cannot slip in between reading and deriving.
    let pinned = tx.snapshot();
    let association = Association { checkout: checkout.to_path_buf() };
    let association_is_new = !tx.manifest().associations.iter().any(|a| a.checkout == association.checkout);
    tx.add_association(association);

    let mut reports = Vec::new();
    let mut appended: BTreeMap<SessionId, Vec<Observation>> = BTreeMap::new();
    let mut total = 0usize;

    for path in transcripts {
        let key = path.display().to_string();
        let bytes = std::fs::read(path).map_err(|source| SyncError::Io { path: path.clone(), source })?;
        let mut cursor = tx.manifest().cursors.get(&key).cloned().unwrap_or(SourceCursor {
            offset: 0,
            next_ordinal: 0,
            session: None,
        });
        if (bytes.len() as u64) < cursor.offset {
            cursor = SourceCursor { offset: 0, next_ordinal: 0, session: None };
        }
        let outcome = parse_transcript(&bytes[cursor.offset as usize..], cursor.next_ordinal);

        let mut new = Vec::new();
        if cursor.session.is_none()
            && let Some(session) = &outcome.session
        {
            new.push(Observation::SessionStart(SessionStart {
                session: session.clone(),
                source: Source::ClaudeCode,
                source_ids: BTreeMap::from([(
                    "transcript".to_owned(),
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                )]),
                checkout: outcome.checkout.clone().unwrap_or_else(|| checkout.to_path_buf()),
                branch: outcome.branch.clone(),
                time: outcome.first_time.clone().unwrap_or_else(|| supersigil_record::Timestamp::new("")),
            }));
            cursor.session = Some(session.clone());
        }
        new.extend(outcome.observations);
        let count = new.len();
        if count > 0 {
            tx.append_observations(&new)?;
            for observation in new {
                appended.entry(observation.session().clone()).or_default().push(observation);
            }
        }
        total += count;
        cursor.offset += outcome.consumed;
        cursor.next_ordinal = outcome.next_ordinal;
        tx.set_cursor(&key, cursor.clone());
        reports.push(TranscriptReport {
            path: path.clone(),
            session: cursor.session,
            new_observations: count,
            consumed: outcome.consumed,
            trailing_partial: outcome.trailing_partial,
            unknown_records: outcome.unknown_records,
            malformed_lines: outcome.malformed_lines,
            abandoned_tool_uses: outcome.abandoned_tool_uses,
            failed_tool_uses: outcome.failed_tool_uses,
            outside_checkout: outcome.outside_checkout,
        });
    }

    if total == 0 && !association_is_new {
        return Ok(SyncReport {
            revision: pinned.revision(),
            sessions: Vec::new(),
            new_observations: 0,
            transcripts: reports,
        });
    }

    let next_revision = tx.manifest().revision.next();
    let touched: BTreeSet<SessionId> = appended.keys().cloned().collect();
    for session in &touched {
        let mut observations = pinned.observations(session)?;
        observations.extend(appended.remove(session).unwrap_or_default());
        let set = derive(session, &observations, next_revision);
        tx.put_derivations(&set)?;
    }
    let revision = tx.commit()?;
    Ok(SyncReport {
        revision,
        sessions: touched.into_iter().collect(),
        new_observations: total,
        transcripts: reports,
    })
}
```

The `consumed` field in a `TranscriptReport` is the parser's consumed count for this call, relative to the cursor's previous offset; the tests compare it against absolute offsets only when the previous offset was zero.

In `crates/supersigil-session/src/lib.rs` add `pub mod discover;`, `pub mod sync;`, and `pub use sync::{SyncError, SyncReport, TranscriptReport, sync};`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil-session && cargo clippy -p supersigil-session --all-targets --all-features`
Expected: all pass, no warnings. `cast_possible_truncation` fires on `cursor.offset as usize`: replace it with `usize::try_from(cursor.offset).unwrap_or(usize::MAX).min(bytes.len())` and add a one-line comment that a transcript larger than the address space is not a case this tool handles.

- [ ] **Step 6: Commit**

```bash
git add crates/supersigil-session/src/discover.rs crates/supersigil-session/src/sync.rs crates/supersigil-session/src/lib.rs crates/supersigil-session/tests/sync_fixture.rs
git commit -m "feat: sync Claude Code transcripts into the record"
```

---
### Task 8: CLI `session sync`, `session list`, `session show`

**Files:**
- Modify: `crates/supersigil-cli/Cargo.toml`, `crates/supersigil-cli/src/commands.rs`, `crates/supersigil-cli/src/error.rs`, `crates/supersigil-cli/src/lib.rs`, `crates/supersigil-cli/src/main.rs`
- Create: `crates/supersigil-cli/src/record_dir.rs`, `crates/supersigil-cli/src/commands/session.rs`
- Test: `crates/supersigil-cli/tests/cmd_session.rs`

**Interfaces:**
- Consumes: `Store::{create_with_id, open, manifest, snapshot}`, `RecordId::generate`, `Association`, `sync::sync`, `discover::discover_transcripts`, `RecordSnapshot::{sessions, observations, derivations}`, existing `OutputFormat`, `ColorConfig`, `write_json`, `hint`.
- Produces:
  - `Command::Session(SessionArgs)` with `SessionArgs { record_dir: Option<PathBuf>, checkout: Option<PathBuf>, command: SessionCommand }`, `SessionCommand::{Sync(SessionSyncArgs), List(SessionListArgs), Show(SessionShowArgs)}`, `SessionSyncArgs { transcripts: Vec<PathBuf>, claude_home: Option<PathBuf>, format: OutputFormat }`, `SessionListArgs { format: OutputFormat }`, `SessionShowArgs { session: String }`.
  - `record_dir::resolve_record_dir(flag: Option<&Path>) -> Result<PathBuf, CliError>`: flag, then `SUPERSIGIL_RECORD_DIR`, then `$XDG_DATA_HOME/supersigil/records`, then `$HOME/.local/share/supersigil/records`.
  - `record_dir::canonical_checkout(flag: Option<&Path>) -> Result<PathBuf, CliError>`.
  - `record_dir::find_record(records_dir, checkout) -> Result<Option<Store>, CliError>`: exactly one association wins; two records claiming the same checkout is an error that names both, never a silent pick. `open_or_create_record(records_dir, checkout) -> Result<Store, CliError>` holds an exclusive lock on `<records_dir>/.lock` from lookup through creation, so two first syncs cannot each create a record; a new record lives at `<records_dir>/<record id>`.
  - `commands::session::run(args: &SessionArgs, color: ColorConfig) -> Result<(), CliError>`.
  - `CliError::Record(StoreError)`, `CliError::Session(SyncError)`.
  - JSON shapes: `session sync --format json` prints the `SyncReport`; `session list --format json` prints `[{ "session", "started", "branch", "checkout", "turns", "edits", "commands", "restores", "discontinuities" }]`; `session show <id>` prints `{ "session", "revision", "observations": [...], "derivations": {...} | null }`.

After plan 0, `main.rs` has a single `match` over `Command`; the `session` group is one more arm and needs no configuration file.

- [ ] **Step 1: Write the failing CLI tests**

`crates/supersigil-cli/tests/cmd_session.rs`:

```rust
//! `supersigil session` against the slice fixture.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use predicates::prelude::*;

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../supersigil-session/tests/fixtures/slice.jsonl")
        .canonicalize()
        .unwrap()
}

struct Env {
    _dir: tempfile::TempDir,
    records: PathBuf,
    checkout: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let records = dir.path().join("records");
    let checkout = dir.path().join("checkout");
    std::fs::create_dir_all(&checkout).unwrap();
    Env { _dir: dir, records, checkout }
}

fn session_cmd(env: &Env) -> Command {
    let mut cmd = common::supersigil_cmd();
    cmd.env("SUPERSIGIL_RECORD_DIR", &env.records)
        .env("HOME", env.checkout.parent().unwrap())
        .env_remove("XDG_DATA_HOME")
        .current_dir(&env.checkout)
        .arg("session");
    cmd
}

#[test]
fn sync_with_explicit_transcript_creates_a_record() {
    let e = env();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success()
        .stdout(predicate::str::contains("25 observations"))
        .stdout(predicate::str::contains("revision 1"))
        .stdout(predicate::str::contains("unknown record types: ai-title (1)"));

    let records: Vec<_> = std::fs::read_dir(&e.records).unwrap().flatten().collect();
    assert_eq!(records.len(), 1);
    assert!(records[0].path().join("manifest.json").exists());
}

#[test]
fn sync_twice_reports_nothing_new() {
    let e = env();
    session_cmd(&e).args(["sync", "--transcript"]).arg(fixture_path()).assert().success();
    session_cmd(&e)
        .args(["sync", "--transcript"])
        .arg(fixture_path())
        .assert()
        .success()
        .stdout(predicate::str::contains("0 observations"))
        .stdout(predicate::str::contains("revision 1"));
}

#[test]
fn sync_json_prints_the_report() {
    let e = env();
    let output = session_cmd(&e)
        .args(["sync", "--format", "json", "--transcript"])
        .arg(fixture_path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["new_observations"], 25);
    assert_eq!(report["revision"], 1);
    assert_eq!(report["transcripts"][0]["trailing_partial"], false);
}

#[test]
fn sync_without_transcripts_hints_and_succeeds() {
    let e = env();
    session_cmd(&e)
        .args(["sync", "--claude-home"])
        .arg(e.checkout.join("no-such-claude-home"))
        .assert()
        .success()
        .stderr(predicate::str::contains("no transcripts found"))
        .stderr(predicate::str::contains("--transcript"));
}

#[test]
fn list_before_any_sync_says_so() {
    let e = env();
    session_cmd(&e)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("no record for"));
}

#[test]
fn list_json_summarizes_sessions() {
    let e = env();
    session_cmd(&e).args(["sync", "--transcript"]).arg(fixture_path()).assert().success();
    let output = session_cmd(&e).args(["list", "--format", "json"]).output().unwrap();
    assert!(output.status.success());
    let list: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    let entry = &list[0];
    assert_eq!(entry["session"], "11111111-1111-4111-8111-111111111111");
    assert_eq!(entry["started"], "2026-09-28T10:00:00.000Z");
    assert_eq!(entry["branch"], "main");
    assert_eq!(entry["checkout"], "/work/repo");
    assert_eq!(entry["turns"], 17);
    assert_eq!(entry["edits"], 5);
    assert_eq!(entry["commands"], 2);
    assert_eq!(entry["restores"], 1);
    assert_eq!(entry["discontinuities"], 1);
}

#[test]
fn list_terminal_prints_one_row_per_session() {
    let e = env();
    session_cmd(&e).args(["sync", "--transcript"]).arg(fixture_path()).assert().success();
    session_cmd(&e)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("11111111"))
        .stdout(predicate::str::contains("17 turns"))
        .stdout(predicate::str::contains("5 edits"));
}

#[test]
fn show_accepts_a_unique_prefix_and_prints_everything() {
    let e = env();
    session_cmd(&e).args(["sync", "--transcript"]).arg(fixture_path()).assert().success();
    let output = session_cmd(&e).args(["show", "1111"]).output().unwrap();
    assert!(output.status.success());
    let shown: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(shown["session"], "11111111-1111-4111-8111-111111111111");
    assert_eq!(shown["revision"], 1);
    assert_eq!(shown["observations"].as_array().unwrap().len(), 25);
    assert_eq!(shown["observations"][0]["kind"], "session_start");
    assert_eq!(shown["derivations"]["restores"].as_array().unwrap().len(), 1);
}

#[test]
fn show_unknown_session_fails() {
    let e = env();
    session_cmd(&e).args(["sync", "--transcript"]).arg(fixture_path()).assert().success();
    session_cmd(&e)
        .args(["show", "zzz"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no session matching"));
}

#[test]
fn session_commands_need_no_config_file() {
    let e = env();
    assert!(!e.checkout.join("supersigil.toml").exists());
    session_cmd(&e).arg("list").assert().success();
}

#[test]
fn two_records_for_one_checkout_is_an_error_that_names_both() {
    use supersigil_record::store::{Association, Store};
    let e = env();
    session_cmd(&e).args(["sync", "--transcript"]).arg(fixture_path()).assert().success();
    let checkout = e.checkout.canonicalize().unwrap();
    Store::create(&e.records.join("second"), Association { checkout }).unwrap();
    let first = std::fs::read_dir(&e.records)
        .unwrap()
        .flatten()
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .find(|n| n != "second" && n != ".lock")
        .unwrap();
    session_cmd(&e)
        .arg("list")
        .assert()
        .failure()
        .stderr(predicate::str::contains("several records"))
        .stderr(predicate::str::contains(&first))
        .stderr(predicate::str::contains("second"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p supersigil --test cmd_session`
Expected: every test fails; clap reports `unrecognized subcommand 'session'`.

- [ ] **Step 3: Wire dependencies and errors**

`crates/supersigil-cli/Cargo.toml`, under `[dependencies]` after `thiserror.workspace = true`:

```toml
supersigil-record.workspace = true
supersigil-session.workspace = true
```

`crates/supersigil-cli/src/error.rs`, add two variants before `Io`:

```rust
    /// Record store error.
    #[error("{0}")]
    Record(#[from] supersigil_record::StoreError),
    /// Session sync error.
    #[error("{0}")]
    Session(#[from] supersigil_session::SyncError),
```

- [ ] **Step 4: Add the argument structs**

In `crates/supersigil-cli/src/commands.rs`, add a variant to `Command`:

```rust
    /// Sync and inspect agent sessions recorded for this checkout
    Session(SessionArgs),
```

and the structs:

```rust
/// Arguments for the `session` command group.
#[derive(Debug, clap::Args)]
pub struct SessionArgs {
    /// Records directory (default: $SUPERSIGIL_RECORD_DIR, then $XDG_DATA_HOME/supersigil/records)
    #[arg(long, global = true)]
    pub record_dir: Option<PathBuf>,
    /// Checkout to operate on (default: current directory)
    #[arg(long, global = true)]
    pub checkout: Option<PathBuf>,
    /// Subcommand
    #[command(subcommand)]
    pub command: SessionCommand,
}

/// Subcommands of `session`.
#[derive(Debug, clap::Subcommand)]
pub enum SessionCommand {
    /// Append new observations from transcripts to this checkout's record
    Sync(SessionSyncArgs),
    /// List sessions in this checkout's record
    List(SessionListArgs),
    /// Print one session's observations and derivations as JSON
    Show(SessionShowArgs),
}

/// Arguments for `session sync`.
#[derive(Debug, clap::Args)]
pub struct SessionSyncArgs {
    /// Transcript file to read (repeatable). Without it, transcripts are discovered.
    #[arg(long = "transcript")]
    pub transcripts: Vec<PathBuf>,
    /// Claude home directory used for discovery (default: ~/.claude)
    #[arg(long)]
    pub claude_home: Option<PathBuf>,
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

/// Arguments for `session list`.
#[derive(Debug, clap::Args)]
pub struct SessionListArgs {
    /// Output format
    #[arg(long, default_value = "terminal")]
    pub format: OutputFormat,
}

/// Arguments for `session show`.
#[derive(Debug, clap::Args)]
pub struct SessionShowArgs {
    /// Session id; a unique prefix is enough
    pub session: String,
}
```

Add `pub mod session;` next to the other command modules in `commands.rs`, and add `SessionArgs, SessionCommand, SessionListArgs, SessionShowArgs, SessionSyncArgs` to the `pub use commands::{...}` list in `lib.rs`, plus `pub mod record_dir;`.

- [ ] **Step 5: Implement record directory resolution**

`crates/supersigil-cli/src/record_dir.rs`:

```rust
//! Where records live and which record belongs to a checkout.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use supersigil_record::RecordId;
use supersigil_record::store::{Association, Store};

use crate::error::CliError;

/// Resolves the records directory: the flag, then `SUPERSIGIL_RECORD_DIR`,
/// then `$XDG_DATA_HOME/supersigil/records`, then
/// `$HOME/.local/share/supersigil/records`.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] if neither `XDG_DATA_HOME` nor `HOME`
/// is set and no flag was given.
pub fn resolve_record_dir(flag: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(dir) = flag {
        return Ok(dir.to_path_buf());
    }
    if let Some(dir) = std::env::var_os("SUPERSIGIL_RECORD_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg).join("supersigil").join("records"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".local/share/supersigil/records"));
    }
    Err(CliError::CommandFailed(
        "cannot locate the records directory: set SUPERSIGIL_RECORD_DIR or HOME".to_owned(),
    ))
}

/// The checkout to operate on, canonicalized.
///
/// # Errors
///
/// Returns an I/O error if the path does not exist.
pub fn canonical_checkout(flag: Option<&Path>) -> Result<PathBuf, CliError> {
    let path = match flag {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Ok(path.canonicalize()?)
}

/// Finds the record associated with `checkout`, if any. Two records
/// claiming the same checkout is an error that names both; picking one
/// silently would hide evidence written to the other.
///
/// # Errors
///
/// Returns a store error if a record directory cannot be read, or
/// [`CliError::CommandFailed`] on duplicate associations.
pub fn find_record(records_dir: &Path, checkout: &Path) -> Result<Option<Store>, CliError> {
    let Ok(entries) = std::fs::read_dir(records_dir) else {
        return Ok(None);
    };
    let mut candidates: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    candidates.sort();
    let mut matches = Vec::new();
    for candidate in candidates {
        let Ok(store) = Store::open(&candidate) else {
            continue;
        };
        if store.manifest()?.associations.iter().any(|a| a.checkout == checkout) {
            matches.push(store);
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => {
            let roots: Vec<String> = matches.iter().map(|s| s.root().display().to_string()).collect();
            Err(CliError::CommandFailed(format!(
                "{} is associated with several records: {}. Remove or merge all but one.",
                checkout.display(),
                roots.join(", ")
            )))
        }
    }
}

/// Finds the record for `checkout` or creates one at `<records_dir>/<id>`.
/// Lookup and creation happen under an exclusive lock on
/// `<records_dir>/.lock`, so two first syncs racing on one checkout cannot
/// both create a record.
///
/// # Errors
///
/// Returns a store error if the record cannot be created or read, or an I/O
/// error if the records directory or its lock cannot be created.
pub fn open_or_create_record(records_dir: &Path, checkout: &Path) -> Result<Store, CliError> {
    std::fs::create_dir_all(records_dir)?;
    let guard = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(records_dir.join(".lock"))?;
    guard.lock()?;
    let store = match find_record(records_dir, checkout)? {
        Some(store) => store,
        None => {
            let id = RecordId::generate();
            let root = records_dir.join(id.as_str());
            Store::create_with_id(&root, id, Association { checkout: checkout.to_path_buf() })?
        }
    };
    drop(guard);
    Ok(store)
}
```

- [ ] **Step 6: Implement the command module**

`crates/supersigil-cli/src/commands/session.rs`:

```rust
//! `supersigil session`: sync transcripts into the record and inspect it.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use supersigil_record::observations::Observation;
use supersigil_record::store::Store;
use supersigil_record::{DerivationSet, SessionId};
use supersigil_session::discover::discover_transcripts;
use supersigil_session::sync::{SyncReport, sync};

use crate::commands::{SessionArgs, SessionCommand, SessionListArgs, SessionShowArgs, SessionSyncArgs};
use crate::error::CliError;
use crate::format::{ColorConfig, OutputFormat, hint, write_json};
use crate::record_dir;

/// One row of `session list`.
#[derive(Debug, Serialize)]
pub struct SessionSummary {
    /// Session id.
    pub session: SessionId,
    /// Time of the session start, if recorded.
    pub started: Option<String>,
    /// Branch at session start.
    pub branch: Option<String>,
    /// Checkout the session worked in.
    pub checkout: Option<PathBuf>,
    /// Number of turns.
    pub turns: usize,
    /// Number of edits.
    pub edits: usize,
    /// Number of commands.
    pub commands: usize,
    /// Number of derived restores.
    pub restores: usize,
    /// Number of derived discontinuities.
    pub discontinuities: usize,
}

#[derive(Debug, Serialize)]
struct ShowOutput {
    session: SessionId,
    revision: u64,
    observations: Vec<Observation>,
    derivations: Option<DerivationSet>,
}

/// Runs the `session` command group.
///
/// # Errors
///
/// Returns `CliError` if the record cannot be located, read, or written.
pub fn run(args: &SessionArgs, color: ColorConfig) -> Result<(), CliError> {
    let records_dir = record_dir::resolve_record_dir(args.record_dir.as_deref())?;
    let checkout = record_dir::canonical_checkout(args.checkout.as_deref())?;
    match &args.command {
        SessionCommand::Sync(sync_args) => run_sync(sync_args, &records_dir, &checkout, color),
        SessionCommand::List(list_args) => run_list(list_args, &records_dir, &checkout),
        SessionCommand::Show(show_args) => run_show(show_args, &records_dir, &checkout),
    }
}

fn claude_home(flag: Option<&Path>) -> Option<PathBuf> {
    flag.map(Path::to_path_buf)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")))
}

fn run_sync(
    args: &SessionSyncArgs,
    records_dir: &Path,
    checkout: &Path,
    color: ColorConfig,
) -> Result<(), CliError> {
    let transcripts: Vec<PathBuf> = if args.transcripts.is_empty() {
        claude_home(args.claude_home.as_deref())
            .map(|home| discover_transcripts(checkout, &home))
            .unwrap_or_default()
    } else {
        args.transcripts.clone()
    };
    if transcripts.is_empty() {
        hint(
            color,
            &format!(
                "no transcripts found for {}; pass --transcript <path> or --claude-home <dir>",
                checkout.display()
            ),
        );
        return Ok(());
    }
    let store = record_dir::open_or_create_record(records_dir, checkout)?;
    let report = sync(&store, checkout, &transcripts)?;
    match args.format {
        OutputFormat::Json => write_json(&report)?,
        OutputFormat::Terminal => print_sync_report(&report)?,
    }
    Ok(())
}

fn print_sync_report(report: &SyncReport) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "synced {} observations from {} transcript(s) into revision {}",
        report.new_observations,
        report.transcripts.len(),
        report.revision.get()
    )?;
    for t in &report.transcripts {
        let session = t.session.as_ref().map_or("unknown session", SessionId::as_str);
        writeln!(out, "  {}: {} new, {}", t.path.display(), t.new_observations, session)?;
        if t.trailing_partial {
            writeln!(out, "    incomplete final line left for the next sync")?;
        }
        if !t.unknown_records.is_empty() {
            let list: Vec<String> = t.unknown_records.iter().map(|(k, v)| format!("{k} ({v})")).collect();
            writeln!(out, "    unknown record types: {}", list.join(", "))?;
        }
        if t.malformed_lines > 0 {
            writeln!(out, "    malformed lines skipped: {}", t.malformed_lines)?;
        }
        if t.abandoned_tool_uses > 0 {
            writeln!(out, "    tool uses without a result: {}", t.abandoned_tool_uses)?;
        }
        if t.failed_tool_uses > 0 {
            writeln!(out, "    failed editing tool uses, not recorded as edits: {}", t.failed_tool_uses)?;
        }
        if t.outside_checkout > 0 {
            writeln!(out, "    edits outside the checkout dropped: {}", t.outside_checkout)?;
        }
    }
    Ok(())
}

fn summarize(store: &Store) -> Result<(u64, Vec<SessionSummary>), CliError> {
    let snapshot = store.snapshot()?;
    let mut rows = Vec::new();
    for session in snapshot.sessions() {
        let observations = snapshot.observations(&session)?;
        let derivations = snapshot.derivations(&session)?;
        let start = observations.iter().find_map(|o| match o {
            Observation::SessionStart(s) => Some(s),
            _ => None,
        });
        rows.push(SessionSummary {
            started: start.map(|s| s.time.as_str().to_owned()),
            branch: start.and_then(|s| s.branch.clone()),
            checkout: start.map(|s| s.checkout.clone()),
            turns: observations.iter().filter(|o| matches!(o, Observation::Turn(_))).count(),
            edits: observations.iter().filter(|o| matches!(o, Observation::Edit(_))).count(),
            commands: observations.iter().filter(|o| matches!(o, Observation::Command(_))).count(),
            restores: derivations.as_ref().map_or(0, |d| d.restores.len()),
            discontinuities: derivations.as_ref().map_or(0, |d| d.discontinuities.len()),
            session,
        });
    }
    Ok((snapshot.revision().get(), rows))
}

fn run_list(args: &SessionListArgs, records_dir: &Path, checkout: &Path) -> Result<(), CliError> {
    let Some(store) = record_dir::find_record(records_dir, checkout)? else {
        println!("no record for {}; run `supersigil session sync` first", checkout.display());
        return Ok(());
    };
    let (_, rows) = summarize(&store)?;
    match args.format {
        OutputFormat::Json => write_json(&rows)?,
        OutputFormat::Terminal => {
            let mut out = io::stdout().lock();
            for row in &rows {
                writeln!(
                    out,
                    "{}  {}  {}  {} turns, {} edits, {} commands, {} restores, {} discontinuities",
                    row.session,
                    row.started.as_deref().unwrap_or("-"),
                    row.branch.as_deref().unwrap_or("-"),
                    row.turns,
                    row.edits,
                    row.commands,
                    row.restores,
                    row.discontinuities
                )?;
            }
        }
    }
    Ok(())
}

fn run_show(args: &SessionShowArgs, records_dir: &Path, checkout: &Path) -> Result<(), CliError> {
    let Some(store) = record_dir::find_record(records_dir, checkout)? else {
        return Err(CliError::CommandFailed(format!(
            "no record for {}; run `supersigil session sync` first",
            checkout.display()
        )));
    };
    let snapshot = store.snapshot()?;
    let matches: Vec<SessionId> = snapshot
        .sessions()
        .into_iter()
        .filter(|s| s.as_str().starts_with(&args.session))
        .collect();
    let session = match matches.as_slice() {
        [one] => one.clone(),
        [] => {
            return Err(CliError::CommandFailed(format!("no session matching '{}'", args.session)));
        }
        many => {
            let ids: Vec<&str> = many.iter().map(SessionId::as_str).collect();
            return Err(CliError::CommandFailed(format!(
                "'{}' matches several sessions: {}",
                args.session,
                ids.join(", ")
            )));
        }
    };
    let output = ShowOutput {
        revision: snapshot.revision().get(),
        observations: snapshot.observations(&session)?,
        derivations: snapshot.derivations(&session)?,
        session,
    };
    write_json(&output)?;
    Ok(())
}
```

- [ ] **Step 7: Route the command**

In `crates/supersigil-cli/src/main.rs`, rename the `_color` parameter of `run` to `color` and add an arm to its `match cli.command`:

```rust
        Command::Session(ref args) => {
            supersigil_cli::commands::session::run(args, color)?;
        }
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo nextest run -p supersigil && cargo clippy -p supersigil --all-targets --all-features`
Expected: all pass, no warnings.

- [ ] **Step 9: Run the full workspace gate**

Run, in order:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo nextest run
```

Expected: no warnings, all tests pass.

- [ ] **Step 10: Commit**

```bash
git add Cargo.lock crates/supersigil-cli/Cargo.toml crates/supersigil-cli/src/commands.rs crates/supersigil-cli/src/commands/session.rs crates/supersigil-cli/src/record_dir.rs crates/supersigil-cli/src/error.rs crates/supersigil-cli/src/lib.rs crates/supersigil-cli/src/main.rs crates/supersigil-cli/tests/cmd_session.rs
git commit -m "feat: add session sync, list, and show commands"
```

---

## What the next plans build on this

- **Plan 2, review model.** Adds git snapshots and the retention ref, chain composition and attribution, the interpretation and contribution layers on the same store (`put_document` for annotation revisions, `append_log` for `contributions/judgments.jsonl`), the review basis with change detection, gate items, and the `annotate`, `review --format json`, `why`, and `judge` commands. It reads sessions through `RecordSnapshot::observations` and `derivations` exactly as `session show` does.
- **Plan 3, terminal review.** Adds the ratatui crate over the plan 2 review model, syntect highlighting, and the end-to-end slice test from section 8 of the design.

Public types in this plan are the contract for both. If plan 2 needs a field this plan did not add, it adds the field with a `#[serde(default)]` so revision-1 logs still parse.

Two things the design describes that this plan deliberately leaves out: the Claude Code stop hook that calls `session sync --transcript "$transcript_path"` is installed by `supersigil init` in plan 3, once review-open reconciliation exists to back it up; and `SessionEnd` is defined here but never emitted, because a transcript does not say when a session ended. Plan 2 decides whether an end is inferred from the hook or left absent.
