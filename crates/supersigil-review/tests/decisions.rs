//! Tests the decisions a review shows: which record supplies a session's
//! annotation, how decisions are named, how each decision explains the spans
//! of the change, open questions as gate items, the basis entries, and
//! interpretation changes between reviews.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::path::PathBuf;
use std::time::Duration;

use common::{eid, finishes_within, ids, revision, strings};
use supersigil_anchor::ChainClass;
use supersigil_record::derivations::Restore;
use supersigil_record::interpretations::{
    ANNOTATION_SCHEMA, Alternative, AlternativeStatus, Annotation, AnnotationDocument,
    AnnotationRevision, Decision, Implication, Observed, OpenQuestion, Prepared, Rationale, Source,
    Speaker,
};
use supersigil_record::reviews::{
    Algorithms, BASIS_SCHEMA, Basis, BasisAnnotation, BasisBase, BasisDecision, BasisId,
    BasisScope, BasisTarget, DecisionRef, FingerprintPart, GateDetail, GateItem, ItemId, Judgment,
    JudgmentKind, Origin, PathBytes, Subject, TargetKind, Who,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, RecordId, Revision, SessionId, TurnId};
use supersigil_review::decisions::{
    Annotated, InView, RecordCandidate, choose_record, decision_pins, in_excerpt, in_view,
    interpretation_changes, names, question_items,
};
use supersigil_review::judging::JudgedStore;
use supersigil_review::model::{
    AlternativeInfo, Association, BytesStatus, CitesInfo, DecisionInfo, ExplainedSpan,
    FileKindInfo, FileReview, FileStatus, HunkReview, ImplicationInfo, InterpretationChange,
    ItemStateInfo, RationaleInfo, RelatedReading, Span, SpanSide,
};
use supersigil_review::names::ItemNames;
use supersigil_review::outcome::{AttributedEdit, Outcome, Relation, UnattributedReason};

/// Two sessions whose 8-character prefixes differ, and a third sharing the
/// first's.
const S1: &str = "3f2a9c1e-0000-4000-8000-000000000001";
const S2: &str = "9c0d1e2f-0000-4000-8000-000000000002";
const S3: &str = "3f2a9c1e-7777-4000-8000-000000000003";
const RECORD: &str = "r1";

/// An edit attributed on both sides of `src/a.rs`.
const ATTRIBUTED: &str = "edit:a";
/// An edit attributed beside [`ATTRIBUTED`] on one span.
const BESIDE: &str = "edit:b";
/// An edit whose whitespace-only change is `src/a.rs` 50-51.
const WHITESPACE: &str = "edit:w";
/// An edit matched by content alone in `src/b.rs`, its search incomplete.
const MATCHED: &str = "edit:m";
/// An edit of the session that contributes to no line of the change.
const OUTSIDE: &str = "edit:o";
/// A command of the session.
const COMMAND: &str = "command:c";

fn span(side: SpanSide, start: usize, count: usize, outcome: Outcome) -> Span {
    Span {
        side,
        start,
        count,
        outcome,
        provenance: Vec::new(),
    }
}

fn attributed(relation: Relation, edits: &[&str]) -> Outcome {
    Outcome::Attributed {
        edits: edits
            .iter()
            .map(|edit| AttributedEdit {
                edit: eid(edit),
                relation,
                classes: BTreeSet::from([ChainClass::ExactFromBase]),
            })
            .collect(),
    }
}

