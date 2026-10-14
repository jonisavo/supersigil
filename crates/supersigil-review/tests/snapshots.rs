//! Snapshots of the review JSON, the terminal summary, and `why`.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::slice;
use std::time::Duration;

use common::{
    attribution, base_line, chain, eid, escape, finishes_within, introduced, kept, no_fate,
    replaced, unexplained,
};
use serde_json::{Value, json};
use supersigil_anchor::lines::split_lines;
use supersigil_anchor::{
    ChainClass, ChainEnd, Conflict, LineOutcome, Origin, PathAttribution, PathStatus, Provenance,
    Reading, StopReason,
};
use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, ChangeKind, EditOperation, Hunk, Material, Role,
};
use supersigil_record::reviews::{
    Algorithms, BASIS_SCHEMA, Basis, BasisBase, BasisId, BasisScope, BasisTarget, GateItem,
    Judgment, JudgmentKind, PathBytes, TargetKind, Who,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, RecordId, SessionId};
use supersigil_review::gate::{GateInputs, gate_items};
use supersigil_review::judging::{GateStates, Ledger, Relation};
use supersigil_review::model::{
    AncestryInfo, BaseInfo, BytesStatus, ChangedEvidence, CommandChange, EditInfo, EditOriginInfo,
    EvidenceInfo, FileInput, FileKindInfo, FileReview, FileStatus, GateInfo, JudgmentInfo, Mention,
    MentionResult, NewObservations, NotCapturedInfo, OriginsInfo, PromptInfo, RecordInfo,
    RelatedInfo, RepoPathInfo, SavedBasis, ScopeInfo, StatusInfo, TargetInfo, TargetKindInfo,
    TranscriptInfo, UncoveredInfo, UnreconciledInfo, Unsaved, UntrackedInfo, file_review,
    unattributed_summary,
};
use supersigil_review::names::ItemNames;
use supersigil_review::outcome::AttributionState;
use supersigil_review::summary::{Addressing, next_lines, render_summary, shell_word};
use supersigil_review::why::{
    OnDiskCheck, Tristate, Why, WhyTarget, line_command_changes, render_why, why_line,
};
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
        removed_transcripts: Vec::new(),
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
            origin: EditOriginInfo::Tool,
            prompt: Some(PromptInfo {
                turn: "u1".to_owned(),
                role: Role::Human,
                excerpt: Material::Retained("Add a greeting function and clean up.".to_owned()),
            }),
        },
    )])
}

/// A command the harness reported as changing a file, issued at `time`,
/// with the hunks it reported or why there are none.
fn command_change(id: &str, time: &str, text: &str, patch: Material<Vec<Hunk>>) -> CommandChange {
    CommandChange {
        command: id.to_owned(),
        session: "s1".to_owned(),
        turn: "a9".to_owned(),
        transcript: Some(TRANSCRIPT.to_owned()),
        time: time.to_owned(),
        checkout: "/work/repo".to_owned(),
        worktree: "/work/repo".to_owned(),
        text: text.to_owned(),
        kind: ChangeKind::Modified,
        patch,
        unlisted: 0,
        outside: 0,
        flags: BTreeSet::new(),
    }
}

/// One hunk at line 1 holding `lines`, as the harness reports them.
fn hunks(lines: &[&str]) -> Material<Vec<Hunk>> {
    Material::Retained(vec![Hunk {
        old_start: 1,
        old_lines: 1,
        new_start: 1,
        new_lines: 1,
        lines: lines.iter().map(|line| (*line).to_owned()).collect(),
    }])
}

/// A formatter run that rewrote the first line of `notes.txt`.
fn formatter_run() -> CommandChange {
    command_change(
        "command:2",
        "2026-09-28T10:00:09.000Z",
        "cargo fmt --all",
        hunks(&["-draft", "+final"]),
    )
}

/// One text file's review; modes and blob ids follow from which sides exist.
fn text_file(
    path: &str,
    status: FileStatus,
    base: Option<&[u8]>,
    target: Option<&[u8]>,
    attr: &PathAttribution,
    mentions: Vec<Mention>,
    command_changes: Vec<CommandChange>,
) -> FileReview {
    let info = RepoPathInfo {
        display: path.to_owned(),
        escaped: None,
        bytes: PathBytes::new(path),
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
        command_changes,
        conflicting_edits: Vec::new(),
    })
}

/// A binary file, a file no chain explains, a file deleted by a command, and
/// a file one edit changed exactly from the base, sorted by path.
fn sample_files() -> Vec<FileReview> {
    let logo = RepoPathInfo {
        display: "logo.png".to_owned(),
        escaped: None,
        bytes: PathBytes::new("logo.png"),
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
        command_changes: Vec::new(),
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
            vec![formatter_run()],
        ),
        text_file(
            "old.txt",
            FileStatus::Deleted,
            Some(b"gone\n"),
            None,
            &old,
            vec![rm],
            Vec::new(),
        ),
        text_file(
            "src/lib.rs",
            FileStatus::Modified,
            Some(b"fn a() {}\nfn old() {}\n"),
            Some(b"fn a() {}\nfn new() {}\n"),
            &lib,
            Vec::new(),
            Vec::new(),
        ),
    ]
}

fn sample_scope() -> ScopeInfo {
    ScopeInfo {
        untracked_excluded: vec![
            UntrackedInfo {
                path: "src/new.rs".to_owned(),
                path_escaped: None,
                path_bytes: PathBytes::new("src/new.rs"),
                recorded_edit: true,
                include_flag: "--include-untracked src/new.rs".to_owned(),
            },
            UntrackedInfo {
                path: "scratch.txt".to_owned(),
                path_escaped: None,
                path_bytes: PathBytes::new("scratch.txt"),
                recorded_edit: false,
                include_flag: "--include-untracked scratch.txt".to_owned(),
            },
        ],
        not_captured: vec![NotCapturedInfo {
            path: "config.toml".to_owned(),
            path_escaped: None,
            path_bytes: PathBytes::new("config.toml"),
            on_disk: "present".to_owned(),
            cause: "assume_unchanged".to_owned(),
        }],
        ..ScopeInfo::default()
    }
}

/// The sample review's saved basis, with nothing in its lists.
fn sample_basis() -> SavedBasis {
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
        gate: Vec::new(),
        salt: None,
    };
    SavedBasis {
        id: document.basis_id().unwrap(),
        document,
    }
}

/// The sample review, with its gate open: no judgment recorded.
fn sample_review() -> Review {
    let files = sample_files();
    let scope = sample_scope();
    let review = Review {
        schema: REVIEW_SCHEMA,
        basis: Some(sample_basis()),
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
        status: StatusInfo::default(),
        gate: GateInfo::default(),
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
    };
    gated(review, |_, _| Vec::new())
}

