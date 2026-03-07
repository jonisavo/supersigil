//! Unit tests for concrete examples and edge cases.
//!
//! These complement the property tests by covering:
//! - The `auth/req/login` example from the supersigil design document
//! - Edge cases: empty collections, no components, self-cycles, empty queries
//! - Cross-phase error aggregation

use crate::SpecDocument;
use crate::graph::query::PlanQuery;
use crate::graph::tests::generators::{
    make_acceptance_criteria, make_criterion, make_doc, make_doc_full, make_refs_component,
    make_task, make_tracked_files_component, single_project_config,
};
use crate::graph::{GraphError, ILLUSTRATES, IMPLEMENTS, VALIDATES, build_graph};

// ===========================================================================
// 18.1: Concrete examples from the supersigil design document
// ===========================================================================

/// Build the auth/req/login scenario from the design document:
///
/// - `auth/req/login` — requirement with 3 criteria (valid-creds,
///   invalid-password, rate-limit) and `TrackedFiles`
/// - `auth/prop/token-generation` — validates `auth/req/login#valid-creds`
/// - `auth/design/login-flow` — implements `auth/req/login`
/// - `auth/tasks/login` — tasks doc with 4 tasks in dependency chain,
///   one implementing `#valid-creds`
/// - `auth/example/login-happy-path` — illustrates `auth/req/login#valid-creds`
#[allow(
    clippy::too_many_lines,
    reason = "test scenario builder with many documents"
)]
fn build_auth_login_scenario() -> (
    crate::graph::DocumentGraph,
    SpecDocument, // req doc (for assertion)
) {
    let config = single_project_config();

    // Requirement document: auth/req/login
    let req_doc = make_doc_full(
        "auth/req/login",
        Some("requirement"),
        Some("approved"),
        vec![
            make_tracked_files_component("src/auth/**/*.rs", 1),
            make_acceptance_criteria(
                vec![
                    make_criterion("valid-creds", 3),
                    make_criterion("invalid-password", 4),
                    make_criterion("rate-limit", 5),
                ],
                2,
            ),
        ],
    );

    // Property document: validates valid-creds
    let prop_doc = make_doc_full(
        "auth/prop/token-generation",
        Some("property"),
        Some("verified"),
        vec![make_refs_component(
            VALIDATES,
            "auth/req/login#valid-creds",
            1,
        )],
    );

    // Design document: implements auth/req/login
    let design_doc = make_doc_full(
        "auth/design/login-flow",
        Some("design"),
        Some("approved"),
        vec![make_refs_component(IMPLEMENTS, "auth/req/login", 1)],
    );

    // Tasks document with dependency chain:
    // type-alignment (done) → adapter-code (in-progress, implements #valid-creds)
    //   → switch-over (ready) → cleanup (draft)
    let tasks_doc = make_doc(
        "auth/tasks/login",
        vec![
            make_task("type-alignment", Some("done"), None, None, 1),
            make_task(
                "adapter-code",
                Some("in-progress"),
                Some("auth/req/login#valid-creds"),
                Some("type-alignment"),
                2,
            ),
            make_task("switch-over", Some("ready"), None, Some("adapter-code"), 3),
            make_task("cleanup", Some("draft"), None, Some("switch-over"), 4),
        ],
    );

    // Example document: illustrates valid-creds criterion
    let example_doc = make_doc(
        "auth/example/login-happy-path",
        vec![make_refs_component(
            ILLUSTRATES,
            "auth/req/login#valid-creds",
            1,
        )],
    );

    let graph = build_graph(
        vec![
            req_doc.clone(),
            prop_doc,
            design_doc,
            tasks_doc,
            example_doc,
        ],
        &config,
    )
    .expect("auth/req/login scenario should build without errors");

    (graph, req_doc)
}

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

    // 9.2: valid-creds is validated by auth/prop/token-generation with status "verified".
    let valid_creds = ctx.criteria.iter().find(|c| c.id == "valid-creds").unwrap();
    assert!(
        valid_creds
            .validated_by
            .iter()
            .any(|d| d.doc_id == "auth/prop/token-generation"
                && d.status.as_deref() == Some("verified")),
        "valid-creds should be validated by token-generation: {:?}",
        valid_creds.validated_by
    );

    // invalid-password and rate-limit have no validators.
    let invalid_pw = ctx
        .criteria
        .iter()
        .find(|c| c.id == "invalid-password")
        .unwrap();
    assert!(
        invalid_pw.validated_by.is_empty(),
        "invalid-password should have no validators"
    );

    let rate_limit = ctx.criteria.iter().find(|c| c.id == "rate-limit").unwrap();
    assert!(
        rate_limit.validated_by.is_empty(),
        "rate-limit should have no validators"
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
fn auth_login_context_illustrations() {
    let (graph, _) = build_auth_login_scenario();
    let ctx = graph.context("auth/req/login").unwrap();

    // 9.4: Criterion-level illustration.
    let valid_creds = ctx.criteria.iter().find(|c| c.id == "valid-creds").unwrap();
    assert!(
        valid_creds
            .illustrated_by
            .contains(&"auth/example/login-happy-path".to_owned()),
        "valid-creds should be illustrated by login-happy-path: {:?}",
        valid_creds.illustrated_by
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
fn auth_login_plan_outstanding_criteria() {
    let (graph, _) = build_auth_login_scenario();
    let plan = graph
        .plan(&PlanQuery::Document("auth/req/login".to_owned()))
        .expect("plan should succeed");

    // 10.1: Outstanding criteria — invalid-password and rate-limit have no validators.
    let outstanding_ids: Vec<&str> = plan
        .outstanding_criteria
        .iter()
        .map(|c| c.criterion_id.as_str())
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
        !outstanding_ids.contains(&"valid-creds"),
        "valid-creds should NOT be outstanding (it has a validator): {outstanding_ids:?}"
    );
}

/// 10.1 updated: A done task implementing a criterion makes it non-outstanding.
#[test]
fn done_task_implementing_criterion_makes_it_non_outstanding() {
    let config = single_project_config();

    let req_doc = make_doc_full(
        "my/req",
        Some("requirement"),
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
        .outstanding_criteria
        .iter()
        .map(|c| c.criterion_id.as_str())
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
fn auth_login_plan_illustrations() {
    let (graph, _) = build_auth_login_scenario();
    let plan = graph
        .plan(&PlanQuery::Document("auth/req/login".to_owned()))
        .unwrap();

    // 10.4: Illustrating documents.
    assert!(
        plan.illustrated_by
            .iter()
            .any(|i| i.doc_id == "auth/example/login-happy-path"),
        "plan should include illustration: {:?}",
        plan.illustrated_by
    );
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
        plan.outstanding_criteria
            .iter()
            .any(|c| c.doc_id == "auth/req/login" && c.criterion_id == "invalid-password"),
        "prefix plan should include outstanding criteria"
    );
}

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
    assert!(graph.validates("bare-doc", None).is_empty());
    assert!(graph.implements("bare-doc").is_empty());
    assert!(graph.illustrates("bare-doc", None).is_empty());

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
    assert!(ctx.illustrated_by.is_empty());
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

    assert!(plan.outstanding_criteria.is_empty());
    assert!(plan.pending_tasks.is_empty());
    assert!(plan.completed_tasks.is_empty());
    assert!(plan.illustrated_by.is_empty());
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
        vec![make_refs_component(VALIDATES, "ghost/doc#phantom", 1)],
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
