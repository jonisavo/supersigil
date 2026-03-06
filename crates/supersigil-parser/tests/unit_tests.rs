// Unit tests for supersigil-parser

mod common;
use common::dummy_path;

// ── Stage 1: Preprocessing ──────────────────────────────────────────────────

mod preprocess {
    use super::*;
    use supersigil_parser::preprocess;

    #[test]
    fn valid_utf8_passthrough() {
        let input = b"hello world";
        let result = preprocess(input, &dummy_path()).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn non_utf8_returns_io_error() {
        // 0xFF 0xFE is not valid UTF-8 (without being a BOM in UTF-8 context)
        let input: &[u8] = &[0x80, 0x81, 0x82];
        let result = preprocess(input, &dummy_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, supersigil_core::ParseError::IoError { .. }),
            "expected IoError, got {err:?}"
        );
    }

    #[test]
    fn bom_stripped() {
        // UTF-8 BOM is EF BB BF
        let mut input = vec![0xEF, 0xBB, 0xBF];
        input.extend_from_slice(b"content after bom");
        let result = preprocess(&input, &dummy_path()).unwrap();
        assert_eq!(result, "content after bom");
    }

    #[test]
    fn no_bom_content_unchanged() {
        let input = b"no bom here";
        let result = preprocess(input, &dummy_path()).unwrap();
        assert_eq!(result, "no bom here");
    }

    #[test]
    fn file_with_only_bom_produces_empty_string() {
        let input: &[u8] = &[0xEF, 0xBB, 0xBF];
        let result = preprocess(input, &dummy_path()).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn bom_followed_by_front_matter_delimiter() {
        let mut input = vec![0xEF, 0xBB, 0xBF];
        input.extend_from_slice(b"---\nsupersigil:\n  id: test\n---\n");
        let result = preprocess(&input, &dummy_path()).unwrap();
        assert!(
            result.starts_with("---"),
            "BOM should be stripped, leaving --- at start"
        );
    }

    #[test]
    fn crlf_normalized_to_lf() {
        let input = b"line1\r\nline2\r\nline3";
        let result = preprocess(input, &dummy_path()).unwrap();
        assert_eq!(result, "line1\nline2\nline3");
        assert!(!result.contains("\r\n"), "no CRLF should remain");
    }

    #[test]
    fn mixed_crlf_and_lf_normalized_bare_cr_preserved() {
        // Mix of \r\n and \n, plus a bare \r
        let input = b"a\r\nb\nc\rd\r\ne";
        let result = preprocess(input, &dummy_path()).unwrap();
        assert_eq!(result, "a\nb\nc\rd\ne");
        assert!(!result.contains("\r\n"), "no CRLF should remain");
        assert!(result.contains('\r'), "bare \\r should be preserved");
    }

    #[test]
    fn file_with_only_crlf() {
        let input = b"\r\n";
        let result = preprocess(input, &dummy_path()).unwrap();
        assert_eq!(result, "\n");
    }
}

// ── Stage 1: Front matter extraction ────────────────────────────────────────

mod extract_front_matter {
    use super::*;
    use supersigil_parser::extract_front_matter;

    #[test]
    fn valid_front_matter_extracts_yaml_and_body() {
        let content = "---\nsupersigil:\n  id: test\n---\nbody";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "supersigil:\n  id: test\n");
        assert_eq!(body, "body");
    }

    #[test]
    fn delimiter_with_trailing_whitespace_accepted() {
        let content = "---  \nsupersigil:\n  id: test\n---  \nbody text";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "supersigil:\n  id: test\n");
        assert_eq!(body, "body text");
    }

    #[test]
    fn no_opening_delimiter_returns_none() {
        let content = "no front matter here\njust content";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        assert!(result.is_none(), "expected None for no opening ---");
    }

    #[test]
    fn unclosed_front_matter_returns_error() {
        let content = "---\nsupersigil:\n  id: test\nno closing delimiter";
        let result = extract_front_matter(content, &dummy_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, supersigil_core::ParseError::UnclosedFrontMatter { .. }),
            "expected UnclosedFrontMatter, got {err:?}"
        );
    }

    #[test]
    fn empty_yaml_between_delimiters() {
        let content = "---\n---\nbody";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "");
        assert_eq!(body, "body");
    }

    #[test]
    fn triple_dash_inside_yaml_terminates_front_matter() {
        // The first `---` on its own line after the opening closes the front matter.
        // Multi-document YAML separators are not supported.
        let content = "---\nkey: value\n---\nmore content\n---\nfinal";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "key: value\n");
        assert_eq!(body, "more content\n---\nfinal");
    }

    #[test]
    fn opening_delimiter_not_on_first_line_returns_none() {
        let content = "some text\n---\nsupersigil:\n  id: test\n---\n";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn empty_content_returns_none() {
        let content = "";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn only_opening_delimiter_returns_error() {
        let content = "---\n";
        let result = extract_front_matter(content, &dummy_path());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            supersigil_core::ParseError::UnclosedFrontMatter { .. }
        ));
    }

    #[test]
    fn body_after_closing_delimiter_with_no_trailing_newline() {
        let content = "---\nid: x\n---";
        let result = extract_front_matter(content, &dummy_path()).unwrap();
        let (yaml, body) = result.unwrap();
        assert_eq!(yaml, "id: x\n");
        assert_eq!(body, "");
    }
}

