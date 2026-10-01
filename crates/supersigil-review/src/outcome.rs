//! One outer outcome per diff line, decided by the ordered rules below.
//!
//! Anchor gives every target line a combined provenance (a set of origins
//! plus contributors) and every base line a set of fates, or marks the line
//! ambiguous. The rules, in order:
//!
//! 1. the path's search is incomplete: *unresolved*;
//! 2. the line is ambiguous: *ambiguous*;
//! 3. an added line with any unexplained origin, or a removed line with no
//!    fate: *content match* when whole-edit blocks cover it, otherwise
//!    *unattributed* with the reason, which is that content matching ran
//!    out of budget whenever it did, since a match may exist that was never
//!    looked for. A partly explained line lands here, so it is never shown
//!    as attributed;
//! 4. an introducing edit among the origins, or a replacing edit among the
//!    fates: *attributed*;
//! 5. everything is base content: *whitespace-only change* when whitespace
//!    contributors exist, *line ending changed* when the blob lines still
//!    differ, *realigned* when they are byte-identical.
//!
//! Every outcome comes with the line's full provenance, anchor's combined
//! outcome serialized as-is, so nothing the rules summarize is lost.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;
use supersigil_anchor::{
    BaseLineOutcome, ChainClass, Fate, LineOutcome, Origin, PathAttribution, PathStatus, Provenance,
};
use supersigil_record::EventId;

/// How an edit contributed to an attributed line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// The edit wrote the retained content of an added line.
    Introduced,
    /// The edit removed a base line outside a whitespace-only change.
    Replaced,
}

/// One edit an attributed line names, with the classes of the chains that
/// agree on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttributedEdit {
    /// The edit.
    pub edit: EventId,
    /// How it contributed.
    pub relation: Relation,
    /// The strongest class among the chains that agree on the line.
    pub classes: BTreeSet<ChainClass>,
}

/// Why a diff line has no attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnattributedReason {
    /// No chain survived for the path; anchor's status says why.
    NoSurvivingChain {
        /// Anchor's status for the path, with its stop reasons.
        status: PathStatus,
    },
    /// Chains exist, but the line is inherited from before their start.
    GapBeforeChain,
    /// No whole-edit block was found to cover the line, but content
    /// matching ran out of budget first: one may exist.
    ContentMatchIncomplete,
    /// The file's attribution could not be computed.
    AttributionUnavailable {
        /// Why, for example "conversion changed line structure".
        reason: String,
    },
}

/// The one outer outcome of a diff line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// The search ran out of budget: the chains' readings for this line are
    /// candidate evidence, never a result.
    Unresolved {
        /// Anchor's combined outcome for the line, serialized as-is.
        candidates: Value,
    },
    /// The surviving chains disagree about this line.
    Ambiguous {
        /// Every chain's reading with its class, serialized as-is.
        readings: Value,
    },
    /// No chain explains the line, but whole edit blocks cover it.
    ContentMatch {
        /// Introduced for added lines, replaced for removed lines.
        relation: Relation,
        /// Every covering edit; nothing picks among them.
        edits: Vec<EventId>,
    },
    /// Nothing explains the line.
    Unattributed {
        /// Why.
        reason: UnattributedReason,
    },
    /// An edit introduced the added line or replaced the removed one.
    Attributed {
        /// The introducing or replacing edits.
        edits: Vec<AttributedEdit>,
    },
    /// Only whitespace changed, by these edits.
    WhitespaceOnly {
        /// The whitespace-only contributors.
        edits: Vec<EventId>,
    },
    /// The blob lines differ only in line ending, which no edit explains.
    LineEndingChanged,
    /// Every chain carries the line unchanged; the diff aligned it
    /// differently.
    Realigned {
        /// The corresponding line on the other side (0-based; 1-based in
        /// JSON).
        #[serde(serialize_with = "supersigil_anchor::one_based")]
        line: usize,
    },
}

/// A file's attribution as the rules see it.
#[derive(Debug, Clone)]
pub enum AttributionState<'a> {
    /// Anchor's result for the path.
    Available(&'a PathAttribution),
    /// Attribution could not be computed, with the reason.
    Unavailable {
        /// Why, for example "conversion changed line structure".
        reason: String,
    },
}

/// A line's outcome before what every line of a path shares is copied into
/// it: the path's status when no chain survived (one stop reason per
/// rejected head) and the reason attribution is unavailable. Copying either
/// per line costs lines times its size, so spans group lines on verdicts,
/// which tell shared parts apart by address, and copy them once per span
/// ([`Verdict::into_outcome`]).
#[derive(Debug)]
pub(crate) enum Verdict<'a> {
    /// An outcome that shares nothing with other lines.
    Decided(Outcome),
    /// Unattributed because no chain survived, with anchor's status for the
    /// path.
    NoSurvivingChain(&'a PathStatus),
    /// Unattributed because the file's attribution could not be computed,
    /// with the reason.
    Unavailable(&'a str),
}

impl PartialEq for Verdict<'_> {
    fn eq(&self, other: &Self) -> bool {
        // Every line of a path borrows the same status and reason, so equal
        // addresses decide without comparing what they hold.
        match (self, other) {
            (Self::Decided(a), Self::Decided(b)) => a == b,
            (Self::NoSurvivingChain(a), Self::NoSurvivingChain(b)) => {
                std::ptr::eq(*a, *b) || a == b
            }
            (Self::Unavailable(a), Self::Unavailable(b)) => std::ptr::eq(*a, *b) || a == b,
            _ => false,
        }
    }
}

