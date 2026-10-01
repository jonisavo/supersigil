//! Snapshots of the review JSON, the terminal summary, and `why`.

mod common;

use std::collections::BTreeMap;

use common::{
    attribution, base_line, chain, eid, escape, introduced, kept, no_fate, replaced, unexplained,
};
use supersigil_anchor::lines::split_lines;
use supersigil_anchor::{ChainClass, ChainEnd, Conflict, PathAttribution, PathStatus, StopReason};
use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, EditOperation, Material, Role,
};
use supersigil_record::{RecordId, SessionId};
use supersigil_review::model::{
    AncestryInfo, BaseInfo, BytesStatus, EditInfo, EvidenceInfo, FileInput, FileKindInfo,
    FileReview, FileStatus, Mention, MentionResult, NotCapturedInfo, OriginsInfo, PromptInfo,
    RecordInfo, RepoPathInfo, ScopeInfo, TargetInfo, TargetKindInfo, TranscriptInfo,
    UnreconciledInfo, UntrackedInfo, file_review, unattributed_summary,
};
use supersigil_review::outcome::AttributionState;
use supersigil_review::summary::render_summary;
use supersigil_review::why::{OnDiskCheck, Tristate, Why, WhyTarget, render_why, why_line};
use supersigil_review::{REVIEW_SCHEMA, Review, WHY_SCHEMA};

const TRANSCRIPT: &str = "/home/u/.claude/projects/-work-repo/s1.jsonl";

fn bytes(base: &str, target: &str) -> BytesStatus {
    BytesStatus {
        base: base.to_owned(),
        target: target.to_owned(),
    }
}

fn records() -> Vec<RecordInfo> {
    vec![
        RecordInfo {
            id: "rec-1".to_owned(),
            revision: Some(4),
            reconciled: true,
            reason: None,
        },
        RecordInfo {
            id: "rec-2".to_owned(),
            revision: Some(2),
            reconciled: false,
            reason: Some("locked by another writer".to_owned()),
        },
    ]
}

fn evidence() -> EvidenceInfo {
    EvidenceInfo {
        candidate_transcripts: vec![TranscriptInfo {
            transcript: TRANSCRIPT.to_owned(),
            session: "s1".to_owned(),
            capture_limitations: vec![CaptureLimitation {
                session: SessionId::new("s1"),
                transcript: TRANSCRIPT.to_owned(),
                from_ordinal: 0,
                to_ordinal: 19,
                counts: CaptureCounts {
                    abandoned_tool_uses: 1,
                    ..CaptureCounts::default()
                },
            }],
            localized: false,
        }],
        unplaced_edits: 0,
        unreconciled_checkouts: vec![UnreconciledInfo {
            checkout: "/work/repo/.claude/worktrees/x".to_owned(),
            transcripts: 2,
            reason: "records directory locked".to_owned(),
        }],
        conflicting_edits: Vec::new(),
    }
}

/// An edit whose sightings in two records disagree.
fn conflict() -> Conflict {
    Conflict {
        edit: eid("e9"),
        records: vec![RecordId::new("rec-1"), RecordId::new("rec-2")],
    }
}

fn edits() -> BTreeMap<String, EditInfo> {
    BTreeMap::from([(
        "e1".to_owned(),
        EditInfo {
            session: "s1".to_owned(),
            transcript: Some(TRANSCRIPT.to_owned()),
            turn: "a1".to_owned(),
            time: "2026-09-28T10:00:01.000Z".to_owned(),
            worktree: "/work/repo".to_owned(),
            operation: EditOperation::Replace,
            prompt: Some(PromptInfo {
                turn: "u1".to_owned(),
                role: Role::Human,
                excerpt: Material::Retained("Add a greeting function and clean up.".to_owned()),
            }),
        },
    )])
}

/// One text file's review; modes and blob ids follow from which sides exist.
fn text_file(
    path: &str,
    status: FileStatus,
    base: Option<&[u8]>,
    target: Option<&[u8]>,
    attr: &PathAttribution,
    mentions: Vec<Mention>,
) -> FileReview {
    let info = RepoPathInfo {
        display: path.to_owned(),
        escaped: None,
    };
    let mode = |side: Option<&[u8]>| if side.is_some() { "100644" } else { "000000" };
    let presence = |side: Option<&[u8]>| {
        if side.is_some() {
            "identical"
        } else {
            "absent"
        }
    };
    file_review(FileInput {
        path: &info,
        status,
        old_mode: mode(base).to_owned(),
        new_mode: mode(target).to_owned(),
        old_blob: base.map(|_| format!("{path}@base")),
        new_blob: target.map(|_| format!("{path}@target")),
        kind: FileKindInfo::Text,
        bytes_status: bytes(presence(base), presence(target)),
        base_blob: base,
        target_blob: target,
        attribution: AttributionState::Available(attr),
        mentions,
        conflicting_edits: Vec::new(),
    })
}

