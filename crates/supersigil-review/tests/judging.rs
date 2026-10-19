//! Tests related reviews, standing judgments, gate states, completion,
//! and withdrawal plans using constructed bases and judgments.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::path::PathBuf;
use std::slice;
use std::time::Duration;

use common::finishes_within;
use supersigil_record::interpretations::AnnotationRevision;
use supersigil_record::reviews::{
    Algorithms, BASIS_SCHEMA, Basis, BasisBase, BasisDecision, BasisId, BasisScope, BasisTarget,
    CorrectionKind, DecisionRef, Explanation, Fingerprint, FingerprintPart, GateDetail, GateItem,
    ItemId, Judgment, JudgmentKind, PathBytes, Subject, TargetKind, Who, Withdrawal,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, RecordId, SessionId};
use supersigil_review::judging::{
    AlreadyClosed, Call, Earlier, FindJudgmentError, ItemChange, ItemState, JudgedReview,
    JudgedStore, Ledger, NotClosed, Planned, Refusal, Relation, Report, Uncovered, WithdrawRefusal,
    Words, explanations_differ, uncovered_items,
};
use supersigil_review::model::{
    ChangedEvidence, CompletionSource, GateInfo, ItemStateInfo, JudgmentInfo, NewObservations,
    RelatedInfo, RelatedReading, StatusInfo, UncoveredInfo,
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

fn untracked(file: &str, on_disk: FingerprintPart) -> GateItem {
    item(
        path(file),
        GateDetail::UntrackedRecorded { edits: 1 },
        &[("edits", known("edits")), ("on_disk", on_disk)],
    )
}

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
        decision: None,
        correction: None,
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

fn correction(id: &str, basis: &BasisId, at: i64) -> Judgment {
    Judgment {
        reason: Some("the plan came from the reviewer".to_owned()),
        decision: Some(DecisionRef {
            record: RecordId::new("r1"),
            session: SessionId::new("s1"),
            revision: AnnotationRevision::FIRST,
            id: "d1".to_owned(),
        }),
        correction: Some(CorrectionKind::Wrong),
        ..judgment(id, basis, JudgmentKind::Correct, at)
    }
}

fn complete(id: &str, basis: &BasisId, closes: &[(&GateItem, &str)], at: i64) -> Judgment {
    Judgment {
        closes: closes
            .iter()
            .map(|(item, judgment)| (item.id.clone(), JudgmentId::new(*judgment)))
            .collect(),
        ..judgment(id, basis, JudgmentKind::Complete, at)
    }
}

fn withdrawal(judgment: &str, item: Option<&GateItem>) -> Withdrawal {
    Withdrawal {
        judgment: JudgmentId::new(judgment),
        item: item.map(|item| item.id.clone()),
    }
}

fn undo(id: &str, basis: &BasisId, withdraws: &[(&str, Option<&GateItem>)], at: i64) -> Judgment {
    Judgment {
        withdraws: withdraws
            .iter()
            .map(|(judgment, item)| withdrawal(judgment, *item))
            .collect(),
        ..judgment(id, basis, JudgmentKind::Undo, at)
    }
}

/// Returns `plan` after checking that each withdrawal names a judgment in
/// `ledger`.
fn withdrawals(ledger: &Ledger<'_>, plan: &[Withdrawal]) -> Vec<Withdrawal> {
    for withdrawal in plan {
        assert!(
            ledger.judgment(&withdrawal.judgment).is_some(),
            "{withdrawal:?}"
        );
    }
    plan.to_vec()
}

/// The judgment `planned` records as `planned` by Joni at time 100.
fn recorded(planned: Planned) -> Judgment {
    let who = Who {
        name: "Joni Savolainen".to_owned(),
        email: "joni@example.com".to_owned(),
    };
    planned.record(JudgmentId::new("planned"), who, UtcTime::from_unix(100))
}

/// Acknowledging `items` without a reason.
fn acknowledge(items: &[ItemId]) -> Call<'_> {
    Call::Acknowledge {
        items,
        reason: None,
    }
}

/// Undoing `items` without a reason.
fn undo_items(items: &[ItemId]) -> Call<'_> {
    Call::UndoItems {
        items,
        reason: None,
    }
}

/// Undoing the judgment whose ID starts with `prefix`, without a reason.
fn undo_judgment(prefix: &str) -> Call<'_> {
    Call::UndoJudgment {
        prefix,
        reason: None,
    }
}

/// Completing without a note.
const COMPLETE: Call<'static> = Call::Complete { text: None };

/// A store of `judgments` relating reviews to the `earlier` ones.
fn store_of<'a>(judgments: &'a [Judgment], earlier: &'a [Saved]) -> JudgedStore<'a> {
    JudgedStore::new(judgments, earlier.iter().map(|(id, basis)| (id, basis)))
}

fn item_states<'a>(review: &JudgedReview<'a>) -> Vec<ItemState<'a>> {
    review
        .items()
        .iter()
        .map(|judged| judged.state.clone())
        .collect()
}

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
        annotations: Vec::new(),
        decisions: Vec::new(),
        gate,
        salt: None,
    }
}

fn saved(basis: Basis) -> Saved {
    (basis.basis_id().unwrap(), basis)
}

fn as_commit(mut basis: Basis, commit: char) -> Basis {
    basis.target.kind = TargetKind::Commit;
    basis.target.commit = Some(commit.to_string().repeat(40));
    basis
}

