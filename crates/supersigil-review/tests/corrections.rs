//! Tests anchoring corrections to the decisions in view: directly by the
//! full reference, through the edits the addressed decision cited, or not
//! at all, and how an anchored correction marks its decision.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use common::{eid, finishes_within, ids, reference, revision, strings};
use supersigil_anchor::ChainClass;
use supersigil_record::interpretations::{
    ANNOTATION_SCHEMA, Annotation, AnnotationDocument, AnnotationRevision, Decision, Observed,
    Prepared, Rationale, Speaker,
};
use supersigil_record::reviews::{
    BasisId, CorrectionKind, DecisionRef, Judgment, JudgmentKind, PathBytes, Who, Withdrawal,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{EventId, JudgmentId, RecordId, Revision, SessionId};
use supersigil_review::decisions::corrections::anchor;
use supersigil_review::decisions::{Annotated, in_view, names};
use supersigil_review::model::{
    BytesStatus, CorrectionInfo, DecisionInfo, FileKindInfo, FileReview, FileStatus, HunkReview,
    RationaleInfo, Span, SpanSide,
};
use supersigil_review::outcome::{AttributedEdit, Outcome, Relation};

/// Two sessions whose 8-character prefixes differ.
const S1: &str = "3f2a9c1e-0000-4000-8000-000000000001";
const S2: &str = "9c0d1e2f-0000-4000-8000-000000000002";
/// Two records, both holding [`S1`] as nested checkouts do.
const R1: &str = "r1";
const R2: &str = "r2";

/// Edits contributing to the change, one span each.
const E1: &str = "edit:1";
const E2: &str = "edit:2";
const E3: &str = "edit:3";
/// An edit of the session that contributes to no line of the change.
const OUTSIDE: &str = "edit:o";
/// An edit no shown decision cites.
const UNCITED: &str = "edit:u";
/// A command of the session.
const COMMAND: &str = "command:c";

/// A text file whose one hunk holds a span attributed to each of `edits`.
fn file(path: &str, edits: &[String]) -> FileReview {
    let spans = edits
        .iter()
        .enumerate()
        .map(|(at, edit)| Span {
            side: SpanSide::Target,
            start: at + 1,
            count: 1,
            outcome: Outcome::Attributed {
                edits: vec![AttributedEdit {
                    edit: eid(edit),
                    relation: Relation::Introduced,
                    classes: BTreeSet::from([ChainClass::ExactFromBase]),
                }],
            },
            provenance: Vec::new(),
        })
        .collect();
    FileReview {
        path: path.to_owned(),
        path_escaped: None,
        path_bytes: PathBytes::new(path),
        status: FileStatus::Modified,
        old_mode: "100644".to_owned(),
        new_mode: "100644".to_owned(),
        old_blob: None,
        new_blob: None,
        kind: FileKindInfo::Text,
        attribution_bytes: BytesStatus {
            base: "identical".to_owned(),
            target: "identical".to_owned(),
        },
        attribution: None,
        conflicting_edits: Vec::new(),
        coarse: false,
        hunks: vec![HunkReview {
            base_start: 0,
            base_count: 0,
            target_start: 0,
            target_count: 0,
            removed: Vec::new(),
            added: Vec::new(),
            spans,
        }],
        mentions: Vec::new(),
        command_changes: Vec::new(),
    }
}

/// The change: [`E1`], [`E2`] and [`E3`] each attributed one line.
fn change() -> Vec<FileReview> {
    vec![file("src/a.rs", &strings(&[E1, E2, E3]))]
}

/// The annotator's one rationale line for decision `id`.
fn inferred(id: &str) -> RationaleInfo {
    RationaleInfo {
        text: format!("inferred for {id}"),
        speaker: Speaker::Annotator,
        explicit: false,
        source: None,
        correction: None,
    }
}

/// One shown revision of a session's annotation.
struct Entry {
    session: SessionId,
    record: RecordId,
    annotation: Annotation,
}

/// The revisions a review shows, owned for [`Annotated`] to borrow.
#[derive(Default)]
struct Fixture {
    entries: Vec<Entry>,
}

impl Fixture {
    /// Shows `revision` of `session`'s annotation from `record`, holding
    /// each decision id with the ids it cites and one inferred rationale.
    fn show(
        mut self,
        record: &str,
        session: &str,
        revision: u32,
        decisions: &[(&str, &[&str])],
    ) -> Self {
        let decisions = decisions
            .iter()
            .map(|(id, cites)| Decision {
                id: (*id).to_owned(),
                title: format!("Decision {id}"),
                cites: ids(cites),
                rationale: vec![Rationale {
                    text: inferred(id).text,
                    speaker: Speaker::Annotator,
                    explicit: false,
                    source: None,
                }],
                alternatives: Vec::new(),
                implications: Vec::new(),
            })
            .collect();
        let document = AnnotationDocument {
            schema: ANNOTATION_SCHEMA.to_owned(),
            session: SessionId::new(session),
            based_on: None,
            prepared: Prepared {
                record: RecordId::new(record),
                observation_revision: Revision::ZERO.next(),
                observed: Observed {
                    count: 10,
                    last: None,
                },
            },
            produced_by: "claude-code/supersigil-annotate".to_owned(),
            corrections_seen: Vec::new(),
            decisions,
            open_questions: Vec::new(),
        };
        self.entries.push(Entry {
            session: SessionId::new(session),
            record: RecordId::new(record),
            annotation: Annotation {
                revision: AnnotationRevision::new(revision),
                at: UtcTime::from_unix(0),
                document,
            },
        });
        self
    }

    /// Lists `judgments` among the corrections the last shown revision's
    /// annotator saw.
    fn seen(mut self, judgments: &[&str]) -> Self {
        let entry = self.entries.last_mut().expect("a revision that saw them");
        entry.annotation.document.corrections_seen =
            judgments.iter().map(|id| JudgmentId::new(*id)).collect();
        self
    }

    fn annotated(&self) -> Vec<Annotated<'_>> {
        self.entries
            .iter()
            .map(|entry| Annotated {
                session: &entry.session,
                record: &entry.record,
                annotation: &entry.annotation,
                restores: &[],
            })
            .collect()
    }

    /// Anchors `corrections` to the decisions in view of [`change`], with
    /// the cites `addressed` read for each reference. Returns the decisions
    /// and the corrections anchored to none.
    fn anchor(
        &self,
        corrections: &[&Judgment],
        addressed: &[(DecisionRef, Vec<EventId>)],
    ) -> (Vec<DecisionInfo>, Vec<CorrectionInfo>) {
        self.anchor_in(&change(), corrections, addressed)
    }

    /// [`Self::anchor`] over the change `files`.
    fn anchor_in(
        &self,
        files: &[FileReview],
        corrections: &[&Judgment],
        addressed: &[(DecisionRef, Vec<EventId>)],
    ) -> (Vec<DecisionInfo>, Vec<CorrectionInfo>) {
        let annotated = self.annotated();
        let mut decisions = in_view(&annotated, &names(&annotated), files).decisions;
        let cites = |reference: &DecisionRef| {
            addressed
                .iter()
                .find(|(addressed, _)| addressed == reference)
                .map(|(_, cites)| cites.as_slice())
        };
        let unanchored = anchor(corrections, cites, &annotated, &mut decisions);
        (decisions, unanchored)
    }
}

