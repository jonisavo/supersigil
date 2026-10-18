//! The decisions behind a change as the terminal summary and `why` print
//! them: how each explains its lines, corrections ahead of the rationale,
//! corrections with no decision in view, open-question rows, the annotation
//! notes, and the interpretation status line.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use common::{agreed, attribution, chain, eid, introduced, unexplained};
use serde_json::json;
use supersigil_anchor::lines::split_lines;
use supersigil_anchor::{ChainClass, ChainEnd, Origin, PathAttribution};
use supersigil_record::interpretations::{
    AlternativeStatus, AnnotationRevision, Observed, Source, Speaker,
};
use supersigil_record::reviews::{
    Algorithms, BASIS_SCHEMA, Basis, BasisBase, BasisId, BasisScope, BasisTarget, CorrectionKind,
    DecisionRef, Fingerprint, GateDetail, GateItem, JudgmentKind, PathBytes, Subject, TargetKind,
    Who,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, RecordId, SessionId, TurnId};
use supersigil_review::judging::Relation;
use supersigil_review::model::{
    AlternativeInfo, AncestryInfo, AnnotationInfo, Association, BaseInfo, BytesStatus,
    ChangedEvidence, CitesInfo, CompletionSource, CorrectionInfo, DecisionInfo, EvidenceInfo,
    ExplainedSpan, FileKindInfo, FileReview, FileStatus, GateInfo, HunkReview, ImplicationInfo,
    InterpretationChange, JudgmentInfo, OriginsInfo, RationaleInfo, RelatedInfo, SavedBasis,
    ScopeInfo, Span, SpanSide, StatusInfo, TargetInfo, TargetKindInfo, UnattributedSummary,
};
use supersigil_review::names::ItemNames;
use supersigil_review::outcome::{
    AttributedEdit, AttributionState, Outcome, Relation as LineRelation,
};
use supersigil_review::printed::Addressing;
use supersigil_review::summary::render_summary;
use supersigil_review::why::{OnDiskCheck, Why, WhyLine, WhyTarget, render_why, why_line};
use supersigil_review::{REVIEW_SCHEMA, Review, WHY_SCHEMA};

/// Candidate sessions: two annotated, one not.
const S1: &str = "3f2a9c1e-5b7d-4e0a-9c1f-0a1b2c3d4e5f";
const S2: &str = "9c0d1e2f-3a4b-4c5d-8e6f-7a8b9c0d1e2f";
const S3: &str = "7b1e44d0-2c3d-4e5f-a6b7-c8d9e0f1a2b3";
/// A session an earlier review read an annotation of, whose record is gone.
const S4: &str = "c4d5e6f7-8a9b-4c0d-9e1f-2a3b4c5d6e7f";
/// The records holding annotations, and the gone one.
const R1: &str = "0a1b2c3d-1111-4111-8111-111111111111";
const R2: &str = "5e6f7a8b-2222-4222-8222-222222222222";
const GONE: &str = "9f8e7d6c-3333-4333-8333-333333333333";

/// The reviewed worktree, where the review ran.
const WORKTREE: &str = "/work/repo";

const FILES: &str = "src/files.rs";
const PUBLICATION: &str = "src/files/publication.rs";
const WRITE: &str = "src/reviews/write.rs";

fn time(text: &str) -> UtcTime {
    text.parse().unwrap()
}

fn who() -> Who {
    Who {
        name: "Joni Savolainen".to_owned(),
        email: "joni@example.com".to_owned(),
    }
}

/// The earlier review the corrections and the completion were recorded on.
fn earlier() -> BasisId {
    serde_json::from_value(json!("1".repeat(64))).unwrap()
}

/// `session`'s decision `id` in `revision` of `record`.
fn reference(record: &str, session: &str, revision: u32, id: &str) -> DecisionRef {
    DecisionRef {
        record: RecordId::new(record),
        session: SessionId::new(session),
        revision: AnnotationRevision::new(revision),
        id: id.to_owned(),
    }
}

/// Correction `judgment` of `S1`'s `d1` in revision 1, recorded at `at`
/// with `words`: the reason of a `wrong` or `not_together` correction, the
/// text of a `constraint`. Anchored directly and not yet seen.
fn correction(judgment: &str, kind: CorrectionKind, words: &str, at: &str) -> CorrectionInfo {
    let words = Some(words.to_owned());
    let (reason, text) = if kind == CorrectionKind::Constraint {
        (None, words)
    } else {
        (words, None)
    };
    CorrectionInfo {
        judgment: JudgmentId::new(judgment),
        decision: reference(R1, S1, 1, "d1"),
        kind,
        reason,
        text,
        who: who(),
        at: time(at),
        addressed: None,
        seen: false,
        review: earlier(),
    }
}

