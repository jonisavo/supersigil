use std::fmt;
use std::io::{self, IsTerminal, Write};

use anstyle::{AnsiColor, Effects, Style};
use serde::Serialize;
use supersigil_core::TaskInfo;

// ---------------------------------------------------------------------------
// Semantic color tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum Token {
    Header,
    Label,
    DocId,
    DocType,
    Status,
    StatusGood,
    StatusBad,
    Count,
    Path,
    Success,
    Error,
    Warning,
    Hint,
}

impl Token {
    fn style(self) -> Style {
        match self {
            Token::Header | Token::Label | Token::Count => Style::new().effects(Effects::BOLD),
            Token::DocId => Style::new().fg_color(Some(AnsiColor::Cyan.into())),
            Token::DocType => Style::new().fg_color(Some(AnsiColor::Blue.into())),
            Token::Status => Style::new().fg_color(Some(AnsiColor::Yellow.into())),
            Token::StatusGood => Style::new().fg_color(Some(AnsiColor::Green.into())),
            Token::StatusBad => Style::new().fg_color(Some(AnsiColor::Red.into())),
            Token::Path | Token::Hint => Style::new().effects(Effects::DIMMED),
            Token::Success => Style::new()
                .fg_color(Some(AnsiColor::Green.into()))
                .effects(Effects::BOLD),
            Token::Error => Style::new()
                .fg_color(Some(AnsiColor::Red.into()))
                .effects(Effects::BOLD),
            Token::Warning => Style::new()
                .fg_color(Some(AnsiColor::Yellow.into()))
                .effects(Effects::BOLD),
        }
    }
}

// ---------------------------------------------------------------------------
// Painted wrapper
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Painted<'a> {
    text: &'a str,
    style: Style,
}

impl fmt::Display for Painted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}",
            self.style.render(),
            self.text,
            self.style.render_reset()
        )
    }
}

// ---------------------------------------------------------------------------
// ColorChoice / ColorConfig
// ---------------------------------------------------------------------------

/// Raw clap enum for `--color` flag.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum ColorChoice {
    Always,
    Never,
    #[default]
    Auto,
}

/// Resolved runtime color/unicode configuration.
#[derive(Debug, Clone, Copy)]
pub struct ColorConfig {
    color: bool,
    unicode: bool,
}

impl ColorConfig {
    /// Resolve the final color configuration.
    ///
    /// Priority: `--color` flag > `FORCE_COLOR` > `NO_COLOR` > TTY detection.
    /// Unicode mirrors the color decision (per design spec symbol table).
    #[must_use]
    pub fn resolve(choice: ColorChoice) -> Self {
        let color = match choice {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => Self::detect_from_env(),
        };
        Self {
            color,
            unicode: color,
        }
    }

    fn detect_from_env() -> bool {
        if std::env::var_os("FORCE_COLOR").is_some() {
            return true;
        }
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        std::io::stdout().is_terminal()
    }

    #[must_use]
    pub fn use_color(self) -> bool {
        self.color
    }

    #[must_use]
    pub fn use_unicode(self) -> bool {
        self.unicode
    }

    #[must_use]
    pub fn paint(self, token: Token, text: &str) -> Painted<'_> {
        let style = if self.color {
            token.style()
        } else {
            Style::new()
        };
        Painted { text, style }
    }
}

// ---------------------------------------------------------------------------
// ExitStatus
// ---------------------------------------------------------------------------

/// Outcome of a command run, mapped to process exit codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// Exit code 0.
    Success,
    /// Exit code 1 — findings already printed (no extra error message).
    VerifyFailed,
    /// Exit code 2 — warnings only.
    VerifyWarnings,
}

// ---------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------

impl ColorConfig {
    #[must_use]
    pub fn ok(self) -> Painted<'static> {
        let text = if self.unicode { "✔" } else { "[ok]" };
        self.paint(Token::Success, text)
    }

    #[must_use]
    pub fn err(self) -> Painted<'static> {
        let text = if self.unicode { "✖" } else { "[err]" };
        self.paint(Token::Error, text)
    }

    #[must_use]
    pub fn warn(self) -> Painted<'static> {
        let text = if self.unicode { "⚠" } else { "[warn]" };
        self.paint(Token::Warning, text)
    }

    #[must_use]
    pub fn info(self) -> Painted<'static> {
        let text = if self.unicode { "ℹ" } else { "[info]" };
        self.paint(Token::Header, text)
    }
}

// ---------------------------------------------------------------------------
// Hints
// ---------------------------------------------------------------------------

/// Print a next-step hint to stderr.
pub fn hint(color: ColorConfig, msg: &str) {
    let prefix = color.paint(Token::Hint, "hint:");
    let _ = writeln!(io::stderr(), "{prefix} {msg}");
}

// ---------------------------------------------------------------------------
// OutputFormat
// ---------------------------------------------------------------------------

/// Output format for commands that support `--format`.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Terminal,
    Json,
}

