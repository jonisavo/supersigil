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