/// A text file whose one hunk holds `spans`.
fn file(path: &str, spans: Vec<Span>) -> FileReview {
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

/// The change: `src/a.rs` with attributed and whitespace-only spans, and
/// `src/b.rs` with a content-matched span and an unattributed one.
fn files() -> Vec<FileReview> {
    vec![
        file(
            "src/a.rs",
            vec![
                span(
                    SpanSide::Base,
                    12,
                    1,
                    attributed(Relation::Replaced, &[ATTRIBUTED]),
                ),
                span(
                    SpanSide::Target,
                    12,
                    29,
                    attributed(Relation::Introduced, &[ATTRIBUTED, BESIDE]),
                ),
                span(
                    SpanSide::Target,
                    50,
                    2,
                    Outcome::WhitespaceOnly {
                        edits: vec![eid(WHITESPACE)],
                    },
                ),
            ],
        ),
        file(
            "src/b.rs",
            vec![
                span(
                    SpanSide::Target,
                    3,
                    2,
                    Outcome::ContentMatch {
                        relation: Relation::Introduced,
                        edits: vec![eid(MATCHED)],
                        search_complete: false,
                    },
                ),
                span(
                    SpanSide::Target,
                    10,
                    1,
                    Outcome::Unattributed {
                        reason: UnattributedReason::GapBeforeChain,
                    },
                ),
            ],
        ),
    ]
}

fn explained(
    path: &str,
    side: SpanSide,
    start: usize,
    count: usize,
    association: Association,
    search_complete: Option<bool>,
) -> ExplainedSpan {
    ExplainedSpan {
        path: PathBytes::new(path),
        side,
        start,
        count,
        association,
        search_complete,
    }
}

/// `src/a.rs`'s two spans attributed to [`ATTRIBUTED`].
fn attributed_spans() -> Vec<ExplainedSpan> {
    vec![
        explained(
            "src/a.rs",
            SpanSide::Base,
            12,
            1,
            Association::Attributed,
            None,
        ),
        explained(
            "src/a.rs",
            SpanSide::Target,
            12,
            29,
            Association::Attributed,
            None,
        ),
    ]
}

const HUMAN_QUOTE: &str = "make the sync plan inspectable";
const INFERENCE: &str = "the author preferred one plan over per-store syncs";
const IMPLICATION: &str = "every new file kind needs a step in the plan";

fn source() -> Source {
    Source {
        turn: TurnId::new("u1"),
        quote: HUMAN_QUOTE.to_owned(),
    }
}

/// A decision citing `cites`, with a stated and an inferred rationale and an
/// inferred implication.
fn decision(id: &str, cites: &[&str]) -> Decision {
    Decision {
        id: id.to_owned(),
        title: format!("Decision {id}"),
        cites: cites.iter().map(|cite| eid(cite)).collect(),
        rationale: vec![
            Rationale {
                text: HUMAN_QUOTE.to_owned(),
                speaker: Speaker::Human,
                explicit: true,
                source: Some(source()),
            },
            Rationale {
                text: INFERENCE.to_owned(),
                speaker: Speaker::Annotator,
                explicit: false,
                source: None,
            },
        ],
        alternatives: Vec::new(),
        implications: vec![Implication {
            text: IMPLICATION.to_owned(),
            explicit: false,
            source: None,
        }],
    }
}

fn question(id: &str, about: &[&str]) -> OpenQuestion {
    OpenQuestion {
        id: id.to_owned(),
        text: format!("Why {id}?"),
        about: about.iter().map(|id| eid(id)).collect(),
    }
}

/// One session's annotation in [`RECORD`] and its derivations' restores.
struct Entry {
    session: SessionId,
    record: RecordId,
    annotation: Annotation,
    restores: Vec<Restore>,
}

/// Annotated sessions, owned for [`Annotated`] to borrow.
#[derive(Default)]
struct Fixture {
    entries: Vec<Entry>,
}

impl Fixture {
    /// Adds revision 1 of `session`'s annotation with `decisions` and
    /// `questions`.
    fn annotate(
        mut self,
        session: &str,
        decisions: Vec<Decision>,
        questions: Vec<OpenQuestion>,
    ) -> Self {
        let document = AnnotationDocument {
            schema: ANNOTATION_SCHEMA.to_owned(),
            session: SessionId::new(session),
            based_on: None,
            prepared: Prepared {
                record: RecordId::new(RECORD),
                observation_revision: Revision::ZERO.next(),
                observed: Observed {
                    count: 10,
                    last: None,
                },
            },
            produced_by: "claude-code/supersigil-annotate".to_owned(),
            corrections_seen: Vec::new(),
            decisions,
            open_questions: questions,
        };
        self.entries.push(Entry {
            session: SessionId::new(session),
            record: RecordId::new(RECORD),
            annotation: Annotation {
                revision: AnnotationRevision::FIRST,
                at: UtcTime::from_unix(0),
                document,
            },
            restores: Vec::new(),
        });
        self
    }

    /// Records, in the last session's derivations, an edit restoring
    /// `restored`.
    fn restoring(mut self, restored: &[&str]) -> Self {
        let entry = self.entries.last_mut().expect("a session to restore in");
        entry.restores.push(Restore {
            edit: eid(&format!("edit:restore{}", entry.restores.len())),
            restores: restored.iter().map(|edit| eid(edit)).collect(),
        });
        self
    }

    fn annotated(&self) -> Vec<Annotated<'_>> {
        self.entries
            .iter()
            .map(|entry| Annotated {
                session: &entry.session,
                record: &entry.record,
                annotation: &entry.annotation,
                restores: &entry.restores,
            })
            .collect()
    }

    /// The decisions in view of [`files`].
    fn view(&self) -> InView {
        let annotated = self.annotated();
        in_view(&annotated, &names(&annotated), &files())
    }

    /// The question items of [`files`], named as a gate names them.
    fn question_names(&self) -> Vec<String> {
        let annotated = self.annotated();
        let in_change = in_view(&annotated, &names(&annotated), &files()).in_change;
        let items = question_items(&annotated, &in_change);
        ItemNames::new(&items, BTreeMap::new(), false, |_| None)
            .names()
            .to_vec()
    }
}

fn decision_names(view: &InView) -> Vec<&str> {
    view.decisions
        .iter()
        .map(|decision| decision.name.as_str())
        .collect()
}

fn the_decision(view: &InView) -> &DecisionInfo {
    let [decision] = view.decisions.as_slice() else {
        panic!("one decision in view: {:?}", view.decisions);
    };
    decision
}

#[test]
fn an_annotated_record_beats_one_with_longer_logs() {
    let (r1, r2) = (RecordId::new("r1"), RecordId::new("r2"));
    let candidates = [
        RecordCandidate {
            record: &r1,
            annotated: false,
            log_bytes: 9_000,
        },
        RecordCandidate {
            record: &r2,
            annotated: true,
            log_bytes: 10,
        },
    ];

    assert_eq!(choose_record(&candidates), Some(&r2));
}

