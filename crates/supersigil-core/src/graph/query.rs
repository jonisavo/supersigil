//! Structured query outputs for `context` and `plan` commands.

use std::collections::HashSet;

use super::{CRITERION, DocumentGraph, TASK};
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
#[derive(Debug, Clone, PartialEq)]
pub struct ContextOutput {
    /// The target document.
    pub document: crate::SpecDocument,
    /// Criteria with their validation/illustration status.
    pub criteria: Vec<CriterionContext>,
    /// Documents that implement this document.
    pub implemented_by: Vec<DocRef>,
    /// Documents that illustrate this document (document-level).
    pub illustrated_by: Vec<String>,
    /// Tasks from linked tasks documents, in topological order.
    pub tasks: Vec<TaskInfo>,
}

/// A criterion with its incoming validation and illustration relationships.
#[derive(Debug, Clone, PartialEq)]
pub struct CriterionContext {
    pub id: String,
    pub body_text: Option<String>,
    /// Documents that validate this criterion, with their status.
    pub validated_by: Vec<DocRef>,
    /// Documents that illustrate this criterion.
    pub illustrated_by: Vec<String>,
}

/// A reference to a document with its optional status.
#[derive(Debug, Clone, PartialEq)]
pub struct DocRef {
    pub doc_id: String,
    pub status: Option<String>,
}

/// A task with dependency and implements info, used in both context and plan output.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskInfo {
    /// Which tasks document this task belongs to.
    pub tasks_doc_id: String,
    pub task_id: String,
    pub status: Option<String>,
    pub body_text: Option<String>,
    /// Criteria this task implements: `(doc_id, criterion_id)`.
    pub implements: Vec<(String, String)>,
    /// Task IDs this task depends on.
    pub depends_on: Vec<String>,
}

// ---------------------------------------------------------------------------
// PlanOutput
// ---------------------------------------------------------------------------

/// Structured output for the `plan` command.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanOutput {
    /// Criteria with no validating document.
    pub outstanding_criteria: Vec<OutstandingCriterion>,
    /// Tasks not yet done, in topological order, grouped by tasks document.
    pub pending_tasks: Vec<TaskInfo>,
    /// Completed tasks with the criteria they implement.
    pub completed_tasks: Vec<TaskInfo>,
    /// Documents that illustrate the target.
    pub illustrated_by: Vec<IllustrationRef>,
}

/// A criterion that has no validating document.
#[derive(Debug, Clone, PartialEq)]
pub struct OutstandingCriterion {
    /// The requirement document containing this criterion.
    pub doc_id: String,
    pub criterion_id: String,
    pub body_text: Option<String>,
}

/// A document that illustrates a target criterion or document.
#[derive(Debug, Clone, PartialEq)]
pub struct IllustrationRef {
    pub doc_id: String,
    /// The target criterion or document being illustrated.
    pub target_doc_id: String,
    pub target_fragment: Option<String>,
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

    // Document-level illustrations.
    let illustrated_by = graph.illustrates(id, None).iter().cloned().collect();

    // Linked tasks: find all tasks documents that have tasks implementing
    // criteria in this document, then collect those tasks in topo order.
    let tasks = collect_linked_tasks(graph, id);

    Ok(ContextOutput {
        document,
        criteria,
        implemented_by,
        illustrated_by,
        tasks,
    })
}

/// Recursively extract `Criterion` components from a component tree,
/// building `CriterionContext` with reverse mapping lookups.
fn extract_criteria(
    graph: &DocumentGraph,
    doc_id: &str,
    components: &[ExtractedComponent],
) -> Vec<CriterionContext> {
    let mut result = Vec::new();
    for comp in components {
        if comp.name == CRITERION
            && let Some(crit_id) = comp.attributes.get("id")
        {
            let validated_by = graph
                .validates(doc_id, Some(crit_id))
                .iter()
                .map(|vid| DocRef {
                    doc_id: vid.clone(),
                    status: graph
                        .document(vid)
                        .and_then(|d| d.frontmatter.status.clone()),
                })
                .collect();

            let illustrated_by = graph
                .illustrates(doc_id, Some(crit_id))
                .iter()
                .cloned()
                .collect();

            result.push(CriterionContext {
                id: crit_id.clone(),
                body_text: comp.body_text.clone(),
                validated_by,
                illustrated_by,
            });
        }
        // Recurse into children (e.g., AcceptanceCriteria → Criterion).
        result.extend(extract_criteria(graph, doc_id, &comp.children));
    }
    result
}