/// `review` with the gate a basis of it holds, each item's state and the
/// review's status as given by the judgments `judge` makes from the
/// basis's ID and gate, and that basis as its saved one when it has one.
/// Items are named by their paths.
fn gated(mut review: Review, judge: impl FnOnce(&BasisId, &[GateItem]) -> Vec<Judgment>) -> Review {
    let mut document = sample_basis().document;
    document.gate = gate_items(&review, &GateInputs::default());
    let id = document.basis_id().unwrap();
    (review.status, review.gate) = {
        let judgments = judge(&id, &document.gate);
        let ledger = Ledger::new(&judgments);
        let states = GateStates::new(&id, &document, &[], &ledger);
        let associations = BTreeMap::new();
        let names = ItemNames::new(&document.gate, &associations, false, |path| {
            path.to_str().map(str::to_owned)
        });
        (StatusInfo::new(&states), GateInfo::new(&states, &names))
    };
    if review.basis.is_some() {
        review.basis = Some(SavedBasis { id, document });
    }
    review
}

/// A judgment of `kind` on the review `basis` naming `items`, recorded on
/// 2026-10-13.
fn judgment(id: &str, basis: &BasisId, kind: JudgmentKind, items: &[GateItem]) -> Judgment {
    Judgment {
        id: JudgmentId::new(id),
        basis: basis.clone(),
        kind,
        items: items.iter().map(|item| item.id.clone()).collect(),
        reason: (kind == JudgmentKind::Dismiss).then(|| "generated".to_owned()),
        text: None,
        closes: BTreeMap::new(),
        withdraws: Vec::new(),
        who: Who {
            name: "Joni Savolainen".to_owned(),
            email: "joni@example.com".to_owned(),
        },
        at: "2026-10-13T12:00:00Z".parse::<UtcTime>().unwrap(),
    }
}

/// The sample review with its last gate item dismissed and every other
/// acknowledged.
fn closed_review() -> Review {
    gated(sample_review(), |id, gate| {
        let (last, rest) = gate.split_last().unwrap();
        vec![
            judgment("ack", id, JudgmentKind::Acknowledge, rest),
            judgment("dismiss", id, JudgmentKind::Dismiss, slice::from_ref(last)),
        ]
    })
}

/// The state and what follows it on `name`'s gate row in a terminal
/// summary.
fn row<'t>(text: &'t str, name: &str) -> (&'t str, &'t str) {
    let rest = text
        .lines()
        .find_map(|line| {
            line.strip_prefix("  ")?
                .strip_prefix(name)?
                .strip_prefix(' ')
        })
        .unwrap_or_else(|| panic!("no gate row for {name} in {text}"))
        .trim_start();
    let (state, after) = rest.split_once("  ").unwrap_or((rest, ""));
    (state, after.trim_start())
}

/// A summary with the default addressing: the review is the selection.
fn summary(review: &Review) -> String {
    render_summary(review, &Addressing::default(), escape)
}

#[test]
fn review_json_shape_is_stable() {
    insta::assert_yaml_snapshot!(sample_review());
}

#[test]
fn terminal_summary_is_stable() {
    insta::assert_snapshot!(summary(&sample_review()));
}

#[test]
fn a_closed_gate_says_who_closed_each_item() {
    let review = closed_review();

    let text = summary(&review);

    assert!(
        text.contains("\nReview not complete.\nGate: 0 open, 7 closed.\n\n"),
        "{text}"
    );
    assert_eq!(
        row(&text, "notes.txt"),
        ("acknowledged", "by Joni Savolainen on 2026-10-13")
    );
    assert_eq!(
        row(&text, "unreconciled:/work/repo/.claude/worktrees/x"),
        ("dismissed", "by Joni Savolainen on 2026-10-13")
    );
    assert!(
        text.ends_with("\n\nNext: supersigil judge complete\n"),
        "{text}"
    );
}

#[test]
fn a_complete_review_says_when_and_by_whom_and_needs_nothing_next() {
    let review = gated(sample_review(), |id, gate| {
        let (last, rest) = gate.split_last().unwrap();
        vec![
            judgment("ack", id, JudgmentKind::Acknowledge, rest),
            judgment("dismiss", id, JudgmentKind::Dismiss, slice::from_ref(last)),
            judgment("done", id, JudgmentKind::Complete, &[]),
        ]
    });

    let text = summary(&review);

    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[4..6],
        [
            "Review complete: completed on 2026-10-13 by Joni Savolainen.",
            "Gate: 0 open, 7 closed.",
        ],
        "{text}"
    );
    assert!(!text.contains("Next:"), "{text}");
    assert!(!text.contains("supersigil judge"), "{text}");
}

#[test]
fn a_review_not_saved_says_why_and_how_to_save_it() {
    let mut review = sample_review();
    review.basis = None;
    review.basis_unsaved = Some(Unsaved::Busy);

    let json = serde_json::to_value(&review).unwrap();
    assert_eq!(json["basis"], Value::Null);
    assert_eq!(json["basis_unsaved"], "the review store is busy");
    let text = summary(&review);
    assert_eq!(
        text.lines().next(),
        Some(
            "Review not saved: the review store is busy. Judging needs a saved review; run \
             supersigil review again."
        ),
        "{text}"
    );
    // The gate still shows; judging it waits for a saved review.
    assert_eq!(row(&text, "notes.txt").0, "open");
    assert!(text.ends_with("\n\nNext: supersigil review\n"), "{text}");
    assert!(!text.contains("supersigil judge"), "{text}");
    // A saved review has no reason.
    let saved = serde_json::to_value(sample_review()).unwrap();
    assert_eq!(
        saved["basis"]["id"],
        json!(sample_review().basis.unwrap().id)
    );
    assert!(saved.get("basis_unsaved").is_none(), "{saved}");
}

#[test]
fn a_review_that_can_never_be_saved_does_not_ask_to_run_it_again() {
    let mut review = sample_review();
    review.basis = None;
    review.basis_unsaved = Some(Unsaved::PathNotUtf8);

    let json = serde_json::to_value(&review).unwrap();
    let text = summary(&review);

    let reason = "it names a path that is not valid UTF-8, which the review store cannot hold";
    assert_eq!(json["basis_unsaved"], reason);
    assert_eq!(
        text.lines().next(),
        Some(format!("Review not saved: {reason}. Judging needs a saved review.").as_str()),
        "{text}"
    );
    assert_eq!(row(&text, "notes.txt").0, "open");
    assert!(!text.contains("supersigil review"), "{text}");
    assert!(!text.contains("supersigil judge"), "{text}");
}

