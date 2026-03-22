//! Property tests for error aggregation across pipeline stages.
//!
//! Property 22: Error aggregation
//! Validates: Requirements 13.1, 13.2, 13.3

use proptest::prelude::*;

use crate::graph::tests::generators::{
    arb_config, arb_document_set, arb_id, make_depends_on, make_doc_with_path, make_refs_component,
    make_task, single_project_config,
};
use crate::graph::{GraphError, REFERENCES, build_graph};

// ---------------------------------------------------------------------------
// Property 22: Error aggregation
// ---------------------------------------------------------------------------

proptest! {
    /// Error-free input produces `Ok(DocumentGraph)`.
    ///
    /// Generate a collection of documents with unique IDs and no broken refs,
    /// cycles, or duplicate components. Assert `build_graph` returns `Ok`.
    ///
    /// Validates: Requirements 13.3
    #[test]
    fn prop_error_free_input_produces_ok(
        docs in arb_document_set(4),
        config in arb_config(),
    ) {
        let result = build_graph(docs, &config);
        prop_assert!(
            result.is_ok(),
            "error-free input should produce Ok(DocumentGraph), got: {:?}",
            result.err()
        );
    }

    /// Multiple independent errors within the indexing phase (stages 1–2) are
    /// all collected in a single `Vec<GraphError>`.
    ///
    /// We create two pairs of duplicate document IDs so that two independent
    /// `DuplicateId` errors are produced. Both must appear in the error vector.
    ///
    /// Validates: Requirements 13.1, 13.2
    #[test]
    fn prop_multiple_duplicate_id_errors_aggregated(
        id_a in arb_id(),
        id_b in arb_id(),
    ) {
        prop_assume!(id_a != id_b);

        let config = single_project_config();

        // Two documents sharing id_a, two documents sharing id_b.
        let doc_a1 = make_doc_with_path(&id_a, &format!("specs/a1/{id_a}.md"), vec![]);
        let doc_a2 = make_doc_with_path(&id_a, &format!("specs/a2/{id_a}.md"), vec![]);
        let doc_b1 = make_doc_with_path(&id_b, &format!("specs/b1/{id_b}.md"), vec![]);
        let doc_b2 = make_doc_with_path(&id_b, &format!("specs/b2/{id_b}.md"), vec![]);

        let result = build_graph(vec![doc_a1, doc_a2, doc_b1, doc_b2], &config);
        let errors = result.expect_err("should fail with duplicate IDs");

        let dup_ids: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::DuplicateId { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();

        prop_assert!(
            dup_ids.contains(&id_a.as_str()),
            "errors should contain DuplicateId for '{id_a}', got: {dup_ids:?}"
        );
        prop_assert!(
            dup_ids.contains(&id_b.as_str()),
            "errors should contain DuplicateId for '{id_b}', got: {dup_ids:?}"
        );
    }

    /// Multiple independent errors across resolution and cycle detection
    /// (stages 3–6) are all collected in a single `Vec<GraphError>`.
    ///
    /// We create:
    /// - A document with a broken ref (nonexistent target)
    /// - A tasks document with a dependency cycle
    ///
    /// Both `BrokenRef` and `TaskDependencyCycle` errors must appear.
    ///
    /// Validates: Requirements 13.1, 13.2
    #[test]
    fn prop_broken_ref_and_cycle_errors_aggregated(
        doc_id in arb_id(),
        tasks_id in arb_id(),
    ) {
        prop_assume!(doc_id != tasks_id);

        let config = single_project_config();

        // Document with a References ref to a nonexistent target.
        let broken_ref_doc = make_doc_with_path(
            &doc_id,
            &format!("specs/{doc_id}.md"),
            vec![make_refs_component(REFERENCES, "nonexistent/target", 1)],
        );

        // Tasks document with a self-referencing task (trivial cycle).
        let cyclic_task = make_task("t0", None, None, Some("t0"), 1);
        let tasks_doc = make_doc_with_path(
            &tasks_id,
            &format!("specs/{tasks_id}.md"),
            vec![cyclic_task],
        );

        let result = build_graph(vec![broken_ref_doc, tasks_doc], &config);
        let errors = result.expect_err("should fail with broken ref and cycle");

        let has_broken_ref = errors
            .iter()
            .any(|e| matches!(e, GraphError::BrokenRef { .. }));
        let has_cycle = errors
            .iter()
            .any(|e| matches!(e, GraphError::TaskDependencyCycle { .. }));

        prop_assert!(
            has_broken_ref,
            "errors should contain at least one BrokenRef, got: {errors:?}"
        );
        prop_assert!(
            has_cycle,
            "errors should contain at least one TaskDependencyCycle, got: {errors:?}"
        );
    }

    /// Multiple independent errors across resolution and document cycle
    /// detection are all collected.
    ///
    /// We create:
    /// - A document with a broken ref (nonexistent target)
    /// - Two documents forming a document dependency cycle
    ///
    /// Both `BrokenRef` and `DocumentDependencyCycle` errors must appear.
    ///
    /// Validates: Requirements 13.1, 13.2
    #[test]
    fn prop_broken_ref_and_doc_cycle_errors_aggregated(
        ref_doc_id in arb_id(),
        cycle_a_id in arb_id(),
        cycle_b_id in arb_id(),
    ) {
        prop_assume!(ref_doc_id != cycle_a_id);
        prop_assume!(ref_doc_id != cycle_b_id);
        prop_assume!(cycle_a_id != cycle_b_id);

        let config = single_project_config();

        // Document with a broken ref.
        let broken_ref_doc = make_doc_with_path(
            &ref_doc_id,
            &format!("specs/{ref_doc_id}.md"),
            vec![make_refs_component(REFERENCES, "ghost/doc", 1)],
        );

        // Two documents forming a mutual dependency cycle.
        let cycle_doc_a = make_doc_with_path(
            &cycle_a_id,
            &format!("specs/{cycle_a_id}.md"),
            vec![make_depends_on(&cycle_b_id, 1)],
        );
        let cycle_doc_b = make_doc_with_path(
            &cycle_b_id,
            &format!("specs/{cycle_b_id}.md"),
            vec![make_depends_on(&cycle_a_id, 1)],
        );

        let result = build_graph(
            vec![broken_ref_doc, cycle_doc_a, cycle_doc_b],
            &config,
        );
        let errors = result.expect_err("should fail with broken ref and doc cycle");

        let has_broken_ref = errors
            .iter()
            .any(|e| matches!(e, GraphError::BrokenRef { .. }));
        let has_doc_cycle = errors
            .iter()
            .any(|e| matches!(e, GraphError::DocumentDependencyCycle { .. }));

        prop_assert!(
            has_broken_ref,
            "errors should contain at least one BrokenRef, got: {errors:?}"
        );
        prop_assert!(
            has_doc_cycle,
            "errors should contain at least one DocumentDependencyCycle, got: {errors:?}"
        );
    }

    /// Task cycle errors and broken ref errors from different documents are
    /// aggregated together with document cycle errors — all three error types
    /// in a single pass.
    ///
    /// Validates: Requirements 13.1, 13.2
    #[test]
    fn prop_three_error_types_aggregated(
        ref_doc_id in arb_id(),
        tasks_id in arb_id(),
        cycle_a_id in arb_id(),
        cycle_b_id in arb_id(),
    ) {
        // All IDs must be distinct.
        prop_assume!(ref_doc_id != tasks_id);
        prop_assume!(ref_doc_id != cycle_a_id);
        prop_assume!(ref_doc_id != cycle_b_id);
        prop_assume!(tasks_id != cycle_a_id);
        prop_assume!(tasks_id != cycle_b_id);
        prop_assume!(cycle_a_id != cycle_b_id);

        let config = single_project_config();

        // 1. Document with a broken ref.
        let broken_ref_doc = make_doc_with_path(
            &ref_doc_id,
            &format!("specs/{ref_doc_id}.md"),
            vec![make_refs_component(REFERENCES, "nonexistent/target", 1)],
        );

        // 2. Tasks document with a self-referencing cycle.
        let cyclic_task = make_task("t0", None, None, Some("t0"), 1);
        let tasks_doc = make_doc_with_path(
            &tasks_id,
            &format!("specs/{tasks_id}.md"),
            vec![cyclic_task],
        );

        // 3. Two documents forming a mutual dependency cycle.
        let cycle_doc_a = make_doc_with_path(
            &cycle_a_id,
            &format!("specs/{cycle_a_id}.md"),
            vec![make_depends_on(&cycle_b_id, 1)],
        );
        let cycle_doc_b = make_doc_with_path(
            &cycle_b_id,
            &format!("specs/{cycle_b_id}.md"),
            vec![make_depends_on(&cycle_a_id, 1)],
        );

        let result = build_graph(
            vec![broken_ref_doc, tasks_doc, cycle_doc_a, cycle_doc_b],
            &config,
        );
        let errors = result.expect_err("should fail with multiple error types");

        let has_broken_ref = errors
            .iter()
            .any(|e| matches!(e, GraphError::BrokenRef { .. }));
        let has_task_cycle = errors
            .iter()
            .any(|e| matches!(e, GraphError::TaskDependencyCycle { .. }));
        let has_doc_cycle = errors
            .iter()
            .any(|e| matches!(e, GraphError::DocumentDependencyCycle { .. }));

        prop_assert!(
            has_broken_ref,
            "errors should contain BrokenRef, got: {errors:?}"
        );
        prop_assert!(
            has_task_cycle,
            "errors should contain TaskDependencyCycle, got: {errors:?}"
        );
        prop_assert!(
            has_doc_cycle,
            "errors should contain DocumentDependencyCycle, got: {errors:?}"
        );
    }
}