#[test]
fn longer_logs_beat_a_smaller_record_id() {
    let (r1, r2) = (RecordId::new("r1"), RecordId::new("r2"));
    for annotated in [false, true] {
        let candidates = [
            RecordCandidate {
                record: &r1,
                annotated,
                log_bytes: 10,
            },
            RecordCandidate {
                record: &r2,
                annotated,
                log_bytes: 9_000,
            },
        ];

        assert_eq!(choose_record(&candidates), Some(&r2));
    }
}

#[test]
fn equal_candidates_choose_the_smallest_record_id_in_any_order() {
    let records = [
        RecordId::new("r3"),
        RecordId::new("r1"),
        RecordId::new("r2"),
    ];
    let candidate = |record| RecordCandidate {
        record,
        annotated: true,
        log_bytes: 10,
    };
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for order in orders {
        let candidates = order.map(|index| candidate(&records[index]));

        assert_eq!(choose_record(&candidates), Some(&records[1]), "{order:?}");
    }
}

#[test]
fn no_candidate_chooses_no_record() {
    assert_eq!(choose_record(&[]), None);
}

#[test]
fn a_decision_id_unique_among_annotated_sessions_is_its_name() {
    let fixture = Fixture::default()
        .annotate(S1, vec![decision("d1", &[ATTRIBUTED])], Vec::new())
        .annotate(S2, vec![decision("d2", &[MATCHED])], Vec::new());
    let annotated = fixture.annotated();
    let names = names(&annotated);

    assert_eq!(names.decision(&SessionId::new(S1), "d1"), "d1");
    assert_eq!(decision_names(&fixture.view()), ["d1", "d2"]);
}

#[test]
fn a_decision_id_in_two_sessions_is_qualified_by_each_session() {
    let fixture = Fixture::default()
        .annotate(
            S1,
            vec![decision("d1", &[ATTRIBUTED]), decision("d2", &[MATCHED])],
            Vec::new(),
        )
        .annotate(S2, vec![decision("d1", &[ATTRIBUTED])], Vec::new());

    let view = fixture.view();

    assert_eq!(decision_names(&view), ["3f2a9c1e:d1", "9c0d1e2f:d1", "d2"]);
    assert_eq!(
        view.edit_decisions[ATTRIBUTED],
        strings(&["3f2a9c1e:d1", "9c0d1e2f:d1"])
    );
}

#[test]
fn names_depend_on_every_annotated_session_not_only_those_in_view() {
    // Session 2's `d1` cites nothing in the change, yet session 1's `d1`
    // is qualified: names follow the set of annotated sessions.
    let fixture = Fixture::default()
        .annotate(S1, vec![decision("d1", &[ATTRIBUTED])], Vec::new())
        .annotate(S2, vec![decision("d1", &[OUTSIDE])], Vec::new());

    assert_eq!(decision_names(&fixture.view()), ["3f2a9c1e:d1"]);
}

#[test]
fn a_session_is_named_by_its_shortest_unique_prefix_of_at_least_eight_characters() {
    let fixture = Fixture::default()
        .annotate(S1, vec![decision("d1", &[ATTRIBUTED])], Vec::new())
        .annotate(S2, Vec::new(), Vec::new())
        .annotate(S3, vec![decision("d1", &[MATCHED])], Vec::new());
    let annotated = fixture.annotated();
    let names = names(&annotated);

    assert_eq!(names.short(&SessionId::new(S2)), "9c0d1e2f");
    assert_eq!(names.short(&SessionId::new(S1)), "3f2a9c1e-0");
    assert_eq!(names.short(&SessionId::new(S3)), "3f2a9c1e-7");
    assert_eq!(
        decision_names(&fixture.view()),
        ["3f2a9c1e-0:d1", "3f2a9c1e-7:d1"]
    );
}

#[test]
fn a_question_id_in_two_sessions_names_its_items_by_session() {
    let fixture = Fixture::default()
        .annotate(
            S1,
            vec![decision("d1", &[ATTRIBUTED])],
            vec![question("q1", &[ATTRIBUTED])],
        )
        .annotate(
            S2,
            Vec::new(),
            vec![question("q1", &[]), question("q2", &[])],
        );

    assert_eq!(
        fixture.question_names(),
        [
            "question:3f2a9c1e:q1",
            "question:9c0d1e2f:q1",
            "question:q2"
        ]
    );
    assert_eq!(
        the_decision(&fixture.view()).open_questions,
        strings(&["question:3f2a9c1e:q1"])
    );
}