/// Find all tasks documents linked to the target document (i.e., containing
/// tasks whose `implements` refs point to criteria in this document) and
/// collect those tasks in topological order.
fn collect_linked_tasks(graph: &DocumentGraph, target_doc_id: &str) -> Vec<TaskInfo> {
    let mut tasks = Vec::new();

    // Scan all documents for Task components that implement criteria in the
    // target document. We use task_implements to check linkage.
    for (doc_id, doc) in graph.documents() {
        // Get the topo order for this document (if it has tasks).
        let Some(topo_order) = graph.task_order(doc_id) else {
            continue;
        };

        // Check if any task in this document implements a criterion in the
        // target document.
        let linked_task_ids: Vec<&str> = topo_order
            .iter()
            .filter(|task_id| {
                graph
                    .task_implements(doc_id, task_id)
                    .is_some_and(|impls| impls.iter().any(|(tid, _)| tid == target_doc_id))
            })
            .map(String::as_str)
            .collect();

        if linked_task_ids.is_empty() {
            continue;
        }

        // Collect task details in topo order.
        for task_id in linked_task_ids {
            if let Some(task_comp) = find_task_component(&doc.components, task_id) {
                tasks.push(build_task_info(graph, doc_id, task_id, task_comp));
            }
        }
    }

    tasks
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

    let outstanding_criteria = collect_outstanding_criteria(graph, &target_doc_ids);
    let (pending_tasks, completed_tasks) = collect_plan_tasks(graph, &target_doc_ids);
    let illustrated_by = collect_illustrations(graph, &target_doc_ids);

    Ok(PlanOutput {
        outstanding_criteria,
        pending_tasks,
        completed_tasks,
        illustrated_by,
    })
}

/// Collect criteria that have no validating document from the target docs.
fn collect_outstanding_criteria(
    graph: &DocumentGraph,
    target_doc_ids: &HashSet<String>,
) -> Vec<OutstandingCriterion> {
    let mut result = Vec::new();
    for doc_id in target_doc_ids {
        if let Some(doc) = graph.document(doc_id) {
            collect_outstanding_from_components(graph, doc_id, &doc.components, &mut result);
        }
    }
    result
}

/// Recursively find `Criterion` components with no validators.
fn collect_outstanding_from_components(
    graph: &DocumentGraph,
    doc_id: &str,
    components: &[ExtractedComponent],
    result: &mut Vec<OutstandingCriterion>,
) {
    for comp in components {
        if comp.name == CRITERION
            && let Some(crit_id) = comp.attributes.get("id")
        {
            let validators = graph.validates(doc_id, Some(crit_id));
            if validators.is_empty() {
                result.push(OutstandingCriterion {
                    doc_id: doc_id.to_owned(),
                    criterion_id: crit_id.clone(),
                    body_text: comp.body_text.clone(),
                });
            }
        }
        collect_outstanding_from_components(graph, doc_id, &comp.children, result);
    }
}

/// Collect pending (status ≠ done) and completed (status = done) tasks from
/// tasks documents linked to the target docs. Tasks are in topo order.
fn collect_plan_tasks(
    graph: &DocumentGraph,
    target_doc_ids: &HashSet<String>,
) -> (Vec<TaskInfo>, Vec<TaskInfo>) {
    let mut pending = Vec::new();
    let mut completed = Vec::new();

    // Find all tasks documents that have tasks implementing criteria in any
    // of the target documents.
    for (tasks_doc_id, tasks_doc) in graph.documents() {
        let Some(topo_order) = graph.task_order(tasks_doc_id) else {
            continue;
        };

        // Collect task IDs that implement criteria in any target doc.
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

/// Collect illustration references for the target documents and their criteria.
fn collect_illustrations(
    graph: &DocumentGraph,
    target_doc_ids: &HashSet<String>,
) -> Vec<IllustrationRef> {
    let mut result = Vec::new();
    for doc_id in target_doc_ids {
        // Document-level illustrations.
        for illus_doc_id in graph.illustrates(doc_id, None) {
            result.push(IllustrationRef {
                doc_id: illus_doc_id.clone(),
                target_doc_id: doc_id.to_owned(),
                target_fragment: None,
            });
        }

        // Criterion-level illustrations.
        if let Some(doc) = graph.document(doc_id) {
            collect_criterion_illustrations(graph, doc_id, &doc.components, &mut result);
        }
    }
    result
}

/// Recursively find `Criterion` components and collect their illustrations.
fn collect_criterion_illustrations(
    graph: &DocumentGraph,
    doc_id: &str,
    components: &[ExtractedComponent],
    result: &mut Vec<IllustrationRef>,
) {
    for comp in components {
        if comp.name == CRITERION
            && let Some(crit_id) = comp.attributes.get("id")
        {
            for illus_doc_id in graph.illustrates(doc_id, Some(crit_id)) {
                result.push(IllustrationRef {
                    doc_id: illus_doc_id.clone(),
                    target_doc_id: doc_id.to_owned(),
                    target_fragment: Some(crit_id.clone()),
                });
            }
        }
        collect_criterion_illustrations(graph, doc_id, &comp.children, result);
    }
}
