use super::*;

fn parse(content: &str) -> Result<Vec<XmlNode>, ParseError> {
    parse_supersigil_xml(content, 0, Path::new("test.md"))
}

fn parse_with_offset(content: &str, offset: usize) -> Result<Vec<XmlNode>, ParseError> {
    parse_supersigil_xml(content, offset, Path::new("test.md"))
}

// -- Valid fragments ---------------------------------------------------

#[test]
fn empty_input() {
    let nodes = parse("").unwrap();
    assert!(nodes.is_empty());
}

#[test]
fn whitespace_only_input() {
    let nodes = parse("  \n  \n  ").unwrap();
    assert!(nodes.is_empty());
}

#[test]
fn single_self_closing_element() {
    let nodes = parse(r#"<Spec id="s1" />"#).unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        XmlNode::Element {
            name,
            attributes,
            children,
            offset,
            ..
        } => {
            assert_eq!(name, "Spec");
            assert_eq!(attributes, &[("id".to_owned(), "s1".to_owned())]);
            assert!(children.is_empty());
            assert_eq!(*offset, 0);
        }
        _ => panic!("expected Element"),
    }
}

#[test]
fn self_closing_no_space_before_slash() {
    let nodes = parse(r#"<Spec id="s1"/>"#).unwrap();
    assert_eq!(nodes.len(), 1);
    if let XmlNode::Element { name, .. } = &nodes[0] {
        assert_eq!(name, "Spec");
    } else {
        panic!("expected Element");
    }
}

#[test]
fn element_with_text_content() {
    let nodes = parse("<Title>Hello World</Title>").unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        XmlNode::Element { name, children, .. } => {
            assert_eq!(name, "Title");
            assert_eq!(children.len(), 1);
            assert!(
                matches!(&children[0], XmlNode::Text { content, .. } if content == "Hello World")
            );
        }
        _ => panic!("expected Element"),
    }
}

#[test]
fn element_with_no_attributes() {
    let nodes = parse("<Container></Container>").unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        XmlNode::Element {
            name, attributes, ..
        } => {
            assert_eq!(name, "Container");
            assert!(attributes.is_empty());
        }
        _ => panic!("expected Element"),
    }
}

// -- Nested elements ---------------------------------------------------

#[test]
fn nested_elements() {
    let input = r#"<Parent id="p1"><Child id="c1" /></Parent>"#;
    let nodes = parse(input).unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        XmlNode::Element { name, children, .. } => {
            assert_eq!(name, "Parent");
            assert_eq!(children.len(), 1);
            match &children[0] {
                XmlNode::Element {
                    name, attributes, ..
                } => {
                    assert_eq!(name, "Child");
                    assert_eq!(attributes, &[("id".to_owned(), "c1".to_owned())]);
                }
                _ => panic!("expected nested Element"),
            }
        }
        _ => panic!("expected Element"),
    }
}

#[test]
fn deeply_nested_elements() {
    let input = "<A><B><C>deep</C></B></A>";
    let nodes = parse(input).unwrap();
    assert_eq!(nodes.len(), 1);
    // A -> B -> C -> Text("deep")
    let a = &nodes[0];
    if let XmlNode::Element { children, .. } = a {
        let b = &children[0];
        if let XmlNode::Element { children, .. } = b {
            let c = &children[0];
            if let XmlNode::Element { children, .. } = c {
                assert!(matches!(&children[0], XmlNode::Text { content, .. } if content == "deep"));
            } else {
                panic!("expected C element");
            }
        } else {
            panic!("expected B element");
        }
    } else {
        panic!("expected A element");
    }
}

#[test]
fn mixed_children_text_and_elements() {
    let input = "<Parent>before<Child />after</Parent>";
    let nodes = parse(input).unwrap();
    assert_eq!(nodes.len(), 1);
    if let XmlNode::Element { children, .. } = &nodes[0] {
        assert_eq!(children.len(), 3);
        assert!(matches!(&children[0], XmlNode::Text { content, .. } if content == "before"));
        assert!(matches!(&children[1], XmlNode::Element { name, .. } if name == "Child"));
        assert!(matches!(&children[2], XmlNode::Text { content, .. } if content == "after"));
    } else {
        panic!("expected Element");
    }
}