/// Write a value as pretty-printed JSON to stdout.
///
/// # Errors
///
/// Returns an I/O error if serialization or writing fails.
pub fn write_json<T: Serialize>(value: &T) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value).map_err(io::Error::other)?;
    writeln!(handle)?;
    Ok(())
}

/// Write a value as YAML to stdout.
///
/// # Errors
///
/// Returns an I/O error if serialization or writing fails.
pub fn write_yaml<T: Serialize>(value: &T) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    yaml_serde::to_writer(&mut handle, value).map_err(io::Error::other)?;
    Ok(())
}

/// Write a numbered list of tasks in terminal format.
///
/// # Errors
///
/// Returns an I/O error if writing fails.
pub fn write_tasks(out: &mut impl Write, tasks: &[TaskInfo], color: ColorConfig) -> io::Result<()> {
    for (i, task) in tasks.iter().enumerate() {
        let task_status = task.status.as_deref().unwrap_or("?");
        let tok = status_token(task_status);
        write!(
            out,
            "{}. {} ({})",
            i + 1,
            color.paint(Token::DocId, &task.task_id),
            color.paint(tok, task_status),
        )?;
        if !task.depends_on.is_empty() {
            write!(out, " -- depends on: ")?;
            write_joined(out, &task.depends_on)?;
        }
        writeln!(out)?;
        for (doc_id, crit_id) in &task.implements {
            let ref_str = format!("{doc_id}#{crit_id}");
            writeln!(
                out,
                "   implements: {}",
                color.paint(Token::DocId, &ref_str)
            )?;
        }
    }
    Ok(())
}

/// Map a status string to the appropriate color token.
#[must_use]
pub fn status_token(status: &str) -> Token {
    match status {
        "done" | "implemented" | "approved" | "verified" => Token::StatusGood,
        _ => Token::Status,
    }
}

/// Write a collapsed summary of completed tasks, grouped by tasks doc.
///
/// Each group shows the doc ID, contiguous ranges of task IDs, and a count.
///
/// # Errors
///
/// Returns an I/O error if writing fails.
pub fn write_completed_summary(
    out: &mut impl Write,
    tasks: &[TaskInfo],
    color: ColorConfig,
) -> io::Result<()> {
    if tasks.is_empty() {
        return Ok(());
    }

    // Group tasks by tasks_doc_id, preserving order of first appearance.
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for task in tasks {
        if let Some(group) = groups.iter_mut().find(|(id, _)| *id == task.tasks_doc_id) {
            group.1.push(&task.task_id);
        } else {
            groups.push((&task.tasks_doc_id, vec![&task.task_id]));
        }
    }

    let total: usize = groups.iter().map(|(_, ids)| ids.len()).sum();
    writeln!(
        out,
        "{} {} tasks",
        color.paint(Token::Header, "## Completed:"),
        color.paint(Token::Count, &total.to_string()),
    )?;
    for (doc_id, ids) in &groups {
        let ranges = compact_ranges(ids);
        writeln!(
            out,
            "- {}: {ranges} ({} tasks)",
            color.paint(Token::DocId, doc_id),
            color.paint(Token::Count, &ids.len().to_string()),
        )?;
    }
    Ok(())
}