#[test]
fn an_excerpt_names_its_questions_over_every_question_of_the_sessions() {
    let fixture = Fixture::default()
        .annotate(
            S1,
            vec![decision("d1", &[ATTRIBUTED, MATCHED])],
            vec![question("q1", &[ATTRIBUTED]), question("q3", &[MATCHED])],
        )
        .annotate(S2, Vec::new(), vec![question("q1", &[MATCHED])]);
    let annotated = fixture.annotated();
    // The excerpt is `src/a.rs`: S2's `q1` and S1's `q3` concern `src/b.rs`.
    let files = files();
    let excerpt = &files[..1];
    let spans: Vec<Span> = excerpt[0]
        .hunks
        .iter()
        .flat_map(|hunk| hunk.spans.clone())
        .collect();

    let whole = in_view(&annotated, &names(&annotated), &files);
    let part = in_view(&annotated, &names(&annotated), excerpt);
    let excerpted = in_excerpt(
        &annotated,
        &names(&annotated),
        &excerpt[0].path_bytes,
        &spans,
    );

    // The gate of the whole change qualifies the repeated `q1`.
    assert_eq!(
        fixture.question_names(),
        [
            "question:3f2a9c1e:q1",
            "question:q3",
            "question:9c0d1e2f:q1"
        ]
    );
    assert_eq!(
        the_decision(&whole).open_questions,
        strings(&["question:3f2a9c1e:q1", "question:q3"])
    );
    // Named over the items in the excerpt alone, `q1` looks unique.
    assert_eq!(
        the_decision(&part).open_questions,
        strings(&["question:q1"])
    );
    // Named over every question, it is the gate's name; `q3`, no item in the
    // excerpt, is still not listed.
    assert_eq!(
        the_decision(&excerpted).open_questions,
        strings(&["question:3f2a9c1e:q1"])
    );
    // Nothing else of the view changes.
    assert_eq!(excerpted.in_change, part.in_change);
    assert_eq!(excerpted.edit_decisions, part.edit_decisions);
    let (mut shown, mut named) = (
        the_decision(&part).clone(),
        the_decision(&excerpted).clone(),
    );
    shown.open_questions.clear();
    named.open_questions.clear();
    assert_eq!(shown, named);
}

#[test]
fn a_decision_citing_an_attributed_edit_explains_its_spans_as_attributed() {
    let fixture = Fixture::default().annotate(S1, vec![decision("d1", &[ATTRIBUTED])], Vec::new());

    let view = fixture.view();

    let rationale = vec![
        RationaleInfo {
            text: HUMAN_QUOTE.to_owned(),
            speaker: Speaker::Human,
            explicit: true,
            source: Some(source()),
            correction: None,
        },
        RationaleInfo {
            text: INFERENCE.to_owned(),
            speaker: Speaker::Annotator,
            explicit: false,
            source: None,
            correction: None,
        },
    ];
    assert_eq!(
        view.decisions,
        [DecisionInfo {
            name: "d1".to_owned(),
            session: SessionId::new(S1),
            record: RecordId::new(RECORD),
            revision: AnnotationRevision::FIRST,
            id: "d1".to_owned(),
            title: "Decision d1".to_owned(),
            disputed: false,
            corrections: Vec::new(),
            rationale,
            alternatives: Vec::new(),
            implications: vec![ImplicationInfo {
                text: IMPLICATION.to_owned(),
                explicit: false,
                source: None,
            }],
            open_questions: Vec::new(),
            cites: CitesInfo {
                in_change: ids(&[ATTRIBUTED]),
                elsewhere: Vec::new(),
            },
            explains: attributed_spans(),
        }]
    );
}

#[test]
fn a_span_with_two_cited_edits_is_explained_once() {
    let fixture = Fixture::default().annotate(
        S1,
        vec![decision("d1", &[BESIDE, ATTRIBUTED, BESIDE])],
        Vec::new(),
    );

    assert_eq!(the_decision(&fixture.view()).explains, attributed_spans());
}

#[test]
fn a_content_match_explains_its_span_as_such_with_its_search_flag() {
    // Two decisions may match one span: nothing selects among covering edits.
    let fixture = Fixture::default().annotate(
        S1,
        vec![decision("d1", &[MATCHED]), decision("d2", &[MATCHED])],
        Vec::new(),
    );

    let view = fixture.view();

    let matched = [explained(
        "src/b.rs",
        SpanSide::Target,
        3,
        2,
        Association::ContentMatch,
        Some(false),
    )];
    assert_eq!(view.decisions.len(), 2);
    for decision in &view.decisions {
        assert_eq!(decision.explains, matched, "{}", decision.name);
    }
}

#[test]
fn a_whitespace_only_span_is_explained_as_whitespace_only() {
    let fixture = Fixture::default().annotate(S1, vec![decision("d1", &[WHITESPACE])], Vec::new());

    assert_eq!(
        the_decision(&fixture.view()).explains,
        [explained(
            "src/a.rs",
            SpanSide::Target,
            50,
            2,
            Association::WhitespaceOnly,
            None
        )]
    );
}

#[test]
fn a_decision_citing_nothing_in_the_change_is_not_in_view() {
    let fixture = Fixture::default().annotate(
        S1,
        vec![decision("d1", &[OUTSIDE, COMMAND]), decision("d2", &[])],
        Vec::new(),
    );

    let view = fixture.view();

    assert_eq!(view.decisions, []);
    assert_eq!(view.edit_decisions, BTreeMap::new());
}

