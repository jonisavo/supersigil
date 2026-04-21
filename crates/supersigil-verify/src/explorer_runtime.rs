//! Explorer runtime payloads for lazy-loading graph explorer clients.

use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use supersigil_core::{
    ALTERNATIVE, CRITERION, DECISION, DocumentGraph, ExtractedComponent, RATIONALE, SpecDocument,
    TASK,
};
use supersigil_evidence::{EvidenceId, VerificationEvidenceRecord};

use crate::document_components::{
    DocumentComponentsResult, EdgeData, EvidenceIndex, FenceData, map_provenance, map_test_kind,
    relativize,
};

/// Summary verification counts for one document in the explorer shell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageSummary {
    /// Number of verified criteria in the document.
    pub verified: usize,
    /// Total number of criteria in the document.
    pub total: usize,
}

/// Top-level snapshot payload used by the graph shell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplorerSnapshot {
    /// Opaque revision identifier for the snapshot.
    pub revision: String,
    /// Explorer-visible document summaries.
    pub documents: Vec<ExplorerDocumentSummary>,
    /// Document-level graph edges.
    pub edges: Vec<ExplorerEdge>,
}

/// First-paint summary for one explorer document node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplorerDocumentSummary {
    /// The document ID.
    pub id: String,
    /// The document type, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_type: Option<String>,
    /// The document status, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Human-readable title.
    pub title: String,
    /// Project-relative path.
    pub path: String,
    /// Absolute file URI, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_uri: Option<String>,
    /// Project membership for multi-project workspaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Precomputed verification counts for shell badges/indexes.
    pub coverage_summary: CoverageSummary,
    /// Count of graph-visible components.
    pub component_count: usize,
    /// Flattened graph-visible component outline.
    pub graph_components: Vec<ExplorerGraphComponent>,
}

/// Graph-visible component summary for drilldown and cluster counts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplorerGraphComponent {
    /// Stable component identifier.
    pub id: String,
    /// Component kind (Criterion, Task, Decision, Rationale, Alternative).
    pub kind: String,
    /// Component body text, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Parent decision component ID for nested decision children.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_component_id: Option<String>,
    /// Task implements refs, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implements: Option<Vec<String>>,
}

/// Explorer edge between two documents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExplorerEdge {
    /// Source document ID.
    pub from: String,
    /// Target document ID.
    pub to: String,
    /// Edge kind.
    pub kind: String,
}

/// Parameters for one lazy document-detail request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplorerDocumentParams {
    /// Requested document ID.
    pub document_id: String,
    /// Revision the client believes is active.
    pub revision: String,
}

/// Full document-detail payload for the explorer sidebar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplorerDocument {
    /// Revision used to build this payload.
    pub revision: String,
    /// The document ID.
    pub document_id: String,
    /// Whether this detail payload is stale.
    pub stale: bool,
    /// Rendered fence payloads.
    pub fences: Vec<FenceData>,
    /// Outgoing document edges.
    pub edges: Vec<EdgeData>,
}

/// Revisioned change notification for explorer clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplorerChangedEvent {
    /// The new active revision.
    pub revision: String,
    /// Documents whose summary or incident edges changed.
    pub changed_document_ids: Vec<String>,
    /// Documents that disappeared since the previous snapshot.
    pub removed_document_ids: Vec<String>,
}

/// Input for building an [`ExplorerSnapshot`].
#[derive(Debug)]
pub struct BuildExplorerSnapshotInput<'a> {
    /// Revision identifier to embed into the snapshot.
    pub revision: &'a str,
    /// Current document graph.
    pub graph: &'a DocumentGraph,
    /// Optional evidence index from the latest verify run.
    pub evidence_by_target: Option<&'a EvidenceIndex>,
    /// Project root for relative path and file-URI resolution.
    pub project_root: &'a Path,
}

/// Input for building an [`ExplorerDocument`].
#[derive(Debug, Clone)]
pub struct BuildExplorerDocumentInput<'a> {
    /// Revision identifier to embed into the detail payload.
    pub revision: &'a str,
    /// Already-rendered per-document component payload.
    pub document_components: DocumentComponentsResult,
}

/// Compact fingerprint of a document detail payload for invalidation checks.
pub type ExplorerDocumentFingerprint = u64;

