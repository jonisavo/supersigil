use std::path::PathBuf;

use supersigil_core::{ComponentDefs, Config, DocumentGraph, SpecDocument};

use crate::report::{Finding, RuleName};
use crate::rules::{find_components, has_component};

// ---------------------------------------------------------------------------
// check_required_components
// ---------------------------------------------------------------------------

/// For typed documents, check that all `required_components` from the config
/// type definition are present.
pub fn check_required_components(graph: &DocumentGraph, config: &Config) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (doc_id, doc) in graph.documents() {
        let Some(ref doc_type) = doc.frontmatter.doc_type else {
            continue;
        };
        let Some(type_def) = config.documents.types.get(doc_type) else {
            continue;
        };
        for required in &type_def.required_components {
            let has_it = has_component(&doc.components, required);
            if !has_it {
                findings.push(Finding::new(
                    RuleName::MissingRequiredComponent,
                    Some(doc_id.to_owned()),
                    format!(
                        "document `{doc_id}` (type `{doc_type}`) is missing required component `{required}`"
                    ),
                    None,
                ));
            }
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_id_pattern
// ---------------------------------------------------------------------------

/// If `config.id_pattern` is set, check that each document ID matches the regex.
pub fn check_id_pattern(graph: &DocumentGraph, config: &Config) -> Vec<Finding> {
    let Some(ref pattern) = config.id_pattern else {
        return Vec::new();
    };
    let Ok(re) = regex::Regex::new(pattern) else {
        return Vec::new(); // Invalid pattern is not this rule's problem
    };
    let mut findings = Vec::new();
    for (doc_id, _doc) in graph.documents() {
        if !re.is_match(doc_id) {
            findings.push(Finding::new(
                RuleName::InvalidIdPattern,
                Some(doc_id.to_owned()),
                format!("document ID `{doc_id}` does not match pattern `{pattern}`"),
                None,
            ));
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_isolated
// ---------------------------------------------------------------------------

/// Check each document for incoming or outgoing refs. Documents with neither
/// are flagged as isolated.
pub fn check_isolated(graph: &DocumentGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (doc_id, doc) in graph.documents() {
        // Check outgoing refs (document has ref components)
        let has_outgoing = ["References", "Implements", "DependsOn"]
            .iter()
            .any(|name| has_component(&doc.components, name));

        // Check incoming refs (other docs reference this one)
        let has_incoming = !graph.references(doc_id, None).is_empty()
            || !graph.implements(doc_id).is_empty()
            || !graph.depends_on(doc_id).is_empty();

        if !has_outgoing && !has_incoming {
            findings.push(Finding::new(
                RuleName::IsolatedDocument,
                Some(doc_id.to_owned()),
                format!("document `{doc_id}` has no incoming or outgoing references"),
                None,
            ));
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_orphan_tags
// ---------------------------------------------------------------------------

/// Scan test files for supersigil tags not declared in any `VerifiedBy` component.
pub fn check_orphan_tags(docs: &[&SpecDocument], test_files: &[PathBuf]) -> Vec<Finding> {
    let all_matches = crate::scan::scan_all_tags(test_files);

    // Collect declared tags from VerifiedBy components
    let mut declared_tags = std::collections::HashSet::new();
    for doc in docs {
        for vb in find_components(&doc.components, "VerifiedBy") {
            if vb.attributes.get("strategy").map(String::as_str) == Some("tag")
                && let Some(tag) = vb.attributes.get("tag")
            {
                declared_tags.insert(tag.clone());
            }
        }
    }

    let mut findings = Vec::new();
    let mut seen_orphans = std::collections::HashSet::new();
    for m in &all_matches {
        if !declared_tags.contains(&m.tag) && seen_orphans.insert(m.tag.clone()) {
            findings.push(Finding::new(
                RuleName::OrphanTestTag,
                None,
                format!(
                    "tag `{}` found in test files but not declared in any VerifiedBy",
                    m.tag
                ),
                None,
            ));
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_verified_by_placement
// ---------------------------------------------------------------------------

/// Check that every `VerifiedBy` component is a direct child of a verifiable
/// component (e.g. `Criterion`). `VerifiedBy` at document root or under a
/// non-verifiable component is a structural error.
pub fn check_verified_by_placement(
    docs: &[&SpecDocument],
    component_defs: &ComponentDefs,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        walk_for_verified_by(doc_id, &doc.components, None, component_defs, &mut findings);
    }
    findings
}

/// Recursively walk the component tree. `parent_name` is the name of the
/// immediate parent component (or `None` at the document root level).
fn walk_for_verified_by(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    parent_name: Option<&str>,
    component_defs: &ComponentDefs,
    findings: &mut Vec<Finding>,
) {
    for comp in components {
        if comp.name == "VerifiedBy" {
            let parent_is_verifiable = parent_name
                .and_then(|name| component_defs.get(name))
                .is_some_and(|def| def.verifiable);

            if !parent_is_verifiable {
                let context = match parent_name {
                    Some(name) => format!("under `{name}`"),
                    None => "at document root".into(),
                };
                findings.push(Finding::new(
                    RuleName::InvalidVerifiedByPlacement,
                    Some(doc_id.to_owned()),
                    format!(
                        "VerifiedBy in `{doc_id}` is placed {context}; \
                         it must be a direct child of a verifiable component (e.g. Criterion)"
                    ),
                    Some(comp.position),
                ));
            }
        }
        // Recurse into children
        walk_for_verified_by(
            doc_id,
            &comp.children,
            Some(&comp.name),
            component_defs,
            findings,
        );
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use supersigil_core::DocumentTypeDef;
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
                required_components: vec!["AcceptanceCriteria".into()],
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
                required_components: vec!["AcceptanceCriteria".into()],
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
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_orphan_tags(&doc_refs, &test_files);
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
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_orphan_tags(&doc_refs, &test_files);
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
            name: "Criterion".into(),
            attributes: std::collections::HashMap::from([("id".into(), "req-1".into())]),
            children: vec![
                make_verified_by_tag("auth:tag1", 11),
                make_verified_by_glob("tests/**/*.rs", 12),
                make_verified_by_tag("auth:tag2", 13),
            ],
            body_text: Some("criterion req-1".into()),
            position: pos(10),
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
}
