//! Tests sorted gate items per origin and subject, fingerprint changes with
//! their material, and saved explanation digests and capture entries.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use common::{attribution, base_line, chain, eid, introduced, kept, replaced};
use serde::Serialize;
use serde_json::json;
use supersigil_anchor::{
    ChainClass, ChainEnd, Conflict, LineOutcome, Origin as LineOrigin, Provenance,
};
use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, Edit, EditOperation, EditOrigin, FileState, Material,
};
use supersigil_record::reviews::{
    CaptureEntry, Fingerprint, FingerprintPart, GateDetail, GateItem, Origin, PathBytes, Subject,
};
use supersigil_record::{ContentId, EventId, RecordId, SessionId, Timestamp, TurnId};
use supersigil_review::gate::{
    Gap, GateInputs, OnDiskState, UntrackedEvidence, capture, explanations, gate_items,
};
use supersigil_review::model::{
    AncestryInfo, BaseInfo, BytesStatus, EditInfo, EditOriginInfo, EvidenceInfo, FileInput,
    FileKindInfo, FileReview, FileStatus, GateInfo, Mention, NotCapturedInfo, OriginsInfo,
    RecordInfo, RepoPathInfo, ScopeInfo, Span, StatusInfo, TargetInfo, TargetKindInfo,
    TranscriptInfo, UnavailableInfo, UnreconciledInfo, UntrackedInfo, file_review,
    unattributed_summary,
};
use supersigil_review::outcome::{AttributionState, Outcome};
use supersigil_review::{REVIEW_SCHEMA, Review};

const WORKTREE: &str = "/work/repo";
const TRANSCRIPT: &str = "/home/u/.claude/projects/-work-repo/s1.jsonl";
const COMMIT: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn path(bytes: &[u8]) -> RepoPathInfo {
    RepoPathInfo {
        display: String::from_utf8_lossy(bytes).into_owned(),
        escaped: std::str::from_utf8(bytes)
            .is_err()
            .then(|| format!("{bytes:x?}")),
        bytes: PathBytes::new(bytes),
    }
}

fn subject(bytes: &[u8]) -> Subject {
    Subject::Path(PathBytes::new(bytes))
}

fn file(
    bytes: &[u8],
    kind: FileKindInfo,
    base: &[u8],
    target: &[u8],
    attr: AttributionState<'_>,
) -> FileReview {
    file_review(FileInput {
        path: &path(bytes),
        status: FileStatus::Modified,
        old_mode: "100644".to_owned(),
        new_mode: "100644".to_owned(),
        old_blob: Some("1".repeat(40)),
        new_blob: Some("2".repeat(40)),
        kind,
        bytes_status: BytesStatus {
            base: "identical".to_owned(),
            target: "identical".to_owned(),
        },
        base_blob: Some(base),
        target_blob: Some(target),
        attribution: attr,
        mentions: Vec::new(),
        command_changes: Vec::new(),
        conflicting_edits: Vec::new(),
    })
}

fn binary(bytes: &[u8]) -> FileReview {
    file(
        bytes,
        FileKindInfo::Binary,
        b"\0",
        b"\0\0",
        AttributionState::Unavailable {
            reason: "not diffed: binary".to_owned(),
        },
    )
}

/// `src/a.rs`: edit `e1` wrote one line in place of a base line, and no
/// chain explains the line after it, which `e2` touched earlier.
fn explained_in_part() -> FileReview {
    let unexplained = LineOutcome::Agreed {
        provenance: Provenance {
            origins: BTreeSet::from([LineOrigin::Unexplained]),
            earlier: BTreeSet::from([eid("e2")]),
            ..Provenance::default()
        },
        class: Some(ChainClass::ExactFromBase),
    };
    let attr = attribution(
        vec![chain(
            0,
            ChainClass::ExactFromBase,
            &["e2", "e1"],
            ChainEnd::Base,
        )],
        vec![base_line(0), introduced("e1"), unexplained],
        vec![kept(0), replaced("e1")],
    );
    file(
        b"src/a.rs",
        FileKindInfo::Text,
        b"keep\nold\n",
        b"keep\nnew\nextra\n",
        AttributionState::Available(&attr),
    )
}

fn unexplained_span(review: &mut Review) -> &mut Span {
    let span = review.files[1].hunks[0].spans.last_mut().unwrap();
    assert!(matches!(span.outcome, Outcome::Unattributed { .. }));
    span
}

fn edit_info(time: &str) -> EditInfo {
    EditInfo {
        session: "s1".to_owned(),
        transcript: Some(TRANSCRIPT.to_owned()),
        turn: "a1".to_owned(),
        time: time.to_owned(),
        worktree: WORKTREE.to_owned(),
        operation: EditOperation::Replace,
        origin: EditOriginInfo::Tool,
        prompt: None,
    }
}

