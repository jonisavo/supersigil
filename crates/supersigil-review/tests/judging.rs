//! Tests the judgment rules over hand-made bases and judgments: relations
//! between saved reviews, standing, each gate item's state, completion,
//! and withdrawal plans.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::slice;
use std::time::Duration;

use common::finishes_within;
use supersigil_record::reviews::{
    Algorithms, BASIS_SCHEMA, Basis, BasisBase, BasisId, BasisScope, BasisTarget, Explanation,
    Fingerprint, FingerprintPart, GateDetail, GateItem, Judgment, JudgmentKind, PathBytes, Subject,
    TargetKind, Who, Withdrawal,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, RecordId};
use supersigil_review::judging::{
    AlreadyClosed, CompleteRefusal, FindJudgmentError, GateIndex, GateStates, ItemChange,
    ItemState, Ledger, NotClosed, PlannedWithdrawal, RelatedBasis, Relation, Uncovered,
    WithdrawRefusal, explanations_differ, related_bases, uncovered_items,
};
use supersigil_review::model::{
    ChangedEvidence, GateInfo, ItemStateInfo, JudgmentInfo, NewObservations, RelatedInfo,
    RelatedReading, StatusInfo, UncoveredInfo,
};
use supersigil_review::names::ItemNames;

/// A saved review: its basis ID and basis.
type Saved = (BasisId, Basis);

fn known(material: &str) -> FingerprintPart {
    FingerprintPart::of(material.as_bytes())
}

fn item(subject: Subject, detail: GateDetail, parts: &[(&str, FingerprintPart)]) -> GateItem {
    let parts = parts
        .iter()
        .map(|(name, part)| ((*name).to_owned(), part.clone()))
        .collect();
    GateItem::new(subject, detail, Fingerprint(parts))
}

fn path(path: &str) -> Subject {
    Subject::Path(PathBytes::new(path))
}

/// The `unexplained_lines` item of `file` whose fingerprint parts all hash
/// `material`.
fn lines(file: &str, material: &str) -> GateItem {
    item(
        path(file),
        GateDetail::UnexplainedLines {
            unattributed: 1,
            content_match_incomplete: 0,
            unresolved: 0,
            ambiguous: 0,
            line_ending: 0,
            content_match_only: 0,
        },
        &[
            ("content", known(material)),
            ("evidence", known(material)),
            ("outcomes", known(material)),
        ],
    )
}

/// The `untracked_recorded` item of `file` with on-disk state `on_disk`.
fn untracked(file: &str, on_disk: FingerprintPart) -> GateItem {
    item(
        path(file),
        GateDetail::UntrackedRecorded { edits: 1 },
        &[("edits", known("edits")), ("on_disk", on_disk)],
    )
}

/// The `unreconciled` item of `record`, whose evidence is always unknown.
fn unreconciled(record: &str) -> GateItem {
    item(
        Subject::Record(RecordId::new(record)),
        GateDetail::Unreconciled {
            reason: "locked by another writer".to_owned(),
        },
        &[("evidence", FingerprintPart::Unknown)],
    )
}

fn judgment(id: &str, basis: &BasisId, kind: JudgmentKind, at: i64) -> Judgment {
    Judgment {
        id: JudgmentId::new(id),
        basis: basis.clone(),
        kind,
        items: Vec::new(),
        reason: None,
        text: None,
        closes: BTreeMap::new(),
        withdraws: Vec::new(),
        who: Who {
            name: "Joni Savolainen".to_owned(),
            email: "joni@example.com".to_owned(),
        },
        at: UtcTime::from_unix(at),
    }
}

fn ack(id: &str, basis: &BasisId, items: &[&GateItem], at: i64) -> Judgment {
    Judgment {
        items: items.iter().map(|item| item.id.clone()).collect(),
        ..judgment(id, basis, JudgmentKind::Acknowledge, at)
    }
}

fn dismiss(id: &str, basis: &BasisId, items: &[&GateItem], at: i64) -> Judgment {
    Judgment {
        items: items.iter().map(|item| item.id.clone()).collect(),
        reason: Some("generated".to_owned()),
        ..judgment(id, basis, JudgmentKind::Dismiss, at)
    }
}

fn comment(id: &str, basis: &BasisId, at: i64) -> Judgment {
    Judgment {
        text: Some("looks fine".to_owned()),
        ..judgment(id, basis, JudgmentKind::Comment, at)
    }
}

/// A completion that counts each item closed by the named judgment.
fn complete(id: &str, basis: &BasisId, closes: &[(&GateItem, &str)], at: i64) -> Judgment {
    Judgment {
        closes: closes
            .iter()
            .map(|(item, judgment)| (item.id.clone(), JudgmentId::new(*judgment)))
            .collect(),
        ..judgment(id, basis, JudgmentKind::Complete, at)
    }
}

/// The withdrawal of `judgment`'s `item`, or of the whole judgment.
fn withdrawal(judgment: &str, item: Option<&GateItem>) -> Withdrawal {
    Withdrawal {
        judgment: JudgmentId::new(judgment),
        item: item.map(|item| item.id.clone()),
    }
}

/// An undo of each (judgment, item) pair, or whole judgment without item.
fn undo(id: &str, basis: &BasisId, withdraws: &[(&str, Option<&GateItem>)], at: i64) -> Judgment {
    Judgment {
        withdraws: withdraws
            .iter()
            .map(|(judgment, item)| withdrawal(judgment, *item))
            .collect(),
        ..judgment(id, basis, JudgmentKind::Undo, at)
    }
}

/// The withdrawals `plan` records, after checking that each comes with the
/// judgment it withdraws.
fn withdrawals(plan: &[PlannedWithdrawal<'_>]) -> Vec<Withdrawal> {
    plan.iter()
        .map(|planned| {
            assert_eq!(planned.withdrawal.judgment, planned.judgment.id);
            planned.withdrawal.clone()
        })
        .collect()
}

/// The gate states of `current` among the related reviews in `earlier`.
fn states<'a>(current: &'a Saved, earlier: &'a [Saved], ledger: &'a Ledger<'a>) -> GateStates<'a> {
    let related = related_bases(
        &current.0,
        &current.1,
        earlier.iter().map(|(id, b)| (id, b)),
    );
    GateStates::new(&current.0, &current.1, &related, ledger)
}

/// The state of each gate item, in gate order.
fn item_states<'a>(states: &GateStates<'a>) -> Vec<ItemState<'a>> {
    states
        .items()
        .iter()
        .map(|judged| judged.state.clone())
        .collect()
}

