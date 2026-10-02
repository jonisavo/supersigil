//! Session events captured from agent transcripts.
//!
//! The store appends observations to session logs without changing existing
//! entries. A session ending adds a [`SessionEnd`] event. Edits and commands
//! refer to the [`Turn`] that issued them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{ContentId, EventId, SessionId, Timestamp, TurnId};

/// One captured event or capture limitation in a session's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Observation {
    /// A transcript first identified its session.
    SessionStart(SessionStart),
    /// The source reported that a session ended.
    SessionEnd(SessionEnd),
    /// A conversation turn.
    Turn(Turn),
    /// A file edit made through the agent's editing tools.
    Edit(Edit),
    /// A shell command the agent ran.
    Command(Command),
    /// Transcript records or tool calls that the parser could not capture.
    CaptureLimitation(CaptureLimitation),
}

impl Observation {
    /// Returns the session ID of this observation.
    #[must_use]
    pub fn session(&self) -> &SessionId {
        match self {
            Self::SessionStart(s) => &s.session,
            Self::SessionEnd(s) => &s.session,
            Self::Turn(t) => &t.session,
            Self::Edit(e) => &e.session,
            Self::Command(c) => &c.session,
            Self::CaptureLimitation(l) => &l.session,
        }
    }
}

/// Selects the start event to use as a session's metadata.
///
/// Prefers the earliest [`SessionStart::time`] from a main transcript. If
/// there are only subagent transcripts, selects the earliest of those.
/// Equal timestamps keep the first event in the slice. Returns `None` if
/// there are no start events.
///
/// The caller must supply observations for a single session.
#[must_use]
pub fn session_start(observations: &[Observation]) -> Option<&SessionStart> {
    observations
        .iter()
        .filter_map(|o| match o {
            Observation::SessionStart(start) => Some(start),
            _ => None,
        })
        .min_by_key(|start| (start.sidechain, start.time.as_str()))
}

/// Agent application that produced the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Claude Code JSONL transcripts.
    ClaudeCode,
}

/// Session metadata captured when a transcript first identifies its session.
///
/// A session can have starts from several transcripts. Use [`session_start`]
/// to select one for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStart {
    /// Session identifier from the source.
    pub session: SessionId,
    /// Agent application that wrote the transcript.
    pub source: Source,
    /// Identifiers supplied by the source. For Claude Code, `transcript` is
    /// the transcript's file name and `path` is its canonical path.
    pub source_ids: BTreeMap<String, String>,
    /// Checkout the session worked in.
    pub checkout: PathBuf,
    /// Branch name reported by the source, if any.
    pub branch: Option<String>,
    /// First timestamp reported for the session in this transcript.
    pub time: Timestamp,
    /// Whether this metadata came from a subagent transcript.
    #[serde(default)]
    pub sidechain: bool,
}

/// Reason recorded for a session ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// The source marked the transcript as finished.
    TranscriptEnded,
    /// The source gave no reason.
    Unknown,
}

/// A report that a session ended, stored as a separate event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEnd {
    /// Session identifier.
    pub session: SessionId,
    /// Reported end time.
    pub time: Timestamp,
    /// Why the session ended.
    pub reason: EndReason,
}

/// Author or message type assigned to a conversation turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A message classified as human-authored.
    Human,
    /// A message written by the agent.
    Agent,
    /// Instructions from a parent agent to a subagent.
    Delegation,
    /// A message containing only tool results.
    Tool,
    /// Application metadata carried in a user-role message.
    Meta,
    /// A summary that replaces earlier conversation context after compaction.
    Summary,
}

/// A captured value, or the reason it was not stored.
///
/// Used for turn text, edit text, patches, and command output. JSON stores
/// the variant in `state` and its contents in `value`, for example
/// `{"state":"retained","value":"hello"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum Material<T> {
    /// The captured value is stored here.
    Retained(T),
    /// A capture policy prevented storing the value.
    Withheld {
        /// Name of the capture policy that prevented storage.
        policy: String,
    },
    /// The source did not provide it.
    Unavailable {
        /// Reason the value could not be obtained from the source.
        reason: String,
    },
}

impl<T> Material<T> {
    /// Creates an unavailable value with the supplied reason.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }

