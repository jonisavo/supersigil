//! Document graph: cross-document relationship indexing, validation, and query.
//!
//! The graph builder consumes `SpecDocument` values from the parser and a
//! `Config` from the config loader, producing an indexed, validated graph
//! of document relationships.

mod cycle;
mod error;
mod index;
mod query;
mod resolve;
mod reverse;
mod topo;

#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::fmt;

pub use error::GraphError;
pub use index::glob_prefix;
pub use query::{
    AlternativeContext, ContextOutput, DecisionContext, DocRef, LinkedDecision, OutstandingTarget,
    PlanOutput, PlanQuery, QueryError, TargetContext, TaskInfo, decision_references_target,
};

use crate::{ComponentDefs, Config, ExtractedComponent, SpecDocument};

// Well-known component names used during graph construction and downstream queries.
/// The well-known component name for task components.
pub const TASK: &str = "Task";
/// The well-known component name for criterion components.
pub const CRITERION: &str = "Criterion";
/// The well-known component name for verified-by components.
pub const VERIFIED_BY: &str = "VerifiedBy";
/// The well-known component name for acceptance-criteria wrapper components.
pub const ACCEPTANCE_CRITERIA: &str = "AcceptanceCriteria";
/// The well-known component name for references components.
pub const REFERENCES: &str = "References";
/// The well-known component name for implements components.
pub const IMPLEMENTS: &str = "Implements";
/// The well-known component name for depends-on components.
pub const DEPENDS_ON: &str = "DependsOn";
/// The well-known component name for tracked-files components.
pub const TRACKED_FILES: &str = "TrackedFiles";
/// The well-known component name for example components.
pub const EXAMPLE: &str = "Example";
/// The well-known component name for expected components.
pub const EXPECTED: &str = "Expected";
/// The well-known component name for decision components.
pub const DECISION: &str = "Decision";
/// The well-known component name for rationale components.
pub const RATIONALE: &str = "Rationale";
/// The well-known component name for alternative components.
pub const ALTERNATIVE: &str = "Alternative";
/// The language identifier for supersigil XML fenced code blocks.
pub const SUPERSIGIL_XML_LANG: &str = "supersigil-xml";
/// The well-known fragment name for expected output in ref fences.
pub const EXPECTED_FRAGMENT: &str = "expected";

// ---------------------------------------------------------------------------
// EdgeKind
// ---------------------------------------------------------------------------

/// The kind of a document-level edge in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// An `Implements` edge: the source document implements the target.
    Implements,
    /// A `DependsOn` edge: the source document depends on the target.
    DependsOn,
    /// A `References` edge: the source document references the target.
    References,
}

// ---------------------------------------------------------------------------
// ResolvedRef
// ---------------------------------------------------------------------------

/// A successfully resolved reference.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRef {
    /// The raw ref string as written in the source.
    pub raw: String,
    /// Target document ID.
    pub target_doc_id: String,
    /// Target fragment (criterion/task ID), if present.
    pub fragment: Option<String>,
}

// ---------------------------------------------------------------------------
// DocumentGraph
// ---------------------------------------------------------------------------

/// The primary output of graph construction. Holds all indexes, orderings,
/// and reverse mappings. Provides query methods for `context` and `plan`.
pub struct DocumentGraph {
    /// Document ID → `SpecDocument`.
    doc_index: HashMap<String, SpecDocument>,

    /// `(document_id, fragment)` → `ExtractedComponent`.
    component_index: HashMap<(String, String), ExtractedComponent>,

    /// Resolved refs keyed by source component path.
    /// Key: `(source_doc_id, component_path)` where `component_path` is a
    /// `Vec<usize>` index path from root to the component.
    resolved_refs: HashMap<(String, Vec<usize>), Vec<ResolvedRef>>,

    /// Reverse mapping: target `(doc_id, Option<fragment>)` → set of referencing doc IDs.
    references_reverse: HashMap<(String, Option<String>), BTreeSet<String>>,
    /// Reverse mapping: target `doc_id` → set of implementing doc IDs.
    implements_reverse: HashMap<String, BTreeSet<String>>,
    /// Reverse mapping: target `doc_id` → set of depending doc IDs.
    depends_on_reverse: HashMap<String, BTreeSet<String>>,

    /// Task topological orderings per tasks document.
    task_topo_orders: HashMap<String, Vec<String>>,
    /// Document topological ordering (from `DependsOn` edges).
    doc_topo_order: Vec<String>,