/// A working-tree review of `worktree` comparing the tree named by
/// repeating `base` with the one named by repeating `target`.
fn basis(worktree: &str, base: char, target: char, gate: Vec<GateItem>) -> Basis {
    Basis {
        schema: BASIS_SCHEMA.to_owned(),
        worktree: PathBuf::from(worktree),
        base: BasisBase {
            commit: None,
            tree: base.to_string().repeat(40),
        },
        target: BasisTarget {
            kind: TargetKind::WorkingTree,
            commit: None,
            tree: target.to_string().repeat(40),
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
        gate,
        salt: None,
    }
}

fn saved(basis: Basis) -> Saved {
    (basis.basis_id().unwrap(), basis)
}

/// `basis` reviewed as commit `commit` instead of the working tree.
fn as_commit(mut basis: Basis, commit: char) -> Basis {
    basis.target.kind = TargetKind::Commit;
    basis.target.commit = Some(commit.to_string().repeat(40));
    basis
}

/// How each of `earlier` relates to `current`, in order.
fn relations(current: &Saved, earlier: &[Saved]) -> Vec<Option<Relation>> {
    let related = related_bases(
        &current.0,
        &current.1,
        earlier.iter().map(|(id, b)| (id, b)),
    );
    earlier
        .iter()
        .map(|(id, _)| {
            related
                .iter()
                .find(|related| related.id == id)
                .map(|related| related.relation)
        })
        .collect()
}

#[test]
fn same_content_relates_reviews_from_any_worktree_and_target_kind() {
    let current = saved(basis("/work/a", '1', '2', Vec::new()));
    let earlier = [
        saved(as_commit(basis("/work/a", '1', '2', Vec::new()), 'c')),
        saved(basis("/work/b", '1', '2', Vec::new())),
        saved(as_commit(basis("/work/b", '1', '2', Vec::new()), 'c')),
    ];
    assert_eq!(
        relations(&current, &earlier),
        [Some(Relation::SameContent); 3]
    );
}

#[test]
fn code_changed_needs_the_same_worktree() {
    let current = saved(basis("/work/a", '1', '2', Vec::new()));
    let earlier = [
        saved(basis("/work/a", '1', '3', Vec::new())),
        saved(as_commit(basis("/work/a", '1', '3', Vec::new()), 'c')),
        saved(basis("/work/b", '1', '3', Vec::new())),
    ];
    assert_eq!(
        relations(&current, &earlier),
        [
            Some(Relation::CodeChanged),
            Some(Relation::CodeChanged),
            None
        ]
    );
}

#[test]
fn another_base_tree_or_scope_is_unrelated() {
    let current = saved(basis("/work/a", '1', '2', Vec::new()));
    let mut untracked = basis("/work/a", '1', '2', Vec::new());
    untracked.scope.include_untracked = vec![PathBytes::new("notes.md")];
    let mut filtered = basis("/work/a", '1', '3', Vec::new());
    filtered.scope.paths = vec![PathBytes::new("src")];
    let earlier = [
        saved(basis("/work/a", '0', '2', Vec::new())),
        saved(basis("/work/a", '0', '3', Vec::new())),
        saved(untracked),
        saved(filtered),
    ];
    assert_eq!(relations(&current, &earlier), [None; 4]);
}

#[test]
fn a_review_is_not_related_to_itself() {
    let current = saved(basis("/work/a", '1', '2', Vec::new()));
    let mut salted = current.1.clone();
    salted.salt = Some("s".to_owned());
    let earlier = [current.clone(), saved(salted)];
    assert_eq!(
        relations(&current, &earlier),
        [None, Some(Relation::SameContent)]
    );
}

#[test]
fn own_judgments_close_their_items() {
    let (a, b, c) = (lines("a.rs", "x"), lines("b.rs", "x"), lines("c.rs", "x"));
    let current = saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone(), c]));
    let judgments = [
        ack("ack-a", &current.0, &[&a], 10),
        dismiss("dismiss-b", &current.0, &[&b], 20),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        item_states(&states(&current, &[], &ledger)),
        [
            ItemState::Acknowledged(&judgments[0]),
            ItemState::Dismissed(&judgments[1]),
            ItemState::Open,
        ]
    );
}

#[test]
fn own_beats_carried() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let earlier = [saved(basis("/work/b", '1', '2', vec![a.clone()]))];
    let judgments = [
        dismiss("own", &current.0, &[&a], 10),
        ack("carried", &earlier[0].0, &[&a], 20),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        item_states(&states(&current, &earlier, &ledger)),
        [ItemState::Dismissed(&judgments[0])]
    );
}

#[test]
fn a_related_review_carries_with_a_matching_fingerprint() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let current = saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone()]));
    let earlier = [
        saved(basis("/work/b", '1', '2', vec![a.clone()])),
        saved(basis("/work/a", '1', '3', vec![b.clone()])),
    ];
    let judgments = [
        dismiss("same-content", &earlier[0].0, &[&a], 10),
        ack("code-changed", &earlier[1].0, &[&b], 20),
    ];
    let ledger = Ledger::new(&judgments);
    let states = states(&current, &earlier, &ledger);
    assert_eq!(
        item_states(&states),
        [
            ItemState::Carried(&judgments[0]),
            ItemState::Carried(&judgments[1]),
        ]
    );
    assert_eq!(states.items()[0].state.closing(), Some(&judgments[0]));
}

#[test]
fn only_a_related_review_holding_the_item_carries() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let earlier = [
        saved(basis("/work/b", '1', '3', vec![a.clone()])),
        saved(basis("/work/b", '1', '2', Vec::new())),
    ];
    let judgments = [
        ack("unrelated", &earlier[0].0, &[&a], 10),
        ack("not-in-its-gate", &earlier[1].0, &[&a], 20),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        item_states(&states(&current, &earlier, &ledger)),
        [ItemState::Open]
    );
}

/// An item's subject and detail, and each fingerprint part of its origin
/// with the reason it reopens the item.
type ReasonCase = (Subject, GateDetail, &'static [(&'static str, &'static str)]);

#[test]
fn each_differing_part_reopens_with_its_reason() {
    let cases: [ReasonCase; 6] = [
        (
            path("a.rs"),
            lines("a.rs", "x").detail,
            &[
                ("content", "file content or mode changed"),
                ("evidence", "evidence behind the lines changed"),
                ("outcomes", "line explanations changed"),
            ],
        ),
        (
            path("icon.png"),
            GateDetail::NotDiffed {
                kind: "binary".to_owned(),
            },
            &[
                ("content", "file content or mode changed"),
                ("kind", "file kind changed"),
            ],
        ),
        (
            path("new.rs"),
            GateDetail::UntrackedRecorded { edits: 1 },
            &[
                ("edits", "recorded edits changed"),
                ("on_disk", "file on disk changed"),
            ],
        ),
        (
            path("skipped.rs"),
            GateDetail::NotCaptured {
                cause: "skip_worktree".to_owned(),
                on_disk: "missing".to_owned(),
            },
            &[
                ("cause", "capture result changed"),
                ("on_disk", "file on disk changed"),
            ],
        ),
        (
            Subject::Commit("c".repeat(40)),
            GateDetail::CommitWithoutOrigin,
            &[("origin", "origin evidence changed")],
        ),
        (
            path("a.rs"),
            GateDetail::Discontinuity { gaps: 1 },
            &[("gaps", "gaps changed")],
        ),
    ];
    for (subject, detail, parts) in cases {
        let with_changed = |changed: Option<&str>| {
            let parts: Vec<(&str, FingerprintPart)> = parts
                .iter()
                .map(|(name, _)| (*name, known(if changed == Some(*name) { "y" } else { "x" })))
                .collect();
            item(subject.clone(), detail.clone(), &parts)
        };
        let before = with_changed(None);
        let earlier = [saved(basis("/work/b", '1', '2', vec![before.clone()]))];
        let judgments = [ack("earlier", &earlier[0].0, &[&before], 10)];
        let ledger = Ledger::new(&judgments);
        for (changed, reason) in parts {
            let current = saved(basis(
                "/work/a",
                '1',
                '2',
                vec![with_changed(Some(changed))],
            ));
            assert_eq!(
                item_states(&states(&current, &earlier, &ledger)),
                [ItemState::Reopened {
                    judgment: &judgments[0],
                    reasons: vec![*reason],
                }],
                "{changed}"
            );
        }
    }
}

#[test]
fn every_differing_part_gives_its_reason_in_part_order() {
    let current = saved(basis("/work/a", '1', '2', vec![lines("a.rs", "y")]));
    let before = lines("a.rs", "x");
    let earlier = [saved(basis("/work/a", '1', '3', vec![before.clone()]))];
    let judgments = [ack("earlier", &earlier[0].0, &[&before], 10)];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        item_states(&states(&current, &earlier, &ledger)),
        [ItemState::Reopened {
            judgment: &judgments[0],
            reasons: vec![
                "file content or mode changed",
                "evidence behind the lines changed",
                "line explanations changed",
            ],
        }]
    );
}

