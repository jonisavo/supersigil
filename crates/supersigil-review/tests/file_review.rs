//! Assembling one file's review: hunks, spans, and the unattributed summary.

mod common;

use common::{
    attribution, base_line, chain, eid, finishes_within, introduced, kept, no_fate, replaced,
    unexplained,
};
use std::collections::BTreeSet;
use std::time::Duration;

use supersigil_anchor::{ChainClass, ChainEnd, Conflict, PathStatus, StopReason};
use supersigil_record::RecordId;
use supersigil_review::model::{
    BytesStatus, FileInput, FileKindInfo, FileStatus, Mention, NotCapturedInfo, RepoPathInfo,
    ScopeInfo, SpanSide, UntrackedInfo, file_review, unattributed_summary,
};
use supersigil_review::outcome::{AttributionState, Outcome, UnattributedReason};

fn path(display: &str) -> RepoPathInfo {
    RepoPathInfo {
        display: display.to_owned(),
        escaped: None,
    }
}

fn identical() -> BytesStatus {
    BytesStatus {
        base: "identical".to_owned(),
        target: "identical".to_owned(),
    }
}

fn input<'a>(
    path: &'a RepoPathInfo,
    status: FileStatus,
    base: Option<&'a [u8]>,
    target: Option<&'a [u8]>,
    attribution: AttributionState<'a>,
) -> FileInput<'a> {
    FileInput {
        path,
        status,
        old_mode: if base.is_some() { "100644" } else { "000000" }.to_owned(),
        new_mode: if target.is_some() { "100644" } else { "000000" }.to_owned(),
        old_blob: base.map(|_| "b1".to_owned()),
        new_blob: target.map(|_| "b2".to_owned()),
        kind: FileKindInfo::Text,
        bytes_status: identical(),
        base_blob: base,
        target_blob: target,
        attribution,
        mentions: Vec::new(),
        conflicting_edits: Vec::new(),
    }
}

#[test]
fn modified_file_groups_lines_into_spans_removed_first() {
    // Base:   keep, old1, old2        Target: keep, new1, new2, extra
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![
            base_line(0),
            introduced("e1"),
            introduced("e1"),
            unexplained(),
        ],
        vec![kept(0), replaced("e1"), replaced("e1")],
    );
    let path = path("src/lib.rs");
    let review = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"keep\nold1\nold2\n"),
        Some(b"keep\nnew1\nnew2\nextra\n"),
        AttributionState::Available(&attr),
    ));
    assert_eq!(review.hunks.len(), 1);
    let hunk = &review.hunks[0];
    assert_eq!(
        (
            hunk.base_start,
            hunk.base_count,
            hunk.target_start,
            hunk.target_count
        ),
        (2, 2, 2, 3)
    );
    assert_eq!(hunk.removed, vec!["old1\n", "old2\n"]);
    assert_eq!(hunk.added, vec!["new1\n", "new2\n", "extra\n"]);
    let shape: Vec<(SpanSide, usize, usize)> = hunk
        .spans
        .iter()
        .map(|span| (span.side, span.start, span.count))
        .collect();
    assert_eq!(
        shape,
        vec![
            (SpanSide::Base, 2, 2),
            (SpanSide::Target, 2, 2),
            (SpanSide::Target, 4, 1),
        ]
    );
    assert!(matches!(hunk.spans[0].outcome, Outcome::Attributed { .. }));
    assert_eq!(hunk.spans[1].provenance.len(), 2);
    assert_eq!(
        hunk.spans[2].outcome,
        Outcome::Unattributed {
            reason: UnattributedReason::GapBeforeChain
        }
    );
    let info = review.attribution.unwrap();
    assert_eq!(info.chains.len(), 1);
    assert_eq!(info.unavailable, None);
}

#[test]
fn hunk_starts_follow_unified_numbering_for_empty_ranges() {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![base_line(0), introduced("e1"), base_line(1)],
        vec![kept(0), kept(2)],
    );
    let path = path("f.txt");
    let review = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"a\nc\n"),
        Some(b"a\nb\nc\n"),
        AttributionState::Available(&attr),
    ));
    let hunk = &review.hunks[0];
    // An insertion after base line 1: the base range is empty and names the
    // line before it; the target range starts at line 2.
    assert_eq!(
        (
            hunk.base_start,
            hunk.base_count,
            hunk.target_start,
            hunk.target_count
        ),
        (1, 0, 2, 1)
    );
    assert_eq!(hunk.spans.len(), 1);
    assert_eq!(hunk.spans[0].side, SpanSide::Target);
}