// -- Multiple top-level elements ---------------------------------------

#[test]
fn multiple_top_level_elements() {
    let input = r#"<A id="1" />
<B id="2" />"#;
    let nodes = parse(input).unwrap();
    assert_eq!(nodes.len(), 2);
    if let XmlNode::Element { name, .. } = &nodes[0] {
        assert_eq!(name, "A");
    }
    if let XmlNode::Element { name, .. } = &nodes[1] {
        assert_eq!(name, "B");
    }
}

// -- Attribute parsing -------------------------------------------------

#[test]
fn multiple_attributes() {
    let input = r#"<Criterion id="c1" strategy="tag" />"#;
    let nodes = parse(input).unwrap();
    if let XmlNode::Element { attributes, .. } = &nodes[0] {
        assert_eq!(
            attributes,
            &[
                ("id".to_owned(), "c1".to_owned()),
                ("strategy".to_owned(), "tag".to_owned()),
            ]
        );
    } else {
        panic!("expected Element");
    }
}

#[test]
fn attribute_with_entity_in_value() {
    let input = r#"<Spec desc="a &amp; b" />"#;
    let nodes = parse(input).unwrap();
    if let XmlNode::Element { attributes, .. } = &nodes[0] {
        assert_eq!(attributes[0].1, "a & b");
    } else {
        panic!("expected Element");
    }
}

#[test]
fn all_supported_entities_in_attribute() {
    let input = r#"<Spec val="&amp;&lt;&gt;&quot;" />"#;
    let nodes = parse(input).unwrap();
    if let XmlNode::Element { attributes, .. } = &nodes[0] {
        assert_eq!(attributes[0].1, "&<>\"");
    } else {
        panic!("expected Element");
    }
}

// -- Entity references in text content ---------------------------------

#[test]
fn entity_references_in_text() {
    let input = "<Note>a &lt; b &amp; c &gt; d &quot;e&quot;</Note>";
    let nodes = parse(input).unwrap();
    if let XmlNode::Element { children, .. } = &nodes[0] {
        assert!(
            matches!(&children[0], XmlNode::Text { content, .. } if content == r#"a < b & c > d "e""#)
        );
    } else {
        panic!("expected Element");
    }
}

// -- Position offsetting -----------------------------------------------

#[test]
fn offset_applied_to_elements() {
    let fence_offset = 100;
    let input = r#"<Spec id="s1" />"#;
    let nodes = parse_with_offset(input, fence_offset).unwrap();
    if let XmlNode::Element { offset, .. } = &nodes[0] {
        assert_eq!(*offset, 100);
    } else {
        panic!("expected Element");
    }
}

#[test]
fn offset_applied_to_nested_element() {
    let fence_offset = 50;
    // "<A>" takes 3 bytes, so <B> starts at position 3.
    let input = "<A><B /></A>";
    let nodes = parse_with_offset(input, fence_offset).unwrap();
    if let XmlNode::Element {
        offset, children, ..
    } = &nodes[0]
    {
        assert_eq!(*offset, 50); // A at position 0 + 50
        if let XmlNode::Element { offset, .. } = &children[0] {
            assert_eq!(*offset, 53); // B at position 3 + 50
        } else {
            panic!("expected nested Element");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
fn offset_with_multiple_top_level_elements() {
    let fence_offset = 200;
    let input = "<A />\n<B />";
    let nodes = parse_with_offset(input, fence_offset).unwrap();
    assert_eq!(nodes.len(), 2);
    if let XmlNode::Element { offset, .. } = &nodes[0] {
        assert_eq!(*offset, 200); // A at byte 0
    }
    if let XmlNode::Element { offset, .. } = &nodes[1] {
        assert_eq!(*offset, 206); // B at byte 6 (after "<A />\n")
    }
}

// -- Error cases: unclosed tags ----------------------------------------

#[test]
fn unclosed_element() {
    let err = parse("<Spec>content").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("closing tag"), "got: {msg}");
    assert!(msg.contains("Spec"), "got: {msg}");
}

#[test]
fn mismatched_closing_tag() {
    let err = parse("<A>text</B>").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("mismatched"), "got: {msg}");
    assert!(msg.contains("A"), "got: {msg}");
    assert!(msg.contains("B"), "got: {msg}");
}

// -- Error cases: invalid attributes -----------------------------------

#[test]
fn single_quoted_attribute_value() {
    let err = parse("<Spec id='s1' />").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("double-quoted"), "got: {msg}");
}

#[test]
fn single_quotes_inside_double_quoted_attribute_value() {
    // Single quotes inside a double-quoted attribute value are valid XML.
    let result = parse(r#"<Spec val="a='b'" />"#);
    assert!(result.is_ok(), "got: {}", result.unwrap_err());
}

#[test]
fn missing_attribute_value() {
    let err = parse("<Spec id />").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("="), "got: {msg}");
}

// -- Error cases: unsupported XML features -----------------------------

#[test]
fn processing_instruction_rejected() {
    let err = parse("<?xml version=\"1.0\"?>").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("processing instruction"), "got: {msg}");
}

