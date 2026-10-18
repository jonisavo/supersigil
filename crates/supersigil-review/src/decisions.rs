//! The decisions behind a change: which record supplies each candidate
//! session's annotation, how decisions are named, which decisions a review
//! shows and how each explains the lines of the change, open questions as
//! gate items, what a basis pins, and how a re-annotation shows between
//! reviews.
//!
//! - **Which record.** When several involved records hold observations of
//!   one session, one record supplies its annotation, its basis pin and its
//!   open questions ([`choose_record`]): a record with an annotation of the
//!   session beats one without, then the record whose pinned observation
//!   logs for the session are longest in bytes, then the smallest record
//!   id. The rule is per session, not per transcript.
//! - **Names.** A decision is named by its id, qualified as
//!   `<session short>:<id>` when another annotated session has a decision
//!   with the same id, whether or not that one is in view. A session's short
//!   form is its shortest prefix of at least 8 characters unique among the
//!   annotated sessions ([`Names`]). Names therefore depend on the set of
//!   annotated sessions. Open-question items are named by the gate
//!   ([`ItemNames`]), from its question items alone.
//! - **In view.** A span contributes the edits its outcome names when it is
//!   attributed, whitespace-only, or a content match; other outcomes
//!   contribute none. A decision is in view when a contributing edit is one
//!   it cites, and it explains each span such an edit contributes to,
//!   saying how ([`in_view`]). A content match is never attribution: no
//!   chain explains the span and nothing selects among the covering edits,
//!   so several decisions may match one span. A decision citing nothing
//!   that contributes is history, which no review shows yet.
//! - **Alternatives.** An attempt is restored when the session's
//!   derivations show every edit of it restored.
//! - **Questions.** An open question is a gate item when it is about
//!   nothing or about an edit that contributes ([`question_items`]); one
//!   about only commands or other edits is not an item in this review. Its
//!   fingerprint hashes its text and its sorted ids apart, so rewording
//!   reopens it naming `text`, and changed ids naming `about`. A decision
//!   lists the session's question items about an id it cites.
//! - **Basis.** A basis pins each candidate session's chosen record and
//!   revision, and each decision in view ([`basis_entries`]).
//! - **Status.** A related review's interpretation changed for each session
//!   whose pinned annotation differs ([`interpretation_changes`]). Revisions
//!   are numbered per record, so a pin in another record differs even with
//!   the same revision number.
//!
//! No annotation reaches an evidence fingerprint: decisions are kept beside
//! the file and edit entries that explanation digests and gate fingerprints
//! hash, so a re-annotation reopens no existing item. New open questions
//! are new items.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use supersigil_record::derivations::Restore;
use supersigil_record::interpretations::{
    Alternative, Annotation, AnnotationRevision, Decision, Implication, OpenQuestion, Rationale,
};
use supersigil_record::reviews::{
    BasisAnnotation, BasisDecision, Fingerprint, FingerprintPart, GateDetail, GateItem, Subject,
};
use supersigil_record::{EventId, RecordId, SessionId};

use crate::model::{
    AlternativeInfo, Association, CitesInfo, DecisionInfo, ExplainedSpan, FileReview,
    ImplicationInfo, InterpretationChange, RationaleInfo, outcome_edits,
};
use crate::names::{ItemNames, SESSION_PREFIX, unique_prefix};
use crate::outcome::Outcome;

/// A record holding observations of a candidate session, as
/// [`choose_record`] compares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordCandidate<'a> {
    /// The record.
    pub record: &'a RecordId,
    /// Whether it holds an annotation of the session.
    pub annotated: bool,
    /// The pinned lengths of the session's observation logs in it, summed
    /// over its transcripts, in bytes.
    pub log_bytes: u64,
}

/// Chooses which of `candidates` supplies a session's annotation, by the
/// [module's rule](self): annotated first, then the longest logs, then the
/// smallest record id. The choice is the same for any order of
/// `candidates`, and `None` when there is none.
#[must_use]
pub fn choose_record<'a>(candidates: &[RecordCandidate<'a>]) -> Option<&'a RecordId> {
    candidates
        .iter()
        .max_by(|a, b| {
            (a.annotated, a.log_bytes)
                .cmp(&(b.annotated, b.log_bytes))
                .then_with(|| b.record.cmp(a.record))
        })
        .map(|candidate| candidate.record)
}