#[test]
fn next_lines_address_the_review_shown() {
    let addressing = Addressing {
        review: Some("feaad307".to_owned()),
        checkout: Some(PathBuf::from("../my repo")),
        review_dir: Some(PathBuf::from("/tmp/reviews")),
        ..Addressing::default()
    };
    let flags = "--review=feaad307 --checkout='../my repo' --review-dir=/tmp/reviews";

    assert_eq!(
        next_lines(
            &["src/a b.rs", "lines:src/c.rs"],
            false,
            &addressing,
            escape
        ),
        [
            // The names print once; the dismissal names its own.
            format!("Next: supersigil judge acknowledge 'src/a b.rs' lines:src/c.rs {flags}"),
            format!("  or: supersigil judge dismiss <name>... --reason <text> {flags}"),
            format!("  then: supersigil judge complete {flags}"),
        ]
    );
    assert_eq!(
        next_lines(&[], false, &Addressing::default(), escape),
        ["Next: supersigil judge complete"]
    );
    assert!(next_lines(&[], true, &addressing, escape).is_empty());
    assert_eq!(shell_word("it's"), r"'it'\''s'");
    assert_eq!(shell_word(""), "''");

    let text = render_summary(&sample_review(), &addressing, escape);
    assert!(
        text.contains(&format!("\n  then: supersigil judge complete {flags}\n")),
        "{text}"
    );
}

#[test]
fn flag_values_that_start_with_a_dash_stay_values() {
    let addressing = Addressing {
        review: Some("feaad307".to_owned()),
        checkout: Some(PathBuf::from("-repo")),
        review_dir: Some(PathBuf::from("-reviews")),
        ..Addressing::default()
    };
    assert_eq!(
        next_lines(&[], false, &addressing, escape),
        [
            "Next: supersigil judge complete --review=feaad307 --checkout=-repo --review-dir=-reviews"
        ]
    );
    assert_eq!(
        addressing.review("--target HEAD"),
        "supersigil review --target HEAD --checkout=-repo --review-dir=-reviews"
    );
}

#[test]
fn names_that_read_as_flags_follow_a_double_dash() {
    let addressing = Addressing {
        review: Some("feaad307".to_owned()),
        ..Addressing::default()
    };

    assert_eq!(
        next_lines(&["lines:src/c.rs", "-x.rs"], false, &addressing, escape),
        [
            "Next: supersigil judge acknowledge --review=feaad307 -- lines:src/c.rs -x.rs",
            "  or: supersigil judge dismiss <name>... --reason <text> --review=feaad307",
            "  then: supersigil judge complete --review=feaad307",
        ]
    );
}

