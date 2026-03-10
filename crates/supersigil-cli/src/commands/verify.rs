use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;

use supersigil_verify::{
    Finding, ReportSeverity, ResultStatus, RuleName, VerificationReport, VerifyOptions,
    format_json, format_markdown, resolve_severity,
};

use crate::commands::{VerifyArgs, VerifyFormat};
use crate::error::CliError;
use crate::format::{self, ColorConfig, ExitStatus, Token};
use crate::loader;
use crate::plugins;

/// Maximum number of findings per (document, rule) group before collapsing.
const COLLAPSE_THRESHOLD: usize = 3;
/// Number of individual messages shown when a group is collapsed.
const COLLAPSE_PREVIEW: usize = 2;

/// Run the `verify` command: cross-document verification.
///
/// # Errors
///
/// Returns `CliError` if loading fails or verification encounters a fatal error.
pub fn run(
    args: &VerifyArgs,
    config_path: &Path,
    color: ColorConfig,
) -> Result<ExitStatus, CliError> {
    let (config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);

    let options = VerifyOptions {
        project: args.project.clone(),
        since_ref: args.since.clone(),
        committed_only: args.committed_only,
        use_merge_base: args.merge_base,
    };

    // -- Plugin assembly & artifact graph construction --
    let (artifact_graph, mut plugin_findings) =
        plugins::build_evidence(&config, &graph, project_root, options.project.as_deref());

    let mut report =
        supersigil_verify::verify(&graph, &config, project_root, &options, &artifact_graph)?;

    // Append plugin failure findings (req-8-6), applying severity resolution
    // so they respect the same config overrides as built-in rules.
    if !plugin_findings.is_empty() {
        for finding in &mut plugin_findings {
            let doc_status = finding
                .doc_id
                .as_ref()
                .and_then(|id| graph.document(id))
                .and_then(|doc| doc.frontmatter.status.as_deref());
            finding.effective_severity =
                resolve_severity(&finding.rule, doc_status, &config.verify);
        }
        plugin_findings.retain(|f| f.effective_severity != ReportSeverity::Off);
        report.findings.extend(plugin_findings);
    }

    // Convert artifact graph conflicts into findings so they are visible in
    // the report, not just as a count in the evidence summary.
    if !artifact_graph.conflicts.is_empty() {
        let mut conflict_findings: Vec<Finding> = artifact_graph
            .conflicts
            .iter()
            .map(|conflict| {
                let test_name =
                    format!("{}::{}", conflict.test.file.display(), conflict.test.name,);
                let left: Vec<String> = conflict.left.iter().map(ToString::to_string).collect();
                let right: Vec<String> = conflict.right.iter().map(ToString::to_string).collect();
                let message = format!(
                    "evidence conflict for test `{test_name}`: \
                     criterion sets disagree — [{}] vs [{}]",
                    left.join(", "),
                    right.join(", "),
                );
                Finding::new(RuleName::PluginDiscoveryFailure, None, message, None)
            })
            .collect();
        for finding in &mut conflict_findings {
            finding.effective_severity = resolve_severity(&finding.rule, None, &config.verify);
        }
        conflict_findings.retain(|f| f.effective_severity != ReportSeverity::Off);
        report.findings.extend(conflict_findings);
    }

    // Recompute summary after appending plugin and conflict findings.
    report.summary =
        supersigil_verify::Summary::from_findings(report.summary.total_documents, &report.findings);

    // Populate evidence summary from artifact graph (req-9-1, req-9-2, req-9-3).
    if !artifact_graph.evidence.is_empty() {
        report.evidence_summary = Some(supersigil_verify::EvidenceSummary::from_artifact_graph(
            &artifact_graph,
        ));
    }

    let status = report.result_status();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    match args.format {
        VerifyFormat::Terminal => {
            let text = format_terminal(&report, color);
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
            let n = report.summary.total_documents;
            eprintln!("{} {n} documents verified, no findings.", color.ok());
            Ok(ExitStatus::Success)
        }
        ResultStatus::HasErrors => {
            format::hint(color, "Run `supersigil plan` to see outstanding work.");
            Ok(ExitStatus::VerifyFailed)
        }
        ResultStatus::WarningsOnly => Ok(ExitStatus::VerifyWarnings),
    }
}

