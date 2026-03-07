use supersigil_core::DocumentGraph;

use crate::report::{Finding, RuleName};
use crate::rules::{find_components, has_component};

pub fn check(graph: &DocumentGraph) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (doc_id, doc) in graph.documents() {
        let status = doc.frontmatter.status.as_deref();

        // Check 1: status "verified" but no VerifiedBy
        if status == Some("verified") {
            let has_validates = has_component(&doc.components, "Validates");
            let has_verified_by = has_component(&doc.components, "VerifiedBy");
            if has_validates && !has_verified_by {
                findings.push(Finding::new(
                    RuleName::StatusInconsistency,
                    Some(doc_id.to_owned()),
                    format!(
                        "document `{doc_id}` has status `verified` but contains Validates without VerifiedBy"
                    ),
                    None,
                ));
            }
        }

        // Check 2: status "implemented" but uncovered criteria
        if status == Some("implemented") {
            let criteria = find_components(&doc.components, "Criterion");
            for criterion in &criteria {
                if let Some(criterion_id) = criterion.attributes.get("id")
                    && graph.validates(doc_id, Some(criterion_id)).is_empty()
                {
                    findings.push(Finding::new(
                        RuleName::StatusInconsistency,
                        Some(doc_id.to_owned()),
                        format!(
                            "document `{doc_id}` has status `implemented` but criterion `{criterion_id}` is uncovered"
                        ),
                        Some(criterion.position),
                    ));
                }
            }
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn verified_status_with_no_verified_by_emits_finding() {
        let docs = vec![
            make_doc_with_status(
                "prop/auth",
                "verified",
                vec![
                    make_validates("req/auth#req-1", 5),
                    // No VerifiedBy
                ],
            ),
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::StatusInconsistency);
        assert_eq!(findings[0].doc_id.as_deref(), Some("prop/auth"));
    }

    #[test]
    fn verified_status_with_verified_by_is_clean() {
        let docs = vec![
            make_doc_with_status(
                "prop/auth",
                "verified",
                vec![
                    make_validates("req/auth#req-1", 5),
                    make_verified_by_tag("prop:auth", 6),
                ],
            ),
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty());
    }

    #[test]
    fn implemented_status_with_uncovered_criteria_emits_finding() {
        let docs = vec![
            make_doc_with_status(
                "req/auth",
                "implemented",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10), make_criterion("req-2", 20)],
                    9,
                )],
            ),
            make_doc(
                "prop/auth",
                vec![
                    make_validates("req/auth#req-1", 5),
                    // req-2 not validated
                ],
            ),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::StatusInconsistency);
        assert!(findings[0].message.contains("req-2"));
    }

    #[test]
    fn implemented_status_all_covered_is_clean() {
        let docs = vec![
            make_doc_with_status(
                "req/auth",
                "implemented",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
            make_doc("prop/auth", vec![make_validates("req/auth#req-1", 5)]),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty());
    }

    #[test]
    fn non_status_document_not_checked() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty());
    }
}