/// A candidate session's current annotation in the record chosen for it.
#[derive(Debug, Clone, Copy)]
pub struct Annotated<'a> {
    /// The session.
    pub session: &'a SessionId,
    /// The record chosen for it.
    pub record: &'a RecordId,
    /// Its current annotation in that record.
    pub annotation: &'a Annotation,
    /// The restores among the session's derivations in that record.
    pub restores: &'a [Restore],
}

/// Names decisions for one review, by the [module's rule](self). Names
/// depend on the set of annotated sessions: annotating another session
/// that uses a decision's id qualifies that decision's name.
#[derive(Debug, Clone)]
pub struct Names<'a> {
    /// The annotated sessions' ids with their positions, sorted.
    sessions: Vec<(&'a str, usize)>,
    /// The decision ids that more than one annotated session has.
    repeated: HashSet<&'a str>,
}

/// Names the decisions of the `annotated` sessions.
#[must_use]
pub fn names<'a>(annotated: &[Annotated<'a>]) -> Names<'a> {
    let mut sessions: Vec<(&str, usize)> = annotated
        .iter()
        .enumerate()
        .map(|(at, entry)| (entry.session.as_str(), at))
        .collect();
    sessions.sort_unstable();
    let mut first: HashMap<&str, &SessionId> = HashMap::new();
    let mut repeated = HashSet::new();
    for entry in annotated {
        for decision in &entry.annotation.document.decisions {
            match first.entry(&decision.id) {
                Entry::Vacant(vacant) => {
                    vacant.insert(entry.session);
                }
                Entry::Occupied(occupied) => {
                    if *occupied.get() != entry.session {
                        repeated.insert(decision.id.as_str());
                    }
                }
            }
        }
    }
    Names { sessions, repeated }
}

impl Names<'_> {
    /// The name of `session`'s decision `id`: the id, or `<short>:<id>`
    /// with the session's [short form](Self::short) when another annotated
    /// session has a decision with that id.
    #[must_use]
    pub fn decision(&self, session: &SessionId, id: &str) -> String {
        if self.repeated.contains(id) {
            format!("{}:{id}", self.short(session))
        } else {
            id.to_owned()
        }
    }

    /// `session`'s shortest prefix of at least 8 characters unique among
    /// the annotated sessions, or its whole id when that is shorter.
    #[must_use]
    pub fn short<'s>(&self, session: &'s SessionId) -> &'s str {
        unique_prefix(session.as_str(), &self.sessions, SESSION_PREFIX)
    }
}

/// The decisions in view of a change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InView {
    /// The decisions citing an edit that contributes to a line of the
    /// change, sorted by name. None lists corrections or is disputed yet:
    /// anchoring corrections is a later step.
    pub decisions: Vec<DecisionInfo>,
    /// Per edit id in the change, the sorted names of the decisions citing
    /// it. An edit no decision cites is absent.
    pub edit_decisions: BTreeMap<String, Vec<String>>,
    /// Every edit id that contributes to a line of the change.
    pub in_change: BTreeSet<String>,
}