#[test]
fn cites_split_into_those_in_the_change_and_the_rest() {
    let fixture = Fixture::default().annotate(
        S1,
        vec![decision(
            "d1",
            &[MATCHED, OUTSIDE, ATTRIBUTED, COMMAND, ATTRIBUTED],
        )],
        Vec::new(),
    );

    assert_eq!(
        the_decision(&fixture.view()).cites,
        CitesInfo {
            in_change: ids(&[ATTRIBUTED, MATCHED]),
            elsewhere: ids(&[COMMAND, OUTSIDE]),
        }
    );
}

#[test]
fn every_edit_contributing_to_a_line_is_in_the_change() {
    let fixture = Fixture::default();

    assert_eq!(
        fixture.view().in_change,
        strings(&[ATTRIBUTED, BESIDE, MATCHED, WHITESPACE])
            .into_iter()
            .collect::<BTreeSet<_>>()
    );
}

#[test]
fn edit_decisions_lists_every_decision_citing_each_edit_in_the_change() {
    let fixture = Fixture::default().annotate(
        S1,
        vec![
            decision("d2", &[ATTRIBUTED]),
            decision("d1", &[ATTRIBUTED, MATCHED, OUTSIDE]),
        ],
        Vec::new(),
    );

    let view = fixture.view();

    assert_eq!(decision_names(&view), ["d1", "d2"]);
    assert_eq!(
        view.edit_decisions,
        BTreeMap::from([
            (ATTRIBUTED.to_owned(), strings(&["d1", "d2"])),
            (MATCHED.to_owned(), strings(&["d1"])),
        ])
    );
}

#[test]
fn an_attempt_is_restored_only_when_every_edit_of_it_is_restored() {
    let alternative = |title: &str, status, edits: &[&str]| Alternative {
        title: title.to_owned(),
        status,
        edits: ids(edits),
        reason: None,
    };
    let mut d1 = decision("d1", &[ATTRIBUTED]);
    d1.alternatives = vec![
        alternative(
            "per-store syncs",
            AlternativeStatus::Attempted,
            &["edit:x1", "edit:x2"],
        ),
        alternative(
            "half undone",
            AlternativeStatus::Attempted,
            &["edit:x1", "edit:x3"],
        ),
        alternative("never tried", AlternativeStatus::Proposed, &[]),
    ];
    d1.alternatives[0].reason = Some(Rationale {
        text: INFERENCE.to_owned(),
        speaker: Speaker::Annotator,
        explicit: false,
        source: None,
    });
    // Another session's restore of `edit:x3` says nothing of this one's.
    let fixture = Fixture::default()
        .annotate(S1, vec![d1], Vec::new())
        .restoring(&["edit:x1"])
        .restoring(&["edit:x2", "edit:x9"])
        .annotate(S2, Vec::new(), Vec::new())
        .restoring(&["edit:x3"]);

    let view = fixture.view();

    let restored: Vec<(&str, bool)> = the_decision(&view)
        .alternatives
        .iter()
        .map(|alternative| (alternative.title.as_str(), alternative.restored))
        .collect();
    assert_eq!(
        restored,
        [
            ("per-store syncs", true),
            ("half undone", false),
            ("never tried", false)
        ]
    );
    assert_eq!(
        the_decision(&view).alternatives[0],
        AlternativeInfo {
            title: "per-store syncs".to_owned(),
            status: AlternativeStatus::Attempted,
            edits: ids(&["edit:x1", "edit:x2"]),
            restored: true,
            reason: Some(RationaleInfo {
                text: INFERENCE.to_owned(),
                speaker: Speaker::Annotator,
                explicit: false,
                source: None,
                correction: None,
            }),
        }
    );
}

#[test]
fn a_decision_lists_its_sessions_question_items_about_its_cited_ids() {
    let fixture = Fixture::default()
        .annotate(
            S1,
            vec![decision("d1", &[ATTRIBUTED, OUTSIDE, COMMAND])],
            vec![
                // About a cited id in the change: listed.
                question("q1", &[ATTRIBUTED]),
                // About nothing: an item, but about no decision.
                question("q2", &[]),
                // About only cited ids outside the change: not an item.
                question("q3", &[OUTSIDE, COMMAND]),
                // An item, but about an id the decision does not cite.
                question("q4", &[MATCHED]),
                // An item through `MATCHED`, about a cited command.
                question("q5", &[COMMAND, MATCHED]),
            ],
        )
        // Another session's question about a cited id: not this decision's.
        .annotate(S2, Vec::new(), vec![question("q6", &[ATTRIBUTED])]);

    assert_eq!(
        the_decision(&fixture.view()).open_questions,
        strings(&["question:q1", "question:q5"])
    );
}

/// The question items of `fixture`'s annotations over [`files`].
fn items_of(fixture: &Fixture) -> Vec<GateItem> {
    let annotated = fixture.annotated();
    let in_change = fixture.view().in_change;
    question_items(&annotated, &in_change)
}

fn question_subject(session: &str, id: &str) -> Subject {
    Subject::Question {
        session: SessionId::new(session),
        id: id.to_owned(),
    }
}

