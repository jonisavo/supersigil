//! Property tests for the context query (task 16).
//!
//! Properties 17, 24, and 28a from the design document.

use proptest::prelude::*;

use crate::graph::query::QueryError;
use crate::graph::tests::generators::{
    arb_component_id, arb_id, make_acceptance_criteria, make_criterion, make_doc, make_doc_full,
    make_refs_component, make_task, single_project_config,
};
use crate::graph::{ILLUSTRATES, IMPLEMENTS, VALIDATES, build_graph};

// ---------------------------------------------------------------------------
// Property 17: Context output completeness
// ---------------------------------------------------------------------------

proptest! {
    /// The context output for a document contains the document itself,
    /// its criteria with validation/illustration status, implementing
    /// documents, and linked tasks in topological order.
    ///
    /// Validates: Requirements 9.1, 9.2, 9.3, 9.4, 9.5
    #[test]
    fn prop_context_contains_document_and_criteria(
        req_id in arb_id(),
        val_id in arb_id(),
        impl_id in arb_id(),
        illus_id in arb_id(),
        tasks_id in arb_id(),
        crit_a in arb_component_id(),
        crit_b in arb_component_id(),
        task_id in arb_component_id(),
    ) {
        // Ensure all IDs are distinct.
        let ids = [&req_id, &val_id, &impl_id, &illus_id, &tasks_id];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                prop_assume!(ids[i] != ids[j]);
            }
        }
        prop_assume!(crit_a != crit_b);
        prop_assume!(task_id != crit_a && task_id != crit_b);

        let config = single_project_config();

        // Requirement doc with two criteria.
        let req_doc = make_doc(
            &req_id,
            vec![make_acceptance_criteria(
                vec![make_criterion(&crit_a, 2), make_criterion(&crit_b, 3)],
                1,
            )],
        );

        // Validating doc targets crit_a.
        let val_doc = make_doc_full(
            &val_id,
            None,
            Some("active"),
            vec![make_refs_component(VALIDATES, &format!("{req_id}#{crit_a}"), 1)],
        );

        // Implementing doc.
        let impl_doc = make_doc(
            &impl_id,
            vec![make_refs_component(IMPLEMENTS, &req_id, 1)],
        );

        // Illustrating doc targets crit_b.
        let illus_doc = make_doc(
            &illus_id,
            vec![make_refs_component(ILLUSTRATES, &format!("{req_id}#{crit_b}"), 1)],
        );

        // Tasks doc with a task implementing crit_a.
        let impl_ref = format!("{req_id}#{crit_a}");
        let tasks_doc = make_doc(
            &tasks_id,
            vec![make_task(&task_id, Some("todo"), Some(&impl_ref), None, 1)],
        );

        let graph = build_graph(
            vec![req_doc.clone(), val_doc, impl_doc, illus_doc, tasks_doc],
            &config,
        )
        .expect("build_graph should succeed");

        let ctx = graph.context(&req_id).expect("context should succeed");

        // 9.1: Contains the target document.
        prop_assert_eq!(&ctx.document, &req_doc);

        // 9.2: Criteria include validation status.
        let crit_a_ctx = ctx.criteria.iter().find(|c| c.id == crit_a);
        prop_assert!(crit_a_ctx.is_some(), "crit_a should be in context");
        let crit_a_ctx = crit_a_ctx.unwrap();
        prop_assert!(
            crit_a_ctx.validated_by.iter().any(|d| d.doc_id == val_id && d.status.as_deref() == Some("active")),
            "crit_a should be validated by val_doc with status 'active'"
        );

        // 9.3: Implementing documents.
        prop_assert!(
            ctx.implemented_by.iter().any(|d| d.doc_id == impl_id),
            "context should include implementing doc"
        );

        // 9.4: Illustrating documents per criterion.
        let crit_b_ctx = ctx.criteria.iter().find(|c| c.id == crit_b);
        prop_assert!(crit_b_ctx.is_some(), "crit_b should be in context");
        let crit_b_ctx = crit_b_ctx.unwrap();
        prop_assert!(
            crit_b_ctx.illustrated_by.contains(&illus_id),
            "crit_b should be illustrated by illus_doc"
        );

        // 9.5: Tasks from linked tasks documents.
        prop_assert!(
            ctx.tasks.iter().any(|t| t.task_id == task_id && t.tasks_doc_id == tasks_id),
            "context should include linked task"
        );
    }

    /// Document-level illustrations appear in the context output's
    /// `illustrated_by` field.
    ///
    /// Validates: Requirements 9.4
    #[test]
    fn prop_context_doc_level_illustration(
        req_id in arb_id(),
        illus_id in arb_id(),
    ) {
        prop_assume!(req_id != illus_id);

        let config = single_project_config();

        let req_doc = make_doc(&req_id, vec![]);
        let illus_doc = make_doc(
            &illus_id,
            vec![make_refs_component(ILLUSTRATES, &req_id, 1)],
        );

        let graph = build_graph(vec![req_doc, illus_doc], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&req_id).expect("context should succeed");

        prop_assert!(
            ctx.illustrated_by.contains(&illus_id),
            "doc-level illustration should appear in illustrated_by"
        );
    }

    /// Tasks in context output respect topological order: if task A depends
    /// on task B, B appears before A.
    ///
    /// Validates: Requirements 9.5
    #[test]
    fn prop_context_tasks_in_topo_order(
        req_id in arb_id(),
        tasks_id in arb_id(),
        crit_id in arb_component_id(),
    ) {
        prop_assume!(req_id != tasks_id);

        let config = single_project_config();

        let req_doc = make_doc(
            &req_id,
            vec![make_acceptance_criteria(vec![make_criterion(&crit_id, 2)], 1)],
        );

        let impl_ref = format!("{req_id}#{crit_id}");
        // task-b depends on task-a; both implement the criterion.
        let tasks_doc = make_doc(
            &tasks_id,
            vec![
                make_task("task-a", Some("todo"), Some(&impl_ref), None, 1),
                make_task("task-b", Some("todo"), Some(&impl_ref), Some("task-a"), 2),
            ],
        );

        let graph = build_graph(vec![req_doc, tasks_doc], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&req_id).expect("context should succeed");

        let positions: Vec<_> = ctx.tasks.iter().map(|t| t.task_id.as_str()).collect();
        let pos_a = positions.iter().position(|&id| id == "task-a");
        let pos_b = positions.iter().position(|&id| id == "task-b");

        prop_assert!(pos_a.is_some() && pos_b.is_some(), "both tasks should be present");
        prop_assert!(
            pos_a.unwrap() < pos_b.unwrap(),
            "task-a should appear before task-b (dependency order): {positions:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Property 24: Context query error for nonexistent document
// ---------------------------------------------------------------------------

proptest! {
    /// Calling `context(id)` with a nonexistent ID returns
    /// `QueryError::DocumentNotFound`.
    ///
    /// Validates: Requirements 9.6
    #[test]
    fn prop_context_nonexistent_doc_returns_error(
        existing_id in arb_id(),
        missing_id in arb_id(),
    ) {
        prop_assume!(existing_id != missing_id);

        let config = single_project_config();
        let doc = make_doc(&existing_id, vec![]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let result = graph.context(&missing_id);
        prop_assert!(result.is_err(), "context for nonexistent doc should fail");

        match result.unwrap_err() {
            QueryError::DocumentNotFound { id } => {
                prop_assert_eq!(id, missing_id);
            }
            other @ QueryError::NoMatchingDocuments { .. } => {
                prop_assert!(false, "expected DocumentNotFound, got: {other:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property 28a: Task-to-criterion mappings in ContextOutput
// ---------------------------------------------------------------------------

proptest! {
    /// Tasks in the context output include their resolved `implements` refs
    /// so consumers can see which criteria each task addresses.
    ///
    /// Validates: Requirements 11.3 (context half)
    #[test]
    fn prop_context_tasks_include_implements_refs(
        req_id in arb_id(),
        tasks_id in arb_id(),
        crit_a in arb_component_id(),
        crit_b in arb_component_id(),
        task_a_id in arb_component_id(),
        task_b_id in arb_component_id(),
    ) {
        prop_assume!(req_id != tasks_id);
        prop_assume!(crit_a != crit_b);
        // Ensure task IDs don't collide with criterion IDs.
        let all_comp_ids = [&crit_a, &crit_b, &task_a_id, &task_b_id];
        for i in 0..all_comp_ids.len() {
            for j in (i + 1)..all_comp_ids.len() {
                prop_assume!(all_comp_ids[i] != all_comp_ids[j]);
            }
        }

        let config = single_project_config();

        let req_doc = make_doc(
            &req_id,
            vec![make_acceptance_criteria(
                vec![make_criterion(&crit_a, 2), make_criterion(&crit_b, 3)],
                1,
            )],
        );

        // task_a implements crit_a, task_b implements both crit_a and crit_b.
        let ref_a = format!("{req_id}#{crit_a}");
        let ref_both = format!("{req_id}#{crit_a}, {req_id}#{crit_b}");
        let tasks_doc = make_doc(
            &tasks_id,
            vec![
                make_task(&task_a_id, None, Some(&ref_a), None, 1),
                make_task(&task_b_id, None, Some(&ref_both), None, 2),
            ],
        );

        let graph = build_graph(vec![req_doc, tasks_doc], &config)
            .expect("build_graph should succeed");

        let ctx = graph.context(&req_id).expect("context should succeed");

        // task_a should have implements = [(req_id, crit_a)]
        let task_a_ctx = ctx.tasks.iter().find(|t| t.task_id == task_a_id);
        prop_assert!(task_a_ctx.is_some(), "task_a should be in context");
        let task_a_ctx = task_a_ctx.unwrap();
        prop_assert!(
            task_a_ctx.implements.contains(&(req_id.clone(), crit_a.clone())),
            "task_a implements should contain (req_id, crit_a): {:?}",
            task_a_ctx.implements
        );

        // task_b should have implements = [(req_id, crit_a), (req_id, crit_b)]
        let task_b_ctx = ctx.tasks.iter().find(|t| t.task_id == task_b_id);
        prop_assert!(task_b_ctx.is_some(), "task_b should be in context");
        let task_b_ctx = task_b_ctx.unwrap();
        prop_assert!(
            task_b_ctx.implements.contains(&(req_id.clone(), crit_a.clone())),
            "task_b implements should contain (req_id, crit_a): {:?}",
            task_b_ctx.implements
        );
        prop_assert!(
            task_b_ctx.implements.contains(&(req_id.clone(), crit_b.clone())),
            "task_b implements should contain (req_id, crit_b): {:?}",
            task_b_ctx.implements
        );
    }
}
