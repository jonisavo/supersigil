//! Structured query outputs for `context` and `plan` commands.

use std::collections::HashSet;

use super::{ALTERNATIVE, CRITERION, DECISION, DocumentGraph, RATIONALE, REFERENCES, TASK};
use crate::{ExtractedComponent, split_list_attribute};

// ---------------------------------------------------------------------------
// QueryError
// ---------------------------------------------------------------------------

/// Errors returned by query methods on a successfully built graph.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("document `{id}` not found")]
    DocumentNotFound { id: String },
    #[error("no documents match query `{query}`")]
    NoMatchingDocuments { query: String },
}

// ---------------------------------------------------------------------------
// ContextOutput
// ---------------------------------------------------------------------------

/// Structured output for the `context` command.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ContextOutput {
    /// The target document.
    pub document: crate::SpecDocument,
    /// Verification targets (criteria) with their reference status.
    pub criteria: Vec<TargetContext>,
    /// Decisions with rationale and alternatives.
    pub decisions: Vec<DecisionContext>,
    /// Decisions from other documents whose nested References target this document.
    pub linked_decisions: Vec<LinkedDecision>,
    /// Documents that implement this document.
    pub implemented_by: Vec<DocRef>,
    /// Documents that reference this document (document-level).
    pub referenced_by: Vec<String>,
    /// Tasks from linked tasks documents, in topological order.
    pub tasks: Vec<TaskInfo>,
}

/// A verification target (criterion) with its incoming reference relationships.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TargetContext {
    pub id: String,
    /// Canonical target reference in `document-id#criterion-id` format.
    pub target_ref: String,
    pub body_text: Option<String>,
    /// Documents that reference this criterion, with their status.
    pub referenced_by: Vec<DocRef>,
}

/// A reference to a document with its optional status.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DocRef {
    pub doc_id: String,
    pub status: Option<String>,
}

/// A decision with its rationale and alternatives, extracted from the component tree.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecisionContext {
    pub id: String,
    pub body_text: Option<String>,
    pub rationale_text: Option<String>,
    pub alternatives: Vec<AlternativeContext>,
}

/// An alternative within a decision.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AlternativeContext {
    pub id: String,
    pub status: String,
    pub body_text: Option<String>,
}

/// A decision from another document that references the current document.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LinkedDecision {
    pub source_doc_id: String,
    pub decision_id: String,
    pub body_text: Option<String>,
}

/// Serialize `None` as `"pending"` so JSON consumers see an explicit status.
#[expect(
    clippy::ref_option,
    reason = "serde serialize_with requires &Option<T>"
)]
fn serialize_status_pending<S: serde::Serializer>(
    status: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match status {
        Some(s) => serializer.serialize_str(s),
        None => serializer.serialize_str("pending"),
    }
}

/// A task with dependency and implements info, used in both context and plan output.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TaskInfo {
    /// Which tasks document this task belongs to.
    pub tasks_doc_id: String,
    pub task_id: String,
    #[serde(serialize_with = "serialize_status_pending")]
    pub status: Option<String>,
    pub body_text: Option<String>,
    /// Verification targets this task implements: `(doc_id, target_id)`.
    pub implements: Vec<(String, String)>,
    /// Task IDs this task depends on.
    pub depends_on: Vec<String>,
}

// ---------------------------------------------------------------------------
// PlanOutput
// ---------------------------------------------------------------------------

/// Structured output for the `plan` command.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PlanOutput {
    /// Verification targets with no evidence.
    pub outstanding_targets: Vec<OutstandingTarget>,
    /// Tasks not yet done, in topological order, grouped by tasks document.
    pub pending_tasks: Vec<TaskInfo>,
    /// Completed tasks with the criteria they implement.
    pub completed_tasks: Vec<TaskInfo>,
    /// Pending tasks with no unfinished dependencies (task IDs).
    pub actionable_tasks: Vec<String>,
    /// Pending tasks blocked by unfinished dependencies (task IDs).
    pub blocked_tasks: Vec<String>,
}

