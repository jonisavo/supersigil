use super::*;
use crate::format::ColorChoice;
use std::collections::HashMap;
use supersigil_core::{EXAMPLE, ExtractedComponent};
use supersigil_rust::verifies;
use supersigil_verify::Finding;
use supersigil_verify::test_helpers::{
    build_test_graph, make_acceptance_criteria, make_criterion, make_doc, pos,
};

fn color() -> ColorConfig {
    ColorConfig::resolve(ColorChoice::Always)
}

fn no_color() -> ColorConfig {
    ColorConfig::no_color()
}

fn progress_snapshot(states: &[(&str, ExampleProgressState)]) -> ExampleProgressSnapshot {
    ExampleProgressSnapshot {
        entries: states
            .iter()
            .map(|(example_id, state)| ExampleProgressEntry {
                doc_id: "examples/req".to_string(),
                example_id: (*example_id).to_string(),
                runner: "cargo-test".to_string(),
                state: *state,
            })
            .collect(),
    }
}

#[test]
fn groups_by_document() {
    let findings = vec![
        Finding::new(
            RuleName::MissingVerificationEvidence,
            Some("req/auth".to_string()),
            "criterion AC-1 not covered".to_string(),
            None,
        ),
        Finding::new(
            RuleName::OrphanTestTag,
            None,
            "tag 'foo:bar' has no matching document".to_string(),
            None,
        ),
    ];
    let summary = Summary::from_findings(2, &findings);
    let report = VerificationReport::new(findings, summary, None);

    // With color: Unicode symbols + ANSI
    let out = format_terminal(&report, None, color());
    assert!(out.contains("req/auth"), "should contain doc_id header");
    assert!(out.contains("global"), "should contain global header");
    assert!(out.contains("✖"), "should contain error symbol");
    assert!(out.contains("⚠"), "should contain warning symbol");
    assert!(
        out.contains("[missing_verification_evidence]"),
        "should contain rule name",
    );
    assert!(
        out.contains("error(s)") && out.contains("warning(s)") && out.contains("documents"),
        "should contain summary line, got: {out}",
    );

    // Without color: ASCII symbols, no Unicode
    let out_plain = format_terminal(&report, None, no_color());
    assert!(
        out_plain.contains("[err]"),
        "no-color should use ASCII error, got: {out_plain}",
    );
    assert!(
        out_plain.contains("[warn]"),
        "no-color should use ASCII warning, got: {out_plain}",
    );
    assert!(
        !out_plain.contains('✖') && !out_plain.contains('⚠'),
        "no-color should not contain Unicode symbols, got: {out_plain}",
    );
}

#[test]
fn clean_report() {
    let report = VerificationReport::new(vec![], Summary::from_findings(3, &[]), None);

    let out = format_terminal(&report, None, color());
    assert!(
        out.contains("✔") && out.contains("Clean"),
        "colored clean report should show Unicode, got: {out}",
    );

    let out_plain = format_terminal(&report, None, no_color());
    assert!(
        out_plain.contains("[ok]") && out_plain.contains("Clean"),
        "plain clean report should show ASCII, got: {out_plain}",
    );
}

#[test]
fn clean_report_with_examples_shows_example_summary() {
    let report = VerificationReport::new(vec![], Summary::from_findings(1, &[]), None);
    let summary = ExampleExecutionSummary {
        passed: 1,
        failures: Vec::new(),
    };

    let out = format_terminal(&report, Some(&summary), no_color());
    assert!(out.contains("Examples: 1 passed"), "got:\n{out}");
    assert!(out.contains("Clean"), "got:\n{out}");
}

#[test]
fn clean_report_with_failed_examples_shows_non_blocking_summary() {
    let report = VerificationReport::new(vec![], Summary::from_findings(1, &[]), None);
    let summary = ExampleExecutionSummary {
        passed: 0,
        failures: vec![ExampleFailure {
            doc_id: "examples/req".to_string(),
            example_id: "body-mismatch".to_string(),
            runner: "sh".to_string(),
            details: vec![ExampleFailureDetail::Message("boom".to_string())],
        }],
    };

    let out = format_terminal(&report, Some(&summary), no_color());
    assert!(out.contains("No blocking findings"), "got:\n{out}");
    assert!(!out.contains("Clean: no findings"), "got:\n{out}");
}

