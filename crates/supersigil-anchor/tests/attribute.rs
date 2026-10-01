//! Attributing a path: provenance, combination across chains, the
//! content-match fallback, and the corpus-shaped and unsupported cases.

mod common;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::time::Duration;

use common::{
    candidate, finishes_within, overwrite, replace, replace_all, request, unknown_op, with_hashes,
    with_patch,
};
use supersigil_anchor::walk::walk;
use supersigil_anchor::{
    BaseLineOutcome, CandidateEdit, ChainClass, Fate, LineOutcome, Origin, PathAttribution,
    PathStatus, Provenance, StopReason, TargetKind, attribute,
};
use supersigil_record::EventId;

fn id(name: &str) -> EventId {
    EventId::new(name)
}

fn wt(base: &str, target: &str, edits: Vec<CandidateEdit>) -> PathAttribution {
    attribute(request(
        Some(base),
        Some(target),
        TargetKind::WorkingTree,
        edits,
    ))
}

/// The agreed provenance of a target line; panics when ambiguous.
fn agreed(out: &PathAttribution, line: usize) -> (&Provenance, Option<ChainClass>) {
    match &out.target[line] {
        LineOutcome::Agreed { provenance, class } => (provenance, *class),
        other @ LineOutcome::Ambiguous { .. } => panic!("line {line} is {other:?}"),
    }
}

fn origins(out: &PathAttribution, line: usize) -> BTreeSet<Origin> {
    agreed(out, line).0.origins.clone()
}

fn base_fates(out: &PathAttribution, line: usize) -> BTreeSet<Fate> {
    match &out.base[line] {
        BaseLineOutcome::Agreed { fates, .. } => fates.clone(),
        other @ BaseLineOutcome::Ambiguous { .. } => panic!("base line {line} is {other:?}"),
    }
}

fn base(line: usize) -> Origin {
    Origin::Base(line)
}

fn introduced(name: &str) -> Origin {
    Origin::Introduced(id(name))
}

fn set<T: Ord>(items: impl IntoIterator<Item = T>) -> BTreeSet<T> {
    items.into_iter().collect()
}

fn ambiguous(out: &PathAttribution, line: usize) -> Vec<BTreeSet<Origin>> {
    match &out.target[line] {
        LineOutcome::Ambiguous { readings } => {
            readings.iter().map(|r| r.value.origins.clone()).collect()
        }
        other @ LineOutcome::Agreed { .. } => panic!("line {line} is {other:?}"),
    }
}