impl Verdict<'_> {
    /// The outcome, with its own copy of any shared part.
    pub(crate) fn into_outcome(self) -> Outcome {
        match self {
            Self::Decided(outcome) => outcome,
            Self::NoSurvivingChain(status) => Outcome::Unattributed {
                reason: UnattributedReason::NoSurvivingChain {
                    status: status.clone(),
                },
            },
            Self::Unavailable(reason) => Outcome::Unattributed {
                reason: UnattributedReason::AttributionUnavailable {
                    reason: reason.to_owned(),
                },
            },
        }
    }
}

/// The outcome of added diff line `line` (0-based target line) and its full
/// provenance (anchor's `LineOutcome` as JSON, or null when attribution is
/// unavailable).
///
/// `base_blob_lines` and `target_blob_lines` are the blob lines, used only to
/// tell *realigned* from *line ending changed*.
#[must_use]
pub fn target_line_outcome(
    attribution: &AttributionState<'_>,
    line: usize,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> (Outcome, Value) {
    let (verdict, provenance) =
        target_line_verdict(attribution, line, base_blob_lines, target_blob_lines);
    (verdict.into_outcome(), provenance)
}

/// [`target_line_outcome`] with the shared part of the outcome borrowed.
pub(crate) fn target_line_verdict<'s>(
    attribution: &'s AttributionState<'_>,
    line: usize,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> (Verdict<'s>, Value) {
    let missing = LineOutcome::Agreed {
        provenance: Provenance {
            origins: BTreeSet::from([Origin::Unexplained]),
            ..Provenance::default()
        },
        class: None,
    };
    line_verdict(
        attribution,
        line,
        |attr| &attr.target,
        &missing,
        |attr, combined| match combined {
            LineOutcome::Ambiguous { readings } => Verdict::Decided(Outcome::Ambiguous {
                readings: json(readings),
            }),
            LineOutcome::Agreed { provenance, class } => agreed_target(
                attr,
                line,
                provenance,
                *class,
                base_blob_lines,
                target_blob_lines,
            ),
        },
    )
}

/// The outcome of removed diff line `line` (0-based base line) and its full
/// provenance (anchor's `BaseLineOutcome` as JSON, or null when attribution
/// is unavailable).
#[must_use]
pub fn base_line_outcome(
    attribution: &AttributionState<'_>,
    line: usize,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> (Outcome, Value) {
    let (verdict, provenance) =
        base_line_verdict(attribution, line, base_blob_lines, target_blob_lines);
    (verdict.into_outcome(), provenance)
}

/// [`base_line_outcome`] with the shared part of the outcome borrowed.
pub(crate) fn base_line_verdict<'s>(
    attribution: &'s AttributionState<'_>,
    line: usize,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> (Verdict<'s>, Value) {
    let missing = BaseLineOutcome::Agreed {
        fates: BTreeSet::new(),
        class: None,
    };
    line_verdict(
        attribution,
        line,
        |attr| &attr.base,
        &missing,
        |attr, combined| match combined {
            BaseLineOutcome::Ambiguous { readings } => Verdict::Decided(Outcome::Ambiguous {
                readings: json(readings),
            }),
            BaseLineOutcome::Agreed { fates, class } => agreed_base(
                attr,
                line,
                fates,
                *class,
                base_blob_lines,
                target_blob_lines,
            ),
        },
    )
}

/// What both sides share: unavailable attribution, the default for a line
/// anchor has no entry for, and an incomplete search. `decide` gives the
/// verdict for a complete search.
fn line_verdict<'s, T: Serialize>(
    attribution: &'s AttributionState<'_>,
    line: usize,
    side: impl FnOnce(&PathAttribution) -> &[T],
    missing: &T,
    decide: impl FnOnce(&'s PathAttribution, &T) -> Verdict<'s>,
) -> (Verdict<'s>, Value) {
    let attr: &'s PathAttribution = match attribution {
        AttributionState::Unavailable { reason } => {
            return (Verdict::Unavailable(reason), Value::Null);
        }
        AttributionState::Available(attr) => attr,
    };
    let combined = side(attr).get(line).unwrap_or(missing);
    let provenance = json(combined);
    if attr.status == PathStatus::SearchIncomplete {
        return (
            Verdict::Decided(Outcome::Unresolved {
                candidates: provenance.clone(),
            }),
            provenance,
        );
    }
    (decide(attr, combined), provenance)
}

/// Rules 3 to 5 for an added line every chain agrees on.
fn agreed_target<'s>(
    attr: &'s PathAttribution,
    line: usize,
    provenance: &Provenance,
    class: Option<ChainClass>,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> Verdict<'s> {
    if provenance.origins.is_empty() || provenance.origins.contains(&Origin::Unexplained) {
        return fallback(attr, &attr.content.target, line, Relation::Introduced);
    }
    Verdict::Decided(explained_target(
        line,
        provenance,
        class,
        base_blob_lines,
        target_blob_lines,
    ))
}