fn relations(current: &Saved, earlier: &[Saved]) -> Vec<Option<Relation>> {
    let store = store_of(&[], earlier);
    let mut related = Vec::new();
    store
        .review(&current.0, &current.1)
        .report_reading(&names(&current.1.gate), |earlier| {
            related = earlier
                .iter()
                .map(|earlier| (earlier.id().clone(), earlier.relation()))
                .collect::<Vec<_>>();
            Ok::<_, Infallible>(vec![RelatedReading::default(); earlier.len()])
        })
        .unwrap();
    earlier
        .iter()
        .map(|(id, _)| {
            related
                .iter()
                .find(|(related, _)| related == id)
                .map(|&(_, relation)| relation)
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
    assert_eq!(
        item_states(&store_of(&judgments, &[]).review(&current.0, &current.1)),
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
    assert_eq!(
        item_states(&store_of(&judgments, &earlier).review(&current.0, &current.1)),
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
    let store = store_of(&judgments, &earlier);
    let states = store.review(&current.0, &current.1);
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
    assert_eq!(
        item_states(&store_of(&judgments, &earlier).review(&current.0, &current.1)),
        [ItemState::Open]
    );
}

/// Three reviews of `a` and `b` over base `1`: `/work/a` at target `3`,
/// then at target `2`, and `/work/b` at target `2`, related to the second
/// only, with gate `current`.
fn chain(a: &GateItem, b: &GateItem, current: Vec<GateItem>) -> [Saved; 3] {
    [
        saved(basis("/work/a", '1', '3', vec![a.clone()])),
        saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone()])),
        saved(basis("/work/b", '1', '2', current)),
    ]
}

/// `a` acknowledged on the first review of [`chain`], `b` on the second,
/// and the second completed with `a` carried, then `extra`.
fn chained(
    a: &GateItem,
    b: &GateItem,
    reviews: &[Saved; 3],
    extra: Vec<Judgment>,
) -> Vec<Judgment> {
    let mut judgments = vec![
        ack("first", &reviews[0].0, &[a], 10),
        ack("second", &reviews[1].0, &[b], 20),
        complete("done", &reviews[1].0, &[(a, "first"), (b, "second")], 30),
    ];
    judgments.extend(extra);
    judgments
}

#[test]
fn a_completion_carries_the_judgments_it_counted_to_reviews_related_to_its_own() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let reviews = chain(&a, &b, vec![a.clone(), b.clone()]);
    let judgments = chained(&a, &b, &reviews, Vec::new());
    let store = store_of(&judgments, &reviews[..2]);
    let current = store.review(&reviews[2].0, &reviews[2].1);
    let completed = store.review(&reviews[1].0, &reviews[1].1);

    assert_eq!(
        item_states(&current),
        [
            ItemState::Carried(&judgments[0]),
            ItemState::Carried(&judgments[1]),
        ]
    );
    // On the completed review itself, `a` stays carried, not its own.
    assert_eq!(
        item_states(&completed)[0],
        ItemState::Carried(&judgments[0])
    );
}

#[test]
fn a_judgment_a_completion_counted_reopens_an_item_whose_fingerprint_differs() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let reviews = chain(&a, &b, vec![lines("a.rs", "y"), b.clone()]);
    let judgments = chained(&a, &b, &reviews, Vec::new());
    let store = store_of(&judgments, &reviews[..2]);
    let states = item_states(&store.review(&reviews[2].0, &reviews[2].1));

    assert!(
        matches!(&states[0], ItemState::Reopened { judgment, .. } if *judgment == &judgments[0]),
        "{states:?}"
    );
}

#[test]
fn a_withdrawn_completion_or_judgment_carries_nothing_through_it() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let reviews = chain(&a, &b, vec![a.clone(), b.clone()]);
    let completion = [undo("undo", &reviews[1].0, &[("done", None)], 40)];
    let pair = [undo("undo", &reviews[0].0, &[("first", Some(&a))], 40)];
    for extra in [completion, pair] {
        let judgments = chained(&a, &b, &reviews, extra.to_vec());
        assert_eq!(
            item_states(&store_of(&judgments, &reviews[..2]).review(&reviews[2].0, &reviews[2].1)),
            [ItemState::Open, ItemState::Carried(&judgments[1])]
        );
    }
}

#[test]
fn a_completion_of_the_current_review_keeps_carrying_what_it_counted() {
    // The third review is completed with `a` carried through the second's
    // completion, which is then withdrawn as a whole: the third's own
    // completion still counts `first`.
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let reviews = chain(&a, &b, vec![a.clone(), b.clone()]);
    let extra = vec![
        complete(
            "done-too",
            &reviews[2].0,
            &[(&a, "first"), (&b, "second")],
            40,
        ),
        undo("undo", &reviews[1].0, &[("done", None)], 50),
    ];
    let judgments = chained(&a, &b, &reviews, extra);
    let store = store_of(&judgments, &reviews[..2]);
    let states = store.review(&reviews[2].0, &reviews[2].1);

    assert_eq!(states.completion(), Some(&judgments[3]));
    assert_eq!(
        item_states(&states),
        [
            ItemState::Carried(&judgments[0]),
            ItemState::Carried(&judgments[1]),
        ]
    );
    assert_eq!(
        withdrawals(
            store.ledger(),
            &states
                .plan(undo_items(slice::from_ref(&a.id)))
                .map(|planned| recorded(planned).withdraws)
                .unwrap()
        ),
        [withdrawal("first", Some(&a)), withdrawal("done-too", None)]
    );
}

#[test]
fn many_large_completions_are_counted_promptly() {
    // 5 reviews of the same content from other worktrees, each with one
    // acknowledgment of the same 20,000 items and a completion counting it.
    // Checking that the acknowledgment names each counted item by scanning
    // its items took about 10 s here in a debug build; an index of the
    // standing pairs takes about 1 s.
    let gate: Vec<GateItem> = (0..20_000)
        .map(|k| lines(&format!("src/f{k}.rs"), "x"))
        .collect();
    let current = saved(basis("/work/now", '1', '2', gate.clone()));
    let earlier: Vec<Saved> = (0..5)
        .map(|k| saved(basis(&format!("/work/w{k}"), '1', '2', gate.clone())))
        .collect();
    let named: Vec<&GateItem> = gate.iter().collect();
    let judgments: Vec<Judgment> = earlier
        .iter()
        .zip(0..)
        .flat_map(|((id, _), k)| {
            let acked = format!("ack-{k}");
            let closes: Vec<(&GateItem, &str)> =
                named.iter().map(|&item| (item, acked.as_str())).collect();
            [
                ack(&acked, id, &named, 2 * k),
                complete(&format!("done-{k}"), id, &closes, 2 * k + 1),
            ]
        })
        .collect();

    let open = finishes_within(Duration::from_secs(8), move || {
        let store = store_of(&judgments, &earlier);
        let states = store.review(&current.0, &current.1);
        states
            .items()
            .iter()
            .filter(|judged| judged.state.closing().is_none())
            .count()
    });

    assert_eq!(open, 0);
}