#[test]
fn duplicate_lines_keep_their_own_provenance() {
    let out = wt(
        "A\nX\nX\nB\n",
        "A\nX\nB\n",
        vec![candidate(replace("e", "t", 1, "A\nX\n", "A\n"))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(origins(&out, 1), set([base(2)]));
    assert_eq!(base_fates(&out, 1), set([Fate::Replaced { edit: id("e") }]));
    assert_eq!(base_fates(&out, 2), set([Fate::KeptAs { line: 1 }]));
}

#[test]
fn a_string_literal_whitespace_change_is_whitespace_only_never_replaced() {
    let out = wt(
        "return \"a b\";\n",
        "return \"ab\";\n",
        vec![candidate(replace("e", "t", 1, "\"a b\"", "\"ab\""))],
    );
    let (provenance, class) = agreed(&out, 0);
    assert_eq!(provenance.origins, set([base(0)]));
    assert_eq!(provenance.whitespace_only, set([id("e")]));
    assert_eq!(class, Some(ChainClass::ExactFromBase));
    assert_eq!(
        base_fates(&out, 0),
        set([Fate::CarriedTo {
            line: 0,
            via: set([id("e")])
        }])
    );
}

#[test]
fn a_formatter_join_keeps_both_origins() {
    let out = wt(
        "left\n",
        "left right\n",
        vec![
            candidate(replace("e1", "t", 1, "left\n", "left\nright\n")),
            candidate(replace("e2", "t", 2, "left\nright\n", "left right\n")),
        ],
    );
    let (provenance, _) = agreed(&out, 0);
    assert_eq!(provenance.origins, set([base(0), introduced("e1")]));
    assert_eq!(provenance.whitespace_only, set([id("e2")]));
}

#[test]
fn a_formatter_change_to_a_base_line_carries_it() {
    let out = wt(
        "let x=1;\n",
        "let x = 1;\n",
        vec![candidate(replace(
            "fmt",
            "t",
            1,
            "let x=1;\n",
            "let x = 1;\n",
        ))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(
        base_fates(&out, 0),
        set([Fate::CarriedTo {
            line: 0,
            via: set([id("fmt")])
        }])
    );
}

#[test]
fn a_split_carries_the_base_line_to_both_target_lines() {
    let out = wt(
        "AxZ\nEND\n",
        "Ax\nZ\nEND\n",
        vec![candidate(replace("e", "t", 1, "x", "x\n"))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(origins(&out, 1), set([base(0)]));
    assert_eq!(origins(&out, 2), set([base(1)]));
    assert_eq!(
        base_fates(&out, 0),
        set([
            Fate::CarriedTo {
                line: 0,
                via: set([id("e")])
            },
            Fate::CarriedTo {
                line: 1,
                via: set([id("e")])
            },
        ])
    );
}

#[test]
fn splitting_then_rewriting_one_half_is_both_carried_and_replaced() {
    let out = wt(
        "AxZ\n",
        "Ax\nQ\n",
        vec![
            candidate(replace("split", "t", 1, "x", "x\n")),
            candidate(replace("rewrite", "t", 2, "Z\n", "Q\n")),
        ],
    );
    assert_eq!(
        base_fates(&out, 0),
        set([
            Fate::CarriedTo {
                line: 0,
                via: set([id("split")])
            },
            Fate::Replaced {
                edit: id("rewrite")
            },
        ])
    );
    let (provenance, _) = agreed(&out, 1);
    assert_eq!(provenance.origins, set([introduced("rewrite")]));
    assert_eq!(provenance.earlier, set([id("split")]));
}

#[test]
fn a_blank_intermediate_line_keeps_its_contributor() {
    // E and G, in different transcripts, each turn `a` into a blank line, and
    // F writes `b` over it. The chains [E, F] and [G, F] name different
    // earlier contributors, so the line is ambiguous, never attributed.
    let out = wt(
        "a\n",
        "b\n",
        vec![
            candidate(with_hashes(replace("E", "t1", 1, "a\n", "\n"), "a\n", "\n")),
            candidate(with_hashes(replace("G", "t2", 1, "a\n", "\n"), "a\n", "\n")),
            candidate(with_hashes(replace("F", "t3", 1, "\n", "b\n"), "\n", "b\n")),
        ],
    );
    let LineOutcome::Ambiguous { readings } = &out.target[0] else {
        panic!("line 0 is {:?}", out.target[0]);
    };
    let earlier: Vec<(BTreeSet<Origin>, BTreeSet<EventId>)> = readings
        .iter()
        .map(|r| (r.value.origins.clone(), r.value.earlier.clone()))
        .collect();
    assert_eq!(
        earlier,
        vec![
            (set([introduced("F")]), set([id("E")])),
            (set([introduced("F")]), set([id("G")])),
        ]
    );
}

#[test]
fn a_whitespace_only_blank_line_deletion_is_explained() {
    let out = wt(
        "\nX\n",
        "X\n",
        vec![candidate(replace("d", "t", 1, "\n", ""))],
    );
    assert_eq!(
        base_fates(&out, 0),
        set([Fate::RemovedByWhitespace { edit: id("d") }])
    );
    assert_eq!(base_fates(&out, 1), set([Fate::KeptAs { line: 0 }]));
    assert_eq!(out.set_aside.len(), 1);
}

#[test]
fn a_deletion_that_joins_two_lines_introduces_the_joined_line() {
    let out = wt(
        "foofoo\nfoo\nEND\n",
        "foofoo\nEND\n",
        vec![candidate(replace("d", "t", 1, "foo\n", ""))],
    );
    assert_eq!(origins(&out, 0), set([introduced("d")]));
    assert_eq!(origins(&out, 1), set([base(2)]));
    assert_eq!(base_fates(&out, 0), set([Fate::Replaced { edit: id("d") }]));
    assert_eq!(base_fates(&out, 1), set([Fate::Replaced { edit: id("d") }]));
}

#[test]
fn overlapping_context_makes_the_surviving_line_ambiguous() {
    let out = wt(
        "x\nx\n",
        "x\n",
        vec![candidate(replace("e", "t", 1, "x\nx\n", "x\n"))],
    );
    assert_eq!(ambiguous(&out, 0), vec![set([base(0)]), set([base(1)])]);
    assert!(matches!(out.base[0], BaseLineOutcome::Ambiguous { .. }));
}

#[test]
fn multi_line_replacement_context_stays_base_lines() {
    let out = wt(
        "a\nb\nc\n",
        "a\nB\nc\n",
        vec![candidate(replace("e", "t", 1, "a\nb\nc\n", "a\nB\nc\n"))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(origins(&out, 1), set([introduced("e")]));
    assert_eq!(origins(&out, 2), set([base(2)]));
    assert_eq!(base_fates(&out, 1), set([Fate::Replaced { edit: id("e") }]));
}

#[test]
fn two_heads_that_each_leave_a_line_unexplained_make_both_lines_ambiguous() {
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
        ambiguous(&out, 0),
        vec![set([introduced("ea")]), set([Origin::Unexplained])]
    );
    assert_eq!(
        ambiguous(&out, 1),
        vec![set([Origin::Unexplained]), set([introduced("eb")])]
    );
}

#[test]
fn two_verified_histories_make_only_the_disputed_line_ambiguous() {
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
        ambiguous(&out, 0),
        vec![set([introduced("a1")]), set([introduced("b1")])]
    );
    assert_eq!(
        agreed(&out, 1),
        (
            &Provenance {
                origins: set([introduced("a2")]),
                ..Provenance::default()
            },
            Some(ChainClass::ExactFromBase)
        )
    );
}

#[test]
fn a_verified_chain_and_a_competing_consistent_chain_are_ambiguous() {
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
        ambiguous(&out, 0),
        vec![set([introduced("ea")]), set([introduced("eb")])]
    );
}

#[test]
fn an_occurrence_choice_that_misses_the_base_is_listed_not_combined() {
    let out = wt(
        "a\nx\n",
        "a\na\n",
        vec![candidate(replace("d", "t", 1, "x\n", "a\n"))],
    );
    assert_eq!(origins(&out, 0), set([base(0)]));
    assert_eq!(origins(&out, 1), set([introduced("d")]));
    assert_eq!(out.set_aside.len(), 1);
    assert_eq!(agreed(&out, 1).1, Some(ChainClass::ExactFromBase));
}

#[test]
fn an_occurrence_choice_whose_branches_use_different_edits_is_ambiguous() {
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
        ambiguous(&out, 1),
        vec![set([introduced("p")]), set([introduced("d")])]
    );
    assert!(out.set_aside.is_empty());
}

#[test]
fn the_whole_sequence_and_the_deletion_alone_disagree_on_the_remaining_line() {
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
    let readings = ambiguous(&out, 0);
    assert!(readings.contains(&set([base(1)])));
    assert!(readings.contains(&set([introduced("e3")])));
}

#[test]
fn an_unknown_hash_deletion_without_a_verified_chain_is_ambiguous_across_positions() {
    // The base is never reached, so no candidate position is set aside;
    // the positions disagree on which "y" the earlier edit wrote.
    let out = wt(
        "zz\n",
        "x\ny\n",
        vec![
            candidate(replace("e1", "t", 1, "q\n", "y\n")),
            candidate(replace("e2", "t", 2, "y\n", "")),
        ],
    );
    assert!(out.set_aside.is_empty());
    assert_eq!(out.chains.len(), 4);
    let readings = ambiguous(&out, 1);
    assert!(readings.contains(&set([Origin::Unexplained])));
    assert!(readings.contains(&set([introduced("e1")])));
    assert_eq!(origins(&out, 0), set([Origin::Unexplained]));
}

#[test]
fn an_unknown_hash_deletion_with_a_chain_from_the_base_lists_the_other_positions() {
    let out = wt(
        "c\na\nb\n",
        "a\nb\n",
        vec![candidate(replace("d", "t", 1, "c\n", ""))],
    );
    assert_eq!(out.set_aside.len(), 2);
    assert_eq!(origins(&out, 0), set([base(1)]));
    assert_eq!(origins(&out, 1), set([base(2)]));
    assert_eq!(base_fates(&out, 0), set([Fate::Replaced { edit: id("d") }]));
}

#[test]
fn a_gap_before_the_chain_leaves_inherited_lines_to_content_matching() {
    // E1's recorded after-state is contradicted by a manual b -> q, so the
    // chain stops at E2; E1's whole new_text still occurs in the target.
    let out = wt(
        "a\nb\n",
        "A1\nA2\nQ\n",
        vec![
            candidate(with_hashes(
                replace("e1", "t", 1, "a\n", "A1\nA2\n"),
                "a\nb\n",
                "A1\nA2\nb\n",
            )),
            candidate(replace("e2", "t", 2, "q\n", "Q\n")),
        ],
    );
    assert_eq!(out.chains.len(), 1);
    assert_eq!(out.chains[0].class, ChainClass::Consistent);
    assert_eq!(origins(&out, 0), set([Origin::Unexplained]));
    assert_eq!(origins(&out, 2), set([introduced("e2")]));
    assert_eq!(out.content.target[0], set([id("e1")]));
    assert_eq!(out.content.target[1], set([id("e1")]));
}

#[test]
fn content_matches_cover_whole_lines_and_ignore_blocks_without_letters() {
    let out = wt(
        "old line\nkeep\n",
        "fn a() {\n    let x = 1;\n}\nmid\n}\n",
        vec![
            candidate(unknown_op(
                "whole",
                "t",
                1,
                "old line\n",
                "fn a() {\n    let x = 1;\n}\n",
            )),
            candidate(unknown_op("part", "t", 2, "k", "x")),
            candidate(unknown_op("brace", "t", 3, "z", "}\n")),
        ],
    );
    assert_eq!(out.content.target[0], set([id("whole")]));
    assert_eq!(out.content.target[1], set([id("whole")]));
    assert_eq!(out.content.target[2], set([id("whole")]));
    assert!(out.content.target[3].is_empty());
    assert!(out.content.target[4].is_empty());
    assert_eq!(out.content.base[0], set([id("whole")]));
    assert!(out.content.base[1].is_empty());
}

#[test]
fn large_file_single_session_is_exact_from_base() {
    // Over 10 KB: Claude Code records no originalFile, so no hashes, but it
    // keeps each edit's patch. Without the patch the deletion would be a
    // candidate at every line start and the search would run out.
    let original = large_file(500);
    assert!(original.len() > 10 * 1024);
    let steps = [
        ("e1", "line 0010 of the file\n", "line 0010 renamed\n"),
        (
            "e2",
            "line 0100 of the file\n",
            "line 0100 of the file\nan inserted line\n",
        ),
        ("e3", "line 0200 of the file\n", ""),
        ("e4", "line 0010 renamed\n", "line 0010 renamed twice\n"),
    ];
    let mut edits = Vec::new();
    let mut target = original.clone();
    for ((name, old, new), ordinal) in steps.into_iter().zip(1u64..) {
        let next = target.replacen(old, new, 1);
        edits.push(candidate(with_patch(
            replace(name, "t", ordinal, old, new),
            &target,
            &next,
        )));
        target = next;
    }
    let out = wt(&original, &target, edits);
    assert_eq!(out.status, PathStatus::Composed);
    assert_eq!(out.chains.len(), 1);
    assert_eq!(out.chains[0].class, ChainClass::ExactFromBase);
    assert_eq!(origins(&out, 10), set([introduced("e4")]));
    assert_eq!(agreed(&out, 10).0.earlier, set([id("e1")]));
    assert_eq!(origins(&out, 101), set([introduced("e2")]));
    assert_eq!(origins(&out, 100), set([base(100)]));
    assert_eq!(
        base_fates(&out, 200),
        set([Fate::Replaced { edit: id("e3") }])
    );
}

#[test]
fn large_file_with_a_manual_change_is_consistent() {
    let original = large_file(500);
    let target = original
        .replace("line 0010 of the file\n", "line 0010 renamed\n")
        .replace("line 0150 of the file\n", "line 0150 edited by hand\n");
    let out = wt(
        &original,
        &target,
        vec![candidate(replace(
            "e1",
            "t",
            1,
            "line 0010 of the file\n",
            "line 0010 renamed\n",
        ))],
    );
    assert!(out.chains.iter().all(|c| c.class == ChainClass::Consistent));
    assert_eq!(
        agreed(&out, 10),
        (
            &Provenance {
                origins: set([introduced("e1")]),
                ..Provenance::default()
            },
            Some(ChainClass::Consistent)
        )
    );
    assert_eq!(origins(&out, 150), set([Origin::Unexplained]));
}

/// `lines` distinct lines of 22 bytes: line `i` reads `line {i:04} of the file`.
fn large_file(lines: usize) -> String {
    let mut text = String::new();
    for i in 0..lines {
        writeln!(text, "line {i:04} of the file").unwrap();
    }
    text
}

#[test]
fn an_unknown_operation_stops_the_walk() {
    let out = wt(
        "a\n",
        "b\n",
        vec![candidate(unknown_op("u", "t", 1, "a\n", "b\n"))],
    );
    assert_eq!(
        out.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::OperationUnknown { edit: id("u") }]
        }
    );
}

#[test]
fn an_overwrite_whose_previous_content_is_not_the_base_stops_but_content_matches() {
    let out = wt(
        "old\n",
        "new content\n",
        vec![candidate(overwrite("w", "t", 1, "new content\n"))],
    );
    assert_eq!(
        out.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::WholeFileWrite { edit: id("w") }]
        }
    );
    assert_eq!(out.content.target[0], set([id("w")]));
}

