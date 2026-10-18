//! Interpretations: what an annotator says a session decided, and why.
//!
//! An [`AnnotationDocument`] is what an annotator writes about one session:
//! its decisions, each citing the edits and commands that carried it out,
//! with rationale lines labelled by who gave them, the alternatives
//! proposed or attempted, and the implications, plus the questions the
//! evidence leaves open. An entry marked explicit quotes the turn it rests
//! on; any other entry is the annotator's inference.
//!
//! The record stores each import as an [`Annotation`]: the document
//! unchanged, with its revision and import time. Revisions count from 1 per
//! session and record, and the highest is current. The record keeps every
//! revision, since saved reviews and corrections name them.
//!
//! These types check only the JSON shape and refuse unknown fields anywhere
//! in a document. Whether its citations and quotes hold against the
//! session's observations is checked on import, outside this crate.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::{EventId, JudgmentId, RecordId, Revision, SessionId, TurnId};
use crate::time::UtcTime;

/// Schema name of the annotation documents this crate reads and writes.
pub const ANNOTATION_SCHEMA: &str = "supersigil.annotation/1";

/// Number of one annotation of a session in a record, counted from 1.
///
/// Written as a JSON number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnnotationRevision(u32);

impl AnnotationRevision {
    /// A session's first annotation.
    pub const FIRST: Self = Self(1);

    /// Wraps a revision number, such as one read from a manifest key or
    /// another document that names a revision.
    #[must_use]
    pub const fn new(number: u32) -> Self {
        Self(number)
    }

    /// Returns this revision plus one.
    ///
    /// # Panics
    ///
    /// Panics if the revision would exceed [`u32::MAX`].
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("annotation revision overflow"))
    }

    /// Returns the revision number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for AnnotationRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What an annotator wrote about one session.
///
/// Serialization omits empty lists, and reading treats a missing list as
/// empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotationDocument {
    /// Schema name, [`ANNOTATION_SCHEMA`].
    pub schema: String,
    /// The session annotated.
    pub session: SessionId,
    /// The session's current revision when the annotator prepared, or
    /// `None` when it had none.
    pub based_on: Option<AnnotationRevision>,
    /// The record state the annotator saw, copied from its brief.
    pub prepared: Prepared,
    /// What produced the document, such as `claude-code/supersigil-annotate`.
    pub produced_by: String,
    /// The corrections the annotator was given, by judgment ID.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corrections_seen: Vec<JudgmentId>,
    /// The decisions found in the session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<Decision>,
    /// What the evidence does not say.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_questions: Vec<OpenQuestion>,
}

/// The record state an annotator's brief was read from. It states how much
/// of the session the annotation covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    /// The record read.
    pub record: RecordId,
    /// The record's revision.
    pub observation_revision: Revision,
    /// The session's observations at that revision.
    pub observed: Observed,
}

/// How many observations of a session a record held, and when the last one
/// was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observed {
    /// The number of the session's observations.
    pub count: u64,
    /// The time of the last observation, if known.
    pub last: Option<UtcTime>,
}

/// An interpreted choice the session made, with the evidence for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    /// The annotator's ID, kept stable across revisions.
    pub id: String,
    /// What was decided.
    pub title: String,
    /// The edits and commands that carried it out. A decision citing no
    /// edit is history, which no review shows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cites: Vec<EventId>,
    /// Why it was decided, one reason per entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rationale: Vec<Rationale>,
    /// Other courses proposed or attempted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Alternative>,
    /// What the decision means for later work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implications: Vec<Implication>,
}

/// One reason for a decision, or the reason an alternative was not taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rationale {
    /// The reason.
    pub text: String,
    /// Who gave it.
    pub speaker: Speaker,
    /// Whether the speaker stated it. An explicit reason has a source; one
    /// that is not explicit is the annotator's inference.
    pub explicit: bool,
    /// The passage the reason rests on. Serialization omits it when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

/// Who gave a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    /// The person, in a human turn.
    Human,
    /// The agent, in an agent or delegation turn.
    Agent,
    /// The annotator, inferring. Never explicit.
    Annotator,
}

/// A passage of one turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// The turn quoted.
    pub turn: TurnId,
    /// Words copied verbatim from the turn's retained text.
    pub quote: String,
}

/// A course the session proposed or attempted instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    /// What the alternative was.
    pub title: String,
    /// How far it went.
    pub status: AlternativeStatus,
    /// The edits that attempted it, none for a proposal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<EventId>,
    /// Why it was not taken, if known. Serialization omits it when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<Rationale>,
}

/// How far an alternative went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlternativeStatus {
    /// Raised but never made, so it has no edits.
    Proposed,
    /// Made by at least one edit.
    Attempted,
}

/// What a decision means for later work.
///
/// An implication has no speaker: an explicit one quotes a human or agent
/// turn, and any other is the annotator's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Implication {
    /// The implication.
    pub text: String,
    /// Whether a turn states it. An explicit implication has a source.
    pub explicit: bool,
    /// The passage it rests on. Serialization omits it when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

/// A question the evidence leaves open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenQuestion {
    /// The annotator's ID, kept stable across revisions.
    pub id: String,
    /// The question.
    pub text: String,
    /// The edits and commands it is about.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub about: Vec<EventId>,
}

/// An annotation as the record stores it: the imported document unchanged,
/// with the revision it was stored as and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    /// The revision, numbered per session and record.
    pub revision: AnnotationRevision,
    /// When it was stored: the storing transaction's time. What the
    /// annotation covers is the document's [`Prepared`], never this time.
    pub at: UtcTime,
    /// The document as imported.
    pub document: AnnotationDocument,
}
