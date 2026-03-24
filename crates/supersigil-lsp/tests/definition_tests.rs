//! Integration tests for go-to-definition (req-2-1, req-2-2, req-2-3).

use std::collections::HashMap;
use std::path::PathBuf;

use supersigil_core::{
    Config, ExtractedComponent, Frontmatter, SourcePosition, SpecDocument, build_graph,
};
use supersigil_lsp::definition::{find_ref_at_position, resolve_ref};

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

fn make_criterion(id: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "Criterion".to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children: Vec::new(),
        body_text: Some(format!("criterion {id}")),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: pos(line),
        end_position: pos(line),
    }
}

fn make_acceptance_criteria(children: Vec<ExtractedComponent>, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "AcceptanceCriteria".to_owned(),
        attributes: HashMap::new(),
        children,
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: pos(line),
        end_position: pos(line),
    }
}

fn make_doc(id: &str, path: &str, components: Vec<ExtractedComponent>) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(path),
        frontmatter: Frontmatter {
            id: id.to_owned(),
            doc_type: None,
            status: None,
        },
        extra: HashMap::new(),
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

/// Wrap a single line inside a `supersigil-xml` fence. Returns `(content, line)`
/// where `line` is the 0-based line of the wrapped content (always 1).
fn fenced(inner: &str) -> (String, u32) {
    let content = format!("```supersigil-xml\n{inner}\n```");
    (content, 1)
}

// ---------------------------------------------------------------------------
// find_ref_at_position — cursor inside a single ref
// ---------------------------------------------------------------------------

#[test]
fn cursor_inside_refs_attribute() {
    let (content, line) = fenced(r#"<References refs="auth/req#req-1-1" />"#);
    // Value `auth/req#req-1-1` starts at byte 20 on the inner line.
    let result = find_ref_at_position(&content, line, 25);
    assert_eq!(result.as_deref(), Some("auth/req#req-1-1"));
}

#[test]
fn cursor_inside_implements_attribute() {
    let (content, line) = fenced(r#"<Task id="t1" implements="design/req" />"#);
    // `implements="` starts at 14; value `design/req` starts at 26.
    let result = find_ref_at_position(&content, line, 30);
    assert_eq!(result.as_deref(), Some("design/req"));
}

#[test]
fn cursor_on_second_ref_in_comma_list() {
    let (content, line) = fenced(r#"<References refs="a/req#c1, b/req#c2" />"#);
    // Cursor at 32 is inside b/req#c2.
    let result = find_ref_at_position(&content, line, 32);
    assert_eq!(result.as_deref(), Some("b/req#c2"));
}

#[test]
fn cursor_outside_any_attribute_returns_none() {
    let (content, line) = fenced(r#"<References refs="auth/req#req-1-1" />"#);
    // Cursor at position 5 is on the tag name, not inside a value.
    let result = find_ref_at_position(&content, line, 5);
    assert!(result.is_none());
}

#[test]
fn cursor_on_attribute_name_returns_none() {
    let (content, line) = fenced(r#"<References refs="auth/req#req-1-1" />"#);
    // `refs` attribute name starts at index 13. Cursor at 14 is still on the
    // attribute name, not inside the quoted value.
    let result = find_ref_at_position(&content, line, 14);
    assert!(result.is_none());
}

#[test]
fn cursor_on_closing_quote_returns_none() {
    let (content, line) = fenced(r#"<References refs="ab" />"#);
    // Value `ab` occupies positions 18..20; closing `"` is at 20.
    let result = find_ref_at_position(&content, line, 20);
    assert!(result.is_none());
}

#[test]
fn wrong_line_returns_none() {
    let content = "```supersigil-xml\nfirst line\n<References refs=\"auth/req#req-1\" />\n```";
    // Line 1 has no refs; line 2 does.
    let result = find_ref_at_position(content, 1, 20);
    assert!(result.is_none());
}

#[test]
fn ref_outside_fence_returns_none() {
    // Content is NOT inside a fence — should return None.
    let content = r#"<References refs="auth/req#req-1-1" />"#;
    let result = find_ref_at_position(content, 0, 25);
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// resolve_ref — fragment ref (req-2-1)
// ---------------------------------------------------------------------------

#[test]
fn fragment_ref_resolves_to_source_position() {
    // Build a graph with one document containing a Criterion at a known line.
    let criterion_line = 5usize;
    let doc = make_doc(
        "auth/req",
        "/specs/auth/req.md",
        vec![make_acceptance_criteria(
            vec![make_criterion("req-1-1", criterion_line)],
            2,
        )],
    );

    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");

    let location = resolve_ref("auth/req#req-1-1", &graph).expect("should resolve");

    // SourcePosition is 1-based; LSP is 0-based.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "test line numbers always fit in u32"
    )]
    let expected_line = (criterion_line - 1) as u32;
    assert_eq!(location.range.start.line, expected_line);
    assert_eq!(location.range.start.character, 0); // column 1 -> 0-based
    assert!(
        location.uri.as_str().contains("auth/req.md"),
        "URI should reference the document file: {}",
        location.uri
    );
}

// ---------------------------------------------------------------------------
// resolve_ref — document-level ref (req-2-2)
// ---------------------------------------------------------------------------

#[test]
fn document_ref_resolves_to_file_start() {
    let doc = make_doc("design/req", "/specs/design/req.md", vec![]);
    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");

    let location = resolve_ref("design/req", &graph).expect("should resolve");

    assert_eq!(location.range.start.line, 0);
    assert_eq!(location.range.start.character, 0);
    assert!(location.uri.as_str().contains("design/req.md"));
}

// ---------------------------------------------------------------------------
// resolve_ref — nonexistent target (req-2-3)
// ---------------------------------------------------------------------------

#[test]
fn nonexistent_document_ref_returns_none() {
    let graph = build_graph(vec![], &default_config()).expect("graph must build");
    let result = resolve_ref("no/such/doc", &graph);
    assert!(result.is_none());
}

#[test]
fn nonexistent_fragment_ref_returns_none() {
    let doc = make_doc("auth/req", "/specs/auth/req.md", vec![]);
    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");

    // Document exists but fragment does not.
    let result = resolve_ref("auth/req#missing-frag", &graph);
    assert!(result.is_none());
}
