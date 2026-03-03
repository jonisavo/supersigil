//! Topological sort with tiebreaking (pipeline stage 7).
//!
//! Implements Kahn's algorithm with deterministic tiebreaking:
//! - Tasks: declaration order (index in the source document's component list)
//! - Documents: alphabetical by document ID

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::{ExtractedComponent, SpecDocument, split_list_attribute};

use super::TASK;

use super::ResolvedRef;

// ---------------------------------------------------------------------------
// Stage 7a: Task topological sort (per document)
// ---------------------------------------------------------------------------

/// Compute topological orderings for task dependencies within each document.
///
/// Returns a map from document ID to the ordered list of task IDs.
/// Only documents containing `Task` components produce entries.
///
/// Precondition: cycle detection (stage 5) has already verified that all
/// task dependency graphs are acyclic.
pub(super) fn compute_task_topo_orders(
    doc_index: &HashMap<String, SpecDocument>,
) -> HashMap<String, Vec<String>> {
    let mut result = HashMap::new();

    for (doc_id, doc) in doc_index {
        let order = topo_sort_tasks_at_level(&doc.components);
        if !order.is_empty() {
            result.insert(doc_id.clone(), order);
        }
    }

    result
}

/// Topologically sort tasks at one nesting level, then recurse into children.
/// Returns all task IDs in topological order (flattened across nesting levels,
/// with parent-level tasks first, then children within each parent).
fn topo_sort_tasks_at_level(components: &[ExtractedComponent]) -> Vec<String> {
    // Collect sibling tasks at this level with their declaration index.
    let mut task_decl_order: BTreeMap<String, usize> = BTreeMap::new();
    let mut sibling_ids: Vec<String> = Vec::new();

    for (idx, comp) in components.iter().enumerate() {
        if comp.name == TASK
            && let Some(id) = comp.attributes.get("id")
        {
            task_decl_order.insert(id.clone(), idx);
            sibling_ids.push(id.clone());
        }
    }

    let mut order = Vec::new();

    if !sibling_ids.is_empty() {
        // Build adjacency list and in-degree map.
        // Edge direction: for "A depends on B", edge B → A (B is needed by A).
        // This means A has in-degree incremented for each dependency.
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        let mut in_degree: HashMap<String, usize> = HashMap::new();

        for id in &sibling_ids {
            adj.entry(id.clone()).or_default();
            in_degree.entry(id.clone()).or_insert(0);
        }

        for comp in components {
            if comp.name != TASK {
                continue;
            }
            let Some(task_id) = comp.attributes.get("id") else {
                continue;
            };
            let Some(depends_raw) = comp.attributes.get("depends") else {
                continue;
            };
            if let Ok(dep_ids) = split_list_attribute(depends_raw) {
                for dep_id in dep_ids {
                    // Only consider valid sibling refs (broken refs already
                    // reported by cycle detection stage).
                    if task_decl_order.contains_key(dep_id) {
                        adj.entry(dep_id.to_owned())
                            .or_default()
                            .push(task_id.clone());
                        *in_degree.entry(task_id.clone()).or_insert(0) += 1;
                    }
                }
            }
        }

        // Kahn's algorithm with declaration-order tiebreaking.
        // Use a BTreeSet keyed by (declaration_index, task_id) for deterministic
        // ordering — tasks with lower declaration index are processed first.
        let mut queue: BTreeSet<(usize, String)> = BTreeSet::new();
        for (id, &deg) in &in_degree {
            if deg == 0 {
                let decl_idx = task_decl_order[id];
                queue.insert((decl_idx, id.clone()));
            }
        }

        while let Some((_, node)) = queue.pop_first() {
            order.push(node.clone());
            if let Some(neighbors) = adj.get(&node) {
                for neighbor in neighbors {
                    let deg = in_degree
                        .get_mut(neighbor)
                        .expect("all nodes pre-inserted in in_degree map");
                    *deg -= 1;
                    if *deg == 0 {
                        let decl_idx = task_decl_order[neighbor];
                        queue.insert((decl_idx, neighbor.clone()));
                    }
                }
            }
        }
    }

    // Recurse into children of each task (in declaration order).
    for comp in components {
        if comp.name == TASK && !comp.children.is_empty() {
            let child_order = topo_sort_tasks_at_level(&comp.children);
            order.extend(child_order);
        }
    }

    order
}

// ---------------------------------------------------------------------------
// Stage 7b: Document topological sort
// ---------------------------------------------------------------------------

/// Compute the topological ordering of documents based on `DependsOn` edges.
///
/// Returns all document IDs in topological order. Documents with no
/// `DependsOn` edges are ordered alphabetically among themselves.
///
/// Precondition: cycle detection (stage 6) has already verified that the
/// document dependency graph is acyclic.
pub(super) fn compute_doc_topo_order(
    resolved_refs: &HashMap<(String, Vec<usize>), Vec<ResolvedRef>>,
    doc_index: &HashMap<String, SpecDocument>,
) -> Vec<String> {
    // Build adjacency list and in-degree map.
    // Edge direction: for "A DependsOn B", edge B → A (B must come first).
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_degree: HashMap<String, usize> = HashMap::new();

    for doc_id in doc_index.keys() {
        adj.entry(doc_id.clone()).or_default();
        in_degree.entry(doc_id.clone()).or_insert(0);
    }

    for (source, target) in super::collect_doc_dependency_edges(doc_index, resolved_refs) {
        adj.entry(target.to_owned())
            .or_default()
            .push(source.to_owned());
        *in_degree.entry(source.to_owned()).or_insert(0) += 1;
    }

    // Kahn's algorithm with alphabetical tiebreaking.
    // BTreeSet keyed by doc_id gives us alphabetical ordering for free.
    let mut queue: BTreeSet<String> = BTreeSet::new();
    for (id, &deg) in &in_degree {
        if deg == 0 {
            queue.insert(id.clone());
        }
    }

    let mut order = Vec::with_capacity(doc_index.len());
    while let Some(node) = queue.pop_first() {
        order.push(node.clone());
        if let Some(neighbors) = adj.get(&node) {
            for neighbor in neighbors {
                let deg = in_degree
                    .get_mut(neighbor)
                    .expect("all nodes pre-inserted in in_degree map");
                *deg -= 1;
                if *deg == 0 {
                    queue.insert(neighbor.clone());
                }
            }
        }
    }

    order
}
