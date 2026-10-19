//! Gate items: what a person considers before completing a review, with
//! the identity and fingerprint that carry a judgment between reviews.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::PathBytes;
use crate::ids::{RecordId, SessionId};

/// The kind of gap a gate item reports. Items sort by origin in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Lines of a file no recorded edit explains.
    UnexplainedLines,
    /// A file listed but not diffed.
    NotDiffed,
    /// An untracked file outside the review that a recorded edit names.
    UntrackedRecorded,
    /// A path whose on-disk state the snapshot did not capture.
    NotCaptured,
    /// A reviewed commit no worktree originates.
    CommitWithoutOrigin,
    /// Gaps between recorded edits of a reviewed file.
    Discontinuity,
    /// A record or checkout whose evidence reconcile could not bring up to
    /// date.
    Unreconciled,
    /// A question the annotator left open.
    OpenQuestion,
}

impl Origin {
    /// The snake-case name the JSON uses and item IDs hash.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnexplainedLines => "unexplained_lines",
            Self::NotDiffed => "not_diffed",
            Self::UntrackedRecorded => "untracked_recorded",
            Self::NotCaptured => "not_captured",
            Self::CommitWithoutOrigin => "commit_without_origin",
            Self::Discontinuity => "discontinuity",
            Self::Unreconciled => "unreconciled",
            Self::OpenQuestion => "open_question",
        }
    }

    /// The qualifier that names an item of this origin, as in
    /// `lines:src/parser.rs`, without its colon.
    #[must_use]
    pub const fn qualifier(self) -> &'static str {
        match self {
            Self::UnexplainedLines => "lines",
            Self::NotDiffed => "notdiffed",
            Self::UntrackedRecorded => "untracked",
            Self::NotCaptured => "uncaptured",
            Self::CommitWithoutOrigin => "commit",
            Self::Discontinuity => "gap",
            Self::Unreconciled => "unreconciled",
            Self::OpenQuestion => "question",
        }
    }
}

/// What a gate item is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subject {
    /// A repository path; its ID hashes the raw bytes.
    Path(PathBytes),
    /// A full commit ID; its ID hashes the ID's text.
    Commit(String),
    /// A record; its ID hashes `record:<record id>`.
    Record(RecordId),
    /// A checkout's canonical path; its ID hashes `checkout:<path>`.
    Checkout(PathBuf),
    /// A question in a session's annotation; its ID hashes
    /// `question:<session id>:<question id>`.
    Question {
        /// The annotated session.
        session: SessionId,
        /// The question's id in the annotation.
        id: String,
    },
}

sha256_id!(
    /// Identity of a gate item, the same in every review that has it: the
    /// SHA-256 of its origin's name, a NUL byte, and its subject's bytes.
    ItemId
);

impl ItemId {
    /// The ID of the item of `origin` about `subject`.
    #[must_use]
    pub fn new(origin: Origin, subject: &Subject) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(origin.as_str());
        hasher.update(b"\0");
        match subject {
            Subject::Path(path) => hasher.update(path.as_bytes()),
            Subject::Commit(commit) => hasher.update(commit),
            Subject::Record(record) => {
                hasher.update(b"record:");
                hasher.update(record.as_str());
            }
            Subject::Checkout(checkout) => {
                hasher.update(b"checkout:");
                hasher.update(checkout.as_os_str().as_encoded_bytes());
            }
            Subject::Question { session, id } => {
                hasher.update(b"question:");
                hasher.update(session.as_str());
                hasher.update(b":");
                hasher.update(id);
            }
        }
        Self::from_digest(hasher.finalize())
    }
}

