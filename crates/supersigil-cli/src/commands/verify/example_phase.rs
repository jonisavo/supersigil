use std::io::{self, IsTerminal};
use std::path::Path;

use supersigil_core::{Config, DocumentGraph};
use supersigil_evidence::VerificationEvidenceRecord;
use supersigil_verify::{
    ExampleSkipReason, Finding, collect_examples, execute_examples, execute_examples_with_progress,
    results_to_evidence, results_to_findings,
};

use super::output::{
    ExampleExecutionSummary, ExampleProgressDisplay, ExampleProgressReporter, update_snapshots,
};
use crate::commands::{VerifyArgs, VerifyFormat};
use crate::format::ColorConfig;

pub(super) struct ExamplePhaseResult {
    pub(super) findings: Vec<Finding>,
    pub(super) evidence: Vec<VerificationEvidenceRecord>,
    pub(super) summary: Option<ExampleExecutionSummary>,
    pub(super) progress_display: Option<ExampleProgressDisplay>,
    pub(super) skip_reason: Option<ExampleSkipReason>,
}

impl ExamplePhaseResult {
    fn skipped(skip_reason: ExampleSkipReason) -> Self {
        Self {
            findings: Vec::new(),
            evidence: Vec::new(),
            summary: None,
            progress_display: None,
            skip_reason: Some(skip_reason),
        }
    }

    fn empty() -> Self {
        Self {
            findings: Vec::new(),
            evidence: Vec::new(),
            summary: None,
            progress_display: None,
            skip_reason: None,
        }
    }
}

pub(super) fn run_example_phase(
    args: &VerifyArgs,
    graph: &DocumentGraph,
    config: &Config,
    project_root: &Path,
    color: ColorConfig,
    has_structural_errors: bool,
) -> io::Result<ExamplePhaseResult> {
    if has_structural_errors {
        return Ok(ExamplePhaseResult::skipped(
            ExampleSkipReason::StructuralErrors,
        ));
    }

    if args.skip_examples {
        return Ok(ExamplePhaseResult::skipped(ExampleSkipReason::ExplicitSkip));
    }

    let specs = collect_examples(graph, &config.examples);
    if specs.is_empty() {
        return Ok(ExamplePhaseResult::empty());
    }

    let mut progress_display = None;
    let results = if matches!(args.format, VerifyFormat::Terminal) {
        let display = if io::stdout().is_terminal() {
            ExampleProgressDisplay::LiveSpinner
        } else {
            ExampleProgressDisplay::Stream
        };
        progress_display = Some(display);
        let mut reporter = ExampleProgressReporter::new(&specs, color, display);
        reporter.initialize()?;
        let results =
            execute_examples_with_progress(&specs, project_root, &config.examples, Some(&reporter));
        reporter.finish()?;
        results
    } else {
        execute_examples(&specs, project_root, &config.examples)
    };

    if args.update_snapshots {
        update_snapshots(&results, !matches!(args.format, VerifyFormat::Json));
    }

    Ok(ExamplePhaseResult {
        summary: Some(ExampleExecutionSummary::from_results(&results)),
        evidence: results_to_evidence(&results),
        findings: results_to_findings(&results),
        progress_display,
        skip_reason: None,
    })
}