/// A verification target that has no verification evidence.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct OutstandingTarget {
    /// The document containing this verification target.
    pub doc_id: String,
    pub target_id: String,
    pub body_text: Option<String>,
}

// ---------------------------------------------------------------------------
// PlanQuery
// ---------------------------------------------------------------------------

/// Input type for the `plan` method, supporting three query modes.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanQuery {
    /// Plan for a single document by exact ID.
    Document(String),
    /// Plan for all documents matching a prefix (e.g., `"auth/"`).
    Prefix(String),
    /// Project-wide plan covering all documents.
    All,
}

impl PlanQuery {
    /// Parse a user-provided string into a `PlanQuery`.
    ///
    /// Disambiguation logic:
    /// 1. `None` or empty → `PlanQuery::All`
    /// 2. Exact document ID match → `PlanQuery::Document`
    /// 3. Prefix match → `PlanQuery::Prefix`
    /// 4. Otherwise → `QueryError::NoMatchingDocuments`
    ///
    /// # Errors
    ///
    /// Returns `QueryError::NoMatchingDocuments` if the input matches no
    /// document ID exactly and no document ID by prefix.
    pub fn parse(input: Option<&str>, graph: &DocumentGraph) -> Result<Self, QueryError> {
        let input = match input {
            None | Some("") => return Ok(Self::All),
            Some(s) => s,
        };

        // Exact match
        if graph.document(input).is_some() {
            return Ok(Self::Document(input.to_owned()));
        }

        // Prefix match
        let has_prefix_match = graph.documents().any(|(id, _)| id.starts_with(input));
        if has_prefix_match {
            return Ok(Self::Prefix(input.to_owned()));
        }

        Err(QueryError::NoMatchingDocuments {
            query: input.to_owned(),
        })
    }
}

// ---------------------------------------------------------------------------
// Context query implementation
// ---------------------------------------------------------------------------

/// Build a `ContextOutput` for the given document ID.
pub(super) fn build_context(graph: &DocumentGraph, id: &str) -> Result<ContextOutput, QueryError> {
    let document = graph
        .document(id)
        .ok_or_else(|| QueryError::DocumentNotFound { id: id.to_owned() })?
        .clone();

    // Extract criteria from the document's components (Criterion lives inside
    // AcceptanceCriteria children).
    let criteria = extract_criteria(graph, id, &document.components);

    // Extract decisions from the document's components.
    let decisions = extract_decisions(&document.components);

    // Implementing documents.
    let implemented_by = graph
        .implements(id)
        .iter()
        .map(|doc_id| DocRef {
            doc_id: doc_id.clone(),
            status: graph
                .document(doc_id)
                .and_then(|d| d.frontmatter.status.clone()),
        })
        .collect();

    // Document-level references.
    let referenced_by: Vec<String> = graph.references(id, None).iter().cloned().collect();

    // Linked decisions: scan referencing docs for Decision components whose
    // nested References target the current document.
    let linked_decisions = extract_linked_decisions(graph, id, &referenced_by);

    // Linked tasks: find all tasks documents that have tasks implementing
    // criteria in this document, then collect those tasks in topo order.
    let tasks = collect_linked_tasks(graph, id);

    Ok(ContextOutput {
        document,
        criteria,
        decisions,
        linked_decisions,
        implemented_by,
        referenced_by,
        tasks,
    })
}

/// Recursively extract `Criterion` components from a component tree,
/// building `TargetContext` with reverse mapping lookups.
fn extract_criteria(
    graph: &DocumentGraph,
    doc_id: &str,
    components: &[ExtractedComponent],
) -> Vec<TargetContext> {
    let mut result = Vec::new();
    for comp in components {
        if comp.name == CRITERION
            && let Some(crit_id) = comp.attributes.get("id")
        {
            let referenced_by = graph
                .references(doc_id, Some(crit_id))
                .iter()
                .map(|vid| DocRef {
                    doc_id: vid.clone(),
                    status: graph
                        .document(vid)
                        .and_then(|d| d.frontmatter.status.clone()),
                })
                .collect();

            result.push(TargetContext {
                id: crit_id.clone(),
                target_ref: format!("{doc_id}#{crit_id}"),
                body_text: comp.body_text.clone(),
                referenced_by,
            });
        }
        // Recurse into children (e.g., AcceptanceCriteria → Criterion).
        result.extend(extract_criteria(graph, doc_id, &comp.children));
    }
    result
}