#[test]
fn cdata_rejected() {
    let err = parse("<![CDATA[foo]]>").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("CDATA"), "got: {msg}");
}

#[test]
fn doctype_rejected() {
    let err = parse("<!DOCTYPE html>").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("DTD") || msg.contains("DOCTYPE"), "got: {msg}");
}

#[test]
fn comment_rejected() {
    let err = parse("<!-- comment -->").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("comment"), "got: {msg}");
}

#[test]
fn namespace_in_element_rejected() {
    // The parser treats `:` as not part of a name, so `foo:Bar` would
    // parse `foo` as the name and then fail on `:`. That error is
    // acceptable — the point is it doesn't silently succeed.
    let err = parse("<foo:Bar />").unwrap_err();
    assert!(
        err.to_string().contains("test.md"),
        "error should include path"
    );
}

#[test]
fn unsupported_entity_rejected() {
    let err = parse("<Spec>&apos;</Spec>").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unsupported entity"), "got: {msg}");
}

#[test]
fn unterminated_entity_rejected() {
    let err = parse("<Spec>&amp</Spec>").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unterminated entity"), "got: {msg}");
}

// -- Lowercase elements are parsed successfully -------------------------

#[test]
fn lowercase_element_name_parsed_successfully() {
    let nodes = parse("<spec />").unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        XmlNode::Element { name, .. } => assert_eq!(name, "spec"),
        _ => panic!("expected Element"),
    }
}

#[test]
fn lowercase_element_inside_pascal_case_element() {
    let nodes = parse(r#"<Criterion id="c1">Use <em>fast</em> path</Criterion>"#).unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        XmlNode::Element { name, children, .. } => {
            assert_eq!(name, "Criterion");
            // Children: Text("Use "), Element(em), Text(" path")
            assert_eq!(children.len(), 3);
            assert!(matches!(&children[0], XmlNode::Text { content, .. } if content == "Use "));
            match &children[1] {
                XmlNode::Element { name, children, .. } => {
                    assert_eq!(name, "em");
                    assert_eq!(children.len(), 1);
                    assert!(
                        matches!(&children[0], XmlNode::Text { content, .. } if content == "fast")
                    );
                }
                _ => panic!("expected em Element"),
            }
            assert!(matches!(&children[2], XmlNode::Text { content, .. } if content == " path"));
        }
        _ => panic!("expected Criterion Element"),
    }
}

// -- Error position information ----------------------------------------

#[test]
fn error_includes_line_and_column() {
    // Error on line 2, at the start of the line (column 1).
    // Use a namespaced element (still rejected) to trigger an error.
    let err = parse("<A>\n<ns:B /></A>").unwrap_err();
    if let ParseError::XmlSyntaxError { line, column, .. } = &err {
        assert_eq!(*line, 2);
        assert_eq!(*column, 1);
    } else {
        panic!("expected XmlSyntaxError");
    }
}

#[test]
fn error_includes_file_path() {
    let err = parse_supersigil_xml("<?xml?>", 0, Path::new("/foo/bar.md")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("/foo/bar.md"), "got: {msg}");
}

// -- Closing tag at top level ------------------------------------------

#[test]
fn closing_tag_at_top_level_rejected() {
    let err = parse("</Orphan>").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unexpected closing tag"), "got: {msg}");
}