/// What a gate item's summary row says, one variant per [`Origin`].
///
/// Serialized with its origin's name under `origin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "origin", rename_all = "snake_case")]
pub enum GateDetail {
    /// Line counts by outcome, on both sides of the diff.
    UnexplainedLines {
        /// Lines with the *unattributed* outcome, except those counted in
        /// `content_match_incomplete`.
        unattributed: u64,
        /// Unattributed lines whose content matching ran out of budget.
        content_match_incomplete: u64,
        /// Lines with the *unresolved* outcome.
        unresolved: u64,
        /// Lines with the *ambiguous* outcome.
        ambiguous: u64,
        /// Lines with the *line ending changed* outcome.
        line_ending: u64,
        /// Lines explained only by a content match.
        content_match_only: u64,
    },
    /// Why the file was not diffed.
    NotDiffed {
        /// The not-diffed kind, such as `binary` or `mode_only`.
        kind: String,
    },
    /// How many recorded edits name the untracked file.
    UntrackedRecorded {
        /// Number of recorded edits naming the path.
        edits: u64,
    },
    /// Why the path was not captured and what is on disk.
    NotCaptured {
        /// `assume_unchanged`, `skip_worktree`, or `not_staged`.
        cause: String,
        /// A short description of the on-disk state, such as `missing`.
        on_disk: String,
    },
    /// The commit has no recorded origin.
    CommitWithoutOrigin,
    /// How many gaps the file's recorded edits have.
    Discontinuity {
        /// Number of gaps.
        gaps: u64,
    },
    /// Why the evidence was not brought up to date.
    Unreconciled {
        /// The reason, such as `locked by another writer`.
        reason: String,
    },
    /// The open question.
    OpenQuestion {
        /// The question's text.
        text: String,
    },
}

impl GateDetail {
    /// The origin this detail belongs to.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        match self {
            Self::UnexplainedLines { .. } => Origin::UnexplainedLines,
            Self::NotDiffed { .. } => Origin::NotDiffed,
            Self::UntrackedRecorded { .. } => Origin::UntrackedRecorded,
            Self::NotCaptured { .. } => Origin::NotCaptured,
            Self::CommitWithoutOrigin => Origin::CommitWithoutOrigin,
            Self::Discontinuity { .. } => Origin::Discontinuity,
            Self::Unreconciled { .. } => Origin::Unreconciled,
            Self::OpenQuestion { .. } => Origin::OpenQuestion,
        }
    }
}

/// One fingerprint part: the hex SHA-256 of the part's material, or
/// `unknown` when the material could not be established.
///
/// Serialized as the hex digits or the string `"unknown"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum FingerprintPart {
    /// Hex SHA-256 of the material.
    Known(String),
    /// The material could not be established; matches nothing.
    Unknown,
}

impl FingerprintPart {
    /// The known part for `material`.
    #[must_use]
    pub fn of(material: &[u8]) -> Self {
        Self::Known(hex::encode(Sha256::digest(material)))
    }
}

impl From<String> for FingerprintPart {
    fn from(text: String) -> Self {
        if text == "unknown" {
            Self::Unknown
        } else {
            Self::Known(text)
        }
    }
}

impl From<FingerprintPart> for String {
    fn from(part: FingerprintPart) -> Self {
        match part {
            FingerprintPart::Known(digest) => digest,
            FingerprintPart::Unknown => "unknown".to_owned(),
        }
    }
}

/// The evidence a gate item's judgment saw, by part name. A judgment carries
/// to another review's item only when their fingerprints match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Fingerprint(pub BTreeMap<String, FingerprintPart>);

impl Fingerprint {
    /// Whether every part is known and equal to `other`'s, with the same
    /// part names on both sides. An unknown part matches nothing, not even
    /// another unknown part.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self.differing(other).is_empty()
    }

    /// Returns sorted names of differing parts, parts unknown on either side,
    /// and parts present on one side only.
    #[must_use]
    pub fn differing(&self, other: &Self) -> Vec<String> {
        let names: BTreeSet<&String> = self.0.keys().chain(other.0.keys()).collect();
        names
            .into_iter()
            .filter(|name| match (self.0.get(*name), other.0.get(*name)) {
                (Some(FingerprintPart::Known(a)), Some(FingerprintPart::Known(b))) => a != b,
                _ => true,
            })
            .cloned()
            .collect()
    }

    /// Whether any part is unknown.
    #[must_use]
    pub fn has_unknown(&self) -> bool {
        self.0
            .values()
            .any(|part| *part == FingerprintPart::Unknown)
    }
}

/// One gate item of a review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateItem {
    /// The item's ID, from its origin and subject.
    pub id: ItemId,
    /// What the item is about.
    pub subject: Subject,
    /// What its summary row says, which also gives its origin.
    pub detail: GateDetail,
    /// What a judgment of it saw.
    pub fingerprint: Fingerprint,
}

impl GateItem {
    /// Creates an item whose ID hashes the detail's origin and `subject`.
    #[must_use]
    pub fn new(subject: Subject, detail: GateDetail, fingerprint: Fingerprint) -> Self {
        Self {
            id: ItemId::new(detail.origin(), &subject),
            subject,
            detail,
            fingerprint,
        }
    }

    /// The item's origin.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.detail.origin()
    }
}