#[verifies("executable-examples/req#req-4-8")]
#[test]
fn draft_gating_hint_shown_when_findings_suppressed() {
    let mut finding = Finding::new(
        RuleName::MissingVerificationEvidence,
        Some("req/auth".to_string()),
        "criterion AC-1 not covered".to_string(),
        None,
    );
    // Simulate draft gating: raw stays Error, effective downgraded to Info
    finding.effective_severity = ReportSeverity::Info;

    let report = VerificationReport::new(
        vec![finding.clone()],
        Summary::from_findings(1, &[finding]),
        None,
    );

    let out = format_terminal(&report, None, no_color());
    assert!(
        out.contains("downgraded to info"),
        "should show draft gating hint, got:\n{out}"
    );
    assert!(
        out.contains("status: draft"),
        "hint should mention draft, got:\n{out}"
    );
}

#[test]
fn draft_gating_hint_not_shown_when_no_suppression() {
    let report = VerificationReport::new(vec![], Summary::from_findings(1, &[]), None);

    let out = format_terminal(&report, None, no_color());
    assert!(
        !out.contains("downgraded"),
        "should not show draft hint for clean report, got:\n{out}"
    );
}

#[test]
fn terminal_lists_failed_examples_before_findings() {
    let findings = vec![Finding::new(
        RuleName::ExampleFailed,
        Some("examples/req".to_string()),
        "example 'cargo-fail' (runner: cargo-test) failed".to_string(),
        None,
    )];
    let summary_counts = Summary::from_findings(1, &findings);
    let report = VerificationReport::new(findings, summary_counts, None);
    let summary = ExampleExecutionSummary {
        passed: 1,
        failures: vec![ExampleFailure {
            doc_id: "examples/req".to_string(),
            example_id: "cargo-fail".to_string(),
            runner: "cargo-test".to_string(),
            details: vec![ExampleFailureDetail::Match {
                check: "Status".to_string(),
                expected: "0".to_string(),
                actual: "101".to_string(),
            }],
        }],
    };

    let out = format_terminal(&report, Some(&summary), no_color());
    assert!(out.contains("Examples: 1 passed, 1 failed"), "got:\n{out}");
    assert!(out.contains("Failed examples:"), "got:\n{out}");
    assert!(out.contains("examples/req::cargo-fail"), "got:\n{out}");
    assert!(out.contains("[Status]"), "got:\n{out}");
    assert!(out.contains("expected: 0"), "got:\n{out}");
    assert!(out.contains("actual: 101"), "got:\n{out}");
    assert!(out.contains("[example_failed]"), "got:\n{out}");
}

#[test]
fn live_spinner_omits_redundant_pass_only_example_summary() {
    let clean = ExampleExecutionSummary {
        passed: 2,
        failures: Vec::new(),
    };
    let failed = ExampleExecutionSummary {
        passed: 1,
        failures: vec![ExampleFailure {
            doc_id: "examples/req".to_string(),
            example_id: "failing-example".to_string(),
            runner: "sh".to_string(),
            details: vec![ExampleFailureDetail::Message("boom".to_string())],
        }],
    };

    assert!(!should_render_example_summary(
        &clean,
        ExampleProgressDisplay::LiveSpinner,
    ));
    assert!(should_render_example_summary(
        &clean,
        ExampleProgressDisplay::Stream,
    ));
    assert!(should_render_example_summary(
        &failed,
        ExampleProgressDisplay::LiveSpinner,
    ));
}

#[verifies("executable-examples/req#req-4-7")]
#[test]
fn progress_snapshot_renders_spinner_frames_for_running_examples() {
    let snapshot = progress_snapshot(&[
        ("queued", ExampleProgressState::Queued),
        ("running", ExampleProgressState::Running),
        ("passed", ExampleProgressState::Passed),
        ("failed", ExampleProgressState::Failed),
    ]);

    let frame_zero = render_progress_snapshot(&snapshot, no_color(), 0).join("\n");
    let frame_one = render_progress_snapshot(&snapshot, no_color(), 1).join("\n");
    assert!(
        frame_zero.contains("Executing 4 examples (2/4 complete)"),
        "got:\n{frame_zero}"
    );
    assert!(
        frame_zero.contains("examples/req::queued (cargo-test) queued"),
        "got:\n{frame_zero}",
    );
    assert!(
        frame_zero.contains("examples/req::running (cargo-test) running"),
        "got:\n{frame_zero}",
    );
    assert!(
        frame_zero.contains("examples/req::passed (cargo-test) passed"),
        "got:\n{frame_zero}",
    );
    assert!(
        frame_zero.contains("examples/req::failed (cargo-test) failed"),
        "got:\n{frame_zero}",
    );
    assert!(
        !frame_zero.contains("[#####-----]"),
        "running examples should render a spinner, not a static half bar:\n{frame_zero}",
    );
    assert_ne!(
        frame_zero, frame_one,
        "running spinner frames should change between ticks"
    );
}

