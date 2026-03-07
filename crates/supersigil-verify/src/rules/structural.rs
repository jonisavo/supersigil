use std::path::PathBuf;

use supersigil_core::{Config, DocumentGraph, SpecDocument};

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
                findings.push(Finding {
                    rule: RuleName::MissingRequiredComponent,
                    doc_id: Some(doc_id.to_owned()),
                    message: format!(
                        "document `{doc_id}` (type `{doc_type}`) is missing required component `{required}`"
                    ),
                    effective_severity: RuleName::MissingRequiredComponent.default_severity(),
                    raw_severity: RuleName::MissingRequiredComponent.default_severity(),
                    position: None,
                });
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
            findings.push(Finding {
                rule: RuleName::InvalidIdPattern,
                doc_id: Some(doc_id.to_owned()),
                message: format!("document ID `{doc_id}` does not match pattern `{pattern}`"),
                effective_severity: RuleName::InvalidIdPattern.default_severity(),
                raw_severity: RuleName::InvalidIdPattern.default_severity(),
                position: None,
            });
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
        let has_outgoing = ["Validates", "Implements", "Illustrates", "DependsOn"]
            .iter()
            .any(|name| has_component(&doc.components, name));

        // Check incoming refs (other docs reference this one)
        let has_incoming = !graph.validates(doc_id, None).is_empty()
            || !graph.illustrates(doc_id, None).is_empty()
            || !graph.implements(doc_id).is_empty()
            || !graph.depends_on(doc_id).is_empty();

        if !has_outgoing && !has_incoming {
            findings.push(Finding {
                rule: RuleName::IsolatedDocument,
                doc_id: Some(doc_id.to_owned()),
                message: format!("document `{doc_id}` has no incoming or outgoing references"),
                effective_severity: RuleName::IsolatedDocument.default_severity(),
                raw_severity: RuleName::IsolatedDocument.default_severity(),
                position: None,
            });
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
            findings.push(Finding {
                rule: RuleName::OrphanTestTag,
                doc_id: None,
                message: format!(
                    "tag `{}` found in test files but not declared in any VerifiedBy",
                    m.tag
                ),
                effective_severity: RuleName::OrphanTestTag.default_severity(),
                raw_severity: RuleName::OrphanTestTag.default_severity(),
                position: None,
            });
        }
    }
    findings
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
            "requirement".into(),
            DocumentTypeDef {
                status: vec!["draft".into()],
                required_components: vec!["AcceptanceCriteria".into()],
                description: None,
            },
        );
        let docs = vec![make_doc_typed(
            "req/auth",
            "requirement",
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
            "requirement".into(),
            DocumentTypeDef {
                status: vec!["draft".into()],
                required_components: vec!["AcceptanceCriteria".into()],
                description: None,
            },
        );
        let docs = vec![make_doc_typed(
            "req/auth",
            "requirement",
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
}
