//! Property tests for the plan query (task 17).
//!
//! Properties 18, 19, 25, 26, and 28b from the design document.

use proptest::prelude::*;

use crate::graph::query::{PlanQuery, QueryError};
use crate::graph::tests::generators::{
    make_acceptance_criteria, make_criterion, make_doc, make_doc_full, make_refs_component,
    make_task, single_project_config,
};
use crate::graph::{ILLUSTRATES, VALIDATES, build_graph};

// ---------------------------------------------------------------------------
// Property 18: Plan output correctness
// ---------------------------------------------------------------------------

proptest! {
    /// The plan output for a document contains outstanding criteria (no
    /// validating doc), pending tasks (status ≠ done) in topo order,
    /// completed tasks with implements refs, and illustrating documents.
    ///
    /// Validates: Requirements 10.1, 10.2, 10.3, 10.4
    #[test]
    fn prop_plan_output_correctness(
        req_id in "req/[a-z]{2,6}",
        val_id in "val/[a-z]{2,6}",
        illus_id in "illus/[a-z]{2,6}",
        tasks_id in "tasks/[a-z]{2,6}",
        crit_covered in "[a-z]{2,6}",
        crit_outstanding in "[a-z]{2,6}",
        task_done_id in "td[a-z]{2,4}",
        task_pending_a in "tpa[a-z]{2,4}",
        task_pending_b in "tpb[a-z]{2,4}",
    ) {
        // Ensure all doc IDs are distinct.
        let doc_ids = [&req_id, &val_id, &illus_id, &tasks_id];
        for i in 0..doc_ids.len() {
            for j in (i + 1)..doc_ids.len() {
                prop_assume!(doc_ids[i] != doc_ids[j]);
            }
        }
        // Ensure criterion IDs are distinct.
        prop_assume!(crit_covered != crit_outstanding);
        // Ensure task IDs are distinct.
        let task_ids = [&task_done_id, &task_pending_a, &task_pending_b];
        for i in 0..task_ids.len() {
            for j in (i + 1)..task_ids.len() {
                prop_assume!(task_ids[i] != task_ids[j]);
            }
        }

        let config = single_project_config();

        // Requirement doc with two criteria.
        let req_doc = make_doc(
            &req_id,
            vec![make_acceptance_criteria(
                vec![
                    make_criterion(&crit_covered, 2),
                    make_criterion(&crit_outstanding, 3),
                ],
                1,
            )],
        );

        // Validating doc covers crit_covered only → crit_outstanding is outstanding.
        let val_doc = make_doc_full(
            &val_id,
            None,
            Some("active"),
            vec![make_refs_component(
                VALIDATES,
                &format!("{req_id}#{crit_covered}"),
                1,
            )],
        );

        // Illustrating doc targets the requirement doc.
        let illus_doc = make_doc(
            &illus_id,
            vec![make_refs_component(ILLUSTRATES, &req_id, 1)],
        );

        // Tasks doc: one done task implementing crit_covered, two pending
        // tasks implementing crit_outstanding (b depends on a).
        let ref_covered = format!("{req_id}#{crit_covered}");
        let ref_outstanding = format!("{req_id}#{crit_outstanding}");
        let tasks_doc = make_doc(
            &tasks_id,
            vec![
                make_task(&task_done_id, Some("done"), Some(&ref_covered), None, 1),
                make_task(&task_pending_a, Some("todo"), Some(&ref_outstanding), None, 2),
                make_task(
                    &task_pending_b,
                    Some("todo"),
                    Some(&ref_outstanding),
                    Some(&task_pending_a),
                    3,
                ),
            ],
        );

        let graph = build_graph(
            vec![req_doc, val_doc, illus_doc, tasks_doc],
            &config,
        )
        .expect("build_graph should succeed");

        let plan = graph
            .plan(&PlanQuery::Document(req_id.clone()))
            .expect("plan should succeed");

        // 10.1: Outstanding criteria — crit_outstanding has no validator.
        prop_assert!(
            plan.outstanding_criteria
                .iter()
                .any(|c| c.criterion_id == crit_outstanding && c.doc_id == req_id),
            "crit_outstanding should be outstanding: {:?}",
            plan.outstanding_criteria
        );
        prop_assert!(
            !plan.outstanding_criteria
                .iter()
                .any(|c| c.criterion_id == crit_covered),
            "crit_covered should NOT be outstanding"
        );

        // 10.2: Pending tasks (status ≠ done) in topo order.
        let pending_ids: Vec<&str> = plan
            .pending_tasks
            .iter()
            .map(|t| t.task_id.as_str())
            .collect();
        prop_assert!(
            pending_ids.contains(&task_pending_a.as_str()),
            "task_pending_a should be pending"
        );
        prop_assert!(
            pending_ids.contains(&task_pending_b.as_str()),
            "task_pending_b should be pending"
        );
        // Topo order: a before b (b depends on a).
        let pos_a = pending_ids.iter().position(|&id| id == task_pending_a);
        let pos_b = pending_ids.iter().position(|&id| id == task_pending_b);
        prop_assert!(
            pos_a.unwrap() < pos_b.unwrap(),
            "task_pending_a should appear before task_pending_b: {pending_ids:?}"
        );

        // 10.3: Completed tasks with implements refs.
        prop_assert!(
            plan.completed_tasks
                .iter()
                .any(|t| t.task_id == task_done_id
                    && t.implements.contains(&(req_id.clone(), crit_covered.clone()))),
            "completed tasks should include task_done with implements ref"
        );

        // 10.4: Illustrating documents.
        prop_assert!(
            plan.illustrated_by
                .iter()
                .any(|i| i.doc_id == illus_id && i.target_doc_id == req_id),
            "plan should include illustrating doc: {:?}",
            plan.illustrated_by
        );
    }
}