#[test]
fn an_unknown_part_never_carries() {
    let directory = untracked("vendor", FingerprintPart::Unknown);
    let record = unreconciled("r1");
    let gate = vec![directory.clone(), record.clone()];
    let current = saved(basis("/work/a", '1', '2', gate.clone()));
    let earlier = [saved(basis("/work/b", '1', '2', gate))];
    let judgments = [ack("earlier", &earlier[0].0, &[&directory, &record], 10)];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        item_states(&states(&current, &earlier, &ledger)),
        [
            ItemState::Reopened {
                judgment: &judgments[0],
                reasons: vec!["file on disk changed"],
            },
            ItemState::Reopened {
                judgment: &judgments[0],
                reasons: vec!["evidence not read"],
            },
        ]
    );
}

#[test]
fn the_most_recent_judgment_wins() {
    let (a, b, c, d) = (
        lines("a.rs", "x"),
        lines("b.rs", "x"),
        lines("c.rs", "x"),
        lines("d.rs", "x"),
    );
    let gate = vec![a.clone(), b.clone(), c.clone(), d.clone()];
    let current = saved(basis(
        "/work/a",
        '1',
        '2',
        vec![a.clone(), b.clone(), lines("c.rs", "y"), d.clone()],
    ));
    let earlier = [
        saved(basis("/work/b", '1', '2', gate.clone())),
        saved(basis("/work/c", '1', '2', gate)),
    ];
    let judgments = [
        ack("later-time", &earlier[1].0, &[&a], 30),
        dismiss("earlier-time", &earlier[0].0, &[&a], 20),
        ack("recorded-first", &earlier[1].0, &[&b], 40),
        dismiss("recorded-second", &earlier[0].0, &[&b], 40),
        dismiss("old-reopened", &earlier[0].0, &[&c], 50),
        ack("new-reopened", &earlier[1].0, &[&c], 60),
        ack("own-later", &current.0, &[&d], 30),
        dismiss("own-earlier", &current.0, &[&d], 20),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        item_states(&states(&current, &earlier, &ledger)),
        [
            ItemState::Carried(&judgments[0]),
            ItemState::Carried(&judgments[3]),
            ItemState::Reopened {
                judgment: &judgments[5],
                reasons: vec![
                    "file content or mode changed",
                    "evidence behind the lines changed",
                    "line explanations changed",
                ],
            },
            ItemState::Acknowledged(&judgments[6]),
        ]
    );
}

#[test]
fn many_large_judgments_on_related_reviews_decide_the_states_promptly() {
    // 50 reviews of the same content from other worktrees, each with one
    // acknowledgment of the same 5,000 items.
    let gate: Vec<GateItem> = (0..5000)
        .map(|k| lines(&format!("src/f{k}.rs"), "x"))
        .collect();
    let current = saved(basis("/work/now", '1', '2', gate.clone()));
    let earlier: Vec<Saved> = (0..50)
        .map(|k| saved(basis(&format!("/work/w{k}"), '1', '2', gate.clone())))
        .collect();
    let named: Vec<&GateItem> = gate.iter().collect();
    let judgments: Vec<Judgment> = earlier
        .iter()
        .zip(0..)
        .map(|((id, _), k)| ack(&format!("ack-{k}"), id, &named, k))
        .collect();

    // Checking each item against each judgment's whole list of items took
    // about 5 s here in a debug build; an index by item takes milliseconds.
    let latest = finishes_within(Duration::from_secs(2), move || {
        let ledger = Ledger::new(&judgments);
        let states = states(&current, &earlier, &ledger);
        let closing: BTreeSet<&str> = states
            .items()
            .iter()
            .map(|judged| match judged.state {
                ItemState::Carried(judgment) => judgment.id.as_str(),
                _ => "not carried",
            })
            .collect();
        closing.into_iter().map(str::to_owned).collect::<Vec<_>>()
    });

    assert_eq!(latest, ["ack-49"]);
}

#[test]
fn the_states_of_many_related_reviews_are_decided_promptly_from_shared_indexes() {
    // 200 reviews of one worktree and base, each of another target tree
    // with the same 300 items and a comment; the first also acknowledges
    // every item, which carries to all the others.
    let gate: Vec<GateItem> = (0..300)
        .map(|k| lines(&format!("src/f{k}.rs"), "x"))
        .collect();
    let reviews: Vec<Saved> = (0..200)
        .map(|k| {
            let mut basis = basis("/work/a", '1', '2', gate.clone());
            basis.target.tree = format!("{k:040x}");
            saved(basis)
        })
        .collect();
    let mut judgments: Vec<Judgment> = reviews
        .iter()
        .zip(0..)
        .map(|((id, _), k)| comment(&format!("comment-{k}"), id, k))
        .collect();
    judgments.push(ack(
        "all",
        &reviews[0].0,
        &gate.iter().collect::<Vec<_>>(),
        1000,
    ));

    // Indexing every related review's gate again for each review, and
    // looking each item up in every related gate, took about 8 s here in
    // a debug build; indexing each gate once and visiting only the reviews
    // with a judgment of the item takes milliseconds.
    let (closing, same_as_alone) = finishes_within(Duration::from_secs(2), move || {
        let ledger = Ledger::new(&judgments);
        let indexes: HashMap<&BasisId, GateIndex<'_>> = reviews
            .iter()
            .map(|(id, basis)| (id, GateIndex::new(&basis.gate)))
            .collect();
        let others = || reviews.iter().map(|(id, basis)| (id, basis));
        let closing: BTreeSet<Option<String>> = reviews
            .iter()
            .flat_map(|(id, basis)| {
                let related = related_bases(id, basis, others());
                let states = GateStates::with_indexes(id, basis, &related, &indexes, &ledger);
                let closing: Vec<_> = states
                    .items()
                    .iter()
                    .map(|judged| judged.state.closing().map(|j| j.id.as_str().to_owned()))
                    .collect();
                closing
            })
            .collect();
        let (id, basis) = &reviews[1];
        let related = related_bases(id, basis, others());
        let same_as_alone = item_states(&GateStates::new(id, basis, &related, &ledger))
            == item_states(&GateStates::with_indexes(
                id, basis, &related, &indexes, &ledger,
            ));
        (closing, same_as_alone)
    });

    assert_eq!(closing, BTreeSet::from([Some("all".to_owned())]));
    assert!(same_as_alone);
}