/// A rationale line `speaker` stated, quoting `quote`, which the annotator
/// put as `text`.
fn stated(speaker: Speaker, text: &str, quote: &str) -> RationaleInfo {
    RationaleInfo {
        text: text.to_owned(),
        speaker,
        explicit: true,
        source: Some(Source {
            turn: TurnId::new("t1"),
            quote: quote.to_owned(),
        }),
        correction: None,
    }
}

/// A rationale line the annotator inferred and gave to `speaker`.
fn inferred(speaker: Speaker, text: &str) -> RationaleInfo {
    RationaleInfo {
        text: text.to_owned(),
        speaker,
        explicit: false,
        source: None,
        correction: None,
    }
}

/// The constraint of correction `judgment`, as anchoring adds it.
fn constraint(judgment: &str, text: &str) -> RationaleInfo {
    RationaleInfo {
        text: text.to_owned(),
        speaker: Speaker::Human,
        explicit: true,
        source: None,
        correction: Some(JudgmentId::new(judgment)),
    }
}

/// `count` lines from `start` on `side` of `path`, introduced or replaced
/// by a cited edit.
fn explained(path: &str, side: SpanSide, start: usize, count: usize) -> ExplainedSpan {
    ExplainedSpan {
        path: PathBytes::new(path),
        side,
        start,
        count,
        association: Association::Attributed,
        search_complete: None,
    }
}

/// The same, where the cited edit changed only whitespace.
fn whitespace(path: &str, side: SpanSide, start: usize, count: usize) -> ExplainedSpan {
    ExplainedSpan {
        association: Association::WhitespaceOnly,
        ..explained(path, side, start, count)
    }
}

/// Added lines of `path` that a cited edit's text covers while no chain
/// explains them.
fn matched(path: &str, start: usize, count: usize, search_complete: bool) -> ExplainedSpan {
    ExplainedSpan {
        association: Association::ContentMatch,
        search_complete: Some(search_complete),
        ..explained(path, SpanSide::Target, start, count)
    }
}

/// Decision `name` of `S1`'s second revision in `R1`, with nothing but its
/// title.
fn decision(name: &str, title: &str) -> DecisionInfo {
    DecisionInfo {
        name: name.to_owned(),
        session: SessionId::new(S1),
        record: RecordId::new(R1),
        revision: AnnotationRevision::new(2),
        id: name.to_owned(),
        title: title.to_owned(),
        disputed: false,
        corrections: Vec::new(),
        rationale: Vec::new(),
        alternatives: Vec::new(),
        implications: Vec::new(),
        open_questions: Vec::new(),
        cites: CitesInfo::default(),
        explains: Vec::new(),
    }
}

/// Decision `d1` with a line of every kind: a correction of revision 1
/// anchored through its edits and not yet seen, a constraint revision 2
/// saw, stated, reworded and inferred rationale, three alternatives, an
/// inferred and a stated implication, an open question, and cites outside
/// the change.
fn sync_plan() -> DecisionInfo {
    let wrong = CorrectionInfo {
        addressed: Some(AnnotationRevision::FIRST),
        ..correction(
            "wrong",
            CorrectionKind::Wrong,
            "the plan exists for crash safety, not inspection",
            "2026-10-15T09:30:00Z",
        )
    };
    let must = "both stores must reach disk before the manifest";
    let must_correction = CorrectionInfo {
        decision: reference(R1, S1, 2, "d1"),
        seen: true,
        ..correction(
            "must",
            CorrectionKind::Constraint,
            must,
            "2026-10-16T08:00:00Z",
        )
    };
    let syncs = "Two sync passes would double the fsyncs.";
    DecisionInfo {
        disputed: true,
        corrections: vec![wrong, must_correction],
        rationale: vec![
            constraint("must", must),
            stated(
                Speaker::Human,
                "make the sync plan inspectable",
                "make the sync plan inspectable",
            ),
            stated(
                Speaker::Agent,
                "syncing per file was too slow",
                "Syncing the directory after each file costs a full fsync per file.",
            ),
            inferred(
                Speaker::Annotator,
                "the author preferred one plan over per-store syncs",
            ),
            inferred(Speaker::Agent, "one plan keeps the stores in step"),
        ],
        alternatives: vec![
            AlternativeInfo {
                title: "per-store syncs".to_owned(),
                status: AlternativeStatus::Attempted,
                edits: vec![eid("edit:3")],
                restored: true,
                reason: Some(stated(Speaker::Agent, syncs, syncs)),
            },
            AlternativeInfo {
                title: "a sync thread".to_owned(),
                status: AlternativeStatus::Attempted,
                edits: vec![eid("edit:4"), eid("edit:5")],
                restored: false,
                reason: None,
            },
            AlternativeInfo {
                title: "no directory sync at all".to_owned(),
                status: AlternativeStatus::Proposed,
                edits: Vec::new(),
                restored: false,
                reason: Some(inferred(
                    Speaker::Annotator,
                    "the author never asked for it",
                )),
            },
        ],
        implications: vec![
            ImplicationInfo {
                text: "every new file kind needs a step in the plan".to_owned(),
                explicit: false,
                source: None,
            },
            ImplicationInfo {
                text: "new stores join the plan".to_owned(),
                explicit: true,
                source: Some(Source {
                    turn: TurnId::new("t2"),
                    quote: "new stores join the plan".to_owned(),
                }),
            },
        ],
        open_questions: vec!["question:q1".to_owned()],
        cites: CitesInfo {
            in_change: vec![eid("edit:1"), eid("edit:2")],
            elsewhere: vec![eid("command:1"), eid("edit:9")],
        },
        explains: vec![
            matched(FILES, 3, 2, false),
            explained(PUBLICATION, SpanSide::Base, 12, 3),
            explained(PUBLICATION, SpanSide::Target, 12, 20),
            explained(PUBLICATION, SpanSide::Target, 32, 9),
            whitespace(PUBLICATION, SpanSide::Target, 41, 1),
            explained(WRITE, SpanSide::Target, 226, 35),
        ],
        ..decision("d1", "Publish both stores through one sync plan")
    }
}