#[test]
fn hunk_lines_keep_their_terminators() {
    let attr = attribution(vec![], vec![unexplained()], vec![no_fate()]);
    let path = path("f.txt");
    let review = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"x\n"),
        Some(b"x"),
        AttributionState::Available(&attr),
    ));
    let hunk = &review.hunks[0];
    assert_eq!(hunk.removed, vec!["x\n"]);
    assert_eq!(hunk.added, vec!["x"]);
}

#[test]
fn added_and_deleted_files_have_spans_on_one_side() {
    let attr = attribution(vec![], vec![unexplained(), unexplained()], vec![]);
    let added_path = path("src/new.rs");
    let added = file_review(input(
        &added_path,
        FileStatus::Added,
        None,
        Some(b"a\nb\n"),
        AttributionState::Available(&attr),
    ));
    assert_eq!(added.old_mode, "000000");
    let hunk = &added.hunks[0];
    assert_eq!((hunk.base_start, hunk.base_count), (0, 0));
    assert_eq!(hunk.spans.len(), 1);
    assert_eq!(hunk.spans[0].side, SpanSide::Target);

    let gone = attribution(vec![], vec![], vec![no_fate()]);
    let deleted_path = path("old.txt");
    let mut deleted_input = input(
        &deleted_path,
        FileStatus::Deleted,
        Some(b"gone\n"),
        None,
        AttributionState::Available(&gone),
    );
    deleted_input.mentions = vec![Mention {
        command: "command:1".to_owned(),
        session: "s1".to_owned(),
        turn: "a6".to_owned(),
        checkout: "/work/repo".to_owned(),
        text: "rm old.txt".to_owned(),
        result: None,
    }];
    let deleted = file_review(deleted_input);
    let hunk = &deleted.hunks[0];
    assert_eq!((hunk.target_start, hunk.target_count), (0, 0));
    assert_eq!(hunk.spans[0].side, SpanSide::Base);
    assert_eq!(deleted.mentions.len(), 1);
}

#[test]
fn non_text_file_has_no_hunks_or_attribution() {
    let path = path("logo.png");
    let mut binary = input(
        &path,
        FileStatus::Modified,
        Some(b"\x89PNG\0"),
        Some(b"\x89PNG\0\0"),
        AttributionState::Unavailable {
            reason: "binary".to_owned(),
        },
    );
    binary.kind = FileKindInfo::Binary;
    let review = file_review(binary);
    assert!(review.hunks.is_empty());
    assert_eq!(review.attribution, None);
}

#[test]
fn unavailable_attribution_marks_every_line_and_the_file() {
    let path = path("big.lfs");
    let review = file_review(input(
        &path,
        FileStatus::Modified,
        Some(b"a\n"),
        Some(b"b\n"),
        AttributionState::Unavailable {
            reason: "conversion changed line structure".to_owned(),
        },
    ));
    let info = review.attribution.as_ref().unwrap();
    assert_eq!(info.status, None);
    assert_eq!(
        info.unavailable.as_deref(),
        Some("conversion changed line structure")
    );
    for span in &review.hunks[0].spans {
        assert!(matches!(
            span.outcome,
            Outcome::Unattributed {
                reason: UnattributedReason::AttributionUnavailable { .. }
            }
        ));
    }
}

