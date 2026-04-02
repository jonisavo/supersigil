use supersigil_core::SourcePosition;
use supersigil_rust_macros::verifies;

use super::*;

fn dummy_source_pos(line: usize, column: usize) -> SourcePosition {
    SourcePosition {
        byte_offset: 0,
        line,
        column,
    }
}

/// Helper: deserialise the `data` field of a diagnostic back into
/// [`DiagnosticData`], panicking if it is absent or malformed.
fn extract_data(diag: &Diagnostic) -> DiagnosticData {
    let value = diag.data.as_ref().expect("diagnostic should have data");
    serde_json::from_value(value.clone()).expect("data should deserialize to DiagnosticData")
}

// -----------------------------------------------------------------------
// graph_error_to_diagnostic
// -----------------------------------------------------------------------

#[test]
fn duplicate_id_produces_one_diagnostic_per_path() {
    let path = std::path::PathBuf::from("/tmp/spec-a.md");
    let err = GraphError::DuplicateId {
        id: "REQ-001".into(),
        paths: vec![path.clone()],
    };

    let pairs = graph_error_to_diagnostic(&err);
    assert_eq!(pairs.len(), 1);
    let (url, diag) = &pairs[0];
    assert_eq!(url.to_file_path().unwrap(), path);
    assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
    assert!(diag.message.contains("REQ-001"));
}

#[test]
fn duplicate_id_with_multiple_paths_produces_multiple_diagnostics() {
    let err = GraphError::DuplicateId {
        id: "REQ-002".into(),
        paths: vec![
            std::path::PathBuf::from("/tmp/a.md"),
            std::path::PathBuf::from("/tmp/b.md"),
        ],
    };

    let pairs = graph_error_to_diagnostic(&err);
    assert_eq!(pairs.len(), 2);
    assert!(
        pairs
            .iter()
            .all(|(_, d)| d.severity == Some(DiagnosticSeverity::ERROR))
    );
}

#[test]
fn broken_ref_without_lookup_returns_empty() {
    let err = GraphError::BrokenRef {
        doc_id: "REQ-001".into(),
        ref_str: "REQ-999".into(),
        reason: "not found".into(),
        position: dummy_source_pos(5, 3),
    };

    let pairs = graph_error_to_diagnostic(&err);
    assert!(
        pairs.is_empty(),
        "BrokenRef needs a doc-id lookup; without it returns empty"
    );
}

#[test]
fn task_dependency_cycle_without_lookup_returns_empty() {
    let err = GraphError::TaskDependencyCycle {
        doc_id: "TASK-001".into(),
        cycle: vec!["TASK-001".into(), "TASK-002".into()],
    };

    let pairs = graph_error_to_diagnostic(&err);
    assert!(pairs.is_empty());
}

// -----------------------------------------------------------------------
// graph_error_to_diagnostic_with_lookup
// -----------------------------------------------------------------------

#[test]
fn broken_ref_with_lookup_produces_diagnostic() {
    let path = std::path::PathBuf::from("/tmp/req-001.md");
    let err = GraphError::BrokenRef {
        doc_id: "REQ-001".into(),
        ref_str: "REQ-999".into(),
        reason: "not found".into(),
        position: dummy_source_pos(10, 5),
    };

    let pairs =
        graph_error_to_diagnostic_with_lookup(&err, |id| (id == "REQ-001").then(|| path.clone()));

    assert_eq!(pairs.len(), 1);
    let (url, diag) = &pairs[0];
    assert_eq!(url.to_file_path().unwrap(), path);
    assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
    assert!(diag.message.contains("REQ-999"));
    assert!(diag.message.contains("not found"));
    // line 10, col 5 → 0-based: (9, 4)
    assert_eq!(diag.range.start.line, 9);
    assert_eq!(diag.range.start.character, 4);
}

#[test]
fn broken_ref_with_unknown_doc_id_returns_empty() {
    let err = GraphError::BrokenRef {
        doc_id: "REQ-UNKNOWN".into(),
        ref_str: "REQ-999".into(),
        reason: "not found".into(),
        position: dummy_source_pos(1, 1),
    };

    let pairs = graph_error_to_diagnostic_with_lookup(&err, |_| None);
    assert!(pairs.is_empty());
}

