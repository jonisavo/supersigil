//! Unit tests for concrete examples and edge cases.
//!
//! These complement the property tests by covering:
//! - The `auth/req/login` example from the supersigil design document
//! - Edge cases: empty collections, no components, self-cycles, empty queries
//! - Cross-phase error aggregation

use std::collections::HashMap;

use crate::graph::query::{PlanQuery, QueryError};
use crate::graph::tests::generators::{
    make_acceptance_criteria, make_depends_on, make_doc, make_doc_full, make_refs_component,
    make_tracked_files_component, pos, single_project_config,
};
use crate::graph::{CRITERION, GraphError, ILLUSTRATES, IMPLEMENTS, TASK, VALIDATES, build_graph};
use crate::{ExtractedComponent, SpecDocument};

// ---------------------------------------------------------------------------
// Helpers (unit-test-specific: explicit body text)
// ---------------------------------------------------------------------------

fn make_criterion_with_body(id: &str, body: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: CRITERION.to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children: Vec::new(),
        body_text: Some(body.to_owned()),
        position: pos(line),
    }
}

fn make_task_with_body(
    id: &str,
    status: Option<&str>,
    implements: Option<&str>,
    depends: Option<&str>,
    body: &str,
    line: usize,
) -> ExtractedComponent {
    let mut attributes = HashMap::from([("id".to_owned(), id.to_owned())]);
    if let Some(s) = status {
        attributes.insert("status".to_owned(), s.to_owned());
    }
    if let Some(i) = implements {
        attributes.insert("implements".to_owned(), i.to_owned());
    }
    if let Some(d) = depends {
        attributes.insert("depends".to_owned(), d.to_owned());
    }
    ExtractedComponent {
        name: TASK.to_owned(),
        attributes,
        children: Vec::new(),
        body_text: Some(body.to_owned()),
        position: pos(line),
    }
}

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
                    make_criterion_with_body(
                        "valid-creds",
                        "WHEN valid email and password, THEN return 200 with session token.",
                        3,
                    ),
                    make_criterion_with_body(
                        "invalid-password",
                        "WHEN incorrect password, THEN return 401.",
                        4,
                    ),
                    make_criterion_with_body(
                        "rate-limit",
                        "WHEN 5 failed attempts in 15 min, THEN return 429.",
                        5,
                    ),
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
            make_task_with_body(
                "type-alignment",
                Some("done"),
                None,
                None,
                "Align types.",
                1,
            ),
            make_task_with_body(
                "adapter-code",
                Some("in-progress"),
                Some("auth/req/login#valid-creds"),
                Some("type-alignment"),
                "Implement the login handler.",
                2,
            ),
            make_task_with_body(
                "switch-over",
                Some("ready"),
                None,
                Some("adapter-code"),
                "Switch traffic to new handler.",
                3,
            ),
            make_task_with_body(
                "cleanup",
                Some("draft"),
                None,
                Some("switch-over"),
                "Remove old handler.",
                4,
            ),
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
fn auth_login_plan_is_structured_type() {
    let (graph, _) = build_auth_login_scenario();
    let plan = graph
        .plan(&PlanQuery::Document("auth/req/login".to_owned()))
        .unwrap();

    // 10.8: PlanOutput is a structured data type — verify we can access fields.
    let _ = &plan.outstanding_criteria;
    let _ = &plan.pending_tasks;
    let _ = &plan.completed_tasks;
    let _ = &plan.illustrated_by;
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
fn self_referencing_task_depends_produces_cycle_error() {
    let config = single_project_config();
    let doc = make_doc(
        "tasks/self-ref",
        vec![make_task_with_body(
            "t0",
            None,
            None,
            Some("t0"),
            "Self-ref task.",
            1,
        )],
    );

    let result = build_graph(vec![doc], &config);
    let errors = result.expect_err("self-referencing depends should produce error");

    let has_cycle = errors.iter().any(|e| match e {
        GraphError::TaskDependencyCycle { doc_id, cycle } => {
            doc_id == "tasks/self-ref" && cycle.contains(&"t0".to_owned())
        }
        _ => false,
    });
    assert!(
        has_cycle,
        "should have TaskDependencyCycle for t0: {errors:?}"
    );
}

#[test]
fn self_referencing_depends_on_produces_doc_cycle_error() {
    let config = single_project_config();
    let doc = make_doc("doc/self-ref", vec![make_depends_on("doc/self-ref", 1)]);

    let result = build_graph(vec![doc], &config);
    let errors = result.expect_err("self-referencing DependsOn should produce error");

    let has_doc_cycle = errors.iter().any(|e| match e {
        GraphError::DocumentDependencyCycle { cycle } => cycle.contains(&"doc/self-ref".to_owned()),
        _ => false,
    });
    assert!(
        has_doc_cycle,
        "should have DocumentDependencyCycle: {errors:?}"
    );
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
fn context_for_nonexistent_doc_returns_error() {
    let config = single_project_config();
    let doc = make_doc("exists", vec![]);

    let graph = build_graph(vec![doc], &config).expect("should succeed");
    let result = graph.context("does-not-exist");

    match result {
        Err(QueryError::DocumentNotFound { id }) => {
            assert_eq!(id, "does-not-exist");
        }
        other => panic!("expected DocumentNotFound, got: {other:?}"),
    }
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

#[test]
fn plan_for_nonexistent_doc_returns_error() {
    let config = single_project_config();
    let doc = make_doc("exists", vec![]);

    let graph = build_graph(vec![doc], &config).expect("should succeed");
    let result = PlanQuery::parse(Some("ghost-doc"), &graph);

    match result {
        Err(QueryError::NoMatchingDocuments { query }) => {
            assert_eq!(query, "ghost-doc");
        }
        other => panic!("expected NoMatchingDocuments, got: {other:?}"),
    }
}

// ===========================================================================
// 18.3: Cross-phase error aggregation
// ===========================================================================

#[test]
fn broken_refs_and_cycles_reported_together() {
    let config = single_project_config();

    // Document with a broken Validates ref.
    let broken_doc = make_doc(
        "doc/broken",
        vec![make_refs_component(VALIDATES, "nonexistent/target", 1)],
    );

    // Tasks document with a dependency cycle (a → b → a).
    let cyclic_doc = make_doc(
        "tasks/cyclic",
        vec![
            make_task_with_body("ta", None, None, Some("tb"), "Task A.", 1),
            make_task_with_body("tb", None, None, Some("ta"), "Task B.", 2),
        ],
    );

    let result = build_graph(vec![broken_doc, cyclic_doc], &config);
    let errors = result.expect_err("should fail with both error types");

    let has_broken_ref = errors
        .iter()
        .any(|e| matches!(e, GraphError::BrokenRef { .. }));
    let has_task_cycle = errors
        .iter()
        .any(|e| matches!(e, GraphError::TaskDependencyCycle { .. }));

    assert!(has_broken_ref, "should contain BrokenRef: {errors:?}");
    assert!(
        has_task_cycle,
        "should contain TaskDependencyCycle: {errors:?}"
    );
}

#[test]
fn duplicate_component_ids_and_broken_refs_reported_together() {
    let config = single_project_config();

    // Document with duplicate criterion IDs.
    let dup_doc = make_doc(
        "doc/dup-crit",
        vec![make_acceptance_criteria(
            vec![
                make_criterion_with_body("same-id", "First.", 2),
                make_criterion_with_body("same-id", "Second.", 3),
            ],
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

#[test]
fn broken_ref_and_document_cycle_reported_together() {
    let config = single_project_config();

    // Document with a broken ref.
    let broken_doc = make_doc(
        "doc/broken",
        vec![make_refs_component(VALIDATES, "nonexistent/doc", 1)],
    );

    // Two documents forming a mutual DependsOn cycle.
    let cycle_a = make_doc("cycle/a", vec![make_depends_on("cycle/b", 1)]);
    let cycle_b = make_doc("cycle/b", vec![make_depends_on("cycle/a", 1)]);

    let result = build_graph(vec![broken_doc, cycle_a, cycle_b], &config);
    let errors = result.expect_err("should fail with both error types");

    let has_broken_ref = errors
        .iter()
        .any(|e| matches!(e, GraphError::BrokenRef { .. }));
    let has_doc_cycle = errors
        .iter()
        .any(|e| matches!(e, GraphError::DocumentDependencyCycle { .. }));

    assert!(has_broken_ref, "should contain BrokenRef: {errors:?}");
    assert!(
        has_doc_cycle,
        "should contain DocumentDependencyCycle: {errors:?}"
    );
}