fn untracked(bytes: &[u8], recorded_edit: bool) -> UntrackedInfo {
    let info = path(bytes);
    UntrackedInfo {
        path: info.display,
        path_escaped: info.escaped,
        path_bytes: info.bytes,
        recorded_edit,
        include_flag: "--include-untracked <path>".to_owned(),
    }
}

fn not_captured(bytes: &[u8], cause: &str) -> NotCapturedInfo {
    let info = path(bytes);
    NotCapturedInfo {
        path: info.display,
        path_escaped: info.escaped,
        path_bytes: info.bytes,
        on_disk: "present".to_owned(),
        cause: cause.to_owned(),
    }
}

fn review(files: Vec<FileReview>, scope: ScopeInfo) -> Review {
    Review {
        schema: REVIEW_SCHEMA,
        basis: None,
        basis_unsaved: None,
        worktree: WORKTREE.to_owned(),
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
        records: Vec::new(),
        evidence: EvidenceInfo::default(),
        files,
        edits: BTreeMap::from([
            ("e1".to_owned(), edit_info("2026-10-13T10:00:01.000Z")),
            ("e2".to_owned(), edit_info("2026-10-13T10:00:02.000Z")),
        ]),
        decisions: Vec::new(),
        edit_decisions: BTreeMap::new(),
        annotations: Vec::new(),
    }
}

fn observed(id: &str, text: &str) -> Edit {
    Edit {
        id: EventId::new(id),
        turn: TurnId::new("a1"),
        session: SessionId::new("s1"),
        path: PathBuf::from("src/a.rs"),
        before: FileState::Absent,
        after: FileState::known(ContentId::of(text.as_bytes())),
        patch: Material::unavailable("test"),
        old_text: Material::unavailable("write replaces the whole file"),
        new_text: Material::Retained(text.to_owned()),
        replace_all: false,
        operation: EditOperation::Write,
        origin: EditOrigin::Tool,
        checkout: PathBuf::from(WORKTREE),
        time: Timestamp::new("2026-10-13T10:00:00.000Z"),
        source_ordinal: 0,
        agent_id: None,
        transcript: Some(TRANSCRIPT.to_owned()),
    }
}

/// Owns edit sightings by ID for [`GateInputs::edits`] to borrow.
type Sightings = BTreeMap<String, Vec<Edit>>;

fn borrowed(sightings: &Sightings) -> BTreeMap<String, Vec<&Edit>> {
    sightings
        .iter()
        .map(|(id, seen)| (id.clone(), seen.iter().collect()))
        .collect()
}

/// A [`Gap`] owning its sightings.
#[derive(Clone)]
struct OwnedGap {
    path: PathBytes,
    prev: Vec<Edit>,
    next: Vec<Edit>,
}

/// An [`UntrackedEvidence`] owning its sightings.
#[derive(Clone)]
struct OwnedEvidence {
    on_disk: OnDiskState,
    edits: Vec<Edit>,
}

/// Owns sightings for a review's [`GateInputs`] to borrow.
#[derive(Clone, Default)]
struct OwnedInputs {
    discontinuities: Vec<OwnedGap>,
    untracked: BTreeMap<PathBytes, OwnedEvidence>,
    not_captured: BTreeMap<PathBytes, OnDiskState>,
    edits: Sightings,
    questions: Vec<GateItem>,
}

impl OwnedInputs {
    fn borrowed(&self) -> GateInputs<'_> {
        GateInputs {
            discontinuities: self
                .discontinuities
                .iter()
                .map(|gap| Gap {
                    path: gap.path.clone(),
                    prev: gap.prev.iter().collect(),
                    next: gap.next.iter().collect(),
                })
                .collect(),
            untracked: self
                .untracked
                .iter()
                .map(|(path, evidence)| {
                    let evidence = UntrackedEvidence {
                        on_disk: evidence.on_disk.clone(),
                        edits: evidence.edits.iter().collect(),
                    };
                    (path.clone(), evidence)
                })
                .collect(),
            not_captured: self.not_captured.clone(),
            edits: borrowed(&self.edits),
            questions: self.questions.clone(),
        }
    }
}

fn sightings() -> Sightings {
    BTreeMap::from([
        ("e1".to_owned(), vec![observed("e1", "keep\nnew\n")]),
        ("e2".to_owned(), vec![observed("e2", "keep\nold\n")]),
    ])
}

/// Material hashed by gates and explanations, pairing an edit's review
/// entry with every recorded sighting.
#[derive(Serialize)]
struct Recorded<'a> {
    entry: &'a EditInfo,
    sightings: &'a [Edit],
}