#[test]
fn duplicate_component_id_with_lookup_produces_one_diagnostic_per_position() {
    let path = std::path::PathBuf::from("/tmp/req-comp.md");
    let err = GraphError::DuplicateComponentId {
        doc_id: "REQ-001".into(),
        component_id: "crit-1".into(),
        positions: vec![dummy_source_pos(3, 1), dummy_source_pos(7, 1)],
    };

    let pairs =
        graph_error_to_diagnostic_with_lookup(&err, |id| (id == "REQ-001").then(|| path.clone()));

    assert_eq!(pairs.len(), 2);
    assert!(
        pairs
            .iter()
            .all(|(_, d)| d.severity == Some(DiagnosticSeverity::ERROR))
    );
    assert!(pairs.iter().all(|(_, d)| d.message.contains("crit-1")));
}

// -----------------------------------------------------------------------
// finding_to_diagnostic
// -----------------------------------------------------------------------

#[test]
fn finding_with_position_and_details_path_but_no_line_uses_position() {
    // When attach_doc_paths sets details.path but not details.line/column,
    // the diagnostic should use finding.position, not default to (0, 0).
    let path = std::path::PathBuf::from("/tmp/test-doc.md");

    // Write a minimal file so source_to_lsp_from_file can read it.
    let _ = std::fs::write(
        &path,
        "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\n<Decision>\n",
    );

    let mut finding = Finding::new(
        supersigil_verify::RuleName::IncompleteDecision,
        Some("test/doc".into()),
        "Decision has no Rationale".into(),
        Some(dummy_source_pos(10, 1)),
    );
    // Simulate what attach_doc_paths does: set details.path without line/column
    finding.details = Some(Box::new(supersigil_verify::FindingDetails {
        path: Some(path.to_string_lossy().into_owned()),
        ..Default::default()
    }));

    let result = finding_to_diagnostic(&finding, |_| Some(path.clone()));

    let (_, diag) = result.expect("should produce a diagnostic");
    // Should use finding.position (line 10) → LSP line 9, not (0, 0)
    assert_eq!(
        diag.range.start.line, 9,
        "diagnostic should point to line 10 (0-based: 9), not line 0"
    );
    assert_eq!(diag.range.start.character, 0);

    // Cleanup
    let _ = std::fs::remove_file(&path);
}

#[test]
fn finding_with_details_path_and_line_uses_details_line() {
    // When details has both path AND line, those should be used (existing behavior).
    let path = std::path::PathBuf::from("/tmp/test-doc2.md");
    let _ = std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n");

    let mut finding = Finding::new(
        supersigil_verify::RuleName::IncompleteDecision,
        Some("test/doc".into()),
        "Decision has no Rationale".into(),
        Some(dummy_source_pos(10, 1)), // finding.position says line 10
    );
    // details says line 5 — details.line should win
    finding.details = Some(Box::new(supersigil_verify::FindingDetails {
        path: Some(path.to_string_lossy().into_owned()),
        line: Some(5),
        column: Some(3),
        ..Default::default()
    }));

    let result = finding_to_diagnostic(&finding, |_| Some(path.clone()));

    let (_, diag) = result.expect("should produce a diagnostic");
    // details.line = 5 → LSP line 4
    assert_eq!(
        diag.range.start.line, 4,
        "should use details.line (5 → 0-based 4)"
    );

    let _ = std::fs::remove_file(&path);
}

// -----------------------------------------------------------------------
// OrphanCodeRef / DualSourceConflict position computation
// -----------------------------------------------------------------------

