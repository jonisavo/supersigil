use std::io::{self, Write};

use serde::Serialize;
use supersigil_core::TaskInfo;

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

/// Write a numbered list of tasks in terminal format.
///
/// # Errors
///
/// Returns an I/O error if writing fails.
pub fn write_tasks(out: &mut impl Write, tasks: &[TaskInfo]) -> io::Result<()> {
    for (i, task) in tasks.iter().enumerate() {
        let task_status = task.status.as_deref().unwrap_or("?");
        write!(out, "{}. {} ({task_status})", i + 1, task.task_id)?;
        if !task.depends_on.is_empty() {
            write!(out, " -- depends on: ")?;
            write_joined(out, &task.depends_on)?;
        }
        writeln!(out)?;
        for (doc_id, crit_id) in &task.implements {
            writeln!(out, "   implements: {doc_id}#{crit_id}")?;
        }
    }
    Ok(())
}

/// Write a collapsed summary of completed tasks, grouped by tasks doc.
///
/// Each group shows the doc ID, contiguous ranges of task IDs, and a count.
///
/// # Errors
///
/// Returns an I/O error if writing fails.
pub fn write_completed_summary(out: &mut impl Write, tasks: &[TaskInfo]) -> io::Result<()> {
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
    writeln!(out, "## Completed: {total} tasks")?;
    for (doc_id, ids) in &groups {
        let ranges = compact_ranges(ids);
        writeln!(out, "- {doc_id}: {ranges} ({} tasks)", ids.len())?;
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
        write_completed_summary(&mut buf, &tasks).unwrap();
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
        write_completed_summary(&mut buf, &tasks).unwrap();
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
        write_completed_summary(&mut buf, &tasks).unwrap();
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
        write_completed_summary(&mut buf, &tasks).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("task-5-1 (1 tasks)"),
            "single task should not show range, got: {output}"
        );
    }

    #[test]
    fn completed_summary_empty_is_noop() {
        let mut buf = Vec::new();
        write_completed_summary(&mut buf, &[]).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn compact_ranges_non_consecutive_items() {
        let result = compact_ranges(&["task-1-1", "task-3-1", "task-5-2"]);
        assert_eq!(result, "task-1-1, task-3-1, task-5-2");
    }
}
