use super::*;
use supersigil_core::{ACCEPTANCE_CRITERIA, DocumentTypeDef, ParseWarning};
use supersigil_rust::verifies;
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
            required_components: vec![ACCEPTANCE_CRITERIA.to_owned()],
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
            required_components: vec![ACCEPTANCE_CRITERIA.to_owned()],
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

#[test]
fn tasks_doc_with_task_level_implements_is_not_isolated() {
    let mut task = make_task("task-1", 10);
    task.attributes
        .insert("implements".into(), "req/auth#req-1".into());

    let docs = vec![
        make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 5)],
                4,
            )],
        ),
        make_doc("tasks/auth", vec![task]),
    ];
    let graph = build_test_graph(docs);
    let findings = check_isolated(&graph);

    assert!(
        findings
            .iter()
            .all(|finding| finding.doc_id.as_deref() != Some("tasks/auth")),
        "tasks doc with task-level implements should not be isolated, got: {findings:?}",
    );
}

#[test]
fn task_implements_target_is_not_isolated() {
    let mut task = make_task("task-1", 10);
    task.attributes
        .insert("implements".into(), "req/auth#req-1".into());

    let docs = vec![
        make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 5)],
                4,
            )],
        ),
        make_doc("tasks/auth", vec![task]),
    ];
    let graph = build_test_graph(docs);
    let findings = check_isolated(&graph);

    assert!(
        findings
            .iter()
            .all(|finding| finding.doc_id.as_deref() != Some("req/auth")),
        "task implements target should not be isolated, got: {findings:?}",
    );
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
    let tag_matches = crate::scan::scan_all_tags(&test_files);
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_orphan_tags(&doc_refs, &tag_matches);
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
    let tag_matches = crate::scan::scan_all_tags(&test_files);
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_orphan_tags(&doc_refs, &tag_matches);
    assert!(findings.is_empty());
}

// -----------------------------------------------------------------------
// check_duplicate_rationale
// -----------------------------------------------------------------------

#[test]
fn decision_with_zero_rationale_no_finding() {
    let decision = make_decision(vec![], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_duplicate_rationale(&doc_refs);
    assert!(
        findings.is_empty(),
        "Decision with zero Rationale children should produce no findings, got: {findings:?}",
    );
}

#[test]
fn decision_with_one_rationale_no_finding() {
    let decision = make_decision(vec![make_rationale(11)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_duplicate_rationale(&doc_refs);
    assert!(
        findings.is_empty(),
        "Decision with one Rationale child should produce no findings, got: {findings:?}",
    );
}

#[verifies("decision-components/req#req-2-3")]
#[test]
fn decision_with_two_rationale_emits_finding_on_second() {
    let decision = make_decision(vec![make_rationale(11), make_rationale(12)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_duplicate_rationale(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::DuplicateRationale);
    // Finding should be on the second Rationale (line 12)
    assert_eq!(
        findings[0].position.as_ref().map(|p| p.line),
        Some(12),
        "finding should point to the second Rationale",
    );
    assert!(
        findings[0].message.contains("duplicate"),
        "message should mention duplicate, got: {}",
        findings[0].message,
    );
}

#[test]
fn duplicate_rationale_draft_gating() {
    let decision = make_decision(vec![make_rationale(11), make_rationale(12)], 10);
    let docs = vec![make_doc_with_status("adr/logging", "draft", vec![decision])];
    let graph = build_test_graph(docs);
    let config = test_config();
    let options = crate::VerifyOptions::default();
    let ag = crate::artifact_graph::ArtifactGraph::empty(&graph);
    let report =
        crate::verify(&graph, &config, std::path::Path::new("/tmp"), &options, &ag).unwrap();
    for finding in &report.findings {
        if finding.rule == RuleName::DuplicateRationale {
            assert_eq!(
                finding.effective_severity,
                crate::report::ReportSeverity::Info,
                "draft doc duplicate rationale findings should be Info, got {:?}",
                finding.effective_severity,
            );
        }
    }
}

// -----------------------------------------------------------------------
// check_alternative_status
// -----------------------------------------------------------------------

#[test]
fn alternative_with_status_rejected_no_finding() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "rejected", 11)],
        10,
    );
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert!(
        findings.is_empty(),
        "Alternative with status='rejected' should produce no findings, got: {findings:?}",
    );
}

#[test]
fn alternative_with_status_deferred_no_finding() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "deferred", 11)],
        10,
    );
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert!(
        findings.is_empty(),
        "Alternative with status='deferred' should produce no findings, got: {findings:?}",
    );
}

#[test]
fn alternative_with_status_superseded_no_finding() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "superseded", 11)],
        10,
    );
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert!(
        findings.is_empty(),
        "Alternative with status='superseded' should produce no findings, got: {findings:?}",
    );
}

#[verifies("decision-components/req#req-3-2")]
#[test]
fn alternative_with_status_accepted_emits_finding() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "accepted", 11)],
        10,
    );
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidAlternativeStatus);
    assert!(
        findings[0].message.contains("accepted"),
        "message should mention the invalid status, got: {}",
        findings[0].message,
    );
}

