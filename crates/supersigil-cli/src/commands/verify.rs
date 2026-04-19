use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

mod output;
mod report_phase;

use output::terminal::format_timing_summary;
use output::{format_terminal, remediation_hints};
use report_phase::{ReportPhaseInput, assemble_report};

#[cfg(test)]
use supersigil_verify::{ReportSeverity, RuleName, Summary, VerificationReport};
use supersigil_verify::{
    ResultStatus, VerifyOptions, format_json, format_markdown, resolve_finding_severities,
};

use crate::commands::{VerifyArgs, VerifyFormat};
use crate::error::CliError;
use crate::format::{self, ColorConfig, ExitStatus, write_json_value};
use crate::loader;

/// Write a progress message to stderr, ignoring errors (e.g. broken pipe).
fn progress(msg: &str) {
    let _ = io::stderr().write_all(msg.as_bytes());
}

/// Timing data for each verification phase.
#[cfg_attr(test, derive(Debug))]
pub(crate) struct PhaseTimings {
    pub(crate) doc_count: usize,
    pub(crate) parse: Duration,
    pub(crate) evidence: Duration,
    pub(crate) rules: Duration,
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct AffectedSummary {
    doc_count: usize,
    changed_file_count: usize,
}

/// Run the `verify` command: cross-document verification.
///
/// Orchestrates the multi-phase pipeline:
/// 1. Parse specs and build the document graph
/// 2. Scan evidence and run structural checks
/// 3. Check rules and assemble the report
///
/// Progress lines and timing summary are emitted to stderr in terminal mode.
///
/// When graph construction fails (e.g. broken refs), the command converts
/// graph errors into findings with "did you mean?" suggestions instead of
/// aborting immediately.
///
/// # Errors
///
/// Returns `CliError` if loading fails or verification encounters a fatal error.
#[allow(
    clippy::too_many_lines,
    reason = "sequential pipeline phases are clearer in one function"
)]
pub fn run(
    args: &VerifyArgs,
    config_path: &Path,
    color: ColorConfig,
) -> Result<ExitStatus, CliError> {
    let is_terminal = matches!(args.format, VerifyFormat::Terminal);

    // -- Phase 1: Parse --
    if is_terminal {
        progress("  Parsing specs...");
    }
    let parse_start = Instant::now();

    let (config, documents, parse_errors) = loader::parse_all(config_path).inspect_err(|_| {
        if is_terminal {
            progress("\n");
        }
    })?;
    if !parse_errors.is_empty() {
        if is_terminal {
            progress("\n");
        }
        return Err(CliError::Parse(parse_errors));
    }

    let doc_ids: Vec<String> = documents.iter().map(|d| d.frontmatter.id.clone()).collect();

    let graph = match supersigil_core::build_graph(documents, &config) {
        Ok(graph) => graph,
        Err(graph_errors) => {
            if is_terminal {
                progress("\n");
            }
            let known: Vec<&str> = doc_ids.iter().map(String::as_str).collect();
            let findings = supersigil_verify::graph_error_findings(&graph_errors, &known);
            return output_graph_error_findings(args, findings, doc_ids.len(), color);
        }
    };
    let project_root = loader::project_root(config_path);

    let options = VerifyOptions {
        project: args.project.clone(),
        since_ref: args.since.clone(),
        committed_only: args.committed_only,
        use_merge_base: args.merge_base,
    };

    let parse_elapsed = parse_start.elapsed();
    if is_terminal {
        progress(" done\n");
    }

    // -- Phase 2: Evidence + structural checks --
    if is_terminal {
        progress("  Scanning and checking...");
    }
    let evidence_start = Instant::now();

    let inputs = supersigil_verify::VerifyInputs::resolve_for_project(
        &config,
        project_root,
        options.project.as_deref(),
    );

    let supersigil_verify::VerifyPhaseResult {
        artifact_graph,
        plugin_findings,
        mut structural_findings,
        doc_ids,
    } = supersigil_verify::build_evidence_and_verify_structural(
        &graph,
        &config,
        project_root,
        &options,
        &inputs,
    )
    .inspect_err(|_| {
        if is_terminal {
            progress("\n");
        }
    })?;

    resolve_finding_severities(&mut structural_findings, &graph, &config);

    let evidence_elapsed = evidence_start.elapsed();
    if is_terminal {
        progress(" done\n");
    }

    // -- Phase 3: Report assembly --
    if is_terminal {
        progress("  Assembling report...");
    }
    let rules_start = Instant::now();

    let report = assemble_report(ReportPhaseInput {
        graph: &graph,
        config: &config,
        doc_ids: &doc_ids,
        project_filter: options.project.is_some(),
        artifact_graph,
        structural_findings,
        plugin_findings,
    });
    let status = report.result_status();
    let affected_summary = affected_summary(args, &graph, project_root, &doc_ids);

    let rules_elapsed = rules_start.elapsed();
    if is_terminal {
        progress(" done\n");
    }

    let timings = PhaseTimings {
        doc_count: doc_ids.len(),
        parse: parse_elapsed,
        evidence: evidence_elapsed,
        rules: rules_elapsed,
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    match args.format {
        VerifyFormat::Terminal => {
            let text = format_terminal(&report, color, args.detail == format::Detail::Full);
            write!(out, "{text}")?;
            if let Some(summary) = affected_summary
                .as_ref()
                .filter(|summary| summary.doc_count > 0)
            {
                writeln!(out)?;
                writeln!(
                    out,
                    "{}",
                    format_affected_terminal_note(summary, args, color)
                )?;
            }
            let timing_line = format_timing_summary(&timings, color);
            progress(&timing_line);
        }
        VerifyFormat::Json => {
            let value = verify_json_value(&report, args.detail, status, affected_summary.as_ref())?;
            write_json_value(&value)?;
        }
        VerifyFormat::Markdown => {
            let text = format_markdown(&report);
            write!(out, "{text}")?;
            if let Some(summary) = affected_summary
                .as_ref()
                .filter(|summary| summary.doc_count > 0)
            {
                write!(out, "{}", format_affected_markdown_note(summary, args))?;
            }
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
                let hints = remediation_hints(&report, &config, &graph);
                if hints.is_empty() {
                    format::hint(color, "Run `supersigil plan` to see outstanding work.");
                } else {
                    for hint in hints {
                        format::hint(color, &hint);
                    }
                }
            }
            Ok(ExitStatus::VerifyFailed)
        }
        ResultStatus::WarningsOnly => {
            if !matches!(args.format, VerifyFormat::Json) {
                let hints = remediation_hints(&report, &config, &graph);
                for hint in hints {
                    format::hint(color, &hint);
                }
            }
            Ok(ExitStatus::VerifyWarnings)
        }
    }
}