#[test]
fn a_replace_all_whose_joint_inverse_fails_stops() {
    let out = wt(
        "aa",
        "aaa",
        vec![candidate(with_hashes(
            replace_all("r", "t", 1, "a", "aa"),
            "aa",
            "aaa",
        ))],
    );
    assert_eq!(
        out.status,
        PathStatus::NotComposed {
            reasons: vec![StopReason::ReplaceAllUnverified { edit: id("r") }]
        }
    );
}

#[test]
fn an_exhausted_budget_leaves_nothing_exact() {
    let mut req = request(
        Some("c\na\nb\n"),
        Some("a\nb\n"),
        TargetKind::WorkingTree,
        vec![candidate(replace("d", "t", 1, "c\n", ""))],
    );
    req.budget_bytes = 20;
    assert_eq!(attribute(req).status, PathStatus::SearchIncomplete);
}

#[test]
fn a_join_then_a_split_keeps_each_characters_own_history() {
    // From a b: E changes a to A, J joins the lines, S splits them again.
    // Provenance travels with the characters, so the b line never names E.
    let out = wt(
        "a\nb\n",
        " A\n b\n",
        vec![
            candidate(replace("E", "t", 1, "a\n", "A\n")),
            candidate(replace("J", "t", 2, "A\nb\n", "A b\n")),
            candidate(replace("S", "t", 3, "A b\n", " A\n b\n")),
        ],
    );
    let (first, _) = agreed(&out, 0);
    assert_eq!(first.origins, set([introduced("E")]));
    assert_eq!(first.whitespace_only, set([id("J"), id("S")]));
    let (second, _) = agreed(&out, 1);
    assert_eq!(second.origins, set([base(1)]));
    assert_eq!(second.whitespace_only, set([id("J"), id("S")]));
    assert!(!second.earlier.contains(&id("E")));
}

