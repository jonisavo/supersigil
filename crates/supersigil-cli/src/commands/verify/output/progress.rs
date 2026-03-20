use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use supersigil_verify::{ExampleOutcome, ExampleProgressObserver, ExampleResult, ExampleSpec};

use crate::format::{ColorConfig, Token};

use super::{ExampleExecutionSummary, pluralize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExampleProgressDisplay {
    LiveSpinner,
    Stream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExampleProgressState {
    Queued,
    Running,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExampleProgressEntry {
    pub(crate) doc_id: String,
    pub(crate) example_id: String,
    pub(crate) runner: String,
    pub(crate) state: ExampleProgressState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExampleProgressSnapshot {
    pub(crate) entries: Vec<ExampleProgressEntry>,
}

pub(crate) struct ExampleProgressReporter {
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
    pub(crate) fn new(
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

    pub(crate) fn initialize(&mut self) -> io::Result<()> {
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

    pub(crate) fn finish(&mut self) -> io::Result<()> {
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

pub(crate) fn should_render_example_summary(
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

pub(crate) fn render_progress_snapshot(
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
