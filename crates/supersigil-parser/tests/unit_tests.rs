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
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: Vec::new(),
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            end_position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
        }
    }

    // ── Unknown PascalCase component produces no errors ──
    // Unknown components are now skipped during extraction, so validation
    // never sees them.

    #[test]
    fn unknown_pascal_case_component_no_error() {
        let defs = ComponentDefs::defaults();
        let components = vec![make_component("FooBarBaz", &[])];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert!(
            errors.is_empty(),
            "unknown components should produce no errors, got: {errors:?}"
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

    // ── Lowercase element name → no errors ──

    #[test]
    fn lowercase_element_no_errors() {
        let defs = ComponentDefs::defaults();
        // Lowercase names should never produce errors
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
                verifiable: false,
                target_component: None,
                description: None,
                examples: Vec::new(),
            },
        )]);
        let defs = ComponentDefs::merge(ComponentDefs::defaults(), user_defs).unwrap();

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
        // "References" is a built-in requiring `refs`
        let components = vec![make_component("References", &[])];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
        assert!(
            matches!(
                &errors[0],
                ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "References" && attribute == "refs"
            ),
            "expected MissingRequiredAttribute for References.refs, got: {:?}",
            errors[0]
        );
    }

    // ── Multiple errors: unknown component skipped, missing required attr detected ──

    #[test]
    fn multiple_validation_errors_collected() {
        let defs = ComponentDefs::defaults();
        let components = vec![
            make_component("UnknownThing", &[]), // unknown, skipped by validate
            make_component("Criterion", &[]),    // missing required `id`
        ];
        let mut errors = Vec::new();

        validate_components(&components, &defs, &dummy_path(), &mut errors);

        // Only 1 error: MissingRequiredAttribute for Criterion (UnknownThing is skipped)
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

    // ── Error collection — unknown component skipped, missing attr collected ──

    #[test]
    fn multiple_stage3_errors_all_collected() {
        // File with an unknown component (skipped) AND a known component missing required attr
        let content = "\
---
supersigil:
  id: test
---

```supersigil-xml
<UnknownWidget />
<Criterion />
```
";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        assert!(result.is_err(), "expected errors, got: {result:?}");
        let errors = result.unwrap_err();
        // Only 1 error: MissingRequiredAttribute for Criterion (UnknownWidget is skipped)
        assert_eq!(
            errors.len(),
            1,
            "expected 1 error, got {}: {errors:?}",
            errors.len()
        );

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

    // ── Stage 1 fatal error prevents later stages ──

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

    // ── XML syntax error in one fence does not prevent other fences ──

    #[test]
    fn xml_error_in_one_fence_does_not_block_others() {
        let content = "\
---
supersigil:
  id: test
---

```supersigil-xml
<Criterion id=\"c1\">unclosed
```

```supersigil-xml
<References refs=\"other\" />
```
";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        // Should have errors (at least the XML syntax error) but also
        // extract References from the second fence.
        assert!(result.is_err(), "expected errors, got: {result:?}");
        let errors = result.unwrap_err();
        let has_xml_error = errors
            .iter()
            .any(|e| matches!(e, ParseError::XmlSyntaxError { .. }));
        assert!(has_xml_error, "expected XmlSyntaxError, got: {errors:?}");
    }

    // ── Validation errors collected from XML pipeline ──

    #[test]
    fn validation_errors_collected() {
        // Criterion missing required id attribute, References missing refs
        let content = "\
---
supersigil:
  id: test
---

```supersigil-xml
<Criterion />
<References />
```
";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs);

        assert!(result.is_err(), "expected errors, got: {result:?}");
        let errors = result.unwrap_err();

        let has_missing_criterion = errors.iter().any(|e| {
            matches!(
                e,
                ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "Criterion" && attribute == "id"
            )
        });
        let has_missing_references = errors.iter().any(|e| {
            matches!(
                e,
                ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "References" && attribute == "refs"
            )
        });

        assert!(
            has_missing_criterion,
            "expected MissingRequiredAttribute for Criterion.id"
        );
        assert!(
            has_missing_references,
            "expected MissingRequiredAttribute for References.refs"
        );
    }

    // ── BOM + CRLF handled in full pipeline ──

    #[test]
    fn bom_and_crlf_handled_in_pipeline() {
        let mut content = Vec::new();
        // UTF-8 BOM
        content.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        content.extend_from_slice(
            b"---\r\nsupersigil:\r\n  id: bom-test\r\n---\r\n\r\n```supersigil-xml\r\n<Criterion id=\"c1\">\r\nText\r\n</Criterion>\r\n```\r\n",
        );
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

    // ── Missing id in front matter is fatal ──

    #[test]
    fn missing_id_is_fatal() {
        let content = "\
---
supersigil:
  type: spec
---

```supersigil-xml
<Criterion id=\"c1\" />
```
";
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

    // ── Known-only extraction behavior ──

    #[test]
    fn unknown_pascal_case_skipped_during_extraction() {
        let content = "\
---
supersigil:
  id: test
---

```supersigil-xml
<Aside>This is a note</Aside>
<Criterion id=\"c1\">
Some criterion
</Criterion>
```
";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs).unwrap();
        match result {
            ParseResult::Document(doc) => {
                assert_eq!(doc.components.len(), 1, "only known components extracted");
                assert_eq!(doc.components[0].name, "Criterion");
                assert_eq!(
                    doc.components[0].attributes.get("id").map(String::as_str),
                    Some("c1")
                );
            }
            ParseResult::NotSupersigil(_) => panic!("expected Document"),
        }
    }

    #[test]
    fn known_component_inside_unknown_parent_extracted() {
        let content = "\
---
supersigil:
  id: test
---

```supersigil-xml
<Tabs>
<TabItem>
<References refs=\"other/spec#c1\" />
</TabItem>
</Tabs>
```
";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs).unwrap();
        match result {
            ParseResult::Document(doc) => {
                assert_eq!(
                    doc.components.len(),
                    1,
                    "References should be extracted from inside unknown wrappers"
                );
                assert_eq!(doc.components[0].name, "References");
                assert_eq!(
                    doc.components[0].attributes.get("refs").map(String::as_str),
                    Some("other/spec#c1")
                );
            }
            ParseResult::NotSupersigil(_) => panic!("expected Document"),
        }
    }

    #[test]
    fn body_text_captured_from_unknown_wrapper() {
        let content = "\
---
supersigil:
  id: test
---

```supersigil-xml
<Criterion id=\"c1\">
<Aside>important text</Aside>
</Criterion>
```
";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs).unwrap();
        match result {
            ParseResult::Document(doc) => {
                assert_eq!(doc.components.len(), 1);
                let criterion = &doc.components[0];
                assert_eq!(criterion.name, "Criterion");
                let body = criterion.body_text.as_deref().unwrap_or("");
                assert!(
                    body.contains("important text"),
                    "body_text should contain text from unknown <Aside> wrapper, got: {body:?}"
                );
            }
            ParseResult::NotSupersigil(_) => panic!("expected Document"),
        }
    }

    #[test]
    fn mixed_known_unknown_components() {
        let content = "\
---
supersigil:
  id: test
---

```supersigil-xml
<Aside>A note</Aside>
<TrackedFiles paths=\"src/**/*.rs\" />
<Steps>step content</Steps>
<References refs=\"other/spec\" />
```
";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs).unwrap();
        match result {
            ParseResult::Document(doc) => {
                let names: Vec<&str> = doc.components.iter().map(|c| c.name.as_str()).collect();
                assert_eq!(
                    names,
                    vec!["TrackedFiles", "References"],
                    "only known components should be extracted, got: {names:?}"
                );
            }
            ParseResult::NotSupersigil(_) => panic!("expected Document"),
        }
    }

    // ── No supersigil-xml fences produces empty components ──

    #[test]
    fn no_xml_fences_produces_empty_components() {
        let content = "\
---
supersigil:
  id: test
---

Just some markdown text with no supersigil-xml fences.
";
        let f = write_temp_file(content);
        let defs = ComponentDefs::defaults();

        let result = parse_file(f.path(), &defs).unwrap();
        match result {
            ParseResult::Document(doc) => {
                assert!(
                    doc.components.is_empty(),
                    "no supersigil-xml fences means no components"
                );
            }
            ParseResult::NotSupersigil(_) => panic!("expected Document"),
        }
    }
}

// ── parse_content ────────────────────────────────────────────────────────────

mod parse_content {
    use supersigil_core::{ComponentDefs, ParseResult};

    #[test]
    fn parses_valid_content_from_string() {
        let content = "\
---
supersigil:
  id: test-doc
  type: requirements
  status: draft
title: \"Test\"
---

# Hello

```supersigil-xml
<Criterion id=\"c1\">Some text</Criterion>
```
";
        let path = std::path::Path::new("test.md");
        let defs = ComponentDefs::defaults();
        let result = supersigil_parser::parse_content(path, content, &defs);
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        match result.unwrap() {
            ParseResult::Document(doc) => {
                assert_eq!(doc.frontmatter.id, "test-doc");
                assert_eq!(doc.components.len(), 1);
                assert_eq!(doc.components[0].name, "Criterion");
            }
            ParseResult::NotSupersigil(_) => panic!("expected Document"),
        }
    }

    #[test]
    fn returns_not_supersigil_for_non_supersigil() {
        let content = "---\ntitle: \"Not supersigil\"\n---\n\n# Hello\n";
        let path = std::path::Path::new("other.md");
        let defs = ComponentDefs::defaults();
        let result = supersigil_parser::parse_content(path, content, &defs);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ParseResult::NotSupersigil(_)));
    }

    #[test]
    fn reports_validation_errors() {
        let content = "\
---
supersigil:
  id: err-doc
  type: requirements
  status: draft
title: \"Test\"
---

```supersigil-xml
<Criterion />
```
";
        let path = std::path::Path::new("bad.md");
        let defs = ComponentDefs::defaults();
        let result = supersigil_parser::parse_content(path, content, &defs);
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e,
                supersigil_core::ParseError::MissingRequiredAttribute { component, attribute, .. }
                if component == "Criterion" && attribute == "id"
            )),
            "expected MissingRequiredAttribute for Criterion.id, got: {errors:?}"
        );
    }

    #[test]
    fn reports_xml_syntax_errors() {
        let content = "\
---
supersigil:
  id: err-doc
---

```supersigil-xml
<Criterion id=\"c1\">unclosed
```
";
        let path = std::path::Path::new("bad.md");
        let defs = ComponentDefs::defaults();
        let result = supersigil_parser::parse_content(path, content, &defs);
        let errors = result.unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, supersigil_core::ParseError::XmlSyntaxError { .. })),
            "expected XmlSyntaxError, got: {errors:?}"
        );
    }

    #[test]
    fn no_xml_fences_returns_empty_document() {
        let content = "---\nsupersigil:\n  id: empty\n---\n\n# Just Markdown\n";
        let path = std::path::Path::new("empty.md");
        let defs = ComponentDefs::defaults();
        let result = supersigil_parser::parse_content(path, content, &defs);
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        match result.unwrap() {
            ParseResult::Document(doc) => {
                assert_eq!(doc.frontmatter.id, "empty");
                assert!(doc.components.is_empty());
            }
            ParseResult::NotSupersigil(_) => panic!("expected Document"),
        }
    }
}
