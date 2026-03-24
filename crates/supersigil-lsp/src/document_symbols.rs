//! Document symbol support for the outline panel, breadcrumbs, and
//! "Go to Symbol in Document" (`Ctrl+Shift+O`).
//!
//! Maps `ExtractedComponent` trees from a parsed `SpecDocument` into
//! hierarchical LSP `DocumentSymbol` trees.

use lsp_types::{DocumentSymbol, Range, SymbolKind};
use supersigil_core::{ExtractedComponent, SpecDocument};

use crate::position;

/// Build a hierarchical `DocumentSymbol` list from a parsed spec document.
///
/// Each top-level `ExtractedComponent` becomes a symbol, with nested
/// components as children. The `content` parameter is needed for UTF-16
/// position conversion.
#[must_use]
pub fn document_symbols(doc: &SpecDocument, content: &str) -> Vec<DocumentSymbol> {
    doc.components
        .iter()
        .map(|c| component_to_symbol(c, content))
        .collect()
}

#[allow(
    deprecated,
    reason = "DocumentSymbol::deprecated field is deprecated but required"
)]
fn component_to_symbol(component: &ExtractedComponent, content: &str) -> DocumentSymbol {
    let (name, detail) = if let Some(id) = component.attributes.get("id") {
        (id.clone(), Some(component.name.clone()))
    } else {
        (component.name.clone(), None)
    };

    let start = position::source_to_lsp_utf16(&component.position, content);
    let end = position::source_to_lsp_utf16(&component.end_position, content);

    let range = Range { start, end };
    let selection_range = position::zero_range(start);

    let children = if component.children.is_empty() {
        None
    } else {
        Some(
            component
                .children
                .iter()
                .map(|c| component_to_symbol(c, content))
                .collect(),
        )
    };

    DocumentSymbol {
        name,
        detail,
        kind: symbol_kind(&component.name),
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

fn symbol_kind(name: &str) -> SymbolKind {
    match name {
        "Criterion" => SymbolKind::PROPERTY,
        "Task" => SymbolKind::EVENT,
        "Decision" => SymbolKind::INTERFACE,
        "Alternative" => SymbolKind::ENUM_MEMBER,
        "VerifiedBy" | "TrackedFiles" | "References" | "Implements" | "DependsOn" => {
            SymbolKind::STRUCT
        }
        _ => SymbolKind::OBJECT,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(deprecated, reason = "DocumentSymbol::deprecated field")]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use supersigil_core::{Frontmatter, SourcePosition};

    use super::*;

    fn pos(byte_offset: usize, line: usize, column: usize) -> SourcePosition {
        SourcePosition {
            byte_offset,
            line,
            column,
        }
    }

    fn make_doc(components: Vec<ExtractedComponent>) -> SpecDocument {
        SpecDocument {
            path: PathBuf::from("test.md"),
            frontmatter: Frontmatter {
                id: "test/req".into(),
                doc_type: Some("requirements".into()),
                status: Some("draft".into()),
            },
            extra: HashMap::new(),
            components,
            warnings: Vec::new(),
        }
    }

    fn make_component(
        name: &str,
        id: Option<&str>,
        children: Vec<ExtractedComponent>,
        start: SourcePosition,
        end: SourcePosition,
    ) -> ExtractedComponent {
        let mut attributes = HashMap::new();
        if let Some(id_val) = id {
            attributes.insert("id".into(), id_val.into());
        }
        ExtractedComponent {
            name: name.into(),
            attributes,
            children,
            body_text: None,
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: Vec::new(),
            position: start,
            end_position: end,
        }
    }

    // -- req-3-1: empty document returns empty list --

    #[test]
    fn empty_document_returns_empty_symbols() {
        let doc = make_doc(vec![]);
        let content = "---\nsupersigil:\n  id: test/req\n---\n";
        let symbols = document_symbols(&doc, content);
        assert!(symbols.is_empty());
    }

    // -- req-1-1: returns DocumentSymbol[] --
    // -- req-1-2: components become symbols, children nested --

    /// Build content with enough lines for position conversion.
    fn test_content(num_lines: usize) -> String {
        (0..num_lines)
            .map(|_| "x".repeat(80))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn single_criterion_becomes_symbol() {
        let doc = make_doc(vec![make_component(
            "Criterion",
            Some("req-1-1"),
            vec![],
            pos(0, 1, 1),
            pos(40, 1, 41),
        )]);
        let content = test_content(5);
        let symbols = document_symbols(&doc, &content);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "req-1-1");
        assert_eq!(symbols[0].kind, SymbolKind::PROPERTY);
    }

    // -- req-1-3: name is id when present, component name otherwise --

    #[test]
    fn symbol_name_is_id_with_component_name_as_detail() {
        let doc = make_doc(vec![make_component(
            "Task",
            Some("task-1"),
            vec![],
            pos(0, 1, 1),
            pos(30, 1, 31),
        )]);
        let content = test_content(5);
        let symbols = document_symbols(&doc, &content);

        assert_eq!(symbols[0].name, "task-1");
        assert_eq!(symbols[0].detail.as_deref(), Some("Task"));
    }

    #[test]
    fn symbol_name_is_component_name_when_no_id() {
        let doc = make_doc(vec![make_component(
            "AcceptanceCriteria",
            None,
            vec![],
            pos(0, 1, 1),
            pos(30, 1, 31),
        )]);
        let content = test_content(5);
        let symbols = document_symbols(&doc, &content);

        assert_eq!(symbols[0].name, "AcceptanceCriteria");
        assert!(symbols[0].detail.is_none());
    }

    // -- req-1-4: kind mapping --

    #[test]
    fn kind_mapping() {
        let cases = vec![
            ("Criterion", SymbolKind::PROPERTY),
            ("Task", SymbolKind::EVENT),
            ("Decision", SymbolKind::INTERFACE),
            ("Alternative", SymbolKind::ENUM_MEMBER),
            ("VerifiedBy", SymbolKind::STRUCT),
            ("TrackedFiles", SymbolKind::STRUCT),
            ("References", SymbolKind::STRUCT),
            ("Implements", SymbolKind::STRUCT),
            ("DependsOn", SymbolKind::STRUCT),
            ("AcceptanceCriteria", SymbolKind::OBJECT),
            ("Rationale", SymbolKind::OBJECT),
            ("Example", SymbolKind::OBJECT),
        ];

        let content = test_content(5);
        for (name, expected_kind) in cases {
            let doc = make_doc(vec![make_component(
                name,
                None,
                vec![],
                pos(0, 1, 1),
                pos(30, 1, 31),
            )]);
            let symbols = document_symbols(&doc, &content);
            assert_eq!(
                symbols[0].kind, expected_kind,
                "expected {expected_kind:?} for {name}"
            );
        }
    }

    // -- req-1-2: nested children --

    #[test]
    fn nested_components_produce_nested_symbols() {
        let content = test_content(10);
        let child = make_component(
            "Criterion",
            Some("req-1-1"),
            vec![],
            pos(81, 2, 1),
            pos(161, 2, 81),
        );
        let parent = make_component(
            "AcceptanceCriteria",
            None,
            vec![child],
            pos(0, 1, 1),
            pos(242, 3, 81),
        );
        let doc = make_doc(vec![parent]);
        let symbols = document_symbols(&doc, &content);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "AcceptanceCriteria");
        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "req-1-1");
        assert_eq!(children[0].kind, SymbolKind::PROPERTY);
    }

    // -- req-1-5: range spans full component --

    #[test]
    fn range_spans_full_component() {
        let content = test_content(10);
        let doc = make_doc(vec![make_component(
            "Criterion",
            Some("c1"),
            vec![],
            pos(81, 2, 1),
            pos(242, 3, 81),
        )]);
        let symbols = document_symbols(&doc, &content);

        // range: line 2 → 0-based 1, line 3 → 0-based 2
        assert_eq!(symbols[0].range.start.line, 1);
        assert_eq!(symbols[0].range.end.line, 2);
    }

    // -- req-3-2: multiple components all returned --

    #[test]
    fn returns_symbols_for_all_parsed_components() {
        let content = test_content(10);
        let doc = make_doc(vec![
            make_component(
                "Criterion",
                Some("c1"),
                vec![],
                pos(0, 1, 1),
                pos(30, 1, 31),
            ),
            make_component("Task", Some("t1"), vec![], pos(81, 2, 1), pos(120, 2, 40)),
        ]);
        let symbols = document_symbols(&doc, &content);
        assert_eq!(symbols.len(), 2);
    }

    // -- Integration: end-to-end from parsed content --

    #[test]
    fn end_to_end_from_parsed_content() {
        use supersigil_core::{ComponentDefs, ParseResult};
        use supersigil_parser::parse_content;

        let content = "\
---
supersigil:
  id: test/req
  type: requirements
  status: draft
title: \"Test\"
---

## Requirement 1

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"req-1-1\">
    Users can log in.
  </Criterion>
</AcceptanceCriteria>
```
";
        let defs = ComponentDefs::defaults();
        let result = parse_content(std::path::Path::new("test.md"), content, &defs).unwrap();
        let ParseResult::Document(doc) = result else {
            panic!("expected Document");
        };

        let symbols = document_symbols(&doc, content);
        assert_eq!(symbols.len(), 1, "AcceptanceCriteria");
        assert_eq!(symbols[0].name, "AcceptanceCriteria");
        assert_eq!(symbols[0].kind, SymbolKind::OBJECT);

        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1, "Criterion nested");
        assert_eq!(children[0].name, "req-1-1");
        assert_eq!(children[0].detail.as_deref(), Some("Criterion"));
        assert_eq!(children[0].kind, SymbolKind::PROPERTY);

        // Verify range spans actual content
        assert!(symbols[0].range.start.line < symbols[0].range.end.line);
        // Selection range is zero-width at start
        assert_eq!(
            symbols[0].selection_range.start,
            symbols[0].selection_range.end
        );
    }
}