#[test]
fn alternative_with_empty_status_emits_finding() {
    let decision = make_decision(vec![make_alternative_with_status("alt-1", "", 11)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InvalidAlternativeStatus);
}

#[test]
fn alternative_without_status_attribute_no_finding() {
    // Alternative without any status attribute should not fire this rule
    let decision = make_decision(vec![make_alternative("alt-1", 11)], 10);
    let docs = [make_doc("adr/logging", vec![decision])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_alternative_status(&doc_refs);
    assert!(
        findings.is_empty(),
        "Alternative without status attribute should produce no findings, got: {findings:?}",
    );
}

#[verifies("decision-components/req#req-3-3")]
#[test]
fn alternative_status_default_severity_is_warning() {
    assert_eq!(
        RuleName::InvalidAlternativeStatus.default_severity(),
        crate::report::ReportSeverity::Warning,
    );
}

#[test]
fn alternative_status_draft_gating() {
    let decision = make_decision(
        vec![make_alternative_with_status("alt-1", "accepted", 11)],
        10,
    );
    let docs = vec![make_doc_with_status("adr/logging", "draft", vec![decision])];
    let graph = build_test_graph(docs);
    let config = test_config();
    let options = crate::VerifyOptions::default();
    let ag = crate::artifact_graph::ArtifactGraph::empty(&graph);
    let report =
        crate::verify(&graph, &config, std::path::Path::new("/tmp"), &options, &ag).unwrap();
    for finding in &report.findings {
        if finding.rule == RuleName::InvalidAlternativeStatus {
            assert_eq!(
                finding.effective_severity,
                crate::report::ReportSeverity::Info,
                "draft doc alternative status findings should be Info, got {:?}",
                finding.effective_severity,
            );
        }
    }
}

// -----------------------------------------------------------------------
// check_inline_example_lang
// -----------------------------------------------------------------------

#[test]
fn example_with_fence_lang_and_no_attr_is_valid() {
    // Code block has lang: Some("sh") — no error regardless of attribute
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![supersigil_core::CodeBlock {
            lang: Some("sh".into()),
            content: "echo hello".into(),
            content_offset: 0,
            content_end_offset: "echo hello".len(),
            span_kind: SpanKind::RefFence,
        }],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_inline_example_lang(&doc_refs);
    assert!(
        findings.is_empty(),
        "Example with fence lang should be valid, got: {findings:?}",
    );
}

#[test]
fn example_with_inline_code_and_lang_attr_is_valid() {
    // Code block has lang: None but the Example has a `lang` attribute
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::from([("lang".into(), "python".into())]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![supersigil_core::CodeBlock {
            lang: None,
            content: "print('hello')".into(),
            content_offset: 0,
            content_end_offset: "print('hello')".len(),
            span_kind: SpanKind::XmlInline,
        }],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_inline_example_lang(&doc_refs);
    assert!(
        findings.is_empty(),
        "Example with inline code and lang attr should be valid, got: {findings:?}",
    );
}

#[test]
fn example_with_inline_code_and_no_lang_attr_emits_finding() {
    // Code block has lang: None and no `lang` attribute — error
    let example = supersigil_core::ExtractedComponent {
        name: EXAMPLE.to_owned(),
        attributes: std::collections::HashMap::new(),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![supersigil_core::CodeBlock {
            lang: None,
            content: "print('hello')".into(),
            content_offset: 0,
            content_end_offset: "print('hello')".len(),
            span_kind: SpanKind::XmlInline,
        }],
        position: pos(5),
        end_position: pos(5),
    };
    let docs = [make_doc("ex/doc", vec![example])];
    let doc_refs: Vec<&_> = docs.iter().collect();
    let findings = check_inline_example_lang(&doc_refs);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::InlineExampleWithoutLang);
    assert!(
        findings[0].message.contains("lang"),
        "message should mention lang, got: {}",
        findings[0].message,
    );
}

// -----------------------------------------------------------------------
// check_code_ref_conflicts
// -----------------------------------------------------------------------

#[test]
fn orphan_code_ref_warning_surfaces_as_finding() {
    let mut doc = make_doc_with_status("test/doc", "draft", vec![]);
    doc.warnings.push(ParseWarning::OrphanCodeRef {
        path: doc.path.clone(),
        target: "no-such-test".into(),
        content_offset: 0,
    });
    let findings = check_code_ref_conflicts(&[&doc]);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::CodeRefConflict);
    assert_eq!(findings[0].doc_id.as_deref(), Some("test/doc"));
}

#[test]
fn no_warnings_emits_no_findings() {
    let doc = make_doc_with_status("test/doc", "draft", vec![]);
    let findings = check_code_ref_conflicts(&[&doc]);
    assert!(findings.is_empty());
}

#[test]
fn multiple_warnings_emit_multiple_findings() {
    let mut doc = make_doc_with_status("test/doc", "draft", vec![]);
    doc.warnings.push(ParseWarning::DuplicateCodeRef {
        path: doc.path.clone(),
        target: "dup-test".into(),
    });
    doc.warnings.push(ParseWarning::DualSourceConflict {
        path: doc.path.clone(),
        target: "conflict-test".into(),
        content_offset: 0,
    });
    let findings = check_code_ref_conflicts(&[&doc]);
    assert_eq!(findings.len(), 2);
    assert!(findings.iter().all(|f| f.rule == RuleName::CodeRefConflict));
}
