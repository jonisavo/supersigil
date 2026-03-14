// Property-based tests for supersigil-parser

use proptest::prelude::*;
use supersigil_parser::preprocess;

mod common;
use common::dummy_path;

// ── Property 3: BOM stripping preserves content ─────────────────────────────
// Feature: parser-and-config, Property 3
// For any valid UTF-8 content, preprocessing shall strip a leading BOM if
// present and leave the remaining content unchanged (modulo CRLF normalization).
// Validates: Requirements 1.1, 1.2, 1.3

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn bom_stripping_preserves_content(
        content in "[ -~\n\t]{0,200}",
        prepend_bom in proptest::bool::ANY,
    ) {
        let mut raw = Vec::new();
        if prepend_bom {
            // UTF-8 encoding of U+FEFF
            raw.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        raw.extend_from_slice(content.as_bytes());

        let result = preprocess(&raw, &dummy_path()).unwrap();

        // The result must not start with BOM
        prop_assert!(!result.starts_with('\u{FEFF}'), "BOM should be stripped");

        // Content should be preserved (modulo CRLF normalization)
        let expected = content.replace("\r\n", "\n");
        prop_assert_eq!(result, expected);
    }

    #[test]
    fn non_utf8_always_errors(byte in 0x80u8..=0xFFu8) {
        // A single byte in 0x80..=0xFF is never valid UTF-8 on its own
        let raw = [byte];
        let result = preprocess(&raw, &dummy_path());
        prop_assert!(result.is_err(), "non-UTF-8 byte {byte:#x} should produce an error");
    }
}

// ── Property 4: CRLF normalization replaces all original CRLF pairs ─────────
// Feature: parser-and-config, Property 4
// Every original \r\n pair is replaced with \n. Bare \r (not part of a \r\n
// pair) are preserved as-is. A preserved bare \r may end up adjacent to a \n
// produced by normalizing a subsequent \r\n pair (e.g. \r\r\n → \r\n).
// Validates: Requirements 2.1, 2.2

/// Strategy that generates strings containing arbitrary mixes of \r, \n, \r\n,
/// and printable ASCII.
fn arb_string_with_line_endings() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just("\r\n".to_string()),
            Just("\n".to_string()),
            Just("\r".to_string()),
            "[a-zA-Z0-9 ]{1,5}",
        ],
        0..=30,
    )
    .prop_map(|parts| parts.join(""))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn crlf_normalization_eliminates_all_crlf(
        content in arb_string_with_line_endings(),
    ) {
        let result = preprocess(content.as_bytes(), &dummy_path()).unwrap();
        let (input_crlf_count, input_bare_cr, input_bare_lf) =
            common::count_line_endings(content.as_bytes());

        // Count \r in output — should equal the bare \r count from input
        let output_cr = result.chars().filter(|&c| c == '\r').count();
        prop_assert_eq!(
            output_cr, input_bare_cr,
            "bare \\r count should be preserved"
        );

        // Output length = input length minus removed \r from CRLF pairs
        prop_assert_eq!(
            result.len(),
            content.len() - input_crlf_count,
            "output length should reflect removed \\r from CRLF pairs"
        );

        // \n in output = bare \n from input + normalized CRLF count
        let output_lf = result.chars().filter(|&c| c == '\n').count();
        prop_assert_eq!(
            output_lf,
            input_bare_lf + input_crlf_count,
            "\\n count should equal bare \\n + normalized CRLF count"
        );
    }
}

// ── Property 5: Files without supersigil front matter return NotSupersigil ──
// Feature: parser-and-config, Property 5
// Files without `---` first line, or with YAML but no `supersigil:` key,
// produce NotSupersigil.
// Validates: Requirements 3.3, 4.6, 10.2