/// Decision `d3`, explaining the lines of `src/reviews/write.rs` and
/// matching the content of `src/files.rs`.
fn write_plan() -> DecisionInfo {
    DecisionInfo {
        rationale: vec![inferred(Speaker::Annotator, "reviews are one more store")],
        cites: CitesInfo {
            in_change: vec![eid("edit:2")],
            elsewhere: Vec::new(),
        },
        explains: vec![
            matched(FILES, 3, 2, false),
            explained(WRITE, SpanSide::Target, 226, 35),
        ],
        ..decision("d3", "Write reviews through the same plan")
    }
}

/// `count` lines from `start` on `side`, with `outcome`.
fn span(side: SpanSide, start: usize, count: usize, outcome: Outcome) -> Span {
    Span {
        side,
        start,
        count,
        outcome,
        provenance: Vec::new(),
    }
}

fn attributed(edit: &str, relation: LineRelation) -> Outcome {
    Outcome::Attributed {
        edits: vec![AttributedEdit {
            edit: eid(edit),
            relation,
            classes: BTreeSet::new(),
        }],
    }
}

/// A modified text file whose one hunk holds `spans`.
fn file(path: &str, spans: Vec<Span>) -> FileReview {
    let lines = |side| -> usize {
        spans
            .iter()
            .filter(|span| span.side == side)
            .map(|span| span.count)
            .sum()
    };
    let first = |side| {
        spans
            .iter()
            .find(|span| span.side == side)
            .map_or(0, |span| span.start)
    };
    let hunk = HunkReview {
        base_start: first(SpanSide::Base),
        base_count: lines(SpanSide::Base),
        target_start: first(SpanSide::Target),
        target_count: lines(SpanSide::Target),
        removed: Vec::new(),
        added: Vec::new(),
        spans,
    };
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
        hunks: vec![hunk],
        mentions: Vec::new(),
        command_changes: Vec::new(),
    }
}

/// The files [`sync_plan`] and [`write_plan`] explain.
fn files() -> Vec<FileReview> {
    vec![
        file(
            FILES,
            vec![span(
                SpanSide::Target,
                3,
                2,
                Outcome::ContentMatch {
                    relation: LineRelation::Introduced,
                    edits: vec![eid("edit:1"), eid("edit:2")],
                    search_complete: false,
                },
            )],
        ),
        file(
            PUBLICATION,
            vec![
                span(
                    SpanSide::Base,
                    12,
                    3,
                    attributed("edit:1", LineRelation::Replaced),
                ),
                span(
                    SpanSide::Target,
                    12,
                    20,
                    attributed("edit:1", LineRelation::Introduced),
                ),
                span(
                    SpanSide::Target,
                    32,
                    9,
                    attributed("edit:2", LineRelation::Introduced),
                ),
                span(
                    SpanSide::Target,
                    41,
                    1,
                    Outcome::WhitespaceOnly {
                        edits: vec![eid("edit:2")],
                    },
                ),
            ],
        ),
        file(
            WRITE,
            vec![span(
                SpanSide::Target,
                226,
                35,
                attributed("edit:2", LineRelation::Introduced),
            )],
        ),
    ]
}

/// Corrections anchored to no decision in view: one of a decision `S1`'s
/// shown revision dropped, which that revision saw, and one of a session
/// whose record is gone.
fn unanchored() -> Vec<CorrectionInfo> {
    vec![
        CorrectionInfo {
            decision: reference(R1, S1, 1, "d7"),
            seen: true,
            ..correction(
                "dropped",
                CorrectionKind::Wrong,
                "d7 was never needed",
                "2026-10-14T10:00:00Z",
            )
        },
        CorrectionInfo {
            decision: reference(GONE, S4, 1, "d2"),
            ..correction(
                "gone",
                CorrectionKind::NotTogether,
                "two unrelated fixes",
                "2026-10-15T11:00:00Z",
            )
        },
    ]
}

