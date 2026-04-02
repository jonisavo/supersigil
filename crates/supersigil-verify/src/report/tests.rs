use std::collections::HashSet;

use supersigil_core::KNOWN_RULES;

use super::*;

// -----------------------------------------------------------------------
// config_key <-> KNOWN_RULES round-trip
// -----------------------------------------------------------------------

#[test]
fn config_keys_match_known_rules() {
    let built_in_keys: HashSet<&str> = RuleName::ALL.iter().map(|r| r.config_key()).collect();
    let known: HashSet<&str> = KNOWN_RULES.iter().copied().collect();
    assert_eq!(built_in_keys, known);
}

// -----------------------------------------------------------------------
// default_severity for all 13 variants
// -----------------------------------------------------------------------

#[test]
fn default_severity_all_variants() {
    let expected = [
        (RuleName::MissingVerificationEvidence, ReportSeverity::Error),
        (RuleName::MissingTestFiles, ReportSeverity::Error),
        (RuleName::HookFailure, ReportSeverity::Error),
        (RuleName::InvalidVerifiedByPlacement, ReportSeverity::Error),
        (RuleName::InvalidExpectedPlacement, ReportSeverity::Error),
        (RuleName::InvalidCodeBlockCardinality, ReportSeverity::Error),
        (RuleName::InvalidEnvFormat, ReportSeverity::Error),
        (RuleName::ExampleFailed, ReportSeverity::Error),
        (RuleName::IsolatedDocument, ReportSeverity::Off),
        (RuleName::ZeroTagMatches, ReportSeverity::Warning),
        (RuleName::StaleTrackedFiles, ReportSeverity::Warning),
        (RuleName::EmptyTrackedGlob, ReportSeverity::Warning),
        (RuleName::OrphanTestTag, ReportSeverity::Warning),
        (RuleName::InvalidIdPattern, ReportSeverity::Warning),
        (RuleName::StatusInconsistency, ReportSeverity::Warning),
        (RuleName::MissingRequiredComponent, ReportSeverity::Warning),
        (RuleName::HookOutput, ReportSeverity::Warning),
        (RuleName::PluginDiscoveryFailure, ReportSeverity::Warning),
        (RuleName::PluginDiscoveryWarning, ReportSeverity::Warning),
        (RuleName::SequentialIdOrder, ReportSeverity::Warning),
        (RuleName::SequentialIdGap, ReportSeverity::Warning),
        (RuleName::InvalidRationalePlacement, ReportSeverity::Warning),
        (
            RuleName::InvalidAlternativePlacement,
            ReportSeverity::Warning,
        ),
        (RuleName::DuplicateRationale, ReportSeverity::Warning),
        (RuleName::InvalidAlternativeStatus, ReportSeverity::Warning),
        (RuleName::IncompleteDecision, ReportSeverity::Warning),
        (RuleName::OrphanDecision, ReportSeverity::Warning),
        (RuleName::MissingDecisionCoverage, ReportSeverity::Off),
        (RuleName::EmptyProject, ReportSeverity::Warning),
        (RuleName::MultipleExpectedChildren, ReportSeverity::Error),
        (RuleName::InlineExampleWithoutLang, ReportSeverity::Error),
        (RuleName::CodeRefConflict, ReportSeverity::Warning),
    ];
    for (rule, severity) in expected {
        assert_eq!(rule.default_severity(), severity, "for {rule:?}");
    }
}

// -----------------------------------------------------------------------
// ReportSeverity::from(Severity)
// -----------------------------------------------------------------------

#[test]
fn report_severity_from_core() {
    for (input, expected) in [
        (Severity::Off, ReportSeverity::Off),
        (Severity::Warning, ReportSeverity::Warning),
        (Severity::Error, ReportSeverity::Error),
    ] {
        assert_eq!(ReportSeverity::from(input), expected, "for {input:?}");
    }
}

// -----------------------------------------------------------------------
// VerificationReport JSON serialization
// -----------------------------------------------------------------------

#[test]
fn verification_report_includes_position_when_present() {
    let report = VerificationReport::new(
        vec![Finding {
            rule: RuleName::InvalidIdPattern,
            doc_id: None,
            message: "bad pattern".to_string(),
            effective_severity: ReportSeverity::Warning,
            raw_severity: ReportSeverity::Warning,
            position: Some(SourcePosition {
                byte_offset: 42,
                line: 3,
                column: 1,
            }),
            details: None,
        }],
        Summary {
            total_documents: 1,
            error_count: 0,
            warning_count: 1,
            info_count: 0,
        },
        None,
    );

    let json = serde_json::to_string(&report).expect("serialization should succeed");
    assert!(json.contains("\"position\""), "position should be present");
    assert!(json.contains("\"byte_offset\""), "missing byte_offset");
    assert!(json.contains("\"line\""), "missing line");
    assert!(json.contains("\"column\""), "missing column");
    // doc_id is None, so it should be skipped
    assert!(
        !json.contains("\"doc_id\""),
        "doc_id should be skipped when None",
    );
}

// -----------------------------------------------------------------------
// result_status()
// -----------------------------------------------------------------------

