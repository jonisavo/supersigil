//! Component extraction from parsed XML nodes.
//!
//! Transforms `XmlNode` values (produced by [`crate::xml_parser`]) into
//! [`ExtractedComponent`] values using the same `ComponentDefs`-based
//! filtering as the previous extraction pipeline.
//!
//! **Key behaviors:**
//! - Only known `PascalCase` elements (those in `ComponentDefs`) become components.
//! - Unknown `PascalCase` elements are transparent wrappers — their children are
//!   still traversed.
//! - Lowercase elements are ignored (but their children are traversed).
//! - Attributes are stored as `HashMap<String, String>` (raw strings).
//! - `body_text` is computed from direct `Text` children, trimmed, `None` if empty.
//! - `code_blocks` is always empty in the current Markdown + XML format.
//! - Nested child components are collected recursively.

use std::collections::HashMap;

use supersigil_core::{ComponentDefs, ExtractedComponent, SourcePosition};

use crate::util::{is_pascal_case, line_col};
use crate::xml_parser::XmlNode;

/// Collect body text from the direct children of an XML element.
///
/// Concatenates `Text` node values, recursing into non-component wrapper
/// elements (unknown `PascalCase` and lowercase). Known component children
/// are excluded from the body text.
/// Returns `(text, start_offset, end_offset)` where `text` is `None` if no text
/// was found or the result is empty after trimming, `start_offset` is the byte
/// offset of the first contributing text node, and `end_offset` is the raw source
/// byte offset of the end of the last contributing text node.
fn collect_body_text(
    children: &[XmlNode],
    defs: &ComponentDefs,
) -> (Option<String>, Option<usize>, Option<usize>) {
    let mut buf = String::new();
    let mut first_offset: Option<usize> = None;
    let mut last_end_offset: Option<usize> = None;
    collect_text_recursive(
        &mut buf,
        &mut first_offset,
        &mut last_end_offset,
        children,
        defs,
    );
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        (None, None, None)
    } else {
        // Adjust offset to account for leading whitespace trimmed from the text.
        let leading_ws = buf.len() - buf.trim_start().len();
        let offset = first_offset.map(|o| o + leading_ws);
        // Adjust end offset to account for trailing whitespace trimmed from the text.
        let trailing_ws = buf.len() - buf.trim_end().len();
        let end_offset = last_end_offset.map(|o| o - trailing_ws);
        (Some(trimmed.to_owned()), offset, end_offset)
    }
}

/// Recursively collect text values, skipping known component nodes.
fn collect_text_recursive(
    buf: &mut String,
    first_offset: &mut Option<usize>,
    last_end_offset: &mut Option<usize>,
    nodes: &[XmlNode],
    defs: &ComponentDefs,
) {
    for node in nodes {
        match node {
            XmlNode::Text {
                content,
                offset,
                end_offset,
            } => {
                if first_offset.is_none() {
                    *first_offset = Some(*offset);
                }
                *last_end_offset = Some(*end_offset);
                buf.push_str(content);
            }
            XmlNode::Element { name, children, .. } => {
                // Known components are child components — their text is excluded.
                if defs.is_known(name) {
                    continue;
                }
                // Unknown PascalCase or lowercase elements are transparent
                // wrappers — recurse into their children.
                collect_text_recursive(buf, first_offset, last_end_offset, children, defs);
            }
        }
    }
}

/// Convert a `Vec<(String, String)>` attribute list into a `HashMap`.
fn attributes_to_map(attrs: &[(String, String)]) -> HashMap<String, String> {
    attrs.iter().cloned().collect()
}

// ---------------------------------------------------------------------------
// Extraction context
// ---------------------------------------------------------------------------

/// Shared context threaded through the recursive extraction pipeline.
struct ExtractionCtx<'a> {
    /// The full normalized file content, for line/column computation.
    content: &'a str,
    defs: &'a ComponentDefs,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Walk parsed XML nodes and extract known components as [`ExtractedComponent`]
/// values.
///
/// `nodes` are the top-level `XmlNode` values from [`crate::parse_supersigil_xml`].
/// `content` is the full normalized file content (for line/column computation).
/// `component_defs` defines which `PascalCase` element names are known components.
#[must_use]
pub fn extract_components_from_xml(
    nodes: &[XmlNode],
    content: &str,
    component_defs: &ComponentDefs,
) -> Vec<ExtractedComponent> {
    let ctx = ExtractionCtx {
        content,
        defs: component_defs,
    };
    let mut components = Vec::new();
    collect_from_nodes(nodes, &ctx, &mut components);
    components
}

