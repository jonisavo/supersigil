//! Unit tests for concrete examples and edge cases.
//!
//! These complement the property tests by covering:
//! - The `auth/req/login` example from the supersigil design document
//! - Edge cases: empty collections, no components, self-cycles, empty queries
//! - Cross-phase error aggregation

use crate::SpecDocument;
use crate::graph::query::PlanQuery;
use crate::graph::tests::generators::{
    make_acceptance_criteria, make_criterion, make_doc, make_doc_full, make_example,
    make_refs_component, make_task, make_tracked_files_component, single_project_config,
};
use crate::graph::{GraphError, IMPLEMENTS, REFERENCES, build_graph};

// ===========================================================================
// 18.1: Concrete examples from the supersigil design document
// ===========================================================================

/// Build the auth/req/login scenario from the design document:
///
/// - `auth/req/login` — requirement with 3 criteria (valid-creds,
///   invalid-password, rate-limit) and `TrackedFiles`
/// - `auth/prop/token-generation` — references `auth/req/login#valid-creds`
/// - `auth/design/login-flow` — implements `auth/req/login`
/// - `auth/tasks/login` — tasks doc with 4 tasks in dependency chain,
///   one implementing `#valid-creds`
/// - `auth/example/login-happy-path` — references `auth/req/login#valid-creds`
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
        Some("requirements"),
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

    // Property document: references valid-creds
    let prop_doc = make_doc_full(
        "auth/prop/token-generation",
        Some("design"),
        Some("verified"),
        vec![make_refs_component(
            REFERENCES,
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

    // Example document: references valid-creds criterion
    let example_doc = make_doc(
        "auth/example/login-happy-path",
        vec![make_refs_component(
            REFERENCES,
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

// ===========================================================================
// criteria() and criteria_by_fragment()
// ===========================================================================

#[test]
fn criteria_yields_all_referenceable_components() {
    let config = single_project_config();

    let req_doc = make_doc(
        "my/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 2), make_criterion("crit-b", 3)],
            1,
        )],
    );

    let tasks_doc = make_doc(
        "my/tasks",
        vec![make_task("task-1", Some("todo"), None, None, 1)],
    );

    let graph = build_graph(vec![req_doc, tasks_doc], &config).expect("graph should build");

    let all: Vec<_> = graph.criteria().collect();

    // Should contain the two criteria and the task (all referenceable components).
    assert_eq!(all.len(), 3, "expected 3 referenceable components: {all:?}");

    // Check that doc_id and fragment are correct for each.
    assert!(
        all.iter().any(|(doc_id, frag, comp)| *doc_id == "my/req"
            && *frag == "crit-a"
            && comp.name == "Criterion"),
        "should contain crit-a: {all:?}"
    );
    assert!(
        all.iter().any(|(doc_id, frag, comp)| *doc_id == "my/req"
            && *frag == "crit-b"
            && comp.name == "Criterion"),
        "should contain crit-b: {all:?}"
    );
    assert!(
        all.iter().any(|(doc_id, frag, comp)| *doc_id == "my/tasks"
            && *frag == "task-1"
            && comp.name == "Task"),
        "should contain task-1: {all:?}"
    );
}

#[test]
fn criteria_by_fragment_finds_matches_across_documents() {
    let config = single_project_config();

    // Two documents that each have a component with fragment "shared-id".
    let doc_a = make_doc(
        "doc-a",
        vec![make_acceptance_criteria(
            vec![make_criterion("shared-id", 2)],
            1,
        )],
    );

    let doc_b = make_doc(
        "doc-b",
        vec![make_acceptance_criteria(
            vec![make_criterion("shared-id", 2)],
            1,
        )],
    );

    let graph = build_graph(vec![doc_a, doc_b], &config).expect("graph should build");

    let matches = graph.criteria_by_fragment("shared-id");
    assert_eq!(
        matches.len(),
        2,
        "should find shared-id in both documents: {matches:?}"
    );

    let doc_ids: Vec<&str> = matches.iter().map(|(doc_id, _)| *doc_id).collect();
    assert!(
        doc_ids.contains(&"doc-a"),
        "should include doc-a: {doc_ids:?}"
    );
    assert!(
        doc_ids.contains(&"doc-b"),
        "should include doc-b: {doc_ids:?}"
    );
}

// ===========================================================================
// implements_targets: forward direction of Implements relationships
// ===========================================================================

#[test]
fn implements_targets_returns_target_doc_ids() {
    let config = single_project_config();

    let req_doc = make_doc(
        "feat/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-1", 2)],
            1,
        )],
    );
    let design_doc = make_doc(
        "feat/design",
        vec![make_refs_component(IMPLEMENTS, "feat/req", 1)],
    );

    let graph = build_graph(vec![req_doc, design_doc], &config).expect("graph should build");

    let targets = graph.implements_targets("feat/design");
    assert_eq!(targets, &["feat/req"], "design should implement req");
}

#[test]
fn implements_targets_returns_empty_for_doc_without_implements() {
    let config = single_project_config();

    let doc = make_doc(
        "feat/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-1", 2)],
            1,
        )],
    );

    let graph = build_graph(vec![doc], &config).expect("graph should build");

    let targets = graph.implements_targets("feat/req");
    assert!(
        targets.is_empty(),
        "req doc should not implement anything: {targets:?}"
    );
}

