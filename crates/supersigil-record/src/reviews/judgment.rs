//! Judgments: what a person recorded on a saved review, one JSON line each
//! in the append-only `judgments.jsonl`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{BasisId, ItemId};
use crate::files;
use crate::ids::{JudgmentId, RecordId, SessionId};
use crate::interpretations::AnnotationRevision;
use crate::store::StoreError;
use crate::time::UtcTime;

/// A judgment recorded by one writing call. An undo withdraws earlier
/// judgments without editing them.
///
/// Serialization omits empty lists and maps and absent text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judgment {
    /// A fresh UUID.
    pub id: JudgmentId,
    /// The full ID of the basis the call wrote to.
    pub basis: BasisId,
    /// What the call recorded.
    pub kind: JudgmentKind,
    /// The items it names: acknowledged, dismissed, commented on, or whose
    /// judgments an undo withdrew.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ItemId>,
    /// The reason given with `--reason`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The text given with `--text`, or a correction's constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// For a correction, the decision it addresses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionRef>,
    /// For a correction, its kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<CorrectionKind>,
    /// For a completion, maps each gate item to its closing judgment,
    /// recorded on this review or carried from another.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub closes: BTreeMap<ItemId, JudgmentId>,
    /// For an undo: the judgments it withdraws.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withdraws: Vec<Withdrawal>,
    /// Who recorded it.
    pub who: Who,
    /// When it was recorded.
    pub at: UtcTime,
}

/// What a judgment records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgmentKind {
    /// The person has considered the items.
    Acknowledge,
    /// The items need no follow-up, for the recorded reason.
    Dismiss,
    /// A note on the review or an item, which closes nothing.
    Comment,
    /// A correction of an annotated decision, which closes nothing.
    Correct,
    /// The review is complete.
    Complete,
    /// Withdraws earlier judgments.
    Undo,
}

/// The annotated decision a correction addresses.
///
/// Annotation revisions are numbered per record, and one session can be
/// held by two records, so the record is part of the identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DecisionRef {
    /// The record holding the annotation.
    pub record: RecordId,
    /// The annotated session.
    pub session: SessionId,
    /// The annotation revision the decision was read from.
    pub revision: AnnotationRevision,
    /// The decision's id in that revision.
    pub id: String,
}

/// What a correction says about the decision it addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionKind {
    /// Disputes a rationale: the decision is explained wrongly, for the
    /// judgment's reason.
    Wrong,
    /// Disputes a grouping: the edits the decision cites do not belong
    /// together, for the judgment's reason.
    NotTogether,
    /// Adds a human rationale, shown first: the actual constraint, in the
    /// judgment's text.
    Constraint,
}

impl CorrectionKind {
    /// The snake-case name the JSON uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wrong => "wrong",
            Self::NotTogether => "not_together",
            Self::Constraint => "constraint",
        }
    }
}

/// A judgment an undo withdraws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withdrawal {
    /// The withdrawn judgment.
    pub judgment: JudgmentId,
    /// The one item whose acknowledgment or dismissal is withdrawn, or
    /// `None` for a whole comment or completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<ItemId>,
}

/// The person who recorded a judgment, as git resolves `user.name` and
/// `user.email` in the reviewed repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Who {
    /// `user.name`.
    pub name: String,
    /// `user.email`.
    pub email: String,
}

/// Reads the first `length` bytes of the judgments log at `path`. A log
/// with no published bytes need not exist.
///
/// # Errors
///
/// Returns [`StoreError::Corrupt`] if the log is shorter than `length`,
/// [`StoreError::Io`] if it cannot be read, or [`StoreError::Json`] if its
/// lines are not judgments.
pub(super) fn read_log(path: &Path, length: u64) -> Result<Vec<Judgment>, StoreError> {
    if length == 0 {
        return Ok(Vec::new());
    }
    files::read_log(path, length)
}