// ── Stage 1: Front matter deserialization ───────────────────────────────────

mod deserialize_front_matter {
    use super::*;
    use supersigil_parser::{FrontMatterResult, deserialize_front_matter};

    #[test]
    fn valid_supersigil_with_all_fields() {
        let yaml = "supersigil:\n  id: my-doc\n  type: requirement\n  status: draft\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        match result {
            FrontMatterResult::Supersigil { frontmatter, extra } => {
                assert_eq!(frontmatter.id, "my-doc");
                assert_eq!(frontmatter.doc_type.as_deref(), Some("requirement"));
                assert_eq!(frontmatter.status.as_deref(), Some("draft"));
                assert!(extra.is_empty());
            }
            FrontMatterResult::NotSupersigil => panic!("expected Supersigil, got NotSupersigil"),
        }
    }

    #[test]
    fn supersigil_with_only_id() {
        let yaml = "supersigil:\n  id: minimal\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        match result {
            FrontMatterResult::Supersigil { frontmatter, extra } => {
                assert_eq!(frontmatter.id, "minimal");
                assert!(frontmatter.doc_type.is_none());
                assert!(frontmatter.status.is_none());
                assert!(extra.is_empty());
            }
            FrontMatterResult::NotSupersigil => panic!("expected Supersigil"),
        }
    }

    #[test]
    fn missing_id_returns_error() {
        let yaml = "supersigil:\n  type: requirement\n";
        let result = deserialize_front_matter(yaml, &dummy_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, supersigil_core::ParseError::MissingId { .. }),
            "expected MissingId, got {err:?}"
        );
    }

    #[test]
    fn invalid_yaml_returns_error() {
        let yaml = ":\n  - :\n    bad: [yaml\n";
        let result = deserialize_front_matter(yaml, &dummy_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, supersigil_core::ParseError::InvalidYaml { .. }),
            "expected InvalidYaml, got {err:?}"
        );
    }

    #[test]
    fn no_supersigil_key_returns_not_supersigil() {
        let yaml = "title: My Document\nauthor: someone\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        assert!(
            matches!(result, FrontMatterResult::NotSupersigil),
            "expected NotSupersigil"
        );
    }

    #[test]
    fn extra_metadata_keys_preserved() {
        let yaml = "supersigil:\n  id: doc-1\ntitle: My Doc\nauthor: dev\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        match result {
            FrontMatterResult::Supersigil { frontmatter, extra } => {
                assert_eq!(frontmatter.id, "doc-1");
                assert_eq!(extra.len(), 2);
                assert_eq!(extra.get("title").and_then(|v| v.as_str()), Some("My Doc"));
                assert_eq!(extra.get("author").and_then(|v| v.as_str()), Some("dev"));
            }
            FrontMatterResult::NotSupersigil => panic!("expected Supersigil"),
        }
    }

    #[test]
    fn supersigil_inline_syntax_with_extra_keys() {
        let yaml = "supersigil: { id: x }\nversion: 2\n";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        match result {
            FrontMatterResult::Supersigil { frontmatter, extra } => {
                assert_eq!(frontmatter.id, "x");
                assert!(frontmatter.doc_type.is_none());
                assert!(frontmatter.status.is_none());
                assert_eq!(extra.len(), 1);
                assert!(extra.contains_key("version"));
            }
            FrontMatterResult::NotSupersigil => panic!("expected Supersigil"),
        }
    }

    #[test]
    fn empty_yaml_returns_not_supersigil() {
        let yaml = "";
        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        assert!(matches!(result, FrontMatterResult::NotSupersigil));
    }

    #[test]
    fn supersigil_empty_object_missing_id_returns_error() {
        let yaml = "supersigil: {}\n";
        let result = deserialize_front_matter(yaml, &dummy_path());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            supersigil_core::ParseError::MissingId { .. }
        ));
    }
}

