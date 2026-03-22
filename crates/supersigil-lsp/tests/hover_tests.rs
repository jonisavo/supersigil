//! Integration tests for hover (req-4-1, req-4-2).

use std::collections::HashMap;
use std::path::PathBuf;

use supersigil_core::{
    Config, ExtractedComponent, Frontmatter, SourcePosition, SpecDocument, build_graph,
};
use supersigil_lsp::hover::{hover_at_position, hover_component, hover_ref};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn pos(line: usize) -> SourcePosition {
    SourcePosition {
        byte_offset: line * 40,
        line,
        column: 1,
    }
}

fn make_criterion(id: &str, body: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "Criterion".to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children: Vec::new(),
        body_text: Some(body.to_owned()),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: pos(line),
    }
}

fn make_doc(
    id: &str,
    path: &str,
    doc_type: Option<&str>,
    status: Option<&str>,
    title: Option<&str>,
    components: Vec<ExtractedComponent>,
) -> SpecDocument {
    let mut extra = HashMap::new();
    if let Some(t) = title {
        extra.insert("title".to_owned(), yaml_serde::Value::String(t.to_owned()));
    }
    SpecDocument {
        path: PathBuf::from(path),
        frontmatter: Frontmatter {
            id: id.to_owned(),
            doc_type: doc_type.map(str::to_owned),
            status: status.map(str::to_owned),
        },
        extra,
        components,
        warnings: vec![],
    }
}

fn default_config() -> Config {
    Config {
        paths: Some(vec!["specs/**/*.md".to_owned()]),
        ..Config::default()
    }
}

fn hover_text(h: &lsp_types::Hover) -> &str {
    match &h.contents {
        lsp_types::HoverContents::Markup(mc) => &mc.value,
        _ => panic!("expected MarkupContent hover"),
    }
}

/// Wrap content in a `supersigil-xml` fence. Lines inside start at line 1.
fn fenced(inner: &str) -> String {
    format!("```supersigil-xml\n{inner}\n```")
}

// ---------------------------------------------------------------------------
// hover_component — req-4-1
// ---------------------------------------------------------------------------

#[test]
fn hover_criterion_contains_required_fields() {
    let defs = supersigil_core::ComponentDefs::defaults();
    let h = hover_component("Criterion", &defs).expect("should return hover");
    let text = hover_text(&h);

    assert!(text.contains("Criterion"), "should contain component name");
    assert!(text.contains("id"), "should mention the id attribute");
    assert!(text.contains("yes"), "required=yes should appear");
    assert!(
        text.contains("Referenceable: yes"),
        "should show referenceable"
    );
    assert!(text.contains("Verifiable: yes"), "should show verifiable");
}

#[test]
fn hover_verified_by_contains_all_attributes() {
    let defs = supersigil_core::ComponentDefs::defaults();
    let h = hover_component("VerifiedBy", &defs).expect("should return hover");
    let text = hover_text(&h);

    assert!(text.contains("VerifiedBy"), "should contain component name");
    assert!(text.contains("strategy"), "should mention strategy attr");
    assert!(text.contains("tag"), "should mention tag attr");
    assert!(text.contains("paths"), "should mention paths attr");
    assert!(
        text.contains("Referenceable: no"),
        "should show not referenceable"
    );
    assert!(
        text.contains("Verifiable: no"),
        "should show not verifiable"
    );
}

#[test]
fn hover_nonexistent_component_returns_none() {
    let defs = supersigil_core::ComponentDefs::defaults();
    let result = hover_component("NonExistent", &defs);
    assert!(result.is_none());
}

#[test]
fn hover_component_with_description_includes_it() {
    let defs = supersigil_core::ComponentDefs::defaults();
    let h = hover_component("Criterion", &defs).expect("should return hover");
    let text = hover_text(&h);
    // Criterion has a description in the defaults.
    assert!(
        text.len() > 50,
        "hover with description should have substantial content"
    );
}

#[test]
fn hover_acceptance_criteria_no_attributes_table() {
    let defs = supersigil_core::ComponentDefs::defaults();
    let h = hover_component("AcceptanceCriteria", &defs).expect("should return hover");
    let text = hover_text(&h);
    // AcceptanceCriteria has no attributes — should say so.
    assert!(
        text.contains("No attributes"),
        "should indicate no attributes"
    );
}

// ---------------------------------------------------------------------------
// hover_ref — fragment ref (req-4-2)
// ---------------------------------------------------------------------------