/// The review corrections are recorded on.
fn review() -> BasisId {
    serde_json::from_value(serde_json::Value::String("1".repeat(64))).unwrap()
}

fn who() -> Who {
    Who {
        name: "Joni Savolainen".to_owned(),
        email: "joni@example.com".to_owned(),
    }
}

/// Correction `id` of `decision`, recorded at `at` seconds: a reason for
/// `wrong` and `not_together`, a text for `constraint`.
fn correction(id: &str, decision: DecisionRef, kind: CorrectionKind, at: i64) -> Judgment {
    let (reason, text) = match kind {
        CorrectionKind::Wrong | CorrectionKind::NotTogether => {
            (Some(format!("reason of {id}")), None)
        }
        CorrectionKind::Constraint => (None, Some(format!("constraint of {id}"))),
    };
    Judgment {
        id: JudgmentId::new(id),
        basis: review(),
        kind: JudgmentKind::Correct,
        items: Vec::new(),
        reason,
        text,
        decision: Some(decision),
        correction: Some(kind),
        closes: BTreeMap::new(),
        withdraws: Vec::new(),
        who: who(),
        at: UtcTime::from_unix(at),
    }
}

fn all(corrections: &[Judgment]) -> Vec<&Judgment> {
    corrections.iter().collect()
}

