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

/// The files inside `cwd` that the report in `result` lists in
/// `changedFiles`, relative to `cwd`; none when there is no report or its
/// list has another shape.
///
/// [`change_report`] also keeps a file the report only describes in an
/// entry. This is the narrower statement a heredoc write needs: the
/// harness listed the file as changed.
pub(super) fn listed_files(result: Option<&Value>, cwd: &Path) -> BTreeSet<PathBuf> {
    result
        .and_then(|r| r.get("bashEditDiff")?.get("changedFiles")?.as_array())
        .into_iter()
        .flatten()
        .filter_map(|path| contained(cwd, path.as_str()?))
        .collect()
}

/// What a command's recognized heredoc writes leave in one file, when each
/// of them ran and nothing else in the command touched the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Written {
    /// The whole file: a `>` wrote it, or the first write created it.
    Whole {
        /// The file's content.
        text: String,
        /// Whether the harness says the file did not exist before the
        /// command, so that its diff is the whole file.
        created: bool,
    },
    /// The file's last bytes: only appends, to a file that existed before.
    Tail(String),
}

/// Whether `hunks`, the harness's diff of one file over a whole command,
/// contradict `written`.
///
/// The hunks are display text and may be cut short, so they never confirm
/// a write and never supply bytes. They can only speak against one, by
/// something they show of the file after the command:
///
/// - A whole file: every line on a hunk's new side (context and added
///   lines) must be the written line at that position.
/// - A created file, besides: its diff is the file. When every hunk's
///   header counts exactly the new-side lines the hunk shows (measured:
///   always, over 5,181 hunks; a diff of more than 400 lines has no entry
///   instead of a shortened one), the lines shown must be as many as the
///   lines written.
/// - The end of a file: the hunks must show where the file ends before
///   they can speak against its last bytes. The last hunk does when it has
///   fewer context lines after its last change than before its first: a
///   diff gives both sides the same context, so the file ended first
///   (measured: every hunk that starts after line 1 leads with three
///   context lines, and every hunk followed by another trails with three).
///   When every hunk is counted as above, the lines that hunk shows last
///   must then be the last written lines. And the file, as long as the
///   lines before that hunk and the lines it shows, must hold at least as
///   many lines as were written. They need not be shown as added:
///   a diff may align an appended line with an equal old one and show it
///   as context. The first written line need only end the line it is
///   compared with, since an append onto a line with no newline after it
///   completes that line: appending `y` after an unterminated `x` leaves
///   `xy`. A hunk that may stop before the end of the file, or whose
///   header counts lines it does not show, speaks against nothing.
/// - Either: written text ends with a newline, so a hunk that marks a
///   line of its new side `\ No newline at end of file` contradicts any
///   text written. The same marker on a removed line describes the file
///   before the command, and no other line is that marker.
///
/// An entry without any hunk shows nothing and contradicts nothing.
pub(super) fn contradicts(hunks: &[Hunk], written: &Written) -> bool {
    let sides: Vec<NewSide> = hunks.iter().map(NewSide::of).collect();
    let counted = sides.iter().all(|side| side.counted);
    let (Written::Whole { text, .. } | Written::Tail(text)) = written;
    if !text.is_empty() && sides.iter().any(|side| side.unterminated) {
        return true;
    }
    let expected = lines_of(text);
    match written {
        Written::Whole { created, .. } => {
            let shorter = *created
                && !hunks.is_empty()
                && counted
                && sides.iter().map(|side| side.lines.len()).sum::<usize>() != expected.len();
            shorter
                || hunks.iter().zip(&sides).any(|(hunk, side)| {
                    if side.lines.is_empty() {
                        return false;
                    }
                    // A non-empty new side starts at a 1-based line.
                    let from = usize::try_from(hunk.new_start)
                        .ok()
                        .and_then(|start| start.checked_sub(1));
                    from.and_then(|from| expected.get(from..from.checked_add(side.lines.len())?))
                        != Some(side.lines.as_slice())
                })
        }
        Written::Tail(_) => {
            // The hunk that reaches furthest into the file after the command.
            let last = hunks
                .iter()
                .zip(&sides)
                .max_by_key(|(hunk, _)| hunk.new_start);
            counted
                && last.is_some_and(|(hunk, side)| {
                    side.ends_file
                        && (shorter_than(hunk, side, expected.len())
                            || !ends_with_appended(&side.lines, &expected))
                })
        }
    }
}

