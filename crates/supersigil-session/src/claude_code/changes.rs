//! Reads the report of changed files that Claude Code attaches to a Bash
//! result as `bashEditDiff`.
//!
//! Measured over local transcripts (Claude Code 2.1.272 to 2.1.286), the
//! report is an object with `changedFiles` (absolute paths, at most 200),
//! `files` (at most five entries, each with `filePath`, `hunks`, and
//! optionally `created` or `deleted`), `moreFiles` (the number of changed
//! files without an entry), and sometimes a `shared` or `unavailable` flag.
//! It covers unignored files of the project and describes the whole
//! command's net change. A failed or background call carries none.
//!
//! Paths are compared as paths, never as text: two spellings of one file
//! are one file, and the first entry of a file is the one that speaks for
//! it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use supersigil_record::observations::{ChangeKind, ChangeReport, FileChange, Hunk, Material};

use super::{contained, hunks};

/// Reason recorded when a result carries no report.
const NO_REPORT: &str = "no change report";
/// Reason recorded when a report is not of the shape this module reads.
const UNREADABLE: &str = "unreadable change report";
/// Reason recorded for a file the harness named without an entry.
const NO_ENTRY: &str = "no entry in the harness report";

/// The change report in a Bash call's structured `result`, with paths
/// relative to `cwd`, the call's working directory.
///
/// Unavailable when `result` is absent or holds no `bashEditDiff`, which
/// says nothing about whether files changed, and when the report is not an
/// object of the measured shape: a report that cannot be read is never
/// taken for one that names no file. Paths outside `cwd` are counted, not
/// kept, as an editing tool's path outside its checkout is.
pub(super) fn change_report(result: Option<&Value>, cwd: &Path) -> Material<ChangeReport> {
    let Some(report) = result.and_then(|r| r.get("bashEditDiff")) else {
        return Material::unavailable(NO_REPORT);
    };
    read(report, cwd).map_or_else(|| Material::unavailable(UNREADABLE), Material::Retained)
}

/// Reads `report`, or returns `None` when a part of it has another shape.
/// `files` is the one field every measured report has, so an object
/// without it is another layout, not an empty report.
fn read(report: &Value, cwd: &Path) -> Option<ChangeReport> {
    let fields = report.as_object()?;
    let listed = match fields.get("changedFiles") {
        Some(paths) => strings(paths)?,
        None => Vec::new(),
    };
    let entries = entries(fields.get("files")?)?;
    let more = match fields.get("moreFiles") {
        Some(count) => count.as_u64()?,
        None => 0,
    };
    // The flags this module knows are booleans wherever they appear.
    for flag in ["shared", "unavailable"] {
        if let Some(value) = fields.get(flag) {
            value.as_bool()?;
        }
    }

    let place = |path| place(cwd, path);
    // The first entry of each file.
    let mut by_file: BTreeMap<Result<PathBuf, &str>, &Entry> = BTreeMap::new();
    for entry in &entries {
        by_file.entry(place(entry.path)).or_insert(entry);
    }
    // Listed files in the harness's order, then entries it did not list,
    // each file once.
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    let mut without_entry: u64 = 0;
    for file in listed.into_iter().map(place) {
        if seen.insert(file.clone()) {
            if !by_file.contains_key(&file) {
                without_entry += 1;
            }
            order.push(file);
        }
    }
    for file in entries.iter().map(|entry| place(entry.path)) {
        if seen.insert(file.clone()) {
            order.push(file);
        }
    }

    let mut files = Vec::new();
    let mut outside = 0;
    for file in order {
        let (kind, patch) = match by_file.get(&file) {
            Some(entry) => (entry.kind, entry.patch.clone()),
            None => (ChangeKind::NotStated, Material::unavailable(NO_ENTRY)),
        };
        match file {
            Ok(inside) => files.push(FileChange {
                path: inside,
                kind,
                patch,
            }),
            Err(_) => outside += 1,
        }
    }
    Some(ChangeReport {
        files,
        outside,
        unlisted: more.saturating_sub(without_entry),
        flags: fields
            .iter()
            .filter(|(_, value)| value.as_bool() == Some(true))
            .map(|(name, _)| name.clone())
            .collect(),
    })
}

/// Where a reported path lies: a file inside `cwd`, relative to it, or
/// outside it, as written.
fn place<'a>(cwd: &Path, path: &'a str) -> Result<PathBuf, &'a str> {
    contained(cwd, path).ok_or(path)
}

/// One element of the report's `files`.
struct Entry<'a> {
    path: &'a str,
    kind: ChangeKind,
    patch: Material<Vec<Hunk>>,
}

/// The strings of an array, or `None` when `value` is anything else.
fn strings(value: &Value) -> Option<Vec<&str>> {
    value.as_array()?.iter().map(Value::as_str).collect()
}

/// The entries of `files`, or `None` when it is not an array of objects
/// that each name a path and whose `created` and `deleted`, where present,
/// are booleans: a flag of another type states nothing, and reading it as
/// `false` would turn a malformed entry into a statement about the file.
fn entries(value: &Value) -> Option<Vec<Entry<'_>>> {
    value
        .as_array()?
        .iter()
        .map(|entry| {
            let flag = |name: &str| match entry.get(name) {
                Some(value) => value.as_bool(),
                None => Some(false),
            };
            Some(Entry {
                path: entry.get("filePath")?.as_str()?,
                kind: match (flag("created")?, flag("deleted")?) {
                    (true, false) => ChangeKind::Created,
                    (false, true) => ChangeKind::Deleted,
                    (false, false) => ChangeKind::Modified,
                    // Both at once describe no single change.
                    (true, true) => ChangeKind::NotStated,
                },
                patch: match entry.get("hunks") {
                    Some(value) => hunks(value).map_or_else(
                        || Material::unavailable("unreadable hunks"),
                        Material::Retained,
                    ),
                    None => Material::unavailable("no hunks in the entry"),
                },
            })
        })
        .collect()
}