/// Collapse a list of task IDs into contiguous ranges.
///
/// E.g., `["task-1-1", "task-1-2", "task-3-1"]` → `"task-1-1 .. task-1-2, task-3-1"`
fn compact_ranges(ids: &[&str]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    if ids.len() == 1 {
        return ids[0].to_owned();
    }

    let mut ranges: Vec<(usize, usize)> = Vec::new(); // (start_idx, end_idx) into ids
    ranges.push((0, 0));

    for i in 1..ids.len() {
        let current_range = ranges.last_mut().unwrap();
        if are_consecutive(ids[current_range.1], ids[i]) {
            current_range.1 = i;
        } else {
            ranges.push((i, i));
        }
    }

    ranges
        .iter()
        .map(|&(start, end)| {
            if start == end {
                ids[start].to_owned()
            } else {
                format!("{} .. {}", ids[start], ids[end])
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Check if two task IDs are "consecutive" — same prefix, numeric suffix increments by 1.
///
/// E.g., `task-3-1` and `task-3-2` are consecutive.
/// `task-3-2` and `task-4-1` are NOT consecutive.
fn are_consecutive(a: &str, b: &str) -> bool {
    let Some(dash_a) = a.rfind('-') else {
        return false;
    };
    let Some(dash_b) = b.rfind('-') else {
        return false;
    };
    let (prefix_a, suffix_a) = a.split_at(dash_a);
    let (prefix_b, suffix_b) = b.split_at(dash_b);
    if prefix_a != prefix_b {
        return false;
    }
    let Ok(num_a) = suffix_a[1..].parse::<u32>() else {
        return false;
    };
    let Ok(num_b) = suffix_b[1..].parse::<u32>() else {
        return false;
    };
    num_b == num_a + 1
}

fn write_joined(out: &mut impl Write, values: &[String]) -> io::Result<()> {
    let mut iter = values.iter();
    if let Some(first) = iter.next() {
        write!(out, "{first}")?;
        for value in iter {
            write!(out, ", {value}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(doc_id: &str, task_id: &str) -> TaskInfo {
        TaskInfo {
            tasks_doc_id: doc_id.to_owned(),
            task_id: task_id.to_owned(),
            status: Some("done".to_owned()),
            body_text: None,
            implements: vec![],
            depends_on: vec![],
        }
    }

    #[test]
    fn completed_summary_groups_by_doc() {
        let tasks = vec![
            task("tasks/parser", "task-1-1"),
            task("tasks/parser", "task-1-2"),
            task("tasks/import", "task-1-1"),
        ];
        let mut buf = Vec::new();
        write_completed_summary(&mut buf, &tasks, ColorConfig::resolve(ColorChoice::Never))
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("## Completed: 3 tasks"), "got: {output}");
        assert!(output.contains("tasks/parser:"), "got: {output}");
        assert!(output.contains("tasks/import:"), "got: {output}");
    }

    #[test]
    fn completed_summary_shows_contiguous_ranges() {
        let tasks = vec![
            task("tasks/a", "task-1-1"),
            task("tasks/a", "task-1-2"),
            task("tasks/a", "task-1-3"),
        ];
        let mut buf = Vec::new();
        write_completed_summary(&mut buf, &tasks, ColorConfig::resolve(ColorChoice::Never))
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("task-1-1 .. task-1-3"),
            "should show contiguous range, got: {output}"
        );
    }

    #[test]
    fn completed_summary_shows_multiple_ranges_for_gaps() {
        let tasks = vec![
            task("tasks/a", "task-1-1"),
            task("tasks/a", "task-1-2"),
            // gap: task-1-3 missing
            task("tasks/a", "task-3-1"),
            task("tasks/a", "task-3-2"),
        ];
        let mut buf = Vec::new();
        write_completed_summary(&mut buf, &tasks, ColorConfig::resolve(ColorChoice::Never))
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("task-1-1 .. task-1-2, task-3-1 .. task-3-2"),
            "should show two ranges with gap, got: {output}"
        );
    }

    #[test]
    fn completed_summary_single_task_no_range() {
        let tasks = vec![task("tasks/a", "task-5-1")];
        let mut buf = Vec::new();
        write_completed_summary(&mut buf, &tasks, ColorConfig::resolve(ColorChoice::Never))
            .unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("task-5-1 (1 tasks)"),
            "single task should not show range, got: {output}"
        );
    }

    #[test]
    fn completed_summary_empty_is_noop() {
        let mut buf = Vec::new();
        write_completed_summary(&mut buf, &[], ColorConfig::resolve(ColorChoice::Never)).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn compact_ranges_non_consecutive_items() {
        let result = compact_ranges(&["task-1-1", "task-3-1", "task-5-2"]);
        assert_eq!(result, "task-1-1, task-3-1, task-5-2");
    }

    // ColorConfig tests

    #[test]
    fn color_config_always() {
        let cc = ColorConfig::resolve(ColorChoice::Always);
        assert!(cc.use_color());
        assert!(cc.use_unicode());
    }

    #[test]
    fn color_config_never() {
        let cc = ColorConfig::resolve(ColorChoice::Never);
        assert!(!cc.use_color());
        assert!(!cc.use_unicode());
    }

    #[test]
    fn color_config_symbols_unicode() {
        let cc = ColorConfig::resolve(ColorChoice::Always);
        let ok = cc.ok().to_string();
        let err = cc.err().to_string();
        let warn = cc.warn().to_string();
        let info = cc.info().to_string();
        assert!(ok.contains("\u{2714}"), "ok: {ok}");
        assert!(err.contains("\u{2716}"), "err: {err}");
        assert!(warn.contains("\u{26A0}"), "warn: {warn}");
        assert!(info.contains("\u{2139}"), "info: {info}");
    }

    #[test]
    fn color_config_symbols_ascii() {
        let cc = ColorConfig::resolve(ColorChoice::Never);
        assert_eq!(cc.ok().to_string(), "[ok]");
        assert_eq!(cc.err().to_string(), "[err]");
        assert_eq!(cc.warn().to_string(), "[warn]");
        assert_eq!(cc.info().to_string(), "[info]");
    }

    #[test]
    fn paint_colored_wraps_ansi() {
        let cc = ColorConfig::resolve(ColorChoice::Always);
        let painted = cc.paint(Token::DocId, "cli/req");
        let s = painted.to_string();
        assert!(s.contains("cli/req"), "text missing: {s}");
        // Should contain ANSI escape
        assert!(s.contains("\x1b["), "no ANSI escape: {s}");
    }

    #[test]
    fn paint_no_color_is_plain() {
        let cc = ColorConfig::resolve(ColorChoice::Never);
        let painted = cc.paint(Token::DocId, "cli/req");
        assert_eq!(painted.to_string(), "cli/req");
    }
}