/// Input for fingerprinting explorer-detail payload semantics without building
/// a full [`DocumentComponentsResult`].
#[derive(Debug)]
pub struct FingerprintExplorerDocumentInput<'a> {
    /// The document ID.
    pub document_id: &'a str,
    /// Whether the detail payload should be marked stale.
    pub stale: bool,
    /// Current document content.
    pub content: &'a str,
    /// Extracted components from the spec document.
    pub components: &'a [ExtractedComponent],
    /// Current document graph.
    pub graph: &'a DocumentGraph,
    /// Optional evidence index from the latest verify run.
    pub evidence_by_target: Option<&'a EvidenceIndex>,
    /// Optional evidence records indexed by evidence ID.
    pub evidence_record_lookup: Option<&'a HashMap<EvidenceId, &'a VerificationEvidenceRecord>>,
    /// Project root for path relativization in evidence provenance.
    pub project_root: &'a Path,
}

/// Build an [`ExplorerSnapshot`] from graph and evidence state.
#[must_use]
pub fn build_explorer_snapshot(input: &BuildExplorerSnapshotInput<'_>) -> ExplorerSnapshot {
    let mut documents: Vec<ExplorerDocumentSummary> = input
        .graph
        .documents()
        .map(|(id, doc)| {
            build_document_summary(
                id,
                doc,
                input.graph,
                input.evidence_by_target,
                input.project_root,
            )
        })
        .collect();
    documents.sort_by(|left, right| left.id.cmp(&right.id));

    let mut edges: Vec<ExplorerEdge> = input
        .graph
        .edges()
        .map(|(from, to, kind)| ExplorerEdge {
            from: from.to_owned(),
            to: to.to_owned(),
            kind: kind.as_str().to_owned(),
        })
        .collect();
    edges.sort();
    edges.dedup();

    ExplorerSnapshot {
        revision: input.revision.to_owned(),
        documents,
        edges,
    }
}

/// Wrap a rendered document-components payload into an [`ExplorerDocument`].
#[must_use]
pub fn build_explorer_document(input: &BuildExplorerDocumentInput<'_>) -> ExplorerDocument {
    ExplorerDocument {
        revision: input.revision.to_owned(),
        document_id: input.document_components.document_id.clone(),
        stale: input.document_components.stale,
        fences: input.document_components.fences.clone(),
        edges: input.document_components.edges.clone(),
    }
}

/// Fingerprint explorer-detail payload semantics without allocating the full
/// detail response.
#[must_use]
pub fn fingerprint_explorer_document_detail(
    input: &FingerprintExplorerDocumentInput<'_>,
) -> ExplorerDocumentFingerprint {
    let mut hasher = DefaultHasher::new();

    input.document_id.hash(&mut hasher);
    input.stale.hash(&mut hasher);
    input.content.hash(&mut hasher);
    hash_detail_components(input.components, &mut hasher);
    hash_detail_edges(input.graph, input.document_id, &mut hasher);
    hash_detail_evidence(
        input.document_id,
        input.evidence_by_target,
        input.evidence_record_lookup,
        input.project_root,
        &mut hasher,
    );

    hasher.finish()
}

/// Compute an [`ExplorerChangedEvent`] by diffing two snapshots.
#[must_use]
pub fn diff_explorer_snapshots(
    previous: Option<&ExplorerSnapshot>,
    current: &ExplorerSnapshot,
) -> ExplorerChangedEvent {
    let previous_docs = previous.map(|snapshot| {
        snapshot
            .documents
            .iter()
            .map(|doc| (doc.id.clone(), doc))
            .collect::<HashMap<_, _>>()
    });
    let current_docs: HashMap<String, &ExplorerDocumentSummary> = current
        .documents
        .iter()
        .map(|doc| (doc.id.clone(), doc))
        .collect();
    let previous_neighborhoods = previous.map_or_else(HashMap::new, edge_neighborhoods);
    let current_neighborhoods = edge_neighborhoods(current);
    let empty_neighborhood = BTreeSet::new();
    let (changed, removed) = diff_keyed_documents(
        previous_docs.as_ref(),
        &current_docs,
        |doc_id, previous_doc, current_doc| {
            previous_doc != current_doc
                || previous_neighborhoods
                    .get(doc_id)
                    .unwrap_or(&empty_neighborhood)
                    != current_neighborhoods
                        .get(doc_id)
                        .unwrap_or(&empty_neighborhood)
        },
    );

    ExplorerChangedEvent {
        revision: current.revision.clone(),
        changed_document_ids: changed.into_iter().collect(),
        removed_document_ids: removed.into_iter().collect(),
    }
}

