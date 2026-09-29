//! Restores and gaps found by comparing recorded file states.
//!
//! [`crate::derive::derive`] computes these results from observations. Each
//! [`DerivationSet`] records the input revision and algorithm version so callers
//! can tell which observations and comparison rules produced the results.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{EventId, Revision, SessionId};

/// Version of the comparison rules used by [`crate::derive::derive`].
pub const ALGORITHM_VERSION: u32 = 1;

/// An edit that returns a file to content recorded before an earlier edit.
///
/// For example, after `A -> B` followed by `B -> A`, the second edit restores
/// the content from before the first. The comparison uses known content hashes
/// for the same checkout, path, and transcript. It does not establish why the
/// content was restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restore {
    /// ID of the edit that returned to earlier content.
    pub edit: EventId,
    /// IDs of earlier edits whose before-content matches the restoring edit's
    /// after-content, ordered by position in the transcript.
    pub restores: Vec<EventId>,
}

/// A mismatch between the file state after one edit and before the next.
///
/// For example, one edit leaves content `B`, but the next starts with content
/// `C`. The edits belong to the same checkout, path, and transcript. A missing
/// file also counts as a known state. If either state has unknown content,
/// the comparison cannot detect a discontinuity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discontinuity {
    /// Path relative to the checkout.
    pub path: PathBuf,
    /// Checkout directory shared by both edits.
    pub checkout: PathBuf,
    /// ID of the earlier edit, whose after-state is compared.
    pub prev: EventId,
    /// ID of the next edit, whose before-state differs from that after-state.
    pub next: EventId,
}

/// Restores and discontinuities computed for one session at a given revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivationSet {
    /// Session whose edits were compared.
    pub session: SessionId,
    /// Record revision from which the input observations were read.
    pub observation_revision: Revision,
    /// Version of the comparison rules that produced these results.
    pub algorithm_version: u32,
    /// Edits that returned to earlier content, sorted by the restoring edit ID.
    pub restores: Vec<Restore>,
    /// Consecutive edits with mismatched file states.
    pub discontinuities: Vec<Discontinuity>,
}