/// Recursively extract `Decision` components from a component tree,
/// building `DecisionContext` with rationale and alternatives from children.
fn extract_decisions(components: &[ExtractedComponent]) -> Vec<DecisionContext> {
    let mut result = Vec::new();
    for comp in components {
        if comp.name == DECISION
            && let Some(decision_id) = comp.attributes.get("id")
        {
            // Find the first Rationale child.
            let rationale_text = comp
                .children
                .iter()
                .find(|c| c.name == RATIONALE)
                .and_then(|c| c.body_text.clone());

            // Collect Alternative children.
            let alternatives = comp
                .children
                .iter()
                .filter(|c| c.name == ALTERNATIVE)
                .filter_map(|c| {
                    let alt_id = c.attributes.get("id")?;
                    let status = c.attributes.get("status").cloned().unwrap_or_default();
                    Some(AlternativeContext {
                        id: alt_id.clone(),
                        status,
                        body_text: c.body_text.clone(),
                    })
                })
                .collect();

            result.push(DecisionContext {
                id: decision_id.clone(),
                body_text: comp.body_text.clone(),
                rationale_text,
                alternatives,
            });
        }
        // Recurse into children to find nested decisions.
        result.extend(extract_decisions(&comp.children));
    }
    result
}

/// Extract linked decisions: for each document that references the target doc,
/// walk its component tree looking for Decision components that have a nested
/// References child whose `refs` attribute contains the target doc ID.
fn extract_linked_decisions(
    graph: &DocumentGraph,
    target_doc_id: &str,
    referenced_by: &[String],
) -> Vec<LinkedDecision> {
    let mut result = Vec::new();
    for source_doc_id in referenced_by {
        let Some(source_doc) = graph.document(source_doc_id) else {
            continue;
        };
        collect_linked_decisions_recursive(
            target_doc_id,
            source_doc_id,
            &source_doc.components,
            &mut result,
        );
    }
    result
}

/// Recursively walk a component tree, finding Decision components whose children
/// include a References component with `refs` targeting `target_doc_id`.
fn collect_linked_decisions_recursive(
    target_doc_id: &str,
    source_doc_id: &str,
    components: &[ExtractedComponent],
    result: &mut Vec<LinkedDecision>,
) {
    for comp in components {
        if comp.name == DECISION
            && let Some(decision_id) = comp.attributes.get("id")
            && decision_references_target(&comp.children, target_doc_id)
        {
            result.push(LinkedDecision {
                source_doc_id: source_doc_id.to_owned(),
                decision_id: decision_id.clone(),
                body_text: comp.body_text.clone(),
            });
        }
        // Recurse into children to find nested decisions.
        collect_linked_decisions_recursive(target_doc_id, source_doc_id, &comp.children, result);
    }
}

/// Check whether any child component of a Decision is a References component
/// whose `refs` attribute contains the target document ID.
///
/// `children` should be the direct children of a `Decision` component.
/// Returns `true` if any `References` child has a `refs` attribute entry
/// whose document part (before the optional `#fragment`) equals `target_doc_id`.
#[must_use]
pub fn decision_references_target(children: &[ExtractedComponent], target_doc_id: &str) -> bool {
    for child in children {
        if child.name == REFERENCES
            && let Some(refs_attr) = child.attributes.get("refs")
            && let Ok(items) = split_list_attribute(refs_attr)
        {
            for item in items {
                // Strip any fragment (e.g. "doc-id#fragment" -> "doc-id").
                let doc_part = item.split('#').next().unwrap_or(item);
                if doc_part == target_doc_id {
                    return true;
                }
            }
        }
    }
    false
}