fn affected_summary(
    args: &VerifyArgs,
    graph: &supersigil_core::DocumentGraph,
    project_root: &Path,
    scoped_doc_ids: &[String],
) -> Option<AffectedSummary> {
    let since_ref = args.since.as_deref()?;
    let affected = supersigil_verify::affected::affected(
        graph,
        project_root,
        since_ref,
        args.committed_only,
        args.merge_base,
    )
    .ok()?;

    let direct_by_id: BTreeMap<_, _> = affected
        .iter()
        .filter(|doc| doc.transitive_from.is_none())
        .map(|doc| (doc.id.as_str(), doc))
        .collect();
    let mut direct_sources_by_transitive: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for direct_doc in direct_by_id.values().copied() {
        for referencing_id in graph.references(&direct_doc.id, None) {
            direct_sources_by_transitive
                .entry(referencing_id.as_str())
                .or_default()
                .push(direct_doc);
        }
    }

    let filtered: Vec<_> = if args.project.is_some() {
        let scoped: BTreeSet<&str> = scoped_doc_ids.iter().map(String::as_str).collect();
        affected
            .iter()
            .filter(|doc| scoped.contains(doc.id.as_str()))
            .collect()
    } else {
        affected.iter().collect()
    };

    let changed_file_count = filtered
        .iter()
        .copied()
        .fold(BTreeSet::new(), |mut files, doc| {
            if doc.transitive_from.is_some() {
                if let Some(source_docs) = direct_sources_by_transitive.get(doc.id.as_str()) {
                    for source_doc in source_docs {
                        files.extend(source_doc.changed_files.iter());
                    }
                } else if let Some(source_doc) = doc
                    .transitive_from
                    .as_deref()
                    .and_then(|source_id| direct_by_id.get(source_id).copied())
                {
                    files.extend(source_doc.changed_files.iter());
                }
            } else {
                files.extend(doc.changed_files.iter());
            }
            files
        })
        .len();

    Some(AffectedSummary {
        doc_count: filtered.len(),
        changed_file_count,
    })
}

