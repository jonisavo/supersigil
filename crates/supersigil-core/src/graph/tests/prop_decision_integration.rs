//! Integration tests verifying Decision/Alternative graph pipeline behavior.
//!
//! Covers:
//! - Decision and Alternative are indexed in the component index (referenceable).
//! - Fragment refs `doc#decision-id` and `doc#alternative-id` resolve correctly.
//! - References nested inside Decision produces correct reverse mappings.
//! - `TrackedFiles` nested inside Decision are indexed for staleness.
//! - `DependsOn` nested inside Decision creates document dependency edges.

use proptest::prelude::*;
use std::collections::HashMap;

use crate::graph::tests::generators::{
    arb_component_id, arb_id, make_doc_with_path, make_refs_component,
    make_tracked_files_component, pos, single_project_config,
};
use crate::graph::{ALTERNATIVE, DECISION, REFERENCES, build_graph};
use crate::{ExtractedComponent, SourcePosition};

// ---------------------------------------------------------------------------
// Helper: build a Decision component with optional children
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

fn make_alternative(id: &str, status: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: ALTERNATIVE.to_owned(),
        attributes: HashMap::from([
            ("id".to_owned(), id.to_owned()),
            ("status".to_owned(), status.to_owned()),
        ]),
        children: Vec::new(),
        body_text: Some(format!("alternative {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: SourcePosition {
            byte_offset: line * 40,
            line,
            column: 1,
        },
    }
}

// ---------------------------------------------------------------------------
// Property: Decision and Alternative are indexed as referenceable components
// ---------------------------------------------------------------------------

proptest! {
    /// A Decision with an id attribute should be retrievable from the component
    /// index by (doc_id, decision_id).
    #[test]
    fn prop_decision_indexed_as_referenceable(
        doc_id in arb_id(),
        decision_id in arb_component_id(),
    ) {
        let config = single_project_config();

        let decision = make_decision(&decision_id, vec![], 1);
        let doc = make_doc_with_path(
            &doc_id,
            &format!("specs/{doc_id}.md"),
            vec![decision],
        );

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let looked_up = graph
            .component(&doc_id, &decision_id)
            .expect("Decision should be in the component index");

        prop_assert_eq!(&looked_up.name, DECISION);
        prop_assert_eq!(looked_up.attributes.get("id").unwrap(), &decision_id);
    }

    /// An Alternative (nested inside a Decision) should be retrievable from the
    /// component index by (doc_id, alternative_id).
    #[test]
    fn prop_alternative_indexed_as_referenceable(
        doc_id in arb_id(),
        decision_id in arb_component_id(),
        alt_id in arb_component_id(),
    ) {
        prop_assume!(decision_id != alt_id);

        let config = single_project_config();

        let alt = make_alternative(&alt_id, "rejected", 2);
        let decision = make_decision(&decision_id, vec![alt], 1);
        let doc = make_doc_with_path(
            &doc_id,
            &format!("specs/{doc_id}.md"),
            vec![decision],
        );

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        // Decision is retrievable.
        let looked_up = graph
            .component(&doc_id, &decision_id)
            .expect("Decision should be in the component index");
        prop_assert_eq!(&looked_up.name, DECISION);

        // Alternative nested inside Decision is also retrievable.
        let looked_up = graph
            .component(&doc_id, &alt_id)
            .expect("Alternative nested inside Decision should be in the component index");
        prop_assert_eq!(&looked_up.name, ALTERNATIVE);
        prop_assert_eq!(looked_up.attributes.get("id").unwrap(), &alt_id);
    }
}

// ---------------------------------------------------------------------------
// Property: Fragment refs to Decision and Alternative resolve correctly
// ---------------------------------------------------------------------------