#[test]
fn naming_every_item_of_a_large_gate_is_checked_promptly() {
    // 30,000 items, each acknowledged on the review, all named again.
    let gate: Vec<GateItem> = (0..30_000)
        .map(|k| lines(&format!("src/f{k}.rs"), "x"))
        .collect();
    let current = saved(basis("/work/now", '1', '2', gate));
    let named: Vec<_> = current.1.gate.iter().map(|item| item.id.clone()).collect();
    let judgments = [ack(
        "all",
        &current.0,
        &current.1.gate.iter().collect::<Vec<_>>(),
        10,
    )];

    // Finding each named item by scanning the gate took about 5 s here in
    // a debug build; an index by ID takes milliseconds.
    let (closed, withdrawn) = finishes_within(Duration::from_secs(2), move || {
        let ledger = Ledger::new(&judgments);
        let states = states(&current, &[], &ledger);
        let closed = states.check_close(&named).unwrap_err().len();
        let withdrawn = states.plan_undo(&named).unwrap().len();
        (closed, withdrawn)
    });

    assert_eq!((closed, withdrawn), (30_000, 30_000));
}

#[test]
fn withdrawing_a_pair_leaves_the_judgments_other_items_standing() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let current = saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone()]));
    let earlier = [saved(basis("/work/b", '1', '2', vec![a.clone()]))];
    let judgments = [
        ack("both", &current.0, &[&a, &b], 10),
        ack("carried", &earlier[0].0, &[&a], 20),
        undo("undo-a", &current.0, &[("both", Some(&a))], 30),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        item_states(&states(&current, &earlier, &ledger)),
        [
            ItemState::Carried(&judgments[1]),
            ItemState::Acknowledged(&judgments[0]),
        ]
    );
    assert!(!ledger.stands_for(&judgments[0], &a.id));
    assert!(ledger.stands_for(&judgments[0], &b.id));
    assert!(ledger.stands(&judgments[0]));

    let mut more = judgments.to_vec();
    more.push(undo("undo-b", &current.0, &[("both", Some(&b))], 40));
    assert!(!Ledger::new(&more).stands(&more[0]));
}

#[test]
fn withdrawing_a_whole_judgment_withdraws_every_item() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let current = saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone()]));
    let judgments = [
        ack("both", &current.0, &[&a, &b], 10),
        comment("note", &current.0, 20),
        comment("kept", &current.0, 20),
        undo("undo", &current.0, &[("both", None), ("note", None)], 30),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        item_states(&states(&current, &[], &ledger)),
        [ItemState::Open, ItemState::Open]
    );
    assert!(!ledger.stands_for(&judgments[0], &b.id));
    assert!(!ledger.stands(&judgments[0]));
    assert!(!ledger.stands(&judgments[1]));
    assert!(ledger.stands(&judgments[2]));
    assert!(ledger.stands(&judgments[3]));
    assert!(!ledger.stands_for(&judgments[2], &a.id));
}

#[test]
fn the_own_completion_is_the_most_recent_standing_one_on_the_review() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let earlier = [saved(basis("/work/b", '1', '2', vec![a.clone()]))];
    let judgments = [
        ack("ack", &current.0, &[&a], 10),
        complete("standing", &current.0, &[(&a, "ack")], 20),
        complete("withdrawn", &current.0, &[(&a, "ack")], 30),
        undo("undo", &current.0, &[("withdrawn", None)], 40),
        complete("related", &earlier[0].0, &[(&a, "ack")], 50),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        states(&current, &earlier, &ledger).own_completion(),
        Some(&judgments[1])
    );
    assert_eq!(ledger.completion(&earlier[0].0), Some(&judgments[4]));

    let withdrawn = [
        judgments[0].clone(),
        judgments[2].clone(),
        judgments[3].clone(),
    ];
    let ledger = Ledger::new(&withdrawn);
    assert_eq!(states(&current, &earlier, &ledger).own_completion(), None);
}

#[test]
fn a_completion_covers_only_items_its_gate_held_as_they_are_now() {
    // Review A acknowledged the item as X; review B, of the same content,
    // acknowledged and completed it as Y. Now it is X again: A's
    // acknowledgment closes it, but B's completion does not cover it.
    let (x, y) = (lines("a.rs", "x"), lines("a.rs", "y"));
    let current = saved(basis("/work/a", '1', '2', vec![x.clone()]));
    let earlier = [
        saved(basis("/work/b", '1', '2', vec![x.clone()])),
        saved(basis("/work/c", '1', '2', vec![y.clone()])),
    ];
    let judgments = [
        ack("as-x", &earlier[0].0, &[&x], 10),
        ack("as-y", &earlier[1].0, &[&y], 20),
        complete("done-as-y", &earlier[1].0, &[(&y, "as-y")], 30),
    ];
    let ledger = Ledger::new(&judgments);
    let states = states(&current, &earlier, &ledger);
    assert_eq!(item_states(&states), [ItemState::Carried(&judgments[0])]);
    assert_eq!(
        states.check_complete(),
        Ok(BTreeMap::from([(x.id.clone(), JudgmentId::new("as-x"))]))
    );
    assert_eq!(
        uncovered_items(&current.1.gate, &earlier[1].1.gate),
        [Uncovered {
            item: &current.1.gate[0],
            change: ItemChange::Changed(vec![
                "file content or mode changed",
                "evidence behind the lines changed",
                "line explanations changed",
            ]),
        }]
    );
    assert_eq!(uncovered_items(&current.1.gate, &earlier[0].1.gate), []);
}

#[test]
fn a_gate_lacking_an_item_of_the_completed_gate_is_covered() {
    let a = lines("a.rs", "x");
    let working_tree = [a.clone(), untracked("new.rs", known("bytes"))];
    let commit = [a];
    assert_eq!(uncovered_items(&commit, &working_tree), []);
    assert_eq!(
        uncovered_items(&working_tree, &commit),
        [Uncovered {
            item: &working_tree[1],
            change: ItemChange::New,
        }]
    );
}

#[test]
fn an_item_with_an_unknown_part_is_never_covered() {
    let gate = [lines("a.rs", "x"), unreconciled("r1")];
    assert_eq!(
        uncovered_items(&gate, &gate),
        [Uncovered {
            item: &gate[1],
            change: ItemChange::Changed(vec!["evidence not read"]),
        }]
    );
}

#[test]
fn complete_is_refused_while_an_item_is_open_or_reopened() {
    let (a, b, c) = (lines("a.rs", "x"), lines("b.rs", "x"), lines("c.rs", "y"));
    let before = lines("c.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone(), c]));
    let earlier = [saved(basis("/work/b", '1', '2', vec![before.clone()]))];
    let judgments = [
        ack("ack-a", &current.0, &[&a], 10),
        Judgment {
            items: vec![b.id.clone()],
            ..comment("comment-on-b", &current.0, 20)
        },
        ack("before", &earlier[0].0, &[&before], 30),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        states(&current, &earlier, &ledger).check_complete(),
        Err(CompleteRefusal::Unclosed(vec![
            &current.1.gate[1],
            &current.1.gate[2],
        ]))
    );
}

#[test]
fn complete_is_refused_on_a_review_with_a_standing_completion() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let mut judgments = vec![
        ack("ack", &current.0, &[&a], 10),
        complete("done", &current.0, &[(&a, "ack")], 20),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        states(&current, &[], &ledger).check_complete(),
        Err(CompleteRefusal::AlreadyComplete(&judgments[1]))
    );

    judgments.push(undo("undo", &current.0, &[("done", None)], 30));
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        states(&current, &[], &ledger).check_complete(),
        Ok(BTreeMap::from([(a.id.clone(), JudgmentId::new("ack"))]))
    );
}

