//! Property tests for decision extraction in context output (task 11).
//!
//! Validates that Decision components with Rationale and Alternative children
//! are correctly extracted into `DecisionContext` in the `ContextOutput`.

use std::collections::HashMap;

use proptest::prelude::*;

use crate::ExtractedComponent;
use crate::graph::tests::generators::{
    arb_component_id, arb_id, make_doc, pos, single_project_config,
};
use crate::graph::{ALTERNATIVE, DECISION, RATIONALE, build_graph};

// ---------------------------------------------------------------------------
// Helper: build Decision/Rationale/Alternative components
// ---------------------------------------------------------------------------

fn make_decision(id: &str, children: Vec<ExtractedComponent>, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: DECISION.to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children,
        body_text: Some(format!("decision {id}")),
        code_blocks: Vec::new(),
        position: pos(line),
    }
}

fn make_rationale(body: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: RATIONALE.to_owned(),
        attributes: HashMap::new(),
        children: Vec::new(),
        body_text: Some(body.to_owned()),
        code_blocks: Vec::new(),
        position: pos(line),
    }
}

fn make_alternative(id: &str, status: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: ALTERNATIVE.to_owned(),
        attributes: HashMap::from([
            ("id".to_owned(), id.to_owned()),
            ("status".to_owned(), status.to_owned()),
        ]),
        children: Vec::new(),
        body_text: Some(format!("alternative {id}")),
        code_blocks: Vec::new(),
        position: pos(line),
    }
}

// ---------------------------------------------------------------------------
// Property: Document with Decision components produces decisions in context
// ---------------------------------------------------------------------------

proptest! {
    /// A document containing a Decision with Rationale and Alternatives should
    /// produce a `decisions` entry in ContextOutput with the correct fields.
    #[test]
    fn prop_context_extracts_decisions_with_rationale_and_alternatives(
        doc_id in arb_id(),
        decision_id in arb_component_id(),
        alt_a_id in arb_component_id(),
        alt_b_id in arb_component_id(),
    ) {
        // Ensure all component IDs are distinct.
        prop_assume!(decision_id != alt_a_id);
        prop_assume!(decision_id != alt_b_id);
        prop_assume!(alt_a_id != alt_b_id);

        let config = single_project_config();

        let rationale = make_rationale("the rationale", 2);
        let alt_a = make_alternative(&alt_a_id, "rejected", 3);
        let alt_b = make_alternative(&alt_b_id, "deferred", 4);
        let decision = make_decision(&decision_id, vec![rationale, alt_a, alt_b], 1);
        let doc = make_doc(&doc_id, vec![decision]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&doc_id).expect("context should succeed");

        // Should have exactly one decision.
        prop_assert_eq!(ctx.decisions.len(), 1);

        let dec = &ctx.decisions[0];
        prop_assert_eq!(&dec.id, &decision_id);
        let expected_body = format!("decision {decision_id}");
        prop_assert_eq!(dec.body_text.as_deref(), Some(expected_body.as_str()));
        prop_assert_eq!(dec.rationale_text.as_deref(), Some("the rationale"));
        prop_assert_eq!(dec.alternatives.len(), 2);

        let alt_a_ctx = dec.alternatives.iter().find(|a| a.id == alt_a_id);
        prop_assert!(alt_a_ctx.is_some(), "alt_a should be in alternatives");
        let alt_a_ctx = alt_a_ctx.unwrap();
        prop_assert_eq!(&alt_a_ctx.status, "rejected");
        let expected_alt_body = format!("alternative {alt_a_id}");
        prop_assert_eq!(alt_a_ctx.body_text.as_deref(), Some(expected_alt_body.as_str()));

        let alt_b_ctx = dec.alternatives.iter().find(|a| a.id == alt_b_id);
        prop_assert!(alt_b_ctx.is_some(), "alt_b should be in alternatives");
        let alt_b_ctx = alt_b_ctx.unwrap();
        prop_assert_eq!(&alt_b_ctx.status, "deferred");
    }

    /// A document with no Decision components has empty decisions.
    #[test]
    fn prop_context_no_decisions_produces_empty(
        doc_id in arb_id(),
    ) {
        let config = single_project_config();
        let doc = make_doc(&doc_id, vec![]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&doc_id).expect("context should succeed");

        prop_assert!(ctx.decisions.is_empty(), "decisions should be empty for doc with no Decision components");
    }

    /// A Decision without a Rationale child should have `rationale_text: None`.
    #[test]
    fn prop_context_decision_without_rationale(
        doc_id in arb_id(),
        decision_id in arb_component_id(),
    ) {
        let config = single_project_config();

        let decision = make_decision(&decision_id, vec![], 1);
        let doc = make_doc(&doc_id, vec![decision]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&doc_id).expect("context should succeed");

        prop_assert_eq!(ctx.decisions.len(), 1);
        let dec = &ctx.decisions[0];
        prop_assert_eq!(&dec.id, &decision_id);
        prop_assert!(dec.rationale_text.is_none(), "rationale_text should be None when no Rationale child");
        prop_assert!(dec.alternatives.is_empty(), "alternatives should be empty when no Alternative children");
    }
}
