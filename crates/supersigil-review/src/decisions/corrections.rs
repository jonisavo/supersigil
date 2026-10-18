//! Corrections on the decisions in view: which decisions a standing
//! correction anchors to, and how it marks them.
//!
//! A correction names the decision it addressed by record, session,
//! revision and id. Revisions are numbered per record, and one session can
//! be held by two records, so only the full reference names a decision.
//!
//! - **Directly.** A correction anchors to the decision in view that its
//!   full reference names. When the review shows exactly the named
//!   revision of the session from the named record but that decision is
//!   not in view, it anchors to nothing: the revision's other decisions are
//!   not what was corrected.
//! - **Through cited edits.** When the review shows another revision of the
//!   session, or one from another record, the correction anchors to every
//!   decision in view of that session citing an edit the named decision
//!   cited, as read from the named record and revision, and says which
//!   revision it addressed. Only edits anchor: a command, such as a test
//!   run, can back decisions that have nothing else in common.
//! - **Unanchored.** A correction anchored to no decision in view is listed
//!   apart, never dropped: its decision is not in view, its reference could
//!   not be read, no edit is shared, or the review shows no revision of its
//!   session.
//! - **Marks.** A decision lists the corrections anchored to it oldest
//!   first, by time then judgment id. `wrong` and `not_together` dispute
//!   it. A `constraint` is also its first rationale line, a human's
//!   explicit statement with no source; constraints keep that order among
//!   themselves and precede every line the annotator wrote.
//! - **Seen.** A correction is seen when the shown revision of its session
//!   lists it among the corrections its annotator saw.
//!
//! A re-annotation clears no mark. Only withdrawing a correction does, and
//! the caller decides which corrections stand.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use supersigil_record::interpretations::{AnnotationRevision, Speaker};
use supersigil_record::reviews::{CorrectionKind, DecisionRef, Judgment};
use supersigil_record::{EventId, JudgmentId, RecordId, SessionId};

use super::Annotated;
use crate::model::{CorrectionInfo, DecisionInfo, RationaleInfo};

/// What the decision a correction names cited, read from the record and
/// revision its reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Addressed<'a> {
    /// The ids the decision cites in that revision.
    pub cites: &'a [EventId],
}

/// Anchors `corrections` to `decisions` by the [module's rules](self),
/// marking each decision a correction anchors to, and returns the
/// corrections anchored to none, oldest first.
///
/// `corrections` are the standing corrections in scope, in any order.
/// Whether a correction stands is the caller's to decide: one withdrawn is
/// anchored all the same. A judgment naming no decision or no correction
/// kind is not a correction and is skipped. `addressed` holds what each
/// reference's decision cited, for every reference the caller could read;
/// one it could not read is absent. `annotated` are the shown revisions,
/// one per annotated candidate session, and `decisions` the decisions in
/// view of them as [`in_view`](super::in_view) gives them, with no
/// corrections yet.
///
/// Indexes the decisions by full reference and, per session, by cited
/// edit, so besides sorting the corrections the time taken is linear in
/// the corrections, the decisions, the addressed cites, and the anchors
/// found.
#[must_use]
pub fn anchor(
    corrections: &[&Judgment],
    addressed: &BTreeMap<DecisionRef, Addressed<'_>>,
    annotated: &[Annotated<'_>],
    decisions: &mut [DecisionInfo],
) -> Vec<CorrectionInfo> {
    let mut corrections: Vec<(&Judgment, &DecisionRef, CorrectionKind)> = corrections
        .iter()
        .filter_map(|judgment| Some((*judgment, judgment.decision.as_ref()?, judgment.correction?)))
        .collect();
    corrections.sort_by(|(a, ..), (b, ..)| (a.at, &a.id).cmp(&(b.at, &b.id)));
    let shown = Shown::new(annotated);
    // Every anchor is found before any decision is marked.
    let placed: Vec<(CorrectionInfo, Vec<usize>)> = {
        let view = View::new(decisions);
        corrections
            .into_iter()
            .map(|(judgment, reference, kind)| {
                let (anchors, addressed) = view.anchors(reference, &shown, addressed);
                let info = CorrectionInfo {
                    judgment: judgment.id.clone(),
                    decision: reference.clone(),
                    kind,
                    reason: judgment.reason.clone(),
                    text: judgment.text.clone(),
                    who: judgment.who.clone(),
                    at: judgment.at,
                    addressed,
                    seen: shown.saw(reference, &judgment.id),
                    review: judgment.basis.clone(),
                };
                (info, anchors)
            })
            .collect()
    };
    let mut constraints: Vec<Vec<RationaleInfo>> = vec![Vec::new(); decisions.len()];
    let mut unanchored = Vec::new();
    for (info, anchors) in placed {
        if anchors.is_empty() {
            unanchored.push(info);
            continue;
        }
        for at in anchors {
            let decision = &mut decisions[at];
            decision.disputed |= disputes(info.kind);
            if let (CorrectionKind::Constraint, Some(text)) = (info.kind, &info.text) {
                constraints[at].push(RationaleInfo {
                    text: text.clone(),
                    speaker: Speaker::Human,
                    explicit: true,
                    source: None,
                    correction: Some(info.judgment.clone()),
                });
            }
            decision.corrections.push(info.clone());
        }
    }
    for (decision, lines) in decisions.iter_mut().zip(constraints) {
        decision.rationale.splice(0..0, lines);
    }
    unanchored
}

/// Whether a correction of `kind` disputes the decision it anchors to.
const fn disputes(kind: CorrectionKind) -> bool {
    match kind {
        CorrectionKind::Wrong | CorrectionKind::NotTogether => true,
        CorrectionKind::Constraint => false,
    }
}

/// Whether `id` names an edit. The record derives edit ids as `edit:<hex>`
/// and command ids as `command:<hex>`.
fn is_edit(id: &EventId) -> bool {
    id.as_str().starts_with("edit:")
}

/// The revisions a review shows, one per annotated session.
struct Shown<'a> {
    /// Each shown session's record and revision.
    revisions: HashMap<&'a SessionId, (&'a RecordId, AnnotationRevision)>,
    /// Each shown session with every correction its revision's annotator
    /// saw.
    seen: HashSet<(&'a SessionId, &'a JudgmentId)>,
}

