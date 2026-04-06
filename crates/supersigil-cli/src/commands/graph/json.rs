//! JSON serialization of the document graph matching the `Graph_JSON` schema.

use std::io::{self, Write};

use serde::Serialize;
use supersigil_core::{DECISION, DocumentGraph, EdgeKind, ExtractedComponent, SpecDocument, TASK};

// ---------------------------------------------------------------------------
// Graph_JSON schema types
// ---------------------------------------------------------------------------

/// Top-level JSON output for the document graph.
#[derive(Debug, Serialize)]
pub struct GraphJson {
    pub documents: Vec<DocumentNode>,
    pub edges: Vec<Edge>,
}

/// A document node in the JSON graph.
#[derive(Debug, Serialize)]
pub struct DocumentNode {
    pub id: String,
    pub doc_type: Option<String>,
    pub status: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub components: Vec<Component>,
}

/// A component in the JSON graph.
#[derive(Debug, Serialize)]
pub struct Component {
    pub id: Option<String>,
    pub kind: String,
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Component>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implements: Option<Vec<String>>,
}

/// An edge in the JSON graph.
#[derive(Debug, Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/// Build the JSON graph representation from a `DocumentGraph`.
pub fn build_graph_json(graph: &DocumentGraph) -> GraphJson {
    let mut documents: Vec<DocumentNode> = graph
        .documents()
        .map(|(id, doc)| build_document_node(id, doc, graph))
        .collect();

    // Sort for deterministic output.
    documents.sort_by(|a, b| a.id.cmp(&b.id));

    let mut edges: Vec<Edge> = graph
        .edges()
        .map(|(from, to, kind)| Edge {
            from: from.to_owned(),
            to: to.to_owned(),
            kind: edge_kind_label(kind),
        })
        .collect();

    // Sort for deterministic output.
    edges.sort_by(|a, b| (&a.from, &a.to, &a.kind).cmp(&(&b.from, &b.to, &b.kind)));

    // Deduplicate edges (references_reverse may have multiple fragment entries
    // for the same source→target pair).
    edges.dedup_by(|a, b| a.from == b.from && a.to == b.to && a.kind == b.kind);

    GraphJson { documents, edges }
}

fn build_document_node(id: &str, doc: &SpecDocument, graph: &DocumentGraph) -> DocumentNode {
    let title = doc
        .extra
        .get("title")
        .and_then(|v| v.as_str())
        .map_or_else(|| id.to_owned(), str::to_owned);

    let components = doc.components.iter().map(build_component).collect();
    let project = graph.doc_project(id).map(str::to_owned);

    DocumentNode {
        id: id.to_owned(),
        doc_type: doc.frontmatter.doc_type.clone(),
        status: doc.frontmatter.status.clone(),
        title,
        project,
        components,
    }
}

