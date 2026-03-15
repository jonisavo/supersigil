use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use supersigil_core::{ComponentDefs, Config};
use supersigil_verify::examples::executor;
use supersigil_verify::examples::types::{
    ExampleOutcome, ExampleResult, ExampleSpec, MatchCheck, MatchFormat,
};
use supersigil_verify::{
    Finding, ReportSeverity, ResultStatus, RuleName, VerificationReport, VerifyOptions,
    format_json, format_markdown, resolve_severity,
};

use crate::commands::{VerifyArgs, VerifyFormat};
use crate::error::CliError;
use crate::format::{self, ColorConfig, ExitStatus, Token};
use crate::loader;
use crate::plugins;

/// Maximum number of findings per (document, rule) group before collapsing.
const COLLAPSE_THRESHOLD: usize = 3;
/// Number of individual messages shown when a group is collapsed.
const COLLAPSE_PREVIEW: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExampleExecutionSummary {
    passed: usize,
    failures: Vec<ExampleFailure>,
}

impl ExampleExecutionSummary {
    fn failed(&self) -> usize {
        self.failures.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExampleFailure {
    doc_id: String,
    example_id: String,
    runner: String,
    details: Vec<ExampleFailureDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExampleFailureDetail {
    Match {
        check: String,
        expected: String,
        actual: String,
    },
    Message(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExampleProgressDisplay {
    LiveSpinner,
    Stream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExampleProgressState {
    Queued,
    Running,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExampleProgressEntry {
    doc_id: String,
    example_id: String,
    runner: String,
    state: ExampleProgressState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExampleProgressSnapshot {
    entries: Vec<ExampleProgressEntry>,
}

struct ExampleProgressReporter {
    shared: Arc<ExampleProgressShared>,
    ticker: Option<JoinHandle<()>>,
}

struct ExampleProgressShared {
    state: Mutex<ExampleProgressReporterState>,
    display: ExampleProgressDisplay,
    color: ColorConfig,
    stop_spinner: AtomicBool,
}

struct ExampleProgressReporterState {
    snapshot: ExampleProgressSnapshot,
    rendered_lines: usize,
    spinner_frame: usize,
    writer: Box<dyn Write + Send>,
}

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
    let doc_ids: Option<Vec<String>> = options.project.as_ref().map(|project| {
        graph
            .documents()
            .filter(|(id, _)| graph.doc_project(id) == Some(project.as_str()))
            .map(|(id, _)| id.to_owned())
            .collect()
    });

    // -- Phase 1: Plugin evidence + structural checks --
    let (artifact_graph, mut plugin_findings) =
        plugins::build_evidence(&config, &graph, project_root, options.project.as_deref());

    let mut structural_findings =
        supersigil_verify::verify_structural(&graph, &config, project_root, &options)?;

    resolve_finding_severities(&mut structural_findings, &graph, &config);

    let has_structural_errors = structural_findings
        .iter()
        .any(|f| f.effective_severity == ReportSeverity::Error);

    // -- Phase 2: Example execution (gated by structural errors or --skip-examples) --
    let mut example_findings: Vec<Finding> = Vec::new();
    let mut example_evidence = Vec::new();
    let mut example_summary = None;
    let mut example_progress_display = None;

    let examples_skipped = if has_structural_errors {
        // Structural errors gate example execution
        true
    } else if args.skip_examples {
        true
    } else {
        // Collect and execute examples
        let specs = executor::collect_examples(&graph, &config.examples);
        if !specs.is_empty() {
            let results = if matches!(args.format, VerifyFormat::Terminal) {
                let display = if io::stdout().is_terminal() {
                    ExampleProgressDisplay::LiveSpinner
                } else {
                    ExampleProgressDisplay::Stream
                };
                example_progress_display = Some(display);
                let mut reporter = ExampleProgressReporter::new(&specs, color, display);
                reporter.initialize()?;
                let results = executor::execute_examples_with_progress(
                    &specs,
                    project_root,
                    &config.examples,
                    Some(&reporter),
                );
                reporter.finish()?;
                results
            } else {
                executor::execute_examples(&specs, project_root, &config.examples)
            };
            example_summary = Some(ExampleExecutionSummary::from_results(&results));

            // Handle --update-snapshots
            if args.update_snapshots {
                update_snapshots(&results, !matches!(args.format, VerifyFormat::Json));
            }

            example_evidence = executor::results_to_evidence(&results);
            example_findings = executor::results_to_findings(&results);
        }
        false
    };

    // Optionally add an info finding noting why examples were skipped
    if examples_skipped {
        let reason = if has_structural_errors {
            "example execution skipped due to structural errors in phase 1"
        } else {
            "example execution skipped via --skip-examples"
        };
        let mut skip_finding =
            Finding::new(RuleName::ExampleFailed, None, reason.to_string(), None);
        skip_finding.effective_severity = ReportSeverity::Info;
        skip_finding.raw_severity = ReportSeverity::Info;
        example_findings.push(skip_finding);
    }

    // Merge example evidence into artifact graph if we have any
    let final_artifact_graph = if example_evidence.is_empty() {
        artifact_graph
    } else {
        // Extract existing evidence and append example evidence, then rebuild
        let mut all_plugin_evidence: Vec<_> = artifact_graph.evidence.into_iter().collect();
        all_plugin_evidence.extend(example_evidence);
        supersigil_verify::artifact_graph::build_artifact_graph(&graph, vec![], all_plugin_evidence)
    };

    // -- Phase 3: Coverage --
    let mut coverage_findings = supersigil_verify::verify_coverage(&graph, &final_artifact_graph);

    resolve_finding_severities(&mut coverage_findings, &graph, &config);

    resolve_finding_severities(&mut plugin_findings, &graph, &config);
    plugin_findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    // Convert artifact graph conflicts into findings
    let mut conflict_findings: Vec<Finding> = if final_artifact_graph.conflicts.is_empty() {
        Vec::new()
    } else {
        final_artifact_graph
            .conflicts
            .iter()
            .map(|conflict| {
                let test_name = format!("{}::{}", conflict.test.file.display(), conflict.test.name);
                let left: Vec<String> = conflict.left.iter().map(ToString::to_string).collect();
                let right: Vec<String> = conflict.right.iter().map(ToString::to_string).collect();
                let message = format!(
                    "evidence conflict for test `{test_name}`: \
                     criterion sets disagree — [{}] vs [{}]",
                    left.join(", "),
                    right.join(", "),
                );
                Finding::new(RuleName::PluginDiscoveryFailure, None, message, None)
            })
            .collect()
    };
    resolve_finding_severities(&mut conflict_findings, &graph, &config);
    conflict_findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    // Resolve severities for example findings, but skip the manually-set
    // skip-info finding (it has raw_severity=Info, which real ExampleFailed
    // findings never have since they default to Error).
    for finding in &mut example_findings {
        if finding.raw_severity != ReportSeverity::Info {
            let doc_status = finding
                .doc_id
                .as_ref()
                .and_then(|id| graph.document(id))
                .and_then(|doc| doc.frontmatter.status.as_deref());
            finding.effective_severity =
                resolve_severity(&finding.rule, doc_status, &config.verify);
        }
    }

    // Filter findings to the selected project scope (req-3-4).
    // Structural findings are already filtered by verify_structural().
    if let Some(ref ids) = doc_ids {
        let retain_in_project = |f: &Finding| f.doc_id.as_ref().is_none_or(|id| ids.contains(id));
        coverage_findings.retain(retain_in_project);
        plugin_findings.retain(retain_in_project);
        conflict_findings.retain(retain_in_project);
        example_findings.retain(retain_in_project);
    }

    // Count documents for summary
    let doc_count = doc_ids
        .as_ref()
        .map_or_else(|| graph.documents().count(), Vec::len);

    // Assemble all findings
    let mut all_findings: Vec<Finding> = Vec::new();
    all_findings.extend(structural_findings);
    all_findings.extend(coverage_findings);
    all_findings.extend(plugin_findings);
    all_findings.extend(conflict_findings);
    all_findings.extend(example_findings);

    // Run post-verify hooks
    if !config.hooks.post_verify.is_empty() {
        let interim = VerificationReport::new(
            all_findings.clone(),
            supersigil_verify::Summary::from_findings(doc_count, &all_findings),
            None,
        );
        let interim_json = serde_json::to_string(&interim).unwrap_or_default();
        let hook_findings = supersigil_verify::hooks::run_hooks(
            &config.hooks.post_verify,
            &interim_json,
            config.hooks.timeout_seconds,
        );
        all_findings.extend(hook_findings);
    }

    // Filter out Off-severity findings
    all_findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    // Recompute summary after all findings assembled
    let summary = supersigil_verify::Summary::from_findings(doc_count, &all_findings);

    // Populate evidence summary from the final artifact graph
    let evidence_summary = (!final_artifact_graph.evidence.is_empty())
        .then(|| supersigil_verify::EvidenceSummary::from_artifact_graph(&final_artifact_graph));

    let report = VerificationReport::new(all_findings, summary, evidence_summary);
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
                if examples_skipped {
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

impl ExampleProgressSnapshot {
    fn from_specs(specs: &[ExampleSpec]) -> Self {
        Self {
            entries: specs
                .iter()
                .map(|spec| ExampleProgressEntry {
                    doc_id: spec.doc_id.clone(),
                    example_id: spec.example_id.clone(),
                    runner: spec.runner.clone(),
                    state: ExampleProgressState::Queued,
                })
                .collect(),
        }
    }

    fn mark_started(&mut self, spec: &ExampleSpec) {
        if let Some(entry) = self.find_mut(spec) {
            entry.state = ExampleProgressState::Running;
        }
    }

    fn mark_finished(&mut self, result: &ExampleResult) {
        if let Some(entry) = self.find_mut(&result.spec) {
            entry.state = if matches!(&result.outcome, ExampleOutcome::Pass) {
                ExampleProgressState::Passed
            } else {
                ExampleProgressState::Failed
            };
        }
    }

    fn complete_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.state.is_complete())
            .count()
    }

    fn has_running(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.state == ExampleProgressState::Running)
    }

    fn find_mut(&mut self, spec: &ExampleSpec) -> Option<&mut ExampleProgressEntry> {
        self.entries
            .iter_mut()
            .find(|entry| entry.doc_id == spec.doc_id && entry.example_id == spec.example_id)
    }
}

impl ExampleProgressState {
    fn is_complete(self) -> bool {
        matches!(self, Self::Passed | Self::Failed)
    }
}

impl ExampleProgressReporter {
    fn new(specs: &[ExampleSpec], color: ColorConfig, display: ExampleProgressDisplay) -> Self {
        Self {
            shared: Arc::new(ExampleProgressShared {
                state: Mutex::new(ExampleProgressReporterState {
                    snapshot: ExampleProgressSnapshot::from_specs(specs),
                    rendered_lines: 0,
                    spinner_frame: 0,
                    writer: Box::new(io::stdout()),
                }),
                display,
                color,
                stop_spinner: AtomicBool::new(false),
            }),
            ticker: None,
        }
    }

    fn initialize(&mut self) -> io::Result<()> {
        {
            let mut state = self.shared.state.lock().unwrap();

            match self.shared.display {
                ExampleProgressDisplay::LiveSpinner => self.shared.render_live(&mut state),
                ExampleProgressDisplay::Stream => {
                    let total = state.snapshot.entries.len();
                    writeln!(
                        &mut state.writer,
                        "{} Executing {} {}:",
                        self.shared.color.info(),
                        total,
                        pluralize(total, "example"),
                    )?;
                    state.writer.flush()
                }
            }?;
        };

        if matches!(self.shared.display, ExampleProgressDisplay::LiveSpinner) {
            let shared = Arc::clone(&self.shared);
            self.ticker = Some(std::thread::spawn(move || {
                while !shared.stop_spinner.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(80));

                    if shared.stop_spinner.load(Ordering::Relaxed) {
                        break;
                    }

                    let mut state = shared.state.lock().unwrap();
                    if !state.snapshot.has_running() {
                        continue;
                    }

                    state.spinner_frame = state.spinner_frame.wrapping_add(1);
                    let _ = shared.render_live(&mut state);
                }
            }));
        }

        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        self.shared.stop_spinner.store(true, Ordering::Relaxed);

        if let Some(ticker) = self.ticker.take() {
            let _ = ticker.join();
        }

        if !matches!(self.shared.display, ExampleProgressDisplay::LiveSpinner) {
            return Ok(());
        }

        let mut state = self.shared.state.lock().unwrap();
        writeln!(&mut state.writer)?;
        state.writer.flush()
    }
}

impl ExampleProgressShared {
    fn render_live(&self, state: &mut ExampleProgressReporterState) -> io::Result<()> {
        let lines = render_progress_snapshot(&state.snapshot, self.color, state.spinner_frame);

        if state.rendered_lines > 0 {
            write!(&mut state.writer, "\x1b[{}A", state.rendered_lines)?;
        }

        for line in &lines {
            writeln!(&mut state.writer, "\x1b[2K\r{line}")?;
        }

        state.writer.flush()?;
        state.rendered_lines = lines.len();
        Ok(())
    }

    fn write_stream_started(
        &self,
        state: &mut ExampleProgressReporterState,
        spec: &ExampleSpec,
    ) -> io::Result<()> {
        writeln!(
            &mut state.writer,
            "  {} {}::{} ({}) started",
            self.color.info(),
            spec.doc_id,
            spec.example_id,
            spec.runner,
        )?;
        state.writer.flush()
    }

    fn write_stream_finished(
        &self,
        state: &mut ExampleProgressReporterState,
        result: &ExampleResult,
    ) -> io::Result<()> {
        let (symbol, status) = match &result.outcome {
            ExampleOutcome::Pass => (self.color.ok(), "passed"),
            _ => (self.color.err(), "failed"),
        };

        writeln!(
            &mut state.writer,
            "  {symbol} {}::{} ({}) {status}",
            result.spec.doc_id, result.spec.example_id, result.spec.runner,
        )?;
        state.writer.flush()
    }
}

fn should_render_example_summary(
    summary: &ExampleExecutionSummary,
    display: ExampleProgressDisplay,
) -> bool {
    match display {
        ExampleProgressDisplay::LiveSpinner => summary.failed() > 0,
        ExampleProgressDisplay::Stream => true,
    }
}

impl executor::ExampleProgressObserver for ExampleProgressReporter {
    fn example_started(&self, spec: &ExampleSpec) {
        let mut state = self.shared.state.lock().unwrap();
        state.snapshot.mark_started(spec);

        let _ = match self.shared.display {
            ExampleProgressDisplay::LiveSpinner => self.shared.render_live(&mut state),
            ExampleProgressDisplay::Stream => self.shared.write_stream_started(&mut state, spec),
        };
    }

    fn example_finished(&self, result: &ExampleResult) {
        let mut state = self.shared.state.lock().unwrap();
        state.snapshot.mark_finished(result);

        let _ = match self.shared.display {
            ExampleProgressDisplay::LiveSpinner => self.shared.render_live(&mut state),
            ExampleProgressDisplay::Stream => self.shared.write_stream_finished(&mut state, result),
        };
    }
}

impl ExampleExecutionSummary {
    fn from_results(results: &[ExampleResult]) -> Self {
        let mut passed = 0;
        let mut failures = Vec::new();

        for result in results {
            match &result.outcome {
                ExampleOutcome::Pass => passed += 1,
                ExampleOutcome::Fail(match_failures) => failures.push(ExampleFailure {
                    doc_id: result.spec.doc_id.clone(),
                    example_id: result.spec.example_id.clone(),
                    runner: result.spec.runner.clone(),
                    details: if match_failures.is_empty() {
                        vec![ExampleFailureDetail::Message(
                            "output did not match expected result".to_string(),
                        )]
                    } else {
                        match_failures
                            .iter()
                            .map(|failure| ExampleFailureDetail::Match {
                                check: format!("{:?}", failure.check),
                                expected: failure.expected.clone(),
                                actual: failure.actual.clone(),
                            })
                            .collect()
                    },
                }),
                ExampleOutcome::Timeout => failures.push(ExampleFailure {
                    doc_id: result.spec.doc_id.clone(),
                    example_id: result.spec.example_id.clone(),
                    runner: result.spec.runner.clone(),
                    details: vec![ExampleFailureDetail::Message(format!(
                        "timed out after {}s",
                        result.spec.timeout
                    ))],
                }),
                ExampleOutcome::Error(error) => failures.push(ExampleFailure {
                    doc_id: result.spec.doc_id.clone(),
                    example_id: result.spec.example_id.clone(),
                    runner: result.spec.runner.clone(),
                    details: vec![ExampleFailureDetail::Message(error.clone())],
                }),
            }
        }

        Self { passed, failures }
    }
}

/// Resolve effective severity for a batch of findings against the document graph.
fn resolve_finding_severities(
    findings: &mut [Finding],
    graph: &supersigil_core::DocumentGraph,
    config: &Config,
) {
    for finding in findings {
        let doc_status = finding
            .doc_id
            .as_ref()
            .and_then(|id| graph.document(id))
            .and_then(|doc| doc.frontmatter.status.as_deref());
        finding.effective_severity = resolve_severity(&finding.rule, doc_status, &config.verify);
    }
}

/// Update snapshot files by replacing `body_span` content with the actual output
/// from failed example results.
///
/// Only rewrites `Expected` blocks that use `format="snapshot"`, matching the
/// spec requirement (req-3-4). Source files are normalized (BOM strip, CRLF→LF)
/// before applying byte offsets, since offsets are computed against the
/// normalized source by the parser.
fn update_snapshots(
    results: &[supersigil_verify::examples::types::ExampleResult],
    emit_warnings: bool,
) {
    // Collect patches: (source_path, start, end, replacement)
    let mut patches: BTreeMap<&Path, Vec<(usize, usize, &str)>> = BTreeMap::new();

    for result in results {
        let Some(ref expected) = result.spec.expected else {
            continue;
        };
        if expected.format != MatchFormat::Snapshot {
            continue;
        }
        let Some((start, end)) = expected.body_span else {
            continue;
        };

        let actual_output = match &result.outcome {
            ExampleOutcome::Fail(failures) => failures
                .iter()
                .find(|f| f.check == MatchCheck::Body)
                .map(|f| f.actual.as_str()),
            _ => continue,
        };

        let Some(actual) = actual_output else {
            continue;
        };

        patches
            .entry(&result.spec.source_path)
            .or_default()
            .push((start, end, actual));
    }

    // Apply patches per file, sorted by offset descending so earlier offsets
    // remain valid after later splices.
    for (source_path, mut file_patches) in patches {
        let Ok(raw_source) = std::fs::read_to_string(source_path) else {
            if emit_warnings {
                eprintln!(
                    "warning: could not read {} for snapshot update",
                    source_path.display()
                );
            }
            continue;
        };

        let mut source = supersigil_parser::normalize(&raw_source);

        // Sort by start offset descending: apply from end of file backwards
        file_patches.sort_by(|a, b| b.0.cmp(&a.0));

        let mut skipped = false;
        for (start, end, actual) in &file_patches {
            if *start > *end
                || *end > source.len()
                || !source.is_char_boundary(*start)
                || !source.is_char_boundary(*end)
            {
                if emit_warnings {
                    eprintln!(
                        "warning: stale byte offsets for snapshot in {}, skipping update",
                        source_path.display()
                    );
                }
                skipped = true;
                break;
            }
            source.replace_range(*start..*end, actual);
        }

        if skipped {
            continue;
        }

        if let Err(e) = std::fs::write(source_path, &source)
            && emit_warnings
        {
            eprintln!(
                "warning: could not write snapshot update to {}: {e}",
                source_path.display()
            );
        }
    }
}

fn remediation_hints(report: &VerificationReport, config: &Config) -> Vec<String> {
    let mut hints = Vec::new();

    if report
        .findings
        .iter()
        .any(|finding| finding.rule == RuleName::MissingVerificationEvidence)
    {
        hints.push(
            "Run `supersigil refs` to list canonical criterion refs you can copy into evidence."
                .to_string(),
        );

        if config
            .ecosystem
            .plugins
            .iter()
            .any(|plugin| plugin.as_str() == "rust")
        {
            hints.push(
                "Rust-native fix: annotate a supported test with `#[verifies(\"doc#criterion\")]`."
                    .to_string(),
            );
        }

        hints.push(authored_evidence_hint(config));
    }

    for finding in &report.findings {
        if finding.rule != RuleName::PluginDiscoveryFailure {
            continue;
        }
        let Some(suggestion) = finding
            .details
            .as_ref()
            .and_then(|details| details.suggestion.as_ref())
        else {
            continue;
        };
        if !hints.iter().any(|hint| hint == suggestion) {
            hints.push(suggestion.clone());
        }
    }

    hints
}

fn authored_evidence_hint(config: &Config) -> String {
    let defs = ComponentDefs::merge(ComponentDefs::defaults(), config.components.clone())
        .unwrap_or_else(|_| ComponentDefs::defaults());
    let Some(examples) = defs.get("VerifiedBy").map(|def| def.examples.as_slice()) else {
        return "Authored fix: add criterion-nested `<VerifiedBy ... />` evidence.".to_string();
    };

    let quoted_examples: Vec<String> = examples
        .iter()
        .take(2)
        .map(|example| format!("`{example}`"))
        .collect();

    match quoted_examples.as_slice() {
        [] => "Authored fix: add criterion-nested `<VerifiedBy ... />` evidence.".to_string(),
        [example] => format!("Authored fix: add criterion-nested {example} evidence."),
        [first, second] => {
            format!("Authored fix: add criterion-nested {first} or {second} evidence.")
        }
        _ => unreachable!("only the first two examples are used"),
    }
}

/// Count how many `MissingVerificationEvidence` findings target criteria that
/// have `<Example verifies="...">` refs — i.e., criteria that would be covered
/// if examples had been executed.
fn count_example_pending_criteria(
    report: &VerificationReport,
    graph: &supersigil_core::DocumentGraph,
) -> usize {
    let example_refs = crate::scope::collect_example_verifies_refs(graph);
    if example_refs.is_empty() {
        return 0;
    }

    report
        .findings
        .iter()
        .filter(|f| {
            f.rule == RuleName::MissingVerificationEvidence
                && f.details
                    .as_ref()
                    .and_then(|d| d.target_ref.as_deref())
                    .is_some_and(|r| example_refs.contains(r))
        })
        .count()
}

/// Format a verification report for terminal output using the CLI's styling.
///
/// Groups findings by `doc_id`, sub-groups by rule, and collapses repeated
/// findings of the same rule when there are more than [`COLLAPSE_THRESHOLD`].
fn format_terminal(
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

    // Group findings by doc_id
    let mut groups: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for f in &report.findings {
        let key = f.doc_id.as_deref().unwrap_or("global");
        groups.entry(key).or_default().push(f);
    }

    let mut collapsed = false;

    for (doc, findings) in &groups {
        let _ = writeln!(out, "{}", color.paint(Token::DocId, doc));

        // Sub-group by rule, maintaining first-occurrence order
        let mut rule_groups: Vec<(RuleName, Vec<&Finding>)> = Vec::new();
        for f in findings {
            if f.effective_severity == ReportSeverity::Off {
                continue;
            }
            if let Some(group) = rule_groups.iter_mut().find(|(r, _)| *r == f.rule) {
                group.1.push(f);
            } else {
                rule_groups.push((f.rule, vec![f]));
            }
        }

        for (_rule, group) in &rule_groups {
            let first = group[0];
            let symbol = severity_symbol(first.effective_severity, color);
            let rule_tag = format!("[{}]", first.rule.config_key());
            let rule_label = color.paint(Token::Hint, &rule_tag);

            if group.len() <= COLLAPSE_THRESHOLD {
                for f in group {
                    let _ = writeln!(out, "  {symbol} {rule_label} {}", f.message);
                }
            } else {
                collapsed = true;
                let count_str = group.len().to_string();
                let count = color.paint(Token::Count, &count_str);
                let _ = writeln!(out, "  {symbol} {rule_label} {count} findings");
                for f in group.iter().take(COLLAPSE_PREVIEW) {
                    let _ = writeln!(out, "      {}", color.paint(Token::Hint, &f.message));
                }
                let remaining = group.len() - COLLAPSE_PREVIEW;
                let more = format!("... and {remaining} more");
                let _ = writeln!(out, "      {}", color.paint(Token::Hint, &more));
            }
        }
    }

    let s = &report.summary;
    let err_count = s.error_count.to_string();
    let warn_count = s.warning_count.to_string();
    let doc_count = s.total_documents.to_string();
    let _ = writeln!(
        out,
        "\n{} error(s), {} warning(s), {} info(s) across {} documents",
        color.paint(Token::Error, &err_count),
        color.paint(Token::Warning, &warn_count),
        s.info_count,
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
        .filter(|f| {
            f.effective_severity == ReportSeverity::Info
                && f.raw_severity != ReportSeverity::Info
                && f.raw_severity != ReportSeverity::Off
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

fn render_progress_snapshot(
    snapshot: &ExampleProgressSnapshot,
    color: ColorConfig,
    spinner_frame: usize,
) -> Vec<String> {
    let total = snapshot.entries.len();
    let complete = snapshot.complete_count();
    let mut lines = Vec::with_capacity(total + 1);
    lines.push(format!(
        "{} Executing {} {} ({complete}/{total} complete)",
        color.info(),
        total,
        pluralize(total, "example"),
    ));
    lines.extend(
        snapshot
            .entries
            .iter()
            .map(|entry| render_progress_line(entry, color, spinner_frame)),
    );
    lines
}

fn render_progress_line(
    entry: &ExampleProgressEntry,
    color: ColorConfig,
    spinner_frame: usize,
) -> String {
    let marker = progress_marker(entry.state, color, spinner_frame);
    let status = color.paint(
        progress_status_token(entry.state),
        progress_status(entry.state),
    );
    format!(
        "  {} {}::{} ({}) {status}",
        marker, entry.doc_id, entry.example_id, entry.runner,
    )
}

fn progress_marker(
    state: ExampleProgressState,
    color: ColorConfig,
    spinner_frame: usize,
) -> String {
    match state {
        ExampleProgressState::Queued => {
            let text = if color.use_unicode() { "[·]" } else { "[ ]" };
            color.paint(Token::Hint, text).to_string()
        }
        ExampleProgressState::Running => {
            let frames = spinner_frames(color);
            let text = format!("[{}]", frames[spinner_frame % frames.len()]);
            color.paint(Token::Status, &text).to_string()
        }
        ExampleProgressState::Passed => {
            let text = if color.use_unicode() { "[✔]" } else { "[+]" };
            color.paint(Token::StatusGood, text).to_string()
        }
        ExampleProgressState::Failed => {
            let text = if color.use_unicode() { "[✖]" } else { "[x]" };
            color.paint(Token::StatusBad, text).to_string()
        }
    }
}

fn spinner_frames(color: ColorConfig) -> &'static [&'static str] {
    if color.use_unicode() {
        &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
    } else {
        &["-", "\\", "|", "/"]
    }
}

fn progress_status(state: ExampleProgressState) -> &'static str {
    match state {
        ExampleProgressState::Queued => "queued",
        ExampleProgressState::Running => "running",
        ExampleProgressState::Passed => "passed",
        ExampleProgressState::Failed => "failed",
    }
}

fn progress_status_token(state: ExampleProgressState) -> Token {
    match state {
        ExampleProgressState::Queued => Token::Hint,
        ExampleProgressState::Running => Token::Status,
        ExampleProgressState::Passed => Token::StatusGood,
        ExampleProgressState::Failed => Token::StatusBad,
    }
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

fn pluralize(count: usize, singular: &str) -> String {
    if count == 1 {
        singular.to_string()
    } else {
        format!("{singular}s")
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

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::ColorChoice;
    use supersigil_rust::verifies;
    use supersigil_verify::Summary;

    fn color() -> ColorConfig {
        ColorConfig::resolve(ColorChoice::Always)
    }

    fn no_color() -> ColorConfig {
        ColorConfig::no_color()
    }

    fn progress_snapshot(states: &[(&str, ExampleProgressState)]) -> ExampleProgressSnapshot {
        ExampleProgressSnapshot {
            entries: states
                .iter()
                .map(|(example_id, state)| ExampleProgressEntry {
                    doc_id: "examples/req".to_string(),
                    example_id: (*example_id).to_string(),
                    runner: "cargo-test".to_string(),
                    state: *state,
                })
                .collect(),
        }
    }

    #[test]
    fn groups_by_document() {
        let findings = vec![
            Finding::new(
                RuleName::MissingVerificationEvidence,
                Some("req/auth".to_string()),
                "criterion AC-1 not covered".to_string(),
                None,
            ),
            Finding::new(
                RuleName::OrphanTestTag,
                None,
                "tag 'foo:bar' has no matching document".to_string(),
                None,
            ),
        ];
        let summary = Summary::from_findings(2, &findings);
        let report = VerificationReport::new(findings, summary, None);

        // With color: Unicode symbols + ANSI
        let out = format_terminal(&report, None, color());
        assert!(out.contains("req/auth"), "should contain doc_id header");
        assert!(out.contains("global"), "should contain global header");
        assert!(out.contains("✖"), "should contain error symbol");
        assert!(out.contains("⚠"), "should contain warning symbol");
        assert!(
            out.contains("[missing_verification_evidence]"),
            "should contain rule name",
        );
        assert!(
            out.contains("error(s)") && out.contains("warning(s)") && out.contains("documents"),
            "should contain summary line, got: {out}",
        );

        // Without color: ASCII symbols, no Unicode
        let out_plain = format_terminal(&report, None, no_color());
        assert!(
            out_plain.contains("[err]"),
            "no-color should use ASCII error, got: {out_plain}",
        );
        assert!(
            out_plain.contains("[warn]"),
            "no-color should use ASCII warning, got: {out_plain}",
        );
        assert!(
            !out_plain.contains('✖') && !out_plain.contains('⚠'),
            "no-color should not contain Unicode symbols, got: {out_plain}",
        );
    }

    #[test]
    fn clean_report() {
        let report = VerificationReport::new(vec![], Summary::from_findings(3, &[]), None);

        let out = format_terminal(&report, None, color());
        assert!(
            out.contains("✔") && out.contains("Clean"),
            "colored clean report should show Unicode, got: {out}",
        );

        let out_plain = format_terminal(&report, None, no_color());
        assert!(
            out_plain.contains("[ok]") && out_plain.contains("Clean"),
            "plain clean report should show ASCII, got: {out_plain}",
        );
    }

    #[test]
    fn clean_report_with_examples_shows_example_summary() {
        let report = VerificationReport::new(vec![], Summary::from_findings(1, &[]), None);
        let summary = ExampleExecutionSummary {
            passed: 1,
            failures: Vec::new(),
        };

        let out = format_terminal(&report, Some(&summary), no_color());
        assert!(out.contains("Examples: 1 passed"), "got:\n{out}");
        assert!(out.contains("Clean"), "got:\n{out}");
    }

    #[test]
    fn clean_report_with_failed_examples_shows_non_blocking_summary() {
        let report = VerificationReport::new(vec![], Summary::from_findings(1, &[]), None);
        let summary = ExampleExecutionSummary {
            passed: 0,
            failures: vec![ExampleFailure {
                doc_id: "examples/req".to_string(),
                example_id: "body-mismatch".to_string(),
                runner: "sh".to_string(),
                details: vec![ExampleFailureDetail::Message("boom".to_string())],
            }],
        };

        let out = format_terminal(&report, Some(&summary), no_color());
        assert!(out.contains("No blocking findings"), "got:\n{out}");
        assert!(!out.contains("Clean: no findings"), "got:\n{out}");
    }

    #[verifies("executable-examples/req#req-4-8")]
    #[test]
    fn draft_gating_hint_shown_when_findings_suppressed() {
        let mut finding = Finding::new(
            RuleName::MissingVerificationEvidence,
            Some("req/auth".to_string()),
            "criterion AC-1 not covered".to_string(),
            None,
        );
        // Simulate draft gating: raw stays Error, effective downgraded to Info
        finding.effective_severity = ReportSeverity::Info;

        let report = VerificationReport::new(
            vec![finding.clone()],
            Summary::from_findings(1, &[finding]),
            None,
        );

        let out = format_terminal(&report, None, no_color());
        assert!(
            out.contains("downgraded to info"),
            "should show draft gating hint, got:\n{out}"
        );
        assert!(
            out.contains("status: draft"),
            "hint should mention draft, got:\n{out}"
        );
    }

    #[test]
    fn draft_gating_hint_not_shown_when_no_suppression() {
        let report = VerificationReport::new(vec![], Summary::from_findings(1, &[]), None);

        let out = format_terminal(&report, None, no_color());
        assert!(
            !out.contains("downgraded"),
            "should not show draft hint for clean report, got:\n{out}"
        );
    }

    #[test]
    fn terminal_lists_failed_examples_before_findings() {
        let findings = vec![Finding::new(
            RuleName::ExampleFailed,
            Some("examples/req".to_string()),
            "example 'cargo-fail' (runner: cargo-test) failed".to_string(),
            None,
        )];
        let summary_counts = Summary::from_findings(1, &findings);
        let report = VerificationReport::new(findings, summary_counts, None);
        let summary = ExampleExecutionSummary {
            passed: 1,
            failures: vec![ExampleFailure {
                doc_id: "examples/req".to_string(),
                example_id: "cargo-fail".to_string(),
                runner: "cargo-test".to_string(),
                details: vec![ExampleFailureDetail::Match {
                    check: "Status".to_string(),
                    expected: "0".to_string(),
                    actual: "101".to_string(),
                }],
            }],
        };

        let out = format_terminal(&report, Some(&summary), no_color());
        assert!(out.contains("Examples: 1 passed, 1 failed"), "got:\n{out}");
        assert!(out.contains("Failed examples:"), "got:\n{out}");
        assert!(out.contains("examples/req::cargo-fail"), "got:\n{out}");
        assert!(out.contains("[Status]"), "got:\n{out}");
        assert!(out.contains("expected: 0"), "got:\n{out}");
        assert!(out.contains("actual: 101"), "got:\n{out}");
        assert!(out.contains("[example_failed]"), "got:\n{out}");
    }

    #[test]
    fn live_spinner_omits_redundant_pass_only_example_summary() {
        let clean = ExampleExecutionSummary {
            passed: 2,
            failures: Vec::new(),
        };
        let failed = ExampleExecutionSummary {
            passed: 1,
            failures: vec![ExampleFailure {
                doc_id: "examples/req".to_string(),
                example_id: "failing-example".to_string(),
                runner: "sh".to_string(),
                details: vec![ExampleFailureDetail::Message("boom".to_string())],
            }],
        };

        assert!(!should_render_example_summary(
            &clean,
            ExampleProgressDisplay::LiveSpinner,
        ));
        assert!(should_render_example_summary(
            &clean,
            ExampleProgressDisplay::Stream,
        ));
        assert!(should_render_example_summary(
            &failed,
            ExampleProgressDisplay::LiveSpinner,
        ));
    }

    #[verifies("executable-examples/req#req-4-7")]
    #[test]
    fn progress_snapshot_renders_spinner_frames_for_running_examples() {
        let snapshot = progress_snapshot(&[
            ("queued", ExampleProgressState::Queued),
            ("running", ExampleProgressState::Running),
            ("passed", ExampleProgressState::Passed),
            ("failed", ExampleProgressState::Failed),
        ]);

        let frame_zero = render_progress_snapshot(&snapshot, no_color(), 0).join("\n");
        let frame_one = render_progress_snapshot(&snapshot, no_color(), 1).join("\n");
        assert!(
            frame_zero.contains("Executing 4 examples (2/4 complete)"),
            "got:\n{frame_zero}"
        );
        assert!(
            frame_zero.contains("examples/req::queued (cargo-test) queued"),
            "got:\n{frame_zero}",
        );
        assert!(
            frame_zero.contains("examples/req::running (cargo-test) running"),
            "got:\n{frame_zero}",
        );
        assert!(
            frame_zero.contains("examples/req::passed (cargo-test) passed"),
            "got:\n{frame_zero}",
        );
        assert!(
            frame_zero.contains("examples/req::failed (cargo-test) failed"),
            "got:\n{frame_zero}",
        );
        assert!(
            !frame_zero.contains("[#####-----]"),
            "running examples should render a spinner, not a static half bar:\n{frame_zero}",
        );
        assert_ne!(
            frame_zero, frame_one,
            "running spinner frames should change between ticks"
        );
    }

    #[test]
    fn collapses_repeated_rules() {
        let findings: Vec<Finding> = (0..10)
            .map(|i| {
                Finding::new(
                    RuleName::MissingVerificationEvidence,
                    Some("req/auth".to_string()),
                    format!("criterion `req-{i}` has no validating property"),
                    None,
                )
            })
            .collect();
        let summary = Summary::from_findings(1, &findings);
        let report = VerificationReport::new(findings, summary, None);

        let out = format_terminal(&report, None, no_color());
        assert!(
            out.contains("[missing_verification_evidence] 10 findings"),
            "should show collapsed count, got:\n{out}",
        );
        assert!(
            out.contains("criterion `req-0`"),
            "should show first preview, got:\n{out}",
        );
        assert!(
            out.contains("criterion `req-1`"),
            "should show second preview, got:\n{out}",
        );
        assert!(
            out.contains("and 8 more"),
            "should show remaining count, got:\n{out}",
        );
        assert!(
            !out.contains("criterion `req-9`"),
            "should not show all messages, got:\n{out}",
        );
        assert!(
            out.contains("--format json"),
            "should hint about json format, got:\n{out}",
        );
    }

    #[test]
    fn does_not_collapse_small_groups() {
        let findings: Vec<Finding> = (0..3)
            .map(|i| {
                Finding::new(
                    RuleName::MissingVerificationEvidence,
                    Some("req/auth".to_string()),
                    format!("criterion `req-{i}` has no validating property"),
                    None,
                )
            })
            .collect();
        let summary = Summary::from_findings(1, &findings);
        let report = VerificationReport::new(findings, summary, None);

        let out = format_terminal(&report, None, no_color());
        assert!(out.contains("criterion `req-0`"), "got:\n{out}");
        assert!(out.contains("criterion `req-1`"), "got:\n{out}");
        assert!(out.contains("criterion `req-2`"), "got:\n{out}");
        assert!(!out.contains("findings"), "got:\n{out}");
        assert!(!out.contains("more"), "got:\n{out}");
        assert!(!out.contains("--format json"), "got:\n{out}");
    }

    #[test]
    fn terminal_omits_evidence_summary_when_present() {
        let findings = vec![Finding::new(
            RuleName::MissingVerificationEvidence,
            Some("req/auth".to_string()),
            "criterion req-2 not covered".to_string(),
            None,
        )];
        let summary = Summary::from_findings(1, &findings);
        let report = VerificationReport::new(
            findings,
            summary,
            Some(supersigil_verify::test_helpers::sample_evidence_summary()),
        );

        let out = format_terminal(&report, None, no_color());
        assert!(
            !out.contains("test_login_flow"),
            "terminal output should omit evidence test names even when evidence_summary is present, got:\n{out}",
        );
        assert!(
            !out.contains("rust-attribute"),
            "terminal output should omit evidence kinds even when evidence_summary is present, got:\n{out}",
        );
    }

    #[test]
    fn terminal_no_evidence_when_absent() {
        let findings = vec![Finding::new(
            RuleName::MissingVerificationEvidence,
            Some("req/auth".to_string()),
            "criterion req-2 not covered".to_string(),
            None,
        )];
        let summary = Summary::from_findings(1, &findings);
        let report = VerificationReport::new(findings, summary, None);

        let out = format_terminal(&report, None, no_color());
        // Should not contain evidence-related sections
        assert!(
            !out.contains("Evidence"),
            "terminal output should NOT include Evidence section when absent, got:\n{out}",
        );
    }
}
