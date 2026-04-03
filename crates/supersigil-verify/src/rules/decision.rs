use supersigil_core::{
    DECISION, DEPENDS_ON, DocumentGraph, RATIONALE, REFERENCES, SpecDocument, TRACKED_FILES,
    decision_references_target,
};

use crate::report::{Finding, RuleName};

// ---------------------------------------------------------------------------
// check_incomplete
// ---------------------------------------------------------------------------

/// Check that every `Decision` component has at least one `Rationale` child.
///
/// A Decision without a Rationale is considered incomplete — the author has
/// recorded a decision but not yet explained *why* it was made.
pub fn check_incomplete(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        for decision in super::find_components(&doc.components, DECISION) {
            let has_rationale = decision.children.iter().any(|c| c.name == RATIONALE);
            if !has_rationale {
                findings.push(Finding::new(
                    RuleName::IncompleteDecision,
                    Some(doc_id.to_owned()),
                    format!(
                        "Decision in `{doc_id}` has no Rationale child; \
                         every Decision should include a Rationale"
                    ),
                    Some(decision.position),
                ));
            }
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_orphan
// ---------------------------------------------------------------------------

/// Check that every `Decision` component has at least one outward connection
/// or is referenced by another component.
///
/// A Decision is "orphan" when it has no `References`, `TrackedFiles`, or
/// `DependsOn` children **and** no other document references it via the
/// graph's reverse index.
pub fn check_orphan(docs: &[&SpecDocument], graph: &DocumentGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        for decision in super::find_components(&doc.components, DECISION) {
            // Decisions marked standalone are intentionally unconnected
            if decision.attributes.contains_key("standalone") {
                continue;
            }

            let has_outward = decision
                .children
                .iter()
                .any(|c| c.name == REFERENCES || c.name == TRACKED_FILES || c.name == DEPENDS_ON);

            if !has_outward {
                // Check if any other component references this decision
                let decision_id = decision.attributes.get("id").map(String::as_str);
                let is_referenced = !graph.references(doc_id, decision_id).is_empty();

                if !is_referenced {
                    let label = decision_id.unwrap_or("<unnamed>");
                    findings.push(Finding::new(
                        RuleName::OrphanDecision,
                        Some(doc_id.to_owned()),
                        format!(
                            "Decision `{label}` in `{doc_id}` is orphan: no outward \
                             connections and not referenced by any other component"
                        ),
                        Some(decision.position),
                    ));
                }
            }
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_coverage
// ---------------------------------------------------------------------------

/// Check that every design document has at least one Decision covering it.
///
/// A design document is "covered" when:
/// - it contains a Decision component directly in its own component tree, **or**
/// - another document that references it (via `graph.references`) contains a
///   Decision component whose nested `References` child targets this design doc.
///
/// Only documents with `doc_type == Some("design")` are checked.
pub fn check_coverage(docs: &[&SpecDocument], graph: &DocumentGraph) -> Vec<Finding> {
    let mut findings = Vec::new();

    for doc in docs {
        // Only check design documents
        if doc.frontmatter.doc_type.as_deref() != Some("design") {
            continue;
        }

        let doc_id = &doc.frontmatter.id;

        // (a) Check the document itself for Decision components
        if super::has_component(&doc.components, DECISION) {
            continue;
        }

        // (b) Check reverse references for source docs containing Decisions
        //     whose nested References target this design doc
        let referencing_doc_ids = graph.references(doc_id, None);
        let mut covered = false;

        for ref_doc_id in referencing_doc_ids {
            if let Some(ref_doc) = graph.document(ref_doc_id)
                && has_decision_referencing(&ref_doc.components, doc_id)
            {
                covered = true;
                break;
            }
        }

        if !covered {
            findings.push(Finding::new(
                RuleName::MissingDecisionCoverage,
                Some(doc_id.to_owned()),
                format!(
                    "Design document `{doc_id}` has no Decision covering it; \
                     add a Decision in this document or in a referencing document"
                ),
                None,
            ));
        }
    }

    findings
}

/// Returns `true` if any `Decision` component (recursively) contains a
/// `References` child whose `refs` attribute targets `target_doc_id`.
fn has_decision_referencing(
    components: &[supersigil_core::ExtractedComponent],
    target_doc_id: &str,
) -> bool {
    for comp in components {
        if comp.name == DECISION && decision_references_target(&comp.children, target_doc_id) {
            return true;
        }
        // Recurse into children
        if has_decision_referencing(&comp.children, target_doc_id) {
            return true;
        }
    }
    false
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use supersigil_rust::verifies;

    // ===================================================================
    // check_incomplete tests
    // ===================================================================

    #[test]
    fn decision_with_rationale_no_finding() {
        let docs = [make_doc(
            "adr/logging",
            vec![make_decision(vec![make_rationale(11)], 10)],
        )];
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_incomplete(&refs);
        assert!(
            findings.is_empty(),
            "Decision with Rationale should produce no findings, got: {findings:?}"
        );
    }

    #[verifies("decision-components/req#req-5-1")]
    #[test]
    fn decision_without_rationale_produces_finding() {
        let docs = [make_doc("adr/logging", vec![make_decision(vec![], 10)])];
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_incomplete(&refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::IncompleteDecision);
        assert_eq!(findings[0].doc_id.as_deref(), Some("adr/logging"));
        assert!(findings[0].message.contains("no Rationale child"));
    }

    #[test]
    fn default_severity_is_warning() {
        assert_eq!(
            RuleName::IncompleteDecision.default_severity(),
            crate::report::ReportSeverity::Warning,
        );
    }

    #[verifies("decision-components/req#req-5-4")]
    #[test]
    fn draft_gating_suppresses_to_info() {
        let docs = [make_doc_with_status(
            "adr/logging",
            "draft",
            vec![make_decision(vec![], 10)],
        )];
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_incomplete(&refs);
        assert_eq!(findings.len(), 1);
        // Raw severity should be warning
        assert_eq!(
            findings[0].raw_severity,
            crate::report::ReportSeverity::Warning,
        );
        // Severity resolution is done externally; verify it works via resolve_severity
        let config = supersigil_core::VerifyConfig::default();
        let effective =
            crate::severity::resolve_severity(&findings[0].rule, Some("draft"), &config);
        assert_eq!(effective, crate::report::ReportSeverity::Info);
    }

    #[test]
    fn per_rule_override_to_off_suppresses() {
        let docs = [make_doc("adr/logging", vec![make_decision(vec![], 10)])];
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_incomplete(&refs);
        assert_eq!(findings.len(), 1);

        let config = supersigil_core::VerifyConfig {
            strictness: None,
            rules: std::collections::HashMap::from([(
                "incomplete_decision".to_string(),
                supersigil_core::Severity::Off,
            )]),
        };
        let effective = crate::severity::resolve_severity(&findings[0].rule, None, &config);
        assert_eq!(effective, crate::report::ReportSeverity::Off);
    }

    // ===================================================================
    // check_orphan tests
    // ===================================================================

    #[test]
    fn decision_with_references_child_no_finding() {
        let docs = vec![
            make_doc(
                "adr/logging",
                vec![make_decision_with_id(
                    "dec-1",
                    vec![make_references("other/doc", 11)],
                    10,
                )],
            ),
            make_doc("other/doc", vec![]),
        ];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_orphan(&refs, &graph);
        assert!(
            findings.is_empty(),
            "Decision with References child should not be orphan, got: {findings:?}"
        );
    }

    #[test]
    fn decision_with_tracked_files_child_no_finding() {
        let docs = vec![make_doc(
            "adr/logging",
            vec![make_decision_with_id(
                "dec-1",
                vec![make_tracked_files("src/**/*.rs", 11)],
                10,
            )],
        )];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_orphan(&refs, &graph);
        assert!(
            findings.is_empty(),
            "Decision with TrackedFiles child should not be orphan, got: {findings:?}"
        );
    }

    #[test]
    fn decision_with_depends_on_child_no_finding() {
        let other_doc = make_doc("other/doc", vec![]);
        let docs = vec![
            make_doc(
                "adr/logging",
                vec![make_decision_with_id(
                    "dec-1",
                    vec![make_depends_on("other/doc", 11)],
                    10,
                )],
            ),
            other_doc,
        ];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_orphan(&refs, &graph);
        assert!(
            findings.is_empty(),
            "Decision with DependsOn child should not be orphan, got: {findings:?}"
        );
    }

    #[test]
    fn decision_referenced_by_another_doc_no_finding() {
        // Another document references this decision via References component
        let docs = vec![
            make_doc(
                "adr/logging",
                vec![make_decision_with_id("dec-1", vec![], 10)],
            ),
            make_doc("prop/auth", vec![make_references("adr/logging#dec-1", 5)]),
        ];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_orphan(&refs, &graph);
        assert!(
            findings.is_empty(),
            "Decision referenced by another doc should not be orphan, got: {findings:?}"
        );
    }

    #[verifies("decision-components/req#req-5-2")]
    #[test]
    fn decision_no_connections_and_not_referenced_produces_finding() {
        let docs = vec![make_doc(
            "adr/logging",
            vec![make_decision_with_id("dec-1", vec![], 10)],
        )];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_orphan(&refs, &graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::OrphanDecision);
        assert_eq!(findings[0].doc_id.as_deref(), Some("adr/logging"));
        assert!(findings[0].message.contains("orphan"));
    }

    #[verifies("decision-components/req#req-1-5")]
    #[test]
    fn standalone_decision_no_orphan_finding() {
        let docs = vec![make_doc(
            "adr/technology",
            vec![make_decision_standalone(
                "rust-single-binary",
                "Project-level technology choice with no corresponding requirement",
                vec![make_rationale(11)],
                10,
            )],
        )];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_orphan(&refs, &graph);
        assert!(
            findings.is_empty(),
            "Decision with standalone attribute should not be orphan, got: {findings:?}"
        );
    }

    #[test]
    fn orphan_decision_default_severity_is_warning() {
        assert_eq!(
            RuleName::OrphanDecision.default_severity(),
            crate::report::ReportSeverity::Warning,
        );
    }

    #[test]
    fn orphan_decision_draft_gating_suppresses_to_info() {
        let docs = vec![make_doc_with_status(
            "adr/logging",
            "draft",
            vec![make_decision_with_id("dec-1", vec![], 10)],
        )];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_orphan(&refs, &graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].raw_severity,
            crate::report::ReportSeverity::Warning,
        );
        let config = supersigil_core::VerifyConfig::default();
        let effective =
            crate::severity::resolve_severity(&findings[0].rule, Some("draft"), &config);
        assert_eq!(effective, crate::report::ReportSeverity::Info);
    }

    #[test]
    fn orphan_decision_per_rule_override_to_off() {
        let docs = vec![make_doc(
            "adr/logging",
            vec![make_decision_with_id("dec-1", vec![], 10)],
        )];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_orphan(&refs, &graph);
        assert_eq!(findings.len(), 1);

        let config = supersigil_core::VerifyConfig {
            strictness: None,
            rules: std::collections::HashMap::from([(
                "orphan_decision".to_string(),
                supersigil_core::Severity::Off,
            )]),
        };
        let effective = crate::severity::resolve_severity(&findings[0].rule, None, &config);
        assert_eq!(effective, crate::report::ReportSeverity::Off);
    }

    // ===================================================================
    // check_coverage tests
    // ===================================================================

    #[test]
    fn coverage_design_doc_with_decision_in_another_doc_no_finding() {
        // Another document has a Decision with References pointing to the design doc
        let docs = vec![
            make_doc_typed("design/auth", "design", None, vec![]),
            make_doc(
                "adr/auth-decision",
                vec![make_decision_with_id(
                    "dec-1",
                    vec![make_references("design/auth", 11)],
                    10,
                )],
            ),
        ];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_coverage(&refs, &graph);
        assert!(
            findings.is_empty(),
            "Design doc covered by Decision in another doc should produce no findings, got: {findings:?}"
        );
    }

    #[test]
    fn coverage_design_doc_with_decision_in_same_doc_no_finding() {
        // The design doc itself contains a Decision component
        let docs = vec![make_doc_typed(
            "design/auth",
            "design",
            None,
            vec![make_decision(vec![make_rationale(11)], 10)],
        )];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_coverage(&refs, &graph);
        assert!(
            findings.is_empty(),
            "Design doc with inline Decision should produce no findings, got: {findings:?}"
        );
    }

    #[verifies("decision-components/req#req-5-3")]
    #[test]
    fn coverage_design_doc_with_no_decision_produces_finding() {
        // Design doc with no Decision anywhere
        let docs = vec![make_doc_typed("design/auth", "design", None, vec![])];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_coverage(&refs, &graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::MissingDecisionCoverage);
        assert_eq!(findings[0].doc_id.as_deref(), Some("design/auth"));
        assert!(findings[0].message.contains("no Decision covering it"));
    }

    #[test]
    fn coverage_non_design_doc_no_finding() {
        // A non-design doc with no Decision should NOT produce a finding
        let docs = vec![make_doc("req/auth", vec![])];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_coverage(&refs, &graph);
        assert!(
            findings.is_empty(),
            "Non-design doc should never produce a MissingDecisionCoverage finding, got: {findings:?}"
        );
    }

    #[test]
    fn coverage_default_severity_is_off() {
        assert_eq!(
            RuleName::MissingDecisionCoverage.default_severity(),
            crate::report::ReportSeverity::Off,
        );
    }

    #[test]
    fn coverage_per_rule_override_to_warning_activates() {
        let docs = vec![make_doc_typed("design/auth", "design", None, vec![])];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = check_coverage(&refs, &graph);
        assert_eq!(findings.len(), 1);

        // Default severity is Off, so finding is suppressed by default
        let default_config = supersigil_core::VerifyConfig::default();
        let default_effective =
            crate::severity::resolve_severity(&findings[0].rule, None, &default_config);
        assert_eq!(
            default_effective,
            crate::report::ReportSeverity::Off,
            "default severity should suppress the finding"
        );

        // Per-rule override to warning activates the check
        let config = supersigil_core::VerifyConfig {
            strictness: None,
            rules: std::collections::HashMap::from([(
                "missing_decision_coverage".to_string(),
                supersigil_core::Severity::Warning,
            )]),
        };
        let effective = crate::severity::resolve_severity(&findings[0].rule, None, &config);
        assert_eq!(
            effective,
            crate::report::ReportSeverity::Warning,
            "per-rule override to warning should activate the finding"
        );
    }

    // ===================================================================
    // No document-type enforcement tests
    // ===================================================================

    /// A well-formed Decision: has a Rationale child and a References child so
    /// it satisfies both `check_incomplete` and `check_orphan`.
    fn make_well_formed_decision() -> supersigil_core::ExtractedComponent {
        make_decision_with_id(
            "dec-1",
            vec![make_rationale(11), make_references("other/doc", 12)],
            10,
        )
    }

    #[verifies("decision-components/req#req-1-2")]
    #[test]
    fn decision_in_requirements_doc_is_clean() {
        // Decision components are not restricted to any document type.
        // A `requirements` document containing a valid Decision should produce
        // no findings from any decision rule.
        let other = make_doc("other/doc", vec![]);
        let docs = vec![
            make_doc_typed(
                "req/auth",
                "requirements",
                None,
                vec![make_well_formed_decision()],
            ),
            other,
        ];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();

        let incomplete = check_incomplete(&refs);
        assert!(
            incomplete.is_empty(),
            "requirements doc with Decision should not trigger IncompleteDecision, got: {incomplete:?}"
        );

        let orphan = check_orphan(&refs, &graph);
        assert!(
            orphan.is_empty(),
            "requirements doc with Decision should not trigger OrphanDecision, got: {orphan:?}"
        );

        let coverage = check_coverage(&refs, &graph);
        assert!(
            coverage.is_empty(),
            "requirements doc is not a design doc, so MissingDecisionCoverage must not fire, got: {coverage:?}"
        );
    }

    #[test]
    fn decision_in_tasks_doc_is_clean() {
        // A `tasks` document containing a valid Decision should produce no
        // findings from any decision rule.
        let other = make_doc("other/doc", vec![]);
        let docs = vec![
            make_doc_typed(
                "feature/tasks",
                "tasks",
                None,
                vec![make_well_formed_decision()],
            ),
            other,
        ];
        let graph = build_test_graph(docs.clone());
        let refs: Vec<&SpecDocument> = docs.iter().collect();

        let incomplete = check_incomplete(&refs);
        assert!(
            incomplete.is_empty(),
            "tasks doc with Decision should not trigger IncompleteDecision, got: {incomplete:?}"
        );

        let orphan = check_orphan(&refs, &graph);
        assert!(
            orphan.is_empty(),
            "tasks doc with Decision should not trigger OrphanDecision, got: {orphan:?}"
        );

        let coverage = check_coverage(&refs, &graph);
        assert!(
            coverage.is_empty(),
            "tasks doc is not a design doc, so MissingDecisionCoverage must not fire, got: {coverage:?}"
        );
    }

    #[test]
    fn decision_with_zero_alternatives_is_clean() {
        // There is no minimum cardinality on Alternative children.
        // A Decision with no Alternative children should not produce any
        // placement or cardinality findings.
        let docs = [make_doc(
            "adr/logging",
            vec![make_decision_with_id(
                "dec-1",
                vec![make_rationale(11), make_references("other/doc", 12)],
                10,
            )],
        )];
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = crate::rules::structural::check_alternative_placement(&refs);
        assert!(
            findings.is_empty(),
            "Decision with zero alternatives should produce no placement findings, got: {findings:?}"
        );
    }

    #[test]
    fn decision_with_one_alternative_is_clean() {
        // A Decision with exactly one Alternative child is valid.
        let docs = [make_doc(
            "adr/logging",
            vec![make_decision_with_id(
                "dec-1",
                vec![
                    make_rationale(11),
                    make_references("other/doc", 12),
                    make_alternative("alt-1", 13),
                ],
                10,
            )],
        )];
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = crate::rules::structural::check_alternative_placement(&refs);
        assert!(
            findings.is_empty(),
            "Decision with one alternative should produce no placement findings, got: {findings:?}"
        );
    }

    #[verifies("decision-components/req#req-3-5")]
    #[test]
    fn decision_with_three_alternatives_is_clean() {
        // There is no upper cardinality limit on Alternative children.
        // A Decision with three Alternative children should not produce any
        // placement or cardinality findings.
        let docs = [make_doc(
            "adr/logging",
            vec![make_decision_with_id(
                "dec-1",
                vec![
                    make_rationale(11),
                    make_references("other/doc", 12),
                    make_alternative("alt-1", 13),
                    make_alternative("alt-2", 14),
                    make_alternative("alt-3", 15),
                ],
                10,
            )],
        )];
        let refs: Vec<&SpecDocument> = docs.iter().collect();
        let findings = crate::rules::structural::check_alternative_placement(&refs);
        assert!(
            findings.is_empty(),
            "Decision with three alternatives should produce no placement findings, got: {findings:?}"
        );
    }
}