/// A binary file, a file no chain explains, a file deleted by a command, and
/// a file one edit changed exactly from the base, sorted by path.
fn sample_files() -> Vec<FileReview> {
    let logo = RepoPathInfo {
        display: "logo.png".to_owned(),
        escaped: None,
    };
    let binary = file_review(FileInput {
        path: &logo,
        status: FileStatus::Modified,
        old_mode: "100644".to_owned(),
        new_mode: "100644".to_owned(),
        old_blob: Some("logo.png@base".to_owned()),
        new_blob: Some("logo.png@target".to_owned()),
        kind: FileKindInfo::Binary,
        bytes_status: bytes("identical", "identical"),
        base_blob: None,
        target_blob: None,
        attribution: AttributionState::Unavailable {
            reason: "binary".to_owned(),
        },
        mentions: Vec::new(),
        conflicting_edits: vec![conflict()],
    });
    let mut notes = attribution(vec![], vec![unexplained()], vec![no_fate()]);
    notes.status = PathStatus::NotComposed {
        reasons: vec![StopReason::NoAcceptedCandidate { edit: eid("e2") }],
    };
    let old = attribution(vec![], vec![], vec![no_fate()]);
    let rm = Mention {
        command: "command:1".to_owned(),
        session: "s1".to_owned(),
        turn: "a6".to_owned(),
        checkout: "/work/repo".to_owned(),
        text: "rm old.txt".to_owned(),
        result: Some(MentionResult {
            exit: Some(0),
            outcome: None,
        }),
    };
    let lib = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![base_line(0), introduced("e1")],
        vec![kept(0), replaced("e1")],
    );
    vec![
        binary,
        text_file(
            "notes.txt",
            FileStatus::Modified,
            Some(b"draft\n"),
            Some(b"final\n"),
            &notes,
            Vec::new(),
        ),
        text_file(
            "old.txt",
            FileStatus::Deleted,
            Some(b"gone\n"),
            None,
            &old,
            vec![rm],
        ),
        text_file(
            "src/lib.rs",
            FileStatus::Modified,
            Some(b"fn a() {}\nfn old() {}\n"),
            Some(b"fn a() {}\nfn new() {}\n"),
            &lib,
            Vec::new(),
        ),
    ]
}

fn sample_scope() -> ScopeInfo {
    ScopeInfo {
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
    }
}

fn sample_review() -> Review {
    let files = sample_files();
    let scope = sample_scope();
    Review {
        schema: REVIEW_SCHEMA,
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
        unattributed: unattributed_summary(&files, &scope),
        scope,
        origins: OriginsInfo::default(),
        records: records(),
        evidence: EvidenceInfo {
            conflicting_edits: vec![conflict()],
            ..evidence()
        },
        files,
        edits: edits(),
    }
}

#[test]
fn review_json_shape_is_stable() {
    insta::assert_yaml_snapshot!(sample_review());
}

#[test]
fn terminal_summary_is_stable() {
    insta::assert_snapshot!(render_summary(&sample_review(), escape));
}

#[test]
fn terminal_summary_escapes_untrusted_text() {
    let mut review = sample_review();
    review.files[3].path = "src/\u{1b}[2Klib.rs".to_owned();
    let text = render_summary(&review, escape);
    assert!(text.contains(r"src/\x1b[2Klib.rs"));
    assert!(!text.contains('\u{1b}'));
}

fn sample_why(on_disk: OnDiskCheck) -> Why {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1"), unexplained()],
        vec![],
    );
    let target = b"fn greet() {}\nfn other() {}\n";
    let line = match on_disk {
        OnDiskCheck::Captured => Some(why_line(
            &AttributionState::Available(&attr),
            0,
            &split_lines(target),
            &[],
            Tristate::Yes,
            None,
        )),
        OnDiskCheck::NotCaptured { .. } => None,
    };
    Why {
        schema: WHY_SCHEMA,
        path: "src/lib.rs".to_owned(),
        target: WhyTarget {
            worktree: "/work/repo".to_owned(),
            tree: "c".repeat(40),
            blob: Some("src/lib.rs@target".to_owned()),
            attribution_bytes: "identical".to_owned(),
        },
        records: records(),
        evidence: evidence(),
        on_disk,
        line,
        edits: edits(),
        conflicting_edits: Vec::new(),
    }
}

#[test]
fn why_json_and_terminal_are_stable() {
    let why = sample_why(OnDiskCheck::Captured);
    insta::assert_yaml_snapshot!(why);
    insta::assert_snapshot!(render_why(&why, escape));
}