/// Finds the decisions of the `annotated` sessions that explain `files`,
/// named by `names`, by the [module's rules](self).
///
/// Indexes each decision by its cited ids once and walks every span of
/// every file once, so the time taken is linear in the decisions, the
/// spans, and the explanations found.
#[must_use]
pub fn in_view(annotated: &[Annotated<'_>], names: &Names<'_>, files: &[FileReview]) -> InView {
    let decisions: Vec<(usize, &Decision)> = annotated
        .iter()
        .enumerate()
        .flat_map(|(at, entry)| {
            entry
                .annotation
                .document
                .decisions
                .iter()
                .map(move |decision| (at, decision))
        })
        .collect();
    let (explains, in_change) = explanations(&decisions, files);
    let shown = Shown {
        annotated,
        names,
        in_change: &in_change,
        asking: asking(annotated, &in_change),
        restored: annotated
            .iter()
            .map(|entry| {
                entry
                    .restores
                    .iter()
                    .flat_map(|restore| &restore.restores)
                    .collect()
            })
            .collect(),
    };
    let mut infos: Vec<DecisionInfo> = decisions
        .iter()
        .zip(explains)
        .filter(|(_, explains)| !explains.is_empty())
        .map(|(&(at, decision), explains)| shown.info(at, decision, explains))
        .collect();
    infos.sort_by(|a, b| a.name.cmp(&b.name));
    let mut edit_decisions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // Decisions are sorted, so each edit's names are pushed in order.
    for info in &infos {
        for edit in &info.cites.in_change {
            edit_decisions
                .entry(edit.as_str().to_owned())
                .or_default()
                .push(info.name.clone());
        }
    }
    InView {
        decisions: infos,
        edit_decisions,
        in_change,
    }
}

/// Each decision's explained spans, by position in `decisions`, and every
/// edit id that contributes to a line of `files`.
fn explanations(
    decisions: &[(usize, &Decision)],
    files: &[FileReview],
) -> (Vec<Vec<ExplainedSpan>>, BTreeSet<String>) {
    // Each cited id's decisions, by position, each once.
    let mut citing: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, (_, decision)) in decisions.iter().enumerate() {
        for cite in &decision.cites {
            let citers = citing.entry(cite.as_str()).or_default();
            if citers.last() != Some(&index) {
                citers.push(index);
            }
        }
    }
    let mut explains = vec![Vec::new(); decisions.len()];
    // The last span each decision explains, by its number in the walk, so
    // a span with several cited edits is listed once.
    let mut last = vec![None; decisions.len()];
    let mut in_change = BTreeSet::new();
    let spans = files.iter().flat_map(|file| {
        file.hunks
            .iter()
            .flat_map(|hunk| &hunk.spans)
            .map(move |span| (file, span))
    });
    for (number, (file, span)) in spans.enumerate() {
        let Some((association, search_complete)) = association(&span.outcome) else {
            continue;
        };
        for edit in outcome_edits(&span.outcome) {
            if !in_change.contains(edit.as_str()) {
                in_change.insert(edit.as_str().to_owned());
            }
            for &index in citing.get(edit.as_str()).into_iter().flatten() {
                if last[index] != Some(number) {
                    last[index] = Some(number);
                    explains[index].push(ExplainedSpan {
                        path: file.path_bytes.clone(),
                        side: span.side,
                        start: span.start,
                        count: span.count,
                        association,
                        search_complete,
                    });
                }
            }
        }
    }
    (explains, in_change)
}

/// How `outcome` names its edits, and for a content match whether its
/// search completed; `None` for an outcome that names no contributing edit.
const fn association(outcome: &Outcome) -> Option<(Association, Option<bool>)> {
    match outcome {
        Outcome::Attributed { .. } => Some((Association::Attributed, None)),
        Outcome::WhitespaceOnly { .. } => Some((Association::WhitespaceOnly, None)),
        Outcome::ContentMatch {
            search_complete, ..
        } => Some((Association::ContentMatch, Some(*search_complete))),
        Outcome::Unresolved { .. }
        | Outcome::Ambiguous { .. }
        | Outcome::Unattributed { .. }
        | Outcome::LineEndingChanged
        | Outcome::Realigned { .. } => None,
    }
}

/// Each session's question items, named as the gate names them, by an id
/// they are about.
fn asking<'a>(
    annotated: &[Annotated<'a>],
    in_change: &BTreeSet<String>,
) -> HashMap<(&'a SessionId, &'a str), Vec<String>> {
    let (asked, items): (Vec<_>, Vec<_>) = asked(annotated, in_change).into_iter().unzip();
    let names = ItemNames::new(&items, BTreeMap::new(), false, |_| None);
    let mut asking: HashMap<_, Vec<String>> = HashMap::new();
    for ((session, question), name) in asked.into_iter().zip(names.names()) {
        for about in &question.about {
            asking
                .entry((session, about.as_str()))
                .or_default()
                .push(name.clone());
        }
    }
    asking
}