/// `S1` annotated in two records and observed since, `S3` not annotated,
/// both in the review's checkout's record, and `S2` annotated without a
/// last observation time in a nested checkout's record.
fn annotations() -> Vec<AnnotationInfo> {
    vec![
        AnnotationInfo {
            session: SessionId::new(S1),
            record: Some(RecordId::new(R1)),
            checkout: Some(PathBuf::from(WORKTREE)),
            revision: Some(AnnotationRevision::new(2)),
            observed: Some(Observed {
                count: 100,
                last: Some(time("2026-10-15T14:02:11Z")),
            }),
            observations_now: 112,
            annotated_records: 2,
        },
        AnnotationInfo {
            session: SessionId::new(S3),
            record: Some(RecordId::new(R1)),
            checkout: Some(PathBuf::from(WORKTREE)),
            revision: None,
            observed: None,
            observations_now: 7,
            annotated_records: 0,
        },
        AnnotationInfo {
            session: SessionId::new(S2),
            record: Some(RecordId::new(R2)),
            checkout: Some(PathBuf::from("/work/repo/vendor/sync")),
            revision: Some(AnnotationRevision::FIRST),
            observed: Some(Observed {
                count: 40,
                last: None,
            }),
            observations_now: 41,
            annotated_records: 1,
        },
    ]
}

fn changed(session: &str, from: Option<u32>, to: Option<u32>) -> InterpretationChange {
    InterpretationChange {
        session: SessionId::new(session),
        from: from.map(AnnotationRevision::new),
        to: to.map(AnnotationRevision::new),
    }
}

/// A review of the same content completed on 2026-10-15, whose
/// annotations differ from this one's by `interpretation`.
fn completed(interpretation: Vec<InterpretationChange>) -> RelatedInfo {
    RelatedInfo {
        review: earlier(),
        relation: Relation::SameContent,
        completion: Some(JudgmentInfo {
            id: JudgmentId::new("done"),
            kind: JudgmentKind::Complete,
            review: earlier(),
            reason: None,
            text: None,
            decision: None,
            correction: None,
            who: who(),
            at: time("2026-10-15T16:00:00Z"),
        }),
        changed_paths: Some(Vec::new()),
        changed_evidence: ChangedEvidence::default(),
        interpretation,
        uncovered: Vec::new(),
    }
}

fn basis() -> SavedBasis {
    let document = Basis {
        schema: BASIS_SCHEMA.to_owned(),
        worktree: PathBuf::from("/work/repo"),
        base: BasisBase {
            commit: Some("a".repeat(40)),
            tree: "b".repeat(40),
        },
        target: BasisTarget {
            kind: TargetKind::WorkingTree,
            commit: None,
            tree: "c".repeat(40),
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
        annotations: Vec::new(),
        decisions: Vec::new(),
        gate: Vec::new(),
        salt: None,
    };
    SavedBasis {
        id: document.basis_id().unwrap(),
        document,
    }
}

/// A review whose files [`sync_plan`] and [`write_plan`] explain, with
/// `S1`'s open question in its gate, the [`unanchored`] corrections, the
/// [`annotations`], and a completed review of the same content that read
/// other annotations.
fn review() -> Review {
    let gate = [GateItem::new(
        Subject::Question {
            session: SessionId::new(S1),
            id: "q1".to_owned(),
        },
        GateDetail::OpenQuestion {
            text: "Should the plan also cover the judgments log?".to_owned(),
        },
        Fingerprint::default(),
    )];
    let names = ItemNames::new(&gate, BTreeMap::new(), false, |path| {
        path.to_str().map(str::to_owned)
    });
    Review {
        schema: REVIEW_SCHEMA,
        basis: Some(basis()),
        basis_unsaved: None,
        worktree: "/work/repo".to_owned(),
        base: BaseInfo {
            commit: Some("a".repeat(40)),
            tree: "b".repeat(40),
            ancestry: AncestryInfo::Ancestor,
        },
        target: TargetInfo {
            kind: TargetKindInfo::WorkingTree,
            commit: Some("a".repeat(40)),
            tree: "c".repeat(40),
        },
        status: StatusInfo {
            related: vec![completed(vec![
                changed(S1, Some(1), Some(2)),
                changed(S2, None, Some(1)),
                changed(S4, Some(1), None),
            ])],
            ..StatusInfo::default()
        },
        gate: GateInfo::unjudged(&gate, &names),
        unattributed: UnattributedSummary::default(),
        scope: ScopeInfo::default(),
        origins: OriginsInfo::default(),
        records: Vec::new(),
        evidence: EvidenceInfo::default(),
        files: files(),
        edits: BTreeMap::new(),
        decisions: vec![sync_plan(), write_plan()],
        unanchored_corrections: unanchored(),
        edit_decisions: BTreeMap::new(),
        annotations: annotations(),
    }
}

/// `review`'s summary as a review run in [`WORKTREE`] without store options
/// prints it.
fn summary(review: &Review) -> String {
    render_summary(review, &at_worktree())
}

/// The addressing of a review run in [`WORKTREE`] without options.
fn at_worktree() -> Addressing {
    Addressing {
        resolved_checkout: Some(PathBuf::from(WORKTREE)),
        ..Addressing::default()
    }
}

/// The lines of `text` after the line `header`, up to the next blank line.
fn section<'t>(text: &'t str, header: &str) -> Vec<&'t str> {
    text.lines()
        .skip_while(|line| *line != header)
        .skip(1)
        .take_while(|line| !line.is_empty())
        .collect()
}