#[test]
fn chains_naming_different_whitespace_contributors_are_ambiguous() {
    // Two transcripts each indent the line; both chains say base line 0,
    // but through different edits, so nothing is agreed.
    let out = wt(
        "a\n",
        " a\n",
        vec![
            candidate(with_hashes(
                replace("F", "t1", 1, "a\n", " a\n"),
                "a\n",
                " a\n",
            )),
            candidate(with_hashes(
                replace("G", "t2", 1, "a\n", " a\n"),
                "a\n",
                " a\n",
            )),
        ],
    );
    let LineOutcome::Ambiguous { readings } = &out.target[0] else {
        panic!("line 0 is {:?}", out.target[0]);
    };
    let contributors: Vec<(BTreeSet<Origin>, BTreeSet<EventId>)> = readings
        .iter()
        .map(|r| (r.value.origins.clone(), r.value.whitespace_only.clone()))
        .collect();
    assert_eq!(
        contributors,
        vec![
            (set([base(0)]), set([id("F")])),
            (set([base(0)]), set([id("G")])),
        ]
    );
    assert!(matches!(out.base[0], BaseLineOutcome::Ambiguous { .. }));
}

#[test]
fn mixed_alignment_choices_make_the_joined_line_ambiguous() {
    // Replace-all a -> x, the y pair replaced by an x pair, each x pair
    // contracted to one x, then the two joined: the contraction's pairs
    // choose their surviving line independently, so the joined line may
    // hold E1 twice, E2 twice, or one of each.
    let states = [
        "a\ny\ny\na\n",
        "x\ny\ny\nx\n",
        "x\nx\nx\nx\n",
        "x\nx\n",
        "x x\n",
    ];
    let out = wt(
        states[0],
        states[4],
        vec![
            candidate(with_hashes(
                replace_all("e1", "t", 1, "a", "x"),
                states[0],
                states[1],
            )),
            candidate(with_hashes(
                replace("e2", "t", 2, "y\ny\n", "x\nx\n"),
                states[1],
                states[2],
            )),
            candidate(with_hashes(
                replace_all("e3", "t", 3, "x\nx\n", "x\n"),
                states[2],
                states[3],
            )),
            candidate(with_hashes(
                replace("e4", "t", 4, "x\nx\n", "x x\n"),
                states[3],
                states[4],
            )),
        ],
    );
    let readings = ambiguous(&out, 0);
    assert!(readings.contains(&set([introduced("e1")])));
    assert!(readings.contains(&set([introduced("e2")])));
    assert!(readings.contains(&set([introduced("e1"), introduced("e2")])));
}

