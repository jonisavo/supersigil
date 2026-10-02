//! Reads `bashEditDiff`, Claude Code's Bash file change report.
//!
//! Measured in local transcripts from Claude Code 2.1.272 to 2.1.286: an
//! object with `changedFiles` (absolute paths, at most 200), `files` (at
//! most five entries with `filePath`, `hunks`, and optional `created` or
//! `deleted`), `moreFiles` (changed files without entries), and sometimes
//! `shared` or `unavailable` flags. It describes the whole command's net
//! change to unignored project files. Failed and background calls carry
//! none.
//!
//! Paths are compared as paths, not text. Spellings of one file are merged;
//! its first entry determines its description.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use supersigil_record::observations::{ChangeKind, ChangeReport, FileChange, Hunk, Material};

use super::{contained, hunks_material};

/// Reason recorded when a result carries no report.
const NO_REPORT: &str = "no change report";
/// Reason recorded when a report is not of the shape this module reads.
const UNREADABLE: &str = "unreadable change report";
/// Reason recorded for a file the harness named without an entry.
const NO_ENTRY: &str = "no entry in the harness report";

/// The change report in a Bash call's structured `result`, with paths
/// relative to its working directory `cwd`.
///
/// Unavailable if `result` or `bashEditDiff` is absent, which says nothing
/// about file changes, or if the report lacks the measured object shape.
/// An unreadable report is never treated as an empty one. Paths outside
/// `cwd` are counted and omitted, as editing tools' outside paths are.
pub(super) fn change_report(result: Option<&Value>, cwd: &Path) -> Material<ChangeReport> {
    let Some(report) = result.and_then(|r| r.get("bashEditDiff")) else {
        return Material::unavailable(NO_REPORT);
    };
    read(report, cwd).map_or_else(|| Material::unavailable(UNREADABLE), Material::Retained)
}

/// Files inside `cwd` listed in `result`'s `changedFiles`, relative to
/// `cwd`. Returns none for missing reports or differently shaped lists.
///
/// [`change_report`] also keeps files described only in entries. Heredoc
/// writes need the narrower claim here: the harness listed the file as
/// changed.
pub(super) fn listed_files(result: Option<&Value>, cwd: &Path) -> BTreeSet<PathBuf> {
    result
        .and_then(|r| strings(r.get("bashEditDiff")?.get("changedFiles")?))
        .into_iter()
        .flatten()
        .filter_map(|path| contained(cwd, path))
        .collect()
}

/// What recognized heredoc writes leave in a file if all ran and no other
/// statement touched it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Written {
    /// The whole file: a `>` wrote it, or the first write created it.
    Whole {
        /// The file's content.
        text: String,
        /// Whether the file was absent before the command according to
        /// the harness, so its diff covers the whole file.
        created: bool,
    },
    /// The file's last bytes: only appends, to a file that existed before.
    Tail(String),
}

