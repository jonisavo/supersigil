use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use supersigil_core::{ComponentDefs, Config, DocumentGraph};
use supersigil_verify::{
    ExampleOutcome, ExampleProgressObserver, ExampleResult, ExampleSpec, Finding, MatchCheck,
    MatchFormat, ReportSeverity, ResultStatus, RuleName, VerificationReport,
};

use crate::format::{self, ColorConfig, Token};

/// Maximum number of findings per (document, rule) group before collapsing.
const COLLAPSE_THRESHOLD: usize = 3;
/// Number of individual messages shown when a group is collapsed.
const COLLAPSE_PREVIEW: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExampleExecutionSummary {
    pub(super) passed: usize,
    pub(super) failures: Vec<ExampleFailure>,
}

impl ExampleExecutionSummary {
    pub(super) fn failed(&self) -> usize {
        self.failures.len()
    }

    pub(super) fn from_results(results: &[ExampleResult]) -> Self {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExampleFailure {
    pub(super) doc_id: String,
    pub(super) example_id: String,
    pub(super) runner: String,
    pub(super) details: Vec<ExampleFailureDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ExampleFailureDetail {
    Match {
        check: String,
        expected: String,
        actual: String,
    },
    Message(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExampleProgressDisplay {
    LiveSpinner,
    Stream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExampleProgressState {
    Queued,
    Running,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExampleProgressEntry {
    pub(super) doc_id: String,
    pub(super) example_id: String,
    pub(super) runner: String,
    pub(super) state: ExampleProgressState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExampleProgressSnapshot {
    pub(super) entries: Vec<ExampleProgressEntry>,
}

pub(super) struct ExampleProgressReporter {
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
    pub(super) fn new(
        specs: &[ExampleSpec],
        color: ColorConfig,
        display: ExampleProgressDisplay,
    ) -> Self {
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

    pub(super) fn initialize(&mut self) -> io::Result<()> {
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

    pub(super) fn finish(&mut self) -> io::Result<()> {
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

pub(super) fn should_render_example_summary(
    summary: &ExampleExecutionSummary,
    display: ExampleProgressDisplay,
) -> bool {
    match display {
        ExampleProgressDisplay::LiveSpinner => summary.failed() > 0,
        ExampleProgressDisplay::Stream => true,
    }
}

impl ExampleProgressObserver for ExampleProgressReporter {
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

/// Update snapshot files by replacing `body_span` content with the actual output
/// from failed example results.
///
/// Only rewrites `Expected` blocks that use `format="snapshot"`, matching the
/// spec requirement (req-3-4). Source files are normalized (BOM strip, CRLF→LF)
/// before applying byte offsets, since offsets are computed against the
/// normalized source by the parser.
pub(super) fn update_snapshots(results: &[ExampleResult], emit_warnings: bool) {
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

pub(super) fn remediation_hints(report: &VerificationReport, config: &Config) -> Vec<String> {
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
pub(super) fn count_example_pending_criteria(
    report: &VerificationReport,
    graph: &DocumentGraph,
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
pub(super) fn format_terminal(
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

pub(super) fn render_progress_snapshot(
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
