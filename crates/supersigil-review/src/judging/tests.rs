//! Checks that every history the judging rules allow keeps each standing
//! completion counting, so the guard against one that does not never fires.

use std::collections::BTreeMap;
use std::path::PathBuf;

use proptest::prelude::*;
use supersigil_record::JudgmentId;
use supersigil_record::reviews::{
    Algorithms, BASIS_SCHEMA, Basis, BasisBase, BasisId, BasisScope, BasisTarget, Fingerprint,
    FingerprintPart, GateDetail, GateItem, ItemId, Judgment, JudgmentKind, PathBytes, Subject,
    TargetKind, Who, Withdrawal,
};
use supersigil_record::time::UtcTime;

use super::JudgedStore;
use crate::model::RelatedReading;
use crate::names::ItemNames;

/// The files every gate item is about.
const FILES: [&str; 2] = ["a.rs", "b.rs"];

/// An unexplained-lines item for `file` whose fingerprint is `material`.
fn item(file: &str, material: &str) -> GateItem {
    let part = FingerprintPart::of(material.as_bytes());
    let parts = ["content", "evidence", "outcomes"]
        .into_iter()
        .map(|name| (name.to_owned(), part.clone()))
        .collect();
    GateItem::new(
        Subject::Path(PathBytes::new(file)),
        GateDetail::UnexplainedLines {
            unattributed: 1,
            content_match_incomplete: 0,
            unresolved: 0,
            ambiguous: 0,
            line_ending: 0,
            content_match_only: 0,
        },
        Fingerprint(parts),
    )
}

/// A saved working-tree review of `worktree` from base tree `1` to target
/// tree `target`, holding each file whose material is given.
fn review(worktree: &str, target: char, materials: [Option<&str>; 2]) -> (BasisId, Basis) {
    let gate = FILES
        .iter()
        .zip(materials)
        .filter_map(|(file, material)| Some(item(file, material?)))
        .collect();
    let basis = Basis {
        schema: BASIS_SCHEMA.to_owned(),
        worktree: PathBuf::from(worktree),
        base: BasisBase {
            commit: None,
            tree: "1".repeat(40),
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
    };
    (basis.basis_id().unwrap(), basis)
}

/// Four reviews: the second and fourth have the first's content from other
/// worktrees, and the third the first's worktree with other code. Each
/// holds each file, if at all, as material `x` or `y`.
fn reviews(gates: [[Option<bool>; 2]; 4]) -> Vec<(BasisId, Basis)> {
    let material = |gate: [Option<bool>; 2]| gate.map(|y| y.map(|y| if y { "y" } else { "x" }));
    vec![
        review("/work/a", '2', material(gates[0])),
        review("/work/b", '2', material(gates[1])),
        review("/work/a", '3', material(gates[2])),
        review("/work/c", '2', material(gates[3])),
    ]
}

/// A step someone takes through `supersigil judge`.
#[derive(Debug, Clone)]
enum Step {
    /// Acknowledge, or dismiss when `dismiss`, the files in `files` on
    /// review `review`.
    Close {
        review: usize,
        files: u8,
        dismiss: bool,
    },
    /// Complete review `review`.
    Complete { review: usize },
    /// Undo the files in `files` on review `review`.
    UndoItems { review: usize, files: u8 },
    /// Undo the `judgment`th judgment recorded, modulo their number, on
    /// review `review`.
    UndoJudgment { review: usize, judgment: usize },
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0..4usize, 1..4u8, any::<bool>()).prop_map(|(review, files, dismiss)| Step::Close {
            review,
            files,
            dismiss
        }),
        (0..4usize).prop_map(|review| Step::Complete { review }),
        (0..4usize, 1..4u8).prop_map(|(review, files)| Step::UndoItems { review, files }),
        (0..4usize, any::<usize>())
            .prop_map(|(review, judgment)| Step::UndoJudgment { review, judgment }),
    ]
}

/// A judgment of `kind` on `basis`, the `n`th recorded.
fn judgment(n: usize, basis: &BasisId, kind: JudgmentKind) -> Judgment {
    Judgment {
        id: JudgmentId::new(format!("j{n}")),
        basis: basis.clone(),
        kind,
        items: Vec::new(),
        reason: (kind == JudgmentKind::Dismiss).then(|| "generated".to_owned()),
        text: None,
        closes: BTreeMap::new(),
        withdraws: Vec::new(),
        who: Who {
            name: "Joni Savolainen".to_owned(),
            email: "joni@example.com".to_owned(),
        },
        at: UtcTime::from_unix(i64::try_from(n).unwrap()),
    }
}

