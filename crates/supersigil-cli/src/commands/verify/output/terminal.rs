use std::collections::BTreeMap;
use std::fmt::Write as _;

use supersigil_verify::{Finding, ReportSeverity, ResultStatus, RuleName, VerificationReport};

use crate::format::{self, ColorConfig, Token};

/// Maximum number of findings per (document, rule) group before collapsing.
const COLLAPSE_THRESHOLD: usize = 3;
/// Number of individual messages shown when a group is collapsed.
const COLLAPSE_PREVIEW: usize = 2;

/// Format a verification report for terminal output using the CLI's styling.
///
/// Groups findings by `doc_id`, sub-groups by rule, and collapses repeated
/// findings of the same rule when there are more than `COLLAPSE_THRESHOLD`.
pub(crate) fn format_terminal(report: &VerificationReport, color: ColorConfig) -> String {
    let mut out = String::new();

    if report.result_status() == ResultStatus::Clean {
        let doc_count = report.summary.total_documents;
        let clean_detail = if let Some(ev) = &report.evidence_summary {
            let criteria = ev.coverage.len();
            let evidence = ev.records.len();
            format!(
                "{doc_count} documents, {criteria} criteria, {evidence} evidence records verified"
            )
        } else {
            format!("{doc_count} documents verified")
        };
        let sep = if color.use_unicode() { "—" } else { "-" };
        let _ = writeln!(
            out,
            "{} Clean {sep} {}",
            color.ok(),
            color.paint(Token::Hint, &clean_detail),
        );
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
                    write_location(&mut out, finding, color);
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
            "{} {suppressed} finding(s) downgraded to info because their documents have status: draft \
             (draft documents suppress errors until promoted).",
            color.paint(Token::Hint, "hint:"),
        );
    }
}

/// Write an indented `path:line:col` location line if position and path data are available.
fn write_location(out: &mut String, finding: &Finding, color: ColorConfig) {
    let Some(pos) = &finding.position else {
        return;
    };
    let Some(path) = finding.details.as_ref().and_then(|d| d.path.as_deref()) else {
        return;
    };
    let loc = format!("{path}:{}:{}", pos.line, pos.column);
    let _ = writeln!(out, "      {}", color.paint(Token::Hint, &loc));
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