// -- Realistic example -------------------------------------------------

#[test]
fn realistic_component_example() {
    let input = r#"<Criterion id="perf-latency" strategy="tag">
  P99 latency must be under 100ms for API requests.
</Criterion>
<VerifiedBy strategy="tag" tag="perf-latency" />"#;
    let nodes = parse(input).unwrap();
    assert_eq!(nodes.len(), 2);

    // Criterion
    match &nodes[0] {
        XmlNode::Element {
            name,
            attributes,
            children,
            ..
        } => {
            assert_eq!(name, "Criterion");
            assert_eq!(attributes.len(), 2);
            assert_eq!(attributes[0], ("id".to_owned(), "perf-latency".to_owned()));
            assert_eq!(attributes[1], ("strategy".to_owned(), "tag".to_owned()));
            assert_eq!(children.len(), 1);
            if let XmlNode::Text { content, .. } = &children[0] {
                assert!(content.contains("P99 latency"));
            } else {
                panic!("expected Text child");
            }
        }
        _ => panic!("expected Element"),
    }

    // VerifiedBy
    match &nodes[1] {
        XmlNode::Element {
            name,
            attributes,
            children,
            ..
        } => {
            assert_eq!(name, "VerifiedBy");
            assert_eq!(attributes.len(), 2);
            assert!(children.is_empty());
        }
        _ => panic!("expected Element"),
    }
}

// -- UTF-8 text content ------------------------------------------------