#[test]
fn a_question_is_an_item_when_about_nothing_or_about_an_edit_in_the_change() {
    let fixture = Fixture::default()
        .annotate(
            S2,
            Vec::new(),
            vec![
                question("q9", &[OUTSIDE, MATCHED]),
                question("q1", &[]),
                question("q2", &[ATTRIBUTED]),
                question("q3", &[OUTSIDE]),
                question("q4", &[COMMAND]),
            ],
        )
        .annotate(S1, Vec::new(), vec![question("q1", &[WHITESPACE])]);

    let items = items_of(&fixture);

    let subjects: Vec<&Subject> = items.iter().map(|item| &item.subject).collect();
    assert_eq!(
        subjects,
        [
            &question_subject(S1, "q1"),
            &question_subject(S2, "q1"),
            &question_subject(S2, "q2"),
            &question_subject(S2, "q9"),
        ]
    );
    let item = &items[2];
    assert_eq!(item.origin(), Origin::OpenQuestion);
    assert_eq!(
        item.detail,
        GateDetail::OpenQuestion {
            text: "Why q2?".to_owned()
        }
    );
    assert_eq!(
        item.id,
        ItemId::new(Origin::OpenQuestion, &question_subject(S2, "q2"))
    );
}

#[test]
fn a_question_fingerprints_its_text_and_its_sorted_ids() {
    let fixture = Fixture::default().annotate(
        S1,
        Vec::new(),
        vec![
            question("q1", &[]),
            question("q2", &[OUTSIDE, ATTRIBUTED, OUTSIDE]),
        ],
    );

    let items = items_of(&fixture);

    let parts = |item: &GateItem| item.fingerprint.0.clone();
    assert_eq!(
        parts(&items[0]),
        BTreeMap::from([
            ("about".to_owned(), FingerprintPart::of(b"")),
            ("text".to_owned(), FingerprintPart::of(b"Why q1?")),
        ])
    );
    assert_eq!(
        parts(&items[1]),
        BTreeMap::from([
            ("about".to_owned(), FingerprintPart::of(b"edit:a\0edit:o")),
            ("text".to_owned(), FingerprintPart::of(b"Why q2?")),
        ])
    );
}

#[test]
fn rewording_a_question_or_changing_its_ids_changes_only_that_part() {
    let before = Fixture::default().annotate(S1, Vec::new(), vec![question("q1", &[ATTRIBUTED])]);
    let mut reworded_question = question("q1", &[ATTRIBUTED]);
    reworded_question.text = "Why, really?".to_owned();
    let reworded = Fixture::default().annotate(S1, Vec::new(), vec![reworded_question]);
    let moved =
        Fixture::default().annotate(S1, Vec::new(), vec![question("q1", &[ATTRIBUTED, OUTSIDE])]);
    let repeated = Fixture::default().annotate(
        S1,
        Vec::new(),
        vec![question("q1", &[ATTRIBUTED, ATTRIBUTED])],
    );

    let [before] = items_of(&before).try_into().unwrap();
    let [reworded] = items_of(&reworded).try_into().unwrap();
    let [moved] = items_of(&moved).try_into().unwrap();
    let [repeated] = items_of(&repeated).try_into().unwrap();

    assert_eq!(
        reworded.fingerprint.differing(&before.fingerprint),
        ["text"]
    );
    assert_eq!(moved.fingerprint.differing(&before.fingerprint), ["about"]);
    assert!(repeated.fingerprint.matches(&before.fingerprint));
    for item in [&reworded, &moved, &repeated] {
        assert_eq!(item.id, before.id);
    }
}

fn basis(gate: Vec<GateItem>, annotations: Vec<BasisAnnotation>) -> Basis {
    Basis {
        schema: BASIS_SCHEMA.to_owned(),
        worktree: PathBuf::from("/work/a"),
        base: BasisBase {
            commit: None,
            tree: "1".repeat(40),
        },
        target: BasisTarget {
            kind: TargetKind::WorkingTree,
            commit: None,
            tree: "2".repeat(40),
        },
        scope: BasisScope {
            include_untracked: Vec::new(),
            paths: Vec::new(),
        },
        records: Vec::new(),
        unreconciled_checkouts: Vec::new(),
        algorithms: Algorithms { derivation: 2 },
        explanations: Vec::new(),
        capture: Vec::new(),
        annotations,
        decisions: Vec::new(),
        gate,
        salt: None,
    }
}

fn pin(record: &str, session: &str, revision: Option<u32>) -> BasisAnnotation {
    BasisAnnotation {
        record: RecordId::new(record),
        session: SessionId::new(session),
        revision: revision.map(AnnotationRevision::new),
    }
}

fn acknowledgment(basis: &BasisId, items: &[&GateItem]) -> Judgment {
    Judgment {
        id: JudgmentId::new("ack"),
        basis: basis.clone(),
        kind: JudgmentKind::Acknowledge,
        items: items.iter().map(|item| item.id.clone()).collect(),
        reason: None,
        text: None,
        decision: None,
        correction: None,
        closes: BTreeMap::new(),
        withdraws: Vec::new(),
        who: Who {
            name: "Joni Savolainen".to_owned(),
            email: "joni@example.com".to_owned(),
        },
        at: UtcTime::from_unix(10),
    }
}

