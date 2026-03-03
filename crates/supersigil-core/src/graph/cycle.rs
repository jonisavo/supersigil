//! Cycle detection for task and document dependency graphs (pipeline stages 5–6).

use std::collections::{HashMap, HashSet};

use crate::{ExtractedComponent, SpecDocument, split_list_attribute};

use super::TASK;

use super::ResolvedRef;
use super::error::GraphError;

// ---------------------------------------------------------------------------
// DFS coloring
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    White,
    Gray,
    Black,
}

// ---------------------------------------------------------------------------
// Generic DFS cycle detection
// ---------------------------------------------------------------------------

/// Run DFS-based cycle detection on a directed graph represented as an
/// adjacency list. Returns all unique cycles found, each normalized so the
/// lexicographically smallest node appears first (for deduplication).
fn detect_cycles_dfs(graph: &HashMap<String, Vec<String>>) -> Vec<Vec<String>> {
    let mut colors: HashMap<String, Color> = HashMap::new();
    for key in graph.keys() {
        colors.insert(key.clone(), Color::White);
        for target in &graph[key] {
            colors.entry(target.clone()).or_insert(Color::White);
        }
    }

    let mut path: Vec<String> = Vec::new();
    let mut raw_cycles: Vec<Vec<String>> = Vec::new();

    // Iterate in sorted order for determinism.
    let mut nodes: Vec<String> = colors.keys().cloned().collect();
    nodes.sort_unstable();

    for node in &nodes {
        if colors[node.as_str()] == Color::White {
            dfs(node, graph, &mut colors, &mut path, &mut raw_cycles);
        }
    }

    // Deduplicate by normalizing each cycle.
    let mut seen: HashSet<Vec<String>> = HashSet::new();
    let mut unique_cycles = Vec::new();
    for cycle in raw_cycles {
        let normalized = normalize_cycle(&cycle);
        if seen.insert(normalized.clone()) {
            unique_cycles.push(normalized);
        }
    }

    unique_cycles
}

fn dfs(
    node: &str,
    graph: &HashMap<String, Vec<String>>,
    colors: &mut HashMap<String, Color>,
    path: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    colors.insert(node.to_owned(), Color::Gray);
    path.push(node.to_owned());

    if let Some(neighbors) = graph.get(node) {
        for neighbor in neighbors {
            match colors.get(neighbor.as_str()) {
                Some(Color::Gray) => {
                    // Back edge found — extract cycle from path.
                    if let Some(start) = path.iter().position(|n| n == neighbor) {
                        let cycle: Vec<String> = path[start..].to_vec();
                        cycles.push(cycle);
                    }
                }
                Some(Color::White) => {
                    dfs(neighbor, graph, colors, path, cycles);
                }
                _ => { /* Black — already fully explored, skip */ }
            }
        }
    }

    colors.insert(node.to_owned(), Color::Black);
    path.pop();
}

/// Normalize a cycle by rotating it so the lexicographically smallest node
/// appears first. This allows deduplication of equivalent cycles discovered
/// via different back edges.
fn normalize_cycle(cycle: &[String]) -> Vec<String> {
    if cycle.is_empty() {
        return Vec::new();
    }
    let min_pos = cycle
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.cmp(b))
        .map_or(0, |(i, _)| i);
    let mut normalized = Vec::with_capacity(cycle.len());
    normalized.extend_from_slice(&cycle[min_pos..]);
    normalized.extend_from_slice(&cycle[..min_pos]);
    normalized
}

// ---------------------------------------------------------------------------
// Stage 5: Task dependency cycle detection
// ---------------------------------------------------------------------------

/// Detect cycles in task dependency graphs within each document.
///
/// For each document, collects `Task` components at each nesting level,
/// builds a directed graph from `depends` attributes (scoped to siblings),
/// and runs DFS cycle detection.
pub(super) fn detect_task_cycles(doc_index: &HashMap<String, SpecDocument>) -> Vec<GraphError> {
    let mut errors = Vec::new();

    for (doc_id, doc) in doc_index {
        detect_task_cycles_at_level(doc_id, &doc.components, &mut errors);
    }

    errors
}

/// Process one nesting level of tasks: build the sibling graph, detect cycles,
/// then recurse into children.
fn detect_task_cycles_at_level(
    doc_id: &str,
    components: &[ExtractedComponent],
    errors: &mut Vec<GraphError>,
) {
    // Collect sibling task IDs at this level.
    let sibling_tasks: HashMap<&str, &ExtractedComponent> = components
        .iter()
        .filter(|c| c.name == TASK)
        .filter_map(|c| c.attributes.get("id").map(|id| (id.as_str(), c)))
        .collect();

    if !sibling_tasks.is_empty() {
        // Build adjacency list: for "A depends on B", add edge A → B.
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();

        // Ensure every task node exists in the graph (even with no edges).
        for &task_id in sibling_tasks.keys() {
            graph.entry(task_id.to_owned()).or_default();
        }

        for (&task_id, &task) in &sibling_tasks {
            if let Some(depends_raw) = task.attributes.get("depends") {
                match split_list_attribute(depends_raw) {
                    Ok(dep_ids) => {
                        for dep_id in dep_ids {
                            if sibling_tasks.contains_key(dep_id) {
                                graph
                                    .entry(task_id.to_owned())
                                    .or_default()
                                    .push(dep_id.to_owned());
                            } else {
                                errors.push(GraphError::BrokenRef {
                                    doc_id: doc_id.to_owned(),
                                    ref_str: dep_id.to_owned(),
                                    reason: format!(
                                        "task `{dep_id}` not found among sibling tasks"
                                    ),
                                    position: task.position,
                                });
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(GraphError::BrokenRef {
                            doc_id: doc_id.to_owned(),
                            ref_str: e.raw.clone(),
                            reason: e.to_string(),
                            position: task.position,
                        });
                    }
                }
            }
        }

        // Run cycle detection on this level's graph.
        let cycles = detect_cycles_dfs(&graph);
        for cycle in cycles {
            errors.push(GraphError::TaskDependencyCycle {
                doc_id: doc_id.to_owned(),
                cycle,
            });
        }
    }

    // Recurse into children of each task at this level.
    for comp in components {
        if comp.name == TASK && !comp.children.is_empty() {
            detect_task_cycles_at_level(doc_id, &comp.children, errors);
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 6: Document dependency cycle detection
// ---------------------------------------------------------------------------

/// Detect cycles in the document-level dependency graph built from `DependsOn`
/// resolved refs.
pub(super) fn detect_document_cycles(
    resolved_refs: &HashMap<(String, Vec<usize>), Vec<ResolvedRef>>,
    doc_index: &HashMap<String, SpecDocument>,
) -> Vec<GraphError> {
    // Build adjacency list: for "A DependsOn B", add edge A → B.
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();

    for doc_id in doc_index.keys() {
        graph.entry(doc_id.clone()).or_default();
    }

    for (source, target) in super::collect_doc_dependency_edges(doc_index, resolved_refs) {
        graph
            .entry(source.to_owned())
            .or_default()
            .push(target.to_owned());
    }

    detect_cycles_dfs(&graph)
        .into_iter()
        .map(|cycle| GraphError::DocumentDependencyCycle { cycle })
        .collect()
}
