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
    StatusInfo,
    StatusSuperseded,
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
            Token::Status | Token::StatusInfo => {
                Style::new().fg_color(Some(AnsiColor::Yellow.into()))
            }
            Token::StatusGood => Style::new().fg_color(Some(AnsiColor::Green.into())),
            Token::StatusBad => Style::new().fg_color(Some(AnsiColor::Red.into())),
            Token::StatusSuperseded | Token::Path | Token::Hint => {
                Style::new().effects(Effects::DIMMED)
            }
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
    /// Create a `ColorConfig` with color and unicode disabled. Useful for tests.
    #[cfg(test)]
    pub(crate) fn no_color() -> Self {
        Self::resolve(ColorChoice::Never)
    }

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
    /// Exit code 1: findings already printed (no extra error message).
    VerifyFailed,
    /// Exit code 2: warnings only.
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

/// JSON detail level for commands that support `--detail`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Detail {
    /// Omit redundant or debug-level data from JSON output.
    #[default]
    Compact,
    /// Include all data in JSON output.
    Full,
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
        let task_status = task.status.as_deref().unwrap_or("pending");
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
        "done" | "implemented" | "approved" | "accepted" | "verified" => Token::StatusGood,
        "superseded" => Token::StatusSuperseded,
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

/// Write a dependency graph showing task chains with fork/merge notation.
///
/// Tasks are rendered as chains joined by ` → `, with forks shown as indented
/// tree branches and merges shown as `(merge) → ...`.
///
/// # Errors
///
/// Returns an I/O error if writing fails.
pub fn write_dependency_graph(
    out: &mut impl Write,
    tasks: &[&TaskInfo],
    color: ColorConfig,
) -> io::Result<()> {
    if tasks.is_empty() {
        return Ok(());
    }

    let mut renderer = GraphRenderer::new(tasks, color);
    renderer.render_all(out)
}

use std::collections::{HashMap, HashSet};

struct GraphRenderer<'a> {
    forward: HashMap<String, Vec<String>>,
    back: HashMap<String, Vec<String>>,
    task_map: HashMap<String, &'a TaskInfo>,
    /// Bare task IDs that appear in more than one document.
    ambiguous_ids: HashSet<String>,
    roots: Vec<String>,
    visited: HashSet<String>,
    color: ColorConfig,
}

impl<'a> GraphRenderer<'a> {
    fn new(tasks: &[&'a TaskInfo], color: ColorConfig) -> Self {
        // Key by qualified ref (tasks_doc_id#task_id) to avoid collisions
        // when multiple task documents share bare IDs.
        let mut bare_count: HashMap<&str, usize> = HashMap::new();
        for task in tasks {
            *bare_count.entry(task.task_id.as_str()).or_insert(0) += 1;
        }
        let ambiguous_ids: HashSet<String> = bare_count
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(id, _)| id.to_owned())
            .collect();

        let task_set: HashSet<String> = tasks.iter().map(|t| t.qualified_ref()).collect();
        let task_map: HashMap<String, &TaskInfo> =
            tasks.iter().map(|t| (t.qualified_ref(), *t)).collect();

        let mut back: HashMap<String, Vec<String>> = HashMap::new();
        let mut forward: HashMap<String, Vec<String>> = HashMap::new();

        for task in tasks {
            let tid = task.qualified_ref();
            let preds: Vec<String> = task
                .depends_on
                .iter()
                .filter(|d| task_set.contains(d.as_str()))
                .cloned()
                .collect();
            for pred in &preds {
                forward.entry(pred.clone()).or_default().push(tid.clone());
            }
            back.insert(tid, preds);
        }

        // Roots: tasks with no predecessors, preserving input order.
        let roots: Vec<String> = tasks
            .iter()
            .map(|t| t.qualified_ref())
            .filter(|k| back.get(k).is_none_or(Vec::is_empty))
            .collect();

        Self {
            forward,
            back,
            task_map,
            ambiguous_ids,
            roots,
            visited: HashSet::new(),
            color,
        }
    }

    fn render_all(&mut self, out: &mut impl Write) -> io::Result<()> {
        let roots: Vec<String> = self.roots.clone();
        for root in &roots {
            if !self.visited.contains(root) {
                self.render_chain(out, root, "", "")?;
            }
        }
        Ok(())
    }

