use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use supersigil_verify::{Finding, ReportSeverity, ResultStatus, RuleName, VerificationReport};

use super::super::PhaseTimings;
use crate::format::{self, ColorConfig, Token};

/// Maximum number of findings per (document, rule) group before collapsing.
const COLLAPSE_THRESHOLD: usize = 3;
/// Number of individual messages shown when a group is collapsed.
const COLLAPSE_PREVIEW: usize = 2;

/// Format a verification report for terminal output using the CLI's styling.
///
/// Groups findings by `doc_id`, sub-groups by rule, and collapses repeated
/// findings of the same rule when there are more than `COLLAPSE_THRESHOLD`
/// (unless `detail_full` is `true`, which shows every finding).
pub(crate) fn format_terminal(
    report: &VerificationReport,
    color: ColorConfig,
    detail_full: bool,
) -> String {
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

            if detail_full || group.len() <= COLLAPSE_THRESHOLD {
                for finding in group {
                    let _ = writeln!(out, "  {symbol} {rule_label} {}", finding.message);
                    write_location(&mut out, finding, color);
                    write_suggestion(&mut out, finding, color);
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

    write_rule_breakdown(&mut out, &report.findings, color);
    write_draft_gating_hint(&mut out, &report.findings, color);

    if collapsed {
        let _ = writeln!(
            out,
            "{} Use --detail full to see all findings.",
            color.paint(Token::Hint, "hint:"),
        );
    }

    out
}

/// Emit an indented breakdown of findings grouped by rule name.
///
/// Only includes findings whose effective severity is not `Off`.
/// Sorted by count descending, then alphabetically by rule name for ties.
fn write_rule_breakdown(out: &mut String, findings: &[Finding], color: ColorConfig) {
    let mut counts: HashMap<RuleName, usize> = HashMap::new();
    for finding in findings {
        if finding.effective_severity == ReportSeverity::Off {
            continue;
        }
        *counts.entry(finding.rule).or_default() += 1;
    }

    if counts.is_empty() {
        return;
    }

    let mut entries: Vec<(RuleName, usize)> = counts.into_iter().collect();
    entries.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.config_key().cmp(b.0.config_key()))
    });

    let breakdown: String = entries
        .iter()
        .map(|(rule, count)| format!("{count} {}", rule.config_key()))
        .collect::<Vec<_>>()
        .join(", ");

    let _ = writeln!(out, "  {}", color.paint(Token::Hint, &breakdown));
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

/// Write an indented "did you mean?" hint if the finding has a suggestion.
fn write_suggestion(out: &mut String, finding: &Finding, color: ColorConfig) {
    let Some(suggestion) = &finding.suggestion else {
        return;
    };
    let hint = format!("did you mean '{suggestion}'?");
    let _ = writeln!(out, "      {}", color.paint(Token::Hint, &hint));
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

/// Format the timing summary line for terminal output.
///
/// Produces a line like:
/// `Verified 5 documents in 0.5s (parse: 0.1s, check: 0.3s, report: 0.1s)`
pub(crate) fn format_timing_summary(timings: &PhaseTimings, color: ColorConfig) -> String {
    let total = timings.parse + timings.evidence + timings.rules;
    let noun = if timings.doc_count == 1 {
        "document"
    } else {
        "documents"
    };
    let count_str = timings.doc_count.to_string();
    let count = color.paint(Token::Count, &count_str);

    let detail = format!(
        "(parse: {:.1}s, check: {:.1}s, report: {:.1}s)",
        timings.parse.as_secs_f64(),
        timings.evidence.as_secs_f64(),
        timings.rules.as_secs_f64(),
    );
    let detail_painted = color.paint(Token::Hint, &detail);

    format!(
        "Verified {count} {noun} in {:.1}s {detail_painted}\n",
        total.as_secs_f64(),
    )
}