fn on_disk(executable: bool, digit: char) -> OnDiskState {
    OnDiskState::File {
        executable,
        content: digit.to_string().repeat(64),
    }
}

/// A gap between two recorded edits of `bytes`, each recorded once.
fn gap(bytes: &[u8]) -> OwnedGap {
    OwnedGap {
        path: PathBytes::new(bytes),
        prev: vec![observed("e3", "first")],
        next: vec![observed("e4", "second")],
    }
}

/// Covers every gate origin except `unreconciled`, with the required inputs.
fn sample() -> (Review, OwnedInputs) {
    let scope = ScopeInfo {
        untracked_excluded: vec![
            untracked(b"src/new.rs", true),
            untracked(b"scratch.txt", false),
        ],
        not_captured: vec![not_captured(b"config.toml", "assume_unchanged")],
        ..ScopeInfo::default()
    };
    let mut review = review(vec![binary(b"logo.png"), explained_in_part()], scope);
    review.origins = OriginsInfo {
        commits: Vec::new(),
        without_origin: vec![COMMIT.to_owned()],
        unavailable: vec![UnavailableInfo {
            worktree: "/work/gone".to_owned(),
            reason: "directory missing".to_owned(),
        }],
    };
    let inputs = OwnedInputs {
        discontinuities: vec![gap(b"src/a.rs")],
        untracked: BTreeMap::from([(
            PathBytes::new("src/new.rs"),
            OwnedEvidence {
                on_disk: on_disk(false, 'a'),
                edits: vec![observed("e5", "new")],
            },
        )]),
        not_captured: BTreeMap::from([(PathBytes::new("config.toml"), OnDiskState::Missing)]),
        edits: sightings(),
        questions: Vec::new(),
    };
    (review, inputs)
}

fn record(id: &str, reason: Option<&str>) -> RecordInfo {
    RecordInfo {
        id: id.to_owned(),
        revision: Some(1),
        reconciled: reason.is_none(),
        reason: reason.map(str::to_owned),
    }
}

fn checkout(path: &str, reason: &str) -> UnreconciledInfo {
    UnreconciledInfo {
        checkout: path.to_owned(),
        transcripts: 1,
        reason: reason.to_owned(),
    }
}

fn with_unreconciled(review: &mut Review) {
    review.records = vec![
        record("rec-1", None),
        record("rec-2", Some("locked by another writer")),
    ];
    review.evidence.unreconciled_checkouts = vec![checkout("/work/x", "records directory locked")];
}

fn digest(material: &impl Serialize) -> FingerprintPart {
    FingerprintPart::of(&serde_json::to_vec(material).unwrap())
}

fn item(items: &[GateItem], origin: Origin) -> &GateItem {
    items.iter().find(|item| item.origin() == origin).unwrap()
}

#[test]
fn each_origin_gives_one_item_with_its_subject_detail_and_parts() {
    let (mut review, inputs) = sample();
    with_unreconciled(&mut review);
    let items = gate_items(&review, &inputs.borrowed());
    let shown: Vec<(Subject, GateDetail, Vec<&str>)> = items
        .iter()
        .map(|item| {
            let parts = item.fingerprint.0.keys().map(String::as_str).collect();
            (item.subject.clone(), item.detail.clone(), parts)
        })
        .collect();
    assert_eq!(
        shown,
        vec![
            (
                subject(b"src/a.rs"),
                GateDetail::UnexplainedLines {
                    unattributed: 1,
                    content_match_incomplete: 0,
                    unresolved: 0,
                    ambiguous: 0,
                    line_ending: 0,
                    content_match_only: 0,
                },
                vec!["content", "evidence", "outcomes"],
            ),
            (
                subject(b"logo.png"),
                GateDetail::NotDiffed {
                    kind: "binary".to_owned(),
                },
                vec!["content", "kind"],
            ),
            (
                subject(b"src/new.rs"),
                GateDetail::UntrackedRecorded { edits: 1 },
                vec!["edits", "on_disk"],
            ),
            (
                subject(b"config.toml"),
                GateDetail::NotCaptured {
                    cause: "assume_unchanged".to_owned(),
                    on_disk: "missing".to_owned(),
                },
                vec!["cause", "on_disk"],
            ),
            (
                Subject::Commit(COMMIT.to_owned()),
                GateDetail::CommitWithoutOrigin,
                vec!["origin"],
            ),
            (
                subject(b"src/a.rs"),
                GateDetail::Discontinuity { gaps: 1 },
                vec!["gaps"],
            ),
            (
                Subject::Record(RecordId::new("rec-2")),
                GateDetail::Unreconciled {
                    reason: "locked by another writer".to_owned(),
                },
                vec!["evidence"],
            ),
            (
                Subject::Checkout(PathBuf::from("/work/x")),
                GateDetail::Unreconciled {
                    reason: "records directory locked".to_owned(),
                },
                vec!["evidence"],
            ),
        ]
    );
}