fn build_component(comp: &ExtractedComponent) -> Component {
    let children = (comp.name == DECISION && !comp.children.is_empty())
        .then(|| comp.children.iter().map(build_component).collect());

    let implements = if comp.name == TASK {
        comp.attributes.get("implements").map(|refs| {
            refs.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
    } else {
        None
    };

    Component {
        id: comp.attributes.get("id").cloned(),
        kind: comp.name.clone(),
        body: comp.body_text.clone(),
        children,
        implements,
    }
}

fn edge_kind_label(kind: EdgeKind) -> String {
    kind.as_str().to_owned()
}

/// Write the JSON graph to `out`. Returns the edge count.
pub fn write_json(out: &mut impl Write, graph: &DocumentGraph) -> io::Result<usize> {
    let json = build_graph_json(graph);
    let edge_count = json.edges.len();
    serde_json::to_writer_pretty(&mut *out, &json).map_err(io::Error::other)?;
    writeln!(out)?;
    Ok(edge_count)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use supersigil_core::Frontmatter;
    use supersigil_rust::verifies;
    use supersigil_verify::test_helpers::{
        build_test_graph, make_alternative, make_criterion, make_decision_with_id, make_doc,
        make_doc_typed, make_implements, make_rationale, make_references, make_task, pos,
    };

    // -- req-1-2: DocumentNode fields ----------------------------------------

    #[test]
    #[verifies("graph-explorer/req#req-1-2")]
    fn document_node_includes_id_type_status_title() {
        let docs = vec![make_doc_typed(
            "design/auth",
            "design",
            Some("Draft"),
            vec![make_criterion("auth-1", 5)],
        )];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        assert_eq!(json.documents.len(), 1);
        let node = &json.documents[0];
        assert_eq!(node.id, "design/auth");
        assert_eq!(node.doc_type.as_deref(), Some("design"));
        assert_eq!(node.status.as_deref(), Some("Draft"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-1-2")]
    fn title_from_extra_fallback_to_id() {
        // No title in extra -> falls back to ID
        let docs = vec![make_doc("design/auth", vec![])];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);
        assert_eq!(json.documents[0].title, "design/auth");
    }

    #[test]
    #[verifies("graph-explorer/req#req-1-2")]
    fn title_from_extra_when_present() {
        let doc = SpecDocument {
            path: PathBuf::from("specs/design/auth.md"),
            frontmatter: Frontmatter {
                id: "design/auth".into(),
                doc_type: Some("design".into()),
                status: None,
            },
            extra: HashMap::from([(
                "title".to_owned(),
                yaml_serde::Value::String("Auth System".into()),
            )]),
            components: vec![],
        };
        let graph = build_test_graph(vec![doc]);
        let json = build_graph_json(&graph);
        assert_eq!(json.documents[0].title, "Auth System");
    }

    #[test]
    #[verifies("graph-explorer/req#req-1-2")]
    fn null_doc_type_and_status() {
        let docs = vec![make_doc("design/auth", vec![])];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);
        let node = &json.documents[0];
        assert!(node.doc_type.is_none());
        assert!(node.status.is_none());
    }

    // -- req-1-3: Component fields -------------------------------------------

    #[test]
    #[verifies("graph-explorer/req#req-1-3")]
    fn component_includes_id_kind_body() {
        let docs = vec![make_doc("design/auth", vec![make_criterion("auth-1", 5)])];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let comp = &json.documents[0].components[0];
        assert_eq!(comp.id.as_deref(), Some("auth-1"));
        assert_eq!(comp.kind, "Criterion");
        assert!(comp.body.is_some());
    }

    #[test]
    #[verifies("graph-explorer/req#req-1-3")]
    fn component_id_null_when_absent() {
        // A component with no "id" attribute
        let comp = ExtractedComponent {
            name: "Criterion".into(),
            attributes: HashMap::new(),
            children: vec![],
            body_text: Some("some body".into()),
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: pos(5),
            end_position: pos(5),
        };
        let docs = vec![make_doc("design/auth", vec![comp])];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        assert!(json.documents[0].components[0].id.is_none());
    }

    #[test]
    #[verifies("graph-explorer/req#req-1-3")]
    fn decision_has_nested_children() {
        let decision = make_decision_with_id(
            "dec-1",
            vec![make_rationale(12), make_alternative("alt-1", 15)],
            10,
        );
        let docs = vec![make_doc("design/auth", vec![decision])];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let comp = &json.documents[0].components[0];
        assert_eq!(comp.kind, "Decision");
        let children = comp
            .children
            .as_ref()
            .expect("Decision should have children");
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].kind, "Rationale");
        assert_eq!(children[1].kind, "Alternative");
        assert_eq!(children[1].id.as_deref(), Some("alt-1"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-1-3")]
    fn non_decision_has_no_children_field() {
        let docs = vec![make_doc("design/auth", vec![make_criterion("auth-1", 5)])];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let comp = &json.documents[0].components[0];
        assert!(comp.children.is_none());
    }

    // -- req-1-4: Edges from reverse mappings --------------------------------

    #[test]
    #[verifies("graph-explorer/req#req-1-4")]
    fn edges_from_implements_reverse() {
        let docs = vec![
            make_doc("req/auth", vec![make_criterion("auth-1", 5)]),
            make_doc("design/auth", vec![make_implements("req/auth", 5)]),
        ];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let edge = json
            .edges
            .iter()
            .find(|e| e.kind == "Implements")
            .expect("should have an Implements edge");
        assert_eq!(edge.from, "design/auth");
        assert_eq!(edge.to, "req/auth");
    }

    #[test]
    #[verifies("graph-explorer/req#req-1-4")]
    fn edges_from_references_reverse() {
        let docs = vec![
            make_doc("req/auth", vec![make_criterion("auth-1", 5)]),
            make_doc("design/auth", vec![make_references("req/auth", 5)]),
        ];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let edge = json
            .edges
            .iter()
            .find(|e| e.kind == "References")
            .expect("should have a References edge");
        assert_eq!(edge.from, "design/auth");
        assert_eq!(edge.to, "req/auth");
    }

    // -- Round-trip serialization --------------------------------------------

    #[test]
    #[verifies("graph-explorer/req#req-1-2")]
    fn round_trip_json_serialization() {
        let docs = vec![make_doc_typed(
            "design/auth",
            "design",
            Some("Draft"),
            vec![make_criterion("auth-1", 5)],
        )];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let serialized = serde_json::to_string_pretty(&json).expect("serialization");
        let deserialized: serde_json::Value =
            serde_json::from_str(&serialized).expect("deserialization");

        let doc = &deserialized["documents"][0];
        assert_eq!(doc["id"], "design/auth");
        assert_eq!(doc["doc_type"], "design");
        assert_eq!(doc["status"], "Draft");
    }

    // -- Empty graph ---------------------------------------------------------

    #[test]
    #[verifies("graph-explorer/req#req-1-2")]
    fn empty_graph_produces_empty_arrays() {
        let graph = build_test_graph(vec![]);
        let json = build_graph_json(&graph);

        assert!(json.documents.is_empty());
        assert!(json.edges.is_empty());
    }

    // -- Null fields in JSON -------------------------------------------------

    #[test]
    #[verifies("graph-explorer/req#req-1-2")]
    fn null_fields_serialize_as_null() {
        let docs = vec![make_doc("design/auth", vec![])];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let serialized = serde_json::to_string(&json).expect("serialization");
        let value: serde_json::Value = serde_json::from_str(&serialized).expect("parse");

        let doc = &value["documents"][0];
        assert!(doc["doc_type"].is_null());
        assert!(doc["status"].is_null());
    }

    // -- write_json produces valid output ------------------------------------

    #[test]
    #[verifies("graph-explorer/req#req-1-2")]
    fn write_json_produces_valid_json() {
        let docs = vec![make_doc_typed(
            "req/auth",
            "requirements",
            Some("Approved"),
            vec![make_criterion("auth-1", 5)],
        )];
        let graph = build_test_graph(docs);

        let mut buf = Vec::new();
        let edge_count = write_json(&mut buf, &graph).expect("write");

        assert_eq!(edge_count, 0);
        let output = String::from_utf8(buf).expect("utf-8");
        let _: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
    }

    // -- Task implements attribute --------------------------------------------

    #[test]
    #[verifies("graph-explorer/req#req-1-3")]
    fn task_with_implements_attr() {
        let mut task = make_task("task-1", 10);
        task.attributes
            .insert("implements".into(), "req/auth#auth-1".into());
        let docs = vec![
            make_doc("req/auth", vec![make_criterion("auth-1", 5)]),
            make_doc("tasks/auth", vec![task]),
        ];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let doc_node = json
            .documents
            .iter()
            .find(|d| d.id == "tasks/auth")
            .expect("tasks/auth node");
        let comp = &doc_node.components[0];
        assert_eq!(comp.kind, "Task");
        let impls = comp
            .implements
            .as_ref()
            .expect("Task should have implements");
        assert_eq!(impls, &["req/auth#auth-1"]);
    }

    #[test]
    #[verifies("graph-explorer/req#req-1-3")]
    fn non_task_has_no_implements_field() {
        let docs = vec![make_doc("design/auth", vec![make_criterion("auth-1", 5)])];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let comp = &json.documents[0].components[0];
        assert!(comp.implements.is_none());
    }

    // -- Deterministic output ------------------------------------------------

    #[test]
    #[verifies("graph-explorer/req#req-1-2")]
    fn documents_sorted_by_id() {
        let docs = vec![
            make_doc("z-doc", vec![]),
            make_doc("a-doc", vec![]),
            make_doc("m-doc", vec![]),
        ];
        let graph = build_test_graph(docs);
        let json = build_graph_json(&graph);

        let ids: Vec<&str> = json.documents.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, vec!["a-doc", "m-doc", "z-doc"]);
    }
}
