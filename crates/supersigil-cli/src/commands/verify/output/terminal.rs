use std::collections::BTreeMap;
use std::fmt::Write as _;

use supersigil_verify::{Finding, ReportSeverity, ResultStatus, RuleName, VerificationReport};

use crate::format::{self, ColorConfig, Token};

use super::{ExampleExecutionSummary, ExampleFailureDetail};

/// Maximum number of findings per (document, rule) group before collapsing.
const COLLAPSE_THRESHOLD: usize = 3;
/// Number of individual messages shown when a group is collapsed.
const COLLAPSE_PREVIEW: usize = 2;

/// Format a verification report for terminal output using the CLI's styling.
///
/// Groups findings by `doc_id`, sub-groups by rule, and collapses repeated
/// findings of the same rule when there are more than `COLLAPSE_THRESHOLD`.
pub(crate) fn format_terminal(
    report: &VerificationReport,
    example_summary: Option<&ExampleExecutionSummary>,
    color: ColorConfig,
) -> String {
    let mut out = String::new();

    if let Some(summary) = example_summary {
        write_example_summary(&mut out, summary, color);
    }

    if report.result_status() == ResultStatus::Clean {
        if example_summary.is_some_and(|summary| summary.failed() > 0) {
            let _ = writeln!(out, "{} No blocking findings", color.info());
        } else {
            let _ = writeln!(out, "{} Clean: no findings", color.ok());
        }
        write_draft_gating_hint(&mut out, &report.findings, color);
        return out;
    }

    let mut groups: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for finding in &report.findings {
        let key = finding.doc_id.as_deref().unwrap_or("global");
        groups.entry(key).or_default().push(finding);
    }

    let mut collapsed = false;

    for (doc_id, findings) in &groups {
        let _ = writeln!(out, "{}", color.paint(Token::DocId, doc_id));

        let mut rule_groups: Vec<(RuleName, Vec<&Finding>)> = Vec::new();
        for finding in findings {
            if finding.effective_severity == ReportSeverity::Off {
                continue;
            }
            if let Some(group) = rule_groups
                .iter_mut()
                .find(|(rule_name, _)| *rule_name == finding.rule)
            {
                group.1.push(finding);
            } else {
                rule_groups.push((finding.rule, vec![finding]));
            }
        }

        for (_rule, group) in &rule_groups {
            let first = group[0];
            let symbol = severity_symbol(first.effective_severity, color);
            let rule_tag = format!("[{}]", first.rule.config_key());
            let rule_label = color.paint(Token::Hint, &rule_tag);

            if group.len() <= COLLAPSE_THRESHOLD {
                for finding in group {
                    let _ = writeln!(out, "  {symbol} {rule_label} {}", finding.message);
                }
            } else {
                collapsed = true;
                let count_str = group.len().to_string();
                let count = color.paint(Token::Count, &count_str);
                let _ = writeln!(out, "  {symbol} {rule_label} {count} findings");
                for finding in group.iter().take(COLLAPSE_PREVIEW) {
                    let _ = writeln!(out, "      {}", color.paint(Token::Hint, &finding.message));
                }
                let remaining = group.len() - COLLAPSE_PREVIEW;
                let more = format!("... and {remaining} more");
                let _ = writeln!(out, "      {}", color.paint(Token::Hint, &more));
            }
        }
    }

    let summary = &report.summary;
    let err_count = summary.error_count.to_string();
    let warn_count = summary.warning_count.to_string();
    let doc_count = summary.total_documents.to_string();
    let _ = writeln!(
        out,
        "\n{} error(s), {} warning(s), {} info(s) across {} documents",
        color.paint(Token::Error, &err_count),
        color.paint(Token::Warning, &warn_count),
        summary.info_count,
        color.paint(Token::Count, &doc_count),
    );

    write_draft_gating_hint(&mut out, &report.findings, color);

    if collapsed {
        let _ = writeln!(
            out,
            "{} Use --format json to see all findings.",
            color.paint(Token::Hint, "hint:"),
        );
    }

    out
}

/// Emit a hint when draft gating suppressed findings that would otherwise be errors or warnings.
fn write_draft_gating_hint(out: &mut String, findings: &[Finding], color: ColorConfig) {
    let suppressed = findings
        .iter()
        .filter(|finding| {
            finding.effective_severity == ReportSeverity::Info
                && finding.raw_severity != ReportSeverity::Info
                && finding.raw_severity != ReportSeverity::Off
        })
        .count();
    if suppressed > 0 {
        let _ = writeln!(
            out,
            "{} {suppressed} finding(s) downgraded to info because their documents have status: draft.",
            color.paint(Token::Hint, "hint:"),
        );
    }
}

fn write_example_summary(out: &mut String, summary: &ExampleExecutionSummary, color: ColorConfig) {
    if summary.passed == 0 && summary.failed() == 0 {
        return;
    }

    let _ = write!(
        out,
        "Examples: {} passed",
        color.paint(Token::Count, &summary.passed.to_string()),
    );
    if summary.failed() > 0 {
        let _ = write!(
            out,
            ", {} failed",
            color.paint(Token::Error, &summary.failed().to_string()),
        );
    }
    let _ = writeln!(out);

    if summary.failures.is_empty() {
        let _ = writeln!(out);
        return;
    }

    let _ = writeln!(out, "Failed examples:");
    for failure in &summary.failures {
        let example_ref = format!("{}::{}", failure.doc_id, failure.example_id);
        let _ = writeln!(
            out,
            "  {} {} ({})",
            color.err(),
            color.paint(Token::DocId, &example_ref),
            failure.runner,
        );
        for detail in &failure.details {
            match detail {
                ExampleFailureDetail::Match {
                    check,
                    expected,
                    actual,
                } => {
                    let _ = writeln!(out, "      [{check}]");
                    write_labelled_block(out, "      expected:", expected);
                    write_labelled_block(out, "      actual:", actual);
                }
                ExampleFailureDetail::Message(message) => {
                    let _ = writeln!(out, "      {message}");
                }
            }
        }
    }
    let _ = writeln!(out);
}

fn write_labelled_block(out: &mut String, label: &str, value: &str) {
    if !value.contains('\n') {
        let _ = writeln!(out, "{label} {value}");
        return;
    }

    let _ = writeln!(out, "{label}");
    for line in value.lines() {
        let _ = writeln!(out, "        {line}");
    }
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
