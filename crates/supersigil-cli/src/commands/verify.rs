use std::io::{self, Write};
use std::path::Path;

mod example_phase;
mod output;

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

use supersigil_verify::{
    Finding, ReportSeverity, ResultStatus, VerifyOptions, finalize_example_findings, format_json,
    format_markdown, resolve_finding_severities,
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

    let example_phase = run_example_phase(
        args,
        &graph,
        &config,
        project_root,
        color,
        has_structural_errors,
    )?;
    let mut example_findings = example_phase.findings;
    let example_evidence = example_phase.evidence;
    let example_summary = example_phase.summary;
    let example_progress_display = example_phase.progress_display;
    let example_skip_reason = example_phase.skip_reason;

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
mod tests;