    /// Returns the stored value, or `None` if it was withheld or unavailable.
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
    /// Parent turn ID in the conversation tree, if the source supplied one.
    pub parent: Option<TurnId>,
    /// Author or message type assigned to this turn.
    pub role: Role,
    /// Timestamp on the source record.
    pub time: Timestamp,
    /// Whether the source marked this as a subagent turn.
    pub sidechain: bool,
    /// Subagent ID, if the source supplied one.
    pub agent_id: Option<String>,
    /// Turn text, or the reason it was not stored.
    pub excerpt: Material<String>,
    /// Zero-based line position of the source record in its transcript.
    pub source_ordinal: u64,
    /// Canonical transcript path, matching `path` in [`SessionStart::source_ids`].
    /// Compare `source_ordinal` values only within the same transcript.
    #[serde(default)]
    pub transcript: Option<String>,
}

/// Whether an existing file's content hash is known.
///
/// This stores a hash, not the file's bytes. JSON uses `kind` for the variant
/// and `id` for the hash when known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Content {
    /// The content hash is known.
    Known(ContentId),
    /// The file existed but its content is not known.
    Unknown,
}

/// Whether a file exists before or after an edit, and its content hash if known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FileState {
    /// The file did not exist.
    Absent,
    /// The file existed.
    Present {
        /// Known content hash, or [`Content::Unknown`] if no hash is available.
        content: Content,
    },
}

impl FileState {
    /// Marks a file as present with the supplied content hash.
    #[must_use]
    pub const fn known(content: ContentId) -> Self {
        Self::Present {
            content: Content::Known(content),
        }
    }

    /// Marks a file as present with no known content hash.
    #[must_use]
    pub const fn unknown() -> Self {
        Self::Present {
            content: Content::Unknown,
        }
    }

    /// Returns the content hash, or `None` if the file was absent or its hash is unknown.
    #[must_use]
    pub fn content_id(&self) -> Option<&ContentId> {
        match self {
            Self::Present {
                content: Content::Known(id),
            } => Some(id),
            Self::Present {
                content: Content::Unknown,
            }
            | Self::Absent => None,
        }
    }
}

/// One changed region of a file, represented as a unified-diff hunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// First old-file line in the range, starting at 1. For an empty range,
    /// the preceding line number; 0 means before the first line.
    pub old_start: u32,
    /// Number of old-file lines covered, including unchanged context.
    pub old_lines: u32,
    /// First new-file line in the range, starting at 1. For an empty range,
    /// the preceding line number; 0 means before the first line.
    pub new_start: u32,
    /// Number of new-file lines covered, including unchanged context.
    pub new_lines: u32,
    /// Patch lines prefixed with a space for context, `-` for removal, or `+` for addition.
    pub lines: Vec<String>,
}

/// Editing operation recorded for an edit.
///
/// Reconstructing an earlier file state depends on it: an Edit replaced
/// `old_text` with `new_text`, a Write replaced the whole file, an append
/// added `new_text` after the file's last byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditOperation {
    /// Claude Code's Edit tool: `old_text` replaced by `new_text`.
    Replace,
    /// Claude Code's Write tool: the whole file becomes `new_text`.
    Write,
    /// `new_text` was appended to the file, creating it if absent, as
    /// shell `>>` does.
    Append,
    /// Not recorded (logs written before this field existed) or a tool
    /// anchor cannot reverse, such as `MultiEdit`.
    #[default]
    Unknown,
}

/// How an edit reached the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditOrigin {
    /// A call to an editing tool, whose result states the edit.
    Tool,
    /// A quoted heredoc redirected to a file. Its text comes from the
    /// command; the harness confirms the command changed the file. It
    /// confirms neither this statement's execution nor its resulting
    /// bytes. The edit is inferred, not observed.
    Shell {
        /// The command that held the heredoc.
        command: EventId,
        /// Zero-based position among the command's heredoc writes. The
        /// shell runs them in this order, which orders one command's
        /// edits sharing a source ordinal.
        index: u64,
    },
}