/// The lines under decision `name` in the decisions section of `text`.
fn details<'t>(text: &'t str, name: &str) -> Vec<&'t str> {
    let heading = format!("  {name}  ");
    section(text, "Decisions:")
        .into_iter()
        .skip_while(|line| !line.starts_with(&heading))
        .skip(1)
        .take_while(|line| line.starts_with("      "))
        .collect()
}

/// The status lines after the four header lines.
fn status(text: &str) -> Vec<&str> {
    text.lines()
        .skip(4)
        .take_while(|line| !line.starts_with("Gate: "))
        .collect()
}

#[test]
fn the_summary_prints_the_decisions_behind_a_change() {
    insta::assert_snapshot!(summary(&review()));
}

#[test]
fn each_file_lists_the_decisions_explaining_it_apart_from_content_matches() {
    let text = summary(&review());

    assert_eq!(
        section(&text, "files:"),
        [
            "  M src/files.rs  +2 -0  2 content match (search incomplete)",
            "    decisions by content match only: d1, d3",
            "  M src/files/publication.rs  +30 -3  29 introduced, 3 replaced, 1 whitespace-only",
            "    decisions: d1",
            "  M src/reviews/write.rs  +35 -0  35 introduced",
            "    decisions: d1, d3",
        ],
        "{text}"
    );
}

#[test]
fn explains_merges_adjacent_spans_and_says_which_lines_were_removed() {
    let text = summary(&review());
    let mut apart = review();
    apart.decisions = vec![DecisionInfo {
        explains: vec![
            explained(WRITE, SpanSide::Base, 7, 1),
            explained(WRITE, SpanSide::Base, 8, 2),
            explained(WRITE, SpanSide::Target, 7, 1),
            explained(WRITE, SpanSide::Target, 9, 1),
            whitespace(WRITE, SpanSide::Target, 10, 1),
            matched(WRITE, 11, 1, true),
            matched(WRITE, 12, 3, true),
        ],
        ..decision("d2", "Spans apart")
    }];

    let apart = summary(&apart);

    assert_eq!(
        details(&text, "d1")[..2],
        [
            "      explains src/files/publication.rs 12-40, 41 (whitespace only), (removed) 12-14, \
             src/reviews/write.rs 226-260",
            "      content match only: src/files.rs 3-4 (search incomplete)",
        ],
        "{text}"
    );
    // A single line is one number; lines apart, on the other side, or
    // explained another way stay apart.
    assert_eq!(
        details(&apart, "d2"),
        [
            "      explains src/reviews/write.rs 7, 9, 10 (whitespace only), (removed) 7-9",
            "      content match only: src/reviews/write.rs 11-14",
        ],
        "{apart}"
    );
}

#[test]
fn a_decision_that_only_matches_content_explains_no_line() {
    let mut review = review();
    review.decisions = vec![DecisionInfo {
        explains: vec![matched(FILES, 3, 2, false)],
        ..decision("d2", "Match only")
    }];

    let text = summary(&review);

    assert_eq!(
        details(&text, "d2"),
        ["      content match only: src/files.rs 3-4 (search incomplete)"],
        "{text}"
    );
    assert!(!text.contains("explains"), "{text}");
    assert!(
        text.contains("\n    decisions by content match only: d2\n"),
        "{text}"
    );
    assert!(!text.contains("\n    decisions: "), "{text}");
}

#[test]
fn corrections_precede_the_rationale_saying_what_they_addressed_and_who_saw_them() {
    let text = summary(&review());

    assert_eq!(
        details(&text, "d1")[2..10],
        [
            "      correction (wrong), 2026-10-15, Joni Savolainen: \"the plan exists for crash \
             safety, not inspection\" (addressed revision 1) (not yet seen by an annotation)",
            "      correction (constraint), 2026-10-16, Joni Savolainen: \"both stores must reach \
             disk before the manifest\" (seen by revision 2)",
            "      human, explicit, correction: \"both stores must reach disk before the manifest\"",
            "      human, explicit: \"make the sync plan inspectable\"",
            "      agent, explicit: \"Syncing the directory after each file costs a full fsync per \
             file.\"",
            "        as: syncing per file was too slow",
            "      annotator: the author preferred one plan over per-store syncs",
            "      agent, inferred: one plan keeps the stores in step",
        ],
        "{text}"
    );
}