impl<'a> Shown<'a> {
    fn new(annotated: &[Annotated<'a>]) -> Self {
        let revisions = annotated
            .iter()
            .map(|entry| (entry.session, (entry.record, entry.annotation.revision)))
            .collect();
        let seen = annotated
            .iter()
            .flat_map(|entry| {
                entry
                    .annotation
                    .document
                    .corrections_seen
                    .iter()
                    .map(|judgment| (entry.session, judgment))
            })
            .collect();
        Self { revisions, seen }
    }

    /// Whether the review shows exactly the revision `reference` names,
    /// from the record it names.
    fn shows(&self, reference: &DecisionRef) -> bool {
        self.revisions.get(&reference.session) == Some(&(&reference.record, reference.revision))
    }

    /// Whether the shown revision of `reference`'s session lists
    /// `judgment` among the corrections its annotator saw.
    fn saw(&self, reference: &DecisionRef, judgment: &JudgmentId) -> bool {
        self.seen.contains(&(&reference.session, judgment))
    }
}

/// The decisions in view, indexed for anchoring.
struct View<'d> {
    /// Each decision's position by its full reference.
    by_reference: HashMap<(&'d RecordId, &'d SessionId, AnnotationRevision, &'d str), usize>,
    /// Per session and cited edit, the positions of the decisions citing
    /// it.
    citing: HashMap<(&'d SessionId, &'d EventId), Vec<usize>>,
}

impl<'d> View<'d> {
    fn new(decisions: &'d [DecisionInfo]) -> Self {
        let mut by_reference = HashMap::with_capacity(decisions.len());
        let mut citing: HashMap<_, Vec<usize>> = HashMap::new();
        for (at, decision) in decisions.iter().enumerate() {
            let reference = (
                &decision.record,
                &decision.session,
                decision.revision,
                decision.id.as_str(),
            );
            by_reference.insert(reference, at);
            let cites = decision
                .cites
                .in_change
                .iter()
                .chain(&decision.cites.elsewhere);
            for cite in cites.filter(|cite| is_edit(cite)) {
                citing
                    .entry((&decision.session, cite))
                    .or_default()
                    .push(at);
            }
        }
        Self {
            by_reference,
            citing,
        }
    }

    /// The positions of the decisions `reference` anchors to, ascending,
    /// with the revision it addressed when it anchors through cited edits.
    fn anchors(
        &self,
        reference: &DecisionRef,
        shown: &Shown<'_>,
        addressed: &BTreeMap<DecisionRef, Addressed<'_>>,
    ) -> (Vec<usize>, Option<AnnotationRevision>) {
        let key = (
            &reference.record,
            &reference.session,
            reference.revision,
            reference.id.as_str(),
        );
        if let Some(&at) = self.by_reference.get(&key) {
            return (vec![at], None);
        }
        if shown.shows(reference) {
            return (Vec::new(), None);
        }
        let Some(addressed) = addressed.get(reference) else {
            return (Vec::new(), None);
        };
        let anchors: BTreeSet<usize> = addressed
            .cites
            .iter()
            .filter_map(|cite| self.citing.get(&(&reference.session, cite)))
            .flatten()
            .copied()
            .collect();
        let revision = (!anchors.is_empty()).then_some(reference.revision);
        (anchors.into_iter().collect(), revision)
    }
}
