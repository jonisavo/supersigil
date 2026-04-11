use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

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

    let (config, graph) = loader::load_graph(config_path).inspect_err(|_| {
        if is_terminal {
            progress("\n");
        }
    })?;
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
            let text = format_terminal(&report, color);
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

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests;