#[test]
fn an_explicit_line_without_a_source_is_never_shown_as_a_quote() {
    let mut review = review();
    let unsourced = RationaleInfo {
        source: None,
        ..stated(Speaker::Agent, "the plan is simpler", "the plan is simpler")
    };
    review.decisions = vec![DecisionInfo {
        rationale: vec![unsourced],
        implications: vec![ImplicationInfo {
            text: "nothing else syncs".to_owned(),
            explicit: true,
            source: None,
        }],
        ..decision("d2", "Unsourced")
    }];

    let text = summary(&review);

    assert_eq!(
        details(&text, "d2"),
        [
            "      agent, inferred: the plan is simpler",
            "      implication: nothing else syncs",
        ],
        "{text}"
    );
}

#[test]
fn alternatives_implications_questions_and_other_cites_follow_the_rationale() {
    let text = summary(&review());

    assert_eq!(
        details(&text, "d1")[10..],
        [
            "      alternative, attempted and restored: per-store syncs (edit:3)",
            "        agent, explicit: \"Two sync passes would double the fsyncs.\"",
            "      alternative, attempted: a sync thread (edit:4, edit:5)",
            "      alternative, proposed: no directory sync at all",
            "        annotator: the author never asked for it",
            "      implication: every new file kind needs a step in the plan",
            "      implication, explicit: \"new stores join the plan\"",
            "      open question: question:q1",
            "      cites elsewhere: command:1, edit:9",
        ],
        "{text}"
    );
    assert_eq!(
        details(&text, "d3"),
        [
            "      explains src/reviews/write.rs 226-260",
            "      content match only: src/files.rs 3-4 (search incomplete)",
            "      annotator: reviews are one more store",
        ],
        "{text}"
    );
}

#[test]
fn a_disputed_decision_says_so_after_its_title() {
    let text = summary(&review());

    let headings: Vec<&str> = section(&text, "Decisions:")
        .into_iter()
        .filter(|line| !line.starts_with("    "))
        .collect();

    assert_eq!(
        headings,
        [
            "  d1  Publish both stores through one sync plan  [disputed]",
            "  d3  Write reviews through the same plan",
        ],
        "{text}"
    );
}

#[test]
fn corrections_anchored_to_no_decision_in_view_are_listed_with_what_they_name() {
    let text = summary(&review());
    let mut without = review();
    without.decisions.clear();
    without.unanchored_corrections.clear();

    let without = summary(&without);

    assert_eq!(
        section(&text, "Corrections with no decision in view:"),
        [
            "  3f2a9c1e:d7 (revision 1, 0a1b2c3d)  wrong  2026-10-14  Joni Savolainen: \"d7 was \
             never needed\" (seen by revision 2)",
            "  c4d5e6f7:d2 (revision 1, 9f8e7d6c)  not together  2026-10-15  Joni Savolainen: \
             \"two unrelated fixes\" (not yet seen by an annotation)",
        ],
        "{text}"
    );
    for omitted in [
        "Decisions:",
        "Corrections with no decision in view:",
        "decisions",
    ] {
        assert!(!without.contains(omitted), "{omitted} in {without}");
    }
}

#[test]
fn an_open_question_row_shows_the_question() {
    let text = summary(&review());

    assert!(
        text.contains("\n  question:q1  open  Should the plan also cover the judgments log?\n"),
        "{text}"
    );
}

#[test]
fn notes_name_sessions_not_annotated_annotated_in_part_or_in_several_records() {
    let text = summary(&review());
    let addressing = Addressing {
        checkout: Some(PathBuf::from("/work/repo")),
        review_dir: Some(PathBuf::from("/scratch/re\u{1b}views")),
        record_dir: Some(PathBuf::from("/scratch/records")),
        ..at_worktree()
    };
    let addressed = render_summary(&review(), &addressing);

    // The note names the session by its short id, and the command by its
    // whole id: the skill resolves a prefix among every session of the
    // record, where one unique among the review's sessions can match two.
    assert_eq!(
        section(&text, "Not gate items:"),
        [
            "  session 3f2a9c1e annotated through 2026-10-15 14:02; 12 observations since",
            "  session 3f2a9c1e annotated in 2 records; using 0a1b2c3d",
            &format!(
                "  session 7b1e44d0 not annotated; in Claude Code run /supersigil-annotate {S3}"
            ),
            "  session 9c0d1e2f annotated after 40 observations; 1 observation since",
        ],
        "{text}"
    );
    // The command carries the store options the review was given.
    assert!(
        addressed.contains(&format!(
            "\n  session 7b1e44d0 not annotated; in Claude Code run /supersigil-annotate {S3} \
             --record-dir=/scratch/records --checkout=/work/repo --review-dir=<as given>\n"
        )),
        "{addressed}"
    );
    assert!(
        addressed.ends_with(
            "The --review-dir value is not valid UTF-8 or holds a control character, so \
             commands show <as given> for it: pass it as given.\n"
        ),
        "{addressed}"
    );
}

