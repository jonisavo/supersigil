//! Integration tests for explorer runtime payload builders.

use std::collections::HashMap;
use std::path::Path;

use supersigil_core::{ComponentDefs, Config, ParseResult, SpecDocument, build_graph};
use supersigil_evidence::EvidenceId;
use supersigil_rust::verifies;
use supersigil_verify::document_components::{
    BuildComponentsInput, build_document_components, sample_document_components_result,
};
use supersigil_verify::explorer_runtime::{
    BuildExplorerDocumentInput, BuildExplorerSnapshotInput, CoverageSummary, ExplorerChangedEvent,
    ExplorerDocument, ExplorerGraphComponent, ExplorerSnapshot, build_explorer_document,
    build_explorer_snapshot, diff_explorer_documents, diff_explorer_snapshots,
    fingerprint_document_components,
};

fn build_evidence(
    doc_id: &str,
    target_id: &str,
) -> HashMap<String, HashMap<String, Vec<EvidenceId>>> {
    let mut evidence = HashMap::new();
    evidence
        .entry(doc_id.to_owned())
        .or_insert_with(HashMap::new)
        .insert(target_id.to_owned(), vec![EvidenceId::new(0)]);
    evidence
}

fn parse_doc(path: &Path, content: &str) -> SpecDocument {
    let defs = ComponentDefs::defaults();
    let recovered = supersigil_parser::parse_content_recovering(path, content, &defs)
        .expect("parse should succeed");
    match recovered.result {
        ParseResult::Document(doc) => doc,
        ParseResult::NotSupersigil(_) => panic!("expected supersigil document"),
    }
}

fn build_graph_from_docs(docs: Vec<SpecDocument>) -> supersigil_core::DocumentGraph {
    build_graph(docs, &Config::default()).expect("graph should build")
}

#[test]
#[verifies(
    "graph-explorer-runtime/req#req-1-3",
    "graph-explorer-runtime/req#req-5-1",
    "graph-explorer-runtime/req#req-5-2",
    "graph-explorer-runtime/req#req-5-3"
)]
fn explorer_snapshot_includes_coverage_and_graph_component_outline() {
    let doc = parse_doc(
        Path::new("specs/auth/req.md"),
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
<Task id=\"task-1\" implements=\"auth/req#auth-1\">Implement authentication.</Task>
<Decision id=\"decision-1\">
  <Rationale>Use a standards-based protocol.</Rationale>
  <Alternative id=\"alt-1\">Password-only auth.</Alternative>
</Decision>
```
",
    );
    let graph = build_graph_from_docs(vec![doc]);
    let evidence = build_evidence("auth/req", "auth-1");

    let snapshot = build_explorer_snapshot(&BuildExplorerSnapshotInput {
        revision: "rev-7",
        graph: &graph,
        evidence_by_target: Some(&evidence),
        project_root: Path::new(""),
    });

    assert_eq!(snapshot.revision, "rev-7");
    assert_eq!(snapshot.documents.len(), 1);
    assert!(snapshot.edges.is_empty());

    let doc = &snapshot.documents[0];
    assert_eq!(doc.id, "auth/req");
    assert_eq!(
        doc.coverage_summary,
        CoverageSummary {
            verified: 1,
            total: 1,
        }
    );
    assert_eq!(doc.component_count, 5);

    let criterion = doc
        .graph_components
        .iter()
        .find(|component| component.id == "auth-1")
        .expect("criterion should be present in graph outline");
    assert_eq!(criterion.kind, "Criterion");
    assert_eq!(criterion.parent_component_id, None);

    let task = doc
        .graph_components
        .iter()
        .find(|component| component.id == "task-1")
        .expect("task should be present in graph outline");
    assert_eq!(task.kind, "Task");
    assert_eq!(
        task.implements.as_ref(),
        Some(&vec!["auth/req#auth-1".to_owned()])
    );

    let rationale = doc
        .graph_components
        .iter()
        .find(|component| component.kind == "Rationale")
        .expect("decision child should be flattened into graph outline");
    assert_eq!(rationale.parent_component_id.as_deref(), Some("decision-1"));
}

#[test]
#[verifies(
    "graph-explorer-runtime/req#req-1-3",
    "graph-explorer-runtime/req#req-5-2"
)]
fn explorer_snapshot_counts_id_less_criteria_in_coverage_totals() {
    let doc = parse_doc(
        Path::new("specs/auth/req.md"),
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: draft
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion>Missing id but still unverified.</Criterion>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
```
",
    );
    let graph = build_graph_from_docs(vec![doc]);
    let evidence = build_evidence("auth/req", "auth-1");

    let snapshot = build_explorer_snapshot(&BuildExplorerSnapshotInput {
        revision: "rev-8",
        graph: &graph,
        evidence_by_target: Some(&evidence),
        project_root: Path::new(""),
    });

    assert_eq!(snapshot.revision, "rev-8");
    assert_eq!(snapshot.documents.len(), 1);
    assert_eq!(
        snapshot.documents[0].coverage_summary,
        CoverageSummary {
            verified: 1,
            total: 2,
        }
    );
}