#[test]
fn many_rejected_heads_over_many_changed_lines_are_grouped_promptly() {
    // 10,000 heads rejected for an unknown operation, and 100,000 changed
    // lines on each side that no chain explains. The path's status is the
    // same on every line, so grouping them must not copy or compare its
    // 10,000 reasons once per line (two billion copies).
    let mut attr = attribution(Vec::new(), Vec::new(), Vec::new());
    attr.status = PathStatus::NotComposed {
        reasons: (0..10_000)
            .map(|i| StopReason::OperationUnknown {
                edit: eid(&format!("e{i}")),
            })
            .collect(),
    };
    let base = "b\n".repeat(100_000);
    let target = "a\n".repeat(100_000);
    let review = finishes_within(Duration::from_secs(20), move || {
        let path = path("big.txt");
        file_review(input(
            &path,
            FileStatus::Modified,
            Some(base.as_bytes()),
            Some(target.as_bytes()),
            AttributionState::Available(&attr),
        ))
    });
    assert!(review.coarse);
    let spans = &review.hunks[0].spans;
    assert_eq!(
        spans.iter().map(|s| (s.side, s.count)).collect::<Vec<_>>(),
        [(SpanSide::Base, 100_000), (SpanSide::Target, 100_000)]
    );
    for span in spans {
        assert!(
            matches!(
                &span.outcome,
                Outcome::Unattributed {
                    reason: UnattributedReason::NoSurvivingChain {
                        status: PathStatus::NotComposed { reasons }
                    }
                } if reasons.len() == 10_000
            ),
            "{:?}",
            span.side
        );
    }
}

/// `input` with `conflict` listed on it.
fn with_conflict<'a>(input: FileInput<'a>, conflict: &Conflict) -> FileInput<'a> {
    FileInput {
        conflicting_edits: vec![conflict.clone()],
        ..input
    }
}

#[test]
fn conflicting_edits_are_listed_on_the_file_whatever_its_attribution() {
    let conflict = Conflict {
        edit: eid("e9"),
        records: vec![RecordId::new("r1"), RecordId::new("r2")],
    };
    let attr = attribution(vec![], vec![unexplained()], vec![no_fate()]);
    let path = path("src/lib.rs");
    let attributed = file_review(with_conflict(
        input(
            &path,
            FileStatus::Modified,
            Some(b"a\n"),
            Some(b"b\n"),
            AttributionState::Available(&attr),
        ),
        &conflict,
    ));
    let unavailable = file_review(with_conflict(
        input(
            &path,
            FileStatus::Modified,
            Some(b"a\n"),
            Some(b"b\n"),
            AttributionState::Unavailable {
                reason: "conversion changed line structure".to_owned(),
            },
        ),
        &conflict,
    ));
    let binary = file_review(with_conflict(
        FileInput {
            kind: FileKindInfo::Binary,
            base_blob: None,
            target_blob: None,
            ..input(
                &path,
                FileStatus::Modified,
                Some(b"a\n"),
                Some(b"b\n"),
                AttributionState::Unavailable {
                    reason: "not diffed: binary".to_owned(),
                },
            )
        },
        &conflict,
    ));
    assert!(binary.attribution.is_none());
    for review in [attributed, unavailable, binary] {
        assert_eq!(review.conflicting_edits, vec![conflict.clone()]);
    }
}

#[test]
fn unattributed_summary_counts_incomplete_content_matching_apart() {
    let mut attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1"), unexplained(), unexplained()],
        vec![replaced("e1")],
    );
    attr.content.incomplete = true;
    let lib = path("src/lib.rs");
    let reviewed = file_review(input(
        &lib,
        FileStatus::Modified,
        Some(b"old\n"),
        Some(b"new\nmore\nmore2\n"),
        AttributionState::Available(&attr),
    ));
    let summary = unattributed_summary(&[reviewed], &ScopeInfo::default());
    assert_eq!(summary.files[0].unattributed, 0);
    assert_eq!(summary.files[0].content_match_incomplete, 2);
}