/// Find all tasks documents linked to the target document (i.e., containing
/// tasks whose `implements` refs point to criteria in this document) and
/// collect those tasks in topological order.
fn collect_linked_tasks(graph: &DocumentGraph, target_doc_id: &str) -> Vec<TaskInfo> {
    let target_set = HashSet::from([target_doc_id.to_owned()]);
    let (mut pending, mut completed) = collect_tasks_for_targets(graph, &target_set);
    pending.append(&mut completed);
    pending
}

/// Build a `TaskInfo` from a task component and its graph metadata.
fn build_task_info(
    graph: &DocumentGraph,
    doc_id: &str,
    task_id: &str,
    task_comp: &ExtractedComponent,
) -> TaskInfo {
    let implements = graph
        .task_implements(doc_id, task_id)
        .map(<[(String, String)]>::to_vec)
        .unwrap_or_default();

    let depends_on = task_comp
        .attributes
        .get("depends")
        .and_then(|d| split_list_attribute(d).ok())
        .map(|items| items.into_iter().map(str::to_owned).collect())
        .unwrap_or_default();

    TaskInfo {
        tasks_doc_id: doc_id.to_owned(),
        task_id: task_id.to_owned(),
        status: task_comp.attributes.get("status").cloned(),
        body_text: task_comp.body_text.clone(),
        implements,
        depends_on,
    }
}