#[test]
fn orphan_code_ref_diagnostic_uses_correct_line_from_byte_offset() {
    let path = std::path::PathBuf::from("/tmp/orphan-ref.md");
    // content_offset 12 → "line1\nline2\n" → line 3, column 1
    let buffer = "line1\nline2\norphan ref here\n";
    let err = ParseError::OrphanCodeRef {
        path: path.clone(),
        target: "some-target".into(),
        content_offset: 12,
    };

    let result = parse_error_to_diagnostic(&err, Some(buffer));
    let (_, diag) = result.expect("should produce a diagnostic");
    // Line 3 → 0-based LSP line 2
    assert_eq!(
        diag.range.start.line, 2,
        "orphan code ref should point to line 3 (0-based: 2), not line 0"
    );
    assert_eq!(
        diag.range.start.character, 0,
        "orphan code ref should point to column 1 (0-based: 0)"
    );
}

#[test]
fn dual_source_conflict_diagnostic_uses_correct_line_from_byte_offset() {
    let path = std::path::PathBuf::from("/tmp/dual-source.md");
    // content_offset 18 → "line1\nline2\nline3\n" → line 4, column 1
    let buffer = "line1\nline2\nline3\ndual source here\n";
    let err = ParseError::DualSourceConflict {
        path: path.clone(),
        target: "some-target".into(),
        content_offset: 18,
    };

    let result = parse_error_to_diagnostic(&err, Some(buffer));
    let (_, diag) = result.expect("should produce a diagnostic");
    // Line 4 → 0-based LSP line 3
    assert_eq!(
        diag.range.start.line, 3,
        "dual source conflict should point to line 4 (0-based: 3), not line 0"
    );
}

#[test]
fn task_dependency_cycle_with_lookup_produces_diagnostic() {
    let path = std::path::PathBuf::from("/tmp/tasks.md");
    let err = GraphError::TaskDependencyCycle {
        doc_id: "TASKS".into(),
        cycle: vec!["TASK-A".into(), "TASK-B".into()],
    };

    let pairs =
        graph_error_to_diagnostic_with_lookup(&err, |id| (id == "TASKS").then(|| path.clone()));

    assert_eq!(pairs.len(), 1);
    let (_, diag) = &pairs[0];
    assert!(diag.message.contains("TASK-A"));
    assert!(diag.message.contains("TASK-B"));
}

// -----------------------------------------------------------------------
// DiagnosticData on parse errors (req-1-2)
// -----------------------------------------------------------------------

#[verifies("lsp-code-actions/req#req-1-2")]
#[test]
fn parse_error_missing_attribute_attaches_data() {
    let path = std::path::PathBuf::from("/tmp/parse-attr.md");
    let err = ParseError::MissingRequiredAttribute {
        path: path.clone(),
        component: "Criterion".into(),
        attribute: "id".into(),
        position: SourcePosition {
            byte_offset: 0,
            line: 1,
            column: 1,
        },
    };

    let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
    let data = extract_data(&diag);

    assert!(matches!(
        data.source,
        DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute)
    ));
    assert!(data.doc_id.is_none());
    match &data.context {
        ActionContext::MissingAttribute {
            component,
            attribute,
        } => {
            assert_eq!(component, "Criterion");
            assert_eq!(attribute, "id");
        }
        other => panic!("expected MissingAttribute, got {other:?}"),
    }
}

#[test]
fn parse_error_xml_syntax_attaches_data() {
    let path = std::path::PathBuf::from("/tmp/parse-xml.md");
    let err = ParseError::XmlSyntaxError {
        path: path.clone(),
        line: 3,
        column: 5,
        message: "unexpected EOF".into(),
    };

    let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
    let data = extract_data(&diag);

    assert!(matches!(
        data.source,
        DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError)
    ));
    assert!(matches!(data.context, ActionContext::None));
}

#[test]
fn parse_error_unclosed_frontmatter_attaches_data() {
    let path = std::path::PathBuf::from("/tmp/parse-fm.md");
    let err = ParseError::UnclosedFrontMatter { path: path.clone() };

    let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
    let data = extract_data(&diag);

    assert!(matches!(
        data.source,
        DiagnosticSource::Parse(ParseDiagnosticKind::UnclosedFrontmatter)
    ));
}