#[test]
fn each_known_part_is_the_digest_of_its_material() {
    let (mut review, inputs) = sample();
    with_unreconciled(&mut review);
    let items = gate_items(&review, &inputs.borrowed());
    let part = |origin, name: &str| item(&items, origin).fingerprint.0[name].clone();
    let file = &review.files[1];
    let content = (
        &file.old_blob,
        &file.old_mode,
        &file.new_blob,
        &file.new_mode,
    );
    assert_eq!(part(Origin::UnexplainedLines, "content"), digest(&content));
    assert_eq!(
        part(Origin::UnexplainedLines, "outcomes"),
        digest(&[&file.hunks[0].spans[2]])
    );
    // The edit the counted line names, as the review shows it and as the
    // records hold it.
    let named = Recorded {
        entry: &review.edits["e2"],
        sightings: &inputs.edits["e2"],
    };
    assert_eq!(
        part(Origin::UnexplainedLines, "evidence"),
        digest(&BTreeMap::from([("e2", named)]))
    );
    assert_eq!(part(Origin::NotDiffed, "kind"), digest(&"binary"));
    assert_eq!(
        part(Origin::UntrackedRecorded, "on_disk"),
        digest(&on_disk(false, 'a'))
    );
    assert_eq!(
        part(Origin::UntrackedRecorded, "edits"),
        digest(&[observed("e5", "new")])
    );
    assert_eq!(
        part(Origin::NotCaptured, "cause"),
        digest(&"assume_unchanged")
    );
    assert_eq!(
        part(Origin::NotCaptured, "on_disk"),
        digest(&OnDiskState::Missing)
    );
    assert_eq!(
        part(Origin::CommitWithoutOrigin, "origin"),
        digest(&review.origins.unavailable)
    );
    assert_eq!(
        part(Origin::Discontinuity, "gaps"),
        digest(&[([observed("e3", "first")], [observed("e4", "second")])])
    );
    assert_eq!(
        part(Origin::Unreconciled, "evidence"),
        FingerprintPart::Unknown
    );
}

#[test]
fn items_sort_by_origin_then_subject() {
    let scope = ScopeInfo {
        not_captured: vec![
            not_captured(b"z.toml", "skip_worktree"),
            not_captured(b"a.toml", "skip_worktree"),
        ],
        ..ScopeInfo::default()
    };
    let mut review = review(Vec::new(), scope);
    review.origins.without_origin = vec!["f".repeat(40), "0".repeat(40)];
    review.records = vec![
        record("rec-9", Some("locked")),
        record("rec-1", Some("locked")),
    ];
    review.evidence.unreconciled_checkouts =
        vec![checkout("/work/b", "locked"), checkout("/work/a", "locked")];
    let question = |session: &str, id: &str| Subject::Question {
        session: SessionId::new(session),
        id: id.to_owned(),
    };
    let asked = |session, id| {
        GateItem::new(
            question(session, id),
            GateDetail::OpenQuestion {
                text: "Why?".to_owned(),
            },
            Fingerprint::default(),
        )
    };
    let inputs = OwnedInputs {
        discontinuities: vec![gap(b"z.rs"), gap(b"a.rs")],
        questions: vec![asked("s2", "q1"), asked("s1", "q9"), asked("s1", "q1")],
        ..OwnedInputs::default()
    };
    let subjects: Vec<(Origin, Subject)> = gate_items(&review, &inputs.borrowed())
        .into_iter()
        .map(|item| (item.origin(), item.subject))
        .collect();
    assert_eq!(
        subjects,
        vec![
            (Origin::NotCaptured, subject(b"a.toml")),
            (Origin::NotCaptured, subject(b"z.toml")),
            (Origin::CommitWithoutOrigin, Subject::Commit("0".repeat(40))),
            (Origin::CommitWithoutOrigin, Subject::Commit("f".repeat(40))),
            (Origin::Discontinuity, subject(b"a.rs")),
            (Origin::Discontinuity, subject(b"z.rs")),
            (
                Origin::Unreconciled,
                Subject::Record(RecordId::new("rec-1"))
            ),
            (
                Origin::Unreconciled,
                Subject::Record(RecordId::new("rec-9"))
            ),
            (
                Origin::Unreconciled,
                Subject::Checkout(PathBuf::from("/work/a"))
            ),
            (
                Origin::Unreconciled,
                Subject::Checkout(PathBuf::from("/work/b"))
            ),
            (Origin::OpenQuestion, question("s1", "q1")),
            (Origin::OpenQuestion, question("s1", "q9")),
            (Origin::OpenQuestion, question("s2", "q1")),
        ]
    );
}