#[test]
fn question_items_take_states_and_reannotation_is_an_interpretation_change() {
    // Revision 1 asked q1 and q2, and both were acknowledged. Revision 2
    // rewords q1, keeps q2, and session 2 is newly annotated.
    let earlier_items = items_of(&Fixture::default().annotate(
        S1,
        Vec::new(),
        vec![question("q1", &[]), question("q2", &[])],
    ));
    let mut reworded = question("q1", &[]);
    reworded.text = "Why, really?".to_owned();
    let current_items =
        items_of(&Fixture::default().annotate(S1, Vec::new(), vec![reworded, question("q2", &[])]));
    let earlier = basis(earlier_items.clone(), vec![pin(RECORD, S1, Some(1))]);
    let earlier_id = earlier.basis_id().unwrap();
    let current = basis(
        current_items,
        vec![pin(RECORD, S1, Some(2)), pin(RECORD, S2, Some(1))],
    );
    let current_id = current.basis_id().unwrap();
    let judgments = [acknowledgment(
        &earlier_id,
        &earlier_items.iter().collect::<Vec<_>>(),
    )];
    let bases = [(earlier_id.clone(), earlier)];

    let store = JudgedStore::new(&judgments, bases.iter().map(|(id, basis)| (id, basis)));
    let names = ItemNames::new(&current.gate, BTreeMap::new(), false, |_| None);
    let report = store
        .review(&current_id, &current)
        .report_reading(&names, |related| {
            Ok::<_, Infallible>(vec![RelatedReading::default(); related.len()])
        })
        .unwrap();

    let states: Vec<(&str, ItemStateInfo)> = report
        .gate
        .items
        .iter()
        .map(|item| (item.name.as_str(), item.state))
        .collect();
    assert_eq!(
        states,
        [
            ("question:q1", ItemStateInfo::Reopened),
            ("question:q2", ItemStateInfo::Carried),
        ]
    );
    assert_eq!(report.gate.items[0].reopen_reasons, ["question reworded"]);
    let [related] = report.status.related.as_slice() else {
        panic!("one related review: {:?}", report.status.related);
    };
    assert_eq!(related.review, earlier_id);
    assert_eq!(
        related.interpretation,
        [
            InterpretationChange {
                session: SessionId::new(S1),
                from: Some(revision(1)),
                to: Some(revision(2)),
            },
            InterpretationChange {
                session: SessionId::new(S2),
                from: None,
                to: Some(revision(1)),
            },
        ]
    );
}

#[test]
fn an_unchanged_interpretation_is_omitted_from_a_related_reviews_json() {
    let gate = items_of(&Fixture::default().annotate(S1, Vec::new(), vec![question("q1", &[])]));
    let earlier = basis(gate.clone(), vec![pin(RECORD, S1, Some(1))]);
    let earlier_id = earlier.basis_id().unwrap();
    let mut current = earlier.clone();
    current.worktree = PathBuf::from("/work/b");
    let current_id = current.basis_id().unwrap();
    let bases = [(earlier_id, earlier)];

    let store = JudgedStore::new(&[], bases.iter().map(|(id, basis)| (id, basis)));
    let names = ItemNames::new(&current.gate, BTreeMap::new(), false, |_| None);
    let report = store
        .review(&current_id, &current)
        .report_reading(&names, |related| {
            Ok::<_, Infallible>(vec![RelatedReading::default(); related.len()])
        })
        .unwrap();

    let json = serde_json::to_value(&report.status).unwrap();
    assert_eq!(json["related"][0]["relation"], "same_content");
    assert!(json["related"][0].get("interpretation").is_none(), "{json}");
}

#[test]
fn decision_pins_name_each_decision_in_view() {
    let fixture = Fixture::default()
        .annotate(S1, vec![decision("d1", &[ATTRIBUTED])], Vec::new())
        .annotate(
            S2,
            vec![decision("d2", &[MATCHED]), decision("d1", &[WHITESPACE])],
            Vec::new(),
        );
    let mut decisions = fixture.view().decisions;
    decisions.reverse();

    let pinned = decision_pins(&decisions);

    let decision = |session: &str, id: &str, name: &str| BasisDecision {
        decision: DecisionRef {
            record: RecordId::new(RECORD),
            session: SessionId::new(session),
            revision: AnnotationRevision::FIRST,
            id: id.to_owned(),
        },
        name: name.to_owned(),
    };
    assert_eq!(
        pinned,
        [
            decision(S1, "d1", "3f2a9c1e:d1"),
            decision(S2, "d1", "9c0d1e2f:d1"),
            decision(S2, "d2", "d2"),
        ]
    );
}