#[test]
fn result_status_derives_from_counts() {
    // (error_count, warning_count, info_count, expected)
    let cases = [
        (0, 0, 0, ResultStatus::Clean),
        (0, 0, 5, ResultStatus::Clean),
        (2, 1, 0, ResultStatus::HasErrors),
        (0, 4, 1, ResultStatus::WarningsOnly),
    ];
    for (errors, warnings, infos, expected) in cases {
        let report = VerificationReport::new(
            vec![],
            Summary {
                total_documents: 3,
                error_count: errors,
                warning_count: warnings,
                info_count: infos,
            },
            None,
        );
        assert_eq!(
            report.result_status(),
            expected,
            "for counts ({errors}, {warnings}, {infos})",
        );
    }
}

// -----------------------------------------------------------------------
// format_json
// -----------------------------------------------------------------------

#[test]
fn json_format_roundtrips() {
    let report = VerificationReport::new(
        vec![Finding {
            rule: RuleName::MissingTestFiles,
            doc_id: Some("prop/auth".to_string()),
            message: "no test files found".to_string(),
            effective_severity: ReportSeverity::Error,
            raw_severity: ReportSeverity::Error,
            position: None,
            details: None,
        }],
        Summary {
            total_documents: 1,
            error_count: 1,
            warning_count: 0,
            info_count: 0,
        },
        None,
    );

    let json = format_json(&report);
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("should parse as valid JSON");
    assert_eq!(parsed["summary"]["error_count"], 1);
    assert_eq!(parsed["findings"][0]["rule"], "missing_test_files");
    assert_eq!(parsed["findings"][0]["doc_id"], "prop/auth");
}

// -----------------------------------------------------------------------
// format_markdown
// -----------------------------------------------------------------------

#[test]
fn markdown_format_has_table() {
    let report = VerificationReport::new(
        vec![
            Finding {
                rule: RuleName::MissingVerificationEvidence,
                doc_id: Some("req/auth".to_string()),
                message: "criterion AC-1 not covered".to_string(),
                effective_severity: ReportSeverity::Error,
                raw_severity: ReportSeverity::Error,
                position: None,
                details: None,
            },
            Finding {
                rule: RuleName::ZeroTagMatches,
                doc_id: Some("prop/auth".to_string()),
                message: "tag 'prop:auth' has zero matches".to_string(),
                effective_severity: ReportSeverity::Warning,
                raw_severity: ReportSeverity::Warning,
                position: None,
                details: None,
            },
        ],
        Summary {
            total_documents: 2,
            error_count: 1,
            warning_count: 1,
            info_count: 0,
        },
        None,
    );

    let out = format_markdown(&report);
    assert!(out.contains("# Verification Report"), "should have header",);
    assert!(
        out.contains("| Severity | Document | Rule | Message |"),
        "should have table header, got: {out}",
    );
    assert!(out.contains("error"), "should contain error severity");
    assert!(
        out.contains("missing_verification_evidence"),
        "should contain rule name",
    );
    assert!(out.contains("req/auth"), "should contain doc_id");
    assert!(out.contains("## Summary"), "should have summary section");
}

#[test]
fn markdown_format_clean_report() {
    let report = VerificationReport::new(
        vec![],
        Summary {
            total_documents: 3,
            error_count: 0,
            warning_count: 0,
            info_count: 0,
        },
        None,
    );

    let out = format_markdown(&report);
    assert!(
        out.contains("✔ Clean"),
        "clean report should show clean status, got: {out}",
    );
}

use crate::test_helpers::sample_evidence_summary;

// -----------------------------------------------------------------------
// evidence_summary_serializes_in_json (req-9-1)
// -----------------------------------------------------------------------

#[test]
fn evidence_summary_serializes_in_json() {
    let report = VerificationReport::new(
        vec![],
        Summary {
            total_documents: 1,
            error_count: 0,
            warning_count: 0,
            info_count: 0,
        },
        Some(sample_evidence_summary()),
    );

    let json = format_json(&report);
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("should parse as valid JSON");

    // The evidence_summary key should be present
    assert!(
        parsed.get("evidence_summary").is_some(),
        "JSON should contain evidence_summary key, got: {json}",
    );

    // Drill into the first record
    let records = &parsed["evidence_summary"]["records"];
    assert!(records.is_array(), "records should be an array");
    assert_eq!(records.as_array().unwrap().len(), 2);
    assert_eq!(records[0]["test_name"], "test_login_flow");
    assert_eq!(records[0]["evidence_kind"], "rust-attribute");
    assert_eq!(records[0]["provenance"][0], "plugin:rust");
}

// -----------------------------------------------------------------------
// evidence_summary_absent_when_none
// -----------------------------------------------------------------------

#[test]
fn evidence_summary_absent_when_none() {
    let report = VerificationReport::new(
        vec![],
        Summary {
            total_documents: 1,
            error_count: 0,
            warning_count: 0,
            info_count: 0,
        },
        None,
    );

    let json = format_json(&report);
    assert!(
        !json.contains("evidence_summary"),
        "JSON should NOT contain evidence_summary when None, got: {json}",
    );
}