/// An edit from an agent editing tool or a harness-confirmed shell
/// heredoc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    /// Event ID derived from the session ID and the source's tool-use ID.
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
    /// Patch hunks, or the reason they were not stored.
    pub patch: Material<Vec<Hunk>>,
    /// Replaced text, or the reason it was not stored.
    pub old_text: Material<String>,
    /// Replacement text, or the reason it was not stored.
    pub new_text: Material<String>,
    /// Whether the editing tool was instructed to replace every matching occurrence.
    pub replace_all: bool,
    /// Editing operation, `Unknown` for observations recorded before it was kept.
    #[serde(default)]
    pub operation: EditOperation,
    /// How the edit reached the record.
    pub origin: EditOrigin,
    /// Checkout the edit happened in.
    pub checkout: PathBuf,
    /// Timestamp of the tool result, falling back to the issuing record's
    /// timestamp when the result has none.
    pub time: Timestamp,
    /// Zero-based line position of the record that issued the edit.
    pub source_ordinal: u64,
    /// ID of the subagent that issued the edit, if available.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Canonical transcript path, matching `path` in [`SessionStart::source_ids`].
    /// Compare `source_ordinal` values only within the same transcript.
    #[serde(default)]
    pub transcript: Option<String>,
}

impl Edit {
    /// Returns the before-content hash, or `None` for an absent file or unknown hash.
    #[must_use]
    pub fn before_content(&self) -> Option<&ContentId> {
        self.before.content_id()
    }

    /// Returns the after-content hash, or `None` for an absent file or unknown hash.
    #[must_use]
    pub fn after_content(&self) -> Option<&ContentId> {
        self.after.content_id()
    }
}

/// Type of work a shell command performs, classified from its command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandCategory {
    /// Runs tests.
    TestRun,
    /// Builds code or checks it for errors.
    Build,
    /// Invokes git.
    Git,
    /// No test, build, or git classification matched.
    Other,
}

/// Success or failure inferred from a recorded command result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Reported success.
    Passed,
    /// Reported failure.
    Failed,
}

/// What the harness stated about one file a command changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// The file did not exist before the command.
    Created,
    /// The file no longer exists after the command.
    Deleted,
    /// The file existed before and after the command.
    Modified,
    /// The harness named the path without describing the change.
    NotStated,
}

/// One file the harness reported a command changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Path relative to the command's checkout.
    pub path: PathBuf,
    /// What the harness stated about the change.
    pub kind: ChangeKind,
    /// The harness's capped display hunks for this file over the whole
    /// command, or why none exist. They are never file bytes.
    pub patch: Material<Vec<Hunk>>,
}

/// The harness's report of files a command changed.
///
/// Claude Code attaches it to Bash results as `bashEditDiff`. It describes
/// which files differ after the whole command, regardless of which part
/// changed them. It observes changes while the command ran, never the bytes
/// it wrote.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChangeReport {
    /// The changed files inside the command's checkout, in the harness's order.
    pub files: Vec<FileChange>,
    /// Reported paths outside the command's checkout, which are not kept.
    pub outside: u64,
    /// Changed files the harness counted without naming.
    pub unlisted: u64,
    /// Flag names kept as given because their meaning is not
    /// established.
    pub flags: BTreeSet<String>,
}

/// A shell command the agent ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    /// Event ID derived from the session ID and the source's tool-use ID.
    pub id: EventId,
    /// Turn that issued the command.
    pub turn: TurnId,
    /// Session identifier.
    pub session: SessionId,
    /// Command line passed to the shell tool.
    pub cmd: String,
    /// Exit code when the source reports one.
    pub exit: Option<i32>,
    /// Tail of standard output, if retained.
    pub stdout_tail: Material<String>,
    /// Tail of standard error, if retained.
    pub stderr_tail: Material<String>,
    /// Type of work inferred from the command line.
    pub category: CommandCategory,
    /// Whether the source marked the tool result as an error.
    pub reported_error: bool,
    /// Success or failure inferred from the output and reported error status.
    /// `None` means the parser could not determine an outcome.
    pub outcome: Option<Outcome>,
    /// Time the command was issued.
    pub started: Timestamp,
    /// Timestamp of the tool result, if available.
    pub ended: Option<Timestamp>,
    /// Checkout the command ran in.
    pub checkout: PathBuf,
    /// The harness's file change report, or why none exists. A missing
    /// report does not mean nothing changed.
    pub changes: Material<ChangeReport>,
    /// Zero-based line position of the record that issued the command.
    pub source_ordinal: u64,
    /// ID of the subagent that issued the command, if available.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Canonical transcript path, matching `path` in [`SessionStart::source_ids`].
    /// Compare `source_ordinal` values only within the same transcript.
    #[serde(default)]
    pub transcript: Option<String>,
}