#[test]
fn terminal_summary_shows_the_path_note() {
    let mut review = sample_review();
    review.scope.path_note = Some(
        "a path selector named the worktree root, so the review has no path filter".to_owned(),
    );
    let text = render_summary(&review, escape);
    assert!(text.contains(
        "\nscope: a path selector named the worktree root, so the review has no path filter\n"
    ));
}

#[test]
fn why_says_why_differs_from_head_is_unknown() {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1")],
        vec![],
    );
    let line = why_line(
        &AttributionState::Available(&attr),
        0,
        &split_lines(b"fn greet() {}\n"),
        &[],
        Tristate::Unknown,
        Some("coarse diff".to_owned()),
    );
    assert_eq!(line.differs_from_head, Tristate::Unknown);
    assert_eq!(
        line.differs_from_head_reason.as_deref(),
        Some("coarse diff")
    );
    let mut why = sample_why(OnDiskCheck::Captured);
    why.line = Some(line);
    assert!(render_why(&why, escape).contains("\n  differs from HEAD: unknown (coarse diff)\n"));
}

#[test]
fn why_not_captured_keeps_the_evidence_context() {
    let why = sample_why(OnDiskCheck::NotCaptured {
        reason: "assume-unchanged".to_owned(),
    });
    assert_eq!(why.line, None);
    let text = render_why(&why, escape);
    assert!(text.starts_with("src/lib.rs: not captured: assume-unchanged"));
    assert!(text.contains("record rec-2 not reconciled: locked by another writer"));
    assert!(text.contains("capture limitations in"));
    // No line was explained, so no edit is shown as contributing to one.
    assert!(!text.contains("edit e1"), "{text}");
}

/// `e1`'s details under another id and time.
fn other_edit(time: &str) -> EditInfo {
    EditInfo {
        time: time.to_owned(),
        ..edits()["e1"].clone()
    }
}

#[test]
fn why_prints_only_the_lines_contributors() {
    let mut why = sample_why(OnDiskCheck::Captured);
    why.edits
        .insert("e2".to_owned(), other_edit("2026-09-28T10:00:02.000Z"));
    let text = render_why(&why, escape);
    assert!(text.contains("\n  edit e1 at "), "{text}");
    assert!(!text.contains("edit e2"), "{text}");
}

#[test]
fn why_prints_no_contributors_for_an_unattributed_line() {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1"), unexplained()],
        vec![],
    );
    let mut why = sample_why(OnDiskCheck::Captured);
    why.line = Some(why_line(
        &AttributionState::Available(&attr),
        1,
        &split_lines(b"fn greet() {}\nfn other() {}\n"),
        &[],
        Tristate::Yes,
        None,
    ));
    let text = render_why(&why, escape);
    assert!(text.contains("not attributed"), "{text}");
    assert!(!text.contains("  edit "), "{text}");
}

#[test]
fn why_line_refers_to_its_chains_and_its_outcome() {
    let mut attr = attribution(
        vec![chain(
            0,
            ChainClass::ExactFromStart,
            &["e1"],
            ChainEnd::Stopped {
                reasons: vec![StopReason::AfterHashMismatch { edit: eid("e4") }],
            },
        )],
        vec![introduced("e1"), unexplained()],
        vec![],
    );
    attr.content.target[1] = std::collections::BTreeSet::from([eid("e5")]);
    let lines = split_lines(b"fn greet() {}\nfn other() {}\n");
    let line = |index| {
        why_line(
            &AttributionState::Available(&attr),
            index,
            &lines,
            &[],
            Tristate::Yes,
            None,
        )
    };
    let ids = |names: &[&str]| names.iter().map(|n| eid(n)).collect();
    assert_eq!(line(0).referenced_edits(), ids(&["e1", "e4"]));
    assert_eq!(line(1).referenced_edits(), ids(&["e1", "e4", "e5"]));
}

#[test]
fn terminal_summary_keeps_search_incomplete_lines_unattributed() {
    let mut attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![base_line(0), introduced("e1")],
        vec![kept(0), replaced("e1")],
    );
    attr.status = PathStatus::SearchIncomplete;
    let mut review = sample_review();
    review.files = vec![text_file(
        "src/lib.rs",
        FileStatus::Modified,
        Some(b"fn a() {}\nfn old() {}\n"),
        Some(b"fn a() {}\nfn new() {}\n"),
        &attr,
        Vec::new(),
    )];
    review.unattributed = unattributed_summary(&review.files, &review.scope);
    let text = render_summary(&review, escape);
    assert!(
        text.contains("\n  src/lib.rs: 2 unresolved (search incomplete)\n"),
        "{text}"
    );
    assert!(
        text.contains("\n  M src/lib.rs  +1 -1  2 unresolved (search incomplete)\n"),
        "{text}"
    );
    let files = text.split("\nfiles:\n").nth(1).unwrap();
    for word in ["exact", "introduced", "replaced", "session"] {
        assert!(!files.contains(word), "{word} in {files}");
    }
}