#[test]
fn sources_of_one_subject_fold_into_one_item() {
    let (mut review, mut inputs) = sample();
    // A second gap, and the first one seen again in another record.
    let mut second = gap(b"src/a.rs");
    second.next = vec![observed("e6", "third")];
    inputs.discontinuities.extend([second, gap(b"src/a.rs")]);
    // Two sightings of one edit naming the untracked file.
    let evidence = inputs.untracked.values_mut().next().unwrap();
    evidence.edits.push(observed("e5", "new"));
    // One checkout reported twice, once per reason.
    review.evidence.unreconciled_checkouts = vec![
        checkout(
            "/work/x",
            "transcript /t/a.jsonl removed before it could be read",
        ),
        checkout("/work/x", "records directory locked"),
    ];
    let items = gate_items(&review, &inputs.borrowed());
    assert_eq!(
        item(&items, Origin::Discontinuity).detail,
        GateDetail::Discontinuity { gaps: 2 }
    );
    assert_eq!(
        item(&items, Origin::UntrackedRecorded).detail,
        GateDetail::UntrackedRecorded { edits: 1 }
    );
    assert_eq!(
        item(&items, Origin::Unreconciled).detail,
        GateDetail::Unreconciled {
            reason: "records directory locked; \
                     transcript /t/a.jsonl removed before it could be read"
                .to_owned()
        }
    );
    assert_eq!(
        items
            .iter()
            .filter(|item| item.origin() == Origin::Unreconciled)
            .count(),
        1
    );
}

#[test]
fn paths_that_print_alike_give_separate_items() {
    let (one, two) = (b"a\xff".as_slice(), b"a\xfe".as_slice());
    assert_eq!(path(one).display, path(two).display);
    let scope = ScopeInfo {
        untracked_excluded: vec![untracked(one, true), untracked(two, true)],
        not_captured: vec![
            not_captured(one, "skip_worktree"),
            not_captured(two, "skip_worktree"),
        ],
        ..ScopeInfo::default()
    };
    let review = review(vec![binary(two), binary(one)], scope);
    let evidence = |state| OwnedEvidence {
        on_disk: state,
        edits: vec![observed("e5", "new")],
    };
    let inputs = OwnedInputs {
        discontinuities: Vec::new(),
        untracked: BTreeMap::from([
            (PathBytes::new(one), evidence(OnDiskState::Missing)),
            (PathBytes::new(two), evidence(on_disk(false, 'a'))),
        ]),
        not_captured: BTreeMap::from([
            (PathBytes::new(one), OnDiskState::Missing),
            (PathBytes::new(two), on_disk(true, 'b')),
        ]),
        edits: BTreeMap::new(),
        questions: Vec::new(),
    };
    let items = gate_items(&review, &inputs.borrowed());
    let shown: Vec<(Origin, Subject, &FingerprintPart)> = items
        .iter()
        .map(|item| {
            let first = item.fingerprint.0.values().next().unwrap();
            (item.origin(), item.subject.clone(), first)
        })
        .collect();
    let content = digest(&(
        &review.files[0].old_blob,
        &review.files[0].old_mode,
        &review.files[0].new_blob,
        &review.files[0].new_mode,
    ));
    assert_eq!(
        shown,
        vec![
            (Origin::NotDiffed, subject(two), &content),
            (Origin::NotDiffed, subject(one), &content),
            (
                Origin::UntrackedRecorded,
                subject(two),
                &digest(&[observed("e5", "new")])
            ),
            (
                Origin::UntrackedRecorded,
                subject(one),
                &digest(&[observed("e5", "new")])
            ),
            (Origin::NotCaptured, subject(two), &digest(&"skip_worktree")),
            (Origin::NotCaptured, subject(one), &digest(&"skip_worktree")),
        ]
    );
    // Each path's on-disk state is its own.
    let states: Vec<&FingerprintPart> = items
        .iter()
        .filter_map(|item| item.fingerprint.0.get("on_disk"))
        .collect();
    assert_eq!(
        states,
        [
            &digest(&on_disk(false, 'a')),
            &digest(&OnDiskState::Missing),
            &digest(&on_disk(true, 'b')),
            &digest(&OnDiskState::Missing),
        ]
    );
    let ids: BTreeSet<_> = items.iter().map(|item| &item.id).collect();
    assert_eq!(ids.len(), items.len());
}