#[test]
fn content_matching_skips_lines_a_chain_explains() {
    // The chain explains line 0, so its whole-block match is not needed.
    let out = wt(
        "a\n",
        "b\n",
        vec![candidate(replace("e1", "t", 1, "a\n", "b\n"))],
    );
    assert_eq!(origins(&out, 0), set([introduced("e1")]));
    assert!(out.content.target[0].is_empty());
}

#[test]
fn content_matching_is_skipped_when_the_search_is_incomplete() {
    let mut req = request(
        Some("a\n"),
        Some("b\n"),
        TargetKind::WorkingTree,
        vec![candidate(unknown_op("u", "t", 1, "a\n", "b\n"))],
    );
    req.budget_bytes = 3;
    let out = attribute(req);
    assert_eq!(out.status, PathStatus::SearchIncomplete);
    assert!(out.content.target.iter().all(BTreeSet::is_empty));
}

#[test]
fn content_matching_a_huge_target_is_linear() {
    // 1 MiB of identical lines and a one-line block: every line is a match,
    // found without comparing every occurrence against every line.
    let target = "a\n".repeat(524_288);
    let req = request(
        Some("b\n"),
        Some(&target),
        TargetKind::WorkingTree,
        vec![candidate(unknown_op("u", "t", 1, "q\n", "a\n"))],
    );
    let out = finishes_within(Duration::from_secs(60), move || attribute(req));
    assert_eq!(out.content.target.len(), 524_288);
    assert!(out.content.target.iter().all(|m| m.contains(&id("u"))));
}

