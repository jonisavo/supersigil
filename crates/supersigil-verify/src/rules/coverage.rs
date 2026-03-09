use supersigil_core::DocumentGraph;

use crate::artifact_graph::ArtifactGraph;

use crate::report::{Finding, RuleName};

pub fn check(graph: &DocumentGraph, artifact_graph: &ArtifactGraph<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (doc_id, doc) in graph.documents() {
        for_each_criterion(&doc.components, doc_id, artifact_graph, &mut findings);
    }

    findings
}

fn for_each_criterion(
    components: &[supersigil_core::ExtractedComponent],
    doc_id: &str,
    artifact_graph: &ArtifactGraph<'_>,
    findings: &mut Vec<Finding>,
) {
    for component in components {
        if component.name == "Criterion"
            && let Some(criterion_id) = component.attributes.get("id")
        {
            let has_evidence = artifact_graph.has_evidence(doc_id, criterion_id);
            if !has_evidence {
                let message = format!("criterion `{criterion_id}` has no verification evidence");
                findings.push(Finding::new(
                    RuleName::MissingVerificationEvidence,
                    Some(doc_id.to_owned()),
                    message,
                    Some(component.position),
                ));
            }
        }
        // Recurse into children (Criterion inside AcceptanceCriteria)
        for_each_criterion(&component.children, doc_id, artifact_graph, findings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn criterion_without_evidence_is_uncovered() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ag = ArtifactGraph::empty(&graph);
        let findings = check(&graph, &ag);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::MissingVerificationEvidence);
        assert_eq!(findings[0].doc_id.as_deref(), Some("req/auth"));
        assert!(findings[0].message.contains("req-1"));
        assert!(findings[0].message.contains("no verification evidence"));
    }

    #[test]
    fn references_do_not_satisfy_coverage() {
        let docs = vec![
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
            make_doc("design/auth", vec![make_references("req/auth#req-1", 5)]),
        ];
        let graph = build_test_graph(docs);
        let ag = ArtifactGraph::empty(&graph);
        let findings = check(&graph, &ag);
        assert_eq!(findings.len(), 1, "References should not satisfy coverage");
        assert_eq!(findings[0].rule, RuleName::MissingVerificationEvidence);
    }

    #[test]
    fn multiple_uncovered_criteria() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10), make_criterion("req-2", 20)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ag = ArtifactGraph::empty(&graph);
        let findings = check(&graph, &ag);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn direct_artifact_evidence_satisfies_coverage_without_validating_doc() {
        use std::collections::BTreeMap;
        use std::path::PathBuf;

        use supersigil_evidence::{
            EvidenceId, EvidenceKind, PluginProvenance, SourceLocation, TestIdentity, TestKind,
            VerifiableRef, VerificationEvidenceRecord, VerificationTargets,
        };

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ag = crate::artifact_graph::build_artifact_graph(
            &graph,
            vec![],
            vec![VerificationEvidenceRecord {
                id: EvidenceId(0),
                targets: VerificationTargets::single(VerifiableRef {
                    doc_id: "req/auth".into(),
                    target_id: "req-1".into(),
                }),
                test: TestIdentity {
                    file: PathBuf::from("tests/auth_test.rs"),
                    name: "login_succeeds".into(),
                    kind: TestKind::Unit,
                },
                source_location: SourceLocation {
                    file: PathBuf::from("tests/auth_test.rs"),
                    line: 3,
                    column: 1,
                },
                evidence_kind: EvidenceKind::RustAttribute,
                provenance: vec![PluginProvenance::RustAttribute {
                    attribute_span: SourceLocation {
                        file: PathBuf::from("tests/auth_test.rs"),
                        line: 3,
                        column: 1,
                    },
                }],
                metadata: BTreeMap::new(),
            }],
        );

        let findings = check(&graph, &ag);

        assert!(
            findings.is_empty(),
            "direct criterion evidence should satisfy coverage: {findings:?}",
        );
    }
}