/// Whether `hunk`, which reaches the end of the file, shows the file to
/// hold fewer than `appended` lines: the file is as long as the lines
/// before the hunk and the lines it shows. A hunk that shows no line of
/// the file, or does not start at a 1-based line, places nothing.
fn shorter_than(hunk: &Hunk, side: &NewSide, appended: usize) -> bool {
    if side.lines.is_empty() {
        return false;
    }
    usize::try_from(hunk.new_start)
        .ok()
        .and_then(|start| start.checked_sub(1))
        .and_then(|before| before.checked_add(side.lines.len()))
        .is_some_and(|length| length < appended)
}

/// Whether a file whose last lines are `shown` ends with the bytes of the
/// `appended` lines. Where fewer lines are shown than were appended, the
/// shown ones are compared with the last appended ones. The first appended
/// line need only end its line: it may have completed one that had no
/// newline after it.
fn ends_with_appended(shown: &[&str], appended: &[&str]) -> bool {
    let compared = shown.len().min(appended.len());
    let shown = &shown[shown.len() - compared..];
    let written = &appended[appended.len() - compared..];
    match (shown.split_first(), written.split_first()) {
        (Some((line, shown)), Some((first, written))) => {
            let ends = if compared == appended.len() {
                line.ends_with(first)
            } else {
                line == first
            };
            ends && shown == written
        }
        _ => true,
    }
}

/// The lines of written text, each without its newline. Written text ends
/// with a newline whenever it holds a line.
fn lines_of(text: &str) -> Vec<&str> {
    text.strip_suffix('\n')
        .map(|text| text.split('\n').collect())
        .unwrap_or_default()
}

/// The line a unified diff puts after a line that has no newline after it.
const NO_NEWLINE: &str = "\\ No newline at end of file";

/// What one hunk shows of the file after the command.
struct NewSide<'a> {
    /// Its context and added lines, without their markers.
    lines: Vec<&'a str>,
    /// Whether the hunk marks one of `lines` as having no newline after it.
    unterminated: bool,
    /// Whether the hunk's header counts exactly `lines`.
    counted: bool,
    /// Whether the hunk shows fewer context lines after its last change
    /// than before its first, as only a hunk at the end of the file does.
    /// Never set for a hunk with a line this reader does not know, or
    /// whose header counts other old-side lines than it shows.
    ends_file: bool,
}

impl<'a> NewSide<'a> {
    fn of(hunk: &'a Hunk) -> Self {
        let mut side = Self {
            lines: Vec::new(),
            unterminated: false,
            counted: false,
            ends_file: false,
        };
        // The context lines before the first change, and since the last.
        let mut leading = None;
        let mut trailing = 0_usize;
        // The removed and context lines, and whether every line is known.
        let mut old = 0_usize;
        let mut known = true;
        // The line before the current one, which a marker speaks of.
        let mut previous: Option<&str> = None;
        for line in &hunk.lines {
            if line == NO_NEWLINE {
                // After a removed line it describes the file before the
                // command.
                side.unterminated |= previous.is_some_and(|line| !line.starts_with('-'));
                continue;
            }
            previous = Some(line.as_str());
            match line.as_bytes().first() {
                Some(b' ') => {
                    side.lines.push(&line[1..]);
                    old += 1;
                    trailing += 1;
                }
                Some(kind @ (b'+' | b'-')) => {
                    if *kind == b'+' {
                        side.lines.push(&line[1..]);
                    } else {
                        old += 1;
                    }
                    leading.get_or_insert(trailing);
                    trailing = 0;
                }
                // A line that is neither context, added, removed, nor that
                // marker is not one this reader knows: it shows nothing.
                _ => {
                    known = false;
                    previous = None;
                }
            }
        }
        side.counted = usize::try_from(hunk.new_lines) == Ok(side.lines.len());
        side.ends_file = known
            && usize::try_from(hunk.old_lines) == Ok(old)
            && leading.is_some_and(|leading| trailing < leading);
        side
    }
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