fn named<'d>(decisions: &'d [DecisionInfo], name: &str) -> &'d DecisionInfo {
    decisions
        .iter()
        .find(|decision| decision.name == name)
        .unwrap_or_else(|| panic!("decision {name} in view: {decisions:?}"))
}

/// Each correction's judgment id.
fn judgments(corrections: &[CorrectionInfo]) -> Vec<&str> {
    corrections
        .iter()
        .map(|correction| correction.judgment.as_str())
        .collect()
}

/// The corrections on decision `name`, each with the revision it addressed.
fn addressed_on<'d>(
    decisions: &'d [DecisionInfo],
    name: &str,
) -> Vec<(&'d str, Option<AnnotationRevision>)> {
    named(decisions, name)
        .corrections
        .iter()
        .map(|correction| (correction.judgment.as_str(), correction.addressed))
        .collect()
}

/// Each correction's judgment id with whether it was seen.
fn seen(corrections: &[CorrectionInfo]) -> Vec<(&str, bool)> {
    corrections
        .iter()
        .map(|correction| (correction.judgment.as_str(), correction.seen))
        .collect()
}

#[test]
fn a_correction_anchors_directly_to_the_decision_its_full_reference_names() {
    let fixture = Fixture::default().show(R1, S1, 1, &[("d1", &[E1]), ("d2", &[E2])]);
    let wrong = correction("j1", reference(R1, S1, 1, "d1"), CorrectionKind::Wrong, 10);

    let (decisions, unanchored) = fixture.anchor(&[&wrong], &[]);

    assert_eq!(
        named(&decisions, "d1").corrections,
        [CorrectionInfo {
            judgment: JudgmentId::new("j1"),
            decision: reference(R1, S1, 1, "d1"),
            kind: CorrectionKind::Wrong,
            reason: Some("reason of j1".to_owned()),
            text: None,
            who: who(),
            at: UtcTime::from_unix(10),
            addressed: None,
            seen: false,
            review: review(),
        }]
    );
    assert_eq!(named(&decisions, "d2").corrections, []);
    assert_eq!(unanchored, []);
}

#[test]
fn a_same_named_decision_in_another_records_revision_is_never_a_direct_match() {
    // Both records hold the session; the review shows record 1's revision
    // 1, and the correction names record 2's.
    let fixture = Fixture::default().show(R1, S1, 1, &[("d1", &[E1])]);
    let other = reference(R2, S1, 1, "d1");
    let wrong = correction("j1", other.clone(), CorrectionKind::Wrong, 10);

    // Its cites unknown, it is unanchored.
    let (decisions, unanchored) = fixture.anchor(&[&wrong], &[]);

    assert_eq!(named(&decisions, "d1").corrections, []);
    assert_eq!(judgments(&unanchored), ["j1"]);
    assert_eq!(unanchored[0].decision, other);
    assert_eq!(unanchored[0].addressed, None);

    // Read from record 2, it anchors through the edit both cite and says
    // which revision it addressed.
    let (decisions, unanchored) = fixture.anchor(&[&wrong], &[(other, ids(&[E1]))]);

    assert_eq!(addressed_on(&decisions, "d1"), [("j1", Some(revision(1)))]);
    assert_eq!(unanchored, []);
}

