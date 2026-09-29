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
    /// Logical document name for a session's derivations. The session part is
    /// the session's storage key, so an identifier taken from a transcript
    /// never becomes a raw path component.
    #[must_use]
    pub fn document_name(session: &SessionId) -> String {
        format!("derivations/{}", crate::store::storage_key(session))
    }
}
