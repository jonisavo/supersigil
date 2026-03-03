//! Property tests for referenceable component indexing (pipeline stage 2).

use std::collections::HashMap;

use proptest::prelude::*;

use crate::graph::tests::generators::{arb_component_id, arb_config, arb_id, make_doc};
use crate::graph::{ACCEPTANCE_CRITERIA, CRITERION, GraphError, TASK, build_graph};
use crate::{ExtractedComponent, SourcePosition};

fn make_criterion(id: &str, line: usize, byte_offset: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: CRITERION.to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children: Vec::new(),
        body_text: Some(format!("criterion {id}")),
        position: SourcePosition {
            byte_offset,
            line,
            column: 1,
        },
    }
}

fn make_task(id: &str, line: usize, byte_offset: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: TASK.to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children: Vec::new(),
        body_text: Some(format!("task {id}")),
        position: SourcePosition {
            byte_offset,
            line,
            column: 1,
        },
    }
}

fn make_acceptance_criteria(
    children: Vec<ExtractedComponent>,
    line: usize,
    byte_offset: usize,
) -> ExtractedComponent {
    ExtractedComponent {
        name: ACCEPTANCE_CRITERIA.to_owned(),
        attributes: HashMap::new(),
        children,
        body_text: None,
        position: SourcePosition {
            byte_offset,
            line,
            column: 1,
        },
    }
}

// ---------------------------------------------------------------------------
// Property 3: Referenceable component index round-trip
// ---------------------------------------------------------------------------

proptest! {
    /// For any SpecDocument containing referenceable components (including
    /// nested Criterion inside AcceptanceCriteria), each such component
    /// should be retrievable from the component index by (doc_id, component_id)
    /// and should match the original component.
    ///
    /// Validates: Requirements 2.1, 2.3, 2.4
    #[test]
    fn prop_component_index_round_trip(
        doc_id in arb_id(),
        crit_id_a in arb_component_id(),
        crit_id_b in arb_component_id(),
        task_id in arb_component_id(),
        config in arb_config(),
    ) {
        // Ensure all component IDs are distinct.
        prop_assume!(crit_id_a != crit_id_b);
        prop_assume!(crit_id_a != task_id);
        prop_assume!(crit_id_b != task_id);

        let top_criterion = make_criterion(&crit_id_a, 1, 0);
        let nested_criterion = make_criterion(&crit_id_b, 5, 100);
        let ac = make_acceptance_criteria(vec![nested_criterion], 4, 80);
        let task = make_task(&task_id, 10, 200);

        let doc = make_doc(&doc_id, vec![top_criterion, ac, task]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed with valid components");

        // Top-level Criterion should be retrievable.
        let looked_up = graph
            .component(&doc_id, &crit_id_a)
            .expect("top-level criterion should be in the component index");
        prop_assert_eq!(&looked_up.name, CRITERION);
        prop_assert_eq!(looked_up.attributes.get("id").unwrap(), &crit_id_a);

        // Nested Criterion (inside AcceptanceCriteria) should also be retrievable.
        let looked_up = graph
            .component(&doc_id, &crit_id_b)
            .expect("nested criterion should be in the component index");
        prop_assert_eq!(&looked_up.name, CRITERION);
        prop_assert_eq!(looked_up.attributes.get("id").unwrap(), &crit_id_b);

        // Task should be retrievable.
        let looked_up = graph
            .component(&doc_id, &task_id)
            .expect("task should be in the component index");
        prop_assert_eq!(&looked_up.name, TASK);
        prop_assert_eq!(looked_up.attributes.get("id").unwrap(), &task_id);
    }
}

// ---------------------------------------------------------------------------
// Property 4: Duplicate component ID detection
// ---------------------------------------------------------------------------

proptest! {
    /// When two referenceable components within the same document share the
    /// same `id` attribute, `build_graph` returns a `DuplicateComponentId`
    /// error identifying the conflicting ID and source positions.
    ///
    /// Validates: Requirements 2.2
    #[test]
    fn prop_duplicate_component_id_detection(
        doc_id in arb_id(),
        shared_id in arb_component_id(),
        config in arb_config(),
    ) {
        let comp_a = make_criterion(&shared_id, 1, 0);
        let comp_b = make_task(&shared_id, 10, 200);

        let doc = make_doc(&doc_id, vec![comp_a.clone(), comp_b.clone()]);

        let result = build_graph(vec![doc], &config);
        let errors = result.expect_err("build_graph should fail on duplicate component IDs");

        let dup_errors: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::DuplicateComponentId {
                    doc_id: did,
                    component_id,
                    positions,
                } if did == &doc_id && component_id == &shared_id => Some(positions),
                _ => None,
            })
            .collect();

        prop_assert!(
            dup_errors.len() == 1,
            "expected exactly one DuplicateComponentId error"
        );

        let positions = dup_errors[0];
        prop_assert!(positions.len() == 2, "should report both positions");
        prop_assert!(
            positions.contains(&comp_a.position),
            "should contain first component's position"
        );
        prop_assert!(
            positions.contains(&comp_b.position),
            "should contain second component's position"
        );
    }
}