#[test]
fn content_matching_that_runs_out_is_marked_incomplete() {
    // The walk needs 1 KiB of the 4 KiB budget; matching has its own 4 KiB
    // and runs out while indexing 6,000 bytes of target lines.
    let target = "a\n".repeat(3_000);
    let mut req = request(
        Some("b\n"),
        Some(&target),
        TargetKind::WorkingTree,
        vec![candidate(unknown_op("u", "t", 1, "q\n", "a\n"))],
    );
    req.budget_bytes = 4096;
    let out = attribute(req);
    assert!(matches!(out.status, PathStatus::NotComposed { .. }));
    assert!(out.content.incomplete);
    assert!(!out.content.target.iter().all(|m| m.contains(&id("u"))));
}

#[test]
fn repeated_block_matches_are_charged_once() {
    // A 50,000-line block matches at about 50,000 overlapping positions:
    // marking every match's lines would be 2.5 billion marks.
    let target = "a\n".repeat(100_000);
    let block = "a\n".repeat(50_000);
    let req = request(
        Some("b\n"),
        Some(&target),
        TargetKind::WorkingTree,
        vec![candidate(unknown_op("u", "t", 1, "q\n", &block))],
    );
    let out = finishes_within(Duration::from_secs(20), move || attribute(req));
    // Charged once, the union fits the default budget: matching completes,
    // and an implementation that charged every overlapping match would
    // stop early and fail here.
    assert!(!out.content.incomplete);
    assert_eq!(out.content.target.len(), 100_000);
    assert!(out.content.target.iter().all(|m| m.contains(&id("u"))));
}