// -----------------------------------------------------------------------
// markdown_includes_evidence_section (req-9-2)
// -----------------------------------------------------------------------

#[test]
fn markdown_includes_evidence_section() {
    let report = VerificationReport::new(
        vec![Finding {
            rule: RuleName::MissingVerificationEvidence,
            doc_id: Some("req/auth".to_string()),
            message: "criterion req-1 not covered".to_string(),
            effective_severity: ReportSeverity::Error,
            raw_severity: ReportSeverity::Error,
            position: None,
            details: None,
        }],
        Summary {
            total_documents: 1,
            error_count: 1,
            warning_count: 0,
            info_count: 0,
        },
        Some(sample_evidence_summary()),
    );

    let out = format_markdown(&report);
    assert!(
        out.contains("## Evidence"),
        "markdown should include Evidence section when evidence_summary is present, got: {out}",
    );
    assert!(
        out.contains("test_login_flow"),
        "markdown Evidence section should list test names, got: {out}",
    );
    assert!(
        out.contains("rust-attribute"),
        "markdown Evidence section should show evidence kind, got: {out}",
    );
    assert!(
        out.contains("plugin:rust"),
        "markdown Evidence section should show provenance, got: {out}",
    );
}

// -----------------------------------------------------------------------
// markdown_no_evidence_section_when_absent
// -----------------------------------------------------------------------

#[test]
fn markdown_no_evidence_section_when_absent() {
    let report = VerificationReport::new(
        vec![Finding {
            rule: RuleName::MissingVerificationEvidence,
            doc_id: Some("req/auth".to_string()),
            message: "criterion req-1 not covered".to_string(),
            effective_severity: ReportSeverity::Error,
            raw_severity: ReportSeverity::Error,
            position: None,
            details: None,
        }],
        Summary {
            total_documents: 1,
            error_count: 1,
            warning_count: 0,
            info_count: 0,
        },
        None,
    );

    let out = format_markdown(&report);
    assert!(
        !out.contains("## Evidence"),
        "markdown should NOT include Evidence section when evidence_summary is None, got: {out}",
    );
}

// -----------------------------------------------------------------------
// multiple_tests_per_criterion_listed_separately (req-9-3)
// -----------------------------------------------------------------------

#[test]
fn multiple_tests_per_criterion_listed_separately() {
    let evidence = sample_evidence_summary();
    // Confirm our sample has multiple tests targeting the same criterion
    assert_eq!(evidence.coverage.len(), 1);
    assert_eq!(evidence.coverage[0].test_count, 2);

    let report = VerificationReport::new(
        vec![],
        Summary {
            total_documents: 1,
            error_count: 0,
            warning_count: 0,
            info_count: 0,
        },
        Some(evidence),
    );

    let json = format_json(&report);
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("should parse as valid JSON");

    // Each test should appear as a separate record in the JSON
    let records = &parsed["evidence_summary"]["records"];
    let names: Vec<&str> = records
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["test_name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"test_login_flow"),
        "should list test_login_flow separately",
    );
    assert!(
        names.contains(&"test_session_timeout"),
        "should list test_session_timeout separately",
    );

    // Coverage should show the aggregate
    let coverage = &parsed["evidence_summary"]["coverage"];
    assert_eq!(coverage[0]["target"], "req-1");
    assert_eq!(coverage[0]["test_count"], 2);
}

#[test]
fn finding_details_serialize_only_when_present() {
    let with_details = VerificationReport::new(
        vec![
            Finding::new(
                RuleName::PluginDiscoveryFailure,
                None,
                "plugin failed".to_string(),
                None,
            )
            .with_details(FindingDetails {
                plugin: Some("rust".to_string()),
                target_ref: Some("auth/req/login#happy-path-login".to_string()),
                code: Some("invalid_verifies_attribute".to_string()),
                suggestion: Some("Use #[verifies(\"doc#criterion\")]".to_string()),
                ..FindingDetails::default()
            }),
        ],
        Summary {
            total_documents: 1,
            error_count: 0,
            warning_count: 1,
            info_count: 0,
        },
        None,
    );

    let with_json = format_json(&with_details);
    let with_parsed: serde_json::Value =
        serde_json::from_str(&with_json).expect("details JSON should parse");
    assert!(
        with_parsed["findings"][0].get("details").is_some(),
        "expected details in {with_json}",
    );
    assert_eq!(with_parsed["findings"][0]["details"]["plugin"], "rust");
    assert_eq!(
        with_parsed["findings"][0]["details"]["target_ref"],
        "auth/req/login#happy-path-login"
    );

    let without_details = VerificationReport::new(
        vec![Finding::new(
            RuleName::PluginDiscoveryFailure,
            None,
            "plugin failed".to_string(),
            None,
        )],
        Summary {
            total_documents: 1,
            error_count: 0,
            warning_count: 1,
            info_count: 0,
        },
        None,
    );

    let without_json = format_json(&without_details);
    assert!(
        !without_json.contains("\"details\""),
        "details should be skipped when absent: {without_json}",
    );
}
