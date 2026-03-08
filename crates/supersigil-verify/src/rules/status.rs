use supersigil_core::DocumentGraph;

use crate::report::Finding;

/// Check status field consistency.
///
/// Coverage checking (whether criteria have verification evidence) is handled
/// by `rules::coverage` via `ArtifactGraph`. References are informational and
/// carry no verification semantics, so they are not consulted here.
///
/// This rule currently performs no checks but exists as the extension point for
/// future status-consistency validations (e.g. valid status values, status
/// transition rules).
pub fn check(_graph: &DocumentGraph) -> Vec<Finding> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn status_rule_does_not_check_coverage_via_references() {
        // Coverage is handled by coverage::check via ArtifactGraph.
        // The status rule should not emit findings for uncovered criteria.
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
                "design/auth",
                vec![
                    make_references("req/auth#req-1", 5),
                    // req-2 not referenced — should not matter for status rule
                ],
            ),
        ];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(
            findings.is_empty(),
            "status rule should not emit coverage findings; got: {findings:?}",
        );
    }

    #[test]
    fn implemented_status_emits_no_findings() {
        let docs = vec![make_doc_with_status(
            "req/auth",
            "implemented",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let findings = check(&graph);
        assert!(findings.is_empty());
    }

    #[test]
    fn no_status_document_emits_no_findings() {
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