/// Lists origins and differing fingerprint parts for `after` items that
/// do not match their `before` counterparts.
fn reopened(before: &[GateItem], after: &[GateItem]) -> Vec<(Origin, Vec<String>)> {
    let ids = |items: &[GateItem]| items.iter().map(|item| item.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(before), ids(after), "the same items");
    before
        .iter()
        .zip(after)
        .filter_map(|(old, new)| {
            let parts = old.fingerprint.differing(&new.fingerprint);
            (!parts.is_empty()).then(|| (new.origin(), parts))
        })
        .collect()
}

fn after(change: impl FnOnce(&mut Review, &mut OwnedInputs)) -> Vec<(Origin, Vec<String>)> {
    let (mut review, mut inputs) = sample();
    let before = gate_items(&review, &inputs.borrowed());
    change(&mut review, &mut inputs);
    reopened(&before, &gate_items(&review, &inputs.borrowed()))
}

fn by(origin: Origin, parts: &[&str]) -> Vec<(Origin, Vec<String>)> {
    vec![(
        origin,
        parts.iter().map(|part| (*part).to_owned()).collect(),
    )]
}

#[test]
fn unexplained_lines_reopen_on_their_content_outcomes_and_evidence() {
    let lines = |part| by(Origin::UnexplainedLines, &[part]);
    assert_eq!(
        after(|review, _| review.files[1].new_blob = Some("3".repeat(40))),
        lines("content")
    );
    assert_eq!(
        after(|review, _| review.files[1].new_mode = "100755".to_owned()),
        lines("content")
    );
    assert_eq!(
        after(|review, _| unexplained_span(review).outcome = Outcome::LineEndingChanged),
        lines("outcomes")
    );
    assert_eq!(
        after(|review, _| unexplained_span(review).provenance[0]["class"] = json!("consistent")),
        lines("outcomes")
    );
    assert_eq!(
        after(|review, _| review.edits.get_mut("e2").unwrap().turn = "a2".to_owned()),
        lines("evidence")
    );
    // An edit's payload under the same id.
    assert_eq!(
        after(
            |_, inputs| inputs.edits.get_mut("e2").unwrap()[0].new_text =
                Material::Retained("keep\nolder\n".to_owned())
        ),
        lines("evidence")
    );
    // Duplicate sightings, unnamed edits, uncounted lines, and data outside
    // the gate do not reopen these items.
    assert_eq!(
        after(|_, inputs| {
            let seen = inputs.edits.get_mut("e2").unwrap();
            seen.push(seen[0].clone());
        }),
        []
    );
    assert_eq!(
        after(|review, _| review.edits.get_mut("e1").unwrap().turn = "a2".to_owned()),
        []
    );
    assert_eq!(
        after(|_, inputs| {
            inputs
                .edits
                .insert("e1".to_owned(), vec![observed("e1", "other\n")]);
        }),
        []
    );
    assert_eq!(
        after(|review, _| review.files[1].hunks[0].spans[0].provenance[0] = json!(null)),
        []
    );
    assert_eq!(after(|review, _| review.files[1].coarse = true), []);
}

#[test]
fn not_diffed_files_reopen_on_their_content_and_kind() {
    assert_eq!(
        after(|review, _| review.files[0].old_mode = "100755".to_owned()),
        by(Origin::NotDiffed, &["content"])
    );
    assert_eq!(
        after(|review, _| review.files[0].kind = FileKindInfo::TooLarge),
        by(Origin::NotDiffed, &["kind"])
    );
}

#[test]
fn untracked_files_reopen_on_their_disk_state_and_recorded_edits() {
    let evidence = |inputs: &OwnedInputs| inputs.untracked[&PathBytes::new("src/new.rs")].clone();
    let untracked = |part| by(Origin::UntrackedRecorded, &[part]);
    assert_eq!(
        after(|_, inputs| {
            let mut changed = evidence(inputs);
            changed.on_disk = on_disk(false, 'b');
            inputs
                .untracked
                .insert(PathBytes::new("src/new.rs"), changed);
        }),
        untracked("on_disk")
    );
    // A mode change on disk.
    assert_eq!(
        after(|_, inputs| {
            let mut changed = evidence(inputs);
            changed.on_disk = on_disk(true, 'a');
            inputs
                .untracked
                .insert(PathBytes::new("src/new.rs"), changed);
        }),
        untracked("on_disk")
    );
    // An edit's payload under the same id.
    assert_eq!(
        after(|_, inputs| {
            let mut changed = evidence(inputs);
            changed.edits[0].new_text = Material::Retained("other".to_owned());
            inputs
                .untracked
                .insert(PathBytes::new("src/new.rs"), changed);
        }),
        untracked("edits")
    );
    // The same sightings in another order, or seen again.
    assert_eq!(
        after(|_, inputs| {
            let mut changed = evidence(inputs);
            changed.edits.insert(0, observed("e5", "new"));
            inputs
                .untracked
                .insert(PathBytes::new("src/new.rs"), changed);
        }),
        []
    );
}