/// `count` lines reading `v{step}`: the file after `step` rewrites.
fn rewritten(step: usize, count: usize) -> String {
    format!("v{step}\n").repeat(count)
}

#[test]
fn a_long_rewrite_history_is_replayed_in_bounded_time() {
    // 1,500 edits each rewrite all 200 lines, so every line's earlier
    // contributors grow to 1,499 edits; copying them for every line at
    // every step would be hundreds of millions of copies.
    let (lines, steps) = (200, 1_500);
    let edits = (1..=steps)
        .zip(1u64..)
        .map(|(k, ordinal)| {
            candidate(replace(
                &format!("e{k}"),
                "t",
                ordinal,
                &rewritten(k - 1, lines),
                &rewritten(k, lines),
            ))
        })
        .collect();
    let req = request(
        Some(&rewritten(0, lines)),
        Some(&rewritten(steps, lines)),
        TargetKind::WorkingTree,
        edits,
    );
    let out = finishes_within(Duration::from_secs(20), move || attribute(req));
    assert_eq!(out.status, PathStatus::Composed);
    let (provenance, class) = agreed(&out, lines - 1);
    assert_eq!(class, Some(ChainClass::ExactFromBase));
    assert_eq!(provenance.origins, set([introduced(&format!("e{steps}"))]));
    assert_eq!(
        provenance.earlier,
        (1..steps).map(|k| id(&format!("e{k}"))).collect()
    );
    assert_eq!(
        base_fates(&out, 0),
        set([Fate::Replaced { edit: id("e1") }])
    );
}

/// `count` lines `x` padded with `step` in binary, a space for 0 and a tab
/// for 1: every step changes only whitespace.
fn respaced(step: usize, count: usize) -> String {
    let pad: String = (0..12)
        .map(|bit| if step >> bit & 1 == 1 { '\t' } else { ' ' })
        .collect();
    format!("x{pad}\n").repeat(count)
}

#[test]
fn a_long_whitespace_history_is_replayed_in_bounded_time() {
    // 1,500 whitespace-only edits each respace all 200 lines, so every
    // character gathers 1,500 whitespace-only contributors.
    let (lines, steps) = (200, 1_500);
    let edits = (1..=steps)
        .zip(1u64..)
        .map(|(k, ordinal)| {
            candidate(replace(
                &format!("w{k}"),
                "t",
                ordinal,
                &respaced(k - 1, lines),
                &respaced(k, lines),
            ))
        })
        .collect();
    let req = request(
        Some(&respaced(0, lines)),
        Some(&respaced(steps, lines)),
        TargetKind::WorkingTree,
        edits,
    );
    let out = finishes_within(Duration::from_secs(20), move || attribute(req));
    assert_eq!(out.status, PathStatus::Composed);
    let all: BTreeSet<EventId> = (1..=steps).map(|k| id(&format!("w{k}"))).collect();
    let (provenance, _) = agreed(&out, lines - 1);
    assert_eq!(provenance.origins, set([base(lines - 1)]));
    assert_eq!(provenance.whitespace_only, all);
    assert_eq!(
        base_fates(&out, lines - 1),
        set([Fate::CarriedTo {
            line: lines - 1,
            via: all
        }])
    );
}