#[test]
fn more_than_20_open_items_are_left_to_the_list_above() {
    let names: Vec<String> = (0..21).map(|i| format!("f{i:02}.txt")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let addressing = Addressing {
        review: Some("feaad307".to_owned()),
        ..Addressing::default()
    };

    // The command stays short enough for a command line.
    assert_eq!(
        next_lines(&names, false, &addressing, escape),
        [
            "Next: supersigil judge acknowledge <name>... --review=feaad307 (21 open items, \
             listed above)",
            "  or: supersigil judge dismiss <name>... --reason <text> --review=feaad307",
            "  then: supersigil judge complete --review=feaad307",
        ]
    );
    assert_eq!(
        next_lines(&names[..20], false, &Addressing::default(), escape)[0],
        format!(
            "Next: supersigil judge acknowledge {}",
            names[..20].join(" ")
        )
    );
    let text = summary(&review_of_files(21));
    assert!(
        text.contains(
            "\n\nNext: supersigil judge acknowledge <name>... (21 open items, listed above)\n"
        ),
        "{text}"
    );
}

#[test]
fn follow_up_reviews_repeat_the_checkout_and_reviews_directory() {
    let addressing = Addressing {
        review: Some("feaad307".to_owned()),
        checkout: Some(PathBuf::from("../my repo")),
        review_dir: Some(PathBuf::from("/tmp/reviews")),
        ..Addressing::default()
    };
    let flags = "--checkout='../my repo' --review-dir=/tmp/reviews";
    let mut unsaved = sample_review();
    unsaved.basis = None;
    unsaved.basis_unsaved = Some(Unsaved::Busy);

    let unsaved = render_summary(&unsaved, &addressing, escape);
    let unchanged = render_summary(&review_of_files(0), &addressing, escape);

    assert!(
        unsaved.ends_with(&format!("\n\nNext: supersigil review {flags}\n")),
        "{unsaved}"
    );
    assert!(
        unchanged.contains(&format!(
            "\nfiles:\n  no changes in scope; to review a commit just made, run supersigil \
             review --target HEAD {flags}\n"
        )),
        "{unchanged}"
    );
}

#[test]
fn review_commands_repeat_the_checkout_and_the_reviews_directory() {
    let addressing = Addressing {
        review: Some("feaad307".to_owned()),
        checkout: Some(PathBuf::from("../my repo")),
        review_dir: Some(PathBuf::from("/tmp/reviews")),
        ..Addressing::default()
    };

    assert_eq!(
        addressing.review("--include-untracked=src/new.rs"),
        "supersigil review --include-untracked=src/new.rs --checkout='../my repo' \
         --review-dir=/tmp/reviews"
    );
    assert_eq!(Addressing::default().review(""), "supersigil review");
}

#[test]
fn a_busy_review_is_retried_with_the_arguments_it_was_given() {
    let addressing = Addressing {
        review: Some("feaad307".to_owned()),
        checkout: Some(PathBuf::from("../my repo")),
        review_dir: Some(PathBuf::from("/tmp/reviews")),
        retry: vec!["--base=HEAD~1".to_owned(), "--target=HEAD".to_owned()],
        retry_paths: vec!["src/parser.rs".to_owned(), "a b".to_owned()],
    };
    let mut unsaved = sample_review();
    unsaved.basis = None;
    unsaved.basis_unsaved = Some(Unsaved::Busy);

    let given = render_summary(&unsaved, &addressing, escape);
    let plain = summary(&unsaved);

    assert!(
        given.ends_with(
            "\n\nNext: supersigil review --base=HEAD~1 --target=HEAD --checkout='../my repo' \
             --review-dir=/tmp/reviews -- src/parser.rs 'a b'\n"
        ),
        "{given}"
    );
    assert!(plain.ends_with("\n\nNext: supersigil review\n"), "{plain}");
}

#[test]
fn commands_show_a_flag_value_that_cannot_be_printed_as_given() {
    // Escaped, the control character would print another directory's name.
    let addressing = Addressing {
        review: Some("feaad307".to_owned()),
        checkout: Some(PathBuf::from("../my repo")),
        review_dir: Some(PathBuf::from("/tmp/r-\u{1b}")),
        ..Addressing::default()
    };
    let flags = "--checkout='../my repo' --review-dir=<as given>";
    let note = "The --review-dir value is not valid UTF-8 or holds a control character, so \
                commands show <as given> for it: pass it as given.";
    let mut unsaved = sample_review();
    unsaved.basis = None;
    unsaved.basis_unsaved = Some(Unsaved::Busy);

    let saved = render_summary(&sample_review(), &addressing, escape);
    let unsaved = render_summary(&unsaved, &addressing, escape);
    let unchanged = render_summary(&review_of_files(0), &addressing, escape);

    // The note follows the commands, once.
    assert!(
        saved.contains(&format!(
            "\n  then: supersigil judge complete --review=feaad307 {flags}\n{note}\n"
        )),
        "{saved}"
    );
    assert!(
        unsaved.ends_with(&format!("\n\nNext: supersigil review {flags}\n{note}\n")),
        "{unsaved}"
    );
    assert!(
        unchanged.ends_with(&format!(
            "run supersigil review --target HEAD {flags}\n\n{note}\n"
        )),
        "{unchanged}"
    );
    for text in [&saved, &unsaved, &unchanged] {
        assert_eq!(text.matches(note).count(), 1, "{text}");
    }
    let neither = Addressing {
        checkout: Some(PathBuf::from("\u{7f}")),
        ..addressing.clone()
    };
    assert_eq!(
        neither.review(""),
        "supersigil review --checkout=<as given> --review-dir=<as given>"
    );
    assert_eq!(
        neither.note().as_deref(),
        Some(
            "The --checkout and --review-dir values are not valid UTF-8 or hold a control \
             character, so commands show <as given> for them: pass them as given."
        )
    );
    let printable = Addressing {
        review_dir: Some(PathBuf::from("/tmp/r-ü")),
        ..addressing
    };
    assert_eq!(
        printable.review(""),
        "supersigil review --checkout='../my repo' --review-dir=/tmp/r-ü"
    );
    assert_eq!(printable.note(), None);
}

/// The sample review with only `count` files, each with lines no recorded
/// edit explains, and nothing else in its gate.
fn review_of_files(count: usize) -> Review {
    let unexplained = attribution(vec![], vec![unexplained()], vec![no_fate()]);
    let mut review = sample_review();
    review.files = (0..count)
        .map(|i| {
            text_file(
                &format!("f{i:02}.txt"),
                FileStatus::Modified,
                Some(b"draft\n"),
                Some(b"final\n"),
                &unexplained,
                Vec::new(),
                Vec::new(),
            )
        })
        .collect();
    review.scope = ScopeInfo::default();
    review.records = Vec::new();
    review.evidence = EvidenceInfo::default();
    review.unattributed = unattributed_summary(&review.files, &review.scope);
    gated(review, |_, _| Vec::new())
}

#[test]
fn a_gate_of_more_than_20_rows_suggests_a_pager() {
    let hint = "\nLong output: run the same review command with --format terminal and pipe it \
                to less.\n";
    let twenty = summary(&review_of_files(20));
    let more = summary(&review_of_files(21));

    assert!(twenty.contains("Gate: 20 open, 0 closed."), "{twenty}");
    assert!(!twenty.contains(hint), "{twenty}");
    assert!(more.ends_with(hint), "{more}");
}

#[test]
fn a_repository_wide_change_is_summarized_promptly() {
    // A reformat of every file: 5,000 files with unexplained lines, each
    // one gate item, named and shown.
    let text = finishes_within(Duration::from_secs(20), || summary(&review_of_files(5000)));

    assert!(text.contains("\nGate: 5000 open, 0 closed.\n"), "{text}");
    assert_eq!(row(&text, "f4999.txt").0, "open");
}

#[test]
fn a_working_tree_without_changes_points_at_a_commit_just_made() {
    let mut review = review_of_files(0);

    let working_tree = summary(&review);
    review.target.kind = TargetKindInfo::Commit;
    let commit = summary(&review);

    assert!(
        working_tree.contains("\nGate: no items.\n"),
        "{working_tree}"
    );
    assert!(
        working_tree.contains(
            "\nfiles:\n  no changes in scope; to review a commit just made, run supersigil \
             review --target HEAD\n"
        ),
        "{working_tree}"
    );
    assert!(
        commit.contains("\nfiles:\n  no changes in scope\n"),
        "{commit}"
    );
    assert!(
        commit.starts_with(&format!(
            "Review {}: commit aaaaaaa over aaaaaaa.\n",
            review.basis.as_ref().unwrap().id.short()
        )),
        "{commit}"
    );
}

/// The id of an earlier review, of another target tree than the sample's.
fn earlier_id() -> BasisId {
    let mut document = sample_basis().document;
    document.target.tree = "d".repeat(40);
    document.basis_id().unwrap()
}

/// The earlier review related to `review` as `relation`, completed on
/// 2026-10-13 when `completed`, with nothing changed since.
fn related(relation: Relation, completed: bool) -> RelatedInfo {
    let earlier = earlier_id();
    RelatedInfo {
        completion: completed
            .then(|| JudgmentInfo::new(&judgment("done", &earlier, JudgmentKind::Complete, &[]))),
        review: earlier,
        relation,
        changed_paths: Some(Vec::new()),
        changed_evidence: ChangedEvidence::default(),
        uncovered: Vec::new(),
    }
}

/// The lines of `text` from its status line to the gate line.
fn status_lines(text: &str) -> Vec<&str> {
    text.lines()
        .skip(4)
        .take_while(|line| !line.starts_with("Gate: "))
        .collect()
}

#[test]
fn a_completion_carried_over_names_its_review() {
    let mut review = closed_review();
    let carried = related(Relation::SameContent, true);
    review.status = StatusInfo {
        complete: true,
        completion: carried.completion.clone(),
        related: vec![carried],
    };

    let text = summary(&review);

    assert_eq!(
        status_lines(&text),
        [format!(
            "Review complete: completed on 2026-10-13 by Joni Savolainen for the same content \
             (review {}).",
            earlier_id().short()
        )],
        "{text}"
    );
    assert!(!text.contains("Next:"), "{text}");
}

#[test]
fn changed_code_and_evidence_are_listed_since_the_completion() {
    let mut review = sample_review();
    let mut changed = related(Relation::CodeChanged, true);
    changed.changed_paths = Some(vec![
        PathBytes::new("src/lib.rs"),
        PathBytes::new("src/new.rs"),
    ]);
    changed.changed_evidence = ChangedEvidence {
        explanations: vec![PathBytes::new("notes.txt"), PathBytes::new("old.txt")],
        new: vec![
            NewObservations {
                session: "s1".to_owned(),
                paths: vec![PathBytes::new("notes.txt")],
            },
            NewObservations {
                session: "s2".to_owned(),
                paths: vec![PathBytes::new("notes.txt"), PathBytes::new("old.txt")],
            },
        ],
        capture: vec![TRANSCRIPT.to_owned()],
        gone_or_rewritten: vec![RecordId::new("r1"), RecordId::new("r2")],
    };
    // Only the review the completion was made on says what differs.
    changed.uncovered = vec![UncoveredInfo {
        id: review.gate.items[0].id.clone(),
        name: review.gate.items[0].name.clone(),
        reasons: vec!["new"],
    }];
    review.status.related = vec![changed];

    let text = summary(&review);

    let since = format!(
        "since your completion on 2026-10-13 (review {})",
        earlier_id().short()
    );
    assert_eq!(
        status_lines(&text),
        [
            format!("Review not complete. Code changed {since}:"),
            "  src/lib.rs".to_owned(),
            "  src/new.rs".to_owned(),
            format!("Evidence changed {since}:"),
            "  explanations differ: notes.txt, old.txt".to_owned(),
            "  new: session s1: notes.txt; session s2: notes.txt, old.txt".to_owned(),
            format!("  capture limitations changed: {TRANSCRIPT}"),
            "  gone or rewritten: r1, r2".to_owned(),
        ],
        "{text}"
    );
}

#[test]
fn changed_paths_are_unavailable_once_the_reviewed_snapshot_is_gone() {
    let mut review = sample_review();
    let mut changed = related(Relation::CodeChanged, true);
    changed.changed_paths = None;
    review.status.related = vec![changed];

    let text = summary(&review);
    let json = serde_json::to_value(&review.status).unwrap();

    assert_eq!(
        status_lines(&text),
        [
            format!(
                "Review not complete. Code changed since your completion on 2026-10-13 (review \
                 {}):",
                earlier_id().short()
            ),
            "  changed paths unavailable: the reviewed snapshot is no longer in the repository"
                .to_owned(),
        ],
        "{text}"
    );
    assert_eq!(json["related"][0]["changed_paths"], Value::Null);
}

#[test]
fn a_same_content_completion_names_the_gate_items_it_did_not_cover() {
    let mut review = closed_review();
    let mut same = related(Relation::SameContent, true);
    same.uncovered = review.gate.items[..2]
        .iter()
        .zip([vec!["file on disk changed"], vec!["new"]])
        .map(|(item, reasons)| UncoveredInfo {
            id: item.id.clone(),
            name: item.name.clone(),
            reasons,
        })
        .collect();
    let names: Vec<String> = review.gate.items[..2]
        .iter()
        .map(|item| item.name.clone())
        .collect();
    review.status.related = vec![same];

    let text = summary(&review);

    assert_eq!(
        status_lines(&text),
        [
            "Review not complete. Completed on 2026-10-13 for the same content, but these gate \
             items differ from that review:"
                .to_owned(),
            format!("  {}: file on disk changed", names[0]),
            format!("  {}: new", names[1]),
            "No open gate items. This snapshot has not been marked complete.".to_owned(),
        ],
        "{text}"
    );
    assert!(
        text.ends_with("\n\nNext: supersigil judge complete\n"),
        "{text}"
    );
}

#[test]
fn related_reviews_without_a_completion_change_nothing_in_the_status() {
    let mut review = closed_review();
    let mut changed = related(Relation::CodeChanged, false);
    changed.changed_paths = Some(vec![PathBytes::new("src/lib.rs")]);
    review.status.related = vec![changed];

    let text = summary(&review);

    assert_eq!(status_lines(&text), ["Review not complete."], "{text}");
}

#[test]
fn a_review_without_changes_or_gate_items_has_nothing_to_review() {
    let mut review = review_of_files(0);
    let nothing = summary(&review);
    review.basis = None;
    let unsaved = summary(&review);
    // A gate item without a changed file still needs a judgment.
    let mut gated_only = sample_review();
    gated_only.files.clear();
    let something = summary(&gated_only);

    assert!(
        nothing.contains(
            "\nNothing to review: no changes in scope.\nGate: no items.\n\nfiles:\n  no \
             changes in scope; to review a commit just made, run supersigil review --target \
             HEAD\n"
        ),
        "{nothing}"
    );
    for text in [&nothing, &unsaved] {
        assert!(!text.contains("Review not complete"), "{text}");
        assert!(!text.contains("Next:"), "{text}");
    }
    assert!(
        something.contains("\nReview not complete.\n"),
        "{something}"
    );
    assert!(
        something.contains("\nNext: supersigil judge acknowledge "),
        "{something}"
    );
}

#[test]
fn terminal_summary_escapes_untrusted_text() {
    let mut review = sample_review();
    review.files[3].path = "src/\u{1b}[2Klib.rs".to_owned();
    review.records[1].reason = Some("locked\u{1b}[2K".to_owned());
    let review = gated(review, |_, _| Vec::new());
    let text = summary(&review);
    assert!(text.contains(r"src/\x1b[2Klib.rs"));
    assert!(text.contains(r"evidence not read: locked\x1b[2K"), "{text}");
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
            Ok(b""),
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
        command_changes: line_command_changes(why_command_changes(), line.as_ref()),
        line,
        edits: edits(),
        conflicting_edits: Vec::new(),
    }
}

