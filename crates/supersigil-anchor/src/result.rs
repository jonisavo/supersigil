//! Results of attribution: why a reversal or a chain stopped.

use supersigil_record::EventId;

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