#[test]
fn the_note_names_the_checkout_of_a_session_another_checkout_records() {
    let mut review = review();
    let unannotated = |review: &mut Review, checkout: &str| {
        let annotation = review
            .annotations
            .iter_mut()
            .find(|annotation| annotation.session.as_str() == S3)
            .unwrap();
        annotation.checkout = Some(PathBuf::from(checkout));
    };
    let addressing = Addressing {
        checkout: Some(PathBuf::from("../repo")),
        claude_home: Some(PathBuf::from("/scratch/claude")),
        review_dir: Some(PathBuf::from("/scratch/reviews")),
        ..at_worktree()
    };
    let note = |review: &Review| {
        let text = render_summary(review, &addressing);
        section(&text, "Not gate items:")
            .into_iter()
            .find(|line| line.contains("7b1e44d0 not annotated"))
            .unwrap()
            .to_owned()
    };

    // A session the review read from a sibling worktree's record.
    unannotated(&mut review, "/work/repo-b");
    let elsewhere = note(&review);
    unannotated(&mut review, "/work/repo-\u{1b}b");
    let unprintable = render_summary(&review, &addressing);
    // One read from the record of the checkout the review ran in.
    unannotated(&mut review, WORKTREE);
    let here = note(&review);

    // The record's checkout follows the session, in place of the one given.
    assert_eq!(
        elsewhere,
        format!(
            "  session 7b1e44d0 not annotated; in Claude Code run /supersigil-annotate {S3} \
             --checkout=/work/repo-b --claude-home=/scratch/claude --review-dir=/scratch/reviews"
        )
    );
    assert_eq!(
        here,
        format!(
            "  session 7b1e44d0 not annotated; in Claude Code run /supersigil-annotate {S3} \
             --claude-home=/scratch/claude --checkout=../repo --review-dir=/scratch/reviews"
        )
    );
    assert!(
        unprintable.contains(&format!(
            "/supersigil-annotate {S3} --checkout=<as given> --claude-home="
        )),
        "{unprintable}"
    );
    assert!(
        unprintable.ends_with(
            "The --checkout value is not valid UTF-8 or holds a control character, so commands \
             show <as given> for it: pass it as given.\n"
        ),
        "{unprintable}"
    );
}

#[test]
fn the_status_says_how_the_interpretation_changed_since_the_completion() {
    let text = summary(&review());
    let mut code_changed = review();
    let related = &mut code_changed.status.related[0];
    related.relation = Relation::CodeChanged;
    related.changed_paths = Some(vec![PathBytes::new(WRITE)]);
    let code_changed = summary(&code_changed);
    let mut carried = review();
    let related = RelatedInfo {
        interpretation: vec![changed(S1, Some(1), Some(1))],
        ..completed(Vec::new())
    };
    carried.status = StatusInfo {
        complete: true,
        completion: related.completion.clone(),
        source: CompletionSource::Carried(related.review.clone()),
        related: vec![related],
    };
    let mut unchanged = carried.clone();
    unchanged.status.related[0].interpretation.clear();
    let (carried, unchanged) = (summary(&carried), summary(&unchanged));

    let line = "Interpretation changed since completion on 2026-10-15: session 3f2a9c1e revision \
                1 → 2; session 9c0d1e2f newly annotated; session c4d5e6f7 annotation removed.";
    assert_eq!(
        status(&text),
        [format!("Review not complete. {line}")],
        "{text}"
    );
    assert_eq!(
        status(&code_changed),
        [
            "Review not complete. Code changed since your completion on 2026-10-15 (review \
             11111111):",
            "  src/reviews/write.rs",
            line,
        ],
        "{code_changed}"
    );
    // A carried completion stands; revisions are numbered per record.
    assert_eq!(
        status(&carried),
        [
            "Review complete: completed on 2026-10-15 by Joni Savolainen for the same content \
             (review 11111111).",
            "Interpretation changed since completion on 2026-10-15: session 3f2a9c1e revision 1 \
             → 1 in another record.",
        ],
        "{carried}"
    );
    assert_eq!(
        status(&unchanged),
        [
            "Review complete: completed on 2026-10-15 by Joni Savolainen for the same content \
             (review 11111111)."
        ],
        "{unchanged}"
    );
}