/// Two commands changing the explained file, newest first. One diff adds
/// both sample lines; the harness omits the other's diff.
fn why_command_changes() -> Vec<CommandChange> {
    vec![
        command_change(
            "command:3",
            "2026-09-28T10:00:08.000Z",
            "python3 - <<'EOF'\nimport re\nEOF",
            hunks(&[" // top", "+fn greet() {}", "+fn other() {}"]),
        ),
        command_change(
            "command:2",
            "2026-09-28T10:00:07.000Z",
            "cargo fmt --all",
            Material::unavailable("no entry in the harness report"),
        ),
    ]
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
    let text = summary(&review);
    assert!(text.contains(
        "\n  scope: a path selector named the worktree root, so the review has no path filter\n"
    ));
}

#[test]
fn why_says_why_differs_from_head_is_unknown() {
    let attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)],
        vec![introduced("e1")],
        vec![],
    );
    let head = "old\n".repeat(supersigil_review::diff::COARSE_LINES + 1);
    let target = "fn greet() {}\n".repeat(supersigil_review::diff::COARSE_LINES + 1);
    let line = why_line(
        &AttributionState::Available(&attr),
        0,
        &split_lines(target.as_bytes()),
        Ok(head.as_bytes()),
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
    assert!(text.contains("\n  edit e1 (introduced) at "), "{text}");
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
        Ok(b""),
    ));
    let text = render_why(&why, escape);
    assert!(text.contains("not attributed"), "{text}");
    assert!(!text.contains("  edit "), "{text}");
}