/// Counts records and tool calls that could not be captured in a transcript range.
///
/// The range includes `from_ordinal` and excludes `to_ordinal`. It identifies
/// where evidence may be missing using transcript positions instead of timestamps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureLimitation {
    /// Session identifier.
    pub session: SessionId,
    /// Canonical path of the transcript, as in [`SessionStart::source_ids`]
    /// under `path`.
    pub transcript: String,
    /// First line position in the range, starting at zero.
    pub from_ordinal: u64,
    /// First line position after the range.
    pub to_ordinal: u64,
    /// Counts by reason, serialized as fields of this object.
    #[serde(flatten)]
    pub counts: CaptureCounts,
}

/// Counts of transcript records and tool calls omitted during capture, by reason.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureCounts {
    /// Skipped records by type or reason, such as an unknown type or
    /// `assistant-without-uuid`.
    pub unknown_records: BTreeMap<String, u64>,
    /// Lines that could not be decoded as transcript records.
    pub malformed_lines: u64,
    /// Tool calls still awaiting a result when the next human or delegation
    /// turn arrived.
    pub abandoned_tool_uses: u64,
    /// Editing tool calls reported as failed, with no edit recorded.
    pub failed_tool_uses: u64,
    /// Edits omitted because their paths were outside the checkout, and
    /// edits and commands omitted because they were issued from a working
    /// directory outside it.
    pub outside_checkout: u64,
    /// Editing tool calls omitted because their input and result named
    /// different files.
    #[serde(default)]
    pub conflicting_tool_results: u64,
    /// Tool results omitted because their IDs matched no pending tool call.
    #[serde(default)]
    pub unmatched_tool_results: u64,
    /// Turns omitted because their session IDs differed from the transcript's session.
    #[serde(default)]
    pub session_mismatch: u64,
    /// Tool calls omitted because they had no non-empty ID to match a result against.
    #[serde(default)]
    pub unnamed_tool_uses: u64,
    /// Completed tool calls the capture cannot read, so nothing was recorded
    /// for them: calls to unsupported editing tools, such as `NotebookEdit`,
    /// and `Bash` calls without a command.
    #[serde(default)]
    pub unsupported_tool_uses: u64,
    /// Heredoc writes without harness confirmation, so no edit is recorded:
    /// the call failed, was cut off, ran in the background, had no result,
    /// or its report did not describe the file.
    pub unconfirmed_shell_writes: u64,
}

impl CaptureCounts {
    /// Returns `true` if `unknown_records` is empty and every numeric count is zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.unknown_records.is_empty()
            && self.malformed_lines == 0
            && self.abandoned_tool_uses == 0
            && self.failed_tool_uses == 0
            && self.outside_checkout == 0
            && self.conflicting_tool_results == 0
            && self.unmatched_tool_results == 0
            && self.session_mismatch == 0
            && self.unnamed_tool_uses == 0
            && self.unsupported_tool_uses == 0
            && self.unconfirmed_shell_writes == 0
    }
}

impl std::ops::AddAssign<&Self> for CaptureCounts {
    /// Adds every count of `rhs`, merging `unknown_records` by reason.
    fn add_assign(&mut self, rhs: &Self) {
        for (reason, count) in &rhs.unknown_records {
            *self.unknown_records.entry(reason.clone()).or_default() += count;
        }
        self.malformed_lines += rhs.malformed_lines;
        self.abandoned_tool_uses += rhs.abandoned_tool_uses;
        self.failed_tool_uses += rhs.failed_tool_uses;
        self.outside_checkout += rhs.outside_checkout;
        self.conflicting_tool_results += rhs.conflicting_tool_results;
        self.unmatched_tool_results += rhs.unmatched_tool_results;
        self.session_mismatch += rhs.session_mismatch;
        self.unnamed_tool_uses += rhs.unnamed_tool_uses;
        self.unsupported_tool_uses += rhs.unsupported_tool_uses;
        self.unconfirmed_shell_writes += rhs.unconfirmed_shell_writes;
    }
}