#[test]
fn transcript_text_is_escaped_and_cut_to_one_line_and_kept_whole_in_json() {
    let quote = "make it \u{1b}[31mred\u{1b}[0m";
    let long = "x".repeat(300);
    let mut review = review();
    review.decisions = vec![DecisionInfo {
        rationale: vec![
            stated(Speaker::Human, quote, quote),
            stated(Speaker::Agent, &long, &long),
        ],
        ..decision("d1", "Publish\n\u{1b}[2Kforged line")
    }];
    let why = why_of(attributed_line(), review.decisions.clone());

    let text = summary(&review);
    let why_text = render_why(&why);
    let json = serde_json::to_value(&review).unwrap();

    let shown = format!("      agent, explicit: \"{}…\"", "x".repeat(200));
    assert_eq!(
        section(&text, "Decisions:"),
        [
            "  d1  Publish…",
            r#"      human, explicit: "make it \x1b[31mred\x1b[0m""#,
            shown.as_str(),
        ],
        "{text}"
    );
    assert!(
        why_text.contains("\n  decision d1: Publish…\n    human, explicit: \"make it \\x1b[31m"),
        "{why_text}"
    );
    for output in [&text, &why_text] {
        assert!(!output.contains('\u{1b}'), "{output}");
        assert!(!output.contains("forged"), "{output}");
    }
    let decision = &json["decisions"][0];
    assert_eq!(decision["title"], "Publish\n\u{1b}[2Kforged line");
    assert_eq!(decision["rationale"][0]["source"]["quote"], quote);
    assert_eq!(decision["rationale"][1]["text"], long.as_str());
}

/// `why`'s line 1 of `fn greet() {}` under `attr`.
fn line(attr: &PathAttribution) -> WhyLine {
    why_line(
        &AttributionState::Available(attr),
        0,
        &split_lines(b"fn greet() {}\n"),
        Ok(b""),
    )
}

/// A line edit `e1` introduced.
fn attributed_line() -> WhyLine {
    line(&attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1")],
        vec![],
    ))
}

/// A line no chain explains whose text edit `e1` covers, found by a
/// content search that ran out of budget when `incomplete`.
fn matched_line(incomplete: bool) -> WhyLine {
    let mut attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e0"], ChainEnd::Base)],
        vec![unexplained()],
        vec![],
    );
    attr.content.target[0] = BTreeSet::from([eid("e1")]);
    attr.content.incomplete = incomplete;
    line(&attr)
}

/// A line whose whitespace alone edit `w` changed.
fn whitespace_line() -> WhyLine {
    line(&attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["w"], ChainEnd::Base)],
        vec![agreed(
            [Origin::WhitespaceAdded(eid("w"))],
            Some(ChainClass::ExactFromBase),
        )],
        vec![],
    ))
}

fn why_of(line: WhyLine, decisions: Vec<DecisionInfo>) -> Why {
    Why {
        schema: WHY_SCHEMA,
        path: "src/lib.rs".to_owned(),
        target: WhyTarget {
            worktree: "/work/repo".to_owned(),
            tree: "c".repeat(40),
            blob: None,
            attribution_bytes: "identical".to_owned(),
        },
        records: Vec::new(),
        evidence: EvidenceInfo::default(),
        on_disk: OnDiskCheck::Captured,
        line: Some(line),
        edits: BTreeMap::new(),
        conflicting_edits: Vec::new(),
        command_changes: Vec::new(),
        decisions,
    }
}

#[test]
fn why_prints_the_decisions_behind_its_line_after_its_edits() {
    let why = why_of(attributed_line(), vec![sync_plan()]);
    let without = why_of(attributed_line(), Vec::new());

    let text = render_why(&why);
    let json = serde_json::to_value(&why).unwrap();
    let without = serde_json::to_value(&without).unwrap();

    let expected = "  edit e1 (introduced): no details recorded
  decision d1: Publish both stores through one sync plan  [disputed]
    correction (wrong), 2026-10-15, Joni Savolainen: \"the plan exists for crash safety, not \
                    inspection\" (addressed revision 1) (not yet seen by an annotation)
    correction (constraint), 2026-10-16, Joni Savolainen: \"both stores must reach disk before \
                    the manifest\" (seen by revision 2)
    human, explicit, correction: \"both stores must reach disk before the manifest\"
    human, explicit: \"make the sync plan inspectable\"
    agent, explicit: \"Syncing the directory after each file costs a full fsync per file.\"
      as: syncing per file was too slow
    annotator: the author preferred one plan over per-store syncs
    agent, inferred: one plan keeps the stores in step
";
    assert!(text.ends_with(expected), "{text}");
    assert_eq!(json["schema"], "supersigil.why/2");
    assert_eq!(json["decisions"][0]["name"], "d1");
    assert!(without.get("decisions").is_none(), "{without}");
}

#[test]
fn why_says_how_its_line_relates_to_a_decision_it_does_not_simply_attribute() {
    let shown = |line| render_why(&why_of(line, vec![write_plan()]));

    for (text, heading) in [
        (shown(matched_line(false)), "(content match only)"),
        (
            shown(matched_line(true)),
            "(content match only, search incomplete)",
        ),
        (shown(whitespace_line()), "(whitespace only)"),
    ] {
        assert!(
            text.contains(&format!(
                "\n  decision d3 {heading}: Write reviews through the same plan\n    annotator: \
                 reviews are one more store\n"
            )),
            "{text}"
        );
    }
}