#[test]
fn a_correction_anchors_to_every_decision_citing_an_edit_its_decision_cited() {
    // Revision 1's d1 cited E1 and E2; revision 2 split them into d2 and
    // d3. d4 cites neither. Revision 1's d5 cited only an edit no decision
    // of revision 2 cites.
    let fixture =
        Fixture::default().show(R1, S1, 2, &[("d2", &[E1]), ("d3", &[E2]), ("d4", &[E3])]);
    let split = reference(R1, S1, 1, "d1");
    let dropped = reference(R1, S1, 1, "d5");
    let corrections = [
        correction("j1", split.clone(), CorrectionKind::NotTogether, 10),
        correction("j2", dropped.clone(), CorrectionKind::Wrong, 20),
    ];

    let (decisions, unanchored) = fixture.anchor(
        &all(&corrections),
        &[(split, ids(&[E1, E2])), (dropped, ids(&[UNCITED]))],
    );

    assert_eq!(addressed_on(&decisions, "d2"), [("j1", Some(revision(1)))]);
    assert_eq!(addressed_on(&decisions, "d3"), [("j1", Some(revision(1)))]);
    assert_eq!(addressed_on(&decisions, "d4"), []);
    assert_eq!(judgments(&unanchored), ["j2"]);
}

#[test]
fn a_cited_edit_outside_the_change_anchors_but_a_shared_command_does_not() {
    let fixture =
        Fixture::default().show(R1, S1, 2, &[("d2", &[E1, OUTSIDE]), ("d3", &[E2, COMMAND])]);
    let through_edit = reference(R1, S1, 1, "d1");
    let through_command = reference(R1, S1, 1, "d4");
    let corrections = [
        correction("j1", through_edit.clone(), CorrectionKind::Wrong, 10),
        correction("j2", through_command.clone(), CorrectionKind::Wrong, 20),
    ];

    let (decisions, unanchored) = fixture.anchor(
        &all(&corrections),
        &[
            (through_edit, ids(&[OUTSIDE])),
            (through_command, ids(&[COMMAND])),
        ],
    );

    assert_eq!(addressed_on(&decisions, "d2"), [("j1", Some(revision(1)))]);
    assert_eq!(addressed_on(&decisions, "d3"), []);
    assert_eq!(judgments(&unanchored), ["j2"]);
}

#[test]
fn a_correction_of_a_shown_revisions_decision_out_of_view_is_unanchored() {
    // The review shows exactly the corrected revision, whose d1 cites
    // nothing in the change. Its sibling d2 shares d1's edit, but d2 is
    // not what was corrected.
    let fixture = Fixture::default().show(R1, S1, 1, &[("d1", &[OUTSIDE]), ("d2", &[E1, OUTSIDE])]);
    let corrected = reference(R1, S1, 1, "d1");
    let wrong = correction("j1", corrected.clone(), CorrectionKind::Wrong, 10);

    let (decisions, unanchored) = fixture.anchor(&[&wrong], &[(corrected, ids(&[OUTSIDE]))]);

    assert_eq!(named(&decisions, "d2").corrections, []);
    assert_eq!(judgments(&unanchored), ["j1"]);
}

#[test]
fn unanchored_corrections_are_listed_oldest_first() {
    // Unreadable references, and a session the review does not show, even
    // though another session's decision cites the edit its decision cited.
    let fixture = Fixture::default().show(R1, S1, 2, &[("d2", &[E1])]);
    let unread = reference(R1, S1, 1, "d1");
    let unshown = reference(R1, S2, 1, "d1");
    let corrections = [
        correction("j3", unread.clone(), CorrectionKind::Wrong, 30),
        correction("j2b", unshown.clone(), CorrectionKind::Constraint, 20),
        correction("j1", unread.clone(), CorrectionKind::NotTogether, 10),
        correction("j2a", unread, CorrectionKind::Wrong, 20),
    ];

    let (decisions, unanchored) = fixture.anchor(&all(&corrections), &[(unshown, ids(&[E1]))]);

    assert_eq!(named(&decisions, "d2").corrections, []);
    assert_eq!(judgments(&unanchored), ["j1", "j2a", "j2b", "j3"]);
}