// ---------------------------------------------------------------------------
// Stage 2: MDX AST generation
// ---------------------------------------------------------------------------

mod mdx_parsing {
    use super::dummy_path;

    use markdown::mdast::Node;
    use supersigil_parser::parse_mdx_body;

    fn find_flow_elements(node: &Node) -> Vec<&markdown::mdast::MdxJsxFlowElement> {
        let mut found = Vec::new();
        if let Node::MdxJsxFlowElement(el) = node {
            found.push(el);
        }
        if let Some(children) = node.children() {
            for child in children {
                found.extend(find_flow_elements(child));
            }
        }
        found
    }

    #[test]
    fn valid_mdx_body_produces_ast() {
        let body = "# Hello\n\nSome text here.\n";
        let result = parse_mdx_body(body, &dummy_path());
        assert!(result.is_ok());
        let node = result.unwrap();
        assert!(matches!(node, Node::Root(_)));
    }

    #[test]
    fn invalid_mdx_syntax_returns_error() {
        // Unclosed JSX tag is invalid MDX
        let body = "<Component>\n";
        let result = parse_mdx_body(body, &dummy_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            supersigil_core::ParseError::MdxSyntaxError { .. }
        ));
        // Verify the error contains position and message
        if let supersigil_core::ParseError::MdxSyntaxError { line, message, .. } = err {
            assert!(line > 0, "line should be > 0");
            assert!(!message.is_empty(), "message should not be empty");
        }
    }

    #[test]
    fn body_with_pascal_case_components_produces_flow_elements() {
        let body =
            "<Validates refs=\"REQ-1\" />\n\n<Criterion id=\"c1\">\nSome text\n</Criterion>\n";
        let result = parse_mdx_body(body, &dummy_path());
        assert!(result.is_ok());
        let node = result.unwrap();

        let elements = find_flow_elements(&node);
        assert_eq!(elements.len(), 2, "should find 2 flow elements");

        // Check names
        let names: Vec<_> = elements
            .iter()
            .filter_map(|el| el.name.as_deref())
            .collect();
        assert!(names.contains(&"Validates"));
        assert!(names.contains(&"Criterion"));
    }

    #[test]
    fn body_with_lowercase_html_elements_in_ast() {
        // With MDX constructs, lowercase elements are parsed as MdxJsxFlowElement
        // but with lowercase names (they are standard HTML, not supersigil components).
        // The parser should still produce an AST containing them.
        let body = "<div>\n\nsome content\n\n</div>\n";
        let result = parse_mdx_body(body, &dummy_path());
        assert!(result.is_ok());
        let node = result.unwrap();

        let elements = find_flow_elements(&node);
        // div should appear as a flow element in the AST
        assert!(
            !elements.is_empty(),
            "lowercase HTML elements should appear in AST"
        );
        let names: Vec<_> = elements
            .iter()
            .filter_map(|el| el.name.as_deref())
            .collect();
        assert!(names.contains(&"div"));
    }
}

// ---------------------------------------------------------------------------
// Stage 3: Component extraction
// ---------------------------------------------------------------------------

mod component_extraction {
    use super::common::extract;
    use supersigil_core::ParseError;

    // ── Req 8.1: PascalCase flow element extracted with name and attributes ──

    #[test]
    fn pascal_case_flow_element_extracted_with_name_and_attributes() {
        let body = "<Validates refs=\"REQ-1, REQ-2\" />\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);

        let comp = &components[0];
        assert_eq!(comp.name, "Validates");
        assert_eq!(
            comp.attributes.get("refs").map(String::as_str),
            Some("REQ-1, REQ-2")
        );
    }

    // ── Req 8.1: Lowercase element (div, p) silently ignored ──

    #[test]
    fn lowercase_element_silently_ignored() {
        let body = "<div>\n\nsome content\n\n</div>\n";
        let (components, errors) = extract(body, 0);

        // div is lowercase HTML — should not be extracted as a component
        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert!(
            components.is_empty(),
            "lowercase elements should be ignored, got: {components:?}"
        );
    }

    #[test]
    fn lowercase_p_element_silently_ignored() {
        let body = "<p>\n\ntext\n\n</p>\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert!(components.is_empty(), "lowercase <p> should be ignored");
    }

    // ── Req 8.6: Inline JSX (MdxJsxTextElement) ignored ──

