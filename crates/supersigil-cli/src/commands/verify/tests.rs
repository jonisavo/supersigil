use super::*;
use crate::format::ColorChoice;
use supersigil_core::SourcePosition;
use supersigil_verify::test_helpers::sample_evidence_summary;
use supersigil_verify::{Finding, FindingDetails};

fn color() -> ColorConfig {
    ColorConfig::resolve(ColorChoice::Always)
}

fn no_color() -> ColorConfig {
    ColorConfig::no_color()
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
    let out = format_terminal(&report, color());
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
    let out_plain = format_terminal(&report, no_color());
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

    let out = format_terminal(&report, color());
    assert!(
        out.contains("✔") && out.contains("Clean"),
        "colored clean report should show Unicode, got: {out}",
    );
    assert!(
        out.contains("3 documents verified"),
        "clean report should show document count, got: {out}",
    );

    let out_plain = format_terminal(&report, no_color());
    assert!(
        out_plain.contains("[ok]") && out_plain.contains("Clean"),
        "plain clean report should show ASCII, got: {out_plain}",
    );
    assert!(
        out_plain.contains("3 documents verified"),
        "plain clean report should show document count, got: {out_plain}",
    );
    assert!(
        !out_plain.contains('—'),
        "plain clean report should not contain em dash, got: {out_plain}",
    );
}

#[test]
fn clean_report_with_evidence_shows_criteria_count() {
    let evidence = sample_evidence_summary();
    let criteria_count = evidence.coverage.len();
    let report = VerificationReport::new(vec![], Summary::from_findings(3, &[]), Some(evidence));

    let out = format_terminal(&report, no_color());
    assert!(
        out.contains("3 documents"),
        "should show document count, got: {out}",
    );
    assert!(
        out.contains(&format!("{criteria_count} criteria")),
        "should show criteria count when evidence present, got: {out}",
    );
}

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

    let out = format_terminal(&report, no_color());
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

    let out = format_terminal(&report, no_color());
    assert!(
        !out.contains("downgraded"),
        "should not show draft hint for clean report, got:\n{out}"
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

    let out = format_terminal(&report, no_color());
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

    let out = format_terminal(&report, no_color());
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
    let report = VerificationReport::new(findings, summary, Some(sample_evidence_summary()));

    let out = format_terminal(&report, no_color());
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

    let out = format_terminal(&report, no_color());
    // Should not contain evidence-related sections
    assert!(
        !out.contains("Evidence"),
        "terminal output should NOT include Evidence section when absent, got:\n{out}",
    );
}

#[test]
fn shows_file_location_when_position_and_path_available() {
    let finding = Finding::new(
        RuleName::MissingVerificationEvidence,
        Some("req/auth".to_string()),
        "criterion AC-1 not covered".to_string(),
        Some(SourcePosition {
            byte_offset: 0,
            line: 42,
            column: 15,
        }),
    )
    .with_details(FindingDetails {
        path: Some("specs/auth/auth.req.md".to_string()),
        ..FindingDetails::default()
    });

    let summary = Summary::from_findings(1, std::slice::from_ref(&finding));
    let report = VerificationReport::new(vec![finding], summary, None);

    let out = format_terminal(&report, no_color());
    assert!(
        out.contains("specs/auth/auth.req.md:42:15"),
        "should show file:line:col location, got:\n{out}",
    );
}

#[test]
fn no_location_line_when_position_missing() {
    let finding = Finding::new(
        RuleName::MissingVerificationEvidence,
        Some("req/auth".to_string()),
        "criterion AC-1 not covered".to_string(),
        None,
    );

    let summary = Summary::from_findings(1, std::slice::from_ref(&finding));
    let report = VerificationReport::new(vec![finding], summary, None);

    let out = format_terminal(&report, no_color());
    // Should not contain any path-like location line
    assert!(
        !out.contains(".md:"),
        "should not show location when position is missing, got:\n{out}",
    );
}