#[test]
fn parse_error_duplicate_code_ref_attaches_data() {
    let path = std::path::PathBuf::from("/tmp/parse-dup.md");
    let err = ParseError::DuplicateCodeRef {
        path: path.clone(),
        target: "comp-1".into(),
    };

    let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
    let data = extract_data(&diag);

    assert!(matches!(
        data.source,
        DiagnosticSource::Parse(ParseDiagnosticKind::DuplicateCodeRef)
    ));
}

#[test]
fn parse_error_other_variants_attach_other_kind() {
    let path = std::path::PathBuf::from("/tmp/parse-id.md");
    let err = ParseError::MissingId { path: path.clone() };

    let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
    let data = extract_data(&diag);

    assert!(matches!(
        data.source,
        DiagnosticSource::Parse(ParseDiagnosticKind::Other)
    ));
}

// -----------------------------------------------------------------------
// DiagnosticData on parse warnings (req-1-2)
// -----------------------------------------------------------------------

#[test]
fn parse_warning_duplicate_code_ref_attaches_data() {
    let path = std::path::PathBuf::from("/tmp/warn-dup.md");
    let warn = ParseWarning::DuplicateCodeRef {
        path: path.clone(),
        target: "comp-1".into(),
    };

    let (_, diag) = parse_warning_to_diagnostic(&warn, None).unwrap();
    let data = extract_data(&diag);

    assert!(matches!(
        data.source,
        DiagnosticSource::Parse(ParseDiagnosticKind::DuplicateCodeRef)
    ));
    assert!(matches!(data.context, ActionContext::None));
}

// -----------------------------------------------------------------------
// DiagnosticData on graph errors (req-1-3)
// -----------------------------------------------------------------------

#[verifies("lsp-code-actions/req#req-1-3")]
#[test]
fn graph_error_duplicate_id_attaches_data() {
    let err = GraphError::DuplicateId {
        id: "REQ-001".into(),
        paths: vec![
            std::path::PathBuf::from("/tmp/a.md"),
            std::path::PathBuf::from("/tmp/b.md"),
        ],
    };

    let pairs = graph_error_to_diagnostic(&err);
    assert_eq!(pairs.len(), 2);

    let data_0 = extract_data(&pairs[0].1);
    assert!(matches!(
        data_0.source,
        DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateDocumentId)
    ));
    assert_eq!(data_0.doc_id.as_deref(), Some("REQ-001"));
    match &data_0.context {
        ActionContext::DuplicateId { id, other_path } => {
            assert_eq!(id, "REQ-001");
            assert!(other_path.contains("b.md"));
        }
        other => panic!("expected DuplicateId, got {other:?}"),
    }

    // Second diagnostic should point back at first path
    let data_1 = extract_data(&pairs[1].1);
    match &data_1.context {
        ActionContext::DuplicateId { id, other_path } => {
            assert_eq!(id, "REQ-001");
            assert!(other_path.contains("a.md"));
        }
        other => panic!("expected DuplicateId, got {other:?}"),
    }
}

#[test]
fn graph_error_broken_ref_with_lookup_attaches_data() {
    let path = std::path::PathBuf::from("/tmp/broken-ref.md");
    let err = GraphError::BrokenRef {
        doc_id: "REQ-001".into(),
        ref_str: "REQ-999".into(),
        reason: "not found".into(),
        position: dummy_source_pos(5, 3),
    };

    let pairs =
        graph_error_to_diagnostic_with_lookup(&err, |id| (id == "REQ-001").then(|| path.clone()));
    assert_eq!(pairs.len(), 1);

    let data = extract_data(&pairs[0].1);
    assert!(matches!(
        data.source,
        DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef)
    ));
    assert_eq!(data.doc_id.as_deref(), Some("REQ-001"));
    match &data.context {
        ActionContext::BrokenRef { target_ref } => {
            assert_eq!(target_ref, "REQ-999");
        }
        other => panic!("expected BrokenRef, got {other:?}"),
    }
}

