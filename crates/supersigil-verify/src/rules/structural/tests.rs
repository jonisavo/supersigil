use super::*;
use crate::test_helpers::*;
use supersigil_core::{ACCEPTANCE_CRITERIA, DocumentTypeDef};
use supersigil_rust::verifies;
use tempfile::TempDir;

// -----------------------------------------------------------------------
// check_required_components
// -----------------------------------------------------------------------

#[test]
fn document_missing_required_component_emits_finding() {
    let mut config = test_config();
    config.documents.types.insert(
        "requirements".into(),
        DocumentTypeDef {
            status: vec!["draft".into()],
            required_components: vec![ACCEPTANCE_CRITERIA.to_owned()],
            description: None,
        },
    );
    let docs = vec![make_doc_typed(
        "req/auth",
        "requirements",
        Some("draft"),
        vec![],
    )];
    let graph = build_test_graph_with_config(docs, &config);
    let findings = check_required_components(&graph, &config);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::MissingRequiredComponent);
}

#[test]
fn document_with_required_component_is_clean() {
    let mut config = test_config();
    config.documents.types.insert(
        "requirements".into(),
        DocumentTypeDef {
            status: vec!["draft".into()],
            required_components: vec![ACCEPTANCE_CRITERIA.to_owned()],
            description: None,
        },
    );
    let docs = vec![make_doc_typed(
        "req/auth",
        "requirements",
        Some("draft"),
        vec![make_acceptance_criteria(
            vec![make_criterion("req-1", 10)],
            9,
        )],
    )];
    let graph = build_test_graph_with_config(docs, &config);
    let findings = check_required_components(&graph, &config);
    assert!(findings.is_empty());
}

// -----------------------------------------------------------------------
// check_id_pattern
// -----------------------------------------------------------------------

#[test]
fn id_not_matching_pattern_emits_finding() {
    let mut config = test_config();
    config.id_pattern = Some(r"^(req|design|tasks)/".into());
    let docs = vec![make_doc("bad-id", vec![])];
    let graph = build_test_graph_with_config(docs, &config);
    let findings = check_id_pattern(&graph, &config);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidIdPattern);
}

#[test]
fn id_matching_pattern_is_clean() {
    let mut config = test_config();
    config.id_pattern = Some(r"^(req|design|tasks)/".into());
    let docs = vec![make_doc("req/auth", vec![])];
    let graph = build_test_graph_with_config(docs, &config);
    let findings = check_id_pattern(&graph, &config);
    assert!(findings.is_empty());
}

#[test]
fn no_id_pattern_means_no_findings() {
    let config = test_config();
    let docs = vec![make_doc("anything", vec![])];
    let graph = build_test_graph_with_config(docs, &config);
    let findings = check_id_pattern(&graph, &config);
    assert!(findings.is_empty());
}

// -----------------------------------------------------------------------
// check_isolated
// -----------------------------------------------------------------------

#[test]
fn document_with_no_refs_emits_isolated() {
    let docs = vec![
        make_doc("lonely", vec![]),
        make_doc("connected-a", vec![make_implements("connected-b", 5)]),
        make_doc("connected-b", vec![]),
    ];
    let graph = build_test_graph(docs);
    let findings = check_isolated(&graph);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].message.contains("lonely"));
}

#[test]
fn depends_on_target_is_not_isolated() {
    // If A DependsOn B, then B has an incoming ref and should NOT be isolated.
    let docs = vec![
        make_doc("a", vec![make_depends_on("b", 5)]),
        make_doc("b", vec![]), // no outgoing refs, but has incoming DependsOn
    ];
    let graph = build_test_graph(docs);
    let findings = check_isolated(&graph);
    // Neither document should be isolated: A has outgoing, B has incoming DependsOn
    assert!(
        findings.is_empty(),
        "document 'b' should not be isolated (it is a DependsOn target), got: {findings:?}",
    );
}

#[test]
fn document_with_outgoing_ref_is_not_isolated() {
    let docs = vec![
        make_doc("connected", vec![make_implements("other", 5)]),
        make_doc("other", vec![]),
    ];
    let graph = build_test_graph(docs);
    let findings = check_isolated(&graph);
    // "other" has incoming ref from "connected", so neither is isolated
    assert!(findings.is_empty());
}

