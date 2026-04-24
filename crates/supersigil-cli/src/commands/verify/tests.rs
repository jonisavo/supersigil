use super::*;
use crate::format::ColorChoice;
use std::path::PathBuf;
use supersigil_core::SourcePosition;
use supersigil_verify::test_helpers::sample_evidence_summary;
use supersigil_verify::{AffectedDocument, Finding, FindingDetails};

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
    let out = format_terminal(&report, color(), false);
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
    let out_plain = format_terminal(&report, no_color(), false);
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

    let out = format_terminal(&report, color(), false);
    assert!(
        out.contains("✔") && out.contains("Clean"),
        "colored clean report should show Unicode, got: {out}",
    );
    assert!(
        out.contains("3 documents verified"),
        "clean report should show document count, got: {out}",
    );

    let out_plain = format_terminal(&report, no_color(), false);
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

    let out = format_terminal(&report, no_color(), false);
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

    let out = format_terminal(&report, no_color(), false);
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

    let out = format_terminal(&report, no_color(), false);
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

    let out = format_terminal(&report, no_color(), false);
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
        out.contains("--detail full"),
        "should hint about --detail full, got:\n{out}",
    );
}

#[test]
fn detail_full_disables_collapsing() {
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

    let out = format_terminal(&report, no_color(), true);
    assert!(
        out.contains("criterion `req-9`"),
        "detail_full should show all findings, got:\n{out}",
    );
    assert!(
        !out.contains("more"),
        "detail_full should not collapse, got:\n{out}",
    );
    assert!(
        !out.contains("--detail full"),
        "detail_full should not show hint, got:\n{out}",
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

    let out = format_terminal(&report, no_color(), false);
    assert!(out.contains("criterion `req-0`"), "got:\n{out}");
    assert!(out.contains("criterion `req-1`"), "got:\n{out}");
    assert!(out.contains("criterion `req-2`"), "got:\n{out}");
    assert!(!out.contains("findings"), "got:\n{out}");
    assert!(!out.contains("more"), "got:\n{out}");
    assert!(!out.contains("--detail full"), "got:\n{out}");
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

    let out = format_terminal(&report, no_color(), false);
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

    let out = format_terminal(&report, no_color(), false);
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

    let out = format_terminal(&report, no_color(), false);
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

    let out = format_terminal(&report, no_color(), false);
    // Should not contain any path-like location line
    assert!(
        !out.contains(".md:"),
        "should not show location when position is missing, got:\n{out}",
    );
}

#[test]
fn rule_breakdown_shown_after_summary() {
    let findings = vec![
        Finding::new(
            RuleName::MissingVerificationEvidence,
            Some("req/auth".to_string()),
            "criterion AC-1 not covered".to_string(),
            None,
        ),
        Finding::new(
            RuleName::OrphanTestTag,
            Some("req/auth".to_string()),
            "tag has no matching criterion".to_string(),
            None,
        ),
        Finding::new(
            RuleName::EmptyTrackedGlob,
            Some("req/auth".to_string()),
            "glob matched nothing".to_string(),
            None,
        ),
    ];
    let summary = Summary::from_findings(42, &findings);
    let report = VerificationReport::new(findings, summary, None);

    let out = format_terminal(&report, no_color(), false);
    assert!(
        out.contains("1 empty_tracked_glob, 1 missing_verification_evidence, 1 orphan_test_tag"),
        "should show rule breakdown line, got:\n{out}",
    );
}

#[test]
fn rule_breakdown_sorted_by_count_desc_then_alpha() {
    let findings = vec![
        Finding::new(
            RuleName::OrphanTestTag,
            Some("req/auth".to_string()),
            "orphan 1".to_string(),
            None,
        ),
        Finding::new(
            RuleName::OrphanTestTag,
            Some("req/login".to_string()),
            "orphan 2".to_string(),
            None,
        ),
        Finding::new(
            RuleName::EmptyTrackedGlob,
            Some("req/auth".to_string()),
            "glob 1".to_string(),
            None,
        ),
        Finding::new(
            RuleName::MissingVerificationEvidence,
            Some("req/auth".to_string()),
            "missing 1".to_string(),
            None,
        ),
    ];
    let summary = Summary::from_findings(10, &findings);
    let report = VerificationReport::new(findings, summary, None);

    let out = format_terminal(&report, no_color(), false);
    assert!(
        out.contains("2 orphan_test_tag, 1 empty_tracked_glob, 1 missing_verification_evidence"),
        "should sort by count desc then alpha, got:\n{out}",
    );
}

#[test]
fn rule_breakdown_skips_off_severity() {
    let mut off_finding = Finding::new(
        RuleName::OrphanTestTag,
        Some("req/auth".to_string()),
        "tag orphaned".to_string(),
        None,
    );
    off_finding.effective_severity = ReportSeverity::Off;

    let active_finding = Finding::new(
        RuleName::MissingVerificationEvidence,
        Some("req/auth".to_string()),
        "criterion AC-1 not covered".to_string(),
        None,
    );

    let findings = vec![off_finding, active_finding];
    let summary = Summary::from_findings(5, &findings);
    let report = VerificationReport::new(findings, summary, None);

    let out = format_terminal(&report, no_color(), false);
    assert!(
        out.contains("1 missing_verification_evidence"),
        "should show active rule, got:\n{out}",
    );
    assert!(
        !out.contains("orphan_test_tag"),
        "should not show Off-severity rule, got:\n{out}",
    );
}

#[test]
fn rule_breakdown_not_shown_for_clean_report() {
    let report = VerificationReport::new(vec![], Summary::from_findings(3, &[]), None);

    let out = format_terminal(&report, no_color(), false);
    assert!(
        !out.contains("missing_verification_evidence")
            && !out.contains("orphan_test_tag")
            && !out.contains("empty_tracked_glob"),
        "clean report should not have rule breakdown, got:\n{out}",
    );
}

#[test]
fn rule_breakdown_uses_hint_styling() {
    let findings = vec![Finding::new(
        RuleName::MissingVerificationEvidence,
        Some("req/auth".to_string()),
        "criterion AC-1 not covered".to_string(),
        None,
    )];
    let summary = Summary::from_findings(1, &findings);
    let report = VerificationReport::new(findings, summary, None);

    let out_color = format_terminal(&report, color(), false);
    let out_plain = format_terminal(&report, no_color(), false);

    assert!(
        out_plain.contains("1 missing_verification_evidence"),
        "plain output should have breakdown, got:\n{out_plain}",
    );
    assert!(
        out_color.contains("1 missing_verification_evidence"),
        "color output should have breakdown, got:\n{out_color}",
    );
}

// ---------------------------------------------------------------------------
// Timing summary tests
// ---------------------------------------------------------------------------

#[test]
fn timing_summary_uses_milliseconds_for_subsecond_durations() {
    use output::terminal::format_timing_summary;

    let timings = PhaseTimings {
        doc_count: 5,
        parse: Duration::from_millis(120),
        evidence: Duration::from_millis(340),
        rules: Duration::from_millis(50),
    };

    let out = format_timing_summary(&timings, no_color());
    assert!(
        out.contains("in 510ms"),
        "should include total timing in milliseconds, got:\n{out}"
    );
    assert!(
        out.contains("(parse: 120ms, check: 340ms, report: 50ms)"),
        "should include phase timings in milliseconds, got:\n{out}"
    );
}

#[test]
fn timing_summary_includes_all_phases() {
    use output::terminal::format_timing_summary;

    let timings = PhaseTimings {
        doc_count: 5,
        parse: Duration::from_millis(120),
        evidence: Duration::from_millis(340),
        rules: Duration::from_millis(50),
    };

    let out = format_timing_summary(&timings, no_color());
    assert!(
        out.contains("5 documents"),
        "should include document count, got:\n{out}"
    );
    assert!(
        out.contains("120ms"),
        "should include parse timing, got:\n{out}"
    );
    assert!(
        out.contains("check:"),
        "should include check label, got:\n{out}"
    );
    assert!(
        out.contains("report:"),
        "should include report label, got:\n{out}"
    );
}

#[test]
fn timing_summary_shows_total_elapsed() {
    use output::terminal::format_timing_summary;

    let timings = PhaseTimings {
        doc_count: 10,
        parse: Duration::from_millis(100),
        evidence: Duration::from_millis(200),
        rules: Duration::from_millis(50),
    };

    let out = format_timing_summary(&timings, no_color());
    assert!(
        out.contains("in 350ms"),
        "should include total time, got:\n{out}"
    );
    assert!(
        out.contains("Verified"),
        "should start with 'Verified', got:\n{out}"
    );
}

#[test]
fn timing_summary_keeps_seconds_for_longer_durations() {
    use output::terminal::format_timing_summary;

    let timings = PhaseTimings {
        doc_count: 10,
        parse: Duration::from_millis(100),
        evidence: Duration::from_millis(1_200),
        rules: Duration::from_millis(50),
    };

    let out = format_timing_summary(&timings, no_color());
    assert!(
        out.contains("in 1.4s"),
        "should keep total timing in seconds for longer runs, got:\n{out}"
    );
    assert!(
        out.contains("check: 1.2s"),
        "should keep longer phase timings in seconds, got:\n{out}"
    );
}

#[test]
fn timing_summary_singular_document() {
    use output::terminal::format_timing_summary;

    let timings = PhaseTimings {
        doc_count: 1,
        parse: Duration::from_millis(50),
        evidence: Duration::from_millis(100),
        rules: Duration::from_millis(30),
    };

    let out = format_timing_summary(&timings, no_color());
    assert!(
        out.contains("1 document "),
        "should use singular 'document' for count 1, got:\n{out}"
    );
    assert!(
        !out.contains("documents"),
        "should not use plural for count 1, got:\n{out}"
    );
}

// ---------------------------------------------------------------------------
// Affected note tests
// ---------------------------------------------------------------------------

#[test]
fn affected_sentence_includes_since_ref_when_present() {
    let summary = AffectedSummary {
        doc_count: 3,
        changed_file_count: 5,
    };

    let sentence = affected_sentence(&summary, Some("origin/main"));

    assert!(
        sentence.contains("3 documents"),
        "should include document count, got:\n{sentence}",
    );
    assert!(
        sentence.contains("5 changed files"),
        "should include changed file count, got:\n{sentence}",
    );
    assert!(
        sentence.contains("since origin/main"),
        "should include since ref, got:\n{sentence}",
    );
}

#[test]
fn affected_sentence_omits_since_ref_when_absent() {
    let summary = AffectedSummary {
        doc_count: 1,
        changed_file_count: 1,
    };

    let sentence = affected_sentence(&summary, None);

    assert!(
        sentence.contains("1 document affected by 1 changed file."),
        "should use singular nouns without since-ref wording, got:\n{sentence}",
    );
}

#[test]
fn github_comment_for_clean_run_surfaces_review_for_drift_and_details() {
    let report = VerificationReport::new(vec![], Summary::from_findings(3, &[]), None);
    let affected = AffectedContext {
        documents: vec![
            AffectedDocument {
                id: "graph-explorer/design".to_string(),
                path: PathBuf::from("specs/graph-explorer/graph-explorer.design.md"),
                matched_globs: vec!["website/src/components/explore/*".to_string()],
                changed_files: vec![
                    PathBuf::from("website/src/components/explore/graph-explorer.js"),
                    PathBuf::from("website/src/components/explore/styles.css"),
                ],
                transitive_from: None,
            },
            AffectedDocument {
                id: "graph-explorer/adr".to_string(),
                path: PathBuf::from("specs/graph-explorer/graph-explorer.adr.md"),
                matched_globs: vec![],
                changed_files: vec![],
                transitive_from: Some("graph-explorer/design".to_string()),
            },
        ],
        changed_file_count: 2,
    };

    let out = format_github_comment(&report, Some(&affected), Some("origin/main"));

    assert!(out.contains("## Verification"), "got:\n{out}");
    assert!(out.contains("![status: clean]"), "got:\n{out}");
    assert!(out.contains("![errors: 0]"), "got:\n{out}");
    assert!(out.contains("![warnings: 0]"), "got:\n{out}");
    assert!(out.contains("![affected docs: 2]"), "got:\n{out}");
    assert!(out.contains("### Review for drift"), "got:\n{out}");
    assert!(out.contains("graph-explorer/design"), "got:\n{out}");
    assert!(
        out.contains("1 additional doc is transitively affected."),
        "got:\n{out}"
    );
    assert!(
        out.contains("<summary>Full verification report (0 errors, 0 warnings)</summary>"),
        "got:\n{out}"
    );
    assert!(
        out.contains("<summary>Full affected breakdown (2 docs: 1 direct, 1 transitive)</summary>"),
        "got:\n{out}"
    );
    assert!(
        out.contains("changed: website/src/components/explore/graph-explorer.js"),
        "got:\n{out}"
    );
}

#[test]
fn github_comment_for_errors_surfaces_needs_attention() {
    let findings = vec![
        Finding::new(
            RuleName::MissingVerificationEvidence,
            Some("req/auth".to_string()),
            "criterion AC-1 not covered".to_string(),
            None,
        ),
        Finding::new(
            RuleName::EmptyTrackedGlob,
            Some("design/auth".to_string()),
            "glob `src/**/*.rs` matched nothing".to_string(),
            None,
        ),
    ];
    let summary = Summary::from_findings(2, &findings);
    let report = VerificationReport::new(findings, summary, None);

    let out = format_github_comment(&report, None, None);

    assert!(out.contains("![status: failing]"), "got:\n{out}");
    assert!(out.contains("### Needs attention"), "got:\n{out}");
    assert!(
        out.contains("Verification failed. Address the errors below"),
        "got:\n{out}"
    );
    assert!(
        out.contains(
            "**error** `req/auth` [missing_verification_evidence] criterion AC-1 not covered"
        ),
        "got:\n{out}"
    );
    assert!(
        out.contains("<summary>Full verification report (1 error, 1 warning)</summary>"),
        "got:\n{out}"
    );
}

#[test]
fn github_comment_with_empty_affected_docs_omits_drift_review_section() {
    let report = VerificationReport::new(vec![], Summary::from_findings(3, &[]), None);
    let affected = AffectedContext {
        documents: vec![],
        changed_file_count: 0,
    };

    let out = format_github_comment(&report, Some(&affected), Some("origin/main"));

    assert!(out.contains("## Verification"), "got:\n{out}");
    assert!(out.contains("![affected docs: 0]"), "got:\n{out}");
    assert!(out.contains("Verification passed."), "got:\n{out}");
    assert!(
        !out.contains("Review the affected docs below"),
        "got:\n{out}"
    );
    assert!(!out.contains("### Review for drift"), "got:\n{out}");
    assert!(
        !out.contains("No affected docs were detected"),
        "got:\n{out}"
    );
    assert!(!out.contains("Full affected breakdown"), "got:\n{out}");
}

// ---------------------------------------------------------------------------
// Did-you-mean suggestion tests
// ---------------------------------------------------------------------------

#[test]
fn shows_did_you_mean_when_suggestion_present() {
    let finding = Finding::new(
        RuleName::BrokenRef,
        Some("tasks/auth".to_string()),
        "broken ref `auth/reqs`".to_string(),
        None,
    )
    .with_suggestion("auth/req".to_string());

    let summary = Summary::from_findings(1, std::slice::from_ref(&finding));
    let report = VerificationReport::new(vec![finding], summary, None);

    let out = format_terminal(&report, no_color(), false);
    assert!(
        out.contains("did you mean 'auth/req'?"),
        "should show 'did you mean' hint, got:\n{out}",
    );
}

#[test]
fn no_did_you_mean_when_suggestion_absent() {
    let finding = Finding::new(
        RuleName::BrokenRef,
        Some("tasks/auth".to_string()),
        "broken ref `completely/different`".to_string(),
        None,
    );

    let summary = Summary::from_findings(1, std::slice::from_ref(&finding));
    let report = VerificationReport::new(vec![finding], summary, None);

    let out = format_terminal(&report, no_color(), false);
    assert!(
        !out.contains("did you mean"),
        "should not show 'did you mean' when suggestion is absent, got:\n{out}",
    );
}