/// Format a verification report for terminal output using the CLI's styling.
///
/// Groups findings by `doc_id`, sub-groups by rule, and collapses repeated
/// findings of the same rule when there are more than [`COLLAPSE_THRESHOLD`].
fn format_terminal(report: &VerificationReport, color: ColorConfig) -> String {
    if report.result_status() == ResultStatus::Clean {
        return format!("{} Clean — no findings\n", color.ok());
    }

    // Group findings by doc_id
    let mut groups: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for f in &report.findings {
        let key = f.doc_id.as_deref().unwrap_or("global");
        groups.entry(key).or_default().push(f);
    }

    let mut out = String::new();
    let mut collapsed = false;

    for (doc, findings) in &groups {
        let _ = writeln!(out, "{}", color.paint(Token::DocId, doc));

        // Sub-group by rule, maintaining first-occurrence order
        let mut rule_groups: Vec<(RuleName, Vec<&Finding>)> = Vec::new();
        for f in findings {
            if f.effective_severity == ReportSeverity::Off {
                continue;
            }
            if let Some(group) = rule_groups.iter_mut().find(|(r, _)| *r == f.rule) {
                group.1.push(f);
            } else {
                rule_groups.push((f.rule, vec![f]));
            }
        }

        for (_rule, group) in &rule_groups {
            let first = group[0];
            let symbol = severity_symbol(first.effective_severity, color);
            let rule_tag = format!("[{}]", first.rule.config_key());
            let rule_label = color.paint(Token::Hint, &rule_tag);

            if group.len() <= COLLAPSE_THRESHOLD {
                for f in group {
                    let _ = writeln!(out, "  {symbol} {rule_label} {}", f.message);
                }
            } else {
                collapsed = true;
                let count_str = group.len().to_string();
                let count = color.paint(Token::Count, &count_str);
                let _ = writeln!(out, "  {symbol} {rule_label} {count} findings");
                for f in group.iter().take(COLLAPSE_PREVIEW) {
                    let _ = writeln!(out, "      {}", color.paint(Token::Hint, &f.message));
                }
                let remaining = group.len() - COLLAPSE_PREVIEW;
                let more = format!("... and {remaining} more");
                let _ = writeln!(out, "      {}", color.paint(Token::Hint, &more));
            }
        }
    }

    let s = &report.summary;
    let err_count = s.error_count.to_string();
    let warn_count = s.warning_count.to_string();
    let doc_count = s.total_documents.to_string();
    let _ = writeln!(
        out,
        "\n{} error(s), {} warning(s), {} info(s) across {} documents",
        color.paint(Token::Error, &err_count),
        color.paint(Token::Warning, &warn_count),
        s.info_count,
        color.paint(Token::Count, &doc_count),
    );

    if collapsed {
        let _ = writeln!(
            out,
            "{} Use --format json to see all findings.",
            color.paint(Token::Hint, "hint:"),
        );
    }

    // Evidence summary (optional)
    if let Some(ref ev) = report.evidence_summary {
        let _ = writeln!(out);
        for entry in &ev.records {
            let targets = entry.targets.join(", ");
            let prov = entry.provenance.join(", ");
            let _ = writeln!(
                out,
                "  {} {} [{}] {} ({} via {})",
                color.paint(Token::Hint, "▸"),
                entry.test_name,
                entry.evidence_kind,
                targets,
                entry.test_file,
                prov,
            );
        }
    }

    out
}

/// Return the severity symbol for terminal output, styled with the CLI's tokens.
fn severity_symbol(severity: ReportSeverity, color: ColorConfig) -> format::Painted<'static> {
    match severity {
        ReportSeverity::Error => color.err(),
        ReportSeverity::Warning => color.warn(),
        ReportSeverity::Info => color.info(),
        ReportSeverity::Off => color.paint(Token::Hint, ""),
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::ColorChoice;
    use supersigil_verify::Summary;
    use supersigil_verify::test_helpers::sample_evidence_summary;

    fn color() -> ColorConfig {
        ColorConfig::resolve(ColorChoice::Always)
    }

    fn no_color() -> ColorConfig {
        ColorConfig::resolve(ColorChoice::Never)
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
        let report = VerificationReport {
            findings,
            summary,
            evidence_summary: None,
        };

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
        let report = VerificationReport {
            findings: vec![],
            summary: Summary::from_findings(3, &[]),
            evidence_summary: None,
        };

        let out = format_terminal(&report, color());
        assert!(
            out.contains("✔") && out.contains("Clean"),
            "colored clean report should show Unicode, got: {out}",
        );

        let out_plain = format_terminal(&report, no_color());
        assert!(
            out_plain.contains("[ok]") && out_plain.contains("Clean"),
            "plain clean report should show ASCII, got: {out_plain}",
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
        let report = VerificationReport {
            findings,
            summary,
            evidence_summary: None,
        };

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
        let report = VerificationReport {
            findings,
            summary,
            evidence_summary: None,
        };

        let out = format_terminal(&report, no_color());
        assert!(out.contains("criterion `req-0`"), "got:\n{out}");
        assert!(out.contains("criterion `req-1`"), "got:\n{out}");
        assert!(out.contains("criterion `req-2`"), "got:\n{out}");
        assert!(!out.contains("findings"), "got:\n{out}");
        assert!(!out.contains("more"), "got:\n{out}");
        assert!(!out.contains("--format json"), "got:\n{out}");
    }

    #[test]
    fn terminal_shows_evidence_summary() {
        let findings = vec![Finding::new(
            RuleName::MissingVerificationEvidence,
            Some("req/auth".to_string()),
            "criterion req-2 not covered".to_string(),
            None,
        )];
        let summary = Summary::from_findings(1, &findings);
        let report = VerificationReport {
            findings,
            summary,
            evidence_summary: Some(sample_evidence_summary()),
        };

        let out = format_terminal(&report, no_color());
        assert!(
            out.contains("test_login_flow"),
            "terminal output should include evidence test name when evidence_summary is present, got:\n{out}",
        );
        assert!(
            out.contains("rust-attribute"),
            "terminal output should include evidence kind, got:\n{out}",
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
        let report = VerificationReport {
            findings,
            summary,
            evidence_summary: None,
        };

        let out = format_terminal(&report, no_color());
        // Should not contain evidence-related sections
        assert!(
            !out.contains("Evidence"),
            "terminal output should NOT include Evidence section when absent, got:\n{out}",
        );
    }
}