#[test]
fn collapses_repeated_rules() {
    let findings: Vec<Finding> = (0..10)
        .map(|i| {
            Finding::new(
                RuleName::MissingVerificationEvidence,
                Some("req/auth".to_string()),
                format!("criterion `req-{i}` has no validating property"),
                None,
            )
        })
        .collect();
    let summary = Summary::from_findings(1, &findings);
    let report = VerificationReport::new(findings, summary, None);

    let out = format_terminal(&report, None, no_color());
    assert!(
        out.contains("[missing_verification_evidence] 10 findings"),
        "should show collapsed count, got:\n{out}",
    );
    assert!(
        out.contains("criterion `req-0`"),
        "should show first preview, got:\n{out}",
    );
    assert!(
        out.contains("criterion `req-1`"),
        "should show second preview, got:\n{out}",
    );
    assert!(
        out.contains("and 8 more"),
        "should show remaining count, got:\n{out}",
    );
    assert!(
        !out.contains("criterion `req-9`"),
        "should not show all messages, got:\n{out}",
    );
    assert!(
        out.contains("--format json"),
        "should hint about json format, got:\n{out}",
    );
}

#[test]
fn does_not_collapse_small_groups() {
    let findings: Vec<Finding> = (0..3)
        .map(|i| {
            Finding::new(
                RuleName::MissingVerificationEvidence,
                Some("req/auth".to_string()),
                format!("criterion `req-{i}` has no validating property"),
                None,
            )
        })
        .collect();
    let summary = Summary::from_findings(1, &findings);
    let report = VerificationReport::new(findings, summary, None);

    let out = format_terminal(&report, None, no_color());
    assert!(out.contains("criterion `req-0`"), "got:\n{out}");
    assert!(out.contains("criterion `req-1`"), "got:\n{out}");
    assert!(out.contains("criterion `req-2`"), "got:\n{out}");
    assert!(!out.contains("findings"), "got:\n{out}");
    assert!(!out.contains("more"), "got:\n{out}");
    assert!(!out.contains("--format json"), "got:\n{out}");
}

#[test]
fn terminal_omits_evidence_summary_when_present() {
    let findings = vec![Finding::new(
        RuleName::MissingVerificationEvidence,
        Some("req/auth".to_string()),
        "criterion req-2 not covered".to_string(),
        None,
    )];
    let summary = Summary::from_findings(1, &findings);
    let report = VerificationReport::new(
        findings,
        summary,
        Some(supersigil_verify::test_helpers::sample_evidence_summary()),
    );

    let out = format_terminal(&report, None, no_color());
    assert!(
        !out.contains("test_login_flow"),
        "terminal output should omit evidence test names even when evidence_summary is present, got:\n{out}",
    );
    assert!(
        !out.contains("rust-attribute"),
        "terminal output should omit evidence kinds even when evidence_summary is present, got:\n{out}",
    );
}

#[test]
fn terminal_no_evidence_when_absent() {
    let findings = vec![Finding::new(
        RuleName::MissingVerificationEvidence,
        Some("req/auth".to_string()),
        "criterion req-2 not covered".to_string(),
        None,
    )];
    let summary = Summary::from_findings(1, &findings);
    let report = VerificationReport::new(findings, summary, None);

    let out = format_terminal(&report, None, no_color());
    // Should not contain evidence-related sections
    assert!(
        !out.contains("Evidence"),
        "terminal output should NOT include Evidence section when absent, got:\n{out}",
    );
}

#[test]
fn example_pending_count_deduplicates_target_refs() {
    let docs = vec![make_doc(
        "examples/req",
        vec![
            make_acceptance_criteria(vec![make_criterion("crit-1", 5)], 4),
            ExtractedComponent {
                name: EXAMPLE.to_string(),
                attributes: HashMap::from([(
                    "verifies".to_string(),
                    "examples/req#crit-1".to_string(),
                )]),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: pos(10),
                end_position: pos(10),
            },
        ],
    )];
    let graph = build_test_graph(docs);

    let mut first = Finding::new(
        RuleName::MissingVerificationEvidence,
        Some("examples/req".to_string()),
        "criterion `crit-1` has no validating property".to_string(),
        None,
    );
    first.details = Some(Box::new(supersigil_verify::FindingDetails {
        target_ref: Some("examples/req#crit-1".to_string()),
        ..Default::default()
    }));

    let mut second = first.clone();
    second.message = "criterion `crit-1` is still uncovered".to_string();

    let report = VerificationReport::new(
        vec![first.clone(), second],
        Summary::from_findings(1, &[first.clone()]),
        None,
    );

    assert_eq!(count_example_pending_criteria(&report, &graph), 1);
}
