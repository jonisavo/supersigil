use std::io::{self, IsTerminal, Write};
use std::path::Path;

mod output;

use output::{
    ExampleExecutionSummary, ExampleProgressDisplay, ExampleProgressReporter,
    count_example_pending_criteria, format_terminal, remediation_hints,
    should_render_example_summary, update_snapshots,
};
#[cfg(test)]
use output::{
    ExampleFailure, ExampleFailureDetail, ExampleProgressEntry, ExampleProgressSnapshot,
    ExampleProgressState, render_progress_snapshot,
};

use supersigil_verify::{
    ExampleSkipReason, Finding, ReportSeverity, ResultStatus, VerifyOptions, collect_examples,
    execute_examples, execute_examples_with_progress, finalize_example_findings, format_json,
    format_markdown, resolve_finding_severities, results_to_evidence, results_to_findings,
};
#[cfg(test)]
use supersigil_verify::{RuleName, Summary, VerificationReport};

use crate::commands::{VerifyArgs, VerifyFormat};
use crate::error::CliError;
use crate::format::{self, ColorConfig, ExitStatus};
use crate::loader;
use crate::plugins;

/// Run the `verify` command: cross-document verification.
///
/// Orchestrates the multi-phase pipeline:
/// 1. Plugin evidence + structural checks
/// 2. Example execution (gated by structural errors or --skip-examples)
/// 3. Coverage check against (possibly enriched) artifact graph
/// 4. Hooks
///
/// # Errors
///
/// Returns `CliError` if loading fails or verification encounters a fatal error.
#[allow(
    clippy::too_many_lines,
    reason = "multi-phase verify pipeline: structural + examples + coverage + hooks"
)]
pub fn run(
    args: &VerifyArgs,
    config_path: &Path,
    color: ColorConfig,
) -> Result<ExitStatus, CliError> {
    let (mut config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);

    // CLI -j/--parallelism overrides config file value.
    if let Some(p) = args.parallelism {
        config.examples.parallelism = p.max(1);
    }

    let options = VerifyOptions {
        project: args.project.clone(),
        since_ref: args.since.clone(),
        committed_only: args.committed_only,
        use_merge_base: args.merge_base,
    };

    // Collect document IDs for project filtering. When a project filter is
    // supplied, only findings whose doc_id belongs to the selected project
    // (or is None) are reported. The full workspace graph remains available
    // for non-isolated resolution.
    let doc_ids = supersigil_verify::scoped_doc_ids(&graph, &options);

    // -- Phase 1: Plugin evidence + structural checks --
    let inputs = supersigil_verify::VerifyInputs::resolve(&config, project_root);

    let (artifact_graph, mut plugin_findings) = plugins::build_evidence(
        &config,
        &graph,
        project_root,
        options.project.as_deref(),
        &inputs,
    );

    let mut structural_findings =
        supersigil_verify::verify_structural(&graph, &config, project_root, &options, &inputs)?;

    resolve_finding_severities(&mut structural_findings, &graph, &config);

    let has_structural_errors = structural_findings
        .iter()
        .any(|f| f.effective_severity == ReportSeverity::Error);

    // -- Phase 2: Example execution (gated by structural errors or --skip-examples) --
    let mut example_findings: Vec<Finding> = Vec::new();
    let mut example_evidence = Vec::new();
    let mut example_summary = None;
    let mut example_progress_display = None;

    let example_skip_reason = if has_structural_errors {
        Some(ExampleSkipReason::StructuralErrors)
    } else if args.skip_examples {
        Some(ExampleSkipReason::ExplicitSkip)
    } else {
        // Collect and execute examples
        let specs = collect_examples(&graph, &config.examples);
        if !specs.is_empty() {
            let results = if matches!(args.format, VerifyFormat::Terminal) {
                let display = if io::stdout().is_terminal() {
                    ExampleProgressDisplay::LiveSpinner
                } else {
                    ExampleProgressDisplay::Stream
                };
                example_progress_display = Some(display);
                let mut reporter = ExampleProgressReporter::new(&specs, color, display);
                reporter.initialize()?;
                let results = execute_examples_with_progress(
                    &specs,
                    project_root,
                    &config.examples,
                    Some(&reporter),
                );
                reporter.finish()?;
                results
            } else {
                execute_examples(&specs, project_root, &config.examples)
            };
            example_summary = Some(ExampleExecutionSummary::from_results(&results));

            // Handle --update-snapshots
            if args.update_snapshots {
                update_snapshots(&results, !matches!(args.format, VerifyFormat::Json));
            }

            example_evidence = results_to_evidence(&results);
            example_findings = results_to_findings(&results);
        }
        None
    };

    // Merge example evidence into artifact graph if we have any
    let final_artifact_graph = if example_evidence.is_empty() {
        artifact_graph
    } else {
        // Extract existing evidence and append example evidence, then rebuild
        let mut all_plugin_evidence: Vec<_> = artifact_graph.evidence.into_iter().collect();
        all_plugin_evidence.extend(example_evidence);
        supersigil_verify::build_artifact_graph(&graph, vec![], all_plugin_evidence)
    };

    // -- Phase 3: Coverage --
    let mut coverage_findings = supersigil_verify::verify_coverage(&graph, &final_artifact_graph);

    resolve_finding_severities(&mut coverage_findings, &graph, &config);

    resolve_finding_severities(&mut plugin_findings, &graph, &config);
    plugin_findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    // Convert artifact graph conflicts into findings
    let mut conflict_findings =
        supersigil_verify::artifact_conflict_findings(&final_artifact_graph);
    resolve_finding_severities(&mut conflict_findings, &graph, &config);
    conflict_findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    example_findings =
        finalize_example_findings(example_findings, example_skip_reason, &graph, &config);

    // Filter findings to the selected project scope (req-3-4).
    // Structural findings are already filtered by verify_structural().
    if options.project.is_some() {
        supersigil_verify::filter_findings_to_doc_ids(&mut coverage_findings, &doc_ids);
        supersigil_verify::filter_findings_to_doc_ids(&mut plugin_findings, &doc_ids);
        supersigil_verify::filter_findings_to_doc_ids(&mut conflict_findings, &doc_ids);
        supersigil_verify::filter_findings_to_doc_ids(&mut example_findings, &doc_ids);
    }

    // Count documents for summary
    let doc_count = doc_ids.len();

    // Assemble all findings
    let mut all_findings: Vec<Finding> = Vec::new();
    all_findings.extend(structural_findings);
    all_findings.extend(coverage_findings);
    all_findings.extend(plugin_findings);
    all_findings.extend(conflict_findings);
    all_findings.extend(example_findings);

    if let Some(finding) = supersigil_verify::empty_project_finding(&config, doc_count) {
        all_findings.push(finding);
    }

    let report = supersigil_verify::finalize_report(
        &config,
        doc_count,
        all_findings,
        Some(&final_artifact_graph),
    );
    let status = report.result_status();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    match args.format {
        VerifyFormat::Terminal => {
            let terminal_summary = example_summary.as_ref().filter(|summary| {
                example_progress_display
                    .is_none_or(|display| should_render_example_summary(summary, display))
            });
            let text = format_terminal(&report, terminal_summary, color);
            write!(out, "{text}")?;
        }
        VerifyFormat::Json => {
            let text = format_json(&report);
            writeln!(out, "{text}")?;
        }
        VerifyFormat::Markdown => {
            let text = format_markdown(&report);
            write!(out, "{text}")?;
        }
    }

    match status {
        ResultStatus::Clean => {
            if matches!(args.format, VerifyFormat::Markdown) {
                let n = report.summary.total_documents;
                eprintln!("{} {n} documents verified, no findings.", color.ok());
            }
            Ok(ExitStatus::Success)
        }
        ResultStatus::HasErrors => {
            if !matches!(args.format, VerifyFormat::Json) {
                let hints = remediation_hints(&report, &config);
                if hints.is_empty() {
                    format::hint(color, "Run `supersigil plan` to see outstanding work.");
                } else {
                    for hint in hints {
                        format::hint(color, &hint);
                    }
                }
                if example_skip_reason.is_some() {
                    let n = count_example_pending_criteria(&report, &graph);
                    if n > 0 {
                        format::hint(
                            color,
                            &format!(
                                "{n} uncovered criteria would be covered by examples. \
                                 Run `supersigil verify` (without --skip-examples) to confirm."
                            ),
                        );
                    }
                }
            }
            Ok(ExitStatus::VerifyFailed)
        }
        ResultStatus::WarningsOnly => Ok(ExitStatus::VerifyWarnings),
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::ColorChoice;
    use supersigil_rust::verifies;

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
}
