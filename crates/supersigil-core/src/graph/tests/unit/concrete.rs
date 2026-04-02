use super::*;

#[test]
fn auth_login_context_contains_document_and_criteria() {
    let (graph, req_doc) = build_auth_login_scenario();
    let ctx = graph
        .context("auth/req/login")
        .expect("context should succeed");

    // 9.1: Contains the target document.
    assert_eq!(ctx.document, req_doc);

    // 9.7: Output is a structured type (compile-time guarantee, but verify fields).
    assert_eq!(ctx.criteria.len(), 3);

    let crit_ids: Vec<&str> = ctx.criteria.iter().map(|c| c.id.as_str()).collect();
    assert!(crit_ids.contains(&"valid-creds"));
    assert!(crit_ids.contains(&"invalid-password"));
    assert!(crit_ids.contains(&"rate-limit"));
}

#[test]
fn auth_login_context_validation_status() {
    let (graph, _) = build_auth_login_scenario();
    let ctx = graph.context("auth/req/login").unwrap();

    // 9.2: valid-creds is referenced by auth/prop/token-generation with status "verified".
    let valid_creds = ctx.criteria.iter().find(|c| c.id == "valid-creds").unwrap();
    assert!(
        valid_creds
            .referenced_by
            .iter()
            .any(|d| d.doc_id == "auth/prop/token-generation"
                && d.status.as_deref() == Some("verified")),
        "valid-creds should be referenced by token-generation: {:?}",
        valid_creds.referenced_by
    );

    // invalid-password and rate-limit have no referencing docs.
    let invalid_pw = ctx
        .criteria
        .iter()
        .find(|c| c.id == "invalid-password")
        .unwrap();
    assert!(
        invalid_pw.referenced_by.is_empty(),
        "invalid-password should have no referencing docs"
    );

    let rate_limit = ctx.criteria.iter().find(|c| c.id == "rate-limit").unwrap();
    assert!(
        rate_limit.referenced_by.is_empty(),
        "rate-limit should have no referencing docs"
    );
}

#[test]
fn auth_login_context_implementing_docs() {
    let (graph, _) = build_auth_login_scenario();
    let ctx = graph.context("auth/req/login").unwrap();

    // 9.3: Implementing documents.
    assert!(
        ctx.implemented_by
            .iter()
            .any(|d| d.doc_id == "auth/design/login-flow"),
        "should be implemented by login-flow: {:?}",
        ctx.implemented_by
    );
}

#[test]
fn auth_login_context_criterion_references() {
    let (graph, _) = build_auth_login_scenario();
    let ctx = graph.context("auth/req/login").unwrap();

    // 9.4: Criterion-level references include the example doc.
    let valid_creds = ctx.criteria.iter().find(|c| c.id == "valid-creds").unwrap();
    assert!(
        valid_creds
            .referenced_by
            .iter()
            .any(|d| d.doc_id == "auth/example/login-happy-path"),
        "valid-creds should be referenced by login-happy-path: {:?}",
        valid_creds.referenced_by
    );
}

#[test]
fn auth_login_context_tasks_in_topo_order() {
    let (graph, _) = build_auth_login_scenario();
    let ctx = graph.context("auth/req/login").unwrap();

    // 9.5: Only the task that implements a criterion in this doc is linked.
    // adapter-code implements auth/req/login#valid-creds.
    assert!(
        ctx.tasks.iter().any(|t| t.task_id == "adapter-code"),
        "adapter-code should be in linked tasks: {:?}",
        ctx.tasks
    );

    // The task should carry its implements refs.
    let adapter = ctx
        .tasks
        .iter()
        .find(|t| t.task_id == "adapter-code")
        .unwrap();
    assert!(
        adapter
            .implements
            .contains(&("auth/req/login".to_owned(), "valid-creds".to_owned())),
        "adapter-code should implement valid-creds: {:?}",
        adapter.implements
    );
}

#[test]
fn auth_login_plan_outstanding_targets() {
    let (graph, _) = build_auth_login_scenario();
    let plan = graph
        .plan(&PlanQuery::Document("auth/req/login".to_owned()))
        .expect("plan should succeed");

    // 10.1: Outstanding criteria — all three are outstanding because
    // References links are informational (no verification semantics) and
    // the task implementing valid-creds is in-progress, not done.
    // Evidence-based coverage filtering happens at the CLI layer.
    let outstanding_ids: Vec<&str> = plan
        .outstanding_targets
        .iter()
        .map(|c| c.target_id.as_str())
        .collect();
    assert!(
        outstanding_ids.contains(&"invalid-password"),
        "invalid-password should be outstanding: {outstanding_ids:?}"
    );
    assert!(
        outstanding_ids.contains(&"rate-limit"),
        "rate-limit should be outstanding: {outstanding_ids:?}"
    );
    assert!(
        outstanding_ids.contains(&"valid-creds"),
        "valid-creds should be outstanding (References are informational, task not done): {outstanding_ids:?}"
    );
}