#[test]
#[verifies("graph-explorer-runtime/req#req-1-2")]
fn explorer_document_wraps_rendered_document_with_revision() {
    let result = sample_document_components_result();

    let explorer_document = build_explorer_document(&BuildExplorerDocumentInput {
        revision: "rev-11",
        document_components: result.clone(),
    });

    assert_eq!(
        explorer_document,
        ExplorerDocument {
            revision: "rev-11".to_owned(),
            document_id: result.document_id,
            stale: result.stale,
            fences: result.fences,
            edges: result.edges,
        }
    );
}

#[test]
#[verifies("graph-explorer-runtime/req#req-1-4")]
fn diff_snapshots_reports_changed_and_removed_documents() {
    let old = ExplorerSnapshot {
        revision: "rev-1".to_owned(),
        documents: vec![
            supersigil_verify::explorer_runtime::ExplorerDocumentSummary {
                id: "auth/req".to_owned(),
                doc_type: Some("requirements".to_owned()),
                status: Some("approved".to_owned()),
                title: "auth/req".to_owned(),
                path: "specs/auth/req.md".to_owned(),
                file_uri: None,
                project: None,
                coverage_summary: CoverageSummary {
                    verified: 0,
                    total: 1,
                },
                component_count: 1,
                graph_components: vec![ExplorerGraphComponent {
                    id: "auth-1".to_owned(),
                    kind: "Criterion".to_owned(),
                    body: Some("Users SHALL authenticate.".to_owned()),
                    parent_component_id: None,
                    implements: None,
                }],
            },
            supersigil_verify::explorer_runtime::ExplorerDocumentSummary {
                id: "auth/design".to_owned(),
                doc_type: Some("design".to_owned()),
                status: Some("draft".to_owned()),
                title: "auth/design".to_owned(),
                path: "specs/auth/design.md".to_owned(),
                file_uri: None,
                project: None,
                coverage_summary: CoverageSummary {
                    verified: 0,
                    total: 0,
                },
                component_count: 0,
                graph_components: Vec::new(),
            },
        ],
        edges: vec![supersigil_verify::explorer_runtime::ExplorerEdge {
            from: "auth/design".to_owned(),
            to: "auth/req".to_owned(),
            kind: "Implements".to_owned(),
        }],
    };

    let new = ExplorerSnapshot {
        revision: "rev-2".to_owned(),
        documents: vec![
            supersigil_verify::explorer_runtime::ExplorerDocumentSummary {
                id: "auth/req".to_owned(),
                doc_type: Some("requirements".to_owned()),
                status: Some("approved".to_owned()),
                title: "auth/req".to_owned(),
                path: "specs/auth/req.md".to_owned(),
                file_uri: None,
                project: None,
                coverage_summary: CoverageSummary {
                    verified: 1,
                    total: 1,
                },
                component_count: 1,
                graph_components: vec![ExplorerGraphComponent {
                    id: "auth-1".to_owned(),
                    kind: "Criterion".to_owned(),
                    body: Some("Users SHALL authenticate.".to_owned()),
                    parent_component_id: None,
                    implements: None,
                }],
            },
        ],
        edges: Vec::new(),
    };

    let changed = diff_explorer_snapshots(Some(&old), &new);

    assert_eq!(
        changed,
        ExplorerChangedEvent {
            revision: "rev-2".to_owned(),
            changed_document_ids: vec!["auth/req".to_owned()],
            removed_document_ids: vec!["auth/design".to_owned()],
        }
    );
}

#[test]
#[verifies("graph-explorer-runtime/req#req-1-4")]
fn diff_documents_marks_detail_only_changes_as_changed() {
    let previous = sample_document_components_result();
    let mut current = previous.clone();
    current.stale = true;

    let previous = HashMap::from([(
        previous.document_id.clone(),
        fingerprint_document_components(&previous),
    )]);
    let current = HashMap::from([(
        current.document_id.clone(),
        fingerprint_document_components(&current),
    )]);

    let changed = diff_explorer_documents(Some(&previous), &current);

    assert_eq!(
        changed,
        ["auth/req".to_owned()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
}

#[test]
fn explorer_document_builder_matches_document_components_builder_output() {
    let content = "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
```
";
    let defs = supersigil_core::ComponentDefs::defaults();
    let recovered =
        supersigil_parser::parse_content_recovering(Path::new("specs/auth/req.md"), content, &defs)
            .expect("parse should succeed");
    let doc = match recovered.result {
        supersigil_core::ParseResult::Document(doc) => doc,
        supersigil_core::ParseResult::NotSupersigil(_) => panic!("expected supersigil document"),
    };
    let graph = build_graph_from_docs(vec![doc.clone()]);

    let rendered = build_document_components(&BuildComponentsInput {
        doc: &doc,
        stale: false,
        content,
        graph: &graph,
        evidence_by_target: None,
        evidence_records: None,
        project_root: Path::new(""),
    });

    let explorer_document = build_explorer_document(&BuildExplorerDocumentInput {
        revision: "rev-3",
        document_components: rendered.clone(),
    });

    assert_eq!(explorer_document.document_id, rendered.document_id);
    assert_eq!(explorer_document.fences, rendered.fences);
    assert_eq!(explorer_document.edges, rendered.edges);
}