/// Whether the harness's whole-command diff `hunks` contradicts `written`.
///
/// Hunks are capped display text: they never confirm a write or supply
/// bytes. They can only contradict it through shown final-file lines:
///
/// - Whole file: each new-side line (context or added) must match the
///   written line at that position. Unknown lines may represent new-side
///   lines, so a hunk with one cannot establish positions.
/// - Created file: its diff covers the whole file. If all hunks contain
///   only known lines and headers count exactly their shown new-side lines,
///   shown and written line counts must match. All 5,181 measured hunks
///   have exact new-side counts; diffs over 400 lines have no entry rather
///   than a shortened one.
/// - File end: hunks must establish the end to contradict a suffix. The
///   last hunk does if it has less context after its last change than
///   before its first: both sides get equal context, so the file ended
///   first. In measured hunks, those starting after line 1 lead with three
///   context lines; those followed by another trail with three. A hunk
///   with no old-side line reaches the end too: with that context, an old
///   side without a single line is an empty file, so the new side is the
///   whole file. If every hunk is counted as above, its final lines must
///   match the written suffix. Appended lines may appear as context aligned
///   with equal old lines. The total file length, counting lines before and
///   within the hunk, must accommodate all written lines. The first written
///   line need only match its line's suffix: appending `y` to unterminated
///   `x` leaves `xy`. A hunk that may end before the file does, or whose
///   header counts unseen lines, contradicts nothing.
/// - Either: written text ends with a newline. A new-side line marked `\ No
///   newline at end of file` contradicts any written text. On a removed
///   line, the marker describes the file before the command; no other line
///   is that marker. A marker after a known line counts regardless of the
///   rest of the hunk. Likewise, a hunk that removes lines and shows no
///   new-side line, with every line known and both counts exact, shows the
///   file ended empty, which contradicts any written text.
///
/// An entry without hunks contradicts nothing.
pub(super) fn contradicts(hunks: &[Hunk], written: &Written) -> bool {
    let sides: Vec<NewSide> = hunks.iter().map(NewSide::of).collect();
    let counted = sides.iter().all(|side| side.counted);
    let known = sides
        .iter()
        .all(|side| !matches!(side.reach, Reach::Unknown));
    let (Written::Whole { text, .. } | Written::Tail(text)) = written;
    if !text.is_empty() && sides.iter().any(|side| side.unterminated || side.emptied) {
        return true;
    }
    let expected = lines_of(text);
    match written {
        Written::Whole { created, .. } => {
            let shorter = *created
                && !hunks.is_empty()
                && counted
                && known
                && sides.iter().map(|side| side.lines.len()).sum::<usize>() != expected.len();
            shorter
                || sides.iter().any(|side| {
                    if side.lines.is_empty() || matches!(side.reach, Reach::Unknown) {
                        return false;
                    }
                    side.start
                        .and_then(|from| expected.get(from..from.checked_add(side.lines.len())?))
                        != Some(side.lines.as_slice())
                })
        }
        Written::Tail(_) => {
            // The hunk that reaches furthest into the file after the command.
            let last = sides.iter().max_by_key(|side| side.start);
            counted
                && last.is_some_and(|side| {
                    matches!(side.reach, Reach::End)
                        && (shorter_than(side, expected.len())
                            || !ends_with_appended(&side.lines, &expected))
                })
        }
    }
}

/// Whether an end-reaching hunk's `side` shows a file shorter than
/// `appended` lines, counting lines before and within the hunk. A hunk
/// with no new-side lines or no 1-based start establishes no position.
fn shorter_than(side: &NewSide, appended: usize) -> bool {
    if side.lines.is_empty() {
        return false;
    }
    side.start
        .and_then(|before| before.checked_add(side.lines.len()))
        .is_some_and(|length| length < appended)
}

/// Whether final lines `shown` end with `appended`'s bytes. If fewer lines
/// are shown, compare them with the last appended lines. The first
/// appended line need only match its line's suffix: it may complete an
/// unterminated line.
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

/// Written lines without their newlines. Non-empty written text ends with
/// a newline.
fn lines_of(text: &str) -> Vec<&str> {
    text.strip_suffix('\n')
        .map(|text| text.split('\n').collect())
        .unwrap_or_default()
}

/// The line a unified diff puts after a line that has no newline after it.
const NO_NEWLINE: &str = "\\ No newline at end of file";

/// What one hunk shows of the file after the command.
struct NewSide<'a> {
    /// Lines before the new side's 1-based start; `None` if the header
    /// gives no such start.
    start: Option<usize>,
    /// Its context and added lines, without their markers.
    lines: Vec<&'a str>,
    /// Whether the hunk marks one of `lines` as having no newline after it.
    unterminated: bool,
    /// Whether the hunk shows the file ended empty: it removes lines, shows
    /// no new-side line, knows every line, and its header counts both sides
    /// exactly.
    emptied: bool,
    /// Whether the hunk's header counts exactly `lines`.
    counted: bool,
    /// Where in the file `lines` can be placed.
    reach: Reach,
}