#[test]
fn fragment_ref_hover_shows_title_and_body() {
    let criterion = make_criterion("req-1-1", "User logs in successfully.", 5);
    let doc = make_doc(
        "auth/req",
        "/specs/auth/req.md",
        Some("requirements"),
        Some("approved"),
        Some("User Authentication"),
        vec![criterion],
    );

    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");
    let h = hover_ref("auth/req#req-1-1", &graph).expect("should return hover");
    let text = hover_text(&h);

    assert!(
        text.contains("User Authentication"),
        "should contain doc title"
    );
    assert!(text.contains("Criterion"), "should contain component kind");
    assert!(text.contains("req-1-1"), "should contain fragment id");
    assert!(
        text.contains("User logs in successfully"),
        "should contain criterion body"
    );
    // Fragment hover should NOT show type/status — that's for doc-level hover.
    assert!(
        !text.contains("requirements"),
        "fragment hover should not show doc type"
    );
}

// ---------------------------------------------------------------------------
// hover_ref — document-level ref (req-4-2)
// ---------------------------------------------------------------------------

#[test]
fn document_ref_hover_shows_title_and_status() {
    let doc = make_doc(
        "design/session",
        "/specs/design/session.md",
        Some("design"),
        Some("draft"),
        Some("Session Management"),
        vec![],
    );

    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");
    let h = hover_ref("design/session", &graph).expect("should return hover");
    let text = hover_text(&h);

    assert!(
        text.contains("Session Management"),
        "should contain doc title"
    );
    assert!(text.contains("design"), "should contain doc type");
    assert!(text.contains("draft"), "should contain doc status");
}

#[test]
fn nonexistent_ref_returns_none() {
    let graph = build_graph(vec![], &default_config()).expect("graph must build");
    let result = hover_ref("no/such/doc", &graph);
    assert!(result.is_none());
}

#[test]
fn nonexistent_fragment_ref_returns_none() {
    let doc = make_doc("auth/req", "/specs/auth/req.md", None, None, None, vec![]);
    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");
    let result = hover_ref("auth/req#missing-frag", &graph);
    assert!(result.is_none());
}

#[test]
fn doc_without_title_falls_back_to_id() {
    let doc = make_doc(
        "auth/req",
        "/specs/auth/req.md",
        Some("requirements"),
        Some("draft"),
        None, // no title
        vec![],
    );
    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");
    let h = hover_ref("auth/req", &graph).expect("should return hover");
    let text = hover_text(&h);
    // Falls back to doc ID as title.
    assert!(text.contains("auth/req"));
}

// ---------------------------------------------------------------------------
// hover_at_position — component name detection
// ---------------------------------------------------------------------------

#[test]
fn hover_at_position_on_component_name() {
    let content = fenced("<Criterion id=\"req-1\">\nsome body\n</Criterion>");
    let defs = supersigil_core::ComponentDefs::defaults();
    let graph = build_graph(vec![], &default_config()).expect("graph must build");

    // Line 1 = <Criterion ...> (inside fence), character 3 is inside the name.
    let h = hover_at_position(&content, 1, 3, &defs, &graph).expect("should return hover");
    let text = hover_text(&h);
    assert!(text.contains("Criterion"));
}

#[test]
fn hover_at_position_on_ref_string() {
    let content = fenced("<References refs=\"auth/req#req-1\" />");
    let defs = supersigil_core::ComponentDefs::defaults();

    let criterion = make_criterion("req-1", "test body", 5);
    let doc = make_doc(
        "auth/req",
        "/specs/auth/req.md",
        Some("requirements"),
        Some("approved"),
        Some("Auth Requirements"),
        vec![criterion],
    );
    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");

    // Line 1 inside fence, position 22 is inside "auth/req#req-1".
    let h = hover_at_position(&content, 1, 22, &defs, &graph).expect("should return hover");
    let text = hover_text(&h);
    assert!(text.contains("Auth Requirements"), "should show doc title");
    assert!(text.contains("Criterion"), "should show component kind");
}

#[test]
fn hover_at_position_on_whitespace_returns_none() {
    let content = "  some plain text  ";
    let defs = supersigil_core::ComponentDefs::defaults();
    let graph = build_graph(vec![], &default_config()).expect("graph must build");
    let result = hover_at_position(content, 0, 0, &defs, &graph);
    assert!(result.is_none());
}

#[test]
fn hover_at_position_outside_fence_returns_none() {
    // Component name outside a fence should not trigger hover.
    let content = "<Criterion id=\"req-1\">\nsome body\n</Criterion>";
    let defs = supersigil_core::ComponentDefs::defaults();
    let graph = build_graph(vec![], &default_config()).expect("graph must build");
    let result = hover_at_position(content, 0, 3, &defs, &graph);
    assert!(result.is_none());
}
