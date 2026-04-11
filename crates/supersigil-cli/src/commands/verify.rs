use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

mod output;
mod report_phase;

use output::terminal::{format_scope_header, format_timing_summary};
use output::{format_terminal, remediation_hints};
use report_phase::{ReportPhaseInput, assemble_report};

#[cfg(test)]
use supersigil_verify::{ReportSeverity, RuleName, Summary, VerificationReport};
use supersigil_verify::{
    ResultStatus, VerifyOptions, format_json, format_markdown, resolve_finding_severities,
};

use crate::commands::{VerifyArgs, VerifyFormat};
use crate::error::CliError;
use crate::format::{self, ColorConfig, ExitStatus};
use crate::loader;
use crate::plugins;

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

    let inputs = supersigil_verify::VerifyInputs::resolve(&config, project_root);

    let (artifact_graph, plugin_findings) = plugins::build_evidence(
        &config,
        &graph,
        project_root,
        options.project.as_deref(),
        &inputs,
    );

    let (mut structural_findings, doc_ids) =
        supersigil_verify::verify_structural(&graph, &config, project_root, &options, &inputs)
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

    // -- Scope header (only when --since is used in terminal mode) --
    if is_terminal && let Some(ref since_ref) = args.since {
        let header = format_scope_header(doc_ids.len(), since_ref, color);
        progress(&header);
    }

    // -- Phase 3: Report assembly --
    if is_terminal {
        progress("  Assembling report...");
    }
    let rules_start = Instant::now();

    let mut report = assemble_report(ReportPhaseInput {
        graph: &graph,
        config: &config,
        doc_ids: &doc_ids,
        project_filter: options.project.is_some(),
        artifact_graph,
        structural_findings,
        plugin_findings,
    });
    let status = report.result_status();

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
            let timing_line = format_timing_summary(&timings, color);
            progress(&timing_line);
        }
        VerifyFormat::Json => {
            if args.detail == format::Detail::Compact
                && report.result_status() == ResultStatus::Clean
                && let Some(ref mut summary) = report.evidence_summary
            {
                summary.records.clear();
            }
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