    /// `TrackedFiles` index: document ID → list of path globs.
    tracked_files_index: HashMap<String, Vec<String>>,

    /// Resolved task implements: `(doc_id, task_id)` → `Vec<(target_doc_id, target_id)>`.
    task_implements: HashMap<(String, String), Vec<(String, String)>>,

    /// Project membership: document ID → project name (`None` for single-project).
    doc_project: HashMap<String, Option<String>>,

    /// Merged component definitions used during graph construction.
    component_defs: ComponentDefs,
}

impl fmt::Debug for DocumentGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentGraph")
            .field("doc_count", &self.doc_index.len())
            .field("component_count", &self.component_index.len())
            .finish_non_exhaustive()
    }
}

// Sentinel empty set returned by reverse-mapping accessors for unreferenced targets.
static EMPTY_BTREESET: BTreeSet<String> = BTreeSet::new();

impl DocumentGraph {
    // -- Document index accessors (task 3.3) ---------------------------------

    /// Look up a document by ID. Returns `None` if not found.
    #[must_use]
    pub fn document(&self, id: &str) -> Option<&SpecDocument> {
        self.doc_index.get(id)
    }

    /// Iterate over all `(id, document)` pairs.
    pub fn documents(&self) -> impl Iterator<Item = (&str, &SpecDocument)> {
        self.doc_index.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Get the project a document belongs to.
    #[must_use]
    pub fn doc_project(&self, doc_id: &str) -> Option<&str> {
        self.doc_project.get(doc_id).and_then(|opt| opt.as_deref())
    }

    /// Get the merged component definitions used during graph construction.
    #[must_use]
    pub fn component_defs(&self) -> &ComponentDefs {
        &self.component_defs
    }

    // -- Component index accessor (task 4.3) --------------------------------

    /// Look up a referenceable component by `(document_id, fragment)`.
    #[must_use]
    pub fn component(&self, doc_id: &str, fragment: &str) -> Option<&ExtractedComponent> {
        self.component_index
            .get(&(doc_id.to_owned(), fragment.to_owned()))
    }

    // -- Criteria accessors (ref-discovery) ----------------------------------

    /// Iterate all referenceable components.
    /// Yields `(doc_id, fragment_id, &ExtractedComponent)`.
    pub fn criteria(&self) -> impl Iterator<Item = (&str, &str, &ExtractedComponent)> {
        self.component_index
            .iter()
            .map(|((doc_id, frag), comp)| (doc_id.as_str(), frag.as_str(), comp))
    }

    /// Find all components whose fragment ID matches, across all documents.
    #[must_use]
    pub fn criteria_by_fragment(&self, fragment: &str) -> Vec<(&str, &ExtractedComponent)> {
        self.component_index
            .iter()
            .filter_map(|((doc_id, frag), comp)| {
                (frag == fragment).then_some((doc_id.as_str(), comp))
            })
            .collect()
    }

    // -- Resolved refs stub (replaced in task 6.5) ---------------------------

    /// Get resolved refs for a component at the given index path in a document.
    #[must_use]
    pub fn resolved_refs(&self, doc_id: &str, component_path: &[usize]) -> Option<&[ResolvedRef]> {
        self.resolved_refs
            .get(&(doc_id.to_owned(), component_path.to_vec()))
            .map(Vec::as_slice)
    }

    // -- Task implements accessor (task 7.2) -----------------------------------

    /// Get resolved task implements for a specific task.
    #[must_use]
    pub fn task_implements(&self, doc_id: &str, task_id: &str) -> Option<&[(String, String)]> {
        self.task_implements
            .get(&(doc_id.to_owned(), task_id.to_owned()))
            .map(Vec::as_slice)
    }

    // -- Topological order accessors (task 10.3) ---------------------------

    /// Get the topological ordering of tasks within a tasks document.
    #[must_use]
    pub fn task_order(&self, doc_id: &str) -> Option<&[String]> {
        self.task_topo_orders.get(doc_id).map(Vec::as_slice)
    }

    /// Get the document-level topological ordering.
    #[must_use]
    pub fn doc_order(&self) -> &[String] {
        &self.doc_topo_order
    }

    // -- Reverse mapping accessors (task 12.3) -----------------------

    /// Get all documents that reference a given target.
    #[must_use]
    pub fn references(&self, doc_id: &str, fragment: Option<&str>) -> &BTreeSet<String> {
        let key = (doc_id.to_owned(), fragment.map(str::to_owned));
        self.references_reverse.get(&key).unwrap_or(&EMPTY_BTREESET)
    }

    /// Get all documents that implement a given document (reverse direction).
    #[must_use]
    pub fn implements(&self, doc_id: &str) -> &BTreeSet<String> {
        self.implements_reverse
            .get(doc_id)
            .unwrap_or(&EMPTY_BTREESET)
    }

    /// Get all documents that `doc_id` implements (forward direction).
    ///
    /// Scans the reverse mapping to find targets. Returns an empty vec for
    /// docs that don't implement anything or for unknown doc IDs.
    #[must_use]
    pub fn implements_targets(&self, doc_id: &str) -> Vec<&str> {
        self.implements_reverse
            .iter()
            .filter_map(|(target, sources)| sources.contains(doc_id).then_some(target.as_str()))
            .collect()
    }

    /// Get all documents that depend on a given document.
    #[must_use]
    pub fn depends_on(&self, doc_id: &str) -> &BTreeSet<String> {
        self.depends_on_reverse
            .get(doc_id)
            .unwrap_or(&EMPTY_BTREESET)
    }

    // -- Edge iteration (graph-explorer task 2) ------------------------------

    /// Iterate all edges from the reverse mappings.
    ///
    /// Yields `(source_doc_id, target_doc_id, EdgeKind)` triples sourced
    /// from `implements_reverse`, `depends_on_reverse`, and `references_reverse`.
    pub fn edges(&self) -> impl Iterator<Item = (&str, &str, EdgeKind)> {
        let implements = self
            .implements_reverse
            .iter()
            .flat_map(|(target, sources)| {
                sources
                    .iter()
                    .map(move |src| (src.as_str(), target.as_str(), EdgeKind::Implements))
            });

        let depends_on = self
            .depends_on_reverse
            .iter()
            .flat_map(|(target, sources)| {
                sources
                    .iter()
                    .map(move |src| (src.as_str(), target.as_str(), EdgeKind::DependsOn))
            });

        let references =
            self.references_reverse
                .iter()
                .flat_map(|((target, _fragment), sources)| {
                    sources
                        .iter()
                        .map(move |src| (src.as_str(), target.as_str(), EdgeKind::References))
                });

        implements.chain(depends_on).chain(references)
    }

    // -- TrackedFiles accessors (task 13.3) ----------------------------------

    /// Get `TrackedFiles` globs for a document. Returns `None` if none declared.
    #[must_use]
    pub fn tracked_files(&self, doc_id: &str) -> Option<&[String]> {
        self.tracked_files_index.get(doc_id).map(Vec::as_slice)
    }

    /// Iterate over all `(doc_id, globs)` pairs in the `TrackedFiles` index.
    pub fn all_tracked_files(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.tracked_files_index
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    // -- Query methods (implemented in query.rs, tasks 16 & 17) --------------

    /// Structured context query.
    ///
    /// # Errors
    ///
    /// Returns `QueryError::DocumentNotFound` if `id` is not in the graph.
    pub fn context(&self, id: &str) -> Result<ContextOutput, QueryError> {
        query::build_context(self, id)
    }

    /// Structured plan query.
    ///
    /// # Errors
    ///
    /// Returns `QueryError::NoMatchingDocuments` if the query matches nothing.
    pub fn plan(&self, query: &PlanQuery) -> Result<PlanOutput, QueryError> {
        query::build_plan(self, query)
    }
}

// ---------------------------------------------------------------------------
// build_graph
// ---------------------------------------------------------------------------

/// Build a `DocumentGraph` from parsed documents and config.
///
/// Returns `Ok(DocumentGraph)` if no hard errors occur, or
/// `Err(Vec<GraphError>)` if any hard errors are found.
///
/// # Errors
///
/// Returns all graph construction errors (duplicate IDs, broken refs,
/// dependency cycles) collected across all pipeline stages.
pub fn build_graph(
    documents: Vec<SpecDocument>,
    config: &Config,
) -> Result<DocumentGraph, Vec<GraphError>> {
    let mut errors = Vec::new();

    // Stage 1: Document indexing
    let (doc_index, index_errors) = index::build_doc_index(documents);
    errors.extend(index_errors);

    // Stage 1b: Project membership
    let doc_project = index::build_doc_project(&doc_index, config);

    // Stage 2: Referenceable component indexing
    let component_defs = ComponentDefs::merge(ComponentDefs::defaults(), config.components.clone())
        .map_err(|errs| {
            errs.into_iter()
                .map(GraphError::InvalidComponentDef)
                .collect::<Vec<_>>()
        })?;
    let (component_index, comp_errors) = index::build_component_index(&doc_index, &component_defs);
    errors.extend(comp_errors);

    // Stage 3–4: Ref resolution and task implements resolution
    let project_isolation = build_project_isolation(config);
    let resolve_ctx = resolve::ResolveContext {
        doc_index: &doc_index,
        component_index: &component_index,
        component_defs: &component_defs,
        doc_project: &doc_project,
        project_isolation: &project_isolation,
    };
    let (resolved_refs, ref_errors) = resolve::resolve_refs(&resolve_ctx);
    errors.extend(ref_errors);
    let (task_implements, impl_errors) = resolve::resolve_task_implements(&resolve_ctx);
    errors.extend(impl_errors);

    // Stage 5: Task dependency cycle detection
    let task_cycle_errors = cycle::detect_task_cycles(&doc_index);
    errors.extend(task_cycle_errors);

    // Stage 6: Document dependency cycle detection
    let doc_cycle_errors = cycle::detect_document_cycles(&resolved_refs, &doc_index);
    errors.extend(doc_cycle_errors);

    // Return early with errors if any were found.
    if !errors.is_empty() {
        return Err(errors);
    }

    // Stage 7: Topological sort
    let task_topo_orders = topo::compute_task_topo_orders(&doc_index);
    let doc_topo_order = topo::compute_doc_topo_order(&resolved_refs, &doc_index);

    // Stage 8: Reverse mappings
    let (references_reverse, implements_reverse, depends_on_reverse) =
        reverse::build_reverse_mappings(&resolved_refs, &doc_index);

    // Stage 9: TrackedFiles indexing
    let tracked_files_index = index::build_tracked_files_index(&doc_index);

    Ok(DocumentGraph {
        doc_index,
        component_index,
        resolved_refs,
        references_reverse,
        implements_reverse,
        depends_on_reverse,
        task_topo_orders,
        doc_topo_order,
        tracked_files_index,
        task_implements,
        doc_project,
        component_defs,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect document-level `DependsOn` edges as `(source, target)` pairs.
///
/// Iterates each document's components recursively, finds `DependsOn`
/// components at any nesting depth, looks up their resolved refs, and
/// yields `(source_doc_id, target_doc_id)` pairs.
///
/// Used by both cycle detection (stage 6) and topological sort (stage 7b).
pub(crate) fn collect_doc_dependency_edges<'a>(
    doc_index: &'a HashMap<String, SpecDocument>,
    resolved_refs: &'a HashMap<(String, Vec<usize>), Vec<ResolvedRef>>,
) -> impl Iterator<Item = (&'a str, &'a str)> {
    doc_index.iter().flat_map(move |(doc_id, doc)| {
        let mut edges: Vec<(&str, &str)> = Vec::new();
        collect_depends_on_edges_recursive(
            doc_id.as_str(),
            &doc.components,
            &[],
            resolved_refs,
            &mut edges,
        );
        edges.into_iter()
    })
}

/// Recursively walk components collecting `DependsOn` edges at any depth.
fn collect_depends_on_edges_recursive<'a>(
    doc_id: &'a str,
    components: &'a [ExtractedComponent],
    parent_path: &[usize],
    resolved_refs: &'a HashMap<(String, Vec<usize>), Vec<ResolvedRef>>,
    edges: &mut Vec<(&'a str, &'a str)>,
) {
    for (idx, comp) in components.iter().enumerate() {
        let mut component_path = parent_path.to_vec();
        component_path.push(idx);

        if comp.name == DEPENDS_ON {
            let key = (doc_id.to_owned(), component_path.clone());
            if let Some(refs) = resolved_refs.get(&key) {
                for rr in refs {
                    edges.push((doc_id, rr.target_doc_id.as_str()));
                }
            }
        }

        if !comp.children.is_empty() {
            collect_depends_on_edges_recursive(
                doc_id,
                &comp.children,
                &component_path,
                resolved_refs,
                edges,
            );
        }
    }
}

/// Build a map of project name → isolated flag from config.
fn build_project_isolation(config: &Config) -> HashMap<String, bool> {
    config
        .projects
        .as_ref()
        .map(|projects| {
            projects
                .iter()
                .map(|(name, pc)| (name.clone(), pc.isolated))
                .collect()
        })
        .unwrap_or_default()
}