/// Fingerprint a rendered document-components payload for diffing.
#[must_use]
pub fn fingerprint_document_components(
    document_components: &DocumentComponentsResult,
) -> ExplorerDocumentFingerprint {
    let mut hasher = DefaultHasher::new();
    if let Ok(encoded) = serde_json::to_vec(document_components) {
        encoded.hash(&mut hasher);
    } else {
        // This payload is serde-serializable by construction, so this path
        // should stay unreachable. Keep a stable fallback instead of
        // panicking in the invalidation hot path.
        document_components.document_id.hash(&mut hasher);
        document_components.stale.hash(&mut hasher);
        document_components.project.hash(&mut hasher);
    }
    hasher.finish()
}

/// Compute document IDs whose explorer detail payload changed.
#[must_use]
pub fn diff_explorer_documents<PreviousHasher, CurrentHasher>(
    previous: Option<&HashMap<String, ExplorerDocumentFingerprint, PreviousHasher>>,
    current: &HashMap<String, ExplorerDocumentFingerprint, CurrentHasher>,
) -> BTreeSet<String>
where
    PreviousHasher: std::hash::BuildHasher,
    CurrentHasher: std::hash::BuildHasher,
{
    diff_keyed_documents(previous, current, |_doc_id, previous_doc, current_doc| {
        previous_doc != current_doc
    })
    .0
}

fn hash_detail_components(components: &[ExtractedComponent], hasher: &mut DefaultHasher) {
    components.len().hash(hasher);
    for component in components {
        component.name.hash(hasher);
        let mut attributes = component.attributes.iter().collect::<Vec<_>>();
        attributes.sort_by(|(left_key, _), (right_key, _)| left_key.cmp(right_key));
        for (key, value) in attributes {
            key.hash(hasher);
            value.hash(hasher);
        }
        component.body_text.hash(hasher);
        component.position.byte_offset.hash(hasher);
        component.position.line.hash(hasher);
        component.position.column.hash(hasher);
        component.end_position.byte_offset.hash(hasher);
        component.end_position.line.hash(hasher);
        component.end_position.column.hash(hasher);
        hash_detail_components(&component.children, hasher);
    }
}

fn hash_detail_edges(graph: &DocumentGraph, doc_id: &str, hasher: &mut DefaultHasher) {
    let mut outgoing_edges = graph
        .edges()
        .filter(|(source, _, _)| *source == doc_id)
        .map(|(_, target, kind)| (target.to_owned(), kind.as_str().to_owned()))
        .collect::<Vec<_>>();
    outgoing_edges.sort();
    outgoing_edges.hash(hasher);
}

fn hash_detail_evidence(
    doc_id: &str,
    evidence_by_target: Option<&EvidenceIndex>,
    evidence_record_lookup: Option<&HashMap<EvidenceId, &VerificationEvidenceRecord>>,
    project_root: &Path,
    hasher: &mut DefaultHasher,
) {
    evidence_by_target.is_some().hash(hasher);
    evidence_record_lookup.is_some().hash(hasher);

    let Some(doc_targets) = evidence_by_target.and_then(|index| index.get(doc_id)) else {
        return;
    };

    let mut target_ids = doc_targets.keys().cloned().collect::<Vec<_>>();
    target_ids.sort();
    for target_id in target_ids {
        target_id.hash(hasher);
        let evidence_ids = doc_targets
            .get(target_id.as_str())
            .expect("target id should exist after sorting");
        if let Some(record_lookup) = evidence_record_lookup {
            let mut record_fingerprints = Vec::with_capacity(evidence_ids.len());
            let mut missing_records = 0_usize;
            for evidence_id in evidence_ids {
                if let Some(record) = record_lookup.get(evidence_id) {
                    record_fingerprints
                        .push(explorer_evidence_record_fingerprint(record, project_root));
                } else {
                    missing_records += 1;
                }
            }
            record_fingerprints.sort_unstable();
            evidence_ids.len().hash(hasher);
            missing_records.hash(hasher);
            record_fingerprints.hash(hasher);
        } else {
            evidence_ids.len().hash(hasher);
        }
    }
}

