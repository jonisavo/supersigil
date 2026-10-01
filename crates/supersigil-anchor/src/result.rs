//! Results of attribution: why walks stop, the chains they find, and what
//! each line's history is.

use std::collections::BTreeSet;

use serde::Serializer;
use supersigil_record::{EventId, RecordId};

use crate::walk::AcceptedEdit;

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

/// Where a target line's content came from, in one chain.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Origin {
    /// The content of this base line (0-based; 1-based in JSON).
    Base(#[serde(serialize_with = "one_based")] usize),
    /// Written by this edit.
    Introduced(EventId),
    /// A line of only whitespace that this whitespace-only change added.
    WhitespaceAdded(EventId),
    /// Inherited from before the chain's start: the chain does not explain it.
    Unexplained,
}

/// A target line's origins and contributors, in one chain or agreed by all.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Provenance {
    /// Where the line's content came from.
    pub origins: BTreeSet<Origin>,
    /// Edits that changed only whitespace on the way. Removing or adding
    /// whitespace can still change behavior inside strings or in
    /// indentation-sensitive code, so these are observations, not a claim
    /// that only presentation changed.
    pub whitespace_only: BTreeSet<EventId>,
    /// Edits that participated earlier in the line's region.
    pub earlier: BTreeSet<EventId>,
}

/// What happened to a base line, in one chain.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fate {
    /// Removed by this edit outside a whitespace-only change.
    Replaced {
        /// The edit.
        edit: EventId,
    },
    /// Its characters reached this target line through whitespace-only edits.
    CarriedTo {
        /// The target line (0-based; 1-based in JSON).
        #[serde(serialize_with = "one_based")]
        line: usize,
        /// The whitespace-only edits recorded on that line.
        via: BTreeSet<EventId>,
    },
    /// A line of only whitespace removed by this whitespace-only change.
    RemovedByWhitespace {
        /// The edit.
        edit: EventId,
    },
    /// Unchanged, as this target line (0-based).
    KeptAs {
        /// The target line (0-based; 1-based in JSON).
        #[serde(serialize_with = "one_based")]
        line: usize,
    },
}

/// Writes a 0-based line number as the 1-based one JSON shows.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's serialize_with passes the field by reference"
)]
fn one_based<S: Serializer>(line: &usize, serializer: S) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&line.saturating_add(1), serializer)
}

/// One chain's reading of a line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Reading<T> {
    /// The chain's id.
    pub chain: usize,
    /// The chain's class.
    pub class: ChainClass,
    /// What the chain says about the line.
    pub value: T,
}

/// The combined outcome of a target line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineOutcome {
    /// Every chain gives the identical provenance: origins, whitespace-only
    /// and earlier contributors. `class` is the strongest among them, or
    /// `None` when there is no chain (origins then `Unexplained`).
    Agreed {
        /// The provenance every chain gives.
        provenance: Provenance,
        /// The strongest class among the chains.
        class: Option<ChainClass>,
    },
    /// The chains disagree; each reading is listed.
    Ambiguous {
        /// Every chain's reading.
        readings: Vec<Reading<Provenance>>,
    },
}

/// The combined outcome of a base line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BaseLineOutcome {
    /// Every chain gives the same fates; empty fates mean no fate.
    Agreed {
        /// The agreed fates.
        fates: BTreeSet<Fate>,
        /// The strongest class among the chains, `None` without chains.
        class: Option<ChainClass>,
    },
    /// The chains disagree; each reading is listed.
    Ambiguous {
        /// Every chain's reading.
        readings: Vec<Reading<BTreeSet<Fate>>>,
    },
}

/// Edits whose retained text occurs as a whole block over each line, for
/// the lines that need the fallback.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct ContentMatches {
    /// Per target line, edits whose `new_text` (or written content) covers it.
    pub target: Vec<BTreeSet<EventId>>,
    /// Per base line, edits whose `old_text` covers it.
    pub base: Vec<BTreeSet<EventId>>,
    /// Whether matching ran out of budget before finishing: lines without a
    /// match may then have one that was never looked for.
    pub incomplete: bool,
}

/// Everything anchor established about one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathAttribution {
    /// Whether the search completed and found anything.
    pub status: PathStatus,
    /// The chains not set aside, whose readings are combined line by line;
    /// when replaying them ran out of budget, only those replayed in full
    /// are.
    pub chains: Vec<Chain>,
    /// Alternatives that assume an unrecorded change.
    pub set_aside: Vec<Chain>,
    /// Edits excluded as conflicting evidence.
    pub conflicts: Vec<Conflict>,
    /// The deduplicated edits, for the review's edit details.
    pub accepted: Vec<AcceptedEdit>,
    /// One outcome per target line.
    pub target: Vec<LineOutcome>,
    /// One outcome per base line.
    pub base: Vec<BaseLineOutcome>,
    /// Whole-block content matches per line.
    pub content: ContentMatches,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json<T: serde::Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(value).unwrap()
    }

    #[test]
    fn line_numbers_serialize_one_based() {
        assert_eq!(
            json(&Origin::Base(0)),
            serde_json::json!({"kind": "base", "value": 1})
        );
        assert_eq!(
            json(&Fate::KeptAs { line: 0 }),
            serde_json::json!({"kind": "kept_as", "line": 1})
        );
        assert_eq!(
            json(&Fate::CarriedTo {
                line: 2,
                via: BTreeSet::from([EventId::new("e")])
            }),
            serde_json::json!({"kind": "carried_to", "line": 3, "via": ["e"]})
        );
    }
}
