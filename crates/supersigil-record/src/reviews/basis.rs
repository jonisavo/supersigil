//! A saved review's content and the code and evidence it used.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{GateItem, PathBytes};
use crate::ids::{RecordId, Revision};
use crate::observations::CaptureCounts;

/// Schema name of a basis document.
pub const BASIS_SCHEMA: &str = "supersigil.basis/1";

sha256_id!(
    /// Identity of a saved review: the SHA-256 of its basis's compact JSON
    /// (see [`Basis::basis_id`]).
    BasisId
);

impl BasisId {
    /// The ID of a basis serialized as `bytes`.
    pub(super) fn of(bytes: &[u8]) -> Self {
        Self::from_digest(Sha256::digest(bytes))
    }
}

/// A review's content and the code and evidence it used.
///
/// Its ID ([`Self::basis_id`]) hashes its JSON, with struct fields in
/// declaration order, maps sorted by key, and lists in the supplied order.
/// Callers sort lists by path, record, checkout, or transcript, and `gate`
/// by origin then subject, to give the same review the same ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Basis {
    /// [`BASIS_SCHEMA`].
    pub schema: String,
    /// Canonical path of the reviewed worktree.
    pub worktree: PathBuf,
    /// What the review compared against.
    pub base: BasisBase,
    /// What the review reviewed.
    pub target: BasisTarget,
    /// The review's options that select paths.
    pub scope: BasisScope,
    /// Each involved record and what the review read of it, sorted by ID.
    pub records: Vec<BasisRecord>,
    /// Checkouts reconcile could not bring up to date, sorted by checkout.
    pub unreconciled_checkouts: Vec<BasisCheckout>,
    /// Versions of the algorithms the review ran.
    pub algorithms: Algorithms,
    /// File explanation digests, excluding command mentions, sorted by path.
    pub explanations: Vec<Explanation>,
    /// Candidate transcripts with any capture limitation, with the counts
    /// the review saw, sorted by transcript.
    pub capture: Vec<CaptureEntry>,
    /// The gate items, sorted by origin, then subject.
    pub gate: Vec<GateItem>,
    /// A fresh random value when a gate item has an unknown fingerprint part,
    /// to prevent reuse of an earlier review's ID. Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salt: Option<String>,
}

impl Basis {
    /// Returns the SHA-256 of `serde_json::to_vec(self)`, the bytes the store
    /// writes to `bases/<id>.json`.
    ///
    /// # Errors
    ///
    /// Returns an error if the basis cannot be serialized, which happens
    /// when a `PathBuf` in it is not valid UTF-8.
    pub fn basis_id(&self) -> Result<BasisId, serde_json::Error> {
        serde_json::to_vec(self).map(|bytes| BasisId::of(&bytes))
    }
}

/// The [`Basis`] fields used to relate reviews: worktree, base, target, and
/// scope. [`ReviewSnapshot::basis_if`](super::ReviewSnapshot::basis_if) reads
/// these before parsing the rest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BasisHeader {
    /// Canonical path of the reviewed worktree.
    pub worktree: PathBuf,
    /// What the review compared against.
    pub base: BasisBase,
    /// What the review reviewed.
    pub target: BasisTarget,
    /// The review's options that select paths.
    pub scope: BasisScope,
}

/// The review's base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasisBase {
    /// Full ID of the base commit, if the base is a commit.
    pub commit: Option<String>,
    /// Full ID of the base tree.
    pub tree: String,
}

/// The review's target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasisTarget {
    /// Whether the target is the working tree or a commit.
    pub kind: TargetKind,
    /// Full ID of the target commit, if the target is a commit.
    pub commit: Option<String>,
    /// Full ID of the captured target tree.
    pub tree: String,
}

/// What a review reviewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// The worktree's files as captured.
    WorkingTree,
    /// A commit.
    Commit,
}

/// The review's path options, as repository paths, each list sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasisScope {
    /// Untracked paths included with `--include-untracked`.
    pub include_untracked: Vec<PathBytes>,
    /// Path filters the review is limited to.
    pub paths: Vec<PathBytes>,
}

/// An involved record and what the review read of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasisRecord {
    /// The record's ID.
    pub id: RecordId,
    /// The record revision the review pinned.
    pub revision: Revision,
    /// Each pinned log, by its path in the record.
    pub logs: BTreeMap<String, PinnedLog>,
    /// Why reconcile did not bring the record up to date, if it did not.
    pub not_reconciled: Option<String>,
    /// Sorted checkout associations at the pinned revision, used to name an
    /// unreconciled record's gate item without reading the record.
    pub associations: Vec<PathBuf>,
}

/// The pinned part of one log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedLog {
    /// The pinned length in bytes.
    pub length: u64,
    /// Hex SHA-256 of the log's first `length` bytes.
    pub sha256: String,
}

/// A checkout reconcile could not bring up to date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasisCheckout {
    /// The checkout's canonical path.
    pub checkout: PathBuf,
    /// Why reconcile could not update the checkout.
    pub reason: String,
}

/// Versions of the algorithms a review ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Algorithms {
    /// The derivation algorithm version.
    pub derivation: u32,
}

/// The digest of what a review shows about one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    /// The reviewed file.
    pub path: PathBytes,
    /// Hex SHA-256 of the file's review entry, without its mentions, and
    /// the edits it references.
    pub digest: String,
}

/// A candidate transcript's capture limitations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureEntry {
    /// The transcript's canonical path.
    pub transcript: String,
    /// Its capture limitation counts.
    pub counts: CaptureCounts,
}