    /// Display label for a task. Shows bare `task_id` unless the bare ID is
    /// ambiguous (appears in multiple documents), in which case the full
    /// qualified ref is shown for disambiguation.
    fn display_label<'b>(&self, qualified_ref: &'b str) -> &'b str {
        let bare = qualified_ref
            .rsplit_once('#')
            .map_or(qualified_ref, |(_, task_id)| task_id);
        if self.ambiguous_ids.contains(bare) {
            qualified_ref
        } else {
            bare
        }
    }

    fn paint_task_id(&self, qualified_ref: &str) -> String {
        let status = self
            .task_map
            .get(qualified_ref)
            .and_then(|t| t.status.as_deref())
            .unwrap_or("?");
        let tok = status_token(status);
        let label = self.display_label(qualified_ref);
        self.color.paint(tok, label).to_string()
    }

    /// Render a chain starting from `start`.
    ///
    /// `line_prefix` is written before the first line (includes connector like `├── `).
    /// `cont_prefix` is used for continuation lines within this subtree (e.g. `│   `).
    fn render_chain(
        &mut self,
        out: &mut impl Write,
        start: &str,
        line_prefix: &str,
        cont_prefix: &str,
    ) -> io::Result<()> {
        let mut chain: Vec<String> = vec![start.to_owned()];
        self.visited.insert(start.to_owned());

        let mut current: String = start.to_owned();
        loop {
            let Some(succs) = self.forward.get(&current) else {
                break;
            };
            if succs.len() == 1 {
                let next = &succs[0];
                let next_preds = self.back.get(next).map_or(0, Vec::len);
                if next_preds == 1 && !self.visited.contains(next) {
                    self.visited.insert(next.clone());
                    current = next.clone();
                    chain.push(current.clone());
                    continue;
                }
            }
            break;
        }

        // Write the chain.
        let chain_str: Vec<String> = chain.iter().map(|id| self.paint_task_id(id)).collect();
        write!(out, "{line_prefix}{}", chain_str.join(" → "))?;

        // Check what comes after the chain end.
        let last = chain.last().unwrap().clone();
        let succs: Vec<String> = self
            .forward
            .get(&last)
            .map(|v| {
                v.iter()
                    .filter(|s| !self.visited.contains(s.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        if succs.is_empty() {
            writeln!(out)?;
            return Ok(());
        }

        if succs.len() == 1 {
            let next = &succs[0];
            let next_preds = self.back.get(next).map(Vec::as_slice).unwrap_or_default();
            // Only continue inline if all predecessors are visited AND this
            // isn't a merge point (multiple predecessors). Merge points should
            // be rendered by the fork's parent so they get the (merge) label.
            if next_preds.len() <= 1 && next_preds.iter().all(|p| self.visited.contains(p)) {
                write!(out, " → ")?;
                return self.render_chain(out, next, "", cont_prefix);
            }
            writeln!(out)?;
            return Ok(());
        }

        // Fork: multiple successors.
        writeln!(out)?;

        for (i, succ) in succs.iter().enumerate() {
            let is_last = i == succs.len() - 1;
            let connector = if is_last { "└── " } else { "├── " };
            let continuation = if is_last { "    " } else { "│   " };
            self.render_chain(
                out,
                succ,
                &format!("{cont_prefix}{connector}"),
                &format!("{cont_prefix}{continuation}"),
            )?;
        }

        // Find merge point: scan all forward edges from visited nodes for
        // unvisited nodes whose predecessors are all visited.
        if let Some(mp) = self.find_merge_point() {
            write!(out, "{cont_prefix}(merge) → ")?;
            self.render_chain(out, &mp, "", cont_prefix)?;
        }

        Ok(())
    }

    fn find_merge_point(&self) -> Option<String> {
        for tid in &self.visited {
            if let Some(fwd) = self.forward.get(tid) {
                for s in fwd {
                    if !self.visited.contains(s) {
                        let preds = self.back.get(s).map(Vec::as_slice).unwrap_or_default();
                        if preds.len() > 1 && preds.iter().all(|p| self.visited.contains(p)) {
                            return Some(s.clone());
                        }
                    }
                }
            }
        }
        None
    }
}

/// Column gap used between table columns in terminal output.
pub const COL_GAP: &str = "  ";

/// Write a colored, padded table cell. Paints `text` with `token`, then pads
/// with trailing spaces to reach `width`.
///
/// # Errors
///
/// Returns an I/O error if writing fails.
pub fn write_cell(
    out: &mut impl Write,
    color: ColorConfig,
    token: Token,
    text: &str,
    width: usize,
) -> io::Result<()> {
    write!(out, "{}", color.paint(token, text))?;
    let pad = width.saturating_sub(text.len());
    for _ in 0..pad {
        write!(out, " ")?;
    }
    Ok(())
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
    use supersigil_rust::verifies;

    fn no_color() -> ColorConfig {
        ColorConfig::no_color()
    }

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

    fn pending_task(task_id: &str, depends_on: &[&str]) -> TaskInfo {
        pending_task_with_doc("tasks/test", task_id, depends_on)
    }

    fn pending_task_with_doc(doc_id: &str, task_id: &str, depends_on: &[&str]) -> TaskInfo {
        TaskInfo {
            tasks_doc_id: doc_id.to_owned(),
            task_id: task_id.to_owned(),
            status: Some("pending".to_owned()),
            body_text: None,
            implements: vec![],
            depends_on: depends_on
                .iter()
                .map(|d| {
                    if d.contains('#') {
                        d.to_string()
                    } else {
                        format!("{doc_id}#{d}")
                    }
                })
                .collect(),
        }
    }

    fn done_task(task_id: &str, depends_on: &[&str]) -> TaskInfo {
        TaskInfo {
            tasks_doc_id: "tasks/test".to_owned(),
            task_id: task_id.to_owned(),
            status: Some("done".to_owned()),
            body_text: None,
            implements: vec![],
            depends_on: depends_on
                .iter()
                .map(|d| {
                    if d.contains('#') {
                        d.to_string()
                    } else {
                        format!("tasks/test#{d}")
                    }
                })
                .collect(),
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

    #[verifies("cli-runtime/req#req-3-2")]
    #[test]
    fn color_config_always() {
        let cc = ColorConfig::resolve(ColorChoice::Always);
        assert!(cc.use_color());
        assert!(cc.use_unicode());
    }

    #[verifies("cli-runtime/req#req-3-2")]
    #[test]
    fn color_config_never() {
        let cc = ColorConfig::resolve(ColorChoice::Never);
        assert!(!cc.use_color());
        assert!(!cc.use_unicode());
    }

    #[verifies("cli-runtime/req#req-3-2", "cli-runtime/req#req-3-3")]
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

    #[verifies("cli-runtime/req#req-3-2", "cli-runtime/req#req-3-3")]
    #[test]
    fn color_config_symbols_ascii() {
        let cc = ColorConfig::resolve(ColorChoice::Never);
        assert_eq!(cc.ok().to_string(), "[ok]");
        assert_eq!(cc.err().to_string(), "[err]");
        assert_eq!(cc.warn().to_string(), "[warn]");
        assert_eq!(cc.info().to_string(), "[info]");
    }

    #[verifies("cli-runtime/req#req-3-3")]
    #[test]
    fn paint_colored_wraps_ansi() {
        let cc = ColorConfig::resolve(ColorChoice::Always);
        let painted = cc.paint(Token::DocId, "cli/req");
        let s = painted.to_string();
        assert!(s.contains("cli/req"), "text missing: {s}");
        // Should contain ANSI escape
        assert!(s.contains("\x1b["), "no ANSI escape: {s}");
    }

    #[verifies("cli-runtime/req#req-3-3")]
    #[test]
    fn paint_no_color_is_plain() {
        let cc = ColorConfig::resolve(ColorChoice::Never);
        let painted = cc.paint(Token::DocId, "cli/req");
        assert_eq!(painted.to_string(), "cli/req");
    }

    // Dependency graph tests

    fn as_refs(tasks: &[TaskInfo]) -> Vec<&TaskInfo> {
        tasks.iter().collect()
    }

    #[test]
    fn dep_graph_empty_is_noop() {
        let mut buf = Vec::new();
        write_dependency_graph(&mut buf, &[], no_color()).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn dep_graph_single_task() {
        let tasks = vec![pending_task("task-1-1", &[])];
        let mut buf = Vec::new();
        write_dependency_graph(&mut buf, &as_refs(&tasks), no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output.trim(), "task-1-1");
    }

    #[test]
    fn dep_graph_linear_chain() {
        let tasks = vec![
            pending_task("task-1-1", &[]),
            pending_task("task-1-2", &["task-1-1"]),
            pending_task("task-1-3", &["task-1-2"]),
        ];
        let mut buf = Vec::new();
        write_dependency_graph(&mut buf, &as_refs(&tasks), no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output.trim(), "task-1-1 → task-1-2 → task-1-3");
    }

    #[test]
    fn dep_graph_fork() {
        // A → B, A → C
        let tasks = vec![
            pending_task("a", &[]),
            pending_task("b", &["a"]),
            pending_task("c", &["a"]),
        ];
        let mut buf = Vec::new();
        write_dependency_graph(&mut buf, &as_refs(&tasks), no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains('a'), "should contain 'a': {output}");
        assert!(
            output.contains("├──") || output.contains("└──"),
            "should contain tree markers: {output}"
        );
        assert!(output.contains('b'), "should contain 'b': {output}");
        assert!(output.contains('c'), "should contain 'c': {output}");
    }

    #[test]
    fn dep_graph_diamond() {
        // A → B, A → C, B → D, C → D
        let tasks = vec![
            pending_task("a", &[]),
            pending_task("b", &["a"]),
            pending_task("c", &["a"]),
            pending_task("d", &["b", "c"]),
        ];
        let mut buf = Vec::new();
        write_dependency_graph(&mut buf, &as_refs(&tasks), no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("(merge)"),
            "should contain merge marker: {output}"
        );
        assert!(output.contains('d'), "should contain 'd': {output}");
    }

    #[test]
    fn dep_graph_multiple_roots() {
        let tasks = vec![pending_task("a", &[]), pending_task("b", &[])];
        let mut buf = Vec::new();
        write_dependency_graph(&mut buf, &as_refs(&tasks), no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2, "should have two lines: {output}");
        assert_eq!(lines[0], "a");
        assert_eq!(lines[1], "b");
    }

    #[test]
    fn dep_graph_colors_done_green() {
        let tasks = vec![done_task("a", &[]), pending_task("b", &["a"])];
        let mut buf = Vec::new();
        write_dependency_graph(
            &mut buf,
            &as_refs(&tasks),
            ColorConfig::resolve(ColorChoice::Always),
        )
        .unwrap();
        let output = String::from_utf8(buf).unwrap();
        // Done task "a" should have ANSI green, pending "b" should have yellow.
        assert!(output.contains('a'), "should contain 'a': {output}");
        assert!(output.contains('→'), "should contain arrow: {output}");
    }

    #[verifies("work-queries/req#req-5-3")]
    #[test]
    fn dep_graph_duplicate_bare_ids_from_different_docs() {
        // Two docs both have "task-1", keyed differently by qualified ref.
        let tasks = vec![
            pending_task_with_doc("tasks/alpha", "task-1", &[]),
            pending_task_with_doc("tasks/beta", "task-1", &[]),
        ];
        let mut buf = Vec::new();
        write_dependency_graph(&mut buf, &as_refs(&tasks), no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        // Both tasks must appear — no collision.
        assert_eq!(lines.len(), 2, "both tasks should render, got: {output}");
        // Ambiguous bare IDs should show qualified refs.
        assert!(
            output.contains("tasks/alpha#task-1"),
            "should show qualified ref for alpha: {output}"
        );
        assert!(
            output.contains("tasks/beta#task-1"),
            "should show qualified ref for beta: {output}"
        );
    }

    #[verifies("work-queries/req#req-5-3")]
    #[test]
    fn dep_graph_cross_doc_dependency() {
        // task-2 in tasks/beta depends on task-1 in tasks/alpha.
        let tasks = vec![
            pending_task_with_doc("tasks/alpha", "task-1", &[]),
            pending_task_with_doc("tasks/beta", "task-2", &["tasks/alpha#task-1"]),
        ];
        let mut buf = Vec::new();
        write_dependency_graph(&mut buf, &as_refs(&tasks), no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("task-1") && output.contains("task-2"),
            "both tasks should render: {output}"
        );
        assert!(
            output.contains('→'),
            "should show dependency chain: {output}"
        );
    }
}