#[test]
fn a_correction_is_seen_when_the_shown_revision_of_its_session_lists_it() {
    // The shown revision lists j1, j3 and j4; j4 corrects a session the
    // review does not show, so no shown revision of its session lists it.
    let fixture = Fixture::default()
        .show(R1, S1, 2, &[("d2", &[E1])])
        .seen(&["j1", "j3", "j4"]);
    let shown = reference(R1, S1, 2, "d2");
    let corrections = [
        correction("j1", shown.clone(), CorrectionKind::Wrong, 10),
        correction("j2", shown, CorrectionKind::Wrong, 20),
        correction("j3", reference(R1, S1, 1, "d9"), CorrectionKind::Wrong, 30),
        correction("j4", reference(R1, S2, 1, "d1"), CorrectionKind::Wrong, 40),
    ];

    let (decisions, unanchored) = fixture.anchor(&all(&corrections), &[]);

    assert_eq!(
        seen(&named(&decisions, "d2").corrections),
        [("j1", true), ("j2", false)]
    );
    assert_eq!(seen(&unanchored), [("j3", true), ("j4", false)]);
}

#[test]
fn wrong_and_not_together_dispute_a_decision_and_a_constraint_does_not() {
    let fixture =
        Fixture::default().show(R1, S1, 1, &[("d1", &[E1]), ("d2", &[E2]), ("d3", &[E3])]);
    let corrections = [
        correction("j1", reference(R1, S1, 1, "d1"), CorrectionKind::Wrong, 10),
        correction(
            "j2",
            reference(R1, S1, 1, "d2"),
            CorrectionKind::NotTogether,
            20,
        ),
        correction(
            "j3",
            reference(R1, S1, 1, "d3"),
            CorrectionKind::Constraint,
            30,
        ),
    ];

    let (decisions, _) = fixture.anchor(&all(&corrections), &[]);

    let disputed: Vec<(&str, bool)> = decisions
        .iter()
        .map(|decision| (decision.name.as_str(), decision.disputed))
        .collect();
    assert_eq!(disputed, [("d1", true), ("d2", true), ("d3", false)]);
}

#[test]
fn constraints_lead_the_rationale_and_corrections_are_oldest_first() {
    let fixture = Fixture::default().show(R1, S1, 2, &[("d2", &[E1])]);
    let direct = reference(R1, S1, 2, "d2");
    let earlier = reference(R1, S1, 1, "d1");
    // Out of order, with a tie on time broken by judgment id.
    let corrections = [
        correction("j4", direct.clone(), CorrectionKind::Constraint, 40),
        correction("j2", direct.clone(), CorrectionKind::Wrong, 20),
        correction("j3", earlier.clone(), CorrectionKind::Constraint, 20),
        correction("j1", direct, CorrectionKind::Constraint, 10),
    ];

    let (decisions, _) = fixture.anchor(&all(&corrections), &[(earlier, ids(&[E1]))]);

    let decision = named(&decisions, "d2");
    assert_eq!(judgments(&decision.corrections), ["j1", "j2", "j3", "j4"]);
    let constraint = |id: &str| RationaleInfo {
        text: format!("constraint of {id}"),
        speaker: Speaker::Human,
        explicit: true,
        source: None,
        correction: Some(JudgmentId::new(id)),
    };
    assert_eq!(
        decision.rationale,
        [
            constraint("j1"),
            constraint("j3"),
            constraint("j4"),
            inferred("d2")
        ]
    );
}

#[test]
fn anchoring_trusts_its_caller_to_pass_only_standing_corrections() {
    // An undo withdrew j1, but whether a correction stands is the caller's
    // to decide: anchoring reads no undo and anchors what it is given.
    let fixture = Fixture::default().show(R1, S1, 1, &[("d1", &[E1])]);
    let withdrawn = correction("j1", reference(R1, S1, 1, "d1"), CorrectionKind::Wrong, 10);
    let undo = Judgment {
        id: JudgmentId::new("j2"),
        kind: JudgmentKind::Undo,
        reason: None,
        decision: None,
        correction: None,
        withdraws: vec![Withdrawal {
            judgment: withdrawn.id.clone(),
            item: None,
        }],
        at: UtcTime::from_unix(20),
        ..withdrawn.clone()
    };

    let (decisions, unanchored) = fixture.anchor(&[&withdrawn, &undo], &[]);

    assert_eq!(judgments(&named(&decisions, "d1").corrections), ["j1"]);
    assert!(named(&decisions, "d1").disputed);
    assert_eq!(unanchored, []);
}