#[test]
fn a_completion_counts_own_and_carried_judgments() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let current = saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone()]));
    let earlier = [saved(basis("/work/a", '1', '3', vec![b.clone()]))];
    let judgments = [
        ack("own", &current.0, &[&a], 10),
        dismiss("carried", &earlier[0].0, &[&b], 20),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        states(&current, &earlier, &ledger).check_complete(),
        Ok(BTreeMap::from([
            (a.id.clone(), JudgmentId::new("own")),
            (b.id.clone(), JudgmentId::new("carried")),
        ]))
    );
}

#[test]
fn acknowledging_a_closed_item_is_refused() {
    let (a, b, c, d) = (
        lines("a.rs", "x"),
        lines("b.rs", "x"),
        lines("c.rs", "x"),
        lines("d.rs", "y"),
    );
    let before = lines("d.rs", "x");
    let current = saved(basis(
        "/work/a",
        '1',
        '2',
        vec![a.clone(), b.clone(), c.clone(), d.clone()],
    ));
    let earlier = [saved(basis(
        "/work/b",
        '1',
        '2',
        vec![b.clone(), before.clone()],
    ))];
    let judgments = [
        ack("own", &current.0, &[&a], 10),
        dismiss("carried", &earlier[0].0, &[&b], 20),
        ack("reopened", &earlier[0].0, &[&before], 30),
    ];
    let ledger = Ledger::new(&judgments);
    let states = states(&current, &earlier, &ledger);
    assert_eq!(states.check_close(&[c.id.clone(), d.id.clone()]), Ok(()));
    assert_eq!(
        states.check_close(&[b.id.clone(), c.id.clone(), a.id.clone()]),
        Err(vec![
            AlreadyClosed {
                item: &current.1.gate[1],
                judgment: &judgments[1],
                carried: true,
            },
            AlreadyClosed {
                item: &current.1.gate[0],
                judgment: &judgments[0],
                carried: false,
            },
        ])
    );
}

#[test]
fn undo_withdraws_every_judgment_closing_the_item_and_the_completions_counting_them() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let before = lines("a.rs", "w");
    let current = saved(basis("/work/c", '1', '2', vec![a.clone(), b.clone()]));
    let earlier = [
        saved(basis("/work/a", '1', '2', vec![a.clone()])),
        saved(basis("/work/c", '1', '3', vec![a.clone()])),
        saved(basis("/work/r", '1', '2', vec![before.clone()])),
        saved(basis("/work/z", '1', '2', vec![b.clone()])),
    ];
    let judgments = vec![
        ack("own", &current.0, &[&a, &b], 10),
        ack("same-content", &earlier[0].0, &[&a], 20),
        dismiss("code-changed", &earlier[1].0, &[&a], 30),
        ack("reopened", &earlier[2].0, &[&before], 40),
        complete(
            "done-same-content",
            &earlier[0].0,
            &[(&a, "same-content")],
            50,
        ),
        complete("done-here", &current.0, &[(&a, "own"), (&b, "own")], 60),
        complete("done-b", &earlier[3].0, &[(&b, "own")], 70),
    ];
    let ledger = Ledger::new(&judgments);
    let plan = states(&current, &earlier, &ledger)
        .plan_undo(&[a.id.clone(), a.id.clone()])
        .unwrap();
    assert_eq!(
        withdrawals(&plan),
        [
            withdrawal("own", Some(&a)),
            withdrawal("same-content", Some(&a)),
            withdrawal("code-changed", Some(&a)),
            withdrawal("done-same-content", None),
            withdrawal("done-here", None),
        ]
    );

    let mut after = judgments.clone();
    after.push(Judgment {
        items: vec![a.id.clone()],
        withdraws: withdrawals(&plan),
        ..judgment("undo", &current.0, JudgmentKind::Undo, 80)
    });
    let ledger = Ledger::new(&after);
    let states = states(&current, &earlier, &ledger);
    assert_eq!(
        item_states(&states),
        [
            ItemState::Reopened {
                judgment: &after[3],
                reasons: vec![
                    "file content or mode changed",
                    "evidence behind the lines changed",
                    "line explanations changed",
                ],
            },
            ItemState::Acknowledged(&after[0]),
        ]
    );
    assert_eq!(states.own_completion(), None);
    assert_eq!(ledger.completion(&earlier[3].0), Some(&after[6]));
}

#[test]
fn undoing_a_carried_judgment_withdraws_the_earlier_reviews_completion() {
    let a = lines("a.rs", "x");
    let current = saved(as_commit(basis("/work/a", '1', '2', vec![a.clone()]), 'c'));
    let earlier = [saved(basis("/work/a", '1', '2', vec![a.clone()]))];
    let judgments = [
        ack("earlier", &earlier[0].0, &[&a], 10),
        complete("done-earlier", &earlier[0].0, &[(&a, "earlier")], 20),
    ];
    let ledger = Ledger::new(&judgments);
    let plan = states(&current, &earlier, &ledger)
        .plan_undo(slice::from_ref(&a.id))
        .unwrap();
    assert_eq!(
        withdrawals(&plan),
        [
            withdrawal("earlier", Some(&a)),
            withdrawal("done-earlier", None),
        ]
    );
    assert_eq!(plan[1].judgment.basis, earlier[0].0);
}

#[test]
fn undo_of_an_item_nothing_closes_is_refused() {
    let (a, b, c) = (lines("a.rs", "x"), lines("b.rs", "y"), lines("c.rs", "x"));
    let before = lines("b.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a, b.clone(), c.clone()]));
    let earlier = [saved(basis("/work/b", '1', '2', vec![before.clone()]))];
    let judgments = [
        ack("before", &earlier[0].0, &[&before], 10),
        ack("own", &current.0, &[&c], 20),
    ];
    let ledger = Ledger::new(&judgments);
    let gate = &current.1.gate;
    assert_eq!(
        states(&current, &earlier, &ledger).plan_undo(&[
            gate[0].id.clone(),
            b.id.clone(),
            c.id.clone(),
        ]),
        Err(vec![
            NotClosed {
                item: &gate[0],
                state: ItemState::Open,
            },
            NotClosed {
                item: &gate[1],
                state: ItemState::Reopened {
                    judgment: &judgments[0],
                    reasons: vec![
                        "file content or mode changed",
                        "evidence behind the lines changed",
                        "line explanations changed",
                    ],
                },
            },
        ])
    );
}

#[test]
fn undo_by_judgment_withdraws_a_whole_comment_or_completion() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let judgments = [
        comment("note", &current.0, 10),
        ack("ack", &current.0, &[&a], 20),
        complete("done", &current.0, &[(&a, "ack")], 30),
        comment("withdrawn-note", &current.0, 40),
        undo("undo", &current.0, &[("withdrawn-note", None)], 50),
    ];
    let ledger = Ledger::new(&judgments);
    let plan = |judgment| {
        ledger
            .plan_withdrawal(judgment)
            .map(|plan| withdrawals(&plan))
    };
    assert_eq!(plan(&judgments[0]), Ok(vec![withdrawal("note", None)]));
    assert_eq!(plan(&judgments[2]), Ok(vec![withdrawal("done", None)]));
    assert_eq!(plan(&judgments[3]), Err(WithdrawRefusal::Withdrawn));
    assert_eq!(plan(&judgments[4]), Err(WithdrawRefusal::Undo));
}