#[test]
fn utf8_text_content_preserved() {
    let input = "<Note>cafe\u{0301} \u{1F600}</Note>";
    let nodes = parse(input).unwrap();
    if let XmlNode::Element { children, .. } = &nodes[0] {
        if let XmlNode::Text { content: t, .. } = &children[0] {
            assert!(t.contains("cafe\u{0301}"));
            assert!(t.contains('\u{1F600}'));
        } else {
            panic!("expected Text");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
fn text_node_has_correct_offset() {
    // "<Title>" = 7 bytes, so text "Hello" starts at offset 7
    let input = "<Title>Hello</Title>";
    let nodes = parse(input).unwrap();
    if let XmlNode::Element { children, .. } = &nodes[0] {
        if let XmlNode::Text {
            content, offset, ..
        } = &children[0]
        {
            assert_eq!(content, "Hello");
            assert_eq!(*offset, 7, "text should start at byte 7 (after '<Title>')");
        } else {
            panic!("expected Text");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
fn text_node_offset_with_fence_offset() {
    let fence_offset = 100;
    let input = "<Title>Hello</Title>";
    let nodes = parse_with_offset(input, fence_offset).unwrap();
    if let XmlNode::Element { children, .. } = &nodes[0] {
        if let XmlNode::Text {
            content, offset, ..
        } = &children[0]
        {
            assert_eq!(content, "Hello");
            assert_eq!(*offset, 107, "text should be fence_offset + 7");
        } else {
            panic!("expected Text");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
fn text_node_end_offset_plain_text() {
    // "<Title>Hello</Title>" — text "Hello" starts at 7, ends at 12
    let input = "<Title>Hello</Title>";
    let nodes = parse(input).unwrap();
    if let XmlNode::Element { children, .. } = &nodes[0] {
        if let XmlNode::Text {
            content,
            offset,
            end_offset,
        } = &children[0]
        {
            assert_eq!(content, "Hello");
            assert_eq!(*offset, 7);
            assert_eq!(
                *end_offset, 12,
                "end_offset should be past the last byte of 'Hello'"
            );
            assert_eq!(
                &input[*offset..*end_offset],
                "Hello",
                "offset..end_offset should span the raw text"
            );
        } else {
            panic!("expected Text");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
fn text_node_end_offset_with_entities() {
    // "a &lt; b" in raw source → decoded "a < b"
    // Raw: a(1) space(1) &lt;(4) space(1) b(1) = 8 bytes
    let input = "<T>a &lt; b</T>";
    let nodes = parse(input).unwrap();
    if let XmlNode::Element { children, .. } = &nodes[0] {
        if let XmlNode::Text {
            content,
            offset,
            end_offset,
        } = &children[0]
        {
            assert_eq!(content, "a < b", "content should be entity-decoded");
            assert_eq!(*offset, 3, "text starts after '<T>'");
            assert_eq!(
                *end_offset, 11,
                "end_offset should be past the last byte of 'a &lt; b' in raw source"
            );
            assert_eq!(
                &input[*offset..*end_offset],
                "a &lt; b",
                "offset..end_offset should span the raw source text"
            );
            // Decoded length (5) < raw length (8)
            assert!(content.len() < (*end_offset - *offset));
        } else {
            panic!("expected Text");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
fn text_node_end_offset_with_fence_offset_and_entities() {
    let fence_offset = 50;
    let input = "<T>&amp;</T>";
    let nodes = parse_with_offset(input, fence_offset).unwrap();
    if let XmlNode::Element { children, .. } = &nodes[0] {
        if let XmlNode::Text {
            content,
            offset,
            end_offset,
        } = &children[0]
        {
            assert_eq!(content, "&", "decoded entity");
            assert_eq!(*offset, 53, "starts at fence_offset + 3 (after '<T>')");
            // &amp; = 5 bytes in raw source, starts at position 3 in XML content
            assert_eq!(
                *end_offset, 58,
                "end_offset = fence_offset + 3 + 5 (length of '&amp;')"
            );
        } else {
            panic!("expected Text");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
fn text_node_end_offset_multiple_entities() {
    // "&lt;&gt;" → decoded "<>" (2 chars), raw = 8 bytes
    let input = "<T>&lt;&gt;</T>";
    let nodes = parse(input).unwrap();
    if let XmlNode::Element { children, .. } = &nodes[0] {
        if let XmlNode::Text {
            content,
            offset,
            end_offset,
        } = &children[0]
        {
            assert_eq!(content, "<>");
            assert_eq!(*offset, 3);
            assert_eq!(*end_offset, 11, "past '&lt;&gt;' in raw source");
            assert_eq!(&input[*offset..*end_offset], "&lt;&gt;");
        } else {
            panic!("expected Text");
        }
    } else {
        panic!("expected Element");
    }
}

// -- end_offset on Element ------------------------------------------------

#[test]
fn self_closing_element_end_offset() {
    let input = r#"<Spec id="s1" />"#;
    let nodes = parse(input).unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        XmlNode::Element {
            name,
            offset,
            end_offset,
            ..
        } => {
            assert_eq!(name, "Spec");
            assert_eq!(*offset, 0);
            assert_eq!(*end_offset, input.len());
        }
        _ => panic!("expected Element"),
    }
}

#[test]
fn regular_element_end_offset() {
    let input = "<Title>Hello</Title>";
    let nodes = parse(input).unwrap();
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
        XmlNode::Element {
            name,
            offset,
            end_offset,
            ..
        } => {
            assert_eq!(name, "Title");
            assert_eq!(*offset, 0);
            assert_eq!(*end_offset, input.len());
        }
        _ => panic!("expected Element"),
    }
}

#[test]
fn nested_element_end_offsets() {
    let input = r#"<Parent><Child id="c1" /></Parent>"#;
    let nodes = parse(input).unwrap();
    match &nodes[0] {
        XmlNode::Element {
            end_offset,
            children,
            ..
        } => {
            assert_eq!(*end_offset, input.len());
            match &children[0] {
                XmlNode::Element {
                    name,
                    offset,
                    end_offset,
                    ..
                } => {
                    assert_eq!(name, "Child");
                    assert_eq!(*offset, 8);
                    // "<Child id="c1" />" ends at position 25
                    assert_eq!(*end_offset, 25);
                }
                _ => panic!("expected Element"),
            }
        }
        _ => panic!("expected Element"),
    }
}

#[test]
fn element_end_offset_with_fence_offset() {
    let input = r#"<Spec id="s1" />"#;
    let fence_offset = 100;
    let nodes = parse_with_offset(input, fence_offset).unwrap();
    match &nodes[0] {
        XmlNode::Element {
            offset, end_offset, ..
        } => {
            assert_eq!(*offset, 100);
            assert_eq!(*end_offset, 100 + input.len());
        }
        _ => panic!("expected Element"),
    }
}