fn verify_json_value(
    report: &supersigil_verify::VerificationReport,
    detail: format::Detail,
    status: ResultStatus,
    affected_summary: Option<&AffectedSummary>,
) -> io::Result<serde_json::Value> {
    let mut value = serde_json::to_value(report).map_err(io::Error::other)?;

    if detail == format::Detail::Compact
        && status == ResultStatus::Clean
        && let Some(es) = value
            .get_mut("evidence_summary")
            .and_then(|v| v.as_object_mut())
    {
        es.remove("records");
        es.remove("coverage");
    }

    if let Some(summary) = affected_summary {
        value
            .as_object_mut()
            .expect("verification report serializes as a JSON object")
            .insert(
                "affected_summary".to_owned(),
                serde_json::to_value(summary).map_err(io::Error::other)?,
            );
    }

    Ok(value)
}

fn affected_command(args: &VerifyArgs) -> String {
    let mut command = String::from("supersigil affected");
    if let Some(ref since) = args.since {
        command.push_str(" --since ");
        command.push_str(since);
    }
    if args.merge_base {
        command.push_str(" --merge-base");
    }
    if args.committed_only {
        command.push_str(" --committed-only");
    }
    command
}

fn format_affected_terminal_note(
    summary: &AffectedSummary,
    args: &VerifyArgs,
    color: ColorConfig,
) -> String {
    format!(
        "{} {} Run `{}` for details.",
        color.paint(crate::format::Token::Hint, "note:"),
        affected_sentence(summary, args.since.as_deref()),
        affected_command(args),
    )
}

fn format_affected_markdown_note(summary: &AffectedSummary, args: &VerifyArgs) -> String {
    format!(
        "\n## Affected\n\n- {}\n- Run `{}` for details.\n",
        affected_sentence(summary, args.since.as_deref()),
        affected_command(args),
    )
}

fn affected_sentence(summary: &AffectedSummary, since_ref: Option<&str>) -> String {
    let doc_noun = if summary.doc_count == 1 {
        "document"
    } else {
        "documents"
    };
    let file_noun = if summary.changed_file_count == 1 {
        "changed file"
    } else {
        "changed files"
    };
    match since_ref {
        Some(since_ref) => format!(
            "{} {} affected by {} {} since {}.",
            summary.doc_count, doc_noun, summary.changed_file_count, file_noun, since_ref
        ),
        None => format!(
            "{} {} affected by {} {}.",
            summary.doc_count, doc_noun, summary.changed_file_count, file_noun
        ),
    }
}

/// Output findings from graph construction errors and return an error exit.
fn output_graph_error_findings(
    args: &VerifyArgs,
    findings: Vec<supersigil_verify::Finding>,
    doc_count: usize,
    color: ColorConfig,
) -> Result<ExitStatus, CliError> {
    let summary = supersigil_verify::Summary::from_findings(doc_count, &findings);
    let report = supersigil_verify::VerificationReport::new(findings, summary, None);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    match args.format {
        VerifyFormat::Terminal => {
            let text = format_terminal(&report, color, args.detail == format::Detail::Full);
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

    Ok(ExitStatus::VerifyFailed)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests;
