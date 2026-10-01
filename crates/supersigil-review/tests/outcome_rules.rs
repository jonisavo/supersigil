//! One test per ordered outcome rule, for added (target) and removed (base)
//! diff lines.

mod common;

use std::collections::BTreeSet;

use common::{
    agreed, attribution, base_line, chain, eid, fates, introduced, kept, no_fate, replaced,
    unexplained,
};
use serde_json::json;
use supersigil_anchor::{
    ChainClass, ChainEnd, Fate, LineOutcome, Origin, PathStatus, Provenance, Reading,
};
use supersigil_review::outcome::{
    AttributedEdit, AttributionState, Outcome, Relation, UnattributedReason, base_line_outcome,
    target_line_outcome,
};

fn exact_chain() -> Vec<supersigil_anchor::Chain> {
    vec![chain(0, ChainClass::ExactFromBase, &["e1"], ChainEnd::Base)]
}

#[test]
fn search_incomplete_is_unresolved_even_with_an_attributed_reading() {
    let mut attr = attribution(exact_chain(), vec![introduced("e1")], vec![]);
    attr.status = PathStatus::SearchIncomplete;
    let (outcome, provenance) =
        target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Unresolved {
            candidates: provenance.clone()
        }
    );
    assert_eq!(provenance["kind"], "agreed");
}

#[test]
fn ambiguous_line_is_ambiguous_with_every_reading() {
    let readings = vec![
        Reading {
            chain: 0,
            class: ChainClass::ExactFromBase,
            value: Provenance {
                origins: BTreeSet::from([Origin::Introduced(eid("e1"))]),
                ..Provenance::default()
            },
        },
        Reading {
            chain: 1,
            class: ChainClass::ExactFromBase,
            value: Provenance {
                origins: BTreeSet::from([Origin::Base(1)]),
                ..Provenance::default()
            },
        },
    ];
    let attr = attribution(
        exact_chain(),
        vec![LineOutcome::Ambiguous {
            readings: readings.clone(),
        }],
        vec![],
    );
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"b\n"]);
    assert_eq!(
        outcome,
        Outcome::Ambiguous {
            readings: serde_json::to_value(&readings).unwrap()
        }
    );
}

#[test]
fn unexplained_line_with_covering_edits_is_a_content_match() {
    let mut attr = attribution(exact_chain(), vec![unexplained()], vec![]);
    attr.content.target[0] = BTreeSet::from([eid("e7"), eid("e3")]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::ContentMatch {
            relation: Relation::Introduced,
            edits: vec![eid("e3"), eid("e7")],
            search_complete: true,
        }
    );
}

#[test]
fn unexplained_line_without_chains_is_unattributed_with_anchor_status() {
    let attr = attribution(vec![], vec![unexplained()], vec![]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Unattributed {
            reason: UnattributedReason::NoSurvivingChain {
                status: attr.status.clone()
            }
        }
    );
}

#[test]
fn unexplained_line_inherited_across_a_gap_is_unattributed() {
    let chains = vec![chain(
        0,
        ChainClass::ExactFromStart,
        &["e2"],
        ChainEnd::Stopped {
            reasons: vec![supersigil_anchor::StopReason::NoPredecessor],
        },
    )];
    let attr = attribution(chains, vec![unexplained()], vec![]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Unattributed {
            reason: UnattributedReason::GapBeforeChain
        }
    );
}

#[test]
fn partly_unexplained_line_is_never_attributed() {
    let line = agreed(
        [Origin::Introduced(eid("e1")), Origin::Unexplained],
        Some(ChainClass::ExactFromStart),
    );
    let attr = attribution(exact_chain(), vec![line], vec![]);
    let (outcome, provenance) =
        target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Unattributed {
            reason: UnattributedReason::GapBeforeChain
        }
    );
    // The explained origin stays visible in the provenance.
    assert!(provenance.to_string().contains("\"introduced\""));
}

#[test]
fn introduced_line_is_attributed_with_its_class() {
    let attr = attribution(exact_chain(), vec![introduced("e1")], vec![]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"x\n"]);
    assert_eq!(
        outcome,
        Outcome::Attributed {
            edits: vec![AttributedEdit {
                edit: eid("e1"),
                relation: Relation::Introduced,
                classes: BTreeSet::from([ChainClass::ExactFromBase]),
            }]
        }
    );
}

#[test]
fn whitespace_contributors_make_a_whitespace_only_change() {
    let line = LineOutcome::Agreed {
        provenance: Provenance {
            origins: BTreeSet::from([Origin::Base(0)]),
            whitespace_only: BTreeSet::from([eid("fmt")]),
            ..Provenance::default()
        },
        class: Some(ChainClass::ExactFromBase),
    };
    let attr = attribution(exact_chain(), vec![line], vec![]);
    let (outcome, _) = target_line_outcome(
        &AttributionState::Available(&attr),
        0,
        &[b"a=1\n"],
        &[b"a = 1\n"],
    );
    assert_eq!(
        outcome,
        Outcome::WhitespaceOnly {
            edits: vec![eid("fmt")]
        }
    );
}

