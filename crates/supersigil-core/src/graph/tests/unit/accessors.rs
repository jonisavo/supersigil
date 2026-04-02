use super::*;

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

// ===========================================================================
// Example `verifies` attribute creates reference edges in graph
// ===========================================================================

/// An Example with `verifies="other-doc/req#crit-a"` creates a reference
/// edge that appears in the `references_reverse` mapping, enabling
/// find-all-references from the criterion back to the Example.
#[test]
fn example_verifies_creates_reference_edge() {
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
            None,
            Some("other-doc/req#crit-a"),
            1,
        )],
    );

    let graph =
        build_graph(vec![req_doc, example_doc], &config).expect("graph should build successfully");

    // The verifies edge should appear in references_reverse.
    let refs = graph.references("other-doc/req", Some("crit-a"));
    assert!(
        refs.contains("my/spec"),
        "Example verifies should create a reference edge: {refs:?}"
    );
}

/// An Example with both `references` and `verifies` should create edges for both.
#[test]
fn example_with_both_references_and_verifies() {
    let config = single_project_config();

    let req_doc = make_doc(
        "my/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 2), make_criterion("crit-b", 3)],
            1,
        )],
    );

    let example_doc = make_doc(
        "my/spec",
        vec![make_example(
            "ex-1",
            "sh",
            Some("my/req#crit-a"),
            Some("my/req#crit-b"),
            1,
        )],
    );

    let graph =
        build_graph(vec![req_doc, example_doc], &config).expect("graph should build successfully");

    // Both references and verifies should create reference edges.
    let refs_a = graph.references("my/req", Some("crit-a"));
    assert!(
        refs_a.contains("my/spec"),
        "Example references should create an edge to crit-a: {refs_a:?}"
    );

    let refs_b = graph.references("my/req", Some("crit-b"));
    assert!(
        refs_b.contains("my/spec"),
        "Example verifies should create an edge to crit-b: {refs_b:?}"
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

// ===========================================================================
// component_at_path, resolved_refs_for_doc, task_implements_for_doc
// ===========================================================================

#[test]
fn component_at_path_returns_top_level_component() {
    let (graph, _) = build_auth_login_scenario();
    // The req doc has components: [TrackedFiles(0), AcceptanceCriteria(1)]
    // AcceptanceCriteria has children: [Criterion "valid-creds"(0), Criterion "invalid-password"(1), ...]
    // Path [1, 0] should be the first Criterion (valid-creds).
    let comp = graph
        .component_at_path("auth/req/login", &[1, 0])
        .expect("should find component at path [1, 0]");
    assert_eq!(comp.name, "Criterion");
    assert_eq!(
        comp.attributes.get("id").map(String::as_str),
        Some("valid-creds")
    );
}

#[test]
fn component_at_path_returns_none_for_invalid_path() {
    let (graph, _) = build_auth_login_scenario();
    assert!(graph.component_at_path("auth/req/login", &[99]).is_none());
}

#[test]
fn component_at_path_returns_none_for_unknown_doc() {
    let (graph, _) = build_auth_login_scenario();
    assert!(graph.component_at_path("nonexistent", &[0]).is_none());
}

#[test]
fn resolved_refs_for_doc_returns_refs_from_document() {
    let (graph, _) = build_auth_login_scenario();
    // auth/prop/token-generation has a References component pointing at auth/req/login#valid-creds
    let refs: Vec<_> = graph
        .resolved_refs_for_doc("auth/prop/token-generation")
        .collect();
    assert!(!refs.is_empty(), "should have resolved refs");
    // Check that one of the resolved refs targets auth/req/login
    let has_target = refs
        .iter()
        .any(|(_, resolved)| resolved.iter().any(|r| r.target_doc_id == "auth/req/login"));
    assert!(has_target, "should reference auth/req/login: {refs:?}");
}

#[test]
fn resolved_refs_for_doc_returns_empty_for_doc_without_refs() {
    let (graph, _) = build_auth_login_scenario();
    // auth/req/login has no outgoing refs
    let refs: Vec<_> = graph.resolved_refs_for_doc("auth/req/login").collect();
    assert!(
        refs.is_empty(),
        "req doc should have no outgoing refs: {refs:?}"
    );
}

#[test]
fn resolved_refs_for_doc_returns_empty_for_unknown_doc() {
    let (graph, _) = build_auth_login_scenario();
    let refs: Vec<_> = graph.resolved_refs_for_doc("nonexistent").collect();
    assert!(refs.is_empty());
}

#[test]
fn task_implements_for_doc_returns_entries() {
    let (graph, _) = build_auth_login_scenario();
    // auth/tasks/login has adapter-code implementing auth/req/login#valid-creds
    let entries: Vec<_> = graph.task_implements_for_doc("auth/tasks/login").collect();
    assert!(!entries.is_empty(), "should have task implements entries");
    let has_adapter = entries
        .iter()
        .any(|(task_id, _)| *task_id == "adapter-code");
    assert!(has_adapter, "should have adapter-code: {entries:?}");
}

#[test]
fn task_implements_for_doc_returns_empty_for_doc_without_tasks() {
    let (graph, _) = build_auth_login_scenario();
    let entries: Vec<_> = graph.task_implements_for_doc("auth/req/login").collect();
    assert!(entries.is_empty());
}

#[test]
fn task_implements_for_doc_returns_empty_for_unknown_doc() {
    let (graph, _) = build_auth_login_scenario();
    let entries: Vec<_> = graph.task_implements_for_doc("nonexistent").collect();
    assert!(entries.is_empty());
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
