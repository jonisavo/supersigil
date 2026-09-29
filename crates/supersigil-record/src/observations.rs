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
    /// Where the capture of a transcript range may be missing evidence.
    CaptureLimitation(CaptureLimitation),
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
            Self::CaptureLimitation(l) => &l.session,
        }
    }
}

/// The session start that describes a session, as a read model over its
/// [`SessionStart`] observations: each transcript records one when it first
/// names the session. The earliest start by `time` from a main transcript
/// (not a sidechain) wins; without one, the earliest start overall. Ties
/// keep log order. `None` when there is no start.
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
    /// Source-native identifiers. For Claude Code, `transcript` is the
    /// transcript's file name and `path` its canonical path.
    pub source_ids: BTreeMap<String, String>,
    /// Checkout the session worked in.
    pub checkout: PathBuf,
    /// Branch name reported by the source, if any.
    pub branch: Option<String>,
    /// Time of the first record.
    pub time: Timestamp,
    /// Whether the transcript that produced this start is a subagent
    /// sidechain.
    #[serde(default)]
    pub sidechain: bool,
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
    /// An instruction written by a parent agent to a subagent; not human
    /// intent.
    Delegation,
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
    /// Material the source did not provide, for the given reason.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }

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
    /// Canonical path of the transcript the record came from, as in
    /// [`SessionStart::source_ids`] under `path`. Observations from
    /// different transcripts share no ordinal order.
    #[serde(default)]
    pub transcript: Option<String>,
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
        Self::Present {
            content: Content::Known(content),
        }
    }

    /// A present file whose content is not known.
    #[must_use]
    pub const fn unknown() -> Self {
        Self::Present {
            content: Content::Unknown,
        }
    }

    /// The known content hash, if the file existed and the hash is known.
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
    /// Subagent that produced this event, when inside a subagent.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Canonical path of the transcript the record came from, as in
    /// [`SessionStart::source_ids`] under `path`. Observations from
    /// different transcripts share no ordinal order.
    #[serde(default)]
    pub transcript: Option<String>,
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
    /// Subagent that produced this event, when inside a subagent.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Canonical path of the transcript the record came from, as in
    /// [`SessionStart::source_ids`] under `path`. Observations from
    /// different transcripts share no ordinal order.
    #[serde(default)]
    pub transcript: Option<String>,
}

/// A limitation of the capture over the transcript ordinal range
/// `[from_ordinal, to_ordinal)`: records the parser could not use and tool
/// uses it could not turn into evidence. Recorded so that later readers can
/// show where evidence may be missing, rather than presenting the session as
/// completely observed.
///
/// There is no timestamp; the ordinal range orders it against other
/// observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureLimitation {
    /// Session identifier.
    pub session: SessionId,
    /// Canonical path of the transcript, as in [`SessionStart::source_ids`]
    /// under `path`.
    pub transcript: String,
    /// First ordinal of the range.
    pub from_ordinal: u64,
    /// Ordinal just past the range.
    pub to_ordinal: u64,
    /// What the capture could not use, flattened into this object.
    #[serde(flatten)]
    pub counts: CaptureCounts,
}

/// Counts of transcript material the capture could not turn into evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureCounts {
    /// Records of unknown type, by type, that produced no observation.
    pub unknown_records: BTreeMap<String, u64>,
    /// Lines that were not valid JSON.
    pub malformed_lines: u64,
    /// Tool uses still without a recorded result when a typed message
    /// arrived.
    pub abandoned_tool_uses: u64,
    /// Editing tool uses the harness reported as failed; no edit was recorded.
    pub failed_tool_uses: u64,
    /// Edits outside the checkout that were dropped.
    pub outside_checkout: u64,
    /// Editing tool uses whose input and result name different files; the
    /// edit was dropped rather than recorded under either path.
    #[serde(default)]
    pub conflicting_tool_results: u64,
    /// Tool results whose id matches no tool use awaiting a result, so
    /// whatever they reported was dropped.
    #[serde(default)]
    pub unmatched_tool_results: u64,
    /// Turns whose session id differs from the transcript's established
    /// session; they were not attributed to either session.
    #[serde(default)]
    pub session_mismatch: u64,
    /// Tool uses without a non-empty id, which no result can be paired
    /// with; whatever they did was dropped.
    #[serde(default)]
    pub unnamed_tool_uses: u64,
    /// Resolved tool uses that may have changed files through a tool the
    /// capture does not read, such as `NotebookEdit`; no edit was recorded.
    #[serde(default)]
    pub unsupported_tool_uses: u64,
}

impl CaptureCounts {
    /// Whether nothing was counted, so there is no limitation to record.
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