/// Where in the file a hunk's new-side lines can be placed.
enum Reach {
    /// Nowhere: an unknown line may represent a new-side line, so
    /// positions cannot be established.
    Unknown,
    /// From the hunk's new start; the file may go on after them.
    Lines,
    /// From the new start to the file's end: less context follows the last
    /// change than precedes the first, which occurs only at the end, or the
    /// hunk has no old-side line, so the file was empty. The header also
    /// counts the shown old-side lines.
    End,
}

impl<'a> NewSide<'a> {
    fn of(hunk: &'a Hunk) -> Self {
        let mut lines = Vec::new();
        let mut unterminated = false;
        // The context lines before the first change, and since the last.
        let mut leading = None;
        let mut trailing = 0_usize;
        // The removed and context lines, and whether every line is known.
        let mut old = 0_usize;
        let mut known = true;
        // The preceding line, which a marker describes.
        let mut previous: Option<&str> = None;
        for line in &hunk.lines {
            if line == NO_NEWLINE {
                // After a removed line, it describes the pre-command
                // file.
                unterminated |= previous.is_some_and(|line| !line.starts_with('-'));
                continue;
            }
            previous = Some(line.as_str());
            match line.as_bytes().first() {
                Some(b' ') => {
                    lines.push(&line[1..]);
                    old += 1;
                    trailing += 1;
                }
                Some(kind @ (b'+' | b'-')) => {
                    if *kind == b'+' {
                        lines.push(&line[1..]);
                    } else {
                        old += 1;
                    }
                    leading.get_or_insert(trailing);
                    trailing = 0;
                }
                // Unknown lines (neither context, added, removed, nor the
                // marker) establish nothing.
                _ => {
                    known = false;
                    previous = None;
                }
            }
        }
        let counted = usize::try_from(hunk.new_lines) == Ok(lines.len());
        let old_counted = usize::try_from(hunk.old_lines) == Ok(old);
        // With the harness's context, a side without a single line is a
        // whole empty file: any line beside a change would be shown.
        let reach = if !known {
            Reach::Unknown
        } else if old_counted && (old == 0 || leading.is_some_and(|leading| trailing < leading)) {
            Reach::End
        } else {
            Reach::Lines
        };
        Self {
            start: usize::try_from(hunk.new_start)
                .ok()
                .and_then(|start| start.checked_sub(1)),
            emptied: known && old_counted && counted && old > 0 && lines.is_empty(),
            counted,
            lines,
            unterminated,
            reach,
        }
    }
}

/// Reads `report`, or returns `None` for differently shaped parts. All
/// measured reports have `files`; an object without it has another
/// layout, rather than an empty report.
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
    let reported = listed
        .into_iter()
        .chain(entries.iter().map(|entry| entry.path));
    let mut seen = BTreeSet::new();
    let mut without_entry: u64 = 0;
    let mut files = Vec::new();
    let mut outside = 0;
    for file in reported.map(place) {
        if !seen.insert(file.clone()) {
            continue;
        }
        let (kind, patch) = if let Some(entry) = by_file.get(&file) {
            (entry.kind, entry.patch.clone())
        } else {
            // Only a listed file can be without an entry.
            without_entry += 1;
            (ChangeKind::NotStated, Material::unavailable(NO_ENTRY))
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

/// A reported path inside `cwd`, relative to it, or outside it as
/// written.
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

/// Reads `files` as an array of objects naming paths, with boolean
/// `created` and `deleted` if present; otherwise returns `None`. Other
/// flag types state nothing. Treating them as `false` would turn malformed
/// entries into claims about files.
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
                patch: hunks_material(
                    entry.get("hunks"),
                    "no hunks in the entry",
                    "unreadable hunks",
                ),
            })
        })
        .collect()
}
