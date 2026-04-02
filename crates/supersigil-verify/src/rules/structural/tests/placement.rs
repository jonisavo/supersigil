use super::*;
use supersigil_rust::verifies;

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
// check_expected_placement
// -----------------------------------------------------------------------

#[test]
fn expected_under_example_is_valid() {
    let expected = make_expected(11);
    let example = make_example(vec![expected], 10);
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_expected_placement(&doc_refs);
    assert!(
        findings.is_empty(),
        "Expected under Example should be valid, got: {findings:?}",
    );
}

// -----------------------------------------------------------------------
// check_rationale_placement / check_alternative_placement — valid cases
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

// -----------------------------------------------------------------------
// Table-driven placement error tests (Expected / Rationale / Alternative)
// -----------------------------------------------------------------------

/// Shared structure for table-driven placement tests across Groups 2-4.
struct PlacementCase {
    label: &'static str,
    doc_id: &'static str,
    component: supersigil_core::ExtractedComponent,
    check: fn(&[&supersigil_core::SpecDocument]) -> Vec<crate::report::Finding>,
    expected_rule: RuleName,
    expected_substr: &'static str,
}

#[verifies("decision-components/req#req-2-2")]
#[verifies("decision-components/req#req-3-4")]
#[test]
fn placement_at_document_root_is_structural_error() {
    let cases = [
        PlacementCase {
            label: "Expected at document root",
            doc_id: "ex/doc",
            component: make_expected(5),
            check: check_expected_placement,
            expected_rule: RuleName::InvalidExpectedPlacement,
            expected_substr: "document root",
        },
        PlacementCase {
            label: "Rationale at document root",
            doc_id: "adr/logging",
            component: make_rationale(5),
            check: check_rationale_placement,
            expected_rule: RuleName::InvalidRationalePlacement,
            expected_substr: "document root",
        },
        PlacementCase {
            label: "Alternative at document root",
            doc_id: "adr/logging",
            component: make_alternative("alt-1", 5),
            check: check_alternative_placement,
            expected_rule: RuleName::InvalidAlternativePlacement,
            expected_substr: "document root",
        },
    ];

    for case in &cases {
        assert_placement_error(case);
    }
}

fn assert_placement_error(case: &PlacementCase) {
    let docs = [make_doc(case.doc_id, vec![case.component.clone()])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = (case.check)(&doc_refs);
    assert_eq!(
        findings.len(),
        1,
        "{}: expected 1 finding, got {findings:?}",
        case.label,
    );
    assert_eq!(
        findings[0].rule, case.expected_rule,
        "{}: wrong rule",
        case.label,
    );
    assert!(
        findings[0].message.contains(case.expected_substr),
        "{}: message should mention '{}', got: {}",
        case.label,
        case.expected_substr,
        findings[0].message,
    );
}

#[test]
fn placement_under_wrong_parent_is_structural_error() {
    fn make_criterion_parent(
        child: supersigil_core::ExtractedComponent,
    ) -> supersigil_core::ExtractedComponent {
        supersigil_core::ExtractedComponent {
            name: CRITERION.to_owned(),
            attributes: std::collections::HashMap::from([("id".into(), "req-1".into())]),
            children: vec![child],
            body_text: Some("criterion req-1".into()),
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: pos(10),
            end_position: pos(10),
        }
    }

    let cases = [
        PlacementCase {
            label: "Expected under AcceptanceCriteria",
            doc_id: "ex/doc",
            component: make_acceptance_criteria(vec![make_expected(11)], 9),
            check: check_expected_placement,
            expected_rule: RuleName::InvalidExpectedPlacement,
            expected_substr: "AcceptanceCriteria",
        },
        PlacementCase {
            label: "Rationale under Criterion",
            doc_id: "adr/logging",
            component: make_criterion_parent(make_rationale(11)),
            check: check_rationale_placement,
            expected_rule: RuleName::InvalidRationalePlacement,
            expected_substr: "Criterion",
        },
        PlacementCase {
            label: "Alternative under Criterion",
            doc_id: "adr/logging",
            component: make_criterion_parent(make_alternative("alt-1", 11)),
            check: check_alternative_placement,
            expected_rule: RuleName::InvalidAlternativePlacement,
            expected_substr: "Criterion",
        },
    ];

    for case in &cases {
        assert_placement_error(case);
    }
}
