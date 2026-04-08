use std::io::{self, Write};
use std::path::Path;

mod output;
mod report_phase;

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

/// Run the `verify` command: cross-document verification.
///
/// Orchestrates the multi-phase pipeline:
/// 1. Plugin evidence + structural checks
/// 2. Coverage check + report assembly
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

    // -- Phase 1: Plugin evidence + structural checks --
    let inputs = supersigil_verify::VerifyInputs::resolve(&config, project_root);

    let (artifact_graph, plugin_findings) = plugins::build_evidence(
        &config,
        &graph,
        project_root,
        options.project.as_deref(),
        &inputs,
    );

    let (mut structural_findings, doc_ids) =
        supersigil_verify::verify_structural(&graph, &config, project_root, &options, &inputs)?;

    resolve_finding_severities(&mut structural_findings, &graph, &config);

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

    let stdout = io::stdout();
    let mut out = stdout.lock();

    match args.format {
        VerifyFormat::Terminal => {
            let text = format_terminal(&report, color);
            write!(out, "{text}")?;
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