#[test]
fn graph_error_duplicate_component_id_with_lookup_attaches_data() {
    let path = std::path::PathBuf::from("/tmp/dup-comp.md");
    let err = GraphError::DuplicateComponentId {
        doc_id: "REQ-001".into(),
        component_id: "crit-1".into(),
        positions: vec![dummy_source_pos(3, 1)],
    };

    let pairs =
        graph_error_to_diagnostic_with_lookup(&err, |id| (id == "REQ-001").then(|| path.clone()));
    assert_eq!(pairs.len(), 1);

    let data = extract_data(&pairs[0].1);
    assert!(matches!(
        data.source,
        DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateComponentId)
    ));
    assert_eq!(data.doc_id.as_deref(), Some("REQ-001"));
    match &data.context {
        ActionContext::DuplicateId { id, .. } => {
            assert_eq!(id, "crit-1");
        }
        other => panic!("expected DuplicateId, got {other:?}"),
    }
}

#[test]
fn graph_error_task_dependency_cycle_with_lookup_attaches_data() {
    let path = std::path::PathBuf::from("/tmp/cycle.md");
    let err = GraphError::TaskDependencyCycle {
        doc_id: "TASKS".into(),
        cycle: vec!["A".into(), "B".into()],
    };

    let pairs =
        graph_error_to_diagnostic_with_lookup(&err, |id| (id == "TASKS").then(|| path.clone()));
    assert_eq!(pairs.len(), 1);

    let data = extract_data(&pairs[0].1);
    assert!(matches!(
        data.source,
        DiagnosticSource::Graph(GraphDiagnosticKind::DependencyCycle)
    ));
    assert_eq!(data.doc_id.as_deref(), Some("TASKS"));
}

#[test]
fn graph_error_document_dependency_cycle_with_lookup_attaches_data() {
    let path_a = std::path::PathBuf::from("/tmp/cycle-a.md");
    let path_b = std::path::PathBuf::from("/tmp/cycle-b.md");
    let err = GraphError::DocumentDependencyCycle {
        cycle: vec!["DOC-A".into(), "DOC-B".into()],
    };

    let pairs = graph_error_to_diagnostic_with_lookup(&err, |id| match id {
        "DOC-A" => Some(path_a.clone()),
        "DOC-B" => Some(path_b.clone()),
        _ => None,
    });
    assert_eq!(pairs.len(), 2);

    for (_, diag) in &pairs {
        let data = extract_data(diag);
        assert!(matches!(
            data.source,
            DiagnosticSource::Graph(GraphDiagnosticKind::DependencyCycle)
        ));
    }
}

// -----------------------------------------------------------------------
// DiagnosticData on verify findings (req-1-1)
// -----------------------------------------------------------------------

#[verifies("lsp-code-actions/req#req-1-1")]
#[test]
fn finding_diagnostic_attaches_verify_data() {
    let path = std::path::PathBuf::from("/tmp/finding-data.md");

    let finding = Finding::new(
        RuleName::MissingRequiredComponent,
        Some("REQ-001".into()),
        "missing required component".into(),
        None,
    );

    let result = finding_to_diagnostic(&finding, |_| Some(path.clone()));
    let (_, diag) = result.expect("should produce a diagnostic");
    let data = extract_data(&diag);

    assert!(matches!(
        data.source,
        DiagnosticSource::Verify(RuleName::MissingRequiredComponent)
    ));
    assert_eq!(data.doc_id.as_deref(), Some("REQ-001"));
    assert!(matches!(data.context, ActionContext::None));
}

// -----------------------------------------------------------------------
// Round-trip serialization (req-1-4)
// -----------------------------------------------------------------------

#[verifies("lsp-code-actions/req#req-1-4")]
#[test]
fn diagnostic_data_round_trips_through_json() {
    let original = DiagnosticData {
        source: DiagnosticSource::Verify(RuleName::IncompleteDecision),
        doc_id: Some("DEC-001".into()),
        context: ActionContext::IncompleteDecision {
            decision_id: "DEC-001".into(),
        },
    };

    let json = serde_json::to_value(&original).unwrap();
    let deserialized: DiagnosticData = serde_json::from_value(json).unwrap();

    // Verify source
    assert!(matches!(
        deserialized.source,
        DiagnosticSource::Verify(RuleName::IncompleteDecision)
    ));
    assert_eq!(deserialized.doc_id.as_deref(), Some("DEC-001"));

    // Verify context
    match deserialized.context {
        ActionContext::IncompleteDecision { decision_id } => {
            assert_eq!(decision_id, "DEC-001");
        }
        other => panic!("expected IncompleteDecision, got {other:?}"),
    }
}

