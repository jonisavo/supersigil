//! Property tests for linked decisions in context output (task 12).
//!
//! Validates that when doc A contains a Decision with a nested References
//! component whose `refs` target doc B, the context output for doc B includes
//! doc A's decision in `linked_decisions`.

use std::collections::HashMap;

use proptest::prelude::*;

use crate::ExtractedComponent;
use crate::graph::tests::generators::{
    arb_component_id, arb_id, make_doc, make_refs_component, pos, single_project_config,
};
use crate::graph::{DECISION, REFERENCES, build_graph};

// ---------------------------------------------------------------------------
// Helper: build Decision with optional children
// ---------------------------------------------------------------------------

fn make_decision(id: &str, children: Vec<ExtractedComponent>, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: DECISION.to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children,
        body_text: Some(format!("decision {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: pos(line),
    }
}

// ---------------------------------------------------------------------------
// Property: Decision referencing doc B appears in doc B's linked_decisions
// ---------------------------------------------------------------------------

proptest! {
    /// When doc A contains a Decision with a nested References targeting doc B,
    /// context output for doc B includes that decision in `linked_decisions`.
    #[test]
    fn prop_linked_decision_appears_in_target_context(
        doc_a_id in arb_id(),
        doc_b_id in arb_id(),
        decision_id in arb_component_id(),
    ) {
        prop_assume!(doc_a_id != doc_b_id);

        let config = single_project_config();

        // Doc B: the target (no components needed).
        let doc_b = make_doc(&doc_b_id, vec![]);

        // Doc A: contains a Decision with a nested References pointing to doc B.
        let refs_comp = make_refs_component(REFERENCES, &doc_b_id, 2);
        let decision = make_decision(&decision_id, vec![refs_comp], 1);
        let doc_a = make_doc(&doc_a_id, vec![decision]);

        let graph = build_graph(vec![doc_a, doc_b], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&doc_b_id).expect("context should succeed");

        // linked_decisions should contain the decision from doc A.
        prop_assert!(
            !ctx.linked_decisions.is_empty(),
            "linked_decisions should not be empty"
        );
        let linked = ctx.linked_decisions.iter().find(|ld| ld.decision_id == decision_id);
        prop_assert!(linked.is_some(), "should find decision {decision_id} in linked_decisions");
        let linked = linked.unwrap();
        prop_assert_eq!(&linked.source_doc_id, &doc_a_id);
        let expected_body = format!("decision {decision_id}");
        prop_assert_eq!(linked.body_text.as_deref(), Some(expected_body.as_str()));
    }

    /// When no Decision references doc B, `linked_decisions` is empty.
    #[test]
    fn prop_no_linked_decisions_when_none_reference(
        doc_id in arb_id(),
    ) {
        let config = single_project_config();
        let doc = make_doc(&doc_id, vec![]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&doc_id).expect("context should succeed");

        prop_assert!(
            ctx.linked_decisions.is_empty(),
            "linked_decisions should be empty when no Decision references this doc"
        );
    }

    /// A Decision that references a *different* doc should NOT appear in
    /// the target's linked_decisions.
    #[test]
    fn prop_linked_decision_only_for_matching_target(
        doc_a_id in arb_id(),
        doc_b_id in arb_id(),
        doc_c_id in arb_id(),
        decision_id in arb_component_id(),
    ) {
        prop_assume!(doc_a_id != doc_b_id);
        prop_assume!(doc_a_id != doc_c_id);
        prop_assume!(doc_b_id != doc_c_id);

        let config = single_project_config();

        // Doc B and Doc C: targets.
        let doc_b = make_doc(&doc_b_id, vec![]);
        let doc_c = make_doc(&doc_c_id, vec![]);

        // Doc A: Decision references doc C only.
        let refs_comp = make_refs_component(REFERENCES, &doc_c_id, 2);
        let decision = make_decision(&decision_id, vec![refs_comp], 1);
        let doc_a = make_doc(&doc_a_id, vec![decision]);

        let graph = build_graph(vec![doc_a, doc_b, doc_c], &config)
            .expect("build_graph should succeed");

        // Doc B should have empty linked_decisions.
        let ctx_b = graph.context(&doc_b_id).expect("context should succeed");
        prop_assert!(
            ctx_b.linked_decisions.is_empty(),
            "doc B should have no linked decisions when doc A's Decision references doc C"
        );

        // Doc C should have the linked decision.
        let ctx_c = graph.context(&doc_c_id).expect("context should succeed");
        prop_assert!(
            !ctx_c.linked_decisions.is_empty(),
            "doc C should have linked decisions from doc A"
        );
    }

    /// JSON serialization includes the `linked_decisions` field.
    #[test]
    fn prop_linked_decisions_in_json_serialization(
        doc_a_id in arb_id(),
        doc_b_id in arb_id(),
        decision_id in arb_component_id(),
    ) {
        prop_assume!(doc_a_id != doc_b_id);

        let config = single_project_config();

        let doc_b = make_doc(&doc_b_id, vec![]);
        let refs_comp = make_refs_component(REFERENCES, &doc_b_id, 2);
        let decision = make_decision(&decision_id, vec![refs_comp], 1);
        let doc_a = make_doc(&doc_a_id, vec![decision]);

        let graph = build_graph(vec![doc_a, doc_b], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&doc_b_id).expect("context should succeed");

        let json = serde_json::to_string(&ctx).expect("JSON serialization should succeed");
        prop_assert!(
            json.contains("linked_decisions"),
            "JSON should contain 'linked_decisions' field"
        );
        prop_assert!(
            json.contains(&decision_id),
            "JSON should contain the decision ID"
        );
        prop_assert!(
            json.contains(&doc_a_id),
            "JSON should contain the source doc ID"
        );
    }

    /// Multiple decisions from different documents referencing the same target
    /// all appear in linked_decisions.
    #[test]
    fn prop_multiple_linked_decisions_from_different_docs(
        doc_a_id in arb_id(),
        doc_b_id in arb_id(),
        target_id in arb_id(),
        dec_a_id in arb_component_id(),
        dec_b_id in arb_component_id(),
    ) {
        prop_assume!(doc_a_id != doc_b_id);
        prop_assume!(doc_a_id != target_id);
        prop_assume!(doc_b_id != target_id);
        prop_assume!(dec_a_id != dec_b_id);

        let config = single_project_config();

        let target_doc = make_doc(&target_id, vec![]);

        // Doc A: Decision referencing target.
        let refs_a = make_refs_component(REFERENCES, &target_id, 2);
        let decision_a = make_decision(&dec_a_id, vec![refs_a], 1);
        let doc_a = make_doc(&doc_a_id, vec![decision_a]);

        // Doc B: Decision referencing target.
        let refs_b = make_refs_component(REFERENCES, &target_id, 2);
        let decision_b = make_decision(&dec_b_id, vec![refs_b], 1);
        let doc_b = make_doc(&doc_b_id, vec![decision_b]);

        let graph = build_graph(vec![doc_a, doc_b, target_doc], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&target_id).expect("context should succeed");

        prop_assert_eq!(
            ctx.linked_decisions.len(), 2,
            "should have 2 linked decisions: {:?}", ctx.linked_decisions
        );
        prop_assert!(
            ctx.linked_decisions.iter().any(|ld| ld.decision_id == dec_a_id && ld.source_doc_id == doc_a_id),
            "should find decision from doc A"
        );
        prop_assert!(
            ctx.linked_decisions.iter().any(|ld| ld.decision_id == dec_b_id && ld.source_doc_id == doc_b_id),
            "should find decision from doc B"
        );
    }
}