/// A target line every chain agrees on, with its contributors.
fn agreed_with(
    origins: &[Origin],
    whitespace_only: &[&str],
    earlier: &[&str],
    class: Option<ChainClass>,
) -> LineOutcome {
    LineOutcome::Agreed {
        provenance: Provenance {
            origins: origins.iter().cloned().collect(),
            whitespace_only: whitespace_only.iter().map(|e| eid(e)).collect(),
            earlier: earlier.iter().map(|e| eid(e)).collect(),
        },
        class,
    }
}

/// `why` on line 1 of `fn greet() {}\n` under `attr`, with details for
/// every edit in `ids`, a second apart in the order given.
fn why_on_first_line(attr: &PathAttribution, ids: &[&str]) -> String {
    let mut why = sample_why(OnDiskCheck::Captured);
    why.line = Some(why_line(
        &AttributionState::Available(attr),
        0,
        &split_lines(b"fn greet() {}\n"),
        Ok(b""),
    ));
    why.edits = ids
        .iter()
        .enumerate()
        .map(|(k, id)| {
            let time = format!("2026-09-28T10:00:{:02}.000Z", k + 1);
            ((*id).to_owned(), other_edit(&time))
        })
        .collect();
    render_why(&why, escape)
}

#[test]
fn why_prints_the_earlier_contributors_of_an_attributed_line() {
    // Creation e1, an unrelated edit x elsewhere in the file, then e2
    // replacing the line e1 wrote.
    let attr = attribution(
        vec![chain(
            0,
            ChainClass::ExactFromBase,
            &["e1", "x", "e2"],
            ChainEnd::Base,
        )],
        vec![agreed_with(
            &[Origin::Introduced(eid("e2"))],
            &[],
            &["e1"],
            Some(ChainClass::ExactFromBase),
        )],
        vec![],
    );
    let text = why_on_first_line(&attr, &["e1", "x", "e2"]);
    assert!(text.contains("\n  edit e1 (earlier) at "), "{text}");
    assert!(text.contains("\n  edit e2 (introduced) at "), "{text}");
    // An edit of the chain the line's provenance does not name.
    assert!(!text.contains("edit x"), "{text}");
}

#[test]
fn why_prints_the_whitespace_only_contributors_of_an_attributed_line() {
    // e1 introduced the line; w later changed only its whitespace, which
    // can still change behavior.
    let attr = attribution(
        vec![chain(
            0,
            ChainClass::ExactFromBase,
            &["e1", "w"],
            ChainEnd::Base,
        )],
        vec![agreed_with(
            &[Origin::Introduced(eid("e1"))],
            &["w"],
            &[],
            Some(ChainClass::ExactFromBase),
        )],
        vec![],
    );
    let text = why_on_first_line(&attr, &["e1", "w"]);
    assert!(text.contains("\n  edit e1 (introduced) at "), "{text}");
    assert!(text.contains("\n  edit w (whitespace only) at "), "{text}");
}

#[test]
fn why_prints_every_contributor_of_a_whitespace_only_line() {
    let attr = attribution(
        vec![chain(
            0,
            ChainClass::ExactFromBase,
            &["e1", "w", "v"],
            ChainEnd::Base,
        )],
        vec![agreed_with(
            &[Origin::WhitespaceAdded(eid("v"))],
            &["w"],
            &["e1"],
            Some(ChainClass::ExactFromBase),
        )],
        vec![],
    );
    let text = why_on_first_line(&attr, &["e1", "w", "v"]);
    assert!(text.contains("only whitespace changed"), "{text}");
    assert!(text.contains("\n  edit e1 (earlier) at "), "{text}");
    assert!(text.contains("\n  edit w (whitespace only) at "), "{text}");
    assert!(text.contains("\n  edit v (whitespace only) at "), "{text}");
}

#[test]
fn why_prints_no_contributors_for_an_unresolved_ambiguous_or_matched_line() {
    let named = agreed_with(
        &[Origin::Introduced(eid("e2"))],
        &["w"],
        &["e1"],
        Some(ChainClass::ExactFromBase),
    );
    let chains = || {
        vec![chain(
            0,
            ChainClass::ExactFromBase,
            &["e1", "w", "e2"],
            ChainEnd::Base,
        )]
    };
    let ids = ["e1", "w", "e2"];

    let mut unresolved = attribution(chains(), vec![named.clone()], vec![]);
    unresolved.status = PathStatus::SearchIncomplete;
    let text = why_on_first_line(&unresolved, &ids);
    assert!(text.contains("unresolved"), "{text}");
    assert!(!text.contains("  edit "), "{text}");

    let LineOutcome::Agreed { provenance, .. } = named else {
        unreachable!()
    };
    let ambiguous = attribution(
        chains(),
        vec![LineOutcome::Ambiguous {
            readings: vec![
                Reading {
                    chain: 0,
                    class: ChainClass::ExactFromBase,
                    value: provenance.clone(),
                },
                Reading {
                    chain: 1,
                    class: ChainClass::ExactFromBase,
                    value: Provenance::default(),
                },
            ],
        }],
        vec![],
    );
    let text = why_on_first_line(&ambiguous, &ids);
    assert!(text.contains("ambiguous"), "{text}");
    assert!(!text.contains("  edit "), "{text}");

    // Unexplained origins with contributors: the content match decides the
    // sentence, and the provenance is not an explanation.
    let mut matched = attribution(
        chains(),
        vec![agreed_with(
            &[Origin::Unexplained],
            &["w"],
            &["e1"],
            Some(ChainClass::ExactFromBase),
        )],
        vec![],
    );
    matched.content.target[0] = std::collections::BTreeSet::from([eid("e2")]);
    let text = why_on_first_line(&matched, &ids);
    assert!(text.contains("matches the text of edit e2"), "{text}");
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
    let line = |index| why_line(&AttributionState::Available(&attr), index, &lines, Ok(b""));
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
        Vec::new(),
    )];
    review.unattributed = unattributed_summary(&review.files, &review.scope);
    let text = summary(&gated(review, |_, _| Vec::new()));
    assert_eq!(
        row(&text, "src/lib.rs"),
        (
            "open",
            "2 unexplained lines: 2 unresolved (search incomplete)"
        ),
        "{text}"
    );
    assert!(
        text.contains("\n  M src/lib.rs  +1 -1  2 unresolved (search incomplete)\n"),
        "{text}"
    );
    let after = text.split("\nfiles:\n").nth(1).unwrap();
    let files = after.split("\n\n").next().unwrap();
    for word in ["exact", "introduced", "replaced", "session"] {
        assert!(!files.contains(word), "{word} in {files}");
    }
}