    #[test]
    fn inline_jsx_text_element_ignored() {
        // Inline JSX appears within a paragraph (not on its own block).
        // markdown-rs parses inline JSX as MdxJsxTextElement.
        let body = "Some text with <Validates refs=\"REQ-1\" /> inline.\n";
        let (components, errors) = extract(body, 0);

        // Inline JSX should be ignored — only block-level flow elements extracted
        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert!(
            components.is_empty(),
            "inline JSX (MdxJsxTextElement) should be ignored, got: {components:?}"
        );
    }

    // ── Req 6.1, 6.4: String literal attributes stored as raw strings ──

    #[test]
    fn string_literal_attributes_stored_as_raw_strings() {
        let body = "<Criterion id=\"crit-1\" />\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);

        let comp = &components[0];
        assert_eq!(comp.name, "Criterion");
        // Attribute value should be the exact raw string, no transformation
        assert_eq!(
            comp.attributes.get("id").map(String::as_str),
            Some("crit-1")
        );
    }

    #[test]
    fn multiple_string_attributes_preserved() {
        let body = "<VerifiedBy strategy=\"unit-test\" tag=\"auth\" />\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);

        let comp = &components[0];
        assert_eq!(
            comp.attributes.get("strategy").map(String::as_str),
            Some("unit-test")
        );
        assert_eq!(comp.attributes.get("tag").map(String::as_str), Some("auth"));
    }

    // ── Req 6.2, 6.3: Expression attribute {…} → ExpressionAttribute error ──

    #[test]
    fn expression_attribute_produces_error_and_is_excluded() {
        let body = "<Validates refs={someExpr} />\n";
        let (components, errors) = extract(body, 0);

        // The component should still be extracted, but the expression attribute excluded
        assert_eq!(components.len(), 1);
        let comp = &components[0];
        assert_eq!(comp.name, "Validates");
        assert!(
            !comp.attributes.contains_key("refs"),
            "expression attribute should be excluded from attributes"
        );

        // An ExpressionAttribute error should be recorded
        assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
        assert!(
            matches!(
                &errors[0],
                ParseError::ExpressionAttribute {
                    component,
                    attribute,
                    ..
                } if component == "Validates" && attribute == "refs"
            ),
            "expected ExpressionAttribute error, got: {:?}",
            errors[0]
        );
    }

    // ── Req 8.4: Self-closing component → body_text is None ──

    #[test]
    fn self_closing_component_body_text_is_none() {
        let body = "<Validates refs=\"REQ-1\" />\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);
        assert!(
            components[0].body_text.is_none(),
            "self-closing component should have body_text = None"
        );
    }

    // ── Req 8.3: Component with text content → body_text is trimmed concatenation ──

    #[test]
    fn component_with_text_content_has_trimmed_body_text() {
        let body = "<Criterion id=\"c1\">\n  Some criterion text  \n</Criterion>\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);

        let comp = &components[0];
        assert_eq!(comp.name, "Criterion");
        assert_eq!(
            comp.body_text.as_deref(),
            Some("Some criterion text"),
            "body_text should be trimmed concatenation of text nodes"
        );
    }

    // ── Req 8.5: Component with only child components → body_text is None ──

    #[test]
    fn component_with_only_child_components_body_text_is_none() {
        let body = "<AcceptanceCriteria>\n\n<Criterion id=\"c1\">\nText\n</Criterion>\n\n</AcceptanceCriteria>\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);

        let parent = &components[0];
        assert_eq!(parent.name, "AcceptanceCriteria");
        assert!(
            parent.body_text.is_none(),
            "component with only child components should have body_text = None, got: {:?}",
            parent.body_text
        );
    }

    // ── Req 9.1, 9.2: Nested components: parent's children list populated ──

    #[test]
    fn nested_components_parent_children_populated() {
        let body = "<AcceptanceCriteria>\n\n<Criterion id=\"c1\">\nFirst criterion\n</Criterion>\n\n<Criterion id=\"c2\">\nSecond criterion\n</Criterion>\n\n</AcceptanceCriteria>\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(
            components.len(),
            1,
            "only top-level parent should be in root list"
        );

        let parent = &components[0];
        assert_eq!(parent.name, "AcceptanceCriteria");
        assert_eq!(parent.children.len(), 2, "parent should have 2 children");

        assert_eq!(parent.children[0].name, "Criterion");
        assert_eq!(
            parent.children[0].attributes.get("id").map(String::as_str),
            Some("c1")
        );
        assert_eq!(
            parent.children[0].body_text.as_deref(),
            Some("First criterion")
        );

