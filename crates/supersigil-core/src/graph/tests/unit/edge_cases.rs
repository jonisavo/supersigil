use super::*;

// ===========================================================================
// 18.2: Edge cases
// ===========================================================================

#[test]
fn empty_document_collection_produces_ok() {
    let config = single_project_config();
    let graph = build_graph(vec![], &config).expect("empty input should produce Ok");

    // Empty indexes.
    assert_eq!(graph.documents().count(), 0);
    assert!(graph.document("anything").is_none());
    assert!(graph.doc_order().is_empty());
    assert!(graph.all_tracked_files().count() == 0);
}

#[test]
fn document_with_no_components_is_indexed() {
    let config = single_project_config();
    let doc = make_doc("bare-doc", vec![]);

    let graph = build_graph(vec![doc.clone()], &config).expect("should succeed");

    // Document is indexed.
    assert_eq!(graph.document("bare-doc"), Some(&doc));

    // No component index entries.
    assert!(graph.component("bare-doc", "anything").is_none());

    // No reverse mappings.
    assert!(graph.references("bare-doc", None).is_empty());
    assert!(graph.implements("bare-doc").is_empty());

    // No tracked files.
    assert!(graph.tracked_files("bare-doc").is_none());
}

#[test]
fn context_for_doc_with_no_criteria_no_tasks_no_reverse_mappings() {
    let config = single_project_config();
    let doc = make_doc("lonely-doc", vec![]);

    let graph = build_graph(vec![doc], &config).expect("should succeed");
    let ctx = graph.context("lonely-doc").expect("context should succeed");

    assert!(ctx.criteria.is_empty());
    assert!(ctx.implemented_by.is_empty());
    assert!(ctx.referenced_by.is_empty());
    assert!(ctx.tasks.is_empty());
}

#[test]
fn plan_for_doc_with_no_criteria_no_tasks() {
    let config = single_project_config();
    let doc = make_doc("empty-req", vec![]);

    let graph = build_graph(vec![doc], &config).expect("should succeed");
    let plan = graph
        .plan(&PlanQuery::Document("empty-req".to_owned()))
        .expect("plan should succeed");

    assert!(plan.outstanding_targets.is_empty());
    assert!(plan.pending_tasks.is_empty());
    assert!(plan.completed_tasks.is_empty());
}

// ===========================================================================
// 18.3: Cross-phase error aggregation
// ===========================================================================

#[test]
fn duplicate_component_ids_and_broken_refs_reported_together() {
    let config = single_project_config();

    // Document with duplicate criterion IDs.
    let dup_doc = make_doc(
        "doc/dup-crit",
        vec![make_acceptance_criteria(
            vec![make_criterion("same-id", 2), make_criterion("same-id", 3)],
            1,
        )],
    );

    // Another document with a broken ref.
    let broken_doc = make_doc(
        "doc/broken-ref",
        vec![make_refs_component(REFERENCES, "ghost/doc#phantom", 1)],
    );

    let result = build_graph(vec![dup_doc, broken_doc], &config);
    let errors = result.expect_err("should fail with both error types");

    let has_dup_component = errors
        .iter()
        .any(|e| matches!(e, GraphError::DuplicateComponentId { .. }));
    let has_broken_ref = errors
        .iter()
        .any(|e| matches!(e, GraphError::BrokenRef { .. }));

    // Per Requirement 13, indexing errors should NOT block later stages.
    // Both the duplicate component error and the broken ref error from the
    // separate document should be reported in a single pass.
    assert!(
        has_dup_component,
        "should contain DuplicateComponentId: {errors:?}"
    );
    assert!(has_broken_ref, "should contain BrokenRef: {errors:?}");
}