/// One added and one removed line no chain explains, both covered by edit
/// `e1`'s text, on a path whose chain search completed; the content search
/// ran out of budget when `incomplete`.
fn content_matched(incomplete: bool) -> PathAttribution {
    let mut attr = attribution(
        vec![chain(0, ChainClass::ExactFromBase, &["e0"], ChainEnd::Base)],
        vec![unexplained()],
        vec![no_fate()],
    );
    attr.content.target[0] = BTreeSet::from([eid("e1")]);
    attr.content.base[0] = BTreeSet::from([eid("e1")]);
    attr.content.incomplete = incomplete;
    attr
}

/// The sample review with one file, whose every changed line has a content
/// match: no unmatched line shows that the search ran out.
fn content_matched_review(incomplete: bool) -> Review {
    let attr = content_matched(incomplete);
    let mut review = sample_review();
    review.files = vec![text_file(
        "src/lib.rs",
        FileStatus::Modified,
        Some(b"fn old() {}\n"),
        Some(b"fn new() {}\n"),
        &attr,
        Vec::new(),
        Vec::new(),
    )];
    review.unattributed = unattributed_summary(&review.files, &review.scope);
    gated(review, |_, _| Vec::new())
}

#[test]
fn a_review_keeps_whether_a_content_match_search_completed() {
    for (incomplete, search_complete) in [(false, true), (true, false)] {
        let json = serde_json::to_value(content_matched_review(incomplete)).unwrap();
        let file = &json["files"][0];
        let outcomes: Vec<(&Value, &Value, &Value)> = file["hunks"][0]["spans"]
            .as_array()
            .unwrap()
            .iter()
            .map(|span| {
                (
                    &span["side"],
                    &span["outcome"]["kind"],
                    &span["outcome"]["search_complete"],
                )
            })
            .collect();
        let expected = json!(search_complete);
        assert_eq!(
            outcomes,
            [
                (&json!("base"), &json!("content_match"), &expected),
                (&json!("target"), &json!("content_match"), &expected),
            ],
            "incomplete: {incomplete}"
        );
        // Only the content search ran out: the chain search's status stays.
        assert_eq!(file["attribution"]["status"], json!({"kind": "composed"}));
    }

    let complete = summary(&content_matched_review(false));
    let partial = summary(&content_matched_review(true));
    assert_eq!(
        row(&complete, "src/lib.rs"),
        ("open", "2 unexplained lines: 2 content match only"),
        "{complete}"
    );
    assert!(
        complete.contains("\n  M src/lib.rs  +1 -1  2 content match\n"),
        "{complete}"
    );
    assert_eq!(
        row(&partial, "src/lib.rs"),
        (
            "open",
            "2 unexplained lines: 2 content match only (search incomplete)"
        ),
        "{partial}"
    );
    assert!(
        partial.contains("\n  M src/lib.rs  +1 -1  2 content match (search incomplete)\n"),
        "{partial}"
    );
}

#[test]
fn why_keeps_whether_a_content_match_search_completed() {
    let why = |incomplete| {
        let attr = content_matched(incomplete);
        let mut why = sample_why(OnDiskCheck::Captured);
        why.line = Some(why_line(
            &AttributionState::Available(&attr),
            0,
            &split_lines(b"fn new() {}\n"),
            Ok(b""),
        ));
        why
    };
    for (incomplete, search_complete) in [(false, true), (true, false)] {
        let json = serde_json::to_value(why(incomplete)).unwrap();
        let line = &json["line"];
        assert_eq!(
            (
                &line["outcome"]["kind"],
                &line["outcome"]["search_complete"]
            ),
            (&json!("content_match"), &json!(search_complete)),
            "incomplete: {incomplete}"
        );
        assert_eq!(line["status"], json!({"kind": "composed"}));
    }

    let complete = render_why(&why(false), escape);
    let partial = render_why(&why(true), escape);
    assert!(
        complete
            .contains("\n  matches the text of edit e1 (content match, not a composed chain)\n"),
        "{complete}"
    );
    assert!(
        partial.contains(
            "\n  matches the text of edit e1 (content match (search incomplete), not a composed chain)\n"
        ),
        "{partial}"
    );
}

#[test]
fn a_file_line_counts_the_commands_that_changed_an_unattributed_file() {
    let text = summary(&sample_review());
    assert!(
        line_of(&text, "notes.txt").ends_with("changed by 1 recorded command"),
        "{text}"
    );
    // Two commands, and only on a file that has unattributed lines.
    let mut review = sample_review();
    let other = command_change("command:5", "2026-09-28T10:00:10.000Z", "make", hunks(&[]));
    review.files[1].command_changes.push(other.clone());
    review.files[3].command_changes.push(other);
    let text = summary(&review);
    assert!(text.contains("changed by 2 recorded commands"), "{text}");
    assert_eq!(text.matches("changed by").count(), 1, "{text}");
    // One command listed for the path in two worktrees is one command.
    let mut elsewhere = formatter_run();
    elsewhere.worktree = "/work/repo/.claude/worktrees/x".to_owned();
    let mut review = sample_review();
    review.files[1].command_changes.push(elsewhere);
    let text = summary(&review);
    assert!(
        line_of(&text, "notes.txt").ends_with("changed by 1 recorded command"),
        "{text}"
    );
}

/// The file line of `path` in a terminal summary.
fn line_of(text: &str, path: &str) -> String {
    text.lines()
        .find(|line| line.contains(path) && line.contains("  +"))
        .unwrap_or_else(|| panic!("no file line for {path} in {text}"))
        .to_owned()
}

/// `why` on the unattributed second line of the sample file.
fn why_on_the_unattributed_line() -> Why {
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
        Ok(b""),
    ));
    why.command_changes = line_command_changes(why_command_changes(), why.line.as_ref());
    why
}

#[test]
fn why_marks_the_commands_whose_reported_diff_adds_an_unexplained_line() {
    let why = why_on_the_unattributed_line();
    let flags: Vec<bool> = why
        .command_changes
        .iter()
        .map(|c| c.adds_line_text)
        .collect();
    assert_eq!(flags, vec![true, false]);
    let text = render_why(&why, escape);
    let expected = "\
  changed by 2 recorded commands (observed by the harness, not attribution):
    2026-09-28T10:00:08.000Z  command:3  session s1  modified
      python3 - <<'EOF' … (2 more lines)
      its reported diff adds a line with this text (display match)
    2026-09-28T10:00:07.000Z  command:2  session s1  modified
      cargo fmt --all
      reported diff not retained: no entry in the harness report
";
    assert!(text.contains(expected), "{text}");
}

#[test]
fn why_lists_commands_without_markers_under_an_attributed_line() {
    let why = sample_why(OnDiskCheck::Captured);
    // The flag is computed for every line, but shown in the terminal
    // only when no chain explains it.
    assert!(why.command_changes[0].adds_line_text);
    let text = render_why(&why, escape);
    assert!(text.contains("  changed by 2 recorded commands"), "{text}");
    assert!(!text.contains("its reported diff"), "{text}");
    assert!(!text.contains("reported diff not retained"), "{text}");
}