#[test]
fn uncaptured_paths_reopen_on_their_cause_and_disk_state() {
    assert_eq!(
        after(|review, _| review.scope.not_captured[0].cause = "skip_worktree".to_owned()),
        by(Origin::NotCaptured, &["cause"])
    );
    assert_eq!(
        after(|_, inputs| {
            inputs
                .not_captured
                .insert(PathBytes::new("config.toml"), on_disk(false, 'a'));
        }),
        by(Origin::NotCaptured, &["on_disk"])
    );
}

#[test]
fn commits_and_gaps_reopen_on_their_evidence() {
    assert_eq!(
        after(|review, _| review.origins.unavailable[0].reason = "not a directory".to_owned()),
        by(Origin::CommitWithoutOrigin, &["origin"])
    );
    // An edit's payload under the same id, a compared state, and a later
    // sighting of an edit with another payload.
    assert_eq!(
        after(|_, inputs| {
            inputs.discontinuities[0].next[0].new_text = Material::Retained("other".to_owned());
        }),
        by(Origin::Discontinuity, &["gaps"])
    );
    assert_eq!(
        after(|_, inputs| inputs.discontinuities[0].prev[0].after = FileState::unknown()),
        by(Origin::Discontinuity, &["gaps"])
    );
    assert_eq!(
        after(|_, inputs| inputs.discontinuities[0].next.push(observed("e4", "third"))),
        by(Origin::Discontinuity, &["gaps"])
    );
    // The same sightings in another order, or seen again.
    assert_eq!(
        after(|_, inputs| {
            let next = &mut inputs.discontinuities[0].next;
            next.insert(0, next[0].clone());
        }),
        []
    );
}

#[test]
fn unknown_material_gives_an_unknown_part() {
    let (mut review, mut inputs) = sample();
    with_unreconciled(&mut review);
    // An untracked path that is a directory, an uncaptured path whose
    // state the CLI did not supply, and an edit behind counted lines whose
    // sightings it did not supply.
    inputs.untracked.values_mut().next().unwrap().on_disk = OnDiskState::Unknown;
    inputs.not_captured.clear();
    inputs.edits.remove("e2");
    let items = gate_items(&review, &inputs.borrowed());
    let unknown: Vec<(Origin, Vec<&str>)> = items
        .iter()
        .filter(|item| item.fingerprint.has_unknown())
        .map(|item| {
            let parts = item.fingerprint.0.iter();
            let names = parts
                .filter(|(_, part)| **part == FingerprintPart::Unknown)
                .map(|(name, _)| name.as_str())
                .collect();
            (item.origin(), names)
        })
        .collect();
    assert_eq!(
        unknown,
        vec![
            (Origin::UnexplainedLines, vec!["evidence"]),
            (Origin::UntrackedRecorded, vec!["on_disk"]),
            (Origin::NotCaptured, vec!["on_disk"]),
            (Origin::Unreconciled, vec!["evidence"]),
            (Origin::Unreconciled, vec!["evidence"]),
        ]
    );
    assert_eq!(
        item(&items, Origin::NotCaptured).detail,
        GateDetail::NotCaptured {
            cause: "assume_unchanged".to_owned(),
            on_disk: "unknown".to_owned(),
        }
    );
    // An unknown part matches nothing, not even itself.
    let unreconciled = item(&items, Origin::Unreconciled);
    assert!(!unreconciled.fingerprint.matches(&unreconciled.fingerprint));
}

