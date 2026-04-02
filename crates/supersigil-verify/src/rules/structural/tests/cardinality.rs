use super::*;

// -----------------------------------------------------------------------
// check_code_block_cardinality
// -----------------------------------------------------------------------

#[test]
fn example_with_exactly_one_code_block_is_valid() {
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![make_code_block()],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_code_block_cardinality(&doc_refs);
    assert!(
        findings.is_empty(),
        "Example with 1 code block should be valid, got: {findings:?}"
    );
}

#[test]
fn example_with_zero_code_blocks_emits_finding() {
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_code_block_cardinality(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidCodeBlockCardinality);
    assert!(
        findings[0].message.contains("exactly 1"),
        "got: {}",
        findings[0].message
    );
}

#[test]
fn example_with_two_code_blocks_emits_finding() {
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![make_code_block(), make_code_block()],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_code_block_cardinality(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidCodeBlockCardinality);
}

#[test]
fn expected_with_zero_code_blocks_is_valid() {
    let expected = supersigil_core::ExtractedComponent {
        name: EXPECTED.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![expected])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_code_block_cardinality(&doc_refs);
    assert!(
        findings.is_empty(),
        "Expected with 0 code blocks should be valid, got: {findings:?}"
    );
}

#[test]
fn expected_with_one_code_block_is_valid() {
    let expected = supersigil_core::ExtractedComponent {
        name: EXPECTED.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![make_code_block()],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![expected])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_code_block_cardinality(&doc_refs);
    assert!(
        findings.is_empty(),
        "Expected with 1 code block should be valid, got: {findings:?}"
    );
}

#[test]
fn expected_with_two_code_blocks_emits_finding() {
    let expected = supersigil_core::ExtractedComponent {
        name: EXPECTED.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![make_code_block(), make_code_block()],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![expected])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_code_block_cardinality(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidCodeBlockCardinality);
    assert!(
        findings[0].message.contains("at most 1"),
        "got: {}",
        findings[0].message
    );
}

// -----------------------------------------------------------------------
// check_env_format
// -----------------------------------------------------------------------

#[test]
fn example_with_valid_env_is_clean() {
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::from([("env".into(), "FOO=bar,BAZ=qux".into())]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![make_code_block()],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_env_format(&doc_refs);
    assert!(
        findings.is_empty(),
        "valid env items should not emit findings, got: {findings:?}"
    );
}

#[test]
fn example_with_env_item_missing_equals_emits_finding() {
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::from([("env".into(), "FOO=bar,BADITEM".into())]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![make_code_block()],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_env_format(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidEnvFormat);
    assert!(
        findings[0].message.contains("BADITEM"),
        "got: {}",
        findings[0].message
    );
}

#[test]
fn expected_with_env_item_missing_equals_emits_finding() {
    let expected = supersigil_core::ExtractedComponent {
        name: EXPECTED.to_owned(),
        attributes: std::collections::HashMap::from([("env".into(), "NOEQUALS".into())]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![expected])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_env_format(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidEnvFormat);
}

#[test]
fn component_without_env_attribute_is_clean() {
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![make_code_block()],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_env_format(&doc_refs);
    assert!(
        findings.is_empty(),
        "no env attribute should not emit findings, got: {findings:?}"
    );
}

#[test]
fn multiple_invalid_env_items_emit_multiple_findings() {
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::from([(
            "env".into(),
            "NOEQ1,NOEQ2,VALID=ok".into(),
        )]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![make_code_block()],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_env_format(&doc_refs);
    assert_eq!(findings.len(), 2);
}

// -----------------------------------------------------------------------
// check_expected_cardinality
// -----------------------------------------------------------------------

#[test]
fn example_with_zero_expected_children_no_finding() {
    let example = make_example(vec![], 10);
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_expected_cardinality(&doc_refs);
    assert!(
        findings.is_empty(),
        "Example with 0 Expected children should produce no findings, got: {findings:?}",
    );
}

#[test]
fn example_with_one_expected_child_no_finding() {
    let example = make_example(vec![make_expected(11)], 10);
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_expected_cardinality(&doc_refs);
    assert!(
        findings.is_empty(),
        "Example with 1 Expected child should produce no findings, got: {findings:?}",
    );
}

#[test]
fn example_with_two_expected_children_emits_finding() {
    let example = make_example(vec![make_expected(11), make_expected(12)], 10);
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_expected_cardinality(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::MultipleExpectedChildren);
    assert!(
        findings[0].message.contains("2 Expected children"),
        "message should mention count, got: {}",
        findings[0].message,
    );
}
