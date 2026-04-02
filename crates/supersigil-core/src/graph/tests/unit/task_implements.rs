use super::*;

// ===========================================================================
// Task 5: Generalize Task.implements to verifiable targets
// ===========================================================================

/// Task `implements` targeting a Criterion (which is verifiable) should resolve
/// successfully. This is the existing behavior, preserved after generalization.
#[test]
fn task_implements_accepts_refs_to_verifiable_components() {
    let config = single_project_config();

    let req_doc = make_doc(
        "my/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("my-crit", 2)],
            1,
        )],
    );

    let tasks_doc = make_doc(
        "my/tasks",
        vec![make_task(
            "task-1",
            Some("todo"),
            Some("my/req#my-crit"),
            None,
            1,
        )],
    );

    let graph =
        build_graph(vec![req_doc, tasks_doc], &config).expect("graph should build successfully");

    let implements = graph
        .task_implements("my/tasks", "task-1")
        .expect("task-1 should have implements entries");

    assert_eq!(implements.len(), 1);
    assert_eq!(implements[0], ("my/req".to_owned(), "my-crit".to_owned()));
}

/// Task `implements` targeting a Task (which is referenceable but NOT
/// verifiable) should produce a `BrokenRef` error. The validation now checks
/// the `verifiable` flag on the component definition rather than hardcoding
/// the `Criterion` name.
#[test]
fn task_implements_rejects_refs_to_non_verifiable_components() {
    let config = single_project_config();

    // Document with a Task component (referenceable but not verifiable).
    let target_doc = make_doc(
        "my/tasks-target",
        vec![make_task("target-task", Some("done"), None, None, 1)],
    );

    // Another document with a Task that tries to implement the target Task.
    let source_doc = make_doc(
        "my/tasks-source",
        vec![make_task(
            "source-task",
            Some("todo"),
            Some("my/tasks-target#target-task"),
            None,
            1,
        )],
    );

    let result = build_graph(vec![target_doc, source_doc], &config);
    let errors =
        result.expect_err("build_graph should fail: implements ref to non-verifiable Task");

    let broken: Vec<_> = errors
        .iter()
        .filter_map(|e| match e {
            GraphError::BrokenRef {
                doc_id,
                ref_str,
                reason,
                ..
            } if doc_id == "my/tasks-source" && ref_str == "my/tasks-target#target-task" => {
                Some(reason.clone())
            }
            _ => None,
        })
        .collect();

    assert!(
        !broken.is_empty(),
        "expected BrokenRef for implements ref to non-verifiable component: {errors:?}"
    );
    assert!(
        broken[0].contains("verifiable"),
        "error reason should mention 'verifiable', got: {}",
        broken[0]
    );
}

/// The plan/query system still correctly links tasks to criteria they
/// implement after the generalization from `Criterion`-only to verifiable
/// target validation.
#[test]
fn plan_task_linkage_still_works_for_criterion() {
    let config = single_project_config();

    let req_doc = make_doc(
        "my/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-x", 2), make_criterion("crit-y", 3)],
            1,
        )],
    );

    let tasks_doc = make_doc(
        "my/tasks",
        vec![
            make_task("task-a", Some("done"), Some("my/req#crit-x"), None, 1),
            make_task(
                "task-b",
                Some("todo"),
                Some("my/req#crit-y"),
                Some("task-a"),
                2,
            ),
        ],
    );

    let graph = build_graph(vec![req_doc, tasks_doc], &config).expect("graph should build");

    // Plan query: crit-x should NOT be outstanding (done task implements it).
    let plan = graph
        .plan(&PlanQuery::Document("my/req".to_owned()))
        .expect("plan should succeed");

    let outstanding_ids: Vec<&str> = plan
        .outstanding_targets
        .iter()
        .map(|c| c.target_id.as_str())
        .collect();

    assert!(
        !outstanding_ids.contains(&"crit-x"),
        "crit-x should NOT be outstanding (done task): {outstanding_ids:?}"
    );
    assert!(
        outstanding_ids.contains(&"crit-y"),
        "crit-y should be outstanding (task not done): {outstanding_ids:?}"
    );

    // Completed tasks should include task-a with its implements ref.
    let completed = plan.completed_tasks.iter().find(|t| t.task_id == "task-a");
    assert!(completed.is_some(), "task-a should be in completed_tasks");
    assert!(
        completed
            .unwrap()
            .implements
            .contains(&("my/req".to_owned(), "crit-x".to_owned())),
        "task-a should implement crit-x"
    );

    // Pending tasks should include task-b.
    let pending_ids: Vec<&str> = plan
        .pending_tasks
        .iter()
        .map(|t| t.task_id.as_str())
        .collect();
    assert!(
        pending_ids.contains(&"task-b"),
        "task-b should be pending: {pending_ids:?}"
    );
}