#[test]
fn undoing_an_item_carried_through_a_completion_withdraws_its_judgment_and_the_completion() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let reviews = chain(&a, &b, vec![a.clone(), b.clone()]);
    let judgments = chained(&a, &b, &reviews, Vec::new());
    let store = store_of(&judgments, &reviews[..2]);
    let plan = store
        .review(&reviews[2].0, &reviews[2].1)
        .plan(undo_items(slice::from_ref(&a.id)))
        .map(|planned| recorded(planned).withdraws)
        .unwrap();

    assert_eq!(
        withdrawals(store.ledger(), &plan),
        [withdrawal("first", Some(&a)), withdrawal("done", None)]
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
        for (changed, reason) in parts {
            let current = saved(basis(
                "/work/a",
                '1',
                '2',
                vec![with_changed(Some(changed))],
            ));
            assert_eq!(
                item_states(&store_of(&judgments, &earlier).review(&current.0, &current.1)),
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
    assert_eq!(
        item_states(&store_of(&judgments, &earlier).review(&current.0, &current.1)),
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
    assert_eq!(
        item_states(&store_of(&judgments, &earlier).review(&current.0, &current.1)),
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
    assert_eq!(
        item_states(&store_of(&judgments, &earlier).review(&current.0, &current.1)),
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
    let latest = finishes_within(Duration::from_secs(20), move || {
        let store = store_of(&judgments, &earlier);
        let states = store.review(&current.0, &current.1);
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
    // Give 200 reviews the same worktree, base, and 300 items, with different
    // targets and one comment each. The first acknowledges all items, which
    // carries to the others.
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

    // Rebuilding related gate indexes and searching every related gate took
    // about 8 s here in a debug build. Shared gate indexes and visiting only
    // reviews with a judgment of the item take milliseconds.
    let (closing, same_as_alone) = finishes_within(Duration::from_secs(20), move || {
        let store = store_of(&judgments, &reviews);
        let closing: BTreeSet<Option<String>> = reviews
            .iter()
            .flat_map(|(id, basis)| {
                let closing: Vec<_> = store
                    .review(id, basis)
                    .items()
                    .iter()
                    .map(|judged| judged.state.closing().map(|j| j.id.as_str().to_owned()))
                    .collect();
                closing
            })
            .collect();
        // A store of only the review holding the acknowledgment gives the
        // same states.
        let (id, basis) = &reviews[1];
        let same_as_alone = item_states(&store_of(&judgments, &reviews[..1]).review(id, basis))
            == item_states(&store.review(id, basis));
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
    let (closed, withdrawn) = finishes_within(Duration::from_secs(20), move || {
        let store = store_of(&judgments, &[]);
        let states = store.review(&current.0, &current.1);
        let Err(Refusal::AlreadyClosed(closed)) = states.plan(acknowledge(&named)) else {
            panic!("every named item is closed");
        };
        let closed = closed.len();
        let withdrawn = states
            .plan(undo_items(&named))
            .map(|planned| recorded(planned).withdraws)
            .unwrap()
            .len();
        (closed, withdrawn)
    });

    assert_eq!((closed, withdrawn), (30_000, 30_000));
}

#[test]
fn undoing_a_judgment_of_every_item_of_a_large_gate_is_planned_promptly() {
    // Plan undo for a 30,000-item acknowledgment, then retry after every
    // item pair has been withdrawn.
    let gate: Vec<GateItem> = (0..30_000)
        .map(|k| lines(&format!("src/f{k}.rs"), "x"))
        .collect();
    let current = saved(basis("/work/now", '1', '2', gate));
    let named: Vec<&GateItem> = current.1.gate.iter().collect();
    let standing = vec![ack("acknowledged-all", &current.0, &named, 10)];
    let pairs: Vec<_> = named
        .iter()
        .map(|&item| ("acknowledged-all", Some(item)))
        .collect();
    let mut withdrawn = standing.clone();
    withdrawn.push(undo("undo-all", &current.0, &pairs, 20));

    let started = std::time::Instant::now();
    let (planned, refused) = finishes_within(Duration::from_secs(20), move || {
        let withdrawn_count = |judgments: &[Judgment]| match store_of(judgments, &[])
            .review(&current.0, &current.1)
            .plan(undo_judgment("acknowledged-all"))
        {
            Ok(planned) => Ok(recorded(planned).withdraws.len()),
            Err(Refusal::NotWithdrawable(_, refused)) => Err(refused),
            Err(other) => panic!("{other:?}"),
        };
        (withdrawn_count(&standing), withdrawn_count(&withdrawn))
    });
    eprintln!("planned in {:?}", started.elapsed());

    assert_eq!(planned, Ok(30_000));
    assert_eq!(refused, Err(WithdrawRefusal::Withdrawn));
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
    // The acknowledgment no longer closes `a`, which the carried one does,
    // but still closes `b`.
    assert_eq!(
        item_states(&store_of(&judgments, &earlier).review(&current.0, &current.1)),
        [
            ItemState::Carried(&judgments[1]),
            ItemState::Acknowledged(&judgments[0]),
        ]
    );
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
        item_states(&store_of(&judgments, &[]).review(&current.0, &current.1)),
        [ItemState::Open, ItemState::Open]
    );
    assert!(!ledger.stands(&judgments[0]));
    assert!(!ledger.stands(&judgments[1]));
    assert!(ledger.stands(&judgments[2]));
    assert!(ledger.stands(&judgments[3]));
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
        store_of(&judgments, &earlier)
            .review(&current.0, &current.1)
            .completion(),
        Some(&judgments[1])
    );
    assert_eq!(ledger.completion(&earlier[0].0), Some(&judgments[4]));

    let withdrawn = [
        judgments[0].clone(),
        judgments[2].clone(),
        judgments[3].clone(),
    ];
    assert_eq!(
        store_of(&withdrawn, &earlier)
            .review(&current.0, &current.1)
            .completion(),
        None
    );
}

#[test]
fn a_completion_covers_only_items_its_gate_held_as_they_are_now() {
    // A acknowledged X; B reviewed the same content, acknowledged Y, and
    // completed. With X restored, A closes the item but B's gate does not
    // cover it.
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
    let store = store_of(&judgments, &earlier);
    let states = store.review(&current.0, &current.1);
    assert_eq!(item_states(&states), [ItemState::Carried(&judgments[0])]);
    assert_eq!(
        states
            .plan(COMPLETE)
            .map(|planned| recorded(planned).closes),
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
    assert_eq!(
        store_of(&judgments, &earlier)
            .review(&current.0, &current.1)
            .plan(COMPLETE)
            .map(|planned| recorded(planned).closes),
        Err(Refusal::Unclosed(vec![
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
    assert_eq!(
        store_of(&judgments, &[])
            .review(&current.0, &current.1)
            .plan(COMPLETE)
            .map(|planned| recorded(planned).closes),
        Err(Refusal::AlreadyComplete(&judgments[1]))
    );

    judgments.push(undo("undo", &current.0, &[("done", None)], 30));
    assert_eq!(
        store_of(&judgments, &[])
            .review(&current.0, &current.1)
            .plan(COMPLETE)
            .map(|planned| recorded(planned).closes),
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
    assert_eq!(
        store_of(&judgments, &earlier)
            .review(&current.0, &current.1)
            .plan(COMPLETE)
            .map(|planned| recorded(planned).closes),
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
    let store = store_of(&judgments, &earlier);
    let states = store.review(&current.0, &current.1);
    assert_eq!(
        states
            .plan(acknowledge(&[c.id.clone(), d.id.clone()]))
            .map(drop),
        Ok(())
    );
    assert_eq!(
        states
            .plan(acknowledge(&[b.id.clone(), c.id.clone(), a.id.clone()]))
            .map(drop),
        Err(Refusal::AlreadyClosed(vec![
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
        ]))
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
    let store = store_of(&judgments, &earlier);
    let plan = store
        .review(&current.0, &current.1)
        .plan(undo_items(&[a.id.clone(), a.id.clone()]))
        .map(|planned| recorded(planned).withdraws)
        .unwrap();
    assert_eq!(
        withdrawals(store.ledger(), &plan),
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
        withdraws: plan,
        ..judgment("undo", &current.0, JudgmentKind::Undo, 80)
    });
    let ledger = Ledger::new(&after);
    let store = store_of(&after, &earlier);
    let states = store.review(&current.0, &current.1);
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
    assert_eq!(states.completion(), None);
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
    let store = store_of(&judgments, &earlier);
    let plan = store
        .review(&current.0, &current.1)
        .plan(undo_items(slice::from_ref(&a.id)))
        .map(|planned| recorded(planned).withdraws)
        .unwrap();
    assert_eq!(
        withdrawals(store.ledger(), &plan),
        [
            withdrawal("earlier", Some(&a)),
            withdrawal("done-earlier", None),
        ]
    );
    assert_eq!(
        ledger
            .judgment(&plan[1].judgment)
            .map(|judgment| &judgment.basis),
        Some(&earlier[0].0)
    );
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
    let gate = &current.1.gate;
    assert_eq!(
        store_of(&judgments, &earlier)
            .review(&current.0, &current.1)
            .plan(undo_items(&[
                gate[0].id.clone(),
                b.id.clone(),
                c.id.clone(),
            ]))
            .map(|planned| recorded(planned).withdraws),
        Err(Refusal::NotClosed(vec![
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
        ]))
    );
}

#[test]
fn undo_by_judgment_withdraws_a_whole_comment_correction_or_completion() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let judgments = [
        comment("standing-note", &current.0, 10),
        ack("acknowledgment", &current.0, &[&a], 20),
        complete("completion", &current.0, &[(&a, "acknowledgment")], 30),
        comment("withdrawn-note", &current.0, 40),
        undo("undo-of-note", &current.0, &[("withdrawn-note", None)], 50),
        correction("standing-correction", &current.0, 60),
        correction("withdrawn-correction", &current.0, 70),
        undo(
            "undo-of-correction",
            &current.0,
            &[("withdrawn-correction", None)],
            80,
        ),
    ];
    let store = store_of(&judgments, &[]);
    let review = store.review(&current.0, &current.1);
    let undone = |prefix| {
        review
            .plan(undo_judgment(prefix))
            .map(|planned| withdrawals(store.ledger(), &recorded(planned).withdraws))
    };
    assert_eq!(
        undone("standing-note"),
        Ok(vec![withdrawal("standing-note", None)])
    );
    assert_eq!(
        undone("completion"),
        Ok(vec![withdrawal("completion", None)])
    );
    assert_eq!(
        undone("withdrawn-note"),
        Err(Refusal::NotWithdrawable(
            &judgments[3],
            WithdrawRefusal::Withdrawn
        ))
    );
    assert_eq!(
        undone("undo-of-note"),
        Err(Refusal::NotWithdrawable(
            &judgments[4],
            WithdrawRefusal::Undo
        ))
    );
    // A correction names no items, so it stands and is withdrawn whole.
    assert!(store.ledger().stands(&judgments[5]));
    assert_eq!(
        undone("standing-correction"),
        Ok(vec![withdrawal("standing-correction", None)])
    );
    assert!(!store.ledger().stands(&judgments[6]));
    assert_eq!(
        undone("withdrawn-correction"),
        Err(Refusal::NotWithdrawable(
            &judgments[6],
            WithdrawRefusal::Withdrawn
        ))
    );
}

#[test]
fn undo_by_judgment_reaches_only_what_the_review_holds() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    // The same content in another worktree, whose acknowledgment carries.
    let related = saved(basis("/work/b", '1', '2', vec![a.clone()]));
    let unrelated = saved(basis("/work/c", '3', '4', vec![b.clone()]));
    let judgments = [
        ack("carried-acknowledgment", &related.0, &[&a], 10),
        comment("related-note", &related.0, 20),
        ack("unrelated-acknowledgment", &unrelated.0, &[&b], 30),
    ];
    let earlier = [related, unrelated];
    let store = store_of(&judgments, &earlier);
    let review = store.review(&current.0, &current.1);
    let undone = |prefix| {
        review
            .plan(undo_judgment(prefix))
            .map(|planned| recorded(planned).withdraws)
    };
    assert_eq!(
        undone("carried-acknowledgment"),
        Ok(vec![withdrawal("carried-acknowledgment", Some(&a))])
    );
    assert_eq!(
        undone("related-note"),
        Err(Refusal::OtherReview(&judgments[1]))
    );
    assert_eq!(
        undone("unrelated-acknowledgment"),
        Err(Refusal::OtherReview(&judgments[2]))
    );
}

#[test]
fn undo_by_judgment_reaches_an_older_carried_closer_the_state_does_not_show() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    // An earlier review of this worktree's code, and a review of the same
    // content in another worktree; both acknowledge `a.rs`.
    let earlier = saved(basis("/work/1", '1', '2', vec![a.clone()]));
    let same = saved(basis("/work/2", '1', '3', vec![a.clone(), b.clone()]));
    let current = saved(basis("/work/1", '1', '3', vec![a.clone(), b]));
    let judgments = [
        ack("older-acknowledgment", &earlier.0, &[&a], 10),
        ack("newer-acknowledgment", &same.0, &[&a], 20),
    ];
    let related = [earlier, same];
    let store = store_of(&judgments, &related);
    let review = store.review(&current.0, &current.1);
    assert_eq!(
        review.items()[0]
            .state
            .closing()
            .map(|judgment| &judgment.id),
        Some(&judgments[1].id)
    );
    assert_eq!(
        review
            .plan(undo_judgment("older-acknowledgment"))
            .map(|planned| recorded(planned).withdraws),
        Ok(vec![withdrawal("older-acknowledgment", Some(&a))])
    );
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
        ack("acknowledgment", &current.0, &[&a, &b, &c], 10),
        undo("undo-of-c", &current.0, &[("acknowledgment", Some(&c))], 20),
        dismiss("dismissal", &current.0, &[&c], 30),
        complete(
            "completion",
            &current.0,
            &[
                (&a, "acknowledgment"),
                (&b, "acknowledgment"),
                (&c, "dismissal"),
            ],
            40,
        ),
        complete("counts-the-dismissal", &current.0, &[(&c, "dismissal")], 50),
    ];
    let store = store_of(&judgments, &[]);
    assert_eq!(
        store
            .review(&current.0, &current.1)
            .plan(undo_judgment("acknowledgment"))
            .map(|planned| withdrawals(store.ledger(), &recorded(planned).withdraws)),
        Ok(vec![
            withdrawal("acknowledgment", Some(&a)),
            withdrawal("acknowledgment", Some(&b)),
            withdrawal("completion", None),
        ])
    );

    judgments.push(undo(
        "undo-of-the-rest",
        &current.0,
        &[("acknowledgment", Some(&a)), ("acknowledgment", Some(&b))],
        60,
    ));
    assert_eq!(
        store_of(&judgments, &[])
            .review(&current.0, &current.1)
            .plan(undo_judgment("acknowledgment"))
            .map(drop),
        Err(Refusal::NotWithdrawable(
            &judgments[0],
            WithdrawRefusal::Withdrawn
        ))
    );
}

#[test]
fn a_judgment_is_found_by_a_unique_prefix_of_at_least_8_characters() {
    let current = saved(basis("/work/a", '1', '2', Vec::new()));
    let judgments = [
        comment("4f1c9a2e-0001-4000-8000-000000000000", &current.0, 10),
        comment("4f1c9a2e-0002-4000-8000-000000000000", &current.0, 20),
        comment("77d0e5b1-0003-4000-8000-000000000000", &current.0, 30),
    ];
    let store = store_of(&judgments, &[]);
    let review = store.review(&current.0, &current.1);
    let found = |prefix| match review.plan(undo_judgment(prefix)) {
        Ok(planned) => Ok(recorded(planned).withdraws[0].judgment.clone()),
        Err(Refusal::NoJudgment(error)) => Err(error),
        Err(other) => panic!("{other:?}"),
    };
    assert_eq!(found("77d0e5b1"), Ok(judgments[2].id.clone()));
    assert_eq!(found("77D0E5B1"), Ok(judgments[2].id.clone()));
    assert_eq!(found("4f1c9a2e-0002"), Ok(judgments[1].id.clone()));
    assert_eq!(
        found("4f1c9a2e-0001-4000-8000-000000000000"),
        Ok(judgments[0].id.clone())
    );
    assert_eq!(
        found("4f1c9a2e"),
        Err(FindJudgmentError::Ambiguous(vec![
            &judgments[0],
            &judgments[1],
        ]))
    );
    assert_eq!(found("77d0e5b"), Err(FindJudgmentError::TooShort));
    assert_eq!(found("deadbeef"), Err(FindJudgmentError::NoMatch));
}

fn names(gate: &[GateItem]) -> ItemNames<'_> {
    ItemNames::new(gate, BTreeMap::new(), false, |path| {
        path.to_str().map(str::to_owned)
    })
}

fn gate_info(review: &JudgedReview<'_>, gate: &[GateItem]) -> GateInfo {
    review.report(&names(gate)).gate
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

    let gate = gate_info(
        &store_of(&judgments, &earlier).review(&current.0, &current.1),
        &current.1.gate,
    );

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
            decision: None,
            correction: None,
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
fn a_judgments_details_show_a_corrections_decision_and_kind() {
    let current = saved(basis("/work/a", '1', '2', Vec::new()));
    let corrected = JudgmentInfo::new(&correction("correction", &current.0, 10));
    assert_eq!(
        corrected.decision,
        Some(DecisionRef {
            record: RecordId::new("r1"),
            session: SessionId::new("s1"),
            revision: AnnotationRevision::FIRST,
            id: "d1".to_owned(),
        })
    );
    assert_eq!(corrected.correction, Some(CorrectionKind::Wrong));
    let json = serde_json::to_value(&corrected).unwrap();
    assert_eq!(
        json["decision"],
        serde_json::json!({"record": "r1", "session": "s1", "revision": 1, "id": "d1"})
    );
    assert_eq!(json["correction"], "wrong");

    let acknowledged = JudgmentInfo::new(&ack("ack", &current.0, &[], 10));
    assert_eq!(
        (acknowledged.decision.as_ref(), acknowledged.correction),
        (None, None)
    );
    let json = serde_json::to_value(&acknowledged).unwrap();
    assert_eq!(json.get("decision"), None);
    assert_eq!(json.get("correction"), None);
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

    let status = store_of(&judgments[..2], &[])
        .review(&current.0, &current.1)
        .report(&names(&current.1.gate))
        .status;
    assert!(status.complete);
    assert_eq!(status.source, CompletionSource::Own);
    assert_eq!(
        status.completion,
        Some(JudgmentInfo {
            id: JudgmentId::new("done"),
            kind: JudgmentKind::Complete,
            review: current.0.clone(),
            reason: None,
            text: None,
            decision: None,
            correction: None,
            who: judgments[1].who.clone(),
            at: UtcTime::from_unix(20),
        })
    );

    let status = store_of(&judgments, &[])
        .review(&current.0, &current.1)
        .report(&names(&current.1.gate))
        .status;
    assert!(!status.complete);
    assert_eq!(status.completion, None);
    assert_eq!(status.source, CompletionSource::NotChecked);
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

fn unchanged(_: Earlier<'_>) -> RelatedReading {
    RelatedReading {
        changed_paths: Some(Vec::new()),
        ..RelatedReading::default()
    }
}

/// The report on `current` with `judgments`, reading each of the related
/// `earlier` reviews with `read`.
fn reported(
    current: &Saved,
    earlier: &[Saved],
    judgments: &[Judgment],
    read: impl Fn(Earlier<'_>) -> RelatedReading,
) -> Report {
    store_of(judgments, earlier)
        .review(&current.0, &current.1)
        .report_reading(&names(&current.1.gate), |related| {
            Ok::<_, Infallible>(related.iter().map(|&related| read(related)).collect())
        })
        .unwrap()
}

fn status(
    current: &Saved,
    earlier: &[Saved],
    judgments: &[Judgment],
    read: impl Fn(Earlier<'_>) -> RelatedReading,
) -> StatusInfo {
    reported(current, earlier, judgments, read).status
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

    let status = status(&current, &earlier, &judgments, unchanged);

    assert!(status.complete);
    let done = JudgmentInfo::new(&judgments[1]);
    assert_eq!(status.completion, Some(done.clone()));
    assert_eq!(
        status.source,
        CompletionSource::Carried(earlier[0].0.clone())
    );
    assert_eq!(
        status.related,
        [RelatedInfo {
            review: earlier[0].0.clone(),
            relation: Relation::SameContent,
            completion: Some(done),
            changed_paths: Some(Vec::new()),
            changed_evidence: ChangedEvidence::default(),
            interpretation: Vec::new(),
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
    let new = vec![NewObservations {
        session: "s1".to_owned(),
        paths: paths(&["a.rs"]),
    }];
    let capture = vec!["t1".to_owned()];
    let gone = vec![RecordId::new("r1")];

    let status = status(&current, &earlier, &judgments, |related| RelatedReading {
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

    let status = status(&current, &earlier, &judgments, unchanged);

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

    let status = status(&current, &earlier, &judgments, |related| RelatedReading {
        changed_paths: Some(paths(&["b.rs"])),
        ..unchanged(related)
    });

    assert!(!status.complete);
    assert_eq!(status.related[0].relation, Relation::CodeChanged);
    assert_eq!(status.related[0].changed_paths, Some(paths(&["b.rs"])));
}

#[test]
fn without_the_changed_paths_no_explanation_is_said_to_differ() {
    // Without the earlier target tree, code and explanation changes in
    // a.rs cannot be separated.
    let mut current = basis("/work/a", '1', '2', Vec::new());
    current.explanations = vec![explanation("a.rs", 'a')];
    let mut earlier = basis("/work/a", '1', '3', Vec::new());
    earlier.explanations = vec![explanation("a.rs", 'x')];
    let (current, earlier) = (saved(current), [saved(earlier)]);
    let judgments = [complete("done", &earlier[0].0, &[], 20)];

    let unknown = status(&current, &earlier, &judgments, |related| RelatedReading {
        changed_paths: None,
        ..unchanged(related)
    });
    let known = status(&current, &earlier, &judgments, unchanged);

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

    let status = status(&current, &earlier, &judgments, unchanged);

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
    let gone = vec![RecordId::new("r1")];

    // Evidence changed since both.
    let status = status(&current, &earlier, &judgments, |related| RelatedReading {
        gone_or_rewritten: gone.clone(),
        ..unchanged(related)
    });

    assert!(!status.complete);
    assert_eq!(status.completion, None);
    assert_eq!(status.source, CompletionSource::Absent);
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

    let status = status(&current, &earlier, &judgments, unchanged);

    assert!(status.complete);
    assert_eq!(status.completion, Some(JudgmentInfo::new(&judgments[1])));
    assert_eq!(status.source, CompletionSource::Own);
    assert_eq!(status.related.len(), 1);
}

#[test]
fn many_related_reviews_of_other_code_give_the_status_promptly() {
    // Give 100 worktree reviews different code in all 5,000 files, so no
    // current gate item is covered.
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

    // Searching the gate for every uncovered name and changed paths for
    // every explanation took about 15 s here in a debug build. Indexes reduce
    // this to under 2 s.
    let status = finishes_within(Duration::from_secs(10), move || {
        let store = JudgedStore::new(&[], ids.iter().map(|id| (id, &earlier)));
        store
            .review(&current.0, &current.1)
            .report_reading(&names(&current.1.gate), |related| {
                let readings = related
                    .iter()
                    .map(|&related| RelatedReading {
                        changed_paths: Some(changed.clone()),
                        ..unchanged(related)
                    })
                    .collect();
                Ok::<_, Infallible>(readings)
            })
            .unwrap()
            .status
    });

    assert_eq!(status.related.len(), 100);
    assert!(
        status.related.iter().all(|related| {
            related.uncovered.len() == 5000 && related.changed_evidence.is_empty()
        })
    );
}

#[test]
fn a_completion_whose_closing_judgment_no_longer_stands_does_not_count() {
    // An undo always withdraws the completions counting a pair it
    // withdraws; this history skips that.
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let mut judgments = vec![
        ack("ack", &current.0, &[&a], 10),
        complete("done", &current.0, &[(&a, "ack")], 20),
        undo("undo", &current.0, &[("ack", Some(&a))], 30),
    ];
    let store = store_of(&judgments, &[]);
    let review = store.review(&current.0, &current.1);

    assert_eq!(review.completion(), None);
    let status = review.report(&names(&current.1.gate)).status;
    assert!(!status.complete);
    assert_eq!(status.source, CompletionSource::NotChecked);
    assert_eq!(
        review
            .plan(COMPLETE)
            .map(|planned| recorded(planned).closes),
        Err(Refusal::Unclosed(vec![&current.1.gate[0]]))
    );

    // Once the item closes again, a new completion is allowed.
    judgments.push(ack("again", &current.0, &[&a], 40));
    let store = store_of(&judgments, &[]);
    assert_eq!(
        store
            .review(&current.0, &current.1)
            .plan(COMPLETE)
            .map(|planned| recorded(planned).closes),
        Ok(BTreeMap::from([(a.id.clone(), JudgmentId::new("again"))]))
    );
}

#[test]
fn a_completion_closing_too_little_does_not_count() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let current = saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone()]));
    let judgments = [
        ack("ack", &current.0, &[&a, &b], 10),
        complete("done", &current.0, &[(&a, "ack")], 20),
    ];
    let store = store_of(&judgments, &[]);
    let review = store.review(&current.0, &current.1);

    assert_eq!(review.completion(), None);
    assert_eq!(
        review
            .plan(COMPLETE)
            .map(|planned| recorded(planned).closes),
        Ok(BTreeMap::from([
            (a.id.clone(), JudgmentId::new("ack")),
            (b.id.clone(), JudgmentId::new("ack")),
        ]))
    );
}

#[test]
fn a_completion_that_does_not_count_does_not_carry() {
    let a = lines("a.rs", "x");
    let current = saved(basis("/work/a", '1', '2', vec![a.clone()]));
    let earlier = [saved(basis("/work/b", '1', '2', vec![a.clone()]))];
    let judgments = [
        ack("ack", &current.0, &[&a], 10),
        ack("earlier", &earlier[0].0, &[&a], 10),
        complete("done", &earlier[0].0, &[(&a, "earlier")], 20),
        undo("undo", &earlier[0].0, &[("earlier", Some(&a))], 30),
    ];

    let status = status(&current, &earlier, &judgments, unchanged);

    assert!(!status.complete);
    assert_eq!(status.completion, None);
    assert_eq!(status.source, CompletionSource::Absent);
}

#[test]
fn each_call_records_what_its_kind_needs() {
    let (a, b) = (lines("a.rs", "x"), lines("b.rs", "x"));
    let current = saved(basis("/work/a", '1', '2', vec![a.clone(), b.clone()]));
    let both = [a.id.clone(), b.id.clone()];
    let none: [Judgment; 0] = [];
    let store = store_of(&none, &[]);
    let fresh = store.review(&current.0, &current.1);

    let planned = fresh
        .plan(Call::Acknowledge {
            items: &both[..1],
            reason: Words::new("seen"),
        })
        .unwrap();
    assert!(planned.first());
    assert_eq!(
        recorded(planned),
        Judgment {
            items: vec![a.id.clone()],
            reason: Some("seen".to_owned()),
            ..judgment("planned", &current.0, JudgmentKind::Acknowledge, 100)
        }
    );
    let reason = Words::new("not ours").unwrap();
    assert_eq!(
        fresh
            .plan(Call::Dismiss {
                items: &both[1..],
                reason,
            })
            .map(recorded),
        Ok(Judgment {
            items: vec![b.id.clone()],
            reason: Some("not ours".to_owned()),
            ..judgment("planned", &current.0, JudgmentKind::Dismiss, 100)
        })
    );
    assert_eq!(
        fresh
            .plan(Call::Comment {
                item: Some(&a.id),
                text: Words::new("look again").unwrap(),
            })
            .map(recorded),
        Ok(Judgment {
            items: vec![a.id.clone()],
            text: Some("look again".to_owned()),
            ..judgment("planned", &current.0, JudgmentKind::Comment, 100)
        })
    );

    let judgments = [
        ack("ack-a", &current.0, &[&a], 10),
        dismiss("dismiss-b", &current.0, &[&b], 20),
    ];
    let store = store_of(&judgments, &[]);
    let judged = store.review(&current.0, &current.1);
    let planned = judged
        .plan(Call::Complete {
            text: Words::new("done"),
        })
        .unwrap();
    assert!(!planned.first());
    assert_eq!(
        recorded(planned),
        Judgment {
            text: Some("done".to_owned()),
            closes: BTreeMap::from([
                (a.id.clone(), JudgmentId::new("ack-a")),
                (b.id.clone(), JudgmentId::new("dismiss-b")),
            ]),
            ..judgment("planned", &current.0, JudgmentKind::Complete, 100)
        }
    );
    assert_eq!(
        judged
            .plan(Call::Comment {
                item: None,
                text: Words::new("for the review").unwrap(),
            })
            .map(|planned| recorded(planned).items),
        Ok(Vec::new())
    );
}

/// Two annotated sessions whose ids share the prefix `3f2`.
const SESSIONS: [&str; 2] = [
    "3f2a9c1e-5b7d-4e21-9f3a-0c6d8e1f2a3b",
    "3f2b0d4c-8e6f-4a13-b2c5-7d9e1f3a5b6c",
];

/// A decision a review shows: `id` of `SESSIONS[session]` at `revision` in
/// `record`, named `name`.
fn shown(record: &str, session: usize, revision: u32, id: &str, name: &str) -> BasisDecision {
    BasisDecision {
        decision: DecisionRef {
            record: RecordId::new(record),
            session: SessionId::new(SESSIONS[session]),
            revision: AnnotationRevision::new(revision),
            id: id.to_owned(),
        },
        name: name.to_owned(),
    }
}

/// A review of `a.rs` showing `d1` of both [`SESSIONS`], each named with
/// its session's short id, and `d2` of the second, qualified because the
/// first session has a `d2` this review does not show.
fn annotated(a: &GateItem) -> Saved {
    let mut basis = basis("/work/a", '1', '2', vec![a.clone()]);
    basis.decisions = vec![
        shown("r1", 0, 2, "d1", "3f2a9c1e:d1"),
        shown("r2", 1, 1, "d1", "3f2b0d4c:d1"),
        shown("r2", 1, 1, "d2", "3f2b0d4c:d2"),
    ];
    saved(basis)
}

/// The reference a correction of `decision` records.
fn reference(decision: &BasisDecision) -> DecisionRef {
    decision.decision.clone()
}

/// Correcting the decision named `decision` with `kind` and `words`.
fn correct<'c>(decision: &'c str, kind: CorrectionKind, words: &'c str) -> Call<'c> {
    Call::Correct {
        decision,
        kind,
        words: Words::new(words).unwrap(),
    }
}

#[test]
fn a_displayed_decision_name_corrects_the_decision_shown_beside_it() {
    // Names qualify by the session's escaped form, so the name a review
    // prints beside a decision is the one saved for it, and typing it
    // addresses that decision, not another whose raw id reads the same.
    let a = lines("a.rs", "x");
    let mut basis = basis("/work/a", '1', '2', vec![a]);
    let real = SessionId::new("abc\u{1b}defg-real");
    let literal = SessionId::new(r"abc\x1bdefg-literal");
    basis.decisions = [(&real, r"abc\x1bdefg-r:d"), (&literal, r"abc\x1bdefg-l:d")]
        .map(|(session, name)| BasisDecision {
            decision: DecisionRef {
                record: RecordId::new("r1"),
                session: session.clone(),
                revision: AnnotationRevision::new(1),
                id: "d".to_owned(),
            },
            name: name.to_owned(),
        })
        .to_vec();
    let current = saved(basis);
    let none: [Judgment; 0] = [];
    let store = store_of(&none, &[]);

    let planned = store
        .review(&current.0, &current.1)
        .plan(correct(
            r"abc\x1bdefg-r:d",
            CorrectionKind::Wrong,
            "not why",
        ))
        .unwrap();

    assert_eq!(recorded(planned).decision.unwrap().session, real);
}

#[test]
fn each_correction_kind_records_its_decision_and_words() {
    let a = lines("a.rs", "x");
    let current = annotated(&a);
    let none: [Judgment; 0] = [];
    let store = store_of(&none, &[]);
    let fresh = store.review(&current.0, &current.1);
    let addressed = reference(&current.1.decisions[2]);

    let cases = [
        (
            CorrectionKind::Wrong,
            Some("it was the reviewer's plan"),
            None,
        ),
        (
            CorrectionKind::NotTogether,
            Some("two separate fixes"),
            None,
        ),
        (
            CorrectionKind::Constraint,
            None,
            Some("the CLI must stay pure"),
        ),
    ];
    for (kind, reason, text) in cases {
        let words = reason.or(text).unwrap();
        let planned = fresh.plan(correct("3f2b0d4c:d2", kind, words)).unwrap();
        assert!(planned.first(), "{kind:?}");
        assert_eq!(
            recorded(planned),
            Judgment {
                reason: reason.map(str::to_owned),
                text: text.map(str::to_owned),
                decision: Some(addressed.clone()),
                correction: Some(kind),
                ..judgment("planned", &current.0, JudgmentKind::Correct, 100)
            },
            "{kind:?}"
        );
    }

    // As for a comment, a correction of a review with a judgment is not its
    // first.
    let judgments = [comment("note", &current.0, 10)];
    let store = store_of(&judgments, &[]);
    let planned = store
        .review(&current.0, &current.1)
        .plan(correct("3f2b0d4c:d2", CorrectionKind::Wrong, "not why"))
        .unwrap();
    assert!(!planned.first());
}

#[test]
fn a_correction_names_its_decision_by_name_unique_id_or_session_prefix_and_id() {
    let a = lines("a.rs", "x");
    let current = annotated(&a);
    let qualified_in_full = format!("{}:d1", SESSIONS[1]);
    let none: [Judgment; 0] = [];
    let store = store_of(&none, &[]);
    let review = store.review(&current.0, &current.1);
    let resolved = |typed| {
        review
            .plan(correct(typed, CorrectionKind::Wrong, "not why"))
            .map(|planned| recorded(planned).decision)
    };
    let decisions = &current.1.decisions;
    let both = vec!["3f2a9c1e:d1", "3f2b0d4c:d1"];
    let every = vec!["3f2a9c1e:d1", "3f2b0d4c:d1", "3f2b0d4c:d2"];

    assert_eq!(resolved("3f2a9c1e:d1"), Ok(Some(reference(&decisions[0]))));
    assert_eq!(resolved("d2"), Ok(Some(reference(&decisions[2]))));
    assert_eq!(resolved("3f2b:d1"), Ok(Some(reference(&decisions[1]))));
    assert_eq!(
        resolved(&qualified_in_full),
        Ok(Some(reference(&decisions[1])))
    );
    assert_eq!(
        resolved("d1"),
        Err(Refusal::AmbiguousDecision {
            typed: "d1",
            names: both.clone(),
        })
    );
    assert_eq!(
        resolved("3f2:d1"),
        Err(Refusal::AmbiguousDecision {
            typed: "3f2:d1",
            names: both,
        })
    );
    assert_eq!(
        resolved("d7"),
        Err(Refusal::NoDecision {
            typed: "d7",
            names: every.clone(),
        })
    );
    // No session starts with the prefix, or the one that does shows no
    // decision with the id.
    for typed in ["9c0d1e2f:d1", "3f2a:d2"] {
        assert_eq!(
            resolved(typed),
            Err(Refusal::NoDecision {
                typed,
                names: every.clone(),
            })
        );
    }

    // The names are sorted whatever the basis order.
    let mut reversed = current.1.clone();
    reversed.decisions.reverse();
    assert_eq!(
        store
            .review(&current.0, &reversed)
            .plan(correct("d7", CorrectionKind::Wrong, "not why"))
            .map(drop),
        Err(Refusal::NoDecision {
            typed: "d7",
            names: every,
        })
    );
}

#[test]
fn a_correction_closes_nothing_and_counts_for_no_completion() {
    let a = lines("a.rs", "x");
    let current = annotated(&a);
    let none: [Judgment; 0] = [];
    let planned = store_of(&none, &[])
        .review(&current.0, &current.1)
        .plan(correct("d2", CorrectionKind::Wrong, "not why"))
        .unwrap();
    let mut judgments = vec![recorded(planned)];

    let store = store_of(&judgments, &[]);
    let review = store.review(&current.0, &current.1);
    assert_eq!(item_states(&review), [ItemState::Open]);
    assert_eq!(
        review.plan(COMPLETE).map(drop),
        Err(Refusal::Unclosed(vec![&current.1.gate[0]]))
    );

    judgments.push(ack("ack", &current.0, &[&a], 110));
    let store = store_of(&judgments, &[]);
    assert_eq!(
        store
            .review(&current.0, &current.1)
            .plan(COMPLETE)
            .map(|planned| recorded(planned).closes),
        Ok(BTreeMap::from([(a.id.clone(), JudgmentId::new("ack"))]))
    );

    // A correction after a completion leaves the review complete.
    judgments.push(complete("done", &current.0, &[(&a, "ack")], 120));
    judgments.push(correction("later", &current.0, 130));
    let store = store_of(&judgments, &[]);
    assert_eq!(
        store.review(&current.0, &current.1).completion(),
        Some(&judgments[2])
    );
}

#[test]
fn an_undo_by_its_id_withdraws_a_correction_and_an_undo_by_item_leaves_it_standing() {
    let a = lines("a.rs", "x");
    let current = annotated(&a);
    let same_content = saved(basis("/work/b", '1', '2', vec![a.clone()]));
    let judgments = [
        correction("correction", &current.0, 10),
        ack("ack", &current.0, &[&a], 20),
    ];
    let store = store_of(&judgments, slice::from_ref(&same_content));
    let review = store.review(&current.0, &current.1);
    assert_eq!(
        review
            .plan(undo_judgment("correction"))
            .map(|planned| recorded(planned).withdraws),
        Ok(vec![withdrawal("correction", None)])
    );
    assert_eq!(
        review
            .plan(undo_items(slice::from_ref(&a.id)))
            .map(|planned| recorded(planned).withdraws),
        Ok(vec![withdrawal("ack", Some(&a))])
    );
    // A review of the same content in another worktree holds it as another
    // review's.
    assert_eq!(
        store
            .review(&same_content.0, &same_content.1)
            .plan(undo_judgment("correction"))
            .map(drop),
        Err(Refusal::OtherReview(&judgments[0]))
    );

    let mut undone = judgments.to_vec();
    undone.push(undo("undo-a", &current.0, &[("ack", Some(&a))], 30));
    let store = store_of(&undone, &[]);
    assert!(store.ledger().stands(&undone[0]));
    assert_eq!(
        store.review(&current.0, &current.1).corrections(),
        [&undone[0]]
    );
}

#[test]
fn the_standing_corrections_recorded_on_a_review_are_listed_in_store_order() {
    let a = lines("a.rs", "x");
    let current = annotated(&a);
    let same_content = saved(basis("/work/b", '1', '2', vec![a.clone()]));
    let judgments = [
        correction("later-time", &current.0, 30),
        comment("note", &current.0, 5),
        correction("withdrawn", &current.0, 10),
        correction("elsewhere", &same_content.0, 15),
        correction("earlier-time", &current.0, 20),
        undo("undo", &current.0, &[("withdrawn", None)], 40),
    ];
    let store = store_of(&judgments, slice::from_ref(&same_content));
    assert_eq!(
        store.review(&current.0, &current.1).corrections(),
        [&judgments[0], &judgments[4]]
    );
}

#[test]
fn a_correction_names_no_items_and_keeps_its_decision_when_its_items_are_resolved() {
    let words = Words::new("not why").unwrap();
    let typed: Call<'_, String> = Call::Correct {
        decision: "d1",
        kind: CorrectionKind::Wrong,
        words,
    };
    let items = typed.items();
    assert!(items.is_empty(), "{items:?}");
    assert_eq!(typed.kind(), JudgmentKind::Correct);
    assert_eq!(
        typed.naming::<ItemId>(&[]),
        Call::Correct {
            decision: "d1",
            kind: CorrectionKind::Wrong,
            words,
        }
    );
}

#[test]
fn given_words_are_never_blank() {
    assert_eq!(Words::new(""), None);
    assert_eq!(Words::new(" \t\n"), None);
    assert_eq!(Words::new(" why ").map(Words::as_str), Some(" why "));
}