/// The judgment `step` records after `judgments` on `reviews`, if the
/// judging rules allow it.
fn record(step: &Step, reviews: &[(BasisId, Basis)], judgments: &[Judgment]) -> Option<Judgment> {
    let store = JudgedStore::new(judgments, reviews.iter().map(|(id, basis)| (id, basis)));
    let review_of = |index: usize| {
        let (id, basis) = &reviews[index];
        (id, basis, store.review(id, basis))
    };
    let named = |basis: &Basis, files: u8| -> Vec<ItemId> {
        basis
            .gate
            .iter()
            .filter(|item| {
                FILES.iter().enumerate().any(|(k, file)| {
                    files & (1 << k) != 0 && item.subject == Subject::Path(PathBytes::new(*file))
                })
            })
            .map(|item| item.id.clone())
            .collect()
    };
    let n = judgments.len();
    match *step {
        Step::Close {
            review,
            files,
            dismiss,
        } => {
            let (id, basis, judged) = review_of(review);
            let items = named(basis, files);
            if items.is_empty() || judged.check_close(&items).is_err() {
                return None;
            }
            let kind = if dismiss {
                JudgmentKind::Dismiss
            } else {
                JudgmentKind::Acknowledge
            };
            Some(Judgment {
                items,
                ..judgment(n, id, kind)
            })
        }
        Step::Complete { review } => {
            let (id, _, judged) = review_of(review);
            let closes = judged.check_complete().ok()?;
            Some(Judgment {
                closes,
                ..judgment(n, id, JudgmentKind::Complete)
            })
        }
        Step::UndoItems { review, files } => {
            let (id, basis, judged) = review_of(review);
            let items = named(basis, files);
            if items.is_empty() {
                return None;
            }
            let withdraws = judged.plan_undo(&items).ok()?;
            Some(undo(n, id, withdraws))
        }
        Step::UndoJudgment { review, judgment } => {
            if judgments.is_empty() {
                return None;
            }
            let target = &judgments[judgment % judgments.len()];
            let withdraws = store.ledger().plan_withdrawal(target).ok()?;
            Some(undo(n, &reviews[review].0, withdraws))
        }
    }
}

fn undo(n: usize, basis: &BasisId, withdraws: Vec<Withdrawal>) -> Judgment {
    Judgment {
        withdraws,
        ..judgment(n, basis, JudgmentKind::Undo)
    }
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    /// Every standing completion keeps counting, and a review complete by
    /// its own or a carried completion has no open item.
    #[test]
    fn every_standing_completion_counts(
        gates in proptest::array::uniform4(proptest::array::uniform2(proptest::option::of(any::<bool>()))),
        steps in proptest::collection::vec(step(), 1..16),
    ) {
        let reviews = reviews(gates);
        let mut judgments = Vec::new();
        for step in &steps {
            if let Some(recorded) = record(step, &reviews, &judgments) {
                judgments.push(recorded);
            }
            let store = JudgedStore::new(&judgments, reviews.iter().map(|(id, basis)| (id, basis)));
            for (id, basis) in &reviews {
                let judged = store.review(id, basis);
                prop_assert_eq!(judged.uncounted(), Vec::<&Judgment>::new(), "{:?}", step);
                let names = ItemNames::new(&basis.gate, BTreeMap::new(), false, |path| {
                    path.to_str().map(str::to_owned)
                });
                let report = judged
                    .report_reading(&names, |related| {
                        let unchanged = RelatedReading {
                            changed_paths: Some(Vec::new()),
                            ..RelatedReading::default()
                        };
                        Ok::<_, ()>(vec![unchanged; related.len()])
                    })
                    .unwrap();
                if report.status.complete {
                    prop_assert_eq!(report.gate.open, 0, "{:?}", step);
                }
                // The guard never changes which completion completes the
                // review: the own one, else the most recent one carrying.
                let ledger = store.ledger();
                let carrying = report
                    .status
                    .related
                    .iter()
                    .filter(|related| related.carries())
                    .map(|related| &related.review);
                let unguarded = ledger
                    .completion(id)
                    .or_else(|| ledger.latest_completion(carrying));
                prop_assert_eq!(
                    report.status.completion.map(|completion| completion.id),
                    unguarded.map(|completion| completion.id.clone()),
                    "{:?}",
                    step
                );
            }
        }
    }
}