/// What a shown decision's details are built from.
struct Shown<'v, 'a> {
    annotated: &'v [Annotated<'a>],
    names: &'v Names<'v>,
    in_change: &'v BTreeSet<String>,
    asking: HashMap<(&'a SessionId, &'a str), Vec<String>>,
    /// The edits each session's derivations show restored, by position in
    /// `annotated`.
    restored: Vec<HashSet<&'a EventId>>,
}

impl Shown<'_, '_> {
    /// The details of `decision`, from the session at `at`, explaining
    /// `explains`.
    fn info(&self, at: usize, decision: &Decision, explains: Vec<ExplainedSpan>) -> DecisionInfo {
        let entry = &self.annotated[at];
        let cites: BTreeSet<&EventId> = decision.cites.iter().collect();
        let (in_change, elsewhere): (Vec<&EventId>, Vec<&EventId>) = cites
            .iter()
            .partition(|cite| self.in_change.contains(cite.as_str()));
        let open_questions: BTreeSet<&String> = cites
            .iter()
            .filter_map(|cite| self.asking.get(&(entry.session, cite.as_str())))
            .flatten()
            .collect();
        DecisionInfo {
            name: self.names.decision(entry.session, &decision.id),
            session: entry.session.clone(),
            record: entry.record.clone(),
            revision: entry.annotation.revision,
            id: decision.id.clone(),
            title: decision.title.clone(),
            disputed: false,
            corrections: Vec::new(),
            rationale: decision.rationale.iter().map(rationale).collect(),
            alternatives: decision
                .alternatives
                .iter()
                .map(|alternative| self.alternative(at, alternative))
                .collect(),
            implications: decision.implications.iter().map(implication).collect(),
            open_questions: open_questions.into_iter().cloned().collect(),
            cites: CitesInfo {
                in_change: in_change.into_iter().cloned().collect(),
                elsewhere: elsewhere.into_iter().cloned().collect(),
            },
            explains,
        }
    }

    /// `alternative` of a decision of the session at `at`, restored when
    /// that session's derivations restore every edit of it.
    fn alternative(&self, at: usize, alternative: &Alternative) -> AlternativeInfo {
        let restored = &self.restored[at];
        AlternativeInfo {
            title: alternative.title.clone(),
            status: alternative.status,
            edits: alternative.edits.clone(),
            restored: !alternative.edits.is_empty()
                && alternative.edits.iter().all(|edit| restored.contains(edit)),
            reason: alternative.reason.as_ref().map(rationale),
        }
    }
}

fn rationale(rationale: &Rationale) -> RationaleInfo {
    RationaleInfo {
        text: rationale.text.clone(),
        speaker: rationale.speaker,
        explicit: rationale.explicit,
        source: rationale.source.clone(),
        correction: None,
    }
}

fn implication(implication: &Implication) -> ImplicationInfo {
    ImplicationInfo {
        text: implication.text.clone(),
        explicit: implication.explicit,
        source: implication.source.clone(),
    }
}

