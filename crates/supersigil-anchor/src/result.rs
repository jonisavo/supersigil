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