#[test]
fn ten_thousand_corrections_anchor_to_ten_thousand_decisions_promptly() {
    // Each decision cites its own edit. Half the corrections name a shown
    // decision; the other half name an earlier revision's decision that
    // cited two neighbouring edits, so each anchors to two decisions but
    // the last.
    let edit = |index: usize| format!("edit:{index:016x}");
    let edits: Vec<String> = (0..10_000).map(edit).collect();
    let files = vec![file("src/a.rs", &edits)];
    let names: Vec<String> = (0..10_000).map(|index| format!("d{index}")).collect();
    let cited: Vec<[&str; 1]> = edits.iter().map(|edit| [edit.as_str()]).collect();
    let shown: Vec<(&str, &[&str])> = names
        .iter()
        .zip(&cited)
        .map(|(name, cites)| (name.as_str(), cites.as_slice()))
        .collect();
    let corrections: Vec<Judgment> = (0..10_000_i64)
        .map(|index| {
            let revision = if index < 5_000 { 2 } else { 1 };
            correction(
                &format!("j{index:05}"),
                reference(R1, S1, revision, &format!("d{index}")),
                CorrectionKind::Wrong,
                index,
            )
        })
        .collect();
    let addressed: Vec<(DecisionRef, Vec<EventId>)> = (5_000..10_000)
        .map(|index| {
            (
                reference(R1, S1, 1, &format!("d{index}")),
                vec![eid(&edit(index)), eid(&edit(index + 1))],
            )
        })
        .collect();
    let fixture = Fixture::default().show(R1, S1, 2, &shown).seen(
        &corrections
            .iter()
            .map(|correction| correction.id.as_str())
            .collect::<Vec<_>>(),
    );

    let (decisions, unanchored) = finishes_within(Duration::from_secs(20), move || {
        fixture.anchor_in(&files, &all(&corrections), &addressed)
    });

    let anchored: usize = decisions
        .iter()
        .map(|decision| decision.corrections.len())
        .sum();
    assert_eq!(anchored, 5_000 + 2 * 5_000 - 1);
    assert_eq!(unanchored, []);
    assert!(
        decisions
            .iter()
            .flat_map(|decision| &decision.corrections)
            .all(|correction| correction.seen)
    );
}

#[test]
fn corrections_of_a_decision_repeating_an_edit_anchor_promptly() {
    // Revision 1's d1 cited one edit 1,000 times, the most a decision may
    // list, and 100 corrections name it. The shown revision 2 has 1,000
    // decisions citing that edit, so each correction anchors to all of
    // them. Neither the repeated cite nor the repeated reference adds work.
    let files = vec![file("src/a.rs", &strings(&[E1]))];
    let names: Vec<String> = (0..1_000).map(|index| format!("d{index}")).collect();
    let shown: Vec<(&str, &[&str])> = names
        .iter()
        .map(|name| (name.as_str(), &[E1][..]))
        .collect();
    let corrected = reference(R1, S1, 1, "d1");
    let corrections: Vec<Judgment> = (0..100_i64)
        .map(|index| {
            correction(
                &format!("j{index:03}"),
                corrected.clone(),
                CorrectionKind::Wrong,
                index,
            )
        })
        .collect();
    let addressed = vec![(corrected, vec![eid(E1); 1_000])];
    let fixture = Fixture::default().show(R1, S1, 2, &shown);

    let (decisions, unanchored) = finishes_within(Duration::from_secs(10), move || {
        fixture.anchor_in(&files, &all(&corrections), &addressed)
    });

    let oldest_first: Vec<String> = (0..100).map(|index| format!("j{index:03}")).collect();
    assert_eq!(decisions.len(), 1_000);
    assert!(decisions.iter().all(|decision| {
        decision.disputed
            && judgments(&decision.corrections) == oldest_first
            && decision
                .corrections
                .iter()
                .all(|correction| correction.addressed == Some(revision(1)))
    }));
    assert_eq!(unanchored, []);
}