#[test]
fn a_reported_diff_adds_a_line_only_by_an_exact_added_line() {
    let why = why_on_the_unattributed_line();
    let line = why.line.as_ref();
    let adds = |lines: &[&str]| {
        let change = command_change("c", "t", "x", hunks(lines));
        line_command_changes(vec![change], line)[0].adds_line_text
    };
    assert!(adds(&["+fn other() {}"]));
    // Context and removed lines, other text, and other spacing do not count.
    assert!(!adds(&[" fn other() {}", "-fn other() {}"]));
    assert!(!adds(&["+fn other() {} "]));
    assert!(!adds(&["+  fn other() {}"]));
    assert!(!adds(&["++fn other() {}"]));
    assert!(!adds(&[]));
    // No line analyzed: nothing to compare.
    let change = command_change("c", "t", "x", hunks(&["+fn other() {}"]));
    assert!(!line_command_changes(vec![change], None)[0].adds_line_text);
}

#[test]
fn the_marker_compares_the_line_without_its_one_terminator() {
    let attr = attribution(vec![], vec![unexplained()], vec![]);
    let adds = |target: &[u8], hunk_line: &str| {
        let line = why_line(
            &AttributionState::Available(&attr),
            0,
            &split_lines(target),
            Ok(b""),
        );
        let change = command_change("c", "t", "x", hunks(&[hunk_line]));
        line_command_changes(vec![change], Some(&line))[0].adds_line_text
    };
    assert!(adds(b"x\n", "+x"));
    assert!(adds(b"x\r\n", "+x"));
    assert!(adds(b"x", "+x"));
    // Without a newline there is no terminator: the return is text.
    assert!(!adds(b"x\r", "+x"));
    assert!(adds(b"x\r", "+x\r"));
    // A return that is part of the line's text is not its terminator.
    assert!(!adds(b"x\r\r\n", "+x"));
    assert!(adds(b"x\r\r\n", "+x\r"));
    assert!(!adds(b"x\n", "+x\r"));
}

#[test]
fn one_command_listed_in_two_worktrees_is_counted_once_by_why() {
    let mut why = why_on_the_unattributed_line();
    let mut elsewhere = why.command_changes[0].clone();
    elsewhere.change.worktree = "/work/repo/.claude/worktrees/x".to_owned();
    why.command_changes.push(elsewhere);
    let text = render_why(&why, escape);
    // Three entries, two commands.
    assert!(
        text.contains("  changed by 2 recorded commands (observed"),
        "{text}"
    );
    assert_eq!(text.matches("  session s1  ").count(), 3, "{text}");
}

#[test]
fn a_line_of_only_whitespace_matches_no_reported_diff() {
    let attr = attribution(vec![], vec![unexplained(), unexplained()], vec![]);
    for target in [&b"\n  \n"[..], &b"  \r\n\t\n"[..]] {
        for index in 0..2 {
            let line = why_line(
                &AttributionState::Available(&attr),
                index,
                &split_lines(target),
                Ok(b""),
            );
            let change = command_change("c", "t", "x", hunks(&["+", "+  ", "+\t"]));
            let found = line_command_changes(vec![change], Some(&line));
            assert!(!found[0].adds_line_text, "{target:?} line {index}");
        }
    }
}

#[test]
fn why_says_what_a_listed_commands_report_leaves_open() {
    // Flagged reports and unnamed counted files limit what the report
    // shows. The listing states this.
    let mut why = sample_why(OnDiskCheck::Captured);
    assert!(!render_why(&why, escape).contains("its report"));
    let change = &mut why.command_changes[0].change;
    change.flags = BTreeSet::from(["shared".to_owned(), "un\u{1b}available".to_owned()]);
    change.unlisted = 3;
    change.outside = 1;
    let text = render_why(&why, escape);
    assert!(
        text.contains(
            "\n      its report: flagged shared, un\\x1bavailable; 3 changed files it does not \
             name; 1 file outside the checkout\n"
        ),
        "{text}"
    );
    // Each part appears only when it applies.
    let change = &mut why.command_changes[0].change;
    change.flags.clear();
    change.unlisted = 1;
    change.outside = 0;
    let text = render_why(&why, escape);
    assert!(
        text.contains("\n      its report: 1 changed file it does not name\n"),
        "{text}"
    );
}

#[test]
fn why_shows_the_command_behind_a_shell_edit() {
    let mut why = sample_why(OnDiskCheck::Captured);
    why.command_changes.clear();
    why.edits.get_mut("e1").unwrap().origin = EditOriginInfo::Shell {
        command: "command:7".to_owned(),
        text: Some("cat > src/lib.rs <<'EOF'\nfn greet() {}\nEOF\n".to_owned()),
    };
    let text = render_why(&why, escape);
    assert!(
        text.contains(
            "\n    read from a heredoc in command command:7: cat > src/lib.rs <<'EOF' … (2 more lines)\n    \
             the harness reported that the command changed the file, not that this statement ran\n"
        ),
        "{text}"
    );
    // The command may be missing from the records the review read.
    why.edits.get_mut("e1").unwrap().origin = EditOriginInfo::Shell {
        command: "command:7".to_owned(),
        text: None,
    };
    let text = render_why(&why, escape);
    assert!(
        text.contains("read from a heredoc in command command:7: command text not recorded"),
        "{text}"
    );
}

#[test]
fn why_escapes_and_shortens_command_text() {
    let mut why = why_on_the_unattributed_line();
    let long = format!("echo \u{1b}[2K{}\nsecond", "x".repeat(200));
    why.command_changes[1].change.text = long;
    why.command_changes[1].change.patch = Material::Withheld {
        policy: "capture.\u{1b}diff".to_owned(),
    };
    let text = render_why(&why, escape);
    assert!(!text.contains('\u{1b}'), "{text}");
    let shown = format!("      echo \\x1b[2K{}… … (1 more line)\n", "x".repeat(91));
    assert!(text.contains(&shown), "{text}");
    assert!(
        text.contains("      reported diff withheld: capture.\\x1bdiff\n"),
        "{text}"
    );
}

#[test]
fn the_evidence_notes_count_unconfirmed_shell_writes() {
    let mut review = sample_review();
    review.evidence.candidate_transcripts[0].capture_limitations = vec![CaptureLimitation {
        session: SessionId::new("s1"),
        transcript: TRANSCRIPT.to_owned(),
        from_ordinal: 0,
        to_ordinal: 9,
        counts: CaptureCounts {
            unconfirmed_shell_writes: 2,
            ..CaptureCounts::default()
        },
    }];
    let text = summary(&review);
    assert!(
        text.contains("(session s1): 2 unconfirmed shell writes; not localized to a path"),
        "{text}"
    );
}