#[test]
fn tasks_doc_with_task_level_implements_is_not_isolated() {
    let mut task = make_task("task-1", 10);
    task.attributes
        .insert("implements".into(), "req/auth#req-1".into());

    let docs = vec![
        make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 5)],
                4,
            )],
        ),
        make_doc("tasks/auth", vec![task]),
    ];
    let graph = build_test_graph(docs);
    let findings = check_isolated(&graph);

    assert!(
        findings
            .iter()
            .all(|finding| finding.doc_id.as_deref() != Some("tasks/auth")),
        "tasks doc with task-level implements should not be isolated, got: {findings:?}",
    );
}

#[test]
fn task_implements_target_is_not_isolated() {
    let mut task = make_task("task-1", 10);
    task.attributes
        .insert("implements".into(), "req/auth#req-1".into());

    let docs = vec![
        make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 5)],
                4,
            )],
        ),
        make_doc("tasks/auth", vec![task]),
    ];
    let graph = build_test_graph(docs);
    let findings = check_isolated(&graph);

    assert!(
        findings
            .iter()
            .all(|finding| finding.doc_id.as_deref() != Some("req/auth")),
        "task implements target should not be isolated, got: {findings:?}",
    );
}

// -----------------------------------------------------------------------
// check_orphan_tags
// -----------------------------------------------------------------------

#[test]
fn tag_in_file_not_in_any_verified_by_emits_orphan() {
    let dir = TempDir::new().unwrap();
    write_test_file(&dir, "test.rs", "// supersigil: prop:orphaned-tag\n");
    let docs = [make_doc(
        "prop/auth",
        vec![make_verified_by_tag("prop:real-tag", 5)],
    )];
    let test_files = vec![dir.path().join("test.rs")];
    let tag_matches = crate::scan::scan_all_tags(&test_files);
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_orphan_tags(&doc_refs, &tag_matches);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::OrphanTestTag);
    assert!(findings[0].message.contains("prop:orphaned-tag"));
}

#[test]
fn declared_tag_is_not_orphaned() {
    let dir = TempDir::new().unwrap();
    write_test_file(&dir, "test.rs", "// supersigil: prop:real-tag\n");
    let docs = [make_doc(
        "prop/auth",
        vec![make_verified_by_tag("prop:real-tag", 5)],
    )];
    let test_files = vec![dir.path().join("test.rs")];
    let tag_matches = crate::scan::scan_all_tags(&test_files);
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_orphan_tags(&doc_refs, &tag_matches);
    assert!(findings.is_empty());
}

// -----------------------------------------------------------------------
// check_verified_by_placement
// -----------------------------------------------------------------------