#[test]
fn undo_by_judgment_withdraws_what_stands_of_an_acknowledgment() {
    let (a, b, c) = (lines("a.rs", "x"), lines("b.rs", "x"), lines("c.rs", "x"));
    let current = saved(basis(
        "/work/a",
        '1',
        '2',
        vec![a.clone(), b.clone(), c.clone()],
    ));
    let mut judgments = vec![
        ack("ack", &current.0, &[&a, &b, &c], 10),
        undo("undo-c", &current.0, &[("ack", Some(&c))], 20),
        dismiss("other", &current.0, &[&c], 30),
        complete(
            "done",
            &current.0,
            &[(&a, "ack"), (&b, "ack"), (&c, "other")],
            40,
        ),
        complete("counts-other", &current.0, &[(&c, "other")], 50),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        withdrawals(&ledger.plan_withdrawal(&judgments[0]).unwrap()),
        [
            withdrawal("ack", Some(&a)),
            withdrawal("ack", Some(&b)),
            withdrawal("done", None),
        ]
    );

    judgments.push(undo(
        "undo-rest",
        &current.0,
        &[("ack", Some(&a)), ("ack", Some(&b))],
        60,
    ));
    let ledger = Ledger::new(&judgments);
    assert_eq!(
        ledger.plan_withdrawal(&judgments[0]),
        Err(WithdrawRefusal::Withdrawn)
    );
}

#[test]
fn a_judgment_is_found_by_a_unique_prefix_of_at_least_8_characters() {
    let review = saved(basis("/work/a", '1', '2', Vec::new())).0;
    let judgments = [
        comment("4f1c9a2e-0001-4000-8000-000000000000", &review, 10),
        comment("4f1c9a2e-0002-4000-8000-000000000000", &review, 20),
        comment("77d0e5b1-0003-4000-8000-000000000000", &review, 30),
    ];
    let ledger = Ledger::new(&judgments);
    assert_eq!(ledger.find("77d0e5b1"), Ok(&judgments[2]));
    assert_eq!(ledger.find("77D0E5B1"), Ok(&judgments[2]));
    assert_eq!(ledger.find("4f1c9a2e-0002"), Ok(&judgments[1]));
    assert_eq!(
        ledger.find("4f1c9a2e-0001-4000-8000-000000000000"),
        Ok(&judgments[0])
    );
    assert_eq!(
        ledger.find("4f1c9a2e"),
        Err(FindJudgmentError::Ambiguous(vec![
            &judgments[0],
            &judgments[1],
        ]))
    );
    assert_eq!(ledger.find("77d0e5b"), Err(FindJudgmentError::TooShort));
    assert_eq!(ledger.find("deadbeef"), Err(FindJudgmentError::NoMatch));
}

/// The gate `states` shows, each path item named by its path.
fn gate_info(states: &GateStates<'_>, gate: &[GateItem]) -> GateInfo {
    let associations = BTreeMap::new();
    let names = ItemNames::new(gate, &associations, false, |path| {
        path.to_str().map(str::to_owned)
    });
    GateInfo::new(states, &names)
}

#[test]
fn the_gate_shows_each_item_by_name_with_its_state_and_judgment() {
    let (a, b, c) = (lines("a.rs", "x"), lines("b.rs", "x"), lines("c.rs", "now"));
    let d = untracked("d.rs", known("disk"));
    let current = saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone(), c, d]));
    let then = lines("c.rs", "then");
    let earlier = [saved(basis("/work/a", '1', '3', vec![then.clone()]))];
    let judgments = [
        ack("ack-a", &current.0, &[&a], 10),
        dismiss("dismiss-b", &current.0, &[&b], 20),
        ack("ack-c", &earlier[0].0, &[&then], 5),
    ];
    let ledger = Ledger::new(&judgments);

    let gate = gate_info(&states(&current, &earlier, &ledger), &current.1.gate);

    let shown: Vec<_> = gate
        .items
        .iter()
        .map(|item| {
            let judgment = item.judgment.as_ref().map(|j| j.id.as_str());
            (item.name.as_str(), item.state, judgment)
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("a.rs", ItemStateInfo::Acknowledged, Some("ack-a")),
            ("b.rs", ItemStateInfo::Dismissed, Some("dismiss-b")),
            ("c.rs", ItemStateInfo::Reopened, Some("ack-c")),
            ("d.rs", ItemStateInfo::Open, None),
        ]
    );
    // A reopened item needs a judgment again, as an open one does.
    assert_eq!(gate.open, 2);
    assert_eq!(
        gate.items[2].reopen_reasons,
        [
            "file content or mode changed",
            "evidence behind the lines changed",
            "line explanations changed",
        ]
    );
    assert_eq!(
        gate.items[1].judgment,
        Some(JudgmentInfo {
            id: JudgmentId::new("dismiss-b"),
            kind: JudgmentKind::Dismiss,
            review: current.0.clone(),
            reason: Some("generated".to_owned()),
            text: None,
            who: judgments[1].who.clone(),
            at: UtcTime::from_unix(20),
        })
    );
    let json = serde_json::to_value(&gate).unwrap();
    assert_eq!(json["open"], 2);
    let first = &json["items"][0];
    assert_eq!(first["id"], a.id.as_str());
    assert_eq!(first["origin"], "unexplained_lines");
    assert_eq!(first["subject"], serde_json::json!({"path": "a.rs"}));
    assert_eq!(first["detail"]["unattributed"], 1);
    assert_eq!(
        first["fingerprint"],
        serde_json::to_value(&a.fingerprint).unwrap()
    );
    assert_eq!(first["state"], "acknowledged");
    assert_eq!(first["judgment"]["kind"], "acknowledge");
    assert_eq!(first["judgment"]["review"], current.0.as_str());
    assert_eq!(json["items"][3]["judgment"], serde_json::Value::Null);
}

#[test]
fn the_status_is_complete_with_the_reviews_own_standing_completion() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let judgments = [
        ack("ack", &current.0, &[&a], 10),
        complete("done", &current.0, &[(&a, "ack")], 20),
        undo("undo", &current.0, &[("done", None)], 30),
    ];

    let ledger = Ledger::new(&judgments[..2]);
    let status = StatusInfo::new(&states(&current, &[], &ledger));
    assert!(status.complete);
    assert_eq!(
        status.completion,
        Some(JudgmentInfo {
            id: JudgmentId::new("done"),
            kind: JudgmentKind::Complete,
            review: current.0.clone(),
            reason: None,
            text: None,
            who: judgments[1].who.clone(),
            at: UtcTime::from_unix(20),
        })
    );

    let ledger = Ledger::new(&judgments);
    let status = StatusInfo::new(&states(&current, &[], &ledger));
    assert!(!status.complete);
    assert_eq!(status.completion, None);
}