#[test]
fn implements_targets_returns_empty_for_unknown_doc() {
    let config = single_project_config();
    let doc = make_doc("feat/req", vec![]);
    let graph = build_graph(vec![doc], &config).expect("graph should build");

    let targets = graph.implements_targets("nonexistent");
    assert!(targets.is_empty());
}

#[test]
fn criteria_by_fragment_returns_empty_for_unknown_fragment() {
    let config = single_project_config();

    let doc = make_doc(
        "my/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("known-id", 2)],
            1,
        )],
    );

    let graph = build_graph(vec![doc], &config).expect("graph should build");

    let matches = graph.criteria_by_fragment("unknown-fragment");
    assert!(
        matches.is_empty(),
        "should return empty vec for unknown fragment: {matches:?}"
    );
}

// ===========================================================================
// Example `references` attribute creates informational graph edges (req-1-5)
// ===========================================================================

/// An Example with `references="other-doc/req#crit-a"` creates a reference
/// edge that appears in the `references_reverse` mapping.
#[test]
fn example_references_creates_informational_edge() {
    let config = single_project_config();

    let req_doc = make_doc(
        "other-doc/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 2)],
            1,
        )],
    );

    let example_doc = make_doc(
        "my/spec",
        vec![make_example(
            "ex-1",
            "sh",
            Some("other-doc/req#crit-a"),
            None,
            1,
        )],
    );

    let graph =
        build_graph(vec![req_doc, example_doc], &config).expect("graph should build successfully");

    // The reference edge should appear in references_reverse.
    let refs = graph.references("other-doc/req", Some("crit-a"));
    assert!(
        refs.contains("my/spec"),
        "Example references should create a reference edge: {refs:?}"
    );
}

/// An Example with `references` targeting a document (no fragment) creates
/// a document-level reference edge.
#[test]
fn example_references_creates_document_level_edge() {
    let config = single_project_config();

    let target_doc = make_doc("target/req", vec![]);

    let example_doc = make_doc(
        "my/spec",
        vec![make_example("ex-1", "sh", Some("target/req"), None, 1)],
    );

    let graph = build_graph(vec![target_doc, example_doc], &config).expect("graph should build");

    let refs = graph.references("target/req", None);
    assert!(
        refs.contains("my/spec"),
        "Example references should create a document-level reference edge: {refs:?}"
    );
}

/// An Example with `references` does NOT create verification evidence.
/// The reference edge should be purely informational — it should appear
/// in `references_reverse` but NOT affect plan outstanding targets.
#[test]
fn example_references_does_not_create_verification_evidence() {
    let config = single_project_config();

    let req_doc = make_doc(
        "my/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 2)],
            1,
        )],
    );

    // Example references crit-a but does NOT verify it.
    let example_doc = make_doc(
        "my/spec",
        vec![make_example("ex-1", "sh", Some("my/req#crit-a"), None, 1)],
    );

    let graph = build_graph(vec![req_doc, example_doc], &config).expect("graph should build");

    // crit-a should still be outstanding (references are informational).
    let plan = graph
        .plan(&PlanQuery::Document("my/req".to_owned()))
        .expect("plan should succeed");

    let outstanding_ids: Vec<&str> = plan
        .outstanding_targets
        .iter()
        .map(|c| c.target_id.as_str())
        .collect();
    assert!(
        outstanding_ids.contains(&"crit-a"),
        "crit-a should still be outstanding (references are informational): {outstanding_ids:?}"
    );
}

/// An Example without `references` works as before (no regression).
#[test]
fn example_without_references_has_no_reference_edges() {
    let config = single_project_config();

    let req_doc = make_doc(
        "my/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 2)],
            1,
        )],
    );

    let example_doc = make_doc("my/spec", vec![make_example("ex-1", "sh", None, None, 1)]);

    let graph = build_graph(vec![req_doc, example_doc], &config).expect("graph should build");

    // No reference edges from the example doc.
    let refs = graph.references("my/req", Some("crit-a"));
    assert!(
        !refs.contains("my/spec"),
        "Example without references should not create reference edges: {refs:?}"
    );
}

/// An Example with `references` shows up in context query's `referenced_by`.
#[test]
fn example_references_appears_in_context_referenced_by() {
    let config = single_project_config();

    let req_doc = make_doc_full(
        "my/req",
        Some("requirements"),
        Some("approved"),
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 2)],
            1,
        )],
    );

    let example_doc = make_doc(
        "my/spec",
        vec![make_example("ex-1", "sh", Some("my/req#crit-a"), None, 1)],
    );

    let graph = build_graph(vec![req_doc, example_doc], &config).expect("graph should build");

    let ctx = graph.context("my/req").expect("context should succeed");

    let crit_a = ctx.criteria.iter().find(|c| c.id == "crit-a").unwrap();
    assert!(
        crit_a.referenced_by.iter().any(|d| d.doc_id == "my/spec"),
        "crit-a should be referenced by the example doc: {:?}",
        crit_a.referenced_by
    );
}
