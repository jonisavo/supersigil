//! Fixture-based integration tests for the supersigil-parser.
//!
//! Each test loads a real `.mdx` fixture file through `parse_file` and verifies
//! the expected output structure.

use std::path::{Path, PathBuf};

use supersigil_core::{ComponentDefs, Frontmatter, ParseResult};
use supersigil_parser::parse_file;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

// ---------------------------------------------------------------------------
// valid_simple.mdx
// ---------------------------------------------------------------------------

#[test]
fn fixture_valid_simple_produces_document() {
    let path = fixture_path("valid_simple.mdx");
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
// valid_nested.mdx
// ---------------------------------------------------------------------------

#[test]
fn fixture_valid_nested_produces_document_with_children() {
    let path = fixture_path("valid_nested.mdx");
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
// no_frontmatter.mdx
// ---------------------------------------------------------------------------

#[test]
fn fixture_no_frontmatter_returns_not_supersigil() {
    let path = fixture_path("no_frontmatter.mdx");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");

    match result {
        ParseResult::NotSupersigil(p) => {
            assert_eq!(p, path);
        }
        ParseResult::Document(_) => panic!("expected NotSupersigil, got Document"),
    }
}

// ---------------------------------------------------------------------------
// no_supersigil_key.mdx
// ---------------------------------------------------------------------------

#[test]
fn fixture_no_supersigil_key_returns_not_supersigil() {
    let path = fixture_path("no_supersigil_key.mdx");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");

    match result {
        ParseResult::NotSupersigil(p) => {
            assert_eq!(p, path);
        }
        ParseResult::Document(_) => panic!("expected NotSupersigil, got Document"),
    }
}

// ---------------------------------------------------------------------------
// extra_metadata.mdx
// ---------------------------------------------------------------------------

#[test]
fn fixture_extra_metadata_preserved_in_extra_map() {
    let path = fixture_path("extra_metadata.mdx");
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
// Frontmatter round-trip property on fixture data (Req 22.1)
// ---------------------------------------------------------------------------

#[test]
fn fixture_frontmatter_round_trip_valid_simple() {
    let path = fixture_path("valid_simple.mdx");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");

    if let ParseResult::Document(doc) = result {
        let yaml = yaml_serde::to_string(&doc.frontmatter).expect("serialize");
        let deserialized: Frontmatter = yaml_serde::from_str(&yaml).expect("deserialize");
        assert_eq!(doc.frontmatter, deserialized);
    } else {
        panic!("expected Document");
    }
}

#[test]
fn fixture_frontmatter_round_trip_valid_nested() {
    let path = fixture_path("valid_nested.mdx");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");

    if let ParseResult::Document(doc) = result {
        let yaml = yaml_serde::to_string(&doc.frontmatter).expect("serialize");
        let deserialized: Frontmatter = yaml_serde::from_str(&yaml).expect("deserialize");
        assert_eq!(doc.frontmatter, deserialized);
    } else {
        panic!("expected Document");
    }
}

#[test]
fn fixture_frontmatter_round_trip_extra_metadata() {
    let path = fixture_path("extra_metadata.mdx");
    let defs = ComponentDefs::defaults();
    let result = parse_file(&path, &defs).expect("should parse without errors");

    if let ParseResult::Document(doc) = result {
        // Round-trip the frontmatter (id only, no doc_type/status)
        let yaml = yaml_serde::to_string(&doc.frontmatter).expect("serialize");
        let deserialized: Frontmatter = yaml_serde::from_str(&yaml).expect("deserialize");
        assert_eq!(doc.frontmatter, deserialized);

        // Verify the type↔doc_type rename: YAML should use "type", not "doc_type"
        assert!(
            !yaml.contains("doc_type"),
            "YAML should use 'type' not 'doc_type'"
        );
    } else {
        panic!("expected Document");
    }
}