#[test]
fn diagnostic_data_round_trips_parse_source() {
    let original = DiagnosticData {
        source: DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute),
        doc_id: None,
        context: ActionContext::MissingAttribute {
            component: "Criterion".into(),
            attribute: "id".into(),
        },
    };

    let json = serde_json::to_value(&original).unwrap();
    let deserialized: DiagnosticData = serde_json::from_value(json).unwrap();

    assert!(matches!(
        deserialized.source,
        DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute)
    ));
    match deserialized.context {
        ActionContext::MissingAttribute {
            component,
            attribute,
        } => {
            assert_eq!(component, "Criterion");
            assert_eq!(attribute, "id");
        }
        other => panic!("expected MissingAttribute, got {other:?}"),
    }
}

#[test]
fn diagnostic_data_round_trips_graph_broken_ref() {
    let original = DiagnosticData {
        source: DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef),
        doc_id: Some("REQ-001".into()),
        context: ActionContext::BrokenRef {
            target_ref: "REQ-999".into(),
        },
    };

    let json = serde_json::to_value(&original).unwrap();
    let deserialized: DiagnosticData = serde_json::from_value(json).unwrap();

    assert!(matches!(
        deserialized.source,
        DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef)
    ));
    match deserialized.context {
        ActionContext::BrokenRef { target_ref } => {
            assert_eq!(target_ref, "REQ-999");
        }
        other => panic!("expected BrokenRef, got {other:?}"),
    }
}

#[test]
fn diagnostic_data_round_trips_all_action_context_variants() {
    let contexts = vec![
        ActionContext::None,
        ActionContext::BrokenRef {
            target_ref: "REQ-X".into(),
        },
        ActionContext::MissingAttribute {
            component: "C".into(),
            attribute: "a".into(),
        },
        ActionContext::DuplicateId {
            id: "id".into(),
            other_path: "/tmp/x.md".into(),
        },
        ActionContext::IncompleteDecision {
            decision_id: "d".into(),
        },
        ActionContext::MissingComponent {
            component: "Criterion".into(),
            parent_id: "REQ-001".into(),
        },
        ActionContext::OrphanDecision {
            decision_id: "DEC-001".into(),
        },
        ActionContext::InvalidPlacement {
            component: "VerifiedBy".into(),
            expected_parent: "Criterion".into(),
        },
        ActionContext::SequentialIdGap {
            component_type: "Criterion".into(),
        },
    ];

    for ctx in contexts {
        let data = DiagnosticData {
            source: DiagnosticSource::Verify(RuleName::EmptyProject),
            doc_id: None,
            context: ctx,
        };
        let json = serde_json::to_value(&data).unwrap();
        let _: DiagnosticData =
            serde_json::from_value(json).expect("every ActionContext variant should round-trip");
    }
}

// -----------------------------------------------------------------------
// enrich_finding_context
// -----------------------------------------------------------------------