#[test]
fn the_latest_completion_is_the_most_recent_standing_one_on_any_review_named() {
    let one = saved(basis("/work/a", '1', '2', Vec::new()));
    let two = saved(basis("/work/b", '1', '2', Vec::new()));
    let judgments = [
        complete("one-early", &one.0, &[], 10),
        complete("two", &two.0, &[], 20),
        // As recent as the one before it, but recorded later.
        complete("one-late", &one.0, &[], 20),
        complete("withdrawn", &two.0, &[], 30),
        undo("undo", &two.0, &[("withdrawn", None)], 40),
    ];
    let ledger = Ledger::new(&judgments);

    assert_eq!(
        ledger.latest_completion([&one.0, &two.0]),
        Some(&judgments[2])
    );
    assert_eq!(ledger.latest_completion([&two.0]), Some(&judgments[1]));
    assert_eq!(ledger.latest_completion([]), None);
}

/// The explanation of `file` whose digest repeats `digest`.
fn explanation(file: &str, digest: char) -> Explanation {
    Explanation {
        path: PathBytes::new(file),
        digest: digest.to_string().repeat(64),
    }
}

fn paths(paths: &[&str]) -> Vec<PathBytes> {
    paths.iter().map(|path| PathBytes::new(*path)).collect()
}

#[test]
fn explanations_differ_only_where_the_content_did_not_change() {
    let mut current = basis("/work/a", '1', '2', Vec::new());
    current.explanations = vec![
        explanation("a.rs", 'a'),
        explanation("b.rs", 'b'),
        explanation("c.rs", 'c'),
        explanation("d.rs", 'd'),
    ];
    let mut earlier = basis("/work/a", '1', '3', Vec::new());
    earlier.explanations = vec![
        explanation("a.rs", 'a'),
        explanation("b.rs", 'x'),
        explanation("c.rs", 'x'),
        explanation("gone.rs", 'x'),
    ];

    // `c.rs` changed, so its explanation differs with its content; `d.rs`
    // was not explained before.
    assert_eq!(
        explanations_differ(&current, &earlier, &paths(&["c.rs"])),
        paths(&["b.rs", "d.rs"])
    );
    assert_eq!(
        explanations_differ(&current, &earlier, &paths(&["c.rs", "d.rs", "gone.rs"])),
        paths(&["b.rs"])
    );
    assert_eq!(explanations_differ(&current, &current, &[]), []);
}

/// What the CLI read of `related` when nothing changed in the code or the
/// records.
fn unchanged(related: RelatedBasis<'_>) -> RelatedReading<'_> {
    RelatedReading {
        related,
        changed_paths: Some(Vec::new()),
        new: Vec::new(),
        capture: Vec::new(),
        gone_or_rewritten: Vec::new(),
    }
}

/// The status of `current` among the related reviews in `earlier`, with
/// what the CLI read of each from `read`.
fn status<'a>(
    current: &'a Saved,
    earlier: &'a [Saved],
    ledger: &'a Ledger<'a>,
    read: impl Fn(RelatedBasis<'a>) -> RelatedReading<'a>,
) -> StatusInfo {
    let related = related_bases(
        &current.0,
        &current.1,
        earlier.iter().map(|(id, b)| (id, b)),
    );
    let states = GateStates::new(&current.0, &current.1, &related, ledger);
    let gate = gate_info(&states, &current.1.gate);
    let readings = related.iter().map(|related| read(*related)).collect();
    StatusInfo::reviewed(&states, &current.1, readings, &gate)
}

#[test]
fn a_completion_of_the_same_content_carries_over_when_nothing_changed() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let earlier = [saved(as_commit(
        basis("/work/a", '1', '2', vec![a.clone()]),
        'c',
    ))];
    let judgments = [
        ack("ack", &earlier[0].0, &[&a], 10),
        complete("done", &earlier[0].0, &[(&a, "ack")], 20),
    ];
    let ledger = Ledger::new(&judgments);

    let status = status(&current, &earlier, &ledger, unchanged);

    assert!(status.complete);
    let done = JudgmentInfo::new(&judgments[1]);
    assert_eq!(status.completion, Some(done.clone()));
    assert_eq!(
        status.related,
        [RelatedInfo {
            review: earlier[0].0.clone(),
            relation: Relation::SameContent,
            completion: Some(done),
            changed_paths: Some(Vec::new()),
            changed_evidence: ChangedEvidence::default(),
            uncovered: Vec::new(),
        }]
    );
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(json["related"][0]["relation"], "same_content");
    assert_eq!(json["related"][0]["completion"]["id"], "done");
}

#[test]
fn a_completion_does_not_carry_over_once_evidence_changed() {
    let a = lines("a.rs", "x");
    let mut current = basis("/work/a", '1', '2', vec![a.clone()]);
    current.explanations = vec![explanation("a.rs", 'a')];
    let current = saved(current);
    let earlier = [saved(basis("/work/b", '1', '2', vec![a.clone()]))];
    let judgments = [
        ack("ack", &earlier[0].0, &[&a], 10),
        complete("done", &earlier[0].0, &[(&a, "ack")], 20),
    ];
    let ledger = Ledger::new(&judgments);
    let new = vec![NewObservations {
        session: "s1".to_owned(),
        paths: paths(&["a.rs"]),
    }];
    let capture = vec!["t1".to_owned()];
    let gone = vec![RecordId::new("r1")];

    let status = status(&current, &earlier, &ledger, |related| RelatedReading {
        new: new.clone(),
        capture: capture.clone(),
        gone_or_rewritten: gone.clone(),
        ..unchanged(related)
    });

    assert!(!status.complete);
    assert_eq!(status.completion, None);
    assert_eq!(
        status.related[0].changed_evidence,
        ChangedEvidence {
            explanations: paths(&["a.rs"]),
            new,
            capture,
            gone_or_rewritten: gone,
        }
    );
    // The item itself carries: its fingerprint matches.
    assert_eq!(status.related[0].uncovered, []);
}