#[test]
fn whitespace_added_line_is_a_whitespace_only_change() {
    let line = agreed(
        [Origin::WhitespaceAdded(eid("fmt"))],
        Some(ChainClass::ExactFromBase),
    );
    let attr = attribution(exact_chain(), vec![line], vec![]);
    let (outcome, _) = target_line_outcome(&AttributionState::Available(&attr), 0, &[], &[b"\n"]);
    assert_eq!(
        outcome,
        Outcome::WhitespaceOnly {
            edits: vec![eid("fmt")]
        }
    );
}

#[test]
fn base_line_with_identical_blob_is_realigned() {
    let attr = attribution(exact_chain(), vec![base_line(2)], vec![]);
    let base: [&[u8]; 3] = [b"a\n", b"b\n", b"x\n"];
    let (outcome, _) =
        target_line_outcome(&AttributionState::Available(&attr), 0, &base, &[b"x\n"]);
    assert_eq!(outcome, Outcome::Realigned { line: 2 });
}

#[test]
fn base_line_whose_blob_differs_is_a_line_ending_change() {
    let attr = attribution(exact_chain(), vec![base_line(0)], vec![]);
    let (outcome, _) = target_line_outcome(
        &AttributionState::Available(&attr),
        0,
        &[b"x\n"],
        &[b"x\r\n"],
    );
    assert_eq!(outcome, Outcome::LineEndingChanged);
}

#[test]
fn unavailable_attribution_is_unattributed_with_null_provenance() {
    let state = AttributionState::Unavailable {
        reason: "conversion changed line structure".to_owned(),
    };
    let expected = Outcome::Unattributed {
        reason: UnattributedReason::AttributionUnavailable {
            reason: "conversion changed line structure".to_owned(),
        },
    };
    assert_eq!(
        target_line_outcome(&state, 0, &[], &[b"x\n"]),
        (expected.clone(), serde_json::Value::Null)
    );
    assert_eq!(
        base_line_outcome(&state, 0, &[b"x\n"], &[]),
        (expected, serde_json::Value::Null)
    );
}

#[test]
fn replaced_base_line_is_attributed_as_replaced() {
    let attr = attribution(exact_chain(), vec![], vec![replaced("e1")]);
    let (outcome, _) = base_line_outcome(&AttributionState::Available(&attr), 0, &[b"x\n"], &[]);
    assert_eq!(
        outcome,
        Outcome::Attributed {
            edits: vec![AttributedEdit {
                edit: eid("e1"),
                relation: Relation::Replaced,
                classes: BTreeSet::from([ChainClass::ExactFromBase]),
            }]
        }
    );
}

#[test]
fn base_line_without_fate_falls_back_to_content_match_then_unattributed() {
    let mut attr = attribution(vec![], vec![], vec![no_fate(), no_fate()]);
    attr.content.base[1] = BTreeSet::from([eid("e4")]);
    let state = AttributionState::Available(&attr);
    let (first, _) = base_line_outcome(&state, 0, &[b"a\n", b"b\n"], &[]);
    let (second, _) = base_line_outcome(&state, 1, &[b"a\n", b"b\n"], &[]);
    assert_eq!(
        first,
        Outcome::Unattributed {
            reason: UnattributedReason::NoSurvivingChain {
                status: attr.status.clone()
            }
        }
    );
    assert_eq!(
        second,
        Outcome::ContentMatch {
            relation: Relation::Replaced,
            edits: vec![eid("e4")],
            search_complete: true,
        }
    );
}

#[test]
fn unmatched_lines_are_unattributed_when_content_matching_ran_out() {
    let mut attr = attribution(
        exact_chain(),
        vec![unexplained(), unexplained()],
        vec![no_fate()],
    );
    attr.content.incomplete = true;
    attr.content.target[1] = BTreeSet::from([eid("e3")]);
    let state = AttributionState::Available(&attr);
    let base: &[&[u8]] = &[b"a\n"];
    let target: &[&[u8]] = &[b"x\n", b"y\n"];
    let incomplete = Outcome::Unattributed {
        reason: UnattributedReason::ContentMatchIncomplete,
    };
    assert_eq!(target_line_outcome(&state, 0, base, target).0, incomplete);
    assert_eq!(base_line_outcome(&state, 0, base, target).0, incomplete);
    // A match found before the budget ran out still counts, and keeps that
    // the search did not complete.
    assert_eq!(
        target_line_outcome(&state, 1, base, target).0,
        Outcome::ContentMatch {
            relation: Relation::Introduced,
            edits: vec![eid("e3")],
            search_complete: false,
        }
    );
}

