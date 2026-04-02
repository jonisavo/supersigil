// Full pipeline integration and content parsing tests

#[allow(unused, reason = "shared test helpers available to submodules")]
mod common;

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