proptest! {
    /// A References component pointing to `doc#decision-id` should resolve
    /// successfully when the target document has a Decision with that id.
    #[test]
    fn prop_ref_to_decision_resolves(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        decision_id in arb_component_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        let decision = make_decision(&decision_id, vec![], 1);
        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.md"),
            vec![decision],
        );

        let fragment_ref = format!("{target_doc_id}#{decision_id}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.md"),
            vec![make_refs_component(REFERENCES, &fragment_ref, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed: References can target Decision");

        let resolved = graph
            .resolved_refs(&source_doc_id, &[0])
            .expect("resolved_refs should return refs for the References component");

        prop_assert_eq!(resolved.len(), 1);
        prop_assert_eq!(&resolved[0].target_doc_id, &target_doc_id);
        prop_assert_eq!(resolved[0].fragment.as_deref(), Some(decision_id.as_str()));
    }

    /// A References component pointing to `doc#alternative-id` (nested in Decision)
    /// should resolve successfully.
    #[test]
    fn prop_ref_to_alternative_nested_in_decision_resolves(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        decision_id in arb_component_id(),
        alt_id in arb_component_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);
        prop_assume!(decision_id != alt_id);

        let config = single_project_config();

        let alt = make_alternative(&alt_id, "considered", 2);
        let decision = make_decision(&decision_id, vec![alt], 1);
        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.md"),
            vec![decision],
        );

        let fragment_ref = format!("{target_doc_id}#{alt_id}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.md"),
            vec![make_refs_component(REFERENCES, &fragment_ref, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed: References targets Alternative inside Decision");

        let resolved = graph
            .resolved_refs(&source_doc_id, &[0])
            .expect("resolved_refs should return refs");

        prop_assert_eq!(resolved.len(), 1);
        prop_assert_eq!(&resolved[0].target_doc_id, &target_doc_id);
        prop_assert_eq!(resolved[0].fragment.as_deref(), Some(alt_id.as_str()));
    }
}

// ---------------------------------------------------------------------------
// Property: References nested inside Decision produces correct reverse mappings
// ---------------------------------------------------------------------------

proptest! {
    /// A References component nested inside a Decision should produce
    /// the correct reverse mapping entry.
    #[test]
    fn prop_references_nested_in_decision_produces_reverse_mapping(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        decision_id in arb_component_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.md"),
            vec![],
        );

        // References nested inside Decision (child component).
        let refs_component = make_refs_component(REFERENCES, &target_doc_id, 2);
        let decision = make_decision(&decision_id, vec![refs_component], 1);
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.md"),
            vec![decision],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed");

        // The reverse mapping for (target_doc_id, None) should contain source_doc_id.
        let referencing = graph.references(&target_doc_id, None);
        prop_assert!(
            referencing.contains(&source_doc_id),
            "references reverse should contain source doc from nested References: referencing={referencing:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Property: TrackedFiles nested inside Decision are indexed for staleness
// ---------------------------------------------------------------------------

proptest! {
    /// A TrackedFiles component nested inside a Decision should have its
    /// path globs aggregated under the owning document ID.
    #[test]
    fn prop_tracked_files_nested_in_decision_indexed(
        doc_id in arb_id(),
        decision_id in arb_component_id(),
    ) {
        let config = single_project_config();

        // TrackedFiles nested inside Decision.
        let tracked = make_tracked_files_component("src/**/*.rs, tests/**/*.rs", 2);
        let decision = make_decision(&decision_id, vec![tracked], 1);
        let doc = make_doc_with_path(
            &doc_id,
            &format!("specs/{doc_id}.md"),
            vec![decision],
        );

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let globs = graph.tracked_files(&doc_id);
        prop_assert!(
            globs.is_some(),
            "tracked_files should return Some for doc with TrackedFiles inside Decision"
        );
        let globs = globs.unwrap();
        prop_assert_eq!(globs.len(), 2);
        prop_assert!(globs.contains(&"src/**/*.rs".to_owned()));
        prop_assert!(globs.contains(&"tests/**/*.rs".to_owned()));
    }
}

// ---------------------------------------------------------------------------
// Property: DependsOn nested inside Decision creates document dependency edges
// ---------------------------------------------------------------------------

proptest! {
    /// A DependsOn component nested inside a Decision should create a
    /// document dependency edge, making the source document depend on
    /// the target.
    #[test]
    fn prop_depends_on_nested_in_decision_creates_dependency(
        dep_doc_id in arb_id(),
        source_doc_id in arb_id(),
        decision_id in arb_component_id(),
    ) {
        prop_assume!(dep_doc_id != source_doc_id);

        let config = single_project_config();

        let dep_doc = make_doc_with_path(
            &dep_doc_id,
            &format!("specs/{dep_doc_id}.md"),
            vec![],
        );

        // DependsOn nested inside Decision.
        let depends_on = make_refs_component("DependsOn", &dep_doc_id, 2);
        let decision = make_decision(&decision_id, vec![depends_on], 1);
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.md"),
            vec![decision],
        );

        let graph = build_graph(vec![dep_doc, source_doc], &config)
            .expect("build_graph should succeed");

        // depends_on_reverse: target → set of depending docs.
        let depending = graph.depends_on(&dep_doc_id);
        prop_assert!(
            depending.contains(&source_doc_id),
            "depends_on reverse should contain source doc from nested DependsOn: depending={depending:?}"
        );

        // The doc_topo_order should have dep_doc before source_doc.
        let order = graph.doc_order();
        let dep_pos = order.iter().position(|id| id == &dep_doc_id);
        let src_pos = order.iter().position(|id| id == &source_doc_id);
        prop_assert!(dep_pos.is_some() && src_pos.is_some());
        prop_assert!(
            dep_pos.unwrap() < src_pos.unwrap(),
            "dep_doc should come before source_doc in topo order"
        );
    }
}
