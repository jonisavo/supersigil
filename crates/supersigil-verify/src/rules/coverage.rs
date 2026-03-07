use supersigil_core::DocumentGraph;

use crate::report::{Finding, RuleName};

pub fn check(graph: &DocumentGraph) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (doc_id, doc) in graph.documents() {
        for_each_criterion(&doc.components, doc_id, graph, &mut findings);
    }

    findings
}

fn for_each_criterion(
    components: &[supersigil_core::ExtractedComponent],
    doc_id: &str,
    graph: &DocumentGraph,
    findings: &mut Vec<Finding>,
) {
    for component in components {
        if component.name == "Criterion"
            && let Some(criterion_id) = component.attributes.get("id")
        {
            let validators = graph.validates(doc_id, Some(criterion_id));
            if validators.is_empty() {
                findings.push(Finding {
                    rule: RuleName::UncoveredCriterion,
                    doc_id: Some(doc_id.to_owned()),
                    message: format!("criterion `{criterion_id}` has no validating property"),
                    effective_severity: RuleName::UncoveredCriterion.default_severity(),
                    raw_severity: RuleName::UncoveredCriterion.default_severity(),
                    position: Some(component.position),
                });
            }
        }
        // Recurse into children (Criterion inside AcceptanceCriteria)
        for_each_criterion(&component.children, doc_id, graph, findings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn criterion_with_validates_is_covered() {
        let docs = vec![
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
            make_doc("prop/auth", vec![make_validates("req/auth#req-1", 5)]),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty(), "expected no findings: {findings:?}");
    }

    #[test]
    fn criterion_without_validates_is_uncovered() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::UncoveredCriterion);
        assert_eq!(findings[0].doc_id.as_deref(), Some("req/auth"));
        assert!(findings[0].message.contains("req-1"));
    }

    #[test]
    fn illustrates_does_not_satisfy_coverage() {
        let docs = vec![
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
            make_doc("example/auth", vec![make_illustrates("req/auth#req-1", 5)]),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(findings.len(), 1, "Illustrates should not satisfy coverage");
        assert_eq!(findings[0].rule, RuleName::UncoveredCriterion);
    }

    #[test]
    fn multiple_uncovered_criteria() {
        let docs = vec![
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10), make_criterion("req-2", 20)],
                    9,
                )],
            ),
            make_doc("prop/auth", vec![make_validates("req/auth#req-1", 5)]),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("req-2"));
    }
}