        assert_eq!(parent.children[1].name, "Criterion");
        assert_eq!(
            parent.children[1].attributes.get("id").map(String::as_str),
            Some("c2")
        );
        assert_eq!(
            parent.children[1].body_text.as_deref(),
            Some("Second criterion")
        );
    }

    // ── Req 9.2: Recursive nesting preserved ──

    #[test]
    fn recursive_nesting_preserved() {
        // Three levels: AcceptanceCriteria > Criterion > Validates
        let body = "\
<AcceptanceCriteria>

<Criterion id=\"c1\">

<Validates refs=\"REQ-1\" />

</Criterion>

</AcceptanceCriteria>
";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);

        let ac = &components[0];
        assert_eq!(ac.name, "AcceptanceCriteria");
        assert_eq!(ac.children.len(), 1);

        let criterion = &ac.children[0];
        assert_eq!(criterion.name, "Criterion");
        assert_eq!(criterion.children.len(), 1);

        let validates = &criterion.children[0];
        assert_eq!(validates.name, "Validates");
        assert_eq!(
            validates.attributes.get("refs").map(String::as_str),
            Some("REQ-1")
        );
        assert!(validates.children.is_empty());
    }

    // ── Req 9.3: Component with no children has empty children list ──

    #[test]
    fn component_with_no_children_has_empty_children_list() {
        let body = "<Criterion id=\"c1\">\nSome text\n</Criterion>\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);
        assert!(
            components[0].children.is_empty(),
            "component with no child components should have empty children list"
        );
    }

    // ── Req 8.2: Source position offset applied ──

    #[test]
    fn source_position_offset_applied() {
        let body = "<Validates refs=\"REQ-1\" />\n";
        // Simulate front matter of 50 bytes
        let body_offset = 50;
        let (components, errors) = extract(body, body_offset);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);

        let pos = &components[0].position;
        // The byte_offset should include the front matter offset
        assert!(
            pos.byte_offset >= body_offset,
            "byte_offset ({}) should be >= body_offset ({})",
            pos.byte_offset,
            body_offset
        );
    }

    #[test]
    fn source_position_without_offset_starts_at_zero() {
        let body = "<Validates refs=\"REQ-1\" />\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);

        let pos = &components[0].position;
        // With zero offset, position should be at the start of the body
        assert_eq!(
            pos.byte_offset, 0,
            "with zero offset, byte_offset should be 0"
        );
        assert_eq!(pos.line, 1, "first component should be on line 1");
        assert_eq!(pos.column, 1, "first component should start at column 1");
    }

    // ── body_text includes inline code (backtick) content ──

    #[test]
    fn body_text_includes_inline_code() {
        let body = "<Criterion id=\"c1\">\n  WHEN the `id` field is missing, THE Parser SHALL emit an error.\n</Criterion>\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);
        assert_eq!(
            components[0].body_text.as_deref(),
            Some("WHEN the `id` field is missing, THE Parser SHALL emit an error."),
            "body_text should include text from inline code nodes"
        );
    }

    #[test]
    fn body_text_includes_multiple_inline_code_spans() {
        let body =
            "<Criterion id=\"c1\">\n  The `foo` and `bar` fields are required.\n</Criterion>\n";
        let (components, errors) = extract(body, 0);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 1);
        assert_eq!(
            components[0].body_text.as_deref(),
            Some("The `foo` and `bar` fields are required."),
            "body_text should include text from all inline code spans"
        );
    }

    #[test]
    fn source_position_offset_for_second_component() {
        let body = "<Validates refs=\"REQ-1\" />\n\n<Criterion id=\"c1\">\nText\n</Criterion>\n";
        let body_offset = 100;
        let (components, errors) = extract(body, body_offset);

        assert!(errors.is_empty(), "no errors expected, got: {errors:?}");
        assert_eq!(components.len(), 2);

        // Both components should have offsets >= body_offset
        assert!(
            components[0].position.byte_offset >= body_offset,
            "first component byte_offset should include front matter offset"
        );
        assert!(
            components[1].position.byte_offset > components[0].position.byte_offset,
            "second component should be after first"
        );
    }
}

// ===========================================================================
// Lint-time validation tests (Task 12.1)
// Requirements: 21.1, 21.2, 21.3, 25.1, 25.2
// ===========================================================================

mod lint_validation {
    use super::dummy_path;
    use std::collections::HashMap;
    use supersigil_core::{
        AttributeDef, ComponentDef, ComponentDefs, ExtractedComponent, ParseError, SourcePosition,
    };
    use supersigil_parser::validate_components;