/// Strategy that generates content NOT starting with `---` on the first line.
fn arb_no_opening_delimiter() -> impl Strategy<Value = String> {
    // Generate a first line that is not `---` (possibly with trailing ws).
    // We ensure the first line doesn't trim to "---".
    prop_oneof![
        Just("hello\nworld".to_string()),
        "[a-zA-Z0-9 ]{1,20}".prop_map(|s| {
            if s.trim() == "---" {
                format!("x{s}")
            } else {
                s
            }
        }),
        Just(String::new()),
        Just("-- \nstuff".to_string()),
        Just("----\nstuff".to_string()),
    ]
}

/// Strategy that generates valid YAML front matter WITHOUT a `supersigil:` key.
fn arb_yaml_without_supersigil() -> impl Strategy<Value = String> {
    proptest::collection::vec(("[a-z]{1,8}", "[a-zA-Z0-9 ]{1,10}"), 1..=4).prop_map(|pairs| {
        let mut yaml_lines = Vec::new();
        for (k, v) in &pairs {
            // Ensure no key is "supersigil"
            let key = if k == "supersigil" {
                "notsigil".to_string()
            } else {
                k.clone()
            };
            yaml_lines.push(format!("{key}: {v}"));
        }
        let yaml = yaml_lines.join("\n");
        format!("---\n{yaml}\n---\nbody content")
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn no_opening_delimiter_returns_none(
        content in arb_no_opening_delimiter(),
    ) {
        use supersigil_parser::extract_front_matter;
        let result = extract_front_matter(&content, &dummy_path());
        match result {
            Ok(None) | Err(_) => {} // expected: None or error (e.g. unclosed)
            Ok(Some(_)) => prop_assert!(false, "expected None, got Some"),
        }
    }

    #[test]
    fn yaml_without_supersigil_key_returns_not_supersigil(
        content in arb_yaml_without_supersigil(),
    ) {
        use supersigil_parser::{extract_front_matter, deserialize_front_matter, FrontMatterResult};

        let (yaml, _body) = extract_front_matter(&content, &dummy_path())
            .unwrap()
            .expect("should have front matter");

        let result = deserialize_front_matter(yaml, &dummy_path()).unwrap();
        prop_assert!(
            matches!(result, FrontMatterResult::NotSupersigil),
            "expected NotSupersigil for YAML without supersigil key"
        );
    }
}

// ── Property 6: Unclosed front matter produces an error ─────────────────────
// Feature: parser-and-config, Property 6
// Content starting with `---\n` followed by arbitrary content with no closing
// `---` line produces UnclosedFrontMatter error.
// Validates: Requirements 3.2

/// Strategy that generates content with opening `---` but no closing `---` line.
fn arb_unclosed_front_matter() -> impl Strategy<Value = String> {
    // Generate lines that are NOT `---` (after trimming).
    proptest::collection::vec(
        "[a-zA-Z0-9: ]{1,20}".prop_map(|s| {
            if s.trim() == "---" {
                format!("not-a-delimiter: {s}")
            } else {
                s
            }
        }),
        1..=10,
    )
    .prop_map(|lines| {
        let body = lines.join("\n");
        format!("---\n{body}")
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn unclosed_front_matter_produces_error(
        content in arb_unclosed_front_matter(),
    ) {
        use supersigil_parser::extract_front_matter;
        let result = extract_front_matter(&content, &dummy_path());
        prop_assert!(result.is_err(), "expected error for unclosed front matter");
        match result.unwrap_err() {
            supersigil_core::ParseError::UnclosedFrontMatter { .. } => {}
            other => prop_assert!(false, "expected UnclosedFrontMatter, got {other:?}"),
        }
    }
}

// ── Property 7: Extra metadata preservation ─────────────────────────────────
// Feature: parser-and-config, Property 7
// YAML with `supersigil:` key and additional arbitrary keys preserves all
// non-supersigil keys in `extra`.
// Validates: Requirements 4.4

/// Strategy that generates YAML with a `supersigil:` key and extra metadata.
fn arb_yaml_with_extras() -> impl Strategy<Value = (String, Vec<String>)> {
    proptest::collection::vec(
        "[a-z]{2,8}".prop_filter("not supersigil", |k| k != "supersigil"),
        1..=5,
    )
    .prop_map(|keys| {
        // Deduplicate keys
        let mut seen = std::collections::HashSet::new();
        let unique_keys: Vec<String> = keys
            .into_iter()
            .filter(|k| seen.insert(k.clone()))
            .collect();

        let mut yaml_lines = vec!["supersigil:\n  id: test-doc".to_string()];
        for k in &unique_keys {
            yaml_lines.push(format!("{k}: value-{k}"));
        }
        (yaml_lines.join("\n") + "\n", unique_keys)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn extra_metadata_preserved_in_extra_map(
        (yaml, expected_keys) in arb_yaml_with_extras(),
    ) {
        use supersigil_parser::{deserialize_front_matter, FrontMatterResult};

        let result = deserialize_front_matter(&yaml, &dummy_path()).unwrap();
        match result {
            FrontMatterResult::Supersigil { frontmatter, extra } => {
                prop_assert_eq!(frontmatter.id, "test-doc");
                // All extra keys should be present
                for key in &expected_keys {
                    prop_assert!(
                        extra.contains_key(key),
                        "missing extra key: {key}"
                    );
                }
                // No unexpected keys (extra should have exactly the non-supersigil keys)
                prop_assert_eq!(
                    extra.len(),
                    expected_keys.len(),
                    "extra map size mismatch: expected {}, got {}",
                    expected_keys.len(),
                    extra.len()
                );
            }
            FrontMatterResult::NotSupersigil => {
                prop_assert!(false, "expected Supersigil, got NotSupersigil");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 3: Component extraction property tests
// ---------------------------------------------------------------------------

use common::extract;
use supersigil_core::{ComponentDefs, ExtractedComponent, ParseError};
use supersigil_parser::validate_components;

// ---------------------------------------------------------------------------
// Feature: parser-and-config, Property 8: String attribute extraction fidelity
// Validates: Requirements 6.1, 6.4
// ---------------------------------------------------------------------------

/// Strategy that generates valid attribute values (non-empty strings without
/// quotes or angle brackets that would break MDX syntax).
fn arb_attr_value() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_/ ,.-]{1,40}"
}

/// Strategy that generates valid attribute names, excluding `id` which is
/// hardcoded on the Criterion component in the test below.
fn arb_attr_name() -> impl Strategy<Value = String> {
    "[a-z][a-zA-Z0-9_]{0,10}".prop_filter("must not collide with hardcoded id attr", |n| n != "id")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // Property 8: String attribute extraction fidelity
    // For any MDX component with string literal attributes, the parser shall
    // store each attribute value as the exact raw string from the source.
    #[test]
    fn string_attribute_extraction_fidelity(
        name in arb_attr_name(),
        value in arb_attr_value(),
    ) {
        // Use a known component (Criterion) with the `id` attribute to avoid
        // unknown-component errors, but also test with arbitrary attr names
        // on a custom component by using a PascalCase name.
        let mdx = format!("<Criterion {name}=\"{value}\" id=\"c1\" />\n");
        let (components, _errors) = extract(&mdx, 0);

        prop_assert_eq!(components.len(), 1, "expected 1 component");
        let comp = &components[0];

        // The attribute value must be the exact raw string — no transformation.
        prop_assert_eq!(
            comp.attributes.get(&name).map(String::as_str),
            Some(value.as_str()),
            "attribute '{}' should have exact value '{}'",
            name, value
        );
    }
}

// ---------------------------------------------------------------------------
// Feature: parser-and-config, Property 9: Body text is trimmed concatenation
// of non-component text nodes
// Validates: Requirements 8.3, 8.4, 8.5
// ---------------------------------------------------------------------------

/// Strategy that generates non-empty text content (no angle brackets).
fn arb_body_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ]{1,30}"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // Property 9: Body text rules
    // Self-closing → body_text is None
    #[test]
    fn self_closing_has_no_body_text(
        value in arb_attr_value(),
    ) {
        let mdx = format!("<Criterion id=\"{value}\" />\n");
        let (components, errors) = extract(&mdx, 0);

        prop_assert!(errors.is_empty(), "no errors expected: {:?}", errors);
        prop_assert_eq!(components.len(), 1);
        prop_assert!(
            components[0].body_text.is_none(),
            "self-closing component should have body_text = None"
        );
    }

    // Component with text content → body_text is trimmed concatenation
    #[test]
    fn body_text_is_trimmed_concatenation(
        text in arb_body_text(),
    ) {
        let mdx = format!("<Criterion id=\"c1\">\n  {text}  \n</Criterion>\n");
        let (components, errors) = extract(&mdx, 0);

        prop_assert!(errors.is_empty(), "no errors expected: {:?}", errors);
        prop_assert_eq!(components.len(), 1);

        let body = components[0].body_text.as_deref();
        let trimmed_input = text.trim();

        if trimmed_input.is_empty() {
            // Whitespace-only text → body_text should be None
            prop_assert!(
                body.is_none(),
                "whitespace-only text should produce body_text = None"
            );
        } else {
            // Non-empty trimmed text → body_text should be trimmed
            prop_assert!(body.is_some(), "expected body_text for component with text");
            let body = body.unwrap();
            prop_assert_eq!(body, body.trim(), "body_text should be trimmed");
            prop_assert!(
                body.contains(trimmed_input),
                "body_text '{}' should contain trimmed input '{}'",
                body, trimmed_input
            );
        }
    }

    // Component with only child components → body_text is None
    #[test]
    fn only_children_has_no_body_text(
        id1 in "[a-z]{2,6}",
        id2 in "[a-z]{2,6}",
    ) {
        let mdx = format!(
            "<AcceptanceCriteria>\n\n<Criterion id=\"{id1}\">\ntext\n</Criterion>\n\n<Criterion id=\"{id2}\">\ntext\n</Criterion>\n\n</AcceptanceCriteria>\n"
        );
        let (components, errors) = extract(&mdx, 0);

        prop_assert!(errors.is_empty(), "no errors expected: {:?}", errors);
        prop_assert_eq!(components.len(), 1);
        prop_assert!(
            components[0].body_text.is_none(),
            "component with only child components should have body_text = None, got: {:?}",
            components[0].body_text
        );
    }
}

// ---------------------------------------------------------------------------
// Feature: parser-and-config, Property 10: Recursive child collection
// preserves nesting structure
// Validates: Requirements 9.1, 9.2, 9.3
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // Property 10: Recursive child collection preserves nesting structure
    // Generate 1-5 Criterion children inside AcceptanceCriteria, verify tree.
    #[test]
    fn recursive_child_collection_preserves_nesting(
        ids in proptest::collection::vec("[a-z]{2,6}", 1..=5),
    ) {
        // Deduplicate IDs
        let mut seen = std::collections::HashSet::new();
        let unique_ids: Vec<&str> = ids.iter()
            .map(String::as_str)
            .filter(|id| seen.insert(*id))
            .collect();

        let n = unique_ids.len();

        let mut mdx = String::from("<AcceptanceCriteria>\n\n");
        for id in &unique_ids {
            use std::fmt::Write;
            write!(mdx, "<Criterion id=\"{id}\">\ntext for {id}\n</Criterion>\n\n").unwrap();
        }
        mdx.push_str("</AcceptanceCriteria>\n");

        let (components, errors) = extract(&mdx, 0);

        prop_assert!(errors.is_empty(), "no errors expected: {:?}", errors);
        prop_assert_eq!(components.len(), 1, "should have 1 top-level component");

        let parent = &components[0];
        prop_assert_eq!(parent.name.as_str(), "AcceptanceCriteria");
        prop_assert_eq!(
            parent.children.len(), n,
            "parent should have {} children, got {}",
            n, parent.children.len()
        );

        // Each child should be a Criterion with the correct id
        for (i, id) in unique_ids.iter().enumerate() {
            let child = &parent.children[i];
            prop_assert_eq!(child.name.as_str(), "Criterion");
            prop_assert_eq!(
                child.attributes.get("id").map(String::as_str),
                Some(*id),
                "child {} should have id '{}'",
                i, id
            );
            // Leaf children should have empty children list
            prop_assert!(
                child.children.is_empty(),
                "leaf Criterion should have no children"
            );
        }
    }

    // Components with no children have empty children list
    #[test]
    fn leaf_component_has_empty_children(
        id in "[a-z]{2,8}",
    ) {
        let mdx = format!("<Criterion id=\"{id}\">\nsome text\n</Criterion>\n");
        let (components, errors) = extract(&mdx, 0);

        prop_assert!(errors.is_empty(), "no errors expected: {:?}", errors);
        prop_assert_eq!(components.len(), 1);
        prop_assert!(
            components[0].children.is_empty(),
            "leaf component should have empty children list"
        );
    }
}

// ---------------------------------------------------------------------------
// Feature: parser-and-config, Property 16: Missing required attributes are
// detected
// Validates: Requirements 21.1, 21.2
// ---------------------------------------------------------------------------

use supersigil_core::AttributeDef;

/// Strategy that generates a `PascalCase` component name.
fn arb_pascal_name() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{2,8}"
}

/// Strategy that generates a valid attribute name.
fn arb_attr_name_simple() -> impl Strategy<Value = String> {
    "[a-z][a-z_]{1,8}"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // Property 16: Missing required attributes are detected.
    // Generate component defs with required attributes, generate component
    // instances missing some, verify errors. Also verify no false positives
    // when all present.
    #[test]
    fn missing_required_attributes_detected(
        comp_name in arb_pascal_name(),
        required_attrs in proptest::collection::vec(arb_attr_name_simple(), 1..=4),
        // For each required attr, whether the component instance has it
        present_mask in proptest::collection::vec(proptest::bool::ANY, 1..=4),
    ) {
        use std::collections::HashMap;
        use supersigil_core::{ComponentDef, ComponentDefs, SourcePosition};

        // Deduplicate attribute names
        let mut seen = std::collections::HashSet::new();
        let unique_attrs: Vec<&str> = required_attrs.iter()
            .map(String::as_str)
            .filter(|a| seen.insert(*a))
            .collect();

        if unique_attrs.is_empty() {
            return Ok(());
        }

        // Build component def with all attributes required
        let mut attr_defs = HashMap::new();
        for attr in &unique_attrs {
            attr_defs.insert(
                attr.to_string(),
                AttributeDef { required: true, list: false },
            );
        }
        let user_defs = HashMap::from([(
            comp_name.clone(),
            ComponentDef {
                attributes: attr_defs,
                referenceable: false,
                verifiable: false,
                target_component: None,
                description: None,
                examples: Vec::new(),
            },
        )]);
        let defs = ComponentDefs::merge(ComponentDefs::defaults(), user_defs).unwrap();

        // Build component instance with some attributes present based on mask
        let mut instance_attrs = HashMap::new();
        let mut expected_missing = Vec::new();
        for (i, attr) in unique_attrs.iter().enumerate() {
            let is_present = present_mask.get(i).copied().unwrap_or(false);
            if is_present {
                instance_attrs.insert(attr.to_string(), "value".to_string());
            } else {
                expected_missing.push(*attr);
            }
        }

        let component = ExtractedComponent {
            name: comp_name.clone(),
            attributes: instance_attrs,
            children: Vec::new(),
            body_text: None,
            code_blocks: Vec::new(),
            position: SourcePosition { byte_offset: 0, line: 1, column: 1 },
        };

        let mut errors = Vec::new();
        validate_components(&[component], &defs, &dummy_path(), &mut errors);

        // Count MissingRequiredAttribute errors for our component
        let missing_errors: Vec<_> = errors.iter().filter(|e| {
            matches!(e, ParseError::MissingRequiredAttribute { component, .. } if component == &comp_name)
        }).collect();

        // Should have exactly one error per missing required attribute
        prop_assert_eq!(
            missing_errors.len(),
            expected_missing.len(),
            "expected {} missing attr errors, got {}: {:?}",
            expected_missing.len(), missing_errors.len(), missing_errors
        );

        // No false positives: no errors for attributes that are present
        for error in &errors {
            if let ParseError::MissingRequiredAttribute { attribute, .. } = error {
                prop_assert!(
                    expected_missing.contains(&attribute.as_str()),
                    "false positive: error for attribute '{}' which should be present",
                    attribute
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Feature: parser-and-config, Property 17: Unknown component names produce
// no errors (they are skipped during extraction)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // Property 17: Unknown PascalCase component names produce no errors.
    // They are filtered out during extraction and validation ignores them.
    #[test]
    fn unknown_pascal_case_names_produce_no_errors(
        name in "[A-Z][a-z]{3,10}",
    ) {
        use supersigil_core::SourcePosition;

        let defs = ComponentDefs::defaults();

        // Skip if the generated name happens to be a built-in
        if defs.is_known(&name) {
            return Ok(());
        }

        let component = ExtractedComponent {
            name: name.clone(),
            attributes: std::collections::HashMap::new(),
            children: Vec::new(),
            body_text: None,
            code_blocks: Vec::new(),
            position: SourcePosition { byte_offset: 0, line: 1, column: 1 },
        };

        let mut errors = Vec::new();
        validate_components(&[component], &defs, &dummy_path(), &mut errors);

        prop_assert!(
            errors.is_empty(),
            "unknown component '{}' should produce no errors, got: {:?}",
            name, errors
        );
    }

    // Lowercase names never produce errors
    #[test]
    fn lowercase_names_never_produce_errors(
        name in "[a-z][a-z]{1,10}",
    ) {
        use supersigil_core::SourcePosition;

        let defs = ComponentDefs::defaults();

        let component = ExtractedComponent {
            name: name.clone(),
            attributes: std::collections::HashMap::new(),
            children: Vec::new(),
            body_text: None,
            code_blocks: Vec::new(),
            position: SourcePosition { byte_offset: 0, line: 1, column: 1 },
        };

        let mut errors = Vec::new();
        validate_components(&[component], &defs, &dummy_path(), &mut errors);

        prop_assert!(
            errors.is_empty(),
            "lowercase name '{}' should not produce errors, got: {:?}",
            name, errors
        );
    }
}

// ---------------------------------------------------------------------------
// Feature: parser-and-config, Property 18: Parser collects all errors rather
// than stopping at the first
// Validates: Requirements 10.3
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // Property 18: Files with multiple independent error conditions return all
    // errors, not just the first.
    // We generate M components missing required attrs and verify we get M errors.
    #[test]
    fn parser_collects_all_errors(
        missing_attr_count in 1u32..=5u32,
    ) {
        use supersigil_core::SourcePosition;

        let defs = ComponentDefs::defaults();

        let mut components = Vec::new();

        // Add known components missing required attrs (Criterion missing `id`)
        for i in 0..missing_attr_count as usize {
            components.push(ExtractedComponent {
                name: "Criterion".to_string(),
                attributes: std::collections::HashMap::new(),
                children: Vec::new(),
                body_text: None,
                code_blocks: Vec::new(),
                position: SourcePosition {
                    byte_offset: i * 100,
                    line: i + 1,
                    column: 1,
                },
            });
        }

        let expected_missing = missing_attr_count as usize;

        let mut errors = Vec::new();
        validate_components(&components, &defs, &dummy_path(), &mut errors);

        let actual_missing = errors.iter()
            .filter(|e| matches!(e, ParseError::MissingRequiredAttribute { .. }))
            .count();

        prop_assert_eq!(
            actual_missing, expected_missing,
            "expected {} MissingRequiredAttribute errors, got {}",
            expected_missing, actual_missing
        );
        prop_assert_eq!(
            errors.len(), expected_missing,
            "expected {} total errors, got {}",
            expected_missing, errors.len()
        );
    }
}