#[expect(
    clippy::too_many_lines,
    clippy::type_complexity,
    reason = "table-driven test with many cases and closure-based assertions"
)]
#[test]
fn enrich_finding_context_table() {
    let cases: Vec<(&str, RuleName, &str, &str, Box<dyn Fn(ActionContext)>)> = vec![
        (
            "missing_required_component",
            RuleName::MissingRequiredComponent,
            "auth/req",
            "document `auth/req` (type `requirements`) is missing required component `AcceptanceCriteria`",
            Box::new(|ctx| match ctx {
                ActionContext::MissingComponent {
                    component,
                    parent_id,
                } => {
                    assert_eq!(component, "AcceptanceCriteria");
                    assert_eq!(parent_id, "auth/req");
                }
                other => panic!("expected MissingComponent, got {other:?}"),
            }),
        ),
        (
            "orphan_decision",
            RuleName::OrphanDecision,
            "adr/logging",
            "Decision `use-postgres` in `adr/logging` is orphan: no outward connections and not referenced by any other component",
            Box::new(|ctx| match ctx {
                ActionContext::OrphanDecision { decision_id } => {
                    assert_eq!(decision_id, "use-postgres");
                }
                other => panic!("expected OrphanDecision, got {other:?}"),
            }),
        ),
        (
            "invalid_rationale_placement",
            RuleName::InvalidRationalePlacement,
            "adr/design",
            "Rationale in `adr/design` is placed at document root; it must be a direct child of Decision",
            Box::new(|ctx| match ctx {
                ActionContext::InvalidPlacement {
                    component,
                    expected_parent,
                } => {
                    assert_eq!(component, "Rationale");
                    assert_eq!(expected_parent, "Decision");
                }
                other => panic!("expected InvalidPlacement, got {other:?}"),
            }),
        ),
        (
            "invalid_alternative_placement",
            RuleName::InvalidAlternativePlacement,
            "adr/design",
            "Alternative in `adr/design` is placed at document root; it must be a direct child of Decision",
            Box::new(|ctx| match ctx {
                ActionContext::InvalidPlacement {
                    component,
                    expected_parent,
                } => {
                    assert_eq!(component, "Alternative");
                    assert_eq!(expected_parent, "Decision");
                }
                other => panic!("expected InvalidPlacement, got {other:?}"),
            }),
        ),
        (
            "invalid_expected_placement",
            RuleName::InvalidExpectedPlacement,
            "auth/req",
            "Expected in `auth/req` is placed at document root; it must be a direct child of Example",
            Box::new(|ctx| match ctx {
                ActionContext::InvalidPlacement {
                    component,
                    expected_parent,
                } => {
                    assert_eq!(component, "Expected");
                    assert_eq!(expected_parent, "Example");
                }
                other => panic!("expected InvalidPlacement, got {other:?}"),
            }),
        ),
        (
            "sequential_id_gap",
            RuleName::SequentialIdGap,
            "my-doc",
            "gap in sequence: `task-2` is missing (between `task-1` and `task-3` in document `my-doc`)",
            Box::new(|ctx| match ctx {
                ActionContext::SequentialIdGap { component_type } => {
                    assert_eq!(component_type, "task");
                }
                other => panic!("expected SequentialIdGap, got {other:?}"),
            }),
        ),
        (
            "sequential_id_order",
            RuleName::SequentialIdOrder,
            "my-doc",
            "`task-1` is declared after `task-2` in document `my-doc`",
            Box::new(|ctx| match ctx {
                ActionContext::SequentialIdGap { component_type } => {
                    assert_eq!(component_type, "task");
                }
                other => panic!("expected SequentialIdGap, got {other:?}"),
            }),
        ),
        (
            "incomplete_decision",
            RuleName::IncompleteDecision,
            "adr/logging",
            "Decision in `adr/logging` has no Rationale child; every Decision should include a Rationale",
            Box::new(|ctx| match ctx {
                ActionContext::IncompleteDecision { decision_id } => {
                    assert_eq!(decision_id, "adr/logging");
                }
                other => panic!("expected IncompleteDecision, got {other:?}"),
            }),
        ),
    ];

    for (_label, rule, doc_id, message, check) in &cases {
        let finding = Finding::new(*rule, Some((*doc_id).into()), (*message).into(), None);
        let ctx = enrich_finding_context(&finding);
        check(ctx);
    }
}

#[test]
fn enrich_missing_required_component_no_backtick() {
    let finding = Finding::new(
        RuleName::MissingRequiredComponent,
        Some("auth/req".into()),
        "some unusual message".into(),
        None,
    );
    assert!(matches!(
        enrich_finding_context(&finding),
        ActionContext::None
    ));
}

#[test]
fn enrich_unrelated_rule_returns_none() {
    let finding = Finding::new(
        RuleName::EmptyProject,
        None,
        "project is empty".into(),
        None,
    );
    assert!(matches!(
        enrich_finding_context(&finding),
        ActionContext::None
    ));
}