impl<'a> std::iter::Sum<&'a Self> for CaptureCounts {
    /// Totals the counts, starting from none.
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |mut total, counts| {
            total += counts;
            total
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_counts_add_up_field_by_field() {
        let a = CaptureCounts {
            unknown_records: BTreeMap::from([("x".to_owned(), 1), ("y".to_owned(), 2)]),
            malformed_lines: 1,
            abandoned_tool_uses: 2,
            failed_tool_uses: 3,
            outside_checkout: 4,
            conflicting_tool_results: 5,
            unmatched_tool_results: 6,
            session_mismatch: 7,
            unnamed_tool_uses: 8,
            unsupported_tool_uses: 9,
            unconfirmed_shell_writes: 10,
        };
        let b = CaptureCounts {
            unknown_records: BTreeMap::from([("y".to_owned(), 10), ("z".to_owned(), 1)]),
            malformed_lines: 10,
            abandoned_tool_uses: 20,
            failed_tool_uses: 30,
            outside_checkout: 40,
            conflicting_tool_results: 50,
            unmatched_tool_results: 60,
            session_mismatch: 70,
            unnamed_tool_uses: 80,
            unsupported_tool_uses: 90,
            unconfirmed_shell_writes: 100,
        };
        let total: CaptureCounts = [&a, &b].into_iter().sum();
        assert_eq!(
            total,
            CaptureCounts {
                unknown_records: BTreeMap::from([
                    ("x".to_owned(), 1),
                    ("y".to_owned(), 12),
                    ("z".to_owned(), 1),
                ]),
                malformed_lines: 11,
                abandoned_tool_uses: 22,
                failed_tool_uses: 33,
                outside_checkout: 44,
                conflicting_tool_results: 55,
                unmatched_tool_results: 66,
                session_mismatch: 77,
                unnamed_tool_uses: 88,
                unsupported_tool_uses: 99,
                unconfirmed_shell_writes: 110,
            }
        );
        assert!(
            std::iter::empty::<&CaptureCounts>()
                .sum::<CaptureCounts>()
                .is_empty()
        );
    }

    fn start(transcript: &str, time: &str, sidechain: bool) -> Observation {
        Observation::SessionStart(SessionStart {
            session: SessionId::new("s"),
            source: Source::ClaudeCode,
            source_ids: BTreeMap::from([("transcript".to_owned(), transcript.to_owned())]),
            checkout: PathBuf::from("/work/repo"),
            branch: None,
            time: Timestamp::new(time),
            sidechain,
        })
    }

    #[test]
    fn every_count_makes_counts_non_empty() {
        let each = [
            CaptureCounts {
                session_mismatch: 1,
                ..CaptureCounts::default()
            },
            CaptureCounts {
                unnamed_tool_uses: 1,
                ..CaptureCounts::default()
            },
            CaptureCounts {
                unsupported_tool_uses: 1,
                ..CaptureCounts::default()
            },
            CaptureCounts {
                unconfirmed_shell_writes: 1,
                ..CaptureCounts::default()
            },
        ];
        assert!(CaptureCounts::default().is_empty());
        for counts in each {
            assert!(!counts.is_empty(), "{counts:?}");
        }
    }

    fn chosen(observations: &[Observation]) -> Option<&str> {
        session_start(observations).map(|s| s.source_ids["transcript"].as_str())
    }

    #[test]
    fn session_start_prefers_the_earliest_main_transcript() {
        let side = start("agent-a.jsonl", "2026-09-28T09:00:00.000Z", true);
        let late = start("late.jsonl", "2026-09-28T11:00:00.000Z", false);
        let main = start("main.jsonl", "2026-09-28T10:00:00.000Z", false);
        assert_eq!(chosen(&[]), None);
        assert_eq!(chosen(std::slice::from_ref(&side)), Some("agent-a.jsonl"));
        assert_eq!(
            chosen(&[side.clone(), late.clone(), main.clone()]),
            Some("main.jsonl")
        );
        assert_eq!(chosen(&[main, side.clone(), late]), Some("main.jsonl"));
        // Without a main transcript, the earliest sidechain start.
        let later_side = start("agent-b.jsonl", "2026-09-28T09:30:00.000Z", true);
        assert_eq!(chosen(&[later_side, side]), Some("agent-a.jsonl"));
    }
}