// ---------------------------------------------------------------------------
// Recursive helpers
// ---------------------------------------------------------------------------

/// Process a list of XML nodes, collecting known components into `out`.
fn collect_from_nodes(
    nodes: &[XmlNode],
    ctx: &ExtractionCtx<'_>,
    out: &mut Vec<ExtractedComponent>,
) {
    for node in nodes {
        collect_component(node, ctx, out);
    }
}

/// Process a single XML node. If it's a known component element, extract it;
/// otherwise recurse into children looking for nested components.
fn collect_component(node: &XmlNode, ctx: &ExtractionCtx<'_>, out: &mut Vec<ExtractedComponent>) {
    match node {
        XmlNode::Text { .. } => {}

        XmlNode::Element {
            name,
            attributes,
            children,
            offset,
            end_offset,
        } => {
            if !is_pascal_case(name) {
                collect_from_nodes(children, ctx, out);
                return;
            }

            if !ctx.defs.is_known(name) {
                collect_from_nodes(children, ctx, out);
                return;
            }

            // Known component — extract it.
            let (line, column) = line_col(ctx.content, *offset);
            let position = SourcePosition {
                byte_offset: *offset,
                line,
                column,
            };

            let (end_line, end_column) = line_col(ctx.content, *end_offset);
            let end_position = SourcePosition {
                byte_offset: *end_offset,
                line: end_line,
                column: end_column,
            };

            let attrs = attributes_to_map(attributes);

            let mut child_components = Vec::new();
            collect_from_nodes(children, ctx, &mut child_components);

            let (body_text, body_text_offset, body_text_end_offset) =
                collect_body_text(children, ctx.defs);

            out.push(ExtractedComponent {
                name: name.clone(),
                attributes: attrs,
                children: child_components,
                body_text,
                body_text_offset,
                body_text_end_offset,
                code_blocks: Vec::new(),
                position,
                end_position,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(nodes: &[XmlNode], content: &str, defs: &ComponentDefs) -> Vec<ExtractedComponent> {
        extract_components_from_xml(nodes, content, defs)
    }

    /// Helper to build a `XmlNode::Text` for tests (offset 0, `end_offset` = len).
    fn text(s: &str) -> XmlNode {
        XmlNode::Text {
            content: s.into(),
            offset: 0,
            end_offset: s.len(),
        }
    }

    // -- Known component extraction ----------------------------------------

    #[test]
    fn extracts_known_component() {
        let defs = ComponentDefs::defaults();
        let content = "0123456789<Criterion id=\"c1\">Some text</Criterion>";
        let nodes = vec![XmlNode::Element {
            name: "Criterion".into(),
            attributes: vec![("id".into(), "c1".into())],
            children: vec![text("Some text")],
            offset: 10,
            end_offset: content.len(),
        }];

        let result = extract(&nodes, content, &defs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Criterion");
        assert_eq!(result[0].attributes["id"], "c1");
        assert_eq!(result[0].body_text.as_deref(), Some("Some text"));
        assert_eq!(result[0].position.byte_offset, 10);
        assert_eq!(result[0].end_position.byte_offset, content.len());
        assert_eq!(result[0].end_position.line, 1);
        assert_eq!(result[0].end_position.column, content.len() + 1);
    }

    #[test]
    fn extracts_multiple_top_level_components() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![
            XmlNode::Element {
                name: "Criterion".into(),
                attributes: vec![("id".into(), "c1".into())],
                children: vec![text("text")],
                offset: 0,
                end_offset: 0,
            },
            XmlNode::Element {
                name: "VerifiedBy".into(),
                attributes: vec![("refs".into(), "c1".into())],
                children: vec![],
                offset: 50,
                end_offset: 0,
            },
        ];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "Criterion");
        assert_eq!(result[1].name, "VerifiedBy");
    }

    // -- Unknown PascalCase transparency -----------------------------------

    #[test]
    fn unknown_pascal_case_is_transparent_wrapper() {
        let defs = ComponentDefs::defaults();
        // <Aside> is not a known component — children should be traversed.
        let nodes = vec![XmlNode::Element {
            name: "Aside".into(),
            attributes: vec![],
            children: vec![XmlNode::Element {
                name: "Criterion".into(),
                attributes: vec![("id".into(), "c1".into())],
                children: vec![],
                offset: 20,
                end_offset: 0,
            }],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        // Aside should NOT appear; Criterion should.
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Criterion");
    }

    #[test]
    fn deeply_nested_unknown_wrappers_are_transparent() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "Wrapper".into(),
            attributes: vec![],
            children: vec![XmlNode::Element {
                name: "Inner".into(),
                attributes: vec![],
                children: vec![XmlNode::Element {
                    name: "Criterion".into(),
                    attributes: vec![("id".into(), "deep".into())],
                    children: vec![],
                    offset: 40,
                    end_offset: 0,
                }],
                offset: 20,
                end_offset: 0,
            }],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Criterion");
        assert_eq!(result[0].attributes["id"], "deep");
    }

    // -- Lowercase element ignoring ----------------------------------------

    #[test]
    fn lowercase_elements_are_ignored() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "div".into(),
            attributes: vec![],
            children: vec![XmlNode::Element {
                name: "Criterion".into(),
                attributes: vec![("id".into(), "c1".into())],
                children: vec![],
                offset: 10,
                end_offset: 0,
            }],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        // div should not appear, but its child Criterion should.
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Criterion");
    }

    // -- Attribute extraction ----------------------------------------------

    #[test]
    fn attributes_stored_as_raw_strings() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "Criterion".into(),
            attributes: vec![
                ("id".into(), "c1".into()),
                ("strategy".into(), "tag".into()),
            ],
            children: vec![],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        assert_eq!(result[0].attributes.len(), 2);
        assert_eq!(result[0].attributes["id"], "c1");
        assert_eq!(result[0].attributes["strategy"], "tag");
    }

    #[test]
    fn self_closing_element_has_empty_children_and_no_body_text() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "VerifiedBy".into(),
            attributes: vec![("refs".into(), "c1".into())],
            children: vec![],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        assert_eq!(result.len(), 1);
        assert!(result[0].children.is_empty());
        assert_eq!(result[0].body_text, None);
    }

    // -- Body text computation ---------------------------------------------

    #[test]
    fn body_text_from_text_children() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "Criterion".into(),
            attributes: vec![("id".into(), "c1".into())],
            children: vec![text("\n  The system shall do something.\n")],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        assert_eq!(
            result[0].body_text.as_deref(),
            Some("The system shall do something.")
        );
    }

    #[test]
    fn body_text_none_for_whitespace_only() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "Criterion".into(),
            attributes: vec![("id".into(), "c1".into())],
            children: vec![text("   \n  \n  ")],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        assert_eq!(result[0].body_text, None);
    }

    #[test]
    fn body_text_excludes_known_child_components() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "AcceptanceCriteria".into(),
            attributes: vec![],
            children: vec![
                text("Parent text"),
                XmlNode::Element {
                    name: "Criterion".into(),
                    attributes: vec![("id".into(), "c1".into())],
                    children: vec![text("Child text")],
                    offset: 30,
                    end_offset: 0,
                },
            ],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "AcceptanceCriteria");
        // Body text should contain "Parent text" but NOT "Child text"
        assert_eq!(result[0].body_text.as_deref(), Some("Parent text"));
    }

    #[test]
    fn body_text_includes_text_from_unknown_wrapper() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "Criterion".into(),
            attributes: vec![("id".into(), "c1".into())],
            children: vec![XmlNode::Element {
                name: "Emphasis".into(),
                attributes: vec![],
                children: vec![text("important")],
                offset: 20,
                end_offset: 0,
            }],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        // Emphasis is unknown PascalCase — transparent for body text.
        assert_eq!(result[0].body_text.as_deref(), Some("important"));
    }

    // -- Nested children ---------------------------------------------------

    #[test]
    fn nested_child_components_collected() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "AcceptanceCriteria".into(),
            attributes: vec![],
            children: vec![
                XmlNode::Element {
                    name: "Criterion".into(),
                    attributes: vec![("id".into(), "c1".into())],
                    children: vec![text("First")],
                    offset: 20,
                    end_offset: 0,
                },
                XmlNode::Element {
                    name: "Criterion".into(),
                    attributes: vec![("id".into(), "c2".into())],
                    children: vec![text("Second")],
                    offset: 60,
                    end_offset: 0,
                },
            ],
            offset: 0,
            end_offset: 0,
        }];
        let content = &"x".repeat(100);

        let result = extract(&nodes, content, &defs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "AcceptanceCriteria");
        assert_eq!(result[0].children.len(), 2);
        assert_eq!(result[0].children[0].name, "Criterion");
        assert_eq!(result[0].children[0].attributes["id"], "c1");
        assert_eq!(result[0].children[0].body_text.as_deref(), Some("First"));
        assert_eq!(result[0].children[1].name, "Criterion");
        assert_eq!(result[0].children[1].attributes["id"], "c2");
        assert_eq!(result[0].children[1].body_text.as_deref(), Some("Second"));
    }

    // -- Position computation ----------------------------------------------

    #[test]
    fn position_computed_from_byte_offset() {
        let defs = ComponentDefs::defaults();
        // Content: "line1\nline2\n<Criterion>" — offset 12 is line 3, column 1.
        let content = "line1\nline2\n<Criterion id=\"c1\" />";
        let nodes = vec![XmlNode::Element {
            name: "Criterion".into(),
            attributes: vec![("id".into(), "c1".into())],
            children: vec![],
            offset: 12,
            end_offset: 0,
        }];

        let result = extract(&nodes, content, &defs);
        assert_eq!(result[0].position.byte_offset, 12);
        assert_eq!(result[0].position.line, 3);
        assert_eq!(result[0].position.column, 1);
    }

    #[test]
    fn position_mid_line() {
        let defs = ComponentDefs::defaults();
        // Offset 7 in "abcdef\n  <Cr" is line 2, column 3.
        let content = "abcdef\n  <Criterion />";
        let nodes = vec![XmlNode::Element {
            name: "Criterion".into(),
            attributes: vec![("id".into(), "c1".into())],
            children: vec![],
            offset: 9, // "abcdef\n  " = 7 + 2 = 9
            end_offset: 0,
        }];

        let result = extract(&nodes, content, &defs);
        assert_eq!(result[0].position.byte_offset, 9);
        assert_eq!(result[0].position.line, 2);
        assert_eq!(result[0].position.column, 3);
    }

    // -- Empty input -------------------------------------------------------

    #[test]
    fn empty_nodes_produces_empty_result() {
        let defs = ComponentDefs::defaults();
        let result = extract(&[], "", &defs);
        assert!(result.is_empty());
    }

    // -- Text-only nodes at top level --------------------------------------

    #[test]
    fn text_only_nodes_produce_no_components() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![text("just some text")];
        let result = extract(&nodes, "just some text", &defs);
        assert!(result.is_empty());
    }

    // -- Realistic example -------------------------------------------------

    #[test]
    fn realistic_spec_extraction() {
        let defs = ComponentDefs::defaults();
        let content = r#"---
supersigil:
  id: test-spec
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="perf-latency" strategy="tag">
    P99 latency must be under 100ms for API requests.
  </Criterion>
</AcceptanceCriteria>
<VerifiedBy refs="perf-latency" />
```
"#;
        // Simulate offsets as if the XML parser produced them.
        // The actual byte offsets would be computed by the XML parser;
        // here we just test the extraction logic.
        let nodes = vec![
            XmlNode::Element {
                name: "AcceptanceCriteria".into(),
                attributes: vec![],
                children: vec![XmlNode::Element {
                    name: "Criterion".into(),
                    attributes: vec![
                        ("id".into(), "perf-latency".into()),
                        ("strategy".into(), "tag".into()),
                    ],
                    children: vec![text(
                        "\n    P99 latency must be under 100ms for API requests.\n  ",
                    )],
                    offset: 70,
                    end_offset: 0,
                }],
                offset: 50,
                end_offset: 0,
            },
            XmlNode::Element {
                name: "VerifiedBy".into(),
                attributes: vec![("refs".into(), "perf-latency".into())],
                children: vec![],
                offset: 160,
                end_offset: 0,
            },
        ];

        let result = extract(&nodes, content, &defs);
        assert_eq!(result.len(), 2);

        // AcceptanceCriteria
        assert_eq!(result[0].name, "AcceptanceCriteria");
        assert!(result[0].attributes.is_empty());
        assert_eq!(result[0].children.len(), 1);

        // Nested Criterion
        let criterion = &result[0].children[0];
        assert_eq!(criterion.name, "Criterion");
        assert_eq!(criterion.attributes["id"], "perf-latency");
        assert_eq!(criterion.attributes["strategy"], "tag");
        assert_eq!(
            criterion.body_text.as_deref(),
            Some("P99 latency must be under 100ms for API requests.")
        );

        // VerifiedBy
        assert_eq!(result[1].name, "VerifiedBy");
        assert_eq!(result[1].attributes["refs"], "perf-latency");
        assert_eq!(result[1].body_text, None);
        assert!(result[1].children.is_empty());
    }

    // -- Direct public API call -------------------------------------------

    #[test]
    fn public_api_extracts_components() {
        let defs = ComponentDefs::defaults();
        let nodes = vec![XmlNode::Element {
            name: "Criterion".into(),
            attributes: vec![("id".into(), "c1".into())],
            children: vec![],
            offset: 0,
            end_offset: 0,
        }];

        let result = extract_components_from_xml(&nodes, "x", &defs);

        assert_eq!(result.len(), 1);
    }
}