#[test]
fn verified_by_under_criterion_is_valid() {
    let component_defs = supersigil_core::ComponentDefs::defaults();
    let docs = [make_doc(
        "req/auth",
        vec![make_acceptance_criteria(
            vec![make_criterion_with_verified_by(
                "req-1",
                make_verified_by_tag("auth:login", 11),
                10,
            )],
            9,
        )],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_verified_by_placement(&doc_refs, &component_defs);
    assert!(
        findings.is_empty(),
        "VerifiedBy under Criterion should produce no structural errors, got: {findings:?}",
    );
}

#[test]
fn verified_by_at_document_root_is_structural_error() {
    let component_defs = supersigil_core::ComponentDefs::defaults();
    let docs = [make_doc(
        "req/auth",
        vec![
            make_references("other/doc", 5),
            make_verified_by_tag("auth:login", 6),
        ],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_verified_by_placement(&doc_refs, &component_defs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidVerifiedByPlacement);
    assert!(
        findings[0].message.contains("verifiable"),
        "error message should mention 'verifiable', got: {}",
        findings[0].message,
    );
}

#[test]
fn verified_by_under_non_verifiable_component_is_structural_error() {
    let component_defs = supersigil_core::ComponentDefs::defaults();
    // AcceptanceCriteria is not verifiable, so VerifiedBy directly under it is invalid
    let docs = [make_doc(
        "req/auth",
        vec![make_acceptance_criteria(
            vec![make_verified_by_tag("auth:login", 11)],
            9,
        )],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_verified_by_placement(&doc_refs, &component_defs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidVerifiedByPlacement);
}

#[test]
fn nested_verified_by_under_verifiable_component_still_produces_evidence() {
    // This test verifies that evidence extraction (via explicit_evidence) still
    // works for VerifiedBy under Criterion. We check that the structural rule
    // does NOT flag it, which is the structural side of "still produces evidence".
    let component_defs = supersigil_core::ComponentDefs::defaults();

    let docs = [make_doc(
        "req/auth",
        vec![make_acceptance_criteria(
            vec![make_criterion_with_verified_by(
                "req-1",
                make_verified_by_glob("tests/**/*.rs", 11),
                10,
            )],
            9,
        )],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_verified_by_placement(&doc_refs, &component_defs);
    assert!(
        findings.is_empty(),
        "VerifiedBy under Criterion should not produce structural errors, got: {findings:?}",
    );
}

#[test]
fn multiple_verified_by_children_under_one_verifiable_component_are_additive() {
    // Multiple VerifiedBy under one Criterion should all be accepted
    let component_defs = supersigil_core::ComponentDefs::defaults();

    let criterion = supersigil_core::ExtractedComponent {
        name: CRITERION.to_owned(),
        attributes: std::collections::HashMap::from([("id".into(), "req-1".into())]),
        children: vec![
            make_verified_by_tag("auth:tag1", 11),
            make_verified_by_glob("tests/**/*.rs", 12),
            make_verified_by_tag("auth:tag2", 13),
        ],
        body_text: Some("criterion req-1".into()),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(10),
        end_position: pos(10),
    };
    let docs = [make_doc(
        "req/auth",
        vec![make_acceptance_criteria(vec![criterion], 9)],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_verified_by_placement(&doc_refs, &component_defs);
    assert!(
        findings.is_empty(),
        "multiple VerifiedBy under one Criterion should all be valid, got: {findings:?}",
    );
}

// -----------------------------------------------------------------------
// parse_sequential_id
// -----------------------------------------------------------------------

#[test]
fn parse_single_level_id() {
    let parsed = parse_sequential_id("task-3").unwrap();
    assert_eq!(parsed.prefix, "task");
    assert_eq!(parsed.key, NumericKey::One(3));
}

#[test]
fn parse_two_level_id() {
    let parsed = parse_sequential_id("req-1-2").unwrap();
    assert_eq!(parsed.prefix, "req");
    assert_eq!(parsed.key, NumericKey::Two(1, 2));
}

#[test]
fn parse_multi_segment_prefix() {
    let parsed = parse_sequential_id("my-prefix-1-2").unwrap();
    assert_eq!(parsed.prefix, "my-prefix");
    assert_eq!(parsed.key, NumericKey::Two(1, 2));
}

#[test]
fn non_sequential_semantic_id() {
    assert!(parse_sequential_id("login-success").is_none());
}

#[test]
fn non_sequential_suffix_id() {
    assert!(parse_sequential_id("req-1-2-foo").is_none());
}

#[test]
fn non_sequential_three_numeric_segments() {
    assert!(parse_sequential_id("req-1-2-3").is_none());
}

#[test]
fn non_sequential_no_prefix() {
    assert!(parse_sequential_id("123").is_none());
}

#[test]
fn non_sequential_single_segment() {
    assert!(parse_sequential_id("foo").is_none());
}

#[test]
fn non_sequential_empty_string() {
    assert!(parse_sequential_id("").is_none());
}

#[test]
fn numeric_key_ordering() {
    assert!(NumericKey::One(1) < NumericKey::One(2));
    assert!(NumericKey::Two(1, 1) < NumericKey::Two(1, 2));
    assert!(NumericKey::Two(1, 2) < NumericKey::Two(2, 1));
}

// -----------------------------------------------------------------------
// check_sequential_id_order
// -----------------------------------------------------------------------

#[test]
fn order_correct_criteria_no_findings() {
    let docs = [make_doc(
        "feature/req",
        vec![make_acceptance_criteria(
            vec![
                make_criterion("req-1-1", 10),
                make_criterion("req-1-2", 20),
                make_criterion("req-2-1", 30),
            ],
            9,
        )],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_order(&doc_refs);
    assert!(
        findings.is_empty(),
        "correctly ordered IDs should produce no findings, got: {findings:?}"
    );
}

#[test]
fn order_swapped_pair_emits_finding() {
    let docs = [make_doc(
        "feature/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("req-1-2", 10), make_criterion("req-1-1", 20)],
            9,
        )],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_order(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::SequentialIdOrder);
    assert!(
        findings[0].message.contains("req-1-1"),
        "finding should name the out-of-order ID, got: {}",
        findings[0].message
    );
    assert!(
        findings[0].message.contains("req-1-2"),
        "finding should name the predecessor, got: {}",
        findings[0].message
    );
}

#[test]
fn order_multiple_prefix_groups_independent() {
    // req-* in order, task-* out of order
    let docs = [make_doc(
        "feature/tasks",
        vec![
            make_criterion("req-1", 10),
            make_criterion("req-2", 20),
            make_task("task-2", 30),
            make_task("task-1", 40),
        ],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_order(&doc_refs);
    assert_eq!(
        findings.len(),
        1,
        "only task group should have findings, got: {findings:?}"
    );
    assert!(findings[0].message.contains("task-1"));
}

#[test]
fn order_non_sequential_ids_skipped() {
    let docs = [make_doc(
        "feature/req",
        vec![
            make_criterion("login-success", 10),
            make_criterion("login-failure", 20),
        ],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_order(&doc_refs);
    assert!(
        findings.is_empty(),
        "non-sequential IDs should be skipped, got: {findings:?}"
    );
}

#[test]
fn order_mixed_sequential_and_non_sequential() {
    let docs = [make_doc(
        "feature/req",
        vec![make_acceptance_criteria(
            vec![
                make_criterion("req-1", 10),
                make_criterion("login-check", 15),
                make_criterion("req-2", 20),
            ],
            9,
        )],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_order(&doc_refs);
    assert!(
        findings.is_empty(),
        "non-sequential IDs should not interfere, got: {findings:?}"
    );
}

#[test]
fn order_mixed_arity_no_false_positive() {
    // task-1 (One), task-1-1 (Two), task-2 (One) should not flag task-2
    let docs = [make_doc(
        "feature/tasks",
        vec![
            make_task("task-1", 10),
            make_task("task-1-1", 15),
            make_task("task-2", 20),
            make_task("task-4-1", 25),
            make_task("task-4-2", 30),
            make_task("task-5", 35),
        ],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_order(&doc_refs);
    assert!(
        findings.is_empty(),
        "mixed arity should not cause false positives, got: {findings:?}"
    );
}

#[test]
fn order_tasks_correct_no_findings() {
    let docs = [make_doc(
        "feature/tasks",
        vec![
            make_task("task-1", 10),
            make_task("task-2", 20),
            make_task("task-3", 30),
        ],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_order(&doc_refs);
    assert!(findings.is_empty());
}

// -----------------------------------------------------------------------
// check_sequential_id_gap
// -----------------------------------------------------------------------

#[test]
fn gap_contiguous_sequence_no_findings() {
    let docs = [make_doc(
        "feature/tasks",
        vec![
            make_task("task-1", 10),
            make_task("task-2", 20),
            make_task("task-3", 30),
        ],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_gap(&doc_refs);
    assert!(
        findings.is_empty(),
        "contiguous sequence should produce no findings, got: {findings:?}"
    );
}

#[test]
fn gap_missing_middle_element() {
    let docs = [make_doc(
        "feature/tasks",
        vec![make_task("task-1", 10), make_task("task-3", 30)],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_gap(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::SequentialIdGap);
    assert!(
        findings[0].message.contains("task-2"),
        "should name the missing ID, got: {}",
        findings[0].message
    );
    assert!(
        findings[0].message.contains("task-1"),
        "should reference predecessor, got: {}",
        findings[0].message
    );
    assert!(
        findings[0].message.contains("task-3"),
        "should reference successor, got: {}",
        findings[0].message
    );
}

#[test]
fn gap_missing_first_element_leading_gap() {
    let docs = [make_doc(
        "feature/tasks",
        vec![make_task("task-2", 10), make_task("task-3", 20)],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_gap(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert!(
        findings[0].message.contains("task-1"),
        "should name the missing ID, got: {}",
        findings[0].message
    );
    assert!(
        findings[0].message.contains("starts at"),
        "leading gap should say 'starts at', got: {}",
        findings[0].message
    );
    assert!(
        findings[0].message.contains("task-2"),
        "should reference the first present ID, got: {}",
        findings[0].message
    );
}

#[test]
fn gap_two_level_m_contiguity() {
    let docs = [make_doc(
        "feature/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("req-1-1", 10), make_criterion("req-1-3", 30)],
            9,
        )],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_gap(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert!(
        findings[0].message.contains("req-1-2"),
        "should name the missing M-level ID, got: {}",
        findings[0].message
    );
}

#[test]
fn gap_two_level_n_contiguity() {
    // Has req-1-1 and req-3-1, missing the entire req-2 group
    let docs = [make_doc(
        "feature/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("req-1-1", 10), make_criterion("req-3-1", 30)],
            9,
        )],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_gap(&doc_refs);
    assert!(
        findings.iter().any(|f| f.message.contains("req-2-*")),
        "should detect missing N-level group, got: {findings:?}"
    );
}

#[test]
fn gap_non_sequential_ids_skipped() {
    let docs = [make_doc(
        "feature/req",
        vec![
            make_criterion("login-success", 10),
            make_criterion("login-failure", 20),
        ],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_gap(&doc_refs);
    assert!(
        findings.is_empty(),
        "non-sequential IDs should be skipped, got: {findings:?}"
    );
}

#[test]
fn gap_two_level_contiguous_no_findings() {
    let docs = [make_doc(
        "feature/req",
        vec![make_acceptance_criteria(
            vec![
                make_criterion("req-1-1", 10),
                make_criterion("req-1-2", 20),
                make_criterion("req-2-1", 30),
                make_criterion("req-2-2", 40),
            ],
            9,
        )],
    )];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_sequential_id_gap(&doc_refs);
    assert!(
        findings.is_empty(),
        "contiguous two-level sequence should produce no findings, got: {findings:?}"
    );
}

// -----------------------------------------------------------------------
// check_rationale_placement
// -----------------------------------------------------------------------

#[test]
fn rationale_inside_decision_is_valid() {
    let decision = make_decision(vec![make_rationale(11)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_rationale_placement(&doc_refs);
    assert!(
        findings.is_empty(),
        "Rationale inside Decision should be valid, got: {findings:?}",
    );
}

#[verifies("decision-components/req#req-2-2")]
#[test]
fn rationale_at_document_root_emits_finding() {
    let docs = [make_doc("adr/logging", vec![make_rationale(5)])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_rationale_placement(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidRationalePlacement);
    assert!(
        findings[0].message.contains("document root"),
        "message should mention document root, got: {}",
        findings[0].message,
    );
}

#[test]
fn rationale_inside_non_decision_component_emits_finding() {
    // Rationale nested inside Criterion (not Decision)
    let criterion = supersigil_core::ExtractedComponent {
        name: CRITERION.to_owned(),
        attributes: std::collections::HashMap::from([("id".into(), "req-1".into())]),
        children: vec![make_rationale(11)],
        body_text: Some("criterion req-1".into()),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(10),
        end_position: pos(10),
    };
    let docs = [make_doc("adr/logging", vec![criterion])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_rationale_placement(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidRationalePlacement);
    assert!(
        findings[0].message.contains("Criterion"),
        "message should mention parent name, got: {}",
        findings[0].message,
    );
}

// -----------------------------------------------------------------------
// check_alternative_placement
// -----------------------------------------------------------------------

#[test]
fn alternative_inside_decision_is_valid() {
    let decision = make_decision(vec![make_alternative("alt-1", 11)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_placement(&doc_refs);
    assert!(
        findings.is_empty(),
        "Alternative inside Decision should be valid, got: {findings:?}",
    );
}

#[verifies("decision-components/req#req-3-4")]
#[test]
fn alternative_at_document_root_emits_finding() {
    let docs = [make_doc("adr/logging", vec![make_alternative("alt-1", 5)])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_placement(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidAlternativePlacement);
    assert!(
        findings[0].message.contains("document root"),
        "message should mention document root, got: {}",
        findings[0].message,
    );
}

#[test]
fn alternative_inside_non_decision_component_emits_finding() {
    // Alternative nested inside Criterion (not Decision)
    let criterion = supersigil_core::ExtractedComponent {
        name: CRITERION.to_owned(),
        attributes: std::collections::HashMap::from([("id".into(), "req-1".into())]),
        children: vec![make_alternative("alt-1", 11)],
        body_text: Some("criterion req-1".into()),
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(10),
        end_position: pos(10),
    };
    let docs = [make_doc("adr/logging", vec![criterion])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_placement(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidAlternativePlacement);
    assert!(
        findings[0].message.contains("Criterion"),
        "message should mention parent name, got: {}",
        findings[0].message,
    );
}

// -----------------------------------------------------------------------
// check_duplicate_rationale
// -----------------------------------------------------------------------

#[test]
fn decision_with_zero_rationale_no_finding() {
    let decision = make_decision(vec![], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_duplicate_rationale(&doc_refs);
    assert!(
        findings.is_empty(),
        "Decision with zero Rationale children should produce no findings, got: {findings:?}",
    );
}

#[test]
fn decision_with_one_rationale_no_finding() {
    let decision = make_decision(vec![make_rationale(11)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_duplicate_rationale(&doc_refs);
    assert!(
        findings.is_empty(),
        "Decision with one Rationale child should produce no findings, got: {findings:?}",
    );
}

#[verifies("decision-components/req#req-2-3")]
#[test]
fn decision_with_two_rationale_emits_finding_on_second() {
    let decision = make_decision(vec![make_rationale(11), make_rationale(12)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_duplicate_rationale(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::DuplicateRationale);
    // Finding should be on the second Rationale (line 12)
    assert_eq!(
        findings[0].position.as_ref().map(|p| p.line),
        Some(12),
        "finding should point to the second Rationale",
    );
    assert!(
        findings[0].message.contains("duplicate"),
        "message should mention duplicate, got: {}",
        findings[0].message,
    );
}

#[test]
fn duplicate_rationale_draft_gating() {
    let decision = make_decision(vec![make_rationale(11), make_rationale(12)], 10);
    let docs = vec![make_doc_with_status("adr/logging", "draft", vec![decision])];
    let graph = build_test_graph(docs);
    let config = test_config();
    let options = crate::VerifyOptions::default();
    let ag = crate::artifact_graph::ArtifactGraph::empty(&graph);
    let report =
        crate::verify(&graph, &config, std::path::Path::new("/tmp"), &options, &ag).unwrap();
    for finding in &report.findings {
        if finding.rule == RuleName::DuplicateRationale {
            assert_eq!(
                finding.effective_severity,
                crate::report::ReportSeverity::Info,
                "draft doc duplicate rationale findings should be Info, got {:?}",
                finding.effective_severity,
            );
        }
    }
}

// -----------------------------------------------------------------------
// check_alternative_status
// -----------------------------------------------------------------------

#[test]
fn alternative_with_status_rejected_no_finding() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "rejected", 11)],
        10,
    );
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert!(
        findings.is_empty(),
        "Alternative with status='rejected' should produce no findings, got: {findings:?}",
    );
}

#[test]
fn alternative_with_status_deferred_no_finding() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "deferred", 11)],
        10,
    );
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert!(
        findings.is_empty(),
        "Alternative with status='deferred' should produce no findings, got: {findings:?}",
    );
}

#[test]
fn alternative_with_status_superseded_no_finding() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "superseded", 11)],
        10,
    );
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert!(
        findings.is_empty(),
        "Alternative with status='superseded' should produce no findings, got: {findings:?}",
    );
}

#[verifies("decision-components/req#req-3-2")]
#[test]
fn alternative_with_status_accepted_emits_finding() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "accepted", 11)],
        10,
    );
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidAlternativeStatus);
    assert!(
        findings[0].message.contains("accepted"),
        "message should mention the invalid status, got: {}",
        findings[0].message,
    );
}

#[test]
fn alternative_with_empty_status_emits_finding() {
    let decision = make_decision(vec![make_alternative_with_status("alt-1", "", 11)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidAlternativeStatus);
}

#[test]
fn alternative_without_status_attribute_no_finding() {
    // Alternative without any status attribute should not fire this rule
    let decision = make_decision(vec![make_alternative("alt-1", 11)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert!(
        findings.is_empty(),
        "Alternative without status attribute should produce no findings, got: {findings:?}",
    );
}

#[verifies("decision-components/req#req-3-3")]
#[test]
fn alternative_status_default_severity_is_warning() {
    assert_eq!(
        RuleName::InvalidAlternativeStatus.default_severity(),
        crate::report::ReportSeverity::Warning,
    );
}

#[test]
fn alternative_status_draft_gating() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "accepted", 11)],
        10,
    );
    let docs = vec![make_doc_with_status("adr/logging", "draft", vec![decision])];
    let graph = build_test_graph(docs);
    let config = test_config();
    let options = crate::VerifyOptions::default();
    let ag = crate::artifact_graph::ArtifactGraph::empty(&graph);
    let report =
        crate::verify(&graph, &config, std::path::Path::new("/tmp"), &options, &ag).unwrap();
    for finding in &report.findings {
        if finding.rule == RuleName::InvalidAlternativeStatus {
            assert_eq!(
                finding.effective_severity,
                crate::report::ReportSeverity::Info,
                "draft doc alternative status findings should be Info, got {:?}",
                finding.effective_severity,
            );
        }
    }
}
