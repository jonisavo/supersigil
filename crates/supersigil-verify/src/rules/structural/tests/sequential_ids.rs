use super::*;

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
