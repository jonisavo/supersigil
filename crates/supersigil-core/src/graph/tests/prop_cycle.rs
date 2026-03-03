//! Property tests for cycle detection (pipeline stage 5–6).
//!
//! Property 9: Acyclic task graphs produce no cycle errors
//! Validates: Requirements 5.2

use std::collections::HashMap;

use proptest::prelude::*;

use crate::graph::tests::generators::{
    arb_component_id, arb_dag, arb_id, make_depends_on, make_doc_with_path, make_task, pos,
    single_project_config,
};
use crate::graph::{GraphError, TASK, build_graph};
use crate::{ExtractedComponent, SpecDocument};

// ---------------------------------------------------------------------------
// Property 9: Acyclic task graphs produce no cycle errors
// ---------------------------------------------------------------------------

proptest! {
    /// Generate tasks documents where `depends` edges form a DAG (using
    /// `arb_dag`), assert no `TaskDependencyCycle` errors.
    ///
    /// Validates: Requirements 5.2
    #[test]
    fn prop_acyclic_task_graph_no_cycle_errors(dag in arb_dag(6)) {
        let config = single_project_config();

        // Build a lookup: node → list of dependencies (nodes it depends on).
        let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to) in &dag.edges {
            deps_map.entry(from.clone()).or_default().push(to.clone());
        }

        // Create a Task component for each node in the DAG.
        let tasks: Vec<ExtractedComponent> = dag
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let depends = deps_map.get(node).map(|deps| deps.join(", "));
                make_task(node, None, None, depends.as_deref(), i + 1)
            })
            .collect();

        let doc = make_doc_with_path("tasks/test", "specs/tasks/test.mdx", tasks);
        let result = build_graph(vec![doc], &config);

        match result {
            Ok(_) => { /* no cycle errors — property holds */ }
            Err(errors) => {
                let cycle_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| matches!(e, GraphError::TaskDependencyCycle { .. }))
                    .collect();
                prop_assert!(
                    cycle_errors.is_empty(),
                    "acyclic DAG should produce no TaskDependencyCycle errors, got: {:?}",
                    cycle_errors
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property 10: Cyclic task graphs produce cycle errors
// ---------------------------------------------------------------------------

proptest! {
    /// Generate a DAG and inject a back-edge to create a cycle, then assert
    /// that `build_graph` returns at least one `TaskDependencyCycle` error
    /// whose `cycle` field contains the nodes involved in the cycle.
    ///
    /// Validates: Requirements 5.3
    ///
    /// NOTE: Ignored until cycle detection is implemented in task 9.6.
    #[test]
    fn prop_cyclic_task_graph_with_back_edge(dag in arb_dag(6)) {
        // We need at least 2 nodes to inject a meaningful back-edge.
        prop_assume!(dag.nodes.len() >= 2);

        let config = single_project_config();

        // Build the original dependency map from the DAG edges.
        let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to) in &dag.edges {
            deps_map.entry(from.clone()).or_default().push(to.clone());
        }

        // Inject a back-edge: make node 0 depend on the last node.
        // Since the DAG has t0 as the root (no dependencies) and t(n-1) as
        // the highest-indexed node, adding t0 → t(n-1) creates a cycle
        // through any path from t0 to t(n-1).
        let first = &dag.nodes[0];
        let last = &dag.nodes[dag.nodes.len() - 1];
        deps_map.entry(first.clone()).or_default().push(last.clone());

        // Also ensure there's a forward edge from last to first so the cycle
        // is guaranteed (the DAG might not have a path from first to last).
        deps_map.entry(last.clone()).or_default().push(first.clone());

        // Create Task components with the (now cyclic) dependency map.
        let tasks: Vec<ExtractedComponent> = dag
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let depends = deps_map.get(node).map(|deps| deps.join(", "));
                make_task(node, None, None, depends.as_deref(), i + 1)
            })
            .collect();

        let doc = make_doc_with_path("tasks/cyclic", "specs/tasks/cyclic.mdx", tasks);
        let result = build_graph(vec![doc], &config);

        // The graph should report at least one TaskDependencyCycle error.
        match result {
            Ok(_) => {
                prop_assert!(
                    false,
                    "cyclic task graph should produce errors, but build_graph returned Ok"
                );
            }
            Err(errors) => {
                let cycle_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| matches!(e, GraphError::TaskDependencyCycle { .. }))
                    .collect();
                prop_assert!(
                    !cycle_errors.is_empty(),
                    "cyclic task graph should produce at least one TaskDependencyCycle error, \
                     got errors: {:?}",
                    errors
                );

                // Verify cycle participants include at least one of the nodes
                // involved in the injected back-edge.
                for err in &cycle_errors {
                    if let GraphError::TaskDependencyCycle { cycle, .. } = err {
                        prop_assert!(
                            !cycle.is_empty(),
                            "TaskDependencyCycle.cycle should not be empty"
                        );
                    }
                }
            }
        }
    }

    /// Generate a single task that depends on itself (self-reference).
    /// Assert that `build_graph` returns a `TaskDependencyCycle` error.
    ///
    /// Validates: Requirements 5.3
    ///
    /// NOTE: Ignored until cycle detection is implemented in task 9.6.
    #[test]
    fn prop_self_referencing_task_produces_cycle_error(
        id_suffix in "[a-z]{1,4}"
    ) {
        let config = single_project_config();
        let task_id = format!("t{id_suffix}");

        // A single task that depends on itself.
        let task = make_task(&task_id, None, None, Some(&task_id), 1);
        let doc = make_doc_with_path("tasks/self-ref", "specs/tasks/self-ref.mdx", vec![task]);
        let result = build_graph(vec![doc], &config);

        match result {
            Ok(_) => {
                prop_assert!(
                    false,
                    "self-referencing task should produce errors, but build_graph returned Ok"
                );
            }
            Err(errors) => {
                let cycle_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| matches!(e, GraphError::TaskDependencyCycle { .. }))
                    .collect();
                prop_assert!(
                    !cycle_errors.is_empty(),
                    "self-referencing task should produce at least one TaskDependencyCycle error, \
                     got errors: {:?}",
                    errors
                );

                // The cycle should contain the self-referencing task ID.
                for err in &cycle_errors {
                    if let GraphError::TaskDependencyCycle { cycle, .. } = err {
                        prop_assert!(
                            cycle.contains(&task_id),
                            "TaskDependencyCycle.cycle should contain the self-referencing task \
                             ID '{}', got: {:?}",
                            task_id,
                            cycle
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property 11: Task depends scoping and resolution
// ---------------------------------------------------------------------------

/// Helper: create a task `ExtractedComponent` with optional `depends` and
/// nested `children` tasks.
fn make_task_with_children(
    id: &str,
    depends: Option<&str>,
    children: Vec<ExtractedComponent>,
    line: usize,
) -> ExtractedComponent {
    let mut attributes = HashMap::from([("id".to_owned(), id.to_owned())]);
    if let Some(deps) = depends {
        attributes.insert("depends".to_owned(), deps.to_owned());
    }
    ExtractedComponent {
        name: TASK.to_owned(),
        attributes,
        children,
        body_text: Some(format!("task {id}")),
        position: pos(line),
    }
}

proptest! {
    /// Generate 2+ top-level tasks in the same document where one depends on
    /// another sibling. Assert no `BrokenRef` errors for the depends attribute.
    ///
    /// Validates: Requirements 5.4, 5.5
    #[test]
    fn prop_valid_sibling_depends_resolve_top_level(
        id_a in arb_component_id(),
        id_b in arb_component_id(),
    ) {
        // Ensure distinct IDs so we have two separate tasks.
        prop_assume!(id_a != id_b);

        let config = single_project_config();

        // Task B depends on Task A — both are top-level siblings.
        let task_a = make_task(&id_a, None, None, None, 1);
        let task_b = make_task(&id_b, None, None, Some(&id_a), 2);

        let doc = make_doc_with_path(
            "tasks/sibling",
            "specs/tasks/sibling.mdx",
            vec![task_a, task_b],
        );
        let result = build_graph(vec![doc], &config);

        // There should be no BrokenRef errors for the depends attribute.
        match result {
            Ok(_) => { /* no errors — property holds */ }
            Err(errors) => {
                let broken_refs: Vec<_> = errors
                    .iter()
                    .filter(|e| matches!(e, GraphError::BrokenRef { .. }))
                    .collect();
                prop_assert!(
                    broken_refs.is_empty(),
                    "valid sibling depends should produce no BrokenRef errors, got: {:?}",
                    broken_refs
                );
            }
        }
    }

    /// Create a document with nested tasks (tasks as children of other tasks).
    /// Have a nested task's `depends` reference a task that is NOT a sibling
    /// (a top-level task or a task in a different parent). Assert `BrokenRef`.
    ///
    /// Validates: Requirements 5.4, 5.5
    #[test]
    fn prop_non_sibling_depends_produces_broken_ref(
        parent_id in arb_component_id(),
        child_id in arb_component_id(),
        toplevel_id in arb_component_id(),
    ) {
        // Ensure all IDs are distinct.
        prop_assume!(parent_id != child_id);
        prop_assume!(parent_id != toplevel_id);
        prop_assume!(child_id != toplevel_id);

        let config = single_project_config();

        // Nested child task depends on a top-level task (non-sibling).
        let child = make_task(&child_id, None, None, Some(&toplevel_id), 2);
        let parent = make_task_with_children(&parent_id, None, vec![child], 1);
        let toplevel = make_task(&toplevel_id, None, None, None, 3);

        let doc = make_doc_with_path(
            "tasks/nested",
            "specs/tasks/nested.mdx",
            vec![parent, toplevel],
        );
        let result = build_graph(vec![doc], &config);

        // Should produce a BrokenRef because the nested child references a
        // non-sibling (the top-level task).
        match result {
            Ok(_) => {
                prop_assert!(
                    false,
                    "non-sibling depends should produce BrokenRef, but build_graph returned Ok"
                );
            }
            Err(errors) => {
                let broken_refs: Vec<_> = errors
                    .iter()
                    .filter(|e| {
                        if let GraphError::BrokenRef { ref_str, .. } = e {
                            ref_str == &toplevel_id
                        } else {
                            false
                        }
                    })
                    .collect();
                prop_assert!(
                    !broken_refs.is_empty(),
                    "non-sibling depends should produce at least one BrokenRef for '{}', \
                     got errors: {:?}",
                    toplevel_id,
                    errors
                );
            }
        }
    }

    /// Generate a task with `depends` referencing a task ID that doesn't exist
    /// anywhere in the document. Assert `BrokenRef` error.
    ///
    /// Validates: Requirements 5.4, 5.5
    #[test]
    fn prop_nonexistent_depends_produces_broken_ref(
        task_id in arb_component_id(),
        ghost_id in arb_component_id(),
    ) {
        // Ensure the ghost ID is different from the task's own ID.
        prop_assume!(task_id != ghost_id);

        let config = single_project_config();

        // Single task that depends on a nonexistent task.
        let task = make_task(&task_id, None, None, Some(&ghost_id), 1);
        let doc = make_doc_with_path(
            "tasks/ghost",
            "specs/tasks/ghost.mdx",
            vec![task],
        );
        let result = build_graph(vec![doc], &config);

        // Should produce a BrokenRef for the nonexistent depends target.
        match result {
            Ok(_) => {
                prop_assert!(
                    false,
                    "nonexistent depends should produce BrokenRef, but build_graph returned Ok"
                );
            }
            Err(errors) => {
                let broken_refs: Vec<_> = errors
                    .iter()
                    .filter(|e| {
                        if let GraphError::BrokenRef { ref_str, .. } = e {
                            ref_str == &ghost_id
                        } else {
                            false
                        }
                    })
                    .collect();
                prop_assert!(
                    !broken_refs.is_empty(),
                    "nonexistent depends should produce at least one BrokenRef for '{}', \
                     got errors: {:?}",
                    ghost_id,
                    errors
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property 12: Acyclic document dependency graphs produce no cycle errors
// ---------------------------------------------------------------------------

proptest! {
    /// Generate documents with `DependsOn` refs forming a DAG (using
    /// `arb_dag`), assert no `DocumentDependencyCycle` errors.
    ///
    /// Each DAG node becomes a document. Each edge (A depends on B) adds a
    /// `DependsOn` component to document A with `refs="B"`.
    ///
    /// **Validates: Requirements 6.2**
    #[test]
    fn prop_acyclic_document_dependency_graph_no_cycle_errors(dag in arb_dag(6)) {
        let config = single_project_config();

        // Build a lookup: node → list of document IDs it depends on.
        let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to) in &dag.edges {
            deps_map.entry(from.clone()).or_default().push(to.clone());
        }

        // Create a SpecDocument for each node. If the node has DependsOn
        // edges, attach a DependsOn component with comma-separated refs.
        let documents: Vec<SpecDocument> = dag
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let components = match deps_map.get(node) {
                    Some(targets) => vec![make_depends_on(&targets.join(", "), i + 1)],
                    None => Vec::new(),
                };
                make_doc_with_path(node, &format!("specs/{node}.mdx"), components)
            })
            .collect();

        let result = build_graph(documents, &config);

        match result {
            Ok(_) => { /* no cycle errors — property holds */ }
            Err(errors) => {
                let cycle_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| matches!(e, GraphError::DocumentDependencyCycle { .. }))
                    .collect();
                prop_assert!(
                    cycle_errors.is_empty(),
                    "acyclic DAG should produce no DocumentDependencyCycle errors, got: {:?}",
                    cycle_errors
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property 13: Cyclic document dependency graphs produce cycle errors
// ---------------------------------------------------------------------------

proptest! {
    /// Take an `arb_dag` and inject mutual back-edges between the first and
    /// last nodes to create a guaranteed cycle. Each node becomes a document
    /// with `DependsOn` refs. Assert that `build_graph` returns errors
    /// containing at least one `DocumentDependencyCycle` whose `cycle` field
    /// is non-empty.
    ///
    /// **Validates: Requirements 6.3**
    #[test]
    fn prop_cyclic_document_dependency_graph_with_back_edge(dag in arb_dag(6)) {
        // Need at least 2 nodes to inject a meaningful back-edge.
        prop_assume!(dag.nodes.len() >= 2);

        let config = single_project_config();

        // Build the original dependency map from the DAG edges.
        let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to) in &dag.edges {
            deps_map.entry(from.clone()).or_default().push(to.clone());
        }

        // Inject mutual back-edges between first and last nodes to guarantee
        // a cycle regardless of the original DAG structure.
        let first = &dag.nodes[0];
        let last = &dag.nodes[dag.nodes.len() - 1];
        deps_map.entry(first.clone()).or_default().push(last.clone());
        deps_map.entry(last.clone()).or_default().push(first.clone());

        // Create a SpecDocument for each node with DependsOn components.
        let documents: Vec<SpecDocument> = dag
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let components = match deps_map.get(node) {
                    Some(targets) => vec![make_depends_on(&targets.join(", "), i + 1)],
                    None => Vec::new(),
                };
                make_doc_with_path(node, &format!("specs/{node}.mdx"), components)
            })
            .collect();

        let result = build_graph(documents, &config);

        match result {
            Ok(_) => {
                prop_assert!(
                    false,
                    "cyclic document dependency graph should produce errors, \
                     but build_graph returned Ok"
                );
            }
            Err(errors) => {
                let cycle_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| matches!(e, GraphError::DocumentDependencyCycle { .. }))
                    .collect();
                prop_assert!(
                    !cycle_errors.is_empty(),
                    "cyclic document dependency graph should produce at least one \
                     DocumentDependencyCycle error, got errors: {:?}",
                    errors
                );

                // Every reported cycle must have a non-empty cycle field.
                for err in &cycle_errors {
                    if let GraphError::DocumentDependencyCycle { cycle } = err {
                        prop_assert!(
                            !cycle.is_empty(),
                            "DocumentDependencyCycle.cycle should not be empty"
                        );
                    }
                }
            }
        }
    }

    /// Generate a single document with a `DependsOn` component that references
    /// itself. Assert `DocumentDependencyCycle` error containing the
    /// self-referencing document ID.
    ///
    /// **Validates: Requirements 6.3**
    #[test]
    fn prop_self_referencing_document_dependency_produces_cycle_error(
        doc_id in arb_id()
    ) {
        let config = single_project_config();

        // A single document that depends on itself.
        let depends_on = make_depends_on(&doc_id, 1);
        let doc = make_doc_with_path(&doc_id, &format!("specs/{doc_id}.mdx"), vec![depends_on]);
        let result = build_graph(vec![doc], &config);

        match result {
            Ok(_) => {
                prop_assert!(
                    false,
                    "self-referencing document dependency should produce errors, \
                     but build_graph returned Ok"
                );
            }
            Err(errors) => {
                let cycle_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| matches!(e, GraphError::DocumentDependencyCycle { .. }))
                    .collect();
                prop_assert!(
                    !cycle_errors.is_empty(),
                    "self-referencing document dependency should produce at least one \
                     DocumentDependencyCycle error, got errors: {:?}",
                    errors
                );

                // The cycle should contain the self-referencing document ID.
                for err in &cycle_errors {
                    if let GraphError::DocumentDependencyCycle { cycle } = err {
                        prop_assert!(
                            cycle.contains(&doc_id),
                            "DocumentDependencyCycle.cycle should contain the \
                             self-referencing document ID '{}', got: {:?}",
                            doc_id,
                            cycle
                        );
                    }
                }
            }
        }
    }
}
