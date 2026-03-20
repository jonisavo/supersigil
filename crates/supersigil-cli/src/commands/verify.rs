use std::io::{self, Write};
use std::path::Path;

mod example_phase;
mod output;
mod report_phase;

use example_phase::run_example_phase;
#[cfg(test)]
use output::{
    ExampleExecutionSummary, ExampleFailure, ExampleFailureDetail, ExampleProgressDisplay,
    ExampleProgressEntry, ExampleProgressSnapshot, ExampleProgressState, render_progress_snapshot,
};
use output::{
    count_example_pending_criteria, format_terminal, remediation_hints,
    should_render_example_summary,
};
use report_phase::{ReportPhaseInput, assemble_report};

use supersigil_verify::{
    ReportSeverity, ResultStatus, VerifyOptions, format_json, format_markdown,
    resolve_finding_severities,
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

    let has_structural_errors = structural_findings
        .iter()
        .any(|f| f.effective_severity == ReportSeverity::Error);

    let example_phase = run_example_phase(
        args,
        &graph,
        &config,
        project_root,
        color,
        has_structural_errors,
    )?;
    let prepared = assemble_report(ReportPhaseInput {
        graph: &graph,
        config: &config,
        doc_ids: &doc_ids,
        project_filter: options.project.is_some(),
        artifact_graph,
        structural_findings,
        plugin_findings,
        example_phase,
    });
    let report = prepared.report;
    let example_summary = prepared.example_summary;
    let example_progress_display = prepared.example_progress_display;
    let example_skip_reason = prepared.example_skip_reason;
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
                let hints = remediation_hints(&report, &config, &graph);
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
mod tests;