#[test]
fn explanations_digest_what_the_review_shows_about_each_file() {
    let (review, inputs) = sample();
    let shown = explanations(&review, &borrowed(&inputs.edits));
    let paths: Vec<&PathBytes> = shown.iter().map(|entry| &entry.path).collect();
    assert_eq!(
        paths,
        [&PathBytes::new("logo.png"), &PathBytes::new("src/a.rs")]
    );
    // The file's entry with the edits it references, by id, each with
    // every recorded sighting of it.
    let recorded = |id: &str| Recorded {
        entry: &review.edits[id],
        sightings: &inputs.edits[id],
    };
    let edits = BTreeMap::from([("e1", recorded("e1")), ("e2", recorded("e2"))]);
    let FingerprintPart::Known(expected) = digest(&(&review.files[1], edits)) else {
        unreachable!()
    };
    assert_eq!(shown[1].digest, expected);

    let changed = |change: &dyn Fn(&mut Review, &mut Sightings)| {
        let (mut other, mut sightings) = (review.clone(), inputs.edits.clone());
        change(&mut other, &mut sightings);
        let after = explanations(&other, &borrowed(&sightings));
        assert_eq!(after.len(), shown.len());
        shown
            .iter()
            .zip(after)
            .filter(|(old, new)| old.digest != new.digest)
            .map(|(_, new)| new.path)
            .collect::<Vec<_>>()
    };
    let a = vec![PathBytes::new("src/a.rs")];
    assert_eq!(changed(&|review, _| review.files[1].coarse = true), a);
    assert_eq!(
        changed(&|review, _| review.files[1].attribution_bytes.target = "converted".to_owned()),
        a
    );
    assert_eq!(
        changed(&|review, _| review.edits.get_mut("e1").unwrap().time = "later".to_owned()),
        a
    );
    // A changed payload under the same ID changes the digest; a duplicate
    // sighting does not.
    assert_eq!(
        changed(
            &|_, sightings| sightings.get_mut("e1").unwrap()[0].new_text =
                Material::Retained("keep\nnewer\n".to_owned())
        ),
        a
    );
    assert_eq!(
        changed(&|_, sightings| {
            let seen = sightings.get_mut("e1").unwrap();
            seen.push(seen[0].clone());
        }),
        []
    );
    assert_eq!(
        changed(
            &|review, _| review.files[0].conflicting_edits.push(Conflict {
                edit: eid("e9"),
                records: vec![RecordId::new("rec-1"), RecordId::new("rec-2")],
            })
        ),
        [PathBytes::new("logo.png")]
    );
    // Command mentions do not change digests because they explain no lines.
    assert_eq!(
        changed(&|review, _| review.files[1].mentions.push(Mention {
            command: "command:1".to_owned(),
            session: "s1".to_owned(),
            turn: "a3".to_owned(),
            checkout: WORKTREE.to_owned(),
            text: "cat src/a.rs".to_owned(),
            result: None,
        })),
        []
    );
    // Unreferenced edits and data outside the files do not change digests.
    assert_eq!(
        changed(&|review, _| {
            review
                .edits
                .insert("e9".to_owned(), edit_info("2026-10-13T10:00:09.000Z"));
        }),
        []
    );
    assert_eq!(
        changed(&|review, _| review.origins.without_origin.clear()),
        []
    );
}

#[test]
fn an_edit_behind_counted_lines_recorded_again_with_another_payload_reopens_them() {
    // Another sighting of `e2` has the same ID and metadata but a different
    // payload and after-state. The review still shows the same outcomes.
    let (review, inputs) = sample();
    let mut rewritten = inputs.clone();
    rewritten
        .edits
        .insert("e2".to_owned(), vec![observed("e2", "keep\nold")]);
    assert_eq!(
        reopened(
            &gate_items(&review, &inputs.borrowed()),
            &gate_items(&review, &rewritten.borrowed())
        ),
        by(Origin::UnexplainedLines, &["evidence"])
    );
    let digests = |inputs: &OwnedInputs| {
        explanations(&review, &borrowed(&inputs.edits))
            .into_iter()
            .map(|explanation| (explanation.path, explanation.digest))
            .collect::<Vec<_>>()
    };
    let (before, after) = (digests(&inputs), digests(&rewritten));
    // `logo.png` references no edit.
    assert_eq!(before[0], after[0]);
    assert_eq!(before[1].0, after[1].0);
    assert_ne!(before[1].1, after[1].1);
}

fn limitation(counts: CaptureCounts) -> CaptureLimitation {
    CaptureLimitation {
        session: SessionId::new("s1"),
        transcript: TRANSCRIPT.to_owned(),
        from_ordinal: 0,
        to_ordinal: 9,
        counts,
    }
}

fn transcript(path: &str, capture_limitations: Vec<CaptureLimitation>) -> TranscriptInfo {
    TranscriptInfo {
        transcript: path.to_owned(),
        session: "s1".to_owned(),
        capture_limitations,
        localized: false,
    }
}

#[test]
fn capture_lists_each_transcript_with_limitations_and_their_sums() {
    let (mut review, _) = sample();
    let abandoned = |n| CaptureCounts {
        abandoned_tool_uses: n,
        ..CaptureCounts::default()
    };
    review.evidence.candidate_transcripts = vec![
        transcript(
            "/t/z.jsonl",
            vec![limitation(abandoned(1)), limitation(abandoned(2))],
        ),
        transcript("/t/clean.jsonl", Vec::new()),
        transcript("/t/a.jsonl", vec![limitation(abandoned(4))]),
    ];
    assert_eq!(
        capture(&review),
        vec![
            CaptureEntry {
                transcript: "/t/a.jsonl".to_owned(),
                counts: abandoned(4),
            },
            CaptureEntry {
                transcript: "/t/z.jsonl".to_owned(),
                counts: abandoned(3),
            },
        ]
    );
}
