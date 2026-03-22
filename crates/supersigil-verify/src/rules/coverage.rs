use std::collections::HashSet;

use supersigil_core::{CRITERION, DocumentGraph, EXAMPLE, split_criterion_ref};

use crate::artifact_graph::ArtifactGraph;

use crate::report::{Finding, FindingDetails, RuleName};

pub fn check(graph: &DocumentGraph, artifact_graph: &ArtifactGraph<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Collect unique target_ids from unresolved evidence up front.
    let unresolved = artifact_graph.unresolved_evidence();
    let unresolved_fragments: HashSet<&str> = unresolved
        .iter()
        .flat_map(|record| record.targets.iter().map(|t| t.target_id.as_str()))
        .collect();

    // Pre-compute the set of (doc_id, criterion_id) pairs that any unresolved
    // fragment matches, so the per-criterion check is O(1).
    let mut suggestable: HashSet<(&str, &str)> = HashSet::new();
    for &fragment in &unresolved_fragments {
        for (doc_id, comp) in graph.criteria_by_fragment(fragment) {
            if let Some(id) = comp.attributes.get("id") {
                suggestable.insert((doc_id, id.as_str()));
            }
        }
    }

    // Pre-compute (doc_id, criterion_id) pairs targeted by <Example verifies="...">
    // so we can flag findings as example-coverable.
    let example_targets = collect_example_targets(graph);

    for (doc_id, doc) in graph.documents() {
        for_each_criterion(
            &doc.components,
            doc_id,
            artifact_graph,
            &suggestable,
            &example_targets,
            &mut findings,
        );
    }

    findings
}

/// Collect all `(doc_id, criterion_id)` pairs targeted by `<Example verifies="...">`
/// components across the entire graph.
fn collect_example_targets(graph: &DocumentGraph) -> HashSet<(String, String)> {
    let mut targets = HashSet::new();
    for (_doc_id, doc) in graph.documents() {
        collect_example_targets_from(&doc.components, &mut targets);
    }
    targets
}

fn collect_example_targets_from(
    components: &[supersigil_core::ExtractedComponent],
    targets: &mut HashSet<(String, String)>,
) {
    for component in components {
        if component.name == EXAMPLE
            && let Some(verifies) = component.attributes.get("verifies")
        {
            for ref_str in verifies.split(',') {
                let ref_str = ref_str.trim();
                if let Some((doc_id, criterion_id)) = split_criterion_ref(ref_str) {
                    targets.insert((doc_id.to_owned(), criterion_id.to_owned()));
                }
            }
        }
        collect_example_targets_from(&component.children, targets);
    }
}