// ---------------------------------------------------------------------------
// Property 19: Plan prefix aggregation
// ---------------------------------------------------------------------------

proptest! {
    /// When querying by prefix, the plan aggregates outstanding criteria
    /// grouped by source doc and tasks per tasks doc in topo order.
    ///
    /// Validates: Requirements 10.5
    #[test]
    fn prop_plan_prefix_aggregation(
        suffix_a in "[a-z]{2,6}",
        suffix_b in "[a-z]{2,6}",
        suffix_tasks in "[a-z]{2,6}",
        crit_a in "[a-z]{2,6}",
        crit_b in "[a-z]{2,6}",
        task_a in "ta[a-z]{2,4}",
        task_b in "tb[a-z]{2,4}",
    ) {
        // Ensure suffixes are distinct so doc IDs don't collide.
        prop_assume!(suffix_a != suffix_b);
        prop_assume!(suffix_a != suffix_tasks);
        prop_assume!(suffix_b != suffix_tasks);
        prop_assume!(crit_a != crit_b);
        prop_assume!(task_a != task_b);

        let prefix = "auth/";
        let req_id_a = format!("{prefix}{suffix_a}");
        let req_id_b = format!("{prefix}{suffix_b}");
        let tasks_id = format!("{prefix}{suffix_tasks}");

        let config = single_project_config();

        // Two requirement docs under auth/ prefix, each with one criterion.
        let req_doc_a = make_doc(
            &req_id_a,
            vec![make_acceptance_criteria(
                vec![make_criterion(&crit_a, 2)],
                1,
            )],
        );
        let req_doc_b = make_doc(
            &req_id_b,
            vec![make_acceptance_criteria(
                vec![make_criterion(&crit_b, 2)],
                1,
            )],
        );

        // Tasks doc with tasks implementing criteria from both req docs.
        let ref_a = format!("{req_id_a}#{crit_a}");
        let ref_b = format!("{req_id_b}#{crit_b}");
        let tasks_doc = make_doc(
            &tasks_id,
            vec![
                make_task(&task_a, Some("todo"), Some(&ref_a), None, 1),
                make_task(&task_b, Some("todo"), Some(&ref_b), Some(&task_a), 2),
            ],
        );

        let graph = build_graph(
            vec![req_doc_a, req_doc_b, tasks_doc],
            &config,
        )
        .expect("build_graph should succeed");

        let plan = graph
            .plan(&PlanQuery::Prefix(prefix.to_owned()))
            .expect("plan should succeed");

        // Outstanding criteria from both docs (no validators).
        prop_assert!(
            plan.outstanding_criteria
                .iter()
                .any(|c| c.doc_id == req_id_a && c.criterion_id == crit_a),
            "crit_a from req_doc_a should be outstanding"
        );
        prop_assert!(
            plan.outstanding_criteria
                .iter()
                .any(|c| c.doc_id == req_id_b && c.criterion_id == crit_b),
            "crit_b from req_doc_b should be outstanding"
        );

        // Pending tasks in topo order (task_b depends on task_a).
        let pending_ids: Vec<&str> = plan
            .pending_tasks
            .iter()
            .map(|t| t.task_id.as_str())
            .collect();
        prop_assert!(
            pending_ids.contains(&task_a.as_str()),
            "task_a should be pending"
        );
        prop_assert!(
            pending_ids.contains(&task_b.as_str()),
            "task_b should be pending"
        );
        let pos_a = pending_ids.iter().position(|&id| id == task_a);
        let pos_b = pending_ids.iter().position(|&id| id == task_b);
        prop_assert!(
            pos_a.unwrap() < pos_b.unwrap(),
            "task_a before task_b in topo order: {pending_ids:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Property 26: Project-wide plan
// ---------------------------------------------------------------------------

proptest! {
    /// `plan(PlanQuery::All)` covers all documents: outstanding criteria
    /// from all requirement docs, pending and completed tasks from all
    /// tasks docs.
    ///
    /// Validates: Requirements 10.6
    #[test]
    fn prop_plan_all_covers_everything(
        req_id_a in "reqa/[a-z]{2,6}",
        req_id_b in "reqb/[a-z]{2,6}",
        tasks_id in "tasks/[a-z]{2,6}",
        crit_a in "[a-z]{2,6}",
        crit_b in "[a-z]{2,6}",
        task_done in "td[a-z]{2,4}",
        task_pending in "tp[a-z]{2,4}",
    ) {
        let doc_ids = [&req_id_a, &req_id_b, &tasks_id];
        for i in 0..doc_ids.len() {
            for j in (i + 1)..doc_ids.len() {
                prop_assume!(doc_ids[i] != doc_ids[j]);
            }
        }
        prop_assume!(crit_a != crit_b);
        prop_assume!(task_done != task_pending);

        let config = single_project_config();

        let req_doc_a = make_doc(
            &req_id_a,
            vec![make_acceptance_criteria(
                vec![make_criterion(&crit_a, 2)],
                1,
            )],
        );
        let req_doc_b = make_doc(
            &req_id_b,
            vec![make_acceptance_criteria(
                vec![make_criterion(&crit_b, 2)],
                1,
            )],
        );

        let ref_a = format!("{req_id_a}#{crit_a}");
        let ref_b = format!("{req_id_b}#{crit_b}");
        let tasks_doc = make_doc(
            &tasks_id,
            vec![
                make_task(&task_done, Some("done"), Some(&ref_a), None, 1),
                make_task(&task_pending, Some("todo"), Some(&ref_b), None, 2),
            ],
        );

        let graph = build_graph(
            vec![req_doc_a, req_doc_b, tasks_doc],
            &config,
        )
        .expect("build_graph should succeed");

        let plan = graph
            .plan(&PlanQuery::All)
            .expect("plan(All) should succeed");

        // crit_a: done task implements it → NOT outstanding.
        prop_assert!(
            !plan.outstanding_criteria
                .iter()
                .any(|c| c.doc_id == req_id_a && c.criterion_id == crit_a),
            "crit_a should NOT be outstanding (done task implements it)"
        );
        // crit_b: pending task implements it → still outstanding.
        prop_assert!(
            plan.outstanding_criteria
                .iter()
                .any(|c| c.doc_id == req_id_b && c.criterion_id == crit_b),
            "crit_b should be outstanding (task not done)"
        );

        // Completed task from tasks doc.
        prop_assert!(
            plan.completed_tasks.iter().any(|t| t.task_id == task_done),
            "task_done should be in completed_tasks"
        );

        // Pending task from tasks doc.
        prop_assert!(
            plan.pending_tasks.iter().any(|t| t.task_id == task_pending),
            "task_pending should be in pending_tasks"
        );
    }
}

// ---------------------------------------------------------------------------
// Property 25: Plan query error for nonexistent target
// ---------------------------------------------------------------------------

proptest! {
    /// `PlanQuery::parse` with a string that matches no exact ID and no
    /// prefix returns `QueryError::NoMatchingDocuments`.
    ///
    /// Validates: Requirements 10.7
    #[test]
    fn prop_plan_query_nonexistent_returns_error(
        existing_id in "existing/[a-z]{2,6}",
        missing_query in "zzz-missing/[a-z]{2,6}",
    ) {
        prop_assume!(!existing_id.starts_with(&missing_query));
        prop_assume!(!missing_query.starts_with(&existing_id));
        prop_assume!(existing_id != missing_query);

        let config = single_project_config();
        let doc = make_doc(&existing_id, vec![]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let result = PlanQuery::parse(Some(&missing_query), &graph);
        prop_assert!(result.is_err(), "parse should fail for nonexistent query");

        match result.unwrap_err() {
            QueryError::NoMatchingDocuments { query } => {
                prop_assert_eq!(query, missing_query);
            }
            other @ QueryError::DocumentNotFound { .. } => {
                prop_assert!(false, "expected NoMatchingDocuments, got: {other:?}");
            }
        }
    }

    /// `PlanQuery::parse` with `None` or empty string returns `PlanQuery::All`.
    #[test]
    fn prop_plan_query_empty_returns_all(
        existing_id in "existing/[a-z]{2,6}",
    ) {
        let config = single_project_config();
        let doc = make_doc(&existing_id, vec![]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let result_none = PlanQuery::parse(None, &graph);
        prop_assert_eq!(result_none.unwrap(), PlanQuery::All);

        let result_empty = PlanQuery::parse(Some(""), &graph);
        prop_assert_eq!(result_empty.unwrap(), PlanQuery::All);
    }

    /// `PlanQuery::parse` with an exact document ID returns `PlanQuery::Document`.
    #[test]
    fn prop_plan_query_exact_match(
        existing_id in "existing/[a-z]{2,6}",
    ) {
        let config = single_project_config();
        let doc = make_doc(&existing_id, vec![]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let result = PlanQuery::parse(Some(&existing_id), &graph);
        prop_assert_eq!(result.unwrap(), PlanQuery::Document(existing_id));
    }

    /// `PlanQuery::parse` with a prefix that matches documents returns `PlanQuery::Prefix`.
    #[test]
    fn prop_plan_query_prefix_match(
        suffix in "[a-z]{2,6}",
    ) {
        let prefix = "auth/";
        let doc_id = format!("{prefix}{suffix}");

        let config = single_project_config();
        let doc = make_doc(&doc_id, vec![]);

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let result = PlanQuery::parse(Some(prefix), &graph);
        prop_assert_eq!(result.unwrap(), PlanQuery::Prefix(prefix.to_owned()));
    }
}

// ---------------------------------------------------------------------------
// Property 28b: Task-to-criterion mappings in PlanOutput
// ---------------------------------------------------------------------------

proptest! {
    /// Tasks in the plan output include their resolved `implements` refs
    /// so consumers can see which criteria each task addresses.
    ///
    /// Validates: Requirements 11.3 (plan half)
    #[test]
    fn prop_plan_tasks_include_implements_refs(
        req_id in "req/[a-z]{2,6}",
        tasks_id in "tasks/[a-z]{2,6}",
        crit_a in "[a-z]{2,6}",
        crit_b in "[a-z]{2,6}",
        task_pending_id in "tp[a-z]{2,4}",
        task_done_id in "td[a-z]{2,4}",
    ) {
        prop_assume!(req_id != tasks_id);
        prop_assume!(crit_a != crit_b);
        prop_assume!(task_pending_id != task_done_id);

        let config = single_project_config();

        let req_doc = make_doc(
            &req_id,
            vec![make_acceptance_criteria(
                vec![make_criterion(&crit_a, 2), make_criterion(&crit_b, 3)],
                1,
            )],
        );

        // Pending task implements crit_a, done task implements both.
        let ref_a = format!("{req_id}#{crit_a}");
        let ref_both = format!("{req_id}#{crit_a}, {req_id}#{crit_b}");
        let tasks_doc = make_doc(
            &tasks_id,
            vec![
                make_task(&task_pending_id, Some("todo"), Some(&ref_a), None, 1),
                make_task(&task_done_id, Some("done"), Some(&ref_both), None, 2),
            ],
        );

        let graph = build_graph(vec![req_doc, tasks_doc], &config)
            .expect("build_graph should succeed");

        let plan = graph
            .plan(&PlanQuery::Document(req_id.clone()))
            .expect("plan should succeed");

        // Pending task should have implements = [(req_id, crit_a)].
        let pending = plan
            .pending_tasks
            .iter()
            .find(|t| t.task_id == task_pending_id);
        prop_assert!(pending.is_some(), "task_pending should be in pending_tasks");
        let pending = pending.unwrap();
        prop_assert!(
            pending.implements.contains(&(req_id.clone(), crit_a.clone())),
            "pending task implements should contain (req_id, crit_a): {:?}",
            pending.implements
        );

        // Done task should have implements = [(req_id, crit_a), (req_id, crit_b)].
        let done = plan
            .completed_tasks
            .iter()
            .find(|t| t.task_id == task_done_id);
        prop_assert!(done.is_some(), "task_done should be in completed_tasks");
        let done = done.unwrap();
        prop_assert!(
            done.implements.contains(&(req_id.clone(), crit_a.clone())),
            "done task implements should contain (req_id, crit_a): {:?}",
            done.implements
        );
        prop_assert!(
            done.implements.contains(&(req_id.clone(), crit_b.clone())),
            "done task implements should contain (req_id, crit_b): {:?}",
            done.implements
        );
    }
}