#[test]
fn kept_lines_with_long_histories_are_carried_in_bounded_time() {
    // 400 rewrites give each of 400 lines 399 earlier contributors; 1,000
    // edits then each change one other line and keep those 400.
    let (lines, rewrites, others) = (400, 400, 1_000);
    let tail = |changed: usize| -> String {
        (0..others)
            .map(|j| {
                if j < changed {
                    format!("d{j}\n")
                } else {
                    format!("c{j}\n")
                }
            })
            .collect()
    };
    let mut edits = Vec::new();
    let mut ordinals = 1u64..;
    for k in 1..=rewrites {
        let old = rewritten(k - 1, lines);
        let new = rewritten(k, lines);
        edits.push(candidate(replace(
            &format!("r{k}"),
            "t",
            ordinals.next().unwrap(),
            &old,
            &new,
        )));
    }
    for j in 0..others {
        edits.push(candidate(replace(
            &format!("c{j}"),
            "t",
            ordinals.next().unwrap(),
            &format!("c{j}\n"),
            &format!("d{j}\n"),
        )));
    }
    let base_text = rewritten(0, lines) + &tail(0);
    let target_text = rewritten(rewrites, lines) + &tail(others);
    let req = request(
        Some(&base_text),
        Some(&target_text),
        TargetKind::WorkingTree,
        edits,
    );
    let out = finishes_within(Duration::from_secs(20), move || attribute(req));
    assert_eq!(out.status, PathStatus::Composed);
    let (provenance, _) = agreed(&out, 0);
    assert_eq!(
        provenance.origins,
        set([introduced(&format!("r{rewrites}"))])
    );
    assert_eq!(provenance.earlier.len(), rewrites - 1);
    assert_eq!(origins(&out, lines + others - 1), set([introduced("c999")]));
}

#[test]
fn every_chains_line_outcomes_are_charged() {
    // A bucket of five independent edits gives 120 orders, each a chain that
    // stops before the base. Each chain's outcomes for 10,000 base lines are
    // charged, so a budget that holds the walk does not hold them all.
    let edits = ["p", "q", "r", "s", "t"]
        .into_iter()
        .zip(["a", "b", "c", "d", "e"])
        .map(|(old, new)| candidate(replace(&format!("e{new}"), "t", 1, old, new)))
        .collect();
    let mut req = request(
        Some(&"base line\n".repeat(10_000)),
        Some("abcde\n"),
        TargetKind::WorkingTree,
        edits,
    );
    req.budget_bytes = 2 * 1024 * 1024;
    let walked = walk(&req);
    assert_eq!(walked.status, PathStatus::Composed);
    assert_eq!(walked.chains.len(), 120);
    let out = attribute(req);
    assert_eq!(out.status, PathStatus::SearchIncomplete);
    assert_eq!(out.chains.len(), 120);
}

#[test]
fn reading_a_block_is_charged_before_it_is_matched() {
    // Reading a 200,000-byte block alone exceeds the 64 KiB budget, so the
    // matching is incomplete, never "no match".
    let block = "a\n".to_owned() + &"z\n".repeat(100_000);
    let mut req = request(
        Some("b\n"),
        Some("a\n"),
        TargetKind::WorkingTree,
        vec![candidate(unknown_op("u", "t", 1, "q\n", &block))],
    );
    req.budget_bytes = 64 * 1024;
    let out = attribute(req);
    assert!(matches!(out.status, PathStatus::NotComposed { .. }));
    assert!(out.content.incomplete);
}

#[test]
fn indexing_is_charged_for_each_lines_bookkeeping() {
    // 100,000 two-byte lines are 200,000 bytes, but indexing them takes
    // several times that in line tables: a 1 MiB budget holds the bytes and
    // not the tables.
    let target = "a\n".repeat(100_000);
    let mut req = request(
        Some("b\n"),
        Some(&target),
        TargetKind::WorkingTree,
        vec![candidate(unknown_op("u", "t", 1, "q\n", "a\n"))],
    );
    req.budget_bytes = 1024 * 1024;
    let out = attribute(req);
    assert!(matches!(out.status, PathStatus::NotComposed { .. }));
    assert!(out.content.incomplete);
}