#[test]
fn unattributed_summary_counts_lines_and_lists_scope_items() {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1"), unexplained(), unexplained()],
        vec![replaced("e1")],
    );
    let lib = path("src/lib.rs");
    let reviewed = file_review(input(
        &lib,
        FileStatus::Modified,
        Some(b"old\n"),
        Some(b"new\nmore\nmore2\n"),
        AttributionState::Available(&attr),
    ));
    let png = path("logo.png");
    let mut binary_input = input(
        &png,
        FileStatus::Added,
        None,
        Some(b"\0"),
        AttributionState::Unavailable {
            reason: "binary".to_owned(),
        },
    );
    binary_input.kind = FileKindInfo::Binary;
    let binary = file_review(binary_input);
    let scope = ScopeInfo {
        untracked_excluded: vec![
            UntrackedInfo {
                path: "src/new.rs".to_owned(),
                recorded_edit: true,
                include_flag: "--include-untracked src/new.rs".to_owned(),
            },
            UntrackedInfo {
                path: "scratch.txt".to_owned(),
                recorded_edit: false,
                include_flag: "--include-untracked scratch.txt".to_owned(),
            },
        ],
        not_captured: vec![NotCapturedInfo {
            path: "config.toml".to_owned(),
            on_disk: "present".to_owned(),
            cause: "assume_unchanged".to_owned(),
        }],
        ..ScopeInfo::default()
    };
    let summary = unattributed_summary(&[reviewed, binary], &scope);
    assert_eq!(summary.files.len(), 1);
    assert_eq!(summary.files[0].path, "src/lib.rs");
    assert_eq!(summary.files[0].unattributed, 2);
    assert_eq!(summary.not_diffed.len(), 1);
    assert_eq!(summary.not_diffed[0].kind, "binary");
    assert_eq!(summary.untracked_with_recorded_edits, vec!["src/new.rs"]);
    assert_eq!(summary.not_captured, vec!["config.toml"]);
    assert!(summary.coarse.is_empty());
}

#[test]
fn search_incomplete_keeps_every_line_unresolved_and_counted() {
    let mut attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1"), unexplained()],
        vec![replaced("e1")],
    );
    attr.status = PathStatus::SearchIncomplete;
    let lib = path("src/lib.rs");
    let reviewed = file_review(input(
        &lib,
        FileStatus::Modified,
        Some(b"old\n"),
        Some(b"new\nmore\n"),
        AttributionState::Available(&attr),
    ));
    let spans: Vec<_> = reviewed.hunks.iter().flat_map(|h| &h.spans).collect();
    // Each line has its own candidate readings, so no two share a span.
    assert_eq!(spans.len(), 3);
    assert!(
        spans
            .iter()
            .all(|span| matches!(span.outcome, Outcome::Unresolved { .. }))
    );
    assert_eq!(
        reviewed.attribution.as_ref().unwrap().status,
        Some(PathStatus::SearchIncomplete)
    );
    let summary = unattributed_summary(&[reviewed], &ScopeInfo::default());
    assert_eq!(summary.files[0].unresolved, 3);
    assert_eq!(summary.files[0].unattributed, 0);
}

#[test]
fn referenced_edits_are_every_edit_the_file_names() {
    let stopped = |edit: &str| ChainEnd::Stopped {
        reasons: vec![StopReason::AfterHashMismatch { edit: eid(edit) }],
    };
    let mut attr = attribution(
        vec![chain(
            0,
            ChainClass::ExactFromStart,
            &["e1", "e2"],
            stopped("e4"),
        )],
        vec![introduced("e1"), unexplained(), introduced("e2")],
        vec![replaced("e1")],
    );
    attr.set_aside = vec![chain(1, ChainClass::Consistent, &["e3"], ChainEnd::Base)];
    attr.content.target[1] = BTreeSet::from([eid("e5")]);
    let lib = path("src/lib.rs");
    let review = file_review(input(
        &lib,
        FileStatus::Modified,
        Some(b"x\n"),
        Some(b"a\nb\nc\n"),
        AttributionState::Available(&attr),
    ));
    // Chains and set-aside chains, the edit a chain stopped at, and the edit
    // a content match names; never an edit nothing names.
    let expected: BTreeSet<_> = ["e1", "e2", "e3", "e4", "e5"].map(eid).into();
    assert_eq!(review.referenced_edits(), expected);

    let mut rejected = attribution(vec![], vec![unexplained()], vec![no_fate()]);
    rejected.status = PathStatus::NotComposed {
        reasons: vec![StopReason::NotLocatable { edit: eid("e7") }],
    };
    let review = file_review(input(
        &lib,
        FileStatus::Modified,
        Some(b"x\n"),
        Some(b"a\n"),
        AttributionState::Available(&rejected),
    ));
    assert_eq!(review.referenced_edits(), BTreeSet::from([eid("e7")]));

    let unavailable = file_review(input(
        &lib,
        FileStatus::Modified,
        Some(b"x\n"),
        Some(b"a\n"),
        AttributionState::Unavailable {
            reason: "conversion changed line structure".to_owned(),
        },
    ));
    assert!(unavailable.referenced_edits().is_empty());
}