    fn make_component(name: &str, attrs: &[(&str, &str)]) -> ExtractedComponent {
        ExtractedComponent {
            name: name.to_string(),
            attributes: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            children: Vec::new(),
            body_text: None,
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
        }
    }

    // ── Req 25.1: Unknown PascalCase component → UnknownComponent error ──

    #[test]
    fn unknown_pascal_case_component_produces_error() {
        let defs = ComponentDefs::defaults();
        let components = vec![make_component("FooBarBaz", &[])];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
        assert!(
            matches!(&errors[0], ParseError::UnknownComponent { component, .. } if component == "FooBarBaz"),
            "expected UnknownComponent for FooBarBaz, got: {:?}",
            errors[0]
        );
    }

    // ── Req 25.1: Known component → no error ──

    #[test]
    fn known_component_no_error() {
        let defs = ComponentDefs::defaults();
        // Criterion is a built-in with required `id`
        let components = vec![make_component("Criterion", &[("id", "c1")])];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    // ── Req 25.1: Lowercase element name → no unknown component error ──

    #[test]
    fn lowercase_element_no_unknown_component_error() {
        let defs = ComponentDefs::defaults();
        // Lowercase names should never produce UnknownComponent errors
        let components = vec![make_component("div", &[])];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert!(
            errors.is_empty(),
            "lowercase elements should not produce errors, got: {errors:?}"
        );
    }

    // ── Req 21.1: Missing required attribute → MissingRequiredAttribute error ──

    #[test]
    fn missing_required_attribute_produces_error() {
        let defs = ComponentDefs::defaults();
        // Criterion requires `id`, but we omit it
        let components = vec![make_component("Criterion", &[])];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
        assert!(
            matches!(
                &errors[0],
                ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "Criterion" && attribute == "id"
            ),
            "expected MissingRequiredAttribute for Criterion.id, got: {:?}",
            errors[0]
        );
    }

    // ── Req 21.1: Error includes component name, attribute, and position ──

    #[test]
    fn missing_required_attribute_includes_position() {
        let defs = ComponentDefs::defaults();
        let mut comp = make_component("Criterion", &[]);
        comp.position = SourcePosition {
            byte_offset: 42,
            line: 5,
            column: 3,
        };
        let components = vec![comp];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert_eq!(errors.len(), 1);
        match &errors[0] {
            ParseError::MissingRequiredAttribute {
                path,
                component,
                attribute,
                position,
            } => {
                assert_eq!(path, &dummy_path());
                assert_eq!(component, "Criterion");
                assert_eq!(attribute, "id");
                assert_eq!(position.byte_offset, 42);
                assert_eq!(position.line, 5);
                assert_eq!(position.column, 3);
            }
            other => panic!("expected MissingRequiredAttribute, got: {other:?}"),
        }
    }

    // ── Req 21.2: All required attributes present → no error ──

    #[test]
    fn all_required_attributes_present_no_error() {
        let defs = ComponentDefs::defaults();
        // VerifiedBy requires `strategy`
        let components = vec![make_component("VerifiedBy", &[("strategy", "unit-test")])];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    // ── Req 21.3: Validation uses config component defs when provided ──

    #[test]
    fn validation_uses_custom_component_defs() {
        // Create custom defs with a "Widget" component requiring "color"
        let user_defs = HashMap::from([(
            "Widget".to_string(),
            ComponentDef {
                attributes: HashMap::from([(
                    "color".to_string(),
                    AttributeDef {
                        required: true,
                        list: false,
                    },
                )]),
                referenceable: false,
                target_component: None,
                description: None,
                examples: Vec::new(),
            },
        )]);
        let defs = ComponentDefs::merge(ComponentDefs::defaults(), user_defs);

        // Widget is known, but missing required `color`
        let components = vec![make_component("Widget", &[])];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
        assert!(
            matches!(
                &errors[0],
                ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "Widget" && attribute == "color"
            ),
            "expected MissingRequiredAttribute for Widget.color, got: {:?}",
            errors[0]
        );
    }

    // ── Req 25.2: Validation uses built-in defaults when no config ──

    #[test]
    fn validation_uses_builtin_defaults_when_no_config() {
        let defs = ComponentDefs::defaults();
        // "Validates" is a built-in requiring `refs`
        let components = vec![make_component("Validates", &[])];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
        assert!(
            matches!(
                &errors[0],
                ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "Validates" && attribute == "refs"
            ),
            "expected MissingRequiredAttribute for Validates.refs, got: {:?}",
            errors[0]
        );
    }

    // ── Multiple errors: unknown + missing required on different components ──

    #[test]
    fn multiple_validation_errors_collected() {
        let defs = ComponentDefs::defaults();
        let components = vec![
            make_component("UnknownThing", &[]),
            make_component("Criterion", &[]), // missing required `id`
        ];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert_eq!(errors.len(), 2, "expected 2 errors, got: {errors:?}");
        // One should be UnknownComponent, one should be MissingRequiredAttribute
        let has_unknown = errors
            .iter()
            .any(|e| matches!(e, ParseError::UnknownComponent { .. }));
        let has_missing = errors
            .iter()
            .any(|e| matches!(e, ParseError::MissingRequiredAttribute { .. }));
        assert!(has_unknown, "expected UnknownComponent error");
        assert!(has_missing, "expected MissingRequiredAttribute error");
    }

    // ── Nested children are also validated ──

    #[test]
    fn nested_children_validated() {
        let defs = ComponentDefs::defaults();
        let mut parent = make_component("AcceptanceCriteria", &[]);
        // Child Criterion missing required `id`
        parent.children.push(make_component("Criterion", &[]));
        let components = vec![parent];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert_eq!(
            errors.len(),
            1,
            "expected 1 error for nested child, got: {errors:?}"
        );
        assert!(
            matches!(
                &errors[0],
                ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "Criterion" && attribute == "id"
            ),
            "expected MissingRequiredAttribute for nested Criterion.id, got: {:?}",
            errors[0]
        );
    }
}

// ===========================================================================
// parse_file integration tests (Task 12.2)
// Requirements: 10.1, 10.2, 10.3, 21.4, 25.3
// ===========================================================================

mod parse_file_integration {
    use std::io::Write;
    use supersigil_core::{ComponentDefs, ParseError, ParseResult};
    use supersigil_parser::parse_file;

    fn write_temp_file(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    // ── Req 10.1: Full pipeline → ParseResult::Document ──

    #[test]
    fn valid_mdx_file_produces_document() {
        let content = "---\nsupersigil:\n  id: my-spec\n  type: requirement\n  status: draft\ntitle: My Spec\n---\n<Criterion id=\"c1\">\nSome criterion text\n</Criterion>\n";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        let pr = result.unwrap();
        match pr {
            ParseResult::Document(doc) => {
                assert_eq!(doc.frontmatter.id, "my-spec");
                assert_eq!(doc.frontmatter.doc_type.as_deref(), Some("requirement"));
                assert_eq!(doc.frontmatter.status.as_deref(), Some("draft"));
                assert!(
                    doc.extra.contains_key("title"),
                    "extra should contain 'title'"
                );
                assert_eq!(doc.components.len(), 1);
                assert_eq!(doc.components[0].name, "Criterion");
                assert_eq!(
                    doc.components[0].attributes.get("id").map(String::as_str),
                    Some("c1")
                );
            }
            ParseResult::NotSupersigil(_) => panic!("expected Document, got NotSupersigil"),
        }
    }

    // ── Req 10.2: File without front matter → NotSupersigil ──

    #[test]
    fn file_without_front_matter_returns_not_supersigil() {
        let content = "# Just a markdown file\n\nSome content.\n";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs).unwrap();

        assert!(
            matches!(result, ParseResult::NotSupersigil(_)),
            "expected NotSupersigil, got: {result:?}"
        );
    }

    // ── Req 10.2: Front matter without supersigil key → NotSupersigil ──

    #[test]
    fn front_matter_without_supersigil_key_returns_not_supersigil() {
        let content = "---\ntitle: Not a spec\nauthor: someone\n---\nBody content.\n";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs).unwrap();

        assert!(
            matches!(result, ParseResult::NotSupersigil(_)),
            "expected NotSupersigil, got: {result:?}"
        );
    }

    // ── Req 10.3: Error collection — multiple errors returned ──

    #[test]
    fn multiple_stage3_errors_all_collected() {
        // File with an unknown component AND a known component missing required attr
        let content = "---\nsupersigil:\n  id: test\n---\n<UnknownWidget />\n\n<Criterion />\n";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        assert!(result.is_err(), "expected errors, got: {result:?}");
        let errors = result.unwrap_err();
        assert!(
            errors.len() >= 2,
            "expected at least 2 errors, got {}: {errors:?}",
            errors.len()
        );

        let has_unknown = errors
            .iter()
            .any(|e| matches!(e, ParseError::UnknownComponent { component, .. } if component == "UnknownWidget"));
        let has_missing = errors
            .iter()
            .any(|e| matches!(e, ParseError::MissingRequiredAttribute { component, attribute, .. } if component == "Criterion" && attribute == "id"));

        assert!(
            has_unknown,
            "expected UnknownComponent error for UnknownWidget"
        );
        assert!(
            has_missing,
            "expected MissingRequiredAttribute for Criterion.id"
        );
    }

    // ── Req 10.3: Stage 1 fatal error prevents stages 2-3 ──

    #[test]
    fn stage1_fatal_error_prevents_later_stages() {
        // Unclosed front matter — stage 1 fatal error
        let content = "---\nsupersigil:\n  id: test\n";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        assert!(result.is_err(), "expected error, got: {result:?}");
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1, "expected exactly 1 error, got: {errors:?}");
        assert!(
            matches!(&errors[0], ParseError::UnclosedFrontMatter { .. }),
            "expected UnclosedFrontMatter, got: {:?}",
            errors[0]
        );
    }