/// Rules 4 and 5 for an added line whose every origin is explained.
fn explained_target(
    line: usize,
    provenance: &Provenance,
    class: Option<ChainClass>,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> Outcome {
    let introduced: Vec<AttributedEdit> = provenance
        .origins
        .iter()
        .filter_map(|origin| match origin {
            Origin::Introduced(edit) => Some(AttributedEdit {
                edit: edit.clone(),
                relation: Relation::Introduced,
                classes: class.into_iter().collect(),
            }),
            _ => None,
        })
        .collect();
    if !introduced.is_empty() {
        return Outcome::Attributed { edits: introduced };
    }
    let mut whitespace = provenance.whitespace_only.clone();
    for origin in &provenance.origins {
        if let Origin::WhitespaceAdded(edit) = origin {
            whitespace.insert(edit.clone());
        }
    }
    if !whitespace.is_empty() {
        return Outcome::WhitespaceOnly {
            edits: whitespace.into_iter().collect(),
        };
    }
    let bases: Vec<usize> = provenance
        .origins
        .iter()
        .filter_map(|origin| match origin {
            Origin::Base(j) => Some(*j),
            _ => None,
        })
        .collect();
    let target_text = target_blob_lines.get(line);
    let identical = bases.iter().all(|j| {
        base_blob_lines
            .get(*j)
            .is_some_and(|b| Some(b) == target_text)
    });
    match bases.first() {
        Some(&first) if identical => Outcome::Realigned { line: first },
        _ => Outcome::LineEndingChanged,
    }
}

/// Rules 3 to 5 for a removed line every chain agrees on.
fn agreed_base<'s>(
    attr: &'s PathAttribution,
    line: usize,
    fates: &BTreeSet<Fate>,
    class: Option<ChainClass>,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> Verdict<'s> {
    if fates.is_empty() {
        return fallback(attr, &attr.content.base, line, Relation::Replaced);
    }
    Verdict::Decided(fated_base(
        line,
        fates,
        class,
        base_blob_lines,
        target_blob_lines,
    ))
}

/// Rules 4 and 5 for a removed line with at least one fate.
fn fated_base(
    line: usize,
    fates: &BTreeSet<Fate>,
    class: Option<ChainClass>,
    base_blob_lines: &[&[u8]],
    target_blob_lines: &[&[u8]],
) -> Outcome {
    let replaced: Vec<AttributedEdit> = fates
        .iter()
        .filter_map(|fate| match fate {
            Fate::Replaced { edit } => Some(AttributedEdit {
                edit: edit.clone(),
                relation: Relation::Replaced,
                classes: class.into_iter().collect(),
            }),
            _ => None,
        })
        .collect();
    if !replaced.is_empty() {
        return Outcome::Attributed { edits: replaced };
    }
    let mut whitespace = BTreeSet::new();
    let mut kept = None;
    for fate in fates {
        match fate {
            Fate::CarriedTo { via, line } => {
                whitespace.extend(via.iter().cloned());
                kept.get_or_insert(*line);
            }
            Fate::RemovedByWhitespace { edit } => {
                whitespace.insert(edit.clone());
            }
            Fate::KeptAs { line } => {
                kept.get_or_insert(*line);
            }
            Fate::Replaced { .. } => {}
        }
    }
    if !whitespace.is_empty() {
        return Outcome::WhitespaceOnly {
            edits: whitespace.into_iter().collect(),
        };
    }
    match kept {
        Some(k)
            if base_blob_lines.get(line).is_some()
                && base_blob_lines.get(line) == target_blob_lines.get(k) =>
        {
            Outcome::Realigned { line: k }
        }
        _ => Outcome::LineEndingChanged,
    }
}

/// Rule 3's content-match fallback, then the unattributed reason.
fn fallback<'s>(
    attr: &'s PathAttribution,
    matches: &[BTreeSet<EventId>],
    line: usize,
    relation: Relation,
) -> Verdict<'s> {
    if let Some(edits) = matches.get(line).filter(|edits| !edits.is_empty()) {
        return Verdict::Decided(Outcome::ContentMatch {
            relation,
            edits: edits.iter().cloned().collect(),
        });
    }
    let unattributed = |reason| Verdict::Decided(Outcome::Unattributed { reason });
    if attr.content.incomplete {
        unattributed(UnattributedReason::ContentMatchIncomplete)
    } else if attr.chains.is_empty() {
        Verdict::NoSurvivingChain(&attr.status)
    } else {
        unattributed(UnattributedReason::GapBeforeChain)
    }
}

/// Serializes anchor's outcome types. They contain no map with non-string
/// keys, so a failure is not expected; if one happens it is shown as an
/// error object, never as null (null means attribution is unavailable).
fn json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value)
        .unwrap_or_else(|error| serde_json::json!({ "serialization_error": error.to_string() }))
}