#[test]
fn interpretation_changes_list_each_session_whose_annotation_differs() {
    let earlier = [
        pin("r1", "s-a", Some(1)),
        pin("r1", "s-b", None),
        pin("r1", "s-d", Some(1)),
        pin("r1", "s-e", Some(1)),
        pin("r1", "s-f", Some(3)),
        pin("r1", "s-g", Some(1)),
        pin("r1", "s-h", None),
    ];
    let current = [
        pin("r1", "s-a", Some(2)),
        pin("r1", "s-b", Some(1)),
        pin("r1", "s-c", Some(1)),
        pin("r1", "s-d", None),
        pin("r1", "s-f", Some(3)),
        // Revisions are numbered per record: another record's revision 1
        // is another annotation.
        pin("r2", "s-g", Some(1)),
        pin("r2", "s-h", None),
    ];

    let changes = interpretation_changes(&earlier, &current);

    let change = |session: &str, from: Option<u32>, to: Option<u32>| InterpretationChange {
        session: SessionId::new(session),
        from: from.map(revision),
        to: to.map(revision),
    };
    assert_eq!(
        changes,
        [
            change("s-a", Some(1), Some(2)),
            change("s-b", None, Some(1)),
            change("s-c", None, Some(1)),
            change("s-d", Some(1), None),
            change("s-e", Some(1), None),
            change("s-g", Some(1), Some(1)),
        ]
    );
}

#[test]
fn explained_spans_say_how_they_are_associated_in_json() {
    let spans = [
        explained(
            "src/a.rs",
            SpanSide::Target,
            12,
            29,
            Association::Attributed,
            None,
        ),
        explained(
            "src/a.rs",
            SpanSide::Target,
            50,
            2,
            Association::WhitespaceOnly,
            None,
        ),
        explained(
            "src/b.rs",
            SpanSide::Target,
            3,
            2,
            Association::ContentMatch,
            Some(false),
        ),
    ];

    assert_eq!(
        serde_json::to_value(spans).unwrap(),
        serde_json::json!([
            { "path": "src/a.rs", "side": "target", "start": 12, "count": 29,
              "association": "attributed" },
            { "path": "src/a.rs", "side": "target", "start": 50, "count": 2,
              "association": "whitespace_only" },
            { "path": "src/b.rs", "side": "target", "start": 3, "count": 2,
              "association": "content_match", "search_complete": false },
        ])
    );
}

#[test]
fn ten_thousand_decisions_attach_to_five_thousand_spans_promptly() {
    // 50 files of 100 spans, each attributed to its own edit. Each decision
    // cites two edits, and each question asks about one.
    let edit = |index: usize| format!("edit:{:016x}", index % 5_000);
    let files: Vec<FileReview> = (0..50)
        .map(|f| {
            let spans = (0..100)
                .map(|s| {
                    let edit = edit(f * 100 + s);
                    span(
                        SpanSide::Target,
                        s * 3 + 1,
                        2,
                        attributed(Relation::Introduced, &[&edit]),
                    )
                })
                .collect();
            file(&format!("src/f{f:02}.rs"), spans)
        })
        .collect();
    let decisions = (0..10_000)
        .map(|d| decision(&format!("d{d}"), &[&edit(d), &edit(d + 1)]))
        .collect();
    let questions = (0..10_000)
        .map(|q| question(&format!("q{q}"), &[&edit(q)]))
        .collect();
    let fixture = Fixture::default().annotate(S1, decisions, questions);

    let (view, items) = finishes_within(Duration::from_secs(20), move || {
        let annotated = fixture.annotated();
        let view = in_view(&annotated, &names(&annotated), &files);
        let items = question_items(&annotated, &view.in_change);
        (view, items)
    });

    assert_eq!(view.decisions.len(), 10_000);
    assert!(
        view.decisions
            .iter()
            .all(|decision| decision.explains.len() == 2 && decision.open_questions.len() == 4)
    );
    assert_eq!(view.edit_decisions.len(), 5_000);
    assert_eq!(items.len(), 10_000);
}

#[test]
fn questions_repeating_an_edit_list_on_ten_thousand_decisions_promptly() {
    // Every decision cites one edit, and each of 100 questions names that
    // edit 1,000 times, the most a question may list. A repeated id adds no
    // work, in a review or in an excerpt of it.
    let files = vec![file(
        "src/a.rs",
        vec![span(
            SpanSide::Target,
            1,
            1,
            attributed(Relation::Introduced, &[ATTRIBUTED]),
        )],
    )];
    let decisions = (0..10_000)
        .map(|d| decision(&format!("d{d}"), &[ATTRIBUTED]))
        .collect();
    let repeated = vec![ATTRIBUTED; 1_000];
    let questions = (0..100)
        .map(|q| question(&format!("q{q}"), &repeated))
        .collect();
    let fixture = Fixture::default().annotate(S1, decisions, questions);

    let (view, excerpted, items) = finishes_within(Duration::from_secs(20), move || {
        let annotated = fixture.annotated();
        let names = names(&annotated);
        let view = in_view(&annotated, &names, &files);
        let excerpted = in_excerpt(
            &annotated,
            &names,
            &files[0].path_bytes,
            &files[0].hunks[0].spans,
        );
        let items = question_items(&annotated, &view.in_change);
        (view, excerpted, items)
    });

    let mut asked: Vec<String> = (0..100).map(|q| format!("question:q{q}")).collect();
    asked.sort();
    for view in [&view, &excerpted] {
        assert_eq!(view.decisions.len(), 10_000);
        assert!(
            view.decisions
                .iter()
                .all(|decision| decision.open_questions == asked)
        );
    }
    assert_eq!(items.len(), 100);
}