    // ── Req 10.3: Stage 2 error prevents stage 3 ──

    #[test]
    fn stage2_error_prevents_stage3() {
        // Valid front matter but invalid MDX body
        let content = "---\nsupersigil:\n  id: test\n---\n<Criterion id=\"c1\">\n";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        assert!(result.is_err(), "expected error, got: {result:?}");
        let errors = result.unwrap_err();
        // Should have MDX syntax error, no stage 3 errors
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ParseError::MdxSyntaxError { .. })),
            "expected MdxSyntaxError, got: {errors:?}"
        );
        // Should NOT have any stage 3 errors (unknown component, missing attr)
        assert!(
            !errors.iter().any(|e| matches!(
                e,
                ParseError::UnknownComponent { .. } | ParseError::MissingRequiredAttribute { .. }
            )),
            "stage 3 errors should not appear when stage 2 fails"
        );
    }

    // ── Req 10.3: Stage 3 errors collected together ──

    #[test]
    fn stage3_expression_attr_and_validation_errors_collected() {
        // Expression attribute + unknown component + missing required attr
        let content = "---\nsupersigil:\n  id: test\n---\n<Criterion id={expr} />\n\n<UnknownComp />\n\n<Validates />\n";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        assert!(result.is_err(), "expected errors, got: {result:?}");
        let errors = result.unwrap_err();

        let has_expr = errors
            .iter()
            .any(|e| matches!(e, ParseError::ExpressionAttribute { .. }));
        let has_unknown = errors
            .iter()
            .any(|e| matches!(e, ParseError::UnknownComponent { .. }));
        let has_missing_criterion = errors.iter().any(|e| {
            matches!(
                e,
                ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "Criterion" && attribute == "id"
            )
        });
        let has_missing_validates = errors.iter().any(|e| {
            matches!(
                e,
                ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "Validates" && attribute == "refs"
            )
        });

        assert!(has_expr, "expected ExpressionAttribute error");
        assert!(has_unknown, "expected UnknownComponent error");
        assert!(
            has_missing_criterion,
            "expected MissingRequiredAttribute for Criterion.id"
        );
        assert!(
            has_missing_validates,
            "expected MissingRequiredAttribute for Validates.refs"
        );
    }

    // ── BOM + CRLF handled in full pipeline ──

    #[test]
    fn bom_and_crlf_handled_in_pipeline() {
        let mut content = Vec::new();
        // UTF-8 BOM
        content.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        content.extend_from_slice(b"---\r\nsupersigil:\r\n  id: bom-test\r\n---\r\n<Criterion id=\"c1\">\r\nText\r\n</Criterion>\r\n");
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&content).unwrap();
        f.flush().unwrap();
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        match result.unwrap() {
            ParseResult::Document(doc) => {
                assert_eq!(doc.frontmatter.id, "bom-test");
                assert_eq!(doc.components.len(), 1);
                assert_eq!(doc.components[0].name, "Criterion");
            }
            ParseResult::NotSupersigil(_) => panic!("expected Document"),
        }
    }

    // ── Missing id in front matter → fatal error ──

    #[test]
    fn missing_id_is_fatal() {
        let content = "---\nsupersigil:\n  type: spec\n---\n<Criterion id=\"c1\" />\n";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        assert!(result.is_err(), "expected error, got: {result:?}");
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ParseError::MissingId { .. }),
            "expected MissingId, got: {:?}",
            errors[0]
        );
    }
}