fn for_each_criterion(
    components: &[supersigil_core::ExtractedComponent],
    doc_id: &str,
    artifact_graph: &ArtifactGraph<'_>,
    suggestable: &HashSet<(&str, &str)>,
    example_targets: &HashSet<(String, String)>,
    findings: &mut Vec<Finding>,
) {
    for component in components {
        if component.name == CRITERION
            && let Some(criterion_id) = component.attributes.get("id")
        {
            let has_evidence = artifact_graph.has_evidence(doc_id, criterion_id);
            if !has_evidence {
                let example_coverable =
                    example_targets.contains(&(doc_id.to_owned(), criterion_id.to_owned()));

                let mut message =
                    format!("criterion `{criterion_id}` has no verification evidence");

                if suggestable.contains(&(doc_id, criterion_id.as_str())) {
                    message = format!("{message}; did you mean `{doc_id}#{criterion_id}`?");
                }

                let target_ref = format!("{doc_id}#{criterion_id}");
                findings.push(
                    Finding::new(
                        RuleName::MissingVerificationEvidence,
                        Some(doc_id.to_owned()),
                        message,
                        Some(component.position),
                    )
                    .with_details(FindingDetails {
                        suggestion: Some(format!(
                            "Run `supersigil refs` to list targets. \
                             Fix: add a `<VerifiedBy>` component to this criterion, \
                             or use a language plugin (e.g. `#[verifies(\"{target_ref}\")]` for Rust)."
                        )),
                        target_ref: Some(target_ref),
                        example_coverable,
                        ..FindingDetails::default()
                    }),
                );
            }
        }
        // Recurse into children (Criterion inside AcceptanceCriteria)
        for_each_criterion(
            &component.children,
            doc_id,
            artifact_graph,
            suggestable,
            example_targets,
            findings,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use supersigil_core::ExtractedComponent;

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
    fn example_component_does_not_satisfy_coverage() {
        // An Example component is referenceable but NOT verifiable,
        // so its presence alongside an uncovered Criterion must not
        // accidentally satisfy coverage.
        let docs = vec![make_doc(
            "req/auth",
            vec![
                make_acceptance_criteria(vec![make_criterion("req-1", 10)], 9),
                ExtractedComponent {
                    name: EXAMPLE.to_owned(),
                    attributes: HashMap::from([
                        ("id".into(), "ex-1".into()),
                        ("runner".into(), "sh".into()),
                    ]),
                    children: vec![],
                    body_text: None,
                    code_blocks: vec![],
                    position: pos(20),
                },
            ],
        )];
        let graph = build_test_graph(docs);
        let ag = ArtifactGraph::empty(&graph);
        let findings = check(&graph, &ag);
        assert_eq!(
            findings.len(),
            1,
            "Example should NOT satisfy coverage for req-1: {findings:?}",
        );
        assert!(findings[0].message.contains("req-1"));
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
            EvidenceId, PluginProvenance, SourceLocation, TestIdentity, TestKind, VerifiableRef,
            VerificationEvidenceRecord, VerificationTargets,
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
                id: EvidenceId::new(0),
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

    // -----------------------------------------------------------------------
    // "Did you mean" suggestions (req-4-1, req-4-2)
    // -----------------------------------------------------------------------

    fn make_evidence_record(
        doc_id: &str,
        target_id: &str,
    ) -> supersigil_evidence::VerificationEvidenceRecord {
        use std::collections::BTreeMap;
        use std::path::PathBuf;

        use supersigil_evidence::{
            EvidenceId, PluginProvenance, SourceLocation, TestIdentity, TestKind, VerifiableRef,
            VerificationEvidenceRecord, VerificationTargets,
        };

        VerificationEvidenceRecord {
            id: EvidenceId::new(0),
            targets: VerificationTargets::single(VerifiableRef {
                doc_id: doc_id.into(),
                target_id: target_id.into(),
            }),
            test: TestIdentity {
                file: PathBuf::from("tests/test.rs"),
                name: "some_test".into(),
                kind: TestKind::Unit,
            },
            source_location: SourceLocation {
                file: PathBuf::from("tests/test.rs"),
                line: 1,
                column: 1,
            },
            provenance: vec![PluginProvenance::RustAttribute {
                attribute_span: SourceLocation {
                    file: PathBuf::from("tests/test.rs"),
                    line: 1,
                    column: 1,
                },
            }],
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn unresolved_evidence_with_matching_fragment_suggests_full_ref() {
        // Graph has ("req/auth", "crit-1"), evidence targets ("wrong-doc", "crit-1").
        // The evidence is unresolved because graph.component("wrong-doc", "crit-1") is None.
        // The coverage rule should suggest "did you mean `req/auth#crit-1`?".
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        // Evidence targets a wrong doc_id but correct fragment
        let evidence = make_evidence_record("wrong-doc", "crit-1");
        let ag = crate::artifact_graph::build_artifact_graph(&graph, vec![], vec![evidence]);

        let findings = check(&graph, &ag);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("did you mean"),
            "expected 'did you mean' suggestion, got: {}",
            findings[0].message,
        );
        assert!(
            findings[0].message.contains("req/auth#crit-1"),
            "expected suggestion to contain 'req/auth#crit-1', got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn unresolved_evidence_with_non_matching_fragment_keeps_original_message() {
        // Graph has ("req/auth", "crit-1"), evidence targets ("wrong-doc", "nonexistent").
        // The evidence is unresolved, but the fragment doesn't match any criterion.
        // Message should stay as the original "has no verification evidence".
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        let evidence = make_evidence_record("wrong-doc", "nonexistent");
        let ag = crate::artifact_graph::build_artifact_graph(&graph, vec![], vec![evidence]);

        let findings = check(&graph, &ag);
        assert_eq!(findings.len(), 1);
        assert!(
            !findings[0].message.contains("did you mean"),
            "should NOT contain 'did you mean' when fragment doesn't match, got: {}",
            findings[0].message,
        );
        assert!(
            findings[0].message.contains("no verification evidence"),
            "should contain original message, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn no_unresolved_evidence_keeps_original_message() {
        // Graph has ("req/auth", "crit-1"), no evidence at all.
        // Message should stay as the original "has no verification evidence".
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ag = ArtifactGraph::empty(&graph);

        let findings = check(&graph, &ag);
        assert_eq!(findings.len(), 1);
        assert!(
            !findings[0].message.contains("did you mean"),
            "should NOT contain 'did you mean' when no unresolved evidence, got: {}",
            findings[0].message,
        );
        assert!(
            findings[0].message.contains("no verification evidence"),
            "should contain original message, got: {}",
            findings[0].message,
        );
    }
}
