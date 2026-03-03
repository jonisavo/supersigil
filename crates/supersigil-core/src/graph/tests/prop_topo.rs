//! Property tests for topological sort (pipeline stage 7).
//!
//! Property 14: Topological order invariant
//! Property 15: Topological sort determinism
//! Validates: Requirements 7.1, 7.2, 7.3, 7.4

use std::collections::HashMap;

use proptest::prelude::*;

use crate::graph::tests::generators::{
    arb_dag, make_depends_on, make_doc_with_path, make_task, single_project_config,
};
use crate::graph::{GraphError, build_graph};
use crate::{ExtractedComponent, SpecDocument};

// ---------------------------------------------------------------------------
// Property 14: Topological order invariant
// ---------------------------------------------------------------------------

proptest! {
    /// Generate valid task DAGs, compute topo order, assert for every edge
    /// (A depends on B) that B appears before A in the ordering.
    ///
    /// Validates: Requirements 7.1, 7.2, 7.3
    #[test]
    fn prop_task_topo_order_invariant(dag in arb_dag(6)) {
        let config = single_project_config();

        // Build dependency map: node → list of nodes it depends on.
        let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to) in &dag.edges {
            deps_map.entry(from.clone()).or_default().push(to.clone());
        }

        // Create Task components for each node.
        let tasks: Vec<ExtractedComponent> = dag
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let depends = deps_map.get(node).map(|deps| deps.join(", "));
                make_task(node, None, None, depends.as_deref(), i + 1)
            })
            .collect();

        let doc_id = "tasks/topo-test";
        let doc = make_doc_with_path(doc_id, "specs/tasks/topo-test.mdx", tasks);
        let result = build_graph(vec![doc], &config);

        match result {
            Ok(graph) => {
                let order = graph.task_order(doc_id);
                prop_assert!(
                    order.is_some(),
                    "task_order should return Some for a tasks document"
                );
                let order = order.unwrap();

                // Build position map: task_id → index in topo order.
                let pos: HashMap<&str, usize> = order
                    .iter()
                    .enumerate()
                    .map(|(i, id)| (id.as_str(), i))
                    .collect();

                // For every edge (A depends on B), B must appear before A.
                for (from, to) in &dag.edges {
                    let from_pos = pos.get(from.as_str());
                    let to_pos = pos.get(to.as_str());
                    prop_assert!(
                        from_pos.is_some() && to_pos.is_some(),
                        "both nodes should be in topo order: from={from}, to={to}"
                    );
                    prop_assert!(
                        to_pos.unwrap() < from_pos.unwrap(),
                        "dependency {to} (pos {}) should appear before {from} (pos {})",
                        to_pos.unwrap(),
                        from_pos.unwrap()
                    );
                }
            }
            Err(errors) => {
                // Filter out non-cycle errors — DAGs should not produce cycles.
                let cycle_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| matches!(e, GraphError::TaskDependencyCycle { .. }))
                    .collect();
                prop_assert!(
                    cycle_errors.is_empty(),
                    "acyclic DAG should produce no cycle errors, got: {cycle_errors:?}"
                );
            }
        }
    }

    /// Generate valid document DAGs via DependsOn refs, compute topo order,
    /// assert for every edge (A depends on B) that B appears before A.
    ///
    /// Validates: Requirements 7.1, 7.2, 7.3
    #[test]
    fn prop_document_topo_order_invariant(dag in arb_dag(6)) {
        let config = single_project_config();

        // Build dependency map: node → list of doc IDs it depends on.
        let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to) in &dag.edges {
            deps_map.entry(from.clone()).or_default().push(to.clone());
        }

        // Create a SpecDocument for each node.
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
            Ok(graph) => {
                let order = graph.doc_order();

                // Build position map: doc_id → index in topo order.
                let pos: HashMap<&str, usize> = order
                    .iter()
                    .enumerate()
                    .map(|(i, id)| (id.as_str(), i))
                    .collect();

                // For every edge (A depends on B), B must appear before A.
                for (from, to) in &dag.edges {
                    let from_pos = pos.get(from.as_str());
                    let to_pos = pos.get(to.as_str());
                    prop_assert!(
                        from_pos.is_some() && to_pos.is_some(),
                        "both docs should be in topo order: from={from}, to={to}"
                    );
                    prop_assert!(
                        to_pos.unwrap() < from_pos.unwrap(),
                        "dependency {to} (pos {}) should appear before {from} (pos {})",
                        to_pos.unwrap(),
                        from_pos.unwrap()
                    );
                }
            }
            Err(errors) => {
                let cycle_errors: Vec<_> = errors
                    .iter()
                    .filter(|e| matches!(e, GraphError::DocumentDependencyCycle { .. }))
                    .collect();
                prop_assert!(
                    cycle_errors.is_empty(),
                    "acyclic DAG should produce no cycle errors, got: {cycle_errors:?}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property 15: Topological sort determinism
// ---------------------------------------------------------------------------

proptest! {
    /// Generate valid task DAGs, sort twice on identical input, assert
    /// identical output. The tiebreaker is declaration order (index in the
    /// source document's component list).
    ///
    /// Validates: Requirements 7.4
    #[test]
    fn prop_task_topo_sort_determinism(dag in arb_dag(6)) {
        let config = single_project_config();

        let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to) in &dag.edges {
            deps_map.entry(from.clone()).or_default().push(to.clone());
        }

        let tasks: Vec<ExtractedComponent> = dag
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let depends = deps_map.get(node).map(|deps| deps.join(", "));
                make_task(node, None, None, depends.as_deref(), i + 1)
            })
            .collect();

        let doc_id = "tasks/determinism";
        let doc1 = make_doc_with_path(doc_id, "specs/tasks/determinism.mdx", tasks.clone());
        let doc2 = make_doc_with_path(doc_id, "specs/tasks/determinism.mdx", tasks);

        let result1 = build_graph(vec![doc1], &config);
        let result2 = build_graph(vec![doc2], &config);

        match (result1, result2) {
            (Ok(g1), Ok(g2)) => {
                let order1 = g1.task_order(doc_id);
                let order2 = g2.task_order(doc_id);
                prop_assert_eq!(
                    order1, order2,
                    "task topo sort should be deterministic across identical inputs"
                );
            }
            (Err(_), Err(_)) => {
                // Both errored — consistent behavior, property holds.
            }
            (r1, r2) => {
                prop_assert!(
                    false,
                    "inconsistent results: first={r1:?}, second={r2:?}"
                );
            }
        }
    }

    /// Generate valid document DAGs, sort twice on identical input, assert
    /// identical output. The tiebreaker is alphabetical by document ID.
    ///
    /// Validates: Requirements 7.4
    #[test]
    fn prop_document_topo_sort_determinism(dag in arb_dag(6)) {
        let config = single_project_config();

        let mut deps_map: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to) in &dag.edges {
            deps_map.entry(from.clone()).or_default().push(to.clone());
        }

        let make_docs = || -> Vec<SpecDocument> {
            dag.nodes
                .iter()
                .enumerate()
                .map(|(i, node)| {
                    let components = match deps_map.get(node) {
                        Some(targets) => vec![make_depends_on(&targets.join(", "), i + 1)],
                        None => Vec::new(),
                    };
                    make_doc_with_path(node, &format!("specs/{node}.mdx"), components)
                })
                .collect()
        };

        let result1 = build_graph(make_docs(), &config);
        let result2 = build_graph(make_docs(), &config);

        match (result1, result2) {
            (Ok(g1), Ok(g2)) => {
                prop_assert_eq!(
                    g1.doc_order(),
                    g2.doc_order(),
                    "document topo sort should be deterministic across identical inputs"
                );
            }
            (Err(_), Err(_)) => {
                // Both errored — consistent behavior, property holds.
            }
            (r1, r2) => {
                prop_assert!(
                    false,
                    "inconsistent results: first={r1:?}, second={r2:?}"
                );
            }
        }
    }

    /// Verify that when multiple valid orderings exist (no edges between
    /// certain nodes), the tiebreaker for tasks is declaration order.
    ///
    /// Validates: Requirements 7.4
    #[test]
    fn prop_task_tiebreaker_is_declaration_order(n in 2..8usize) {
        let config = single_project_config();

        // Create n independent tasks (no depends edges). The topo sort
        // tiebreaker should produce them in declaration order.
        let tasks: Vec<ExtractedComponent> = (0..n)
            .map(|i| make_task(&format!("t{i}"), None, None, None, i + 1))
            .collect();

        let doc_id = "tasks/tiebreak";
        let doc = make_doc_with_path(doc_id, "specs/tasks/tiebreak.mdx", tasks);
        let result = build_graph(vec![doc], &config);

        match result {
            Ok(graph) => {
                let order = graph.task_order(doc_id);
                prop_assert!(order.is_some());
                let order = order.unwrap();

                // With no edges, declaration order should be preserved.
                let expected: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
                prop_assert_eq!(
                    order,
                    expected.as_slice(),
                    "independent tasks should be ordered by declaration order"
                );
            }
            Err(errors) => {
                prop_assert!(false, "should not error on independent tasks: {errors:?}");
            }
        }
    }

    /// Verify that when multiple valid orderings exist for documents,
    /// the tiebreaker is alphabetical by document ID.
    ///
    /// Validates: Requirements 7.4
    #[test]
    fn prop_document_tiebreaker_is_alphabetical(n in 2..8usize) {
        let config = single_project_config();

        // Create n independent documents (no DependsOn edges).
        let mut ids: Vec<String> = (0..n).map(|i| format!("doc-{i:03}")).collect();
        let documents: Vec<SpecDocument> = ids
            .iter()
            .map(|id| make_doc_with_path(id, &format!("specs/{id}.mdx"), Vec::new()))
            .collect();

        let result = build_graph(documents, &config);

        match result {
            Ok(graph) => {
                let order = graph.doc_order();
                ids.sort_unstable();
                prop_assert_eq!(
                    order,
                    ids.as_slice(),
                    "independent documents should be ordered alphabetically"
                );
            }
            Err(errors) => {
                prop_assert!(false, "should not error on independent docs: {errors:?}");
            }
        }
    }
}
