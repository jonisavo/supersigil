//! The backward walk: heads, links, buckets, ends, classes, deduplication,
//! the budget, and setting aside alternatives that assume an unrecorded
//! change.

mod common;

use std::fmt::Write as _;
use std::time::Duration;

use common::{
    candidate, create, finishes_within, in_worktree, replace, replace_all, request, unknown_op,
    with_hashes, with_patch,
};
use supersigil_anchor::walk::{Walk, dedup, walk};
use supersigil_anchor::{CandidateEdit, ChainClass, ChainEnd, PathStatus, StopReason, TargetKind};
use supersigil_record::observations::{EditOperation, Hunk, Material};
use supersigil_record::{EventId, RecordId};

fn ids(names: &[&str]) -> Vec<EventId> {
    names.iter().map(|n| EventId::new(*n)).collect()
}

/// (edits oldest first, class, ended at the base) of every chain.
fn summary(walk: &Walk) -> Vec<(Vec<EventId>, ChainClass, bool)> {
    walk.chains
        .iter()
        .map(|w| {
            (
                w.chain.edits.clone(),
                w.chain.class,
                w.chain.end == ChainEnd::Base,
            )
        })
        .collect()
}

fn wt(base: &str, target: &str, edits: Vec<CandidateEdit>) -> Walk {
    walk(&request(
        Some(base),
        Some(target),
        TargetKind::WorkingTree,
        edits,
    ))
}

