//! Fixture-based integration tests for the supersigil-parser.
//!
//! Each test loads a real `.md` fixture file through `parse_file` and verifies
//! the expected output structure.

use std::path::{Path, PathBuf};

use supersigil_core::{ComponentDefs, ParseResult};
use supersigil_parser::parse_file;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

// ---------------------------------------------------------------------------
// valid_simple.md
// ---------------------------------------------------------------------------

#[test]
fn fixture_valid_simple_produces_document() {
    let path = fixture_path("valid_simple.md");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");

    match result {
        ParseResult::Document(doc) => {
            assert_eq!(doc.frontmatter.id, "req/login");
            assert_eq!(doc.frontmatter.doc_type.as_deref(), Some("requirement"));
            assert_eq!(doc.frontmatter.status.as_deref(), Some("draft"));
            assert!(doc.extra.is_empty(), "no extra metadata expected");
            assert_eq!(doc.components.len(), 1);

            let validates = &doc.components[0];
            assert_eq!(validates.name, "References");
            assert_eq!(validates.attributes.get("refs").unwrap(), "req/login");
            assert!(validates.body_text.is_none(), "self-closing component");
            assert!(validates.children.is_empty());
        }
        ParseResult::NotSupersigil(_) => panic!("expected Document, got NotSupersigil"),
    }
}

// ---------------------------------------------------------------------------
// valid_nested.md
// ---------------------------------------------------------------------------

#[test]
fn fixture_valid_nested_produces_document_with_children() {
    let path = fixture_path("valid_nested.md");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");

    match result {
        ParseResult::Document(doc) => {
            assert_eq!(doc.frontmatter.id, "req/auth");
            assert_eq!(doc.frontmatter.doc_type.as_deref(), Some("requirement"));
            assert_eq!(doc.frontmatter.status.as_deref(), Some("approved"));
            assert!(doc.extra.is_empty());

            // Top-level: one AcceptanceCriteria component
            assert_eq!(doc.components.len(), 1);
            let ac = &doc.components[0];
            assert_eq!(ac.name, "AcceptanceCriteria");
            assert!(ac.body_text.is_none(), "only child components, no text");

            // Two Criterion children
            assert_eq!(ac.children.len(), 2);

            let crit1 = &ac.children[0];
            assert_eq!(crit1.name, "Criterion");
            assert_eq!(crit1.attributes.get("id").unwrap(), "crit-1");
            assert_eq!(
                crit1.body_text.as_deref(),
                Some("User can log in with valid credentials")
            );
            assert!(crit1.children.is_empty());

            let crit2 = &ac.children[1];
            assert_eq!(crit2.name, "Criterion");
            assert_eq!(crit2.attributes.get("id").unwrap(), "crit-2");
            assert_eq!(
                crit2.body_text.as_deref(),
                Some("Invalid credentials are rejected")
            );
            assert!(crit2.children.is_empty());
        }
        ParseResult::NotSupersigil(_) => panic!("expected Document, got NotSupersigil"),
    }
}

// ---------------------------------------------------------------------------
// extra_metadata.md
// ---------------------------------------------------------------------------

#[test]
fn fixture_extra_metadata_preserved_in_extra_map() {
    let path = fixture_path("extra_metadata.md");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");

    match result {
        ParseResult::Document(doc) => {
            assert_eq!(doc.frontmatter.id, "req/extra");
            assert_eq!(doc.frontmatter.doc_type, None);
            assert_eq!(doc.frontmatter.status, None);

            // Extra keys: title and custom_key
            assert_eq!(doc.extra.len(), 2);
            assert!(doc.extra.contains_key("title"), "missing 'title' key");
            assert!(
                doc.extra.contains_key("custom_key"),
                "missing 'custom_key' key"
            );

            // Verify values
            assert_eq!(
                doc.extra.get("title").and_then(|v| v.as_str()),
                Some("Extra Metadata Test")
            );
            assert_eq!(
                doc.extra.get("custom_key").and_then(|v| v.as_str()),
                Some("custom_value")
            );

            // One References component
            assert_eq!(doc.components.len(), 1);
            assert_eq!(doc.components[0].name, "References");
        }
        ParseResult::NotSupersigil(_) => panic!("expected Document, got NotSupersigil"),
    }
}

// ---------------------------------------------------------------------------
// no_frontmatter.md — files without front matter are NotSupersigil
// ---------------------------------------------------------------------------

#[test]
fn fixture_no_frontmatter_is_not_supersigil() {
    let path = fixture_path("no_frontmatter.md");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");
    assert!(
        matches!(result, ParseResult::NotSupersigil(_)),
        "expected NotSupersigil"
    );
}

// ---------------------------------------------------------------------------
// no_supersigil_key.md — files with frontmatter but no supersigil key
// ---------------------------------------------------------------------------

#[test]
fn fixture_no_supersigil_key_is_not_supersigil() {
    let path = fixture_path("no_supersigil_key.md");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");
    assert!(
        matches!(result, ParseResult::NotSupersigil(_)),
        "expected NotSupersigil"
    );
}

// ---------------------------------------------------------------------------
// multi_fence.md — Multiple supersigil-xml fences
// ---------------------------------------------------------------------------

#[test]
fn fixture_multi_fence_collects_components_from_all_fences() {
    let path = fixture_path("multi_fence.md");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");

    match result {
        ParseResult::Document(doc) => {
            assert_eq!(doc.frontmatter.id, "req/multi");

            // Should have: AcceptanceCriteria (from first fence) + References (from second)
            assert_eq!(
                doc.components.len(),
                2,
                "expected 2 top-level components, got: {:?}",
                doc.components.iter().map(|c| &c.name).collect::<Vec<_>>()
            );

            let ac = &doc.components[0];
            assert_eq!(ac.name, "AcceptanceCriteria");
            assert_eq!(ac.children.len(), 1);
            assert_eq!(ac.children[0].name, "Criterion");
            assert_eq!(ac.children[0].attributes.get("id").unwrap(), "crit-a");

            let refs = &doc.components[1];
            assert_eq!(refs.name, "References");
            assert_eq!(refs.attributes.get("refs").unwrap(), "req/other");
        }
        ParseResult::NotSupersigil(_) => panic!("expected Document, got NotSupersigil"),
    }
}
