//! Custom LSP request and notification types for the Spec Explorer tree view.
//!
//! - `supersigil/documentList`: request returning the flat document list
//! - `supersigil/documentsChanged`: notification sent after re-indexing

use std::path::Path;

use lsp_types::{notification::Notification, request::Request};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request: supersigil/documentList
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct DocumentListRequest;

impl Request for DocumentListRequest {
    type Params = serde_json::Value;
    type Result = DocumentListResult;
    const METHOD: &'static str = "supersigil/documentList";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentListResult {
    pub documents: Vec<DocumentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentEntry {
    pub id: String,
    pub doc_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

// ---------------------------------------------------------------------------
// Notification: supersigil/documentsChanged
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct DocumentsChanged;

impl Notification for DocumentsChanged {
    type Params = ();
    const METHOD: &'static str = "supersigil/documentsChanged";
}

// ---------------------------------------------------------------------------
// Builder: extract document entries from graph state
// ---------------------------------------------------------------------------

use supersigil_core::DocumentGraph;

/// Build a flat list of `DocumentEntry` values from the graph.
///
/// `project_root` is used to make absolute document paths relative.
#[must_use]
pub fn build_document_entries(graph: &DocumentGraph, project_root: &Path) -> Vec<DocumentEntry> {
    let mut entries: Vec<DocumentEntry> = graph
        .documents()
        .map(|(id, doc)| {
            let rel_path = doc
                .path
                .strip_prefix(project_root)
                .unwrap_or(&doc.path)
                .to_string_lossy()
                .into_owned();

            let doc_type = doc.frontmatter.doc_type.clone().unwrap_or_default();

            let project = graph.doc_project(id).map(String::from);

            DocumentEntry {
                id: id.to_owned(),
                doc_type,
                status: doc.frontmatter.status.clone(),
                path: rel_path,
                project,
            }
        })
        .collect();

    // Sort for deterministic output (graph iteration order is not guaranteed).
    entries.sort_by(|a, b| a.id.cmp(&b.id));

    entries
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use supersigil_core::{
        Config, Frontmatter, SpecDocument, build_graph,
        test_helpers::{make_doc, single_project_config},
    };

    use supersigil_rust::verifies;

    use super::*;

    fn make_doc_with_type(id: &str, doc_type: &str, status: Option<&str>) -> SpecDocument {
        SpecDocument {
            path: PathBuf::from(format!("specs/{id}.md")),
            frontmatter: Frontmatter {
                id: id.to_owned(),
                doc_type: Some(doc_type.to_owned()),
                status: status.map(str::to_owned),
            },
            extra: HashMap::new(),
            components: Vec::new(),
        }
    }

    #[test]
    #[verifies("spec-explorer/req#req-1-2")]
    fn empty_graph_returns_empty_list() {
        let config = single_project_config();
        let graph = build_graph(vec![], &config).expect("empty graph");
        let entries = build_document_entries(&graph, Path::new("/project"));
        assert!(entries.is_empty());
    }

    #[test]
    #[verifies("spec-explorer/req#req-1-1")]
    fn single_project_returns_documents_with_relative_paths() {
        let docs = vec![
            make_doc_with_type("feature/req", "requirements", Some("approved")),
            make_doc_with_type("feature/design", "design", Some("draft")),
        ];
        let config = single_project_config();
        let graph = build_graph(docs, &config).expect("graph");
        let entries = build_document_entries(&graph, Path::new(""));

        assert_eq!(entries.len(), 2);

        let req = entries.iter().find(|e| e.id == "feature/req").unwrap();
        assert_eq!(req.doc_type, "requirements");
        assert_eq!(req.status.as_deref(), Some("approved"));
        assert_eq!(req.path, "specs/feature/req.md");
        assert_eq!(req.project, None);

        let design = entries.iter().find(|e| e.id == "feature/design").unwrap();
        assert_eq!(design.doc_type, "design");
        assert_eq!(design.status.as_deref(), Some("draft"));
        assert_eq!(design.project, None);
    }

    #[test]
    #[verifies("spec-explorer/req#req-1-1")]
    fn multi_project_returns_project_assignment() {
        let mut doc_a = make_doc_with_type("core/req", "requirements", Some("approved"));
        doc_a.path = PathBuf::from("frontend/specs/core/req.md");

        let mut doc_b = make_doc_with_type("api/req", "requirements", None);
        doc_b.path = PathBuf::from("backend/specs/api/req.md");

        let config = Config {
            projects: Some(HashMap::from([
                (
                    "frontend".to_owned(),
                    supersigil_core::ProjectConfig {
                        paths: vec!["frontend/specs/**/*.md".to_owned()],
                        tests: Vec::new(),
                        isolated: false,
                    },
                ),
                (
                    "backend".to_owned(),
                    supersigil_core::ProjectConfig {
                        paths: vec!["backend/specs/**/*.md".to_owned()],
                        tests: Vec::new(),
                        isolated: false,
                    },
                ),
            ])),
            ..Config::default()
        };

        let graph = build_graph(vec![doc_a, doc_b], &config).expect("graph");
        let entries = build_document_entries(&graph, Path::new(""));

        assert_eq!(entries.len(), 2);

        let core_entry = entries.iter().find(|e| e.id == "core/req").unwrap();
        assert_eq!(core_entry.project.as_deref(), Some("frontend"));

        let api_entry = entries.iter().find(|e| e.id == "api/req").unwrap();
        assert_eq!(api_entry.project.as_deref(), Some("backend"));
    }

    #[test]
    fn entries_are_sorted_by_id() {
        let docs = vec![
            make_doc_with_type("z/req", "requirements", None),
            make_doc_with_type("a/req", "requirements", None),
            make_doc_with_type("m/req", "requirements", None),
        ];
        let config = single_project_config();
        let graph = build_graph(docs, &config).expect("graph");
        let entries = build_document_entries(&graph, Path::new(""));

        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["a/req", "m/req", "z/req"]);
    }

    #[test]
    fn document_without_type_returns_empty_string() {
        let docs = vec![make_doc("standalone", vec![])];
        let config = single_project_config();
        let graph = build_graph(docs, &config).expect("graph");
        let entries = build_document_entries(&graph, Path::new(""));

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].doc_type, "");
        assert_eq!(entries[0].status, None);
    }

    #[test]
    fn absolute_path_is_made_relative() {
        let mut doc = make_doc_with_type("feature/req", "requirements", None);
        doc.path = PathBuf::from("/project/root/specs/feature/req.md");

        let config = single_project_config();
        let graph = build_graph(vec![doc], &config).expect("graph");
        let entries = build_document_entries(&graph, Path::new("/project/root"));

        assert_eq!(entries[0].path, "specs/feature/req.md");
    }
}