/// 10.1 updated: A done task implementing a criterion makes it non-outstanding.
#[test]
fn done_task_implementing_criterion_makes_it_non_outstanding() {
    let config = single_project_config();

    let req_doc = make_doc_full(
        "my/req",
        Some("requirements"),
        Some("approved"),
        vec![make_acceptance_criteria(
            vec![
                make_criterion("crit-a", 3),
                make_criterion("crit-b", 4),
                make_criterion("crit-c", 5),
            ],
            2,
        )],
    );

    // Tasks doc: done task implements crit-a, in-progress task implements crit-b
    let tasks_doc = make_doc(
        "my/tasks",
        vec![
            make_task("task-1", Some("done"), Some("my/req#crit-a"), None, 1),
            make_task(
                "task-2",
                Some("in-progress"),
                Some("my/req#crit-b"),
                Some("task-1"),
                2,
            ),
        ],
    );

    let graph = build_graph(vec![req_doc, tasks_doc], &config).expect("graph should build");
    let plan = graph
        .plan(&PlanQuery::Document("my/req".to_owned()))
        .expect("plan should succeed");

    let outstanding_ids: Vec<&str> = plan
        .outstanding_targets
        .iter()
        .map(|c| c.target_id.as_str())
        .collect();

    // crit-a: done task implements it → NOT outstanding
    assert!(
        !outstanding_ids.contains(&"crit-a"),
        "crit-a should NOT be outstanding (done task implements it): {outstanding_ids:?}"
    );
    // crit-b: in-progress task implements it → still outstanding
    assert!(
        outstanding_ids.contains(&"crit-b"),
        "crit-b should be outstanding (task not done): {outstanding_ids:?}"
    );
    // crit-c: no task implements it → outstanding
    assert!(
        outstanding_ids.contains(&"crit-c"),
        "crit-c should be outstanding (no task): {outstanding_ids:?}"
    );
}

#[test]
fn auth_login_plan_pending_and_completed_tasks() {
    let (graph, _) = build_auth_login_scenario();
    let plan = graph
        .plan(&PlanQuery::Document("auth/req/login".to_owned()))
        .unwrap();

    // 10.2: Pending tasks (status ≠ done) — adapter-code is the only linked task
    // and it's in-progress, so it should be pending.
    let pending_ids: Vec<&str> = plan
        .pending_tasks
        .iter()
        .map(|t| t.task_id.as_str())
        .collect();
    assert!(
        pending_ids.contains(&"adapter-code"),
        "adapter-code should be pending: {pending_ids:?}"
    );

    // 10.3: No completed linked tasks (type-alignment is done but doesn't
    // implement any criterion, so it's not linked).
    // adapter-code is in-progress, not done.
    assert!(
        !plan
            .completed_tasks
            .iter()
            .any(|t| t.task_id == "adapter-code"),
        "adapter-code is in-progress, not completed"
    );
}

#[test]
fn auth_login_plan_has_no_illustration_field() {
    let (graph, _) = build_auth_login_scenario();
    let plan = graph
        .plan(&PlanQuery::Document("auth/req/login".to_owned()))
        .unwrap();

    // Plan output no longer carries illustrations (removed with Illustrates).
    // Verify the plan succeeds and has expected structure.
    assert!(!plan.outstanding_targets.is_empty());
}

#[test]
fn auth_login_tracked_files() {
    let (graph, _) = build_auth_login_scenario();

    let globs = graph
        .tracked_files("auth/req/login")
        .expect("should have tracked files");
    assert!(
        globs.contains(&"src/auth/**/*.rs".to_owned()),
        "should contain the auth glob: {globs:?}"
    );
}

#[test]
fn auth_login_prefix_plan() {
    let (graph, _) = build_auth_login_scenario();

    // Plan by prefix "auth/" should aggregate all auth documents.
    let plan = graph
        .plan(&PlanQuery::Prefix("auth/".to_owned()))
        .expect("prefix plan should succeed");

    // Should include outstanding criteria from auth/req/login.
    assert!(
        plan.outstanding_targets
            .iter()
            .any(|c| c.doc_id == "auth/req/login" && c.target_id == "invalid-password"),
        "prefix plan should include outstanding criteria"
    );
}