#[test]
fn carried_base_line_is_a_whitespace_only_change() {
    let base = fates([Fate::CarriedTo {
        line: 0,
        via: BTreeSet::from([eid("fmt")]),
    }]);
    let attr = attribution(exact_chain(), vec![], vec![base]);
    let (outcome, _) = base_line_outcome(
        &AttributionState::Available(&attr),
        0,
        &[b"a=1\n"],
        &[b"a = 1\n"],
    );
    assert_eq!(
        outcome,
        Outcome::WhitespaceOnly {
            edits: vec![eid("fmt")]
        }
    );
}

#[test]
fn base_line_removed_by_whitespace_is_a_whitespace_only_change() {
    let base = fates([Fate::RemovedByWhitespace { edit: eid("fmt") }]);
    let attr = attribution(exact_chain(), vec![], vec![base]);
    let (outcome, _) = base_line_outcome(&AttributionState::Available(&attr), 0, &[b"\n"], &[]);
    assert_eq!(
        outcome,
        Outcome::WhitespaceOnly {
            edits: vec![eid("fmt")]
        }
    );
}

#[test]
fn kept_base_line_is_realigned_or_a_line_ending_change() {
    let attr = attribution(exact_chain(), vec![], vec![kept(1)]);
    let state = AttributionState::Available(&attr);
    let (same, _) = base_line_outcome(&state, 0, &[b"x\n"], &[b"a\n", b"x\n"]);
    assert_eq!(same, Outcome::Realigned { line: 1 });
    let (ending, _) = base_line_outcome(&state, 0, &[b"x\n"], &[b"a\n", b"x\r\n"]);
    assert_eq!(ending, Outcome::LineEndingChanged);
}

#[test]
fn provenance_is_the_anchor_outcome_serialized() {
    let attr = attribution(exact_chain(), vec![introduced("e1")], vec![replaced("e1")]);
    let state = AttributionState::Available(&attr);
    let (_, target) = target_line_outcome(&state, 0, &[], &[b"x\n"]);
    assert_eq!(
        target,
        json!({
            "kind": "agreed",
            "provenance": {
                "origins": [{"kind": "introduced", "value": "e1"}],
                "whitespace_only": [],
                "earlier": []
            },
            "class": "exact_from_base"
        })
    );
    let (_, base) = base_line_outcome(&state, 0, &[b"x\n"], &[]);
    assert_eq!(
        base,
        json!({
            "kind": "agreed",
            "fates": [{"kind": "replaced", "edit": "e1"}],
            "class": "exact_from_base"
        })
    );
}

#[test]
fn realigned_line_serializes_one_based() {
    assert_eq!(
        serde_json::to_value(Outcome::Realigned { line: 0 }).unwrap(),
        json!({"kind": "realigned", "line": 1})
    );
    assert_eq!(
        serde_json::to_value(Outcome::Realigned { line: 41 }).unwrap(),
        json!({"kind": "realigned", "line": 42})
    );
}

#[test]
fn search_incomplete_never_attributes_a_removed_line() {
    let mut attr = attribution(exact_chain(), vec![], vec![replaced("e1"), kept(0)]);
    attr.status = PathStatus::SearchIncomplete;
    let state = AttributionState::Available(&attr);
    for line in 0..2 {
        let (outcome, provenance) = base_line_outcome(&state, line, &[b"x\n", b"y\n"], &[b"y\n"]);
        assert_eq!(
            outcome,
            Outcome::Unresolved {
                candidates: provenance
            }
        );
    }
}

#[test]
fn a_content_match_says_whether_its_search_completed() {
    let outcomes = |incomplete| {
        let mut attr = attribution(exact_chain(), vec![unexplained()], vec![no_fate()]);
        attr.content.target[0] = BTreeSet::from([eid("e3")]);
        attr.content.base[0] = BTreeSet::from([eid("e3")]);
        attr.content.incomplete = incomplete;
        let state = AttributionState::Available(&attr);
        let base: &[&[u8]] = &[b"a\n"];
        let target: &[&[u8]] = &[b"x\n"];
        [
            target_line_outcome(&state, 0, base, target).0,
            base_line_outcome(&state, 0, base, target).0,
        ]
        .map(|outcome| serde_json::to_value(outcome).unwrap())
    };
    // The same matches either way, still content matches (the chain search
    // completed), and only the search's completeness differs.
    assert_eq!(
        outcomes(false),
        [
            json!({"kind": "content_match", "relation": "introduced", "edits": ["e3"], "search_complete": true}),
            json!({"kind": "content_match", "relation": "replaced", "edits": ["e3"], "search_complete": true}),
        ]
    );
    assert_eq!(
        outcomes(true),
        [
            json!({"kind": "content_match", "relation": "introduced", "edits": ["e3"], "search_complete": false}),
            json!({"kind": "content_match", "relation": "replaced", "edits": ["e3"], "search_complete": false}),
        ]
    );
}