#[test]
fn a_completion_does_not_carry_over_to_a_gate_it_did_not_cover() {
    // Review A acknowledged the item as X; review B, of the same content,
    // acknowledged and completed it as Y. Now it is X again.
    let (x, y) = (lines("a.rs", "x"), lines("a.rs", "y"));
    let b = lines("b.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![x.clone(), b.clone()]));
    let earlier = [
        saved(basis("/work/b", '1', '2', vec![x.clone()])),
        saved(basis("/work/c", '1', '2', vec![y.clone()])),
    ];
    let judgments = [
        ack("as-x", &earlier[0].0, &[&x], 10),
        ack("as-y", &earlier[1].0, &[&y], 20),
        complete("done-as-y", &earlier[1].0, &[(&y, "as-y")], 30),
    ];
    let ledger = Ledger::new(&judgments);

    let status = status(&current, &earlier, &ledger, unchanged);

    assert!(!status.complete);
    // The review the status is about comes first.
    assert_eq!(status.related[0].review, earlier[1].0);
    assert_eq!(
        status.related[0].uncovered,
        [
            UncoveredInfo {
                id: x.id.clone(),
                name: "a.rs".to_owned(),
                reasons: vec![
                    "file content or mode changed",
                    "evidence behind the lines changed",
                    "line explanations changed",
                ],
            },
            UncoveredInfo {
                id: b.id.clone(),
                name: "b.rs".to_owned(),
                reasons: vec!["new"],
            },
        ]
    );
    assert_eq!(status.related[1].review, earlier[0].0);
    assert_eq!(status.related[1].completion, None);
}

#[test]
fn a_completion_of_earlier_code_never_carries_over() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let earlier = [saved(basis("/work/a", '1', '3', vec![a.clone()]))];
    let judgments = [
        ack("ack", &earlier[0].0, &[&a], 10),
        complete("done", &earlier[0].0, &[(&a, "ack")], 20),
    ];
    let ledger = Ledger::new(&judgments);

    let status = status(&current, &earlier, &ledger, |related| RelatedReading {
        changed_paths: Some(paths(&["b.rs"])),
        ..unchanged(related)
    });

    assert!(!status.complete);
    assert_eq!(status.related[0].relation, Relation::CodeChanged);
    assert_eq!(status.related[0].changed_paths, Some(paths(&["b.rs"])));
}

#[test]
fn without_the_changed_paths_no_explanation_is_said_to_differ() {
    // The earlier target tree is gone: whether a.rs changed with the code
    // or only in its explanation cannot be told.
    let mut current = basis("/work/a", '1', '2', Vec::new());
    current.explanations = vec![explanation("a.rs", 'a')];
    let mut earlier = basis("/work/a", '1', '3', Vec::new());
    earlier.explanations = vec![explanation("a.rs", 'x')];
    let (current, earlier) = (saved(current), [saved(earlier)]);
    let judgments = [complete("done", &earlier[0].0, &[], 20)];
    let ledger = Ledger::new(&judgments);

    let unknown = status(&current, &earlier, &ledger, |related| RelatedReading {
        changed_paths: None,
        ..unchanged(related)
    });
    let known = status(&current, &earlier, &ledger, unchanged);

    assert_eq!(unknown.related[0].changed_paths, None);
    assert_eq!(
        unknown.related[0].changed_evidence,
        ChangedEvidence::default()
    );
    assert_eq!(
        known.related[0].changed_evidence.explanations,
        paths(&["a.rs"])
    );
}

#[test]
fn any_completion_that_carries_over_completes_the_review_whatever_was_completed_since() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    // Two reviews of the same content, completed, then one of other code.
    let earlier = [
        saved(basis("/work/b", '1', '2', vec![a.clone()])),
        saved(basis("/work/c", '1', '2', vec![a.clone()])),
        saved(basis("/work/a", '1', '3', vec![a.clone()])),
    ];
    let judgments = [
        ack("ack", &earlier[0].0, &[&a], 10),
        complete("older", &earlier[0].0, &[(&a, "ack")], 20),
        complete("newer", &earlier[1].0, &[(&a, "ack")], 30),
        complete("code-changed", &earlier[2].0, &[(&a, "ack")], 40),
    ];
    let ledger = Ledger::new(&judgments);

    let status = status(&current, &earlier, &ledger, unchanged);

    // Through the most recent completion that carries over, listed first.
    assert!(status.complete);
    assert_eq!(status.completion, Some(JudgmentInfo::new(&judgments[2])));
    assert_eq!(status.related[0].review, earlier[1].0);
    assert_eq!(status.related[1].review, earlier[0].0);
    assert_eq!(status.related[2].review, earlier[2].0);
}

#[test]
fn without_a_completion_that_carries_over_the_status_is_about_the_most_recent() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let earlier = [
        saved(basis("/work/b", '1', '2', vec![a.clone()])),
        saved(basis("/work/a", '1', '3', vec![a.clone()])),
    ];
    let judgments = [
        ack("ack", &earlier[0].0, &[&a], 10),
        complete("same-content", &earlier[0].0, &[(&a, "ack")], 20),
        complete("code-changed", &earlier[1].0, &[(&a, "ack")], 30),
    ];
    let ledger = Ledger::new(&judgments);
    let gone = vec![RecordId::new("r1")];

    // Evidence changed since both.
    let status = status(&current, &earlier, &ledger, |related| RelatedReading {
        gone_or_rewritten: gone.clone(),
        ..unchanged(related)
    });

    assert!(!status.complete);
    assert_eq!(status.completion, None);
    assert_eq!(status.related[0].review, earlier[1].0);
    assert_eq!(status.related[1].review, earlier[0].0);
}

#[test]
fn the_reviews_own_completion_completes_it_whatever_changed_since_others() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let earlier = [saved(basis("/work/a", '1', '3', vec![a.clone()]))];
    let judgments = [
        ack("ack", &current.0, &[&a], 10),
        complete("own", &current.0, &[(&a, "ack")], 20),
        complete("earlier", &earlier[0].0, &[(&a, "ack")], 30),
    ];
    let ledger = Ledger::new(&judgments);

    let status = status(&current, &earlier, &ledger, unchanged);

    assert!(status.complete);
    assert_eq!(status.completion, Some(JudgmentInfo::new(&judgments[1])));
    assert_eq!(status.related.len(), 1);
}

#[test]
fn many_related_reviews_of_other_code_give_the_status_promptly() {
    // 100 reviews of the worktree, each of other code in all 5,000 files:
    // every file changed, and no item of the current gate is covered.
    let files: Vec<String> = (0..5000).map(|k| format!("src/f{k}.rs")).collect();
    let gate = |material| files.iter().map(|file| lines(file, material)).collect();
    let mut current = basis("/work/a", '1', '2', gate("now"));
    current.explanations = files.iter().map(|file| explanation(file, 'a')).collect();
    let current = saved(current);
    // One basis under 100 ids keeps the setup cheap.
    let earlier = basis("/work/a", '1', '3', gate("then"));
    let ids: Vec<BasisId> = (0..100)
        .map(|k| serde_json::from_value(serde_json::Value::String(format!("{k:064x}"))).unwrap())
        .collect();
    let changed = paths(&files.iter().map(String::as_str).collect::<Vec<_>>());

    // Searching the gate for each uncovered item's name and the changed
    // paths for each explanation took about 15 s here in a debug build;
    // indexed, it takes under 2 s.
    let status = finishes_within(Duration::from_secs(10), move || {
        let ledger = Ledger::new(&[]);
        let related = related_bases(&current.0, &current.1, ids.iter().map(|id| (id, &earlier)));
        let states = GateStates::new(&current.0, &current.1, &related, &ledger);
        let gate = gate_info(&states, &current.1.gate);
        let readings = related
            .iter()
            .map(|&related| RelatedReading {
                changed_paths: Some(changed.clone()),
                ..unchanged(related)
            })
            .collect();
        StatusInfo::reviewed(&states, &current.1, readings, &gate)
    });

    assert_eq!(status.related.len(), 100);
    assert!(
        status.related.iter().all(|related| {
            related.uncovered.len() == 5000 && related.changed_evidence.is_empty()
        })
    );
}