#[test]
fn a_base_state_inside_a_segment_ends_the_chain() {
    // One session: x -> b (the base) -> t. Reviewing b..t uses only the
    // second edit.
    let out = wt(
        "b\n",
        "t\n",
        vec![
            candidate(replace("e1", "t", 1, "x\n", "b\n")),
            candidate(replace("e2", "t", 2, "b\n", "t\n")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e2"]), ChainClass::ExactFromBase, true)]
    );
    assert_eq!(out.status, PathStatus::Composed);
}

#[test]
fn a_state_carried_to_another_worktree_mid_segment_is_reached_by_a_verified_link() {
    // W1 records A -> S -> X; the reviewed worktree branched at S and
    // records S -> T. W1's later X does not supersede W2's copy of S.
    let w1 = "/work/w1";
    let out = wt(
        "A\n",
        "T\n",
        vec![
            in_worktree(
                with_hashes(replace("a1", "t1", 1, "A\n", "S\n"), "A\n", "S\n"),
                w1,
            ),
            in_worktree(
                with_hashes(replace("a2", "t1", 2, "S\n", "X\n"), "S\n", "X\n"),
                w1,
            ),
            candidate(with_hashes(
                replace("b1", "t2", 1, "S\n", "T\n"),
                "S\n",
                "T\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["a1", "b1"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn equal_ordinals_are_unordered() {
    // One record issued both edits; either could have run last.
    let out = wt(
        "a\nb\n",
        "A\nB\n",
        vec![
            candidate(replace("e1", "t", 5, "a\n", "A\n")),
            candidate(replace("e2", "t", 5, "b\n", "B\n")),
        ],
    );
    let heads: Vec<EventId> = out.chains.iter().map(|w| w.chain.head.clone()).collect();
    assert_eq!(heads, ids(&["e1", "e2"]));
    assert!(
        out.chains
            .iter()
            .all(|w| w.chain.class == ChainClass::ExactFromBase)
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn replayed_observations_are_one_edit() {
    let e = replace("e1", "t", 1, "a\n", "b\n");
    let out = wt("a\n", "b\n", vec![candidate(e.clone()), candidate(e)]);
    assert_eq!(out.accepted.len(), 1);
    assert_eq!(
        summary(&out),
        vec![(ids(&["e1"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn an_unknown_and_a_known_operation_are_one_edit() {
    let known = replace("e1", "t", 1, "a\n", "b\n");
    let mut old_log = known.clone();
    old_log.operation = EditOperation::Unknown;
    let (accepted, conflicts) = dedup(vec![
        CandidateEdit {
            record: RecordId::new("old"),
            ..candidate(old_log)
        },
        CandidateEdit {
            record: RecordId::new("new"),
            ..candidate(known)
        },
    ]);
    assert!(conflicts.is_empty());
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].edit.operation, EditOperation::Replace);
    assert_eq!(
        accepted[0].records,
        vec![RecordId::new("old"), RecordId::new("new")]
    );
}

#[test]
fn contradictory_sightings_are_excluded_as_conflicts() {
    let a = replace("e1", "t", 1, "a\n", "b\n");
    let b = replace("e1", "t", 1, "a\n", "c\n");
    let out = wt(
        "a\n",
        "b\n",
        vec![
            CandidateEdit {
                record: RecordId::new("r1"),
                ..candidate(a)
            },
            CandidateEdit {
                record: RecordId::new("r2"),
                ..candidate(b)
            },
        ],
    );
    assert!(out.accepted.is_empty());
    assert_eq!(out.conflicts.len(), 1);
    assert_eq!(
        out.conflicts[0].records,
        vec![RecordId::new("r1"), RecordId::new("r2")]
    );
    assert!(matches!(out.status, PathStatus::NotComposed { .. }));
}

#[test]
fn a_manual_change_that_contradicts_an_internal_after_hash_prevents_exactness() {
    // E1 recorded x x -> y x; a manual change made it x y; E2 then y -> Y.
    // Reversing E2 and the unique y for E1 would reach the base, but E1's
    // recorded after-state contradicts the bytes.
    let out = wt(
        "x\nx\n",
        "x\nY\n",
        vec![
            candidate(with_hashes(
                replace("e1", "t", 1, "x\n", "y\n"),
                "x\nx\n",
                "y\nx\n",
            )),
            candidate(replace("e2", "t", 2, "y\n", "Y\n")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e2"]), ChainClass::Consistent, false)]
    );
    assert_eq!(
        out.chains[0].chain.end,
        ChainEnd::Stopped {
            reasons: vec![StopReason::AfterHashMismatch {
                edit: EventId::new("e1")
            }]
        }
    );
}

#[test]
fn two_verified_histories_that_differ_in_which_edit_produced_a_state_both_stay() {
    // Transcript A: B -> S -> T. Transcript B: B -> S. Both are exact.
    let out = wt(
        "x\nz\n",
        "y\nZ\n",
        vec![
            candidate(with_hashes(
                replace("a1", "tA", 1, "x\n", "y\n"),
                "x\nz\n",
                "y\nz\n",
            )),
            candidate(with_hashes(
                replace("a2", "tA", 2, "z\n", "Z\n"),
                "y\nz\n",
                "y\nZ\n",
            )),
            candidate(with_hashes(
                replace("b1", "tB", 1, "x\n", "y\n"),
                "x\nz\n",
                "y\nz\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["a1", "a2"]), ChainClass::ExactFromBase, true),
            (ids(&["b1", "a2"]), ChainClass::ExactFromBase, true),
        ]
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn heads_that_each_start_elsewhere_are_both_kept() {
    let out = wt(
        "a\nb\n",
        "A\nB\n",
        vec![
            candidate(with_hashes(
                replace("ea", "tA", 1, "a\n", "A\n"),
                "a\nB\n",
                "A\nB\n",
            )),
            candidate(with_hashes(
                replace("eb", "tB", 1, "b\n", "B\n"),
                "A\nb\n",
                "A\nB\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["ea"]), ChainClass::ExactFromStart, false),
            (ids(&["eb"]), ChainClass::ExactFromStart, false),
        ]
    );
}

#[test]
fn a_verified_step_behind_an_unknown_hash_step_is_not_set_aside() {
    let out = wt(
        "A\n",
        "C\n",
        vec![
            candidate(with_hashes(
                replace("ea", "t1", 1, "A\n", "C\n"),
                "A\n",
                "C\n",
            )),
            candidate(replace("ex", "t2", 1, "X\n", "Y\n")),
            candidate(with_hashes(
                replace("eb", "t2", 2, "Y\n", "C\n"),
                "Y\n",
                "C\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["ea"]), ChainClass::ExactFromBase, true),
            (ids(&["ex", "eb"]), ChainClass::Consistent, false),
        ]
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn a_location_choice_that_misses_the_base_is_set_aside() {
    let out = wt(
        "a\nx\n",
        "a\na\n",
        vec![candidate(replace("d", "t", 1, "x\n", "a\n"))],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["d"]), ChainClass::ExactFromBase, true)]
    );
    assert_eq!(out.set_aside.len(), 1);
    assert_eq!(out.set_aside[0].start, common::state(Some("x\na\n")));
}

#[test]
fn a_location_choice_that_misses_the_base_is_set_aside_across_a_link() {
    // D is hashless, so both occurrences of `a` are readings of it. The one
    // at line 1 is reached from the base through P, a verified link into
    // another transcript; the one at line 2 stops with no predecessor. It
    // holds no edit the linked chain lacks, so it is set aside.
    let out = wt(
        "q\na\n",
        "a\na\n",
        vec![
            candidate(replace("d", "tD", 1, "x\n", "a\n")),
            candidate(with_hashes(
                replace("p", "tP", 1, "q\n", "x\n"),
                "q\na\n",
                "x\na\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["p", "d"]), ChainClass::ExactFromBase, true)]
    );
    assert_eq!(out.set_aside.len(), 1);
    assert_eq!(out.set_aside[0].start, common::state(Some("a\nx\n")));
}

#[test]
fn a_location_choice_whose_branches_use_different_edits_is_not_set_aside() {
    // P (verified) is admitted only by the reading that edits line 1.
    let out = wt(
        "a\nx\n",
        "a\na\n",
        vec![
            candidate(with_hashes(
                replace("p", "t", 1, "q\n", "a\n"),
                "x\nq\n",
                "x\na\n",
            )),
            candidate(replace("d", "t", 2, "x\n", "a\n")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["p", "d"]), ChainClass::Consistent, false),
            (ids(&["d"]), ChainClass::ExactFromBase, true),
        ]
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn a_verified_link_is_explored_mid_bucket() {
    // One record issued both hashless edits. After reversing `b -> B`
    // first, the state `A\nb\n` is P's verified after-state, so the chain
    // may leave the bucket there, before `a -> A`, and enter P's
    // transcript. That reading names a different edit, so it stays.
    let out = wt(
        "a\nb\n",
        "A\nB\n",
        vec![
            candidate(replace("ea", "t", 1, "a\n", "A\n")),
            candidate(replace("eb", "t", 1, "b\n", "B\n")),
            candidate(with_hashes(
                replace("p", "tP", 1, "q\n", "A\n"),
                "q\nb\n",
                "A\nb\n",
            )),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["eb", "ea"]), ChainClass::ExactFromBase, true),
            (ids(&["ea", "eb"]), ChainClass::ExactFromBase, true),
            (ids(&["p", "eb"]), ChainClass::Consistent, false),
        ]
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn the_whole_sequence_and_the_deletion_alone_both_replay_from_the_base() {
    // [a, b], a -> x, b -> a, x -> b, delete a, with unknown hashes.
    let out = wt(
        "a\nb\n",
        "b\n",
        vec![
            candidate(replace("e1", "t", 1, "a\n", "x\n")),
            candidate(replace("e2", "t", 2, "b\n", "a\n")),
            candidate(replace("e3", "t", 3, "x\n", "b\n")),
            candidate(replace("e4", "t", 4, "a\n", "")),
        ],
    );
    let exact: Vec<Vec<EventId>> = summary(&out)
        .into_iter()
        .filter(|(_, class, _)| *class == ChainClass::ExactFromBase)
        .map(|(edits, _, _)| edits)
        .collect();
    assert_eq!(exact, vec![ids(&["e4"]), ids(&["e1", "e2", "e3", "e4"])]);
}

#[test]
fn an_edit_reverted_by_hand_still_allows_an_exact_chain() {
    // b -> c was recorded, then reverted by hand; a -> x followed.
    let out = wt(
        "a\nb\n",
        "x\nb\n",
        vec![
            candidate(replace("e1", "t", 1, "b\n", "c\n")),
            candidate(replace("e2", "t", 2, "a\n", "x\n")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e2"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn working_tree_heads_are_only_the_last_bucket_of_the_reviewed_worktree() {
    let edits = || {
        vec![
            candidate(replace("e1", "t", 1, "a\n", "b\n")),
            candidate(replace("e2", "t", 2, "b\n", "c\n")),
        ]
    };
    // The target is e1's result; e2 was later undone by hand. A later
    // observed edit supersedes e1's state in the working tree.
    let working = wt("a\n", "b\n", edits());
    assert!(working.chains.is_empty());
    assert_eq!(
        working.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::NotLocatable {
                edit: EventId::new("e2")
            }]
        }
    );
    // A commit may predate e2, so any edit can be a head.
    let commit = walk(&request(
        Some("a\n"),
        Some("b\n"),
        TargetKind::Commit,
        edits(),
    ));
    assert_eq!(
        summary(&commit),
        vec![(ids(&["e1"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn an_edit_in_an_originating_worktree_can_be_a_working_tree_head() {
    // The reviewed worktree has no edit of the path; the commit came from W1.
    let out = wt(
        "a\n",
        "t\n",
        vec![in_worktree(replace("e1", "t", 1, "a\n", "t\n"), "/work/w1")],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e1"]), ChainClass::ExactFromBase, true)]
    );
}

#[test]
fn an_exhausted_budget_makes_the_search_incomplete() {
    let mut req = request(
        Some("a\n"),
        Some("b\n"),
        TargetKind::WorkingTree,
        vec![candidate(replace("e1", "t", 1, "a\n", "b\n"))],
    );
    req.budget_bytes = 3;
    assert_eq!(walk(&req).status, PathStatus::SearchIncomplete);
}

#[test]
fn no_candidate_edit_is_not_composed() {
    let out = wt("a\n", "b\n", Vec::new());
    assert_eq!(
        out.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::NoPredecessor]
        }
    );
}

#[test]
fn steps_hold_the_states_between_edits_oldest_first() {
    let out = wt(
        "a\n",
        "c\n",
        vec![
            candidate(replace("e1", "t", 1, "a\n", "b\n")),
            candidate(replace("e2", "t", 2, "b\n", "c\n")),
        ],
    );
    let chain = &out.chains[0];
    assert_eq!(chain.start, common::state(Some("a\n")));
    let states: Vec<_> = chain
        .steps
        .iter()
        .map(|s| (s.edit.clone(), s.before.clone(), s.after.clone()))
        .collect();
    assert_eq!(
        states,
        vec![
            (
                EventId::new("e1"),
                common::state(Some("a\n")),
                common::state(Some("b\n"))
            ),
            (
                EventId::new("e2"),
                common::state(Some("b\n")),
                common::state(Some("c\n"))
            ),
        ]
    );
    assert_eq!(chain.chain.head, EventId::new("e2"));
}

#[test]
fn a_sibling_stopped_early_by_a_recorded_check_is_set_aside() {
    // Only the true deletion position satisfies the earlier edit's
    // recorded after-hash; the other positions stop there, using fewer
    // edits than the chain that reaches the base.
    let out = wt(
        "a\nb\nc\n",
        "a\nB\n",
        vec![
            candidate(with_hashes(
                replace("e0", "t", 1, "b\n", "B\n"),
                "a\nb\nc\n",
                "a\nB\nc\n",
            )),
            candidate(replace("e1", "t", 2, "c\n", "")),
        ],
    );
    assert_eq!(
        summary(&out),
        vec![(ids(&["e0", "e1"]), ChainClass::ExactFromBase, true)]
    );
    assert_eq!(out.set_aside.len(), 2);
    assert!(out.set_aside.iter().all(|w| w.chain.edits == ids(&["e1"])));
}

#[test]
fn a_sibling_stopped_early_by_a_creation_is_set_aside() {
    let out = walk(&request(
        None,
        Some("b\n"),
        TargetKind::WorkingTree,
        vec![
            candidate(create("c", "t", 1, "a\nb\n")),
            candidate(replace("d", "t", 2, "a\n", "")),
        ],
    ));
    assert_eq!(
        summary(&out),
        vec![(ids(&["c", "d"]), ChainClass::ExactFromBase, true)]
    );
    assert_eq!(out.set_aside.len(), 1);
}

#[test]
fn verified_heads_survive_a_hunk_in_another_display() {
    // Both edits record both hashes, which decide alone. One retained hunk
    // holds the raw tab, which Claude Code's display never shows; the other
    // shows it as two spaces. Neither patch may stop its head.
    let mut ea = with_hashes(replace("ea", "tA", 1, "\tx\n", "\ty\n"), "\tx\n", "\ty\n");
    ea.patch = Material::Retained(vec![Hunk {
        old_start: 1,
        old_lines: 1,
        new_start: 1,
        new_lines: 1,
        lines: vec!["-\tx".to_owned(), "+\ty".to_owned()],
    }]);
    let eb = with_patch(
        with_hashes(replace("eb", "tB", 1, "\tx\n", "\ty\n"), "\tx\n", "\ty\n"),
        "\tx\n",
        "\ty\n",
    );
    let out = wt("\tx\n", "\ty\n", vec![candidate(ea), candidate(eb)]);
    assert_eq!(
        summary(&out),
        vec![
            (ids(&["ea"]), ChainClass::ExactFromBase, true),
            (ids(&["eb"]), ChainClass::ExactFromBase, true),
        ]
    );
}

#[test]
fn a_large_bucket_of_equal_ordinals_ends_search_incomplete_promptly() {
    // One record issued 25 edits of the file at once, and a commit target
    // lets any of them be the head: every subset and order of the bucket is
    // a branch. Enumeration must stop at the budget, never build the
    // powerset.
    let (mut base, mut target) = (String::new(), String::new());
    for i in 1..=25 {
        writeln!(base, "x{i:02}").unwrap();
        writeln!(target, "y{i:02}").unwrap();
    }
    let edits = (1..=25)
        .map(|i| {
            candidate(replace(
                &format!("e{i}"),
                "t",
                1,
                &format!("x{i:02}\n"),
                &format!("y{i:02}\n"),
            ))
        })
        .collect();
    let req = request(Some(&base), Some(&target), TargetKind::Commit, edits);
    let out = finishes_within(Duration::from_secs(60), move || walk(&req));
    assert_eq!(out.status, PathStatus::SearchIncomplete);
}

#[test]
fn a_huge_bucket_is_charged_for_its_bookkeeping() {
    // 50,000 edits issued by one record, none reversible: each branch
    // fails at once, but naming which of the bucket's other edits still
    // precede the head reads all of them. That is charged too, so the
    // budget runs out after about 170 branches, not 65,536 of them.
    let edits = (1..=50_000)
        .map(|i| candidate(unknown_op(&format!("e{i}"), "t", 1, "a\n", "b\n")))
        .collect();
    let req = request(Some("a\n"), Some("b\n"), TargetKind::Commit, edits);
    let out = finishes_within(Duration::from_secs(10), move || walk(&req));
    assert_eq!(out.status, PathStatus::SearchIncomplete);
}

#[test]
fn a_hashless_deletion_in_a_large_file_ends_exact_from_the_base() {
    // Without the patch every line start of 3,000 lines is a candidate and
    // the default budget runs out; the patch's old side leaves the true one.
    let lines: Vec<String> = (0..3_000).map(|i| format!("line {i:04}\n")).collect();
    let base = lines.concat();
    let target = [&lines[..1_500], &lines[1_501..]].concat().concat();
    let edit = with_patch(replace("d", "t", 1, &lines[1_500], ""), &base, &target);
    let out = wt(&base, &target, vec![candidate(edit)]);
    assert_eq!(out.status, PathStatus::Composed);
    assert_eq!(
        summary(&out),
        vec![(ids(&["d"]), ChainClass::ExactFromBase, true)]
    );
}

/// A session of `count` hashless edits of a one-line file, each turning
/// `v{i-1}` into `v{i}`.
fn counter_session(count: u64) -> Vec<CandidateEdit> {
    (1..=count)
        .map(|i| {
            candidate(replace(
                &format!("e{i}"),
                "t",
                i,
                &format!("v{}\n", i - 1),
                &format!("v{i}\n"),
            ))
        })
        .collect()
}

#[test]
fn a_long_session_is_walked_in_bounded_space() {
    // One chain as deep as the session is long, well within the default
    // budget. The walk keeps one copy of the path, not one per step, and no
    // call stack that grows with it.
    let count = 20_000;
    let req = request(
        Some("v0\n"),
        Some(&format!("v{count}\n")),
        TargetKind::WorkingTree,
        counter_session(count),
    );
    let out = finishes_within(Duration::from_secs(60), move || walk(&req));
    assert_eq!(out.status, PathStatus::Composed);
    assert_eq!(out.chains.len(), 1);
    assert_eq!(out.chains[0].chain.class, ChainClass::ExactFromBase);
    assert_eq!(out.chains[0].steps.len(), 20_000);
}

#[test]
fn each_copy_of_a_shared_path_into_a_chain_is_charged() {
    // A 2,000-step session whose oldest edit is a hashless deletion with no
    // patch: each of the 102 line starts is a reading of it, and each
    // reading is a chain holding its own copy of the session, about 84 MB
    // of states alone, more than the default budget. Exploring charges
    // about 4 MB.
    let pad = "p\n".repeat(100);
    let mut edits = vec![candidate(replace("d", "t", 0, "d\n", ""))];
    edits.extend(counter_session(2_000));
    let out = wt(&format!("v0\nd\n{pad}"), &format!("v2000\n{pad}"), edits);
    assert_eq!(out.status, PathStatus::SearchIncomplete);
}

#[test]
fn links_are_looked_up_without_reading_every_recorded_edit() {
    // A 40,000-step session, and another transcript of 50,000 verified
    // edits whose after-states match none of its states. Finding the links
    // at a state costs what it finds, not a pass over every edit of every
    // other transcript.
    let mut edits = counter_session(40_000);
    edits.extend((1..=50_000).map(|i| {
        let after = format!("u{i}\n");
        candidate(with_hashes(
            replace(&format!("u{i}"), "u", i, "u\n", &after),
            "u\n",
            &after,
        ))
    }));
    let req = request(
        Some("v0\n"),
        Some("v40000\n"),
        TargetKind::WorkingTree,
        edits,
    );
    let out = finishes_within(Duration::from_secs(20), move || walk(&req));
    assert_eq!(out.status, PathStatus::Composed);
    assert_eq!(out.chains.len(), 1);
    assert_eq!(out.chains[0].chain.class, ChainClass::ExactFromBase);
}

#[test]
fn links_into_entered_transcripts_are_charged_as_they_are_passed_over() {
    // 40,000 transcripts each record a verified edit that leaves `x` as it
    // was, so every state of a chain links into every transcript it has not
    // entered yet. At depth k, finding the next link passes over the k
    // transcripts already entered. Each one passed over is charged, so the
    // budget ends the search instead of 800 million uncharged skips.
    let edits = (0..40_000)
        .map(|i| {
            candidate(with_hashes(
                replace(&format!("e{i}"), &format!("t{i}"), 1, "x\n", "x\n"),
                "x\n",
                "x\n",
            ))
        })
        .collect();
    let req = request(Some("q\n"), Some("x\n"), TargetKind::WorkingTree, edits);
    let out = finishes_within(Duration::from_secs(10), move || walk(&req));
    assert_eq!(out.status, PathStatus::SearchIncomplete);
}

#[test]
fn chains_are_settled_without_comparing_every_pair() {
    // A replace-all with 15 independent alignment choices, after a hashless
    // edit with two locations. Under each alignment, one location reaches
    // the base; the other is explained further back by a verified edit P,
    // which no chain reaching the base holds, so it stays. That is 32,768
    // chains of each kind with two keys between them: settling compares
    // the two keys, not a billion pairs of chains.
    let lines = "x\nx\n".repeat(15);
    let shrunk = "x\n".repeat(15);
    let edits = vec![
        candidate(with_hashes(
            replace("p", "t", 0, "p\n", "q\n"),
            &format!("{lines}w\np\n"),
            &format!("{lines}w\nq\n"),
        )),
        candidate(replace("q", "t", 1, "q\n", "w\n")),
        candidate(with_hashes(
            replace_all("all", "t", 2, "x\nx\n", "x\n"),
            &format!("{lines}w\nw\n"),
            &format!("{shrunk}w\nw\n"),
        )),
    ];
    let mut req = request(
        Some(&format!("{lines}q\nw\n")),
        Some(&format!("{shrunk}w\nw\n")),
        TargetKind::WorkingTree,
        edits,
    );
    req.budget_bytes = 256 << 20;
    let out = finishes_within(Duration::from_secs(20), move || walk(&req));
    assert_eq!(out.status, PathStatus::Composed);
    let base_chains = summary(&out).iter().filter(|(_, _, base)| *base).count();
    assert_eq!(base_chains, 1 << 15);
    assert_eq!(out.chains.len(), 1 << 16);
    assert!(out.set_aside.is_empty());
}

#[test]
fn every_alignment_choice_is_charged_to_the_budget() {
    // 18 independent prefix/suffix choices make 2^18 alignments of one
    // step; each is charged, so a 1 MiB budget runs out long before.
    let base = "x\nx\n".repeat(18);
    let target = "x\n".repeat(18);
    let edit = with_hashes(replace_all("e", "t", 1, "x\nx\n", "x\n"), &base, &target);
    let mut req = request(
        Some(&base),
        Some(&target),
        TargetKind::WorkingTree,
        vec![candidate(edit)],
    );
    req.budget_bytes = 1 << 20;
    let out = finishes_within(Duration::from_secs(60), move || walk(&req));
    assert_eq!(out.status, PathStatus::SearchIncomplete);
}