fn explorer_evidence_record_fingerprint(
    record: &VerificationEvidenceRecord,
    project_root: &Path,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    record.test.name.hash(&mut hasher);
    relativize(&record.test.file, project_root).hash(&mut hasher);
    map_test_kind(record.test.kind).hash(&mut hasher);
    record.source_location.line.hash(&mut hasher);
    let mut provenance_fingerprints = record
        .provenance
        .iter()
        .map(|provenance| explorer_provenance_fingerprint(provenance, project_root))
        .collect::<Vec<_>>();
    provenance_fingerprints.sort_unstable();
    provenance_fingerprints.hash(&mut hasher);
    hasher.finish()
}

fn explorer_provenance_fingerprint(
    provenance: &supersigil_evidence::PluginProvenance,
    project_root: &Path,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    map_provenance(provenance, project_root).hash(&mut hasher);
    hasher.finish()
}

fn diff_keyed_documents<PreviousValue, CurrentValue, PreviousHasher, CurrentHasher, F>(
    previous: Option<&HashMap<String, PreviousValue, PreviousHasher>>,
    current: &HashMap<String, CurrentValue, CurrentHasher>,
    mut differs: F,
) -> (BTreeSet<String>, BTreeSet<String>)
where
    PreviousHasher: std::hash::BuildHasher,
    CurrentHasher: std::hash::BuildHasher,
    F: FnMut(&str, &PreviousValue, &CurrentValue) -> bool,
{
    let Some(previous) = previous else {
        return (current.keys().cloned().collect(), BTreeSet::new());
    };

    let mut changed = BTreeSet::new();
    let mut removed = BTreeSet::new();
    let all_doc_ids = previous
        .keys()
        .chain(current.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for doc_id in all_doc_ids {
        match (previous.get(&doc_id), current.get(&doc_id)) {
            (None, Some(_)) => {
                changed.insert(doc_id);
            }
            (Some(_), None) => {
                removed.insert(doc_id);
            }
            (Some(previous_doc), Some(current_doc))
                if differs(doc_id.as_str(), previous_doc, current_doc) =>
            {
                changed.insert(doc_id);
            }
            _ => {}
        }
    }

    (changed, removed)
}

fn build_document_summary(
    id: &str,
    doc: &SpecDocument,
    graph: &DocumentGraph,
    evidence_by_target: Option<&EvidenceIndex>,
    project_root: &Path,
) -> ExplorerDocumentSummary {
    let title = doc
        .extra
        .get("title")
        .and_then(|value| value.as_str())
        .map_or_else(|| id.to_owned(), str::to_owned);

    let coverage_summary = count_document_coverage(id, &doc.components, evidence_by_target);
    let graph_components = collect_graph_components(&doc.components);
    let component_count = graph_components.len();

    ExplorerDocumentSummary {
        id: id.to_owned(),
        doc_type: doc.frontmatter.doc_type.clone(),
        status: doc.frontmatter.status.clone(),
        title,
        path: relative_path_string(&doc.path, project_root),
        file_uri: file_uri(&doc.path),
        project: graph.doc_project(id).map(str::to_owned),
        coverage_summary,
        component_count,
        graph_components,
    }
}

fn count_document_coverage(
    doc_id: &str,
    components: &[ExtractedComponent],
    evidence_by_target: Option<&EvidenceIndex>,
) -> CoverageSummary {
    fn visit(
        doc_id: &str,
        components: &[ExtractedComponent],
        evidence_by_target: Option<&EvidenceIndex>,
        summary: &mut CoverageSummary,
    ) {
        for component in components {
            if component.name == CRITERION {
                summary.total += 1;
                let verified = component
                    .attributes
                    .get("id")
                    .and_then(|target_id| {
                        evidence_by_target
                            .and_then(|index| index.get(doc_id))
                            .and_then(|targets| targets.get(target_id))
                    })
                    .is_some_and(|evidence| !evidence.is_empty());
                if verified {
                    summary.verified += 1;
                }
            }
            visit(doc_id, &component.children, evidence_by_target, summary);
        }
    }

    let mut summary = CoverageSummary {
        verified: 0,
        total: 0,
    };
    visit(doc_id, components, evidence_by_target, &mut summary);
    summary
}

fn collect_graph_components(components: &[ExtractedComponent]) -> Vec<ExplorerGraphComponent> {
    let mut graph_components = Vec::new();
    collect_graph_components_inner(components, None, &mut graph_components);
    graph_components
}

fn collect_graph_components_inner(
    components: &[ExtractedComponent],
    decision_parent_id: Option<&str>,
    output: &mut Vec<ExplorerGraphComponent>,
) {
    for (index, component) in components.iter().enumerate() {
        let explicit_id = component.attributes.get("id").cloned();
        let generated_child_id = if explicit_id.is_none()
            && matches!(component.name.as_str(), RATIONALE | ALTERNATIVE)
        {
            decision_parent_id
                .map(|parent| format!("{parent}-{}-{index}", component.name.to_ascii_lowercase()))
        } else {
            None
        };
        let component_id = explicit_id.or(generated_child_id);

        let next_decision_parent = if component.name == DECISION {
            component_id.clone()
        } else {
            decision_parent_id.map(str::to_owned)
        };

        if is_graph_visible_kind(&component.name)
            && let Some(component_id) = component_id
        {
            output.push(ExplorerGraphComponent {
                id: component_id.clone(),
                kind: component.name.clone(),
                body: component.body_text.clone(),
                parent_component_id: decision_parent_id.map(str::to_owned),
                implements: parse_implements(component),
            });
        }

        collect_graph_components_inner(
            &component.children,
            next_decision_parent.as_deref(),
            output,
        );
    }
}

fn is_graph_visible_kind(kind: &str) -> bool {
    matches!(kind, CRITERION | TASK | DECISION | RATIONALE | ALTERNATIVE)
}

fn parse_implements(component: &ExtractedComponent) -> Option<Vec<String>> {
    if component.name != TASK {
        return None;
    }

    component.attributes.get("implements").map(|refs| {
        refs.split(',')
            .map(str::trim)
            .filter(|ref_value| !ref_value.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

fn edge_neighborhoods(
    snapshot: &ExplorerSnapshot,
) -> HashMap<&str, BTreeSet<(String, String, String)>> {
    let mut neighborhoods = HashMap::new();

    for edge in &snapshot.edges {
        neighborhoods
            .entry(edge.from.as_str())
            .or_insert_with(BTreeSet::new)
            .insert(("out".to_owned(), edge.to.clone(), edge.kind.clone()));
        neighborhoods
            .entry(edge.to.as_str())
            .or_insert_with(BTreeSet::new)
            .insert(("in".to_owned(), edge.from.clone(), edge.kind.clone()));
    }

    neighborhoods
}

fn relative_path_string(path: &Path, project_root: &Path) -> String {
    relative_path(path, project_root)
        .as_deref()
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn relative_path(path: &Path, project_root: &Path) -> Option<PathBuf> {
    if project_root.as_os_str().is_empty() {
        return None;
    }

    path.strip_prefix(project_root)
        .map(Path::to_path_buf)
        .ok()
        .or_else(|| pathdiff::diff_paths(path, project_root))
}

fn file_uri(path: &Path) -> Option<String> {
    url::Url::from_file_path(path).ok().map(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_without_previous_marks_all_current_documents_changed() {
        let snapshot = ExplorerSnapshot {
            revision: "rev-1".to_owned(),
            documents: vec![ExplorerDocumentSummary {
                id: "auth/req".to_owned(),
                doc_type: Some("requirements".to_owned()),
                status: Some("draft".to_owned()),
                title: "auth/req".to_owned(),
                path: "specs/auth/req.md".to_owned(),
                file_uri: None,
                project: None,
                coverage_summary: CoverageSummary {
                    verified: 0,
                    total: 0,
                },
                component_count: 0,
                graph_components: Vec::new(),
            }],
            edges: Vec::new(),
        };

        let changed = diff_explorer_snapshots(None, &snapshot);

        assert_eq!(changed.revision, "rev-1");
        assert_eq!(changed.changed_document_ids, vec!["auth/req".to_owned()]);
        assert!(changed.removed_document_ids.is_empty());
    }

    #[test]
    fn build_explorer_document_discards_legacy_project_field() {
        let document_components = DocumentComponentsResult {
            document_id: "auth/req".to_owned(),
            stale: false,
            project: Some("workspace".to_owned()),
            fences: Vec::new(),
            edges: Vec::new(),
        };

        let document = build_explorer_document(&BuildExplorerDocumentInput {
            revision: "rev-3",
            document_components,
        });

        assert_eq!(document.revision, "rev-3");
        assert_eq!(document.document_id, "auth/req");
        assert!(!document.stale);
        assert!(document.fences.is_empty());
        assert!(document.edges.is_empty());
    }
}