/// Find a `Task` component by ID in a component tree (including nested tasks).
fn find_task_component<'a>(
    components: &'a [ExtractedComponent],
    task_id: &str,
) -> Option<&'a ExtractedComponent> {
    for comp in components {
        if comp.name == TASK && comp.attributes.get("id").map(String::as_str) == Some(task_id) {
            return Some(comp);
        }
        if let Some(found) = find_task_component(&comp.children, task_id) {
            return Some(found);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Plan query implementation
// ---------------------------------------------------------------------------

/// Build a `PlanOutput` for the given query.
pub(super) fn build_plan(
    graph: &DocumentGraph,
    query: &PlanQuery,
) -> Result<PlanOutput, QueryError> {
    // Resolve the set of target document IDs.
    let target_doc_ids: HashSet<String> = match query {
        PlanQuery::Document(id) => {
            if graph.document(id).is_none() {
                return Err(QueryError::NoMatchingDocuments { query: id.clone() });
            }
            HashSet::from([id.clone()])
        }
        PlanQuery::Prefix(prefix) => {
            let ids: HashSet<String> = graph
                .documents()
                .filter(|(id, _)| id.starts_with(prefix.as_str()))
                .map(|(id, _)| id.to_owned())
                .collect();
            if ids.is_empty() {
                return Err(QueryError::NoMatchingDocuments {
                    query: prefix.clone(),
                });
            }
            ids
        }
        PlanQuery::All => graph.documents().map(|(id, _)| id.to_owned()).collect(),
    };

    let (pending_tasks, completed_tasks) = collect_plan_tasks(graph, &target_doc_ids);
    let done_implemented = collect_done_implemented_targets(graph, &completed_tasks);
    let outstanding_targets =
        collect_outstanding_targets(graph, &target_doc_ids, &done_implemented);

    let (actionable_tasks, blocked_tasks) = partition_tasks(&pending_tasks, &completed_tasks);

    Ok(PlanOutput {
        outstanding_targets,
        pending_tasks,
        completed_tasks,
        actionable_tasks,
        blocked_tasks,
    })
}

/// Partition pending tasks into actionable and blocked.
///
/// A task is "actionable" if all its `depends_on` entries are either completed
/// or not in the pending set. Otherwise it is "blocked".
fn partition_tasks(
    pending_tasks: &[TaskInfo],
    completed_tasks: &[TaskInfo],
) -> (Vec<String>, Vec<String>) {
    let completed_ids: HashSet<&str> = completed_tasks.iter().map(|t| t.task_id.as_str()).collect();
    let pending_ids: HashSet<&str> = pending_tasks.iter().map(|t| t.task_id.as_str()).collect();

    let mut actionable = Vec::new();
    let mut blocked = Vec::new();

    for task in pending_tasks {
        let is_actionable = task
            .depends_on
            .iter()
            .all(|dep| completed_ids.contains(dep.as_str()) || !pending_ids.contains(dep.as_str()));
        if is_actionable {
            actionable.push(task.task_id.clone());
        } else {
            blocked.push(task.task_id.clone());
        }
    }

    (actionable, blocked)
}

/// Build the set of `(doc_id, target_id)` pairs covered by completed tasks.
fn collect_done_implemented_targets(
    graph: &DocumentGraph,
    completed_tasks: &[TaskInfo],
) -> HashSet<(String, String)> {
    let mut set = HashSet::new();
    for task in completed_tasks {
        if let Some(impls) = graph.task_implements(&task.tasks_doc_id, &task.task_id) {
            for (doc_id, crit_id) in impls {
                set.insert((doc_id.clone(), crit_id.clone()));
            }
        }
    }
    set
}

/// Collect verification targets that have no referencing document and no
/// completed task implementing them.
fn collect_outstanding_targets(
    graph: &DocumentGraph,
    target_doc_ids: &HashSet<String>,
    done_implemented: &HashSet<(String, String)>,
) -> Vec<OutstandingTarget> {
    let mut result = Vec::new();
    for doc_id in target_doc_ids {
        if let Some(doc) = graph.document(doc_id) {
            collect_outstanding_from_components(
                doc_id,
                &doc.components,
                done_implemented,
                &mut result,
            );
        }
    }
    result
}

/// Recursively find verifiable components with no completed task
/// implementing them.
///
/// Note: `References` links are informational and do not suppress targets.
/// Evidence-based coverage filtering happens at the CLI layer using the
/// `ArtifactGraph`.
fn collect_outstanding_from_components(
    doc_id: &str,
    components: &[ExtractedComponent],
    done_implemented: &HashSet<(String, String)>,
    result: &mut Vec<OutstandingTarget>,
) {
    for comp in components {
        if comp.name == CRITERION
            && let Some(crit_id) = comp.attributes.get("id")
        {
            let has_done_task = done_implemented.contains(&(doc_id.to_owned(), crit_id.clone()));
            if !has_done_task {
                result.push(OutstandingTarget {
                    doc_id: doc_id.to_owned(),
                    target_id: crit_id.clone(),
                    body_text: comp.body_text.clone(),
                });
            }
        }
        collect_outstanding_from_components(doc_id, &comp.children, done_implemented, result);
    }
}

/// Collect pending (status ≠ done) and completed (status = done) tasks from
/// tasks documents linked to the target docs. Tasks are in topo order.
fn collect_plan_tasks(
    graph: &DocumentGraph,
    target_doc_ids: &HashSet<String>,
) -> (Vec<TaskInfo>, Vec<TaskInfo>) {
    collect_tasks_for_targets(graph, target_doc_ids)
}

/// Shared task collection: find all tasks documents linked to the target docs,
/// filter tasks by `implements`, and split into pending/completed by status.
fn collect_tasks_for_targets(
    graph: &DocumentGraph,
    target_doc_ids: &HashSet<String>,
) -> (Vec<TaskInfo>, Vec<TaskInfo>) {
    let mut pending = Vec::new();
    let mut completed = Vec::new();

    for (tasks_doc_id, tasks_doc) in graph.documents() {
        let Some(topo_order) = graph.task_order(tasks_doc_id) else {
            continue;
        };

        let linked_task_ids: Vec<&str> = topo_order
            .iter()
            .filter(|task_id| {
                graph
                    .task_implements(tasks_doc_id, task_id)
                    .is_some_and(|impls| impls.iter().any(|(tid, _)| target_doc_ids.contains(tid)))
            })
            .map(String::as_str)
            .collect();

        if linked_task_ids.is_empty() {
            continue;
        }

        for task_id in linked_task_ids {
            let Some(task_comp) = find_task_component(&tasks_doc.components, task_id) else {
                continue;
            };

            let task_info = build_task_info(graph, tasks_doc_id, task_id, task_comp);

            if task_comp.attributes.get("status").map(String::as_str) == Some("done") {
                completed.push(task_info);
            } else {
                pending.push(task_info);
            }
        }
    }

    (pending, completed)
}