/// Builds a gate item for each open question of the `annotated` sessions
/// that is about nothing or about an edit in `in_change`, sorted as gate
/// items sort.
///
/// An item's subject is the session and the question's id, its detail the
/// question's text, and its fingerprint has two parts: `text`, the SHA-256
/// of the text, and `about`, the SHA-256 of the question's distinct ids,
/// sorted and joined by NUL bytes. A question about nothing hashes the
/// empty string.
#[must_use]
pub fn question_items(annotated: &[Annotated<'_>], in_change: &BTreeSet<String>) -> Vec<GateItem> {
    asked(annotated, in_change)
        .into_iter()
        .map(|(_, item)| item)
        .collect()
}

/// Each of [`question_items`] with its session and question.
fn asked<'a>(
    annotated: &[Annotated<'a>],
    in_change: &BTreeSet<String>,
) -> Vec<((&'a SessionId, &'a OpenQuestion), GateItem)> {
    let mut asked: Vec<_> = annotated
        .iter()
        .flat_map(|entry| {
            entry
                .annotation
                .document
                .open_questions
                .iter()
                .filter(|question| is_item(question, in_change))
                .map(|question| {
                    (
                        (entry.session, question),
                        question_item(entry.session, question),
                    )
                })
        })
        .collect();
    // Every item has the same origin, so subjects give gate order.
    asked.sort_by(|(_, a), (_, b)| a.subject.cmp(&b.subject));
    asked
}

/// Whether `question` is an item in a review whose lines the `in_change`
/// edits contribute to: it is about nothing, or about one of them.
fn is_item(question: &OpenQuestion, in_change: &BTreeSet<String>) -> bool {
    question.about.is_empty()
        || question
            .about
            .iter()
            .any(|id| in_change.contains(id.as_str()))
}

fn question_item(session: &SessionId, question: &OpenQuestion) -> GateItem {
    let mut about: Vec<&str> = question.about.iter().map(EventId::as_str).collect();
    about.sort_unstable();
    about.dedup();
    let fingerprint = Fingerprint(BTreeMap::from([
        (
            "about".to_owned(),
            FingerprintPart::of(about.join("\0").as_bytes()),
        ),
        (
            "text".to_owned(),
            FingerprintPart::of(question.text.as_bytes()),
        ),
    ]));
    GateItem::new(
        Subject::Question {
            session: session.clone(),
            id: question.id.clone(),
        },
        GateDetail::OpenQuestion {
            text: question.text.clone(),
        },
        fingerprint,
    )
}

/// Builds a basis's pins: one [`BasisAnnotation`] per candidate session in
/// `sessions` with a chosen record, sorted by record then session, and one
/// [`BasisDecision`] per decision in view, sorted by name.
///
/// Each of `sessions` gives the session, the record chosen for it (`None`
/// when no involved record holds it, which leaves it unpinned), and its
/// current annotation revision there.
#[must_use]
pub fn basis_entries(
    sessions: &[(&SessionId, Option<&RecordId>, Option<AnnotationRevision>)],
    decisions: &[DecisionInfo],
) -> (Vec<BasisAnnotation>, Vec<BasisDecision>) {
    let mut annotations: Vec<BasisAnnotation> = sessions
        .iter()
        .filter_map(|&(session, record, revision)| {
            record.map(|record| BasisAnnotation {
                record: record.clone(),
                session: session.clone(),
                revision,
            })
        })
        .collect();
    annotations.sort_by(|a, b| (&a.record, &a.session).cmp(&(&b.record, &b.session)));
    let mut pinned: Vec<BasisDecision> = decisions
        .iter()
        .map(|decision| BasisDecision {
            record: decision.record.clone(),
            session: decision.session.clone(),
            revision: decision.revision,
            id: decision.id.clone(),
            name: decision.name.clone(),
        })
        .collect();
    pinned.sort_by(|a, b| a.name.cmp(&b.name));
    (annotations, pinned)
}

/// Lists the sessions whose annotation differs between an `earlier`
/// basis's pins and the `current` one's, sorted by session.
///
/// An annotation is a record and revision; a session pinned without a
/// revision, or not pinned, has none. A session annotated on one side only
/// has `None` on the other, and a session annotated in another record
/// differs even when the revision numbers agree.
#[must_use]
pub fn interpretation_changes(
    earlier: &[BasisAnnotation],
    current: &[BasisAnnotation],
) -> Vec<InterpretationChange> {
    let earlier = annotations(earlier);
    let current = annotations(current);
    let sessions: BTreeSet<&SessionId> = earlier.keys().chain(current.keys()).copied().collect();
    sessions
        .into_iter()
        .filter_map(|session| {
            let (from, to) = (earlier.get(session), current.get(session));
            (from != to).then(|| InterpretationChange {
                session: session.clone(),
                from: from.map(|(_, revision)| *revision),
                to: to.map(|(_, revision)| *revision),
            })
        })
        .collect()
}

/// The record and revision of each session `pins` pins with a revision.
fn annotations(pins: &[BasisAnnotation]) -> BTreeMap<&SessionId, (&RecordId, AnnotationRevision)> {
    pins.iter()
        .filter_map(|pin| {
            pin.revision
                .map(|revision| (&pin.session, (&pin.record, revision)))
        })
        .collect()
}
