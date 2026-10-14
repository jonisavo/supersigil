//! The plain terminal summary of a review.
//!
//! Plain, deterministic text in five parts: a header (the saved review or
//! why it was not saved, the worktree, base, and target, and whether the
//! review is complete or what changed since its most recent related
//! completion), the gate with one row per item, one line per file, the
//! evidence notes that are not gate items, and the command to run next. No
//! diff text: the JSON carries it.
//!
//! Every string that comes from outside the tool (paths, ids, reasons,
//! names, transcript text) goes through the caller's `escape` function,
//! which the CLI supplies as its `format::escape_control`, so an escape
//! sequence in a path cannot rewrite the terminal.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::{Display, Write as _};
use std::path::{Path, PathBuf};

use supersigil_anchor::ChainClass;
use supersigil_record::observations::{CaptureCounts, CaptureLimitation};
use supersigil_record::reviews::{GateDetail, JudgmentKind, PathBytes, Subject};

use crate::judging::Relation as ReviewRelation;
use crate::model::{
    ANCESTRY_NOTE, AncestryInfo, ChangedEvidence, EvidenceInfo, FileKindInfo, FileReview,
    GateItemInfo, ItemStateInfo, RecordInfo, Review, SpanSide, StatusInfo, TargetKindInfo, Unsaved,
};
use crate::outcome::{Outcome, Relation, UnattributedReason};

/// Most gate rows a summary shows before it suggests a pager.
const LONG_GATE: usize = 20;

/// What a review with no changed file and no gate item says in place of
/// its status: there is nothing to judge or complete.
pub const NOTHING_TO_REVIEW: &str = "Nothing to review: no changes in scope.";

/// Renders `review` as the terminal summary, escaping untrusted text with
/// `escape`. Every command it prints is addressed as `addressing` says,
/// with [`Addressing::note`] after the last; a review with no changed file
/// and no gate item says [`NOTHING_TO_REVIEW`] and suggests no judgment.
#[must_use]
pub fn render_summary(
    review: &Review,
    addressing: &Addressing,
    escape: fn(&str) -> String,
) -> String {
    let mut out = String::new();
    push(&mut out, &header_line(review));
    push(
        &mut out,
        &format!("  worktree: {}", escape(&review.worktree)),
    );
    let base = match &review.base.commit {
        Some(commit) => format!("  base: {commit} (tree {})", review.base.tree),
        None => format!("  base: empty tree {}", review.base.tree),
    };
    push(&mut out, &base);
    let target = match (review.target.kind, &review.target.commit) {
        (TargetKindInfo::Commit, Some(commit)) => format!("  target: commit {commit}"),
        (TargetKindInfo::WorkingTree, Some(commit)) => {
            format!("  target: working tree over {commit}")
        }
        (_, None) => "  target: working tree over an unborn HEAD".to_owned(),
    };
    push(&mut out, &format!("{target} (tree {})", review.target.tree));
    let nothing = review.files.is_empty() && review.gate.items.is_empty();
    if nothing {
        push(&mut out, NOTHING_TO_REVIEW);
    } else {
        for line in status_lines(&review.status, review.gate.open, escape) {
            push(&mut out, &line);
        }
    }

    let gate = &review.gate;
    if gate.items.is_empty() {
        push(&mut out, "Gate: no items.");
    } else {
        let closed = gate.items.len() - gate.open;
        push(
            &mut out,
            &format!("Gate: {} open, {closed} closed.", gate.open),
        );
        push(&mut out, "");
        for row in gate_rows(review, escape) {
            push(&mut out, &format!("  {row}"));
        }
    }

    push(&mut out, "");
    push(&mut out, "files:");
    if review.files.is_empty() {
        push(
            &mut out,
            &escape(&no_changes_line(review.target.kind, addressing)),
        );
    }
    for file in &review.files {
        push(&mut out, &format!("  {}", file_line(review, file, escape)));
    }

    let notes = notes(review, escape);
    if !notes.is_empty() {
        push(&mut out, "");
        push(&mut out, "Not gate items:");
        for note in notes {
            push(&mut out, &format!("  {note}"));
        }
    }

    let mut last = if nothing {
        Vec::new()
    } else if review.basis.is_none() {
        if saves_later(review) {
            vec![escape(&format!(
                "Next: {}",
                retry_command(review, addressing)
            ))]
        } else {
            Vec::new()
        }
    } else {
        let open: Vec<&str> = gate
            .items
            .iter()
            .filter(|item| item.state.is_open())
            .map(|item| item.name.as_str())
            .collect();
        next_lines(&open, review.status.complete, addressing, escape)
    };
    // The note on values shown as given follows every command printed, the
    // file list's suggestion to review a commit included.
    let suggests_commit =
        review.files.is_empty() && review.target.kind == TargetKindInfo::WorkingTree;
    if !last.is_empty() || suggests_commit {
        last.extend(addressing.note());
    }
    if gate.items.len() > LONG_GATE {
        last.push(
            "Long output: run the same review command with --format terminal and pipe it to \
             less."
                .to_owned(),
        );
    }
    if !last.is_empty() {
        push(&mut out, "");
        for line in last {
            push(&mut out, &line);
        }
    }
    out
}

/// The first line: the saved review by its id's first 8 digits, what it
/// reviewed over which base, or why it was not saved.
fn header_line(review: &Review) -> String {
    let Some(saved) = &review.basis else {
        let reason = review
            .basis_unsaved
            .map_or_else(String::new, |unsaved| format!(": {}", unsaved.reason()));
        let then = if saves_later(review) {
            "; run supersigil review again"
        } else {
            ""
        };
        return format!("Review not saved{reason}. Judging needs a saved review{then}.");
    };
    let target = review
        .target
        .commit
        .as_deref()
        .filter(|_| matches!(review.target.kind, TargetKindInfo::Commit));
    format!(
        "Review {}: {}.",
        saved.id.short(),
        compared(target, review.base.commit.as_deref())
    )
}

/// What a review compared, as its summary's first line says it: the
/// working tree, or `commit <short id>` when `target_commit` names the
/// reviewed commit, over the base commit's short id or the empty tree.
#[must_use]
pub fn compared(target_commit: Option<&str>, base_commit: Option<&str>) -> String {
    let target = target_commit.map_or_else(
        || "working tree".to_owned(),
        |commit| format!("commit {}", short(commit)),
    );
    format!(
        "{target} over {}",
        base_commit.map_or("the empty tree", short)
    )
}

/// The command that reviews again what `review` shows: its base, a commit
/// target, the untracked files it included, and its path filter, then the
/// addressing flags.
fn retry_command(review: &Review, addressing: &Addressing) -> String {
    let mut words = Vec::new();
    if let Some(base) = &review.base.commit {
        words.push(format!("--base={}", shell_word(base)));
    }
    if review.target.kind == TargetKindInfo::Commit
        && let Some(target) = &review.target.commit
    {
        words.push(format!("--target={}", shell_word(target)));
    }
    for path in &review.scope.include_untracked {
        words.push(format!("--include-untracked={}", shell_word(path)));
    }
    let mut command = addressing.review(&words.join(" "));
    if !review.scope.paths.is_empty() {
        command.push_str(" --");
        for path in &review.scope.paths {
            command.push(' ');
            command.push_str(&shell_word(path));
        }
    }
    command
}

/// Whether running the review again may save it: unless it names a path
/// the review store cannot hold.
fn saves_later(review: &Review) -> bool {
    review.basis_unsaved.is_none_or(Unsaved::may_save_later)
}

/// A commit id's first 7 digits, as git abbreviates it.
fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

/// Whether the review is complete: by whom and when, and through which
/// review when its completion carries over. Otherwise what changed since
/// the most recent related completion still standing (the first related
/// review, when it has one), and that nothing is open when nothing is.
fn status_lines(status: &StatusInfo, open: usize, escape: fn(&str) -> String) -> Vec<String> {
    if let Some(completion) = &status.completion {
        let by = format!(
            "Review complete: completed on {} by {}",
            completion.at.date(),
            escape(&completion.who.name)
        );
        let carried = status
            .related
            .iter()
            .any(|related| related.review == completion.review);
        return vec![if carried {
            format!(
                "{by} for the same content (review {}).",
                completion.review.short()
            )
        } else {
            format!("{by}.")
        }];
    }
    let not_complete = "Review not complete.";
    let Some((related, completion)) = status
        .related
        .first()
        .and_then(|related| Some((related, related.completion.as_ref()?)))
    else {
        return vec![not_complete.to_owned()];
    };
    let since = format!(
        "since your completion on {} (review {})",
        completion.at.date(),
        related.review.short()
    );
    let mut lines = Vec::new();
    if related.relation == ReviewRelation::CodeChanged {
        lines.push(format!("Code changed {since}:"));
        match &related.changed_paths {
            Some(paths) => {
                lines.extend(
                    paths
                        .iter()
                        .map(|path| format!("  {}", shown_path(path, escape))),
                );
            }
            None => lines.push(
                "  changed paths unavailable: the reviewed snapshot is no longer in the \
                 repository"
                    .to_owned(),
            ),
        }
    }
    let evidence = evidence_lines(&related.changed_evidence, escape);
    if !evidence.is_empty() {
        lines.push(format!("Evidence changed {since}:"));
        lines.extend(evidence);
    }
    if related.relation == ReviewRelation::SameContent && !related.uncovered.is_empty() {
        lines.push(format!(
            "Completed on {} for the same content, but these gate items differ from that review:",
            completion.at.date()
        ));
        lines.extend(
            related
                .uncovered
                .iter()
                .map(|item| format!("  {}: {}", escape(&item.name), item.reasons.join("; "))),
        );
    }
    match lines.first_mut() {
        Some(first) => *first = format!("{not_complete} {first}"),
        None => lines.push(not_complete.to_owned()),
    }
    if open == 0 {
        lines.push("No open gate items. This snapshot has not been marked complete.".to_owned());
    }
    lines
}

/// One line per kind of evidence that changed, naming what changed: the
/// files whose explanations differ, the sessions with new observations and
/// the paths they name, the transcripts whose capture limitations changed,
/// and the records gone or rewritten.
fn evidence_lines(evidence: &ChangedEvidence, escape: fn(&str) -> String) -> Vec<String> {
    let paths = |paths: &[PathBytes]| {
        paths
            .iter()
            .map(|path| shown_path(path, escape))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines = Vec::new();
    if !evidence.explanations.is_empty() {
        lines.push(format!(
            "  explanations differ: {}",
            paths(&evidence.explanations)
        ));
    }
    if !evidence.new.is_empty() {
        let sessions: Vec<String> = evidence
            .new
            .iter()
            .map(|new| format!("session {}: {}", escape(&new.session), paths(&new.paths)))
            .collect();
        lines.push(format!("  new: {}", sessions.join("; ")));
    }
    if !evidence.capture.is_empty() {
        let transcripts: Vec<String> = evidence.capture.iter().map(|t| escape(t)).collect();
        lines.push(format!(
            "  capture limitations changed: {}",
            transcripts.join(", ")
        ));
    }
    if !evidence.gone_or_rewritten.is_empty() {
        let records: Vec<String> = evidence
            .gone_or_rewritten
            .iter()
            .map(|record| escape(record.as_str()))
            .collect();
        lines.push(format!("  gone or rewritten: {}", records.join(", ")));
    }
    lines
}

/// A repository path as a terminal shows it.
fn shown_path(path: &PathBytes, escape: fn(&str) -> String) -> String {
    escape(&String::from_utf8_lossy(path.as_bytes()))
}

/// One row per gate item, in gate order, with its columns aligned: its
/// name, its state, and what needs attention or who closed it.
fn gate_rows(review: &Review, escape: fn(&str) -> String) -> Vec<String> {
    let incomplete: BTreeMap<&PathBytes, usize> = review
        .files
        .iter()
        .map(|file| (&file.path_bytes, incomplete_content_matches(file)))
        .collect();
    aligned(
        review
            .gate
            .items
            .iter()
            .map(|item| {
                let incomplete = match &item.subject {
                    Subject::Path(path) => incomplete.get(path).copied().unwrap_or_default(),
                    _ => 0,
                };
                gate_row(item, incomplete, escape)
            })
            .collect(),
    )
}

/// The columns of `item`'s gate row, as a review summary shows them: its
/// name, its state, and what needs attention or who closed it.
/// `incomplete` counts the item's content-matched lines whose search did
/// not complete. Untrusted text is escaped with `escape`.
#[must_use]
pub fn gate_row(item: &GateItemInfo, incomplete: usize, escape: fn(&str) -> String) -> [String; 3] {
    [
        escape(&item.name),
        state_word(item).to_owned(),
        attention(item, incomplete, escape),
    ]
}

/// `rows` as lines, each column but the last padded to its widest cell and
/// two spaces apart, with no trailing spaces.
#[must_use]
pub fn aligned<const N: usize>(rows: Vec<[String; N]>) -> Vec<String> {
    let mut widths = [0; N];
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    rows.into_iter()
        .map(|row| {
            let mut line = String::new();
            for (index, (cell, width)) in row.iter().zip(widths).enumerate() {
                if index + 1 == N {
                    line.push_str(cell);
                } else {
                    let _ = write!(line, "{cell:<width$}  ");
                }
            }
            line.truncate(line.trim_end().len());
            line
        })
        .collect()
}

/// The state a row shows: a carried item shows how the judgment carried to
/// it closed it.
fn state_word(item: &GateItemInfo) -> &'static str {
    match item.state {
        ItemStateInfo::Open => "open",
        ItemStateInfo::Acknowledged => "acknowledged",
        ItemStateInfo::Dismissed => "dismissed",
        ItemStateInfo::Reopened => "reopened",
        ItemStateInfo::Carried => match item.judgment.as_ref().map(|judgment| judgment.kind) {
            Some(JudgmentKind::Dismiss) => "dismissed",
            _ => "acknowledged",
        },
    }
}

/// What a row says after the state: what needs attention for an item that
/// needs a judgment, who closed a closed one and when, and why a reopened
/// one reopened. `incomplete` counts the item's content-matched lines whose
/// search did not complete.
fn attention(item: &GateItemInfo, incomplete: usize, escape: fn(&str) -> String) -> String {
    let Some(judgment) = &item.judgment else {
        return needs(&item.detail, incomplete as u64, escape);
    };
    let by = format!(
        "by {} on {}",
        escape(&judgment.who.name),
        judgment.at.date()
    );
    match item.state {
        ItemStateInfo::Carried => {
            format!("carried from review {}, {by}", judgment.review.short())
        }
        ItemStateInfo::Reopened => {
            let what = match judgment.kind {
                JudgmentKind::Dismiss => "dismissal",
                _ => "acknowledgment",
            };
            format!("{} since your {what}", item.reopen_reasons.join("; "))
        }
        _ => by,
    }
}

/// What needs attention in an item with `detail`, in the words of its
/// origin. `incomplete` counts its content-matched lines whose search did
/// not complete.
fn needs(detail: &GateDetail, incomplete: u64, escape: fn(&str) -> String) -> String {
    match detail {
        GateDetail::UnexplainedLines {
            unattributed,
            content_match_incomplete,
            unresolved,
            ambiguous,
            line_ending,
            content_match_only,
        } => {
            let total = unattributed
                + content_match_incomplete
                + unresolved
                + ambiguous
                + line_ending
                + content_match_only;
            let incomplete = incomplete.min(*content_match_only);
            let parts = count_phrases([
                (*unattributed, "unattributed"),
                (
                    *content_match_incomplete,
                    "unattributed, content matching incomplete",
                ),
                (*unresolved, "unresolved (search incomplete)"),
                (*ambiguous, "ambiguous"),
                (*line_ending, "line ending changed"),
                (content_match_only - incomplete, "content match only"),
                (incomplete, "content match only (search incomplete)"),
            ]);
            let lines = if total == 1 { "line" } else { "lines" };
            format!("{total} unexplained {lines}: {}", parts.join(", "))
        }
        GateDetail::NotDiffed { kind } => format!("not diffed: {}", escape(kind)),
        GateDetail::UntrackedRecorded { .. } => {
            "untracked, a recorded edit names it; not included".to_owned()
        }
        GateDetail::NotCaptured { cause, .. } => {
            format!("on-disk state not captured: {}", escape(cause))
        }
        GateDetail::CommitWithoutOrigin => "commit has no recorded origin".to_owned(),
        GateDetail::Discontinuity { gaps } => {
            let gaps_word = if *gaps == 1 { "gap" } else { "gaps" };
            format!("{gaps} {gaps_word} between recorded edits")
        }
        GateDetail::Unreconciled { reason } => format!("evidence not read: {}", escape(reason)),
    }
}

/// What the file list says when the review has no changed file; a working
/// tree without changes may have just been committed, so it suggests
/// reviewing that commit, addressed as `addressing` says.
fn no_changes_line(target: TargetKindInfo, addressing: &Addressing) -> String {
    match target {
        TargetKindInfo::WorkingTree => format!(
            "  no changes in scope; to review a commit just made, run {}",
            addressing.review("--target HEAD")
        ),
        TargetKindInfo::Commit => "  no changes in scope".to_owned(),
    }
}

/// What a printed command shows in place of a flag value that cannot be
/// printed exactly.
const AS_GIVEN: &str = "<as given>";

/// How the commands an output prints address what it shows: `judge`
/// commands name the review when it is not the worktree's selection, and
/// every command repeats the `--checkout` and `--review-dir` the call was
/// given. A value that is not valid UTF-8 or holds a control character
/// cannot be printed exactly, and a lossy or escaped rendering could name
/// another path, so commands show `<as given>` in its place; an output
/// whose commands do adds [`Addressing::note`] once, after them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Addressing {
    /// The review's id, its first 8 digits, when `judge` would otherwise
    /// apply to another review: an agent session's review, or a `--review`
    /// override.
    pub review: Option<String>,
    /// `--checkout` as the call was given it.
    pub checkout: Option<PathBuf>,
    /// `--review-dir` as the call was given it.
    pub review_dir: Option<PathBuf>,
}

impl Addressing {
    /// `supersigil judge <verb>` with the items `names`, each quoted with
    /// [`shell_word`], then `options`, printed as given, and the flags that
    /// address the review. When a name starts with `-`, the names come
    /// last, after `--`, so `judge` reads none of them as a flag.
    #[must_use]
    pub fn judge(&self, verb: &str, names: &[&str], options: &str) -> String {
        let dashed = names.iter().any(|name| name.starts_with('-'));
        let names = names
            .iter()
            .map(|name| shell_word(name))
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = format!("supersigil judge {verb}");
        if !dashed {
            append(&mut command, &names);
        }
        append(&mut command, options);
        append_flags(
            &mut command,
            &[("--review", self.review.as_deref().map(OsStr::new))],
        );
        append_flags(&mut command, &self.paths());
        if dashed {
            append(&mut command, "--");
            append(&mut command, &names);
        }
        command
    }

    /// `supersigil review <words>` followed by the `--checkout` and
    /// `--review-dir` the call was given. `words` are printed as given.
    #[must_use]
    pub fn review(&self, words: &str) -> String {
        let mut command = "supersigil review".to_owned();
        append(&mut command, words);
        append_flags(&mut command, &self.paths());
        command
    }

    /// The line an output adds once, after the commands it prints, when
    /// they show the `--checkout` or `--review-dir` value as `<as given>`:
    /// which values cannot be printed, and that they must be passed as
    /// given. `None` when every value prints exactly.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        let flags: Vec<&str> = self
            .paths()
            .into_iter()
            .filter(|(_, value)| value.is_some_and(|value| printed(value).is_none()))
            .map(|(flag, _)| flag)
            .collect();
        match flags.as_slice() {
            [] => None,
            [flag] => Some(format!(
                "The {flag} value is not valid UTF-8 or holds a control character, so commands \
                 show {AS_GIVEN} for it: pass it as given."
            )),
            flags => Some(format!(
                "The {} values are not valid UTF-8 or hold a control character, so commands \
                 show {AS_GIVEN} for them: pass them as given.",
                flags.join(" and ")
            )),
        }
    }

    /// The `--checkout` and `--review-dir` flags with the values the call
    /// gave them.
    fn paths(&self) -> [(&'static str, Option<&OsStr>); 2] {
        [
            ("--checkout", self.checkout.as_deref().map(Path::as_os_str)),
            (
                "--review-dir",
                self.review_dir.as_deref().map(Path::as_os_str),
            ),
        ]
    }
}

/// Appends a space and `words` to `command`, unless `words` is empty.
fn append(command: &mut String, words: &str) {
    if !words.is_empty() {
        command.push(' ');
        command.push_str(words);
    }
}

/// Appends each flag that has a value as `<flag>=<value>`, the value quoted
/// with [`shell_word`], so a value that starts with `-` is never read as a
/// flag, or shown as `<as given>` when it cannot be printed exactly.
fn append_flags(command: &mut String, flags: &[(&str, Option<&OsStr>)]) {
    for &(flag, value) in flags {
        if let Some(value) = value {
            let value = printed(value).map_or_else(|| AS_GIVEN.to_owned(), shell_word);
            let _ = write!(command, " {flag}={value}");
        }
    }
}

/// `value` as a command prints it exactly, or `None` when it cannot be:
/// it is not valid UTF-8, or it holds a control character, which output
/// escapes.
fn printed(value: &OsStr) -> Option<&str> {
    value
        .to_str()
        .filter(|text| !text.chars().any(char::is_control))
}

/// Most open items a `Next:` line names. With more, it leaves them to the
/// list the output shows above it, so the command stays short enough to
/// run.
const MOST_NAMED: usize = 20;

/// The lines naming what to run next on a saved review whose items named
/// `open` need a judgment, each escaped with `escape`: acknowledging them,
/// or dismissing items named in place of a placeholder, then completing
/// the review; completing it when none is open; nothing when it is
/// complete. The names print once, so a long gate's lines stay readable;
/// with more than 20, the acknowledgment takes a placeholder too and points
/// at the open items listed above.
#[must_use]
pub fn next_lines(
    open: &[&str],
    complete: bool,
    addressing: &Addressing,
    escape: fn(&str) -> String,
) -> Vec<String> {
    let lines = if complete {
        Vec::new()
    } else if open.is_empty() {
        vec![format!("Next: {}", addressing.judge("complete", &[], ""))]
    } else {
        let acknowledge = if open.len() > MOST_NAMED {
            format!(
                "{} ({} open items, listed above)",
                addressing.judge("acknowledge", &[], "<name>..."),
                open.len()
            )
        } else {
            addressing.judge("acknowledge", open, "")
        };
        vec![
            format!("Next: {acknowledge}"),
            format!(
                "  or: {}",
                addressing.judge("dismiss", &[], "<name>... --reason <text>")
            ),
            format!("  then: {}", addressing.judge("complete", &[], "")),
        ]
    };
    lines.iter().map(|line| escape(line)).collect()
}

/// `word` as a POSIX shell reads it back: as it is when no character in it
/// is special to a shell, single-quoted otherwise.
#[must_use]
pub fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| !c.is_ascii() || c.is_ascii_alphanumeric() || "_-./:@%+=,".contains(c));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// Header lines shared with `why`: records and checkouts not reconciled,
/// then one line per candidate transcript with capture limitations.
pub(crate) fn context_lines(
    records: &[RecordInfo],
    evidence: &EvidenceInfo,
    escape: fn(&str) -> String,
) -> Vec<String> {
    let mut lines = unreconciled_lines(records, evidence, escape);
    lines.extend(capture_lines(evidence, escape));
    lines
}

/// One line per record and per checkout not reconciled, with why.
fn unreconciled_lines(
    records: &[RecordInfo],
    evidence: &EvidenceInfo,
    escape: fn(&str) -> String,
) -> Vec<String> {
    let mut lines = Vec::new();
    for record in records.iter().filter(|record| !record.reconciled) {
        lines.push(format!(
            "record {} not reconciled: {}",
            escape(&record.id),
            escape(record.reason.as_deref().unwrap_or("unknown reason"))
        ));
    }
    for checkout in &evidence.unreconciled_checkouts {
        lines.push(format!(
            "checkout {} not reconciled ({} transcripts): {}",
            escape(&checkout.checkout),
            checkout.transcripts,
            escape(&checkout.reason)
        ));
    }
    lines
}

/// One line per candidate transcript with capture limitations.
fn capture_lines(evidence: &EvidenceInfo, escape: fn(&str) -> String) -> Vec<String> {
    let mut lines = Vec::new();
    for transcript in &evidence.candidate_transcripts {
        let counts = total_counts(&transcript.capture_limitations);
        let words = count_words(&counts);
        if !words.is_empty() {
            lines.push(format!(
                "capture limitations in {} (session {}): {}; not localized to a path",
                escape(&transcript.transcript),
                escape(&transcript.session),
                words.join(", ")
            ));
        }
    }
    lines
}

/// Lines of `file` with a content match whose search did not complete. The
/// gate's counts do not tell them apart, so they come from the spans.
fn incomplete_content_matches(file: &FileReview) -> usize {
    file.hunks
        .iter()
        .flat_map(|hunk| &hunk.spans)
        .filter(|span| {
            matches!(
                span.outcome,
                Outcome::ContentMatch {
                    search_complete: false,
                    ..
                }
            )
        })
        .map(|span| span.count)
        .sum()
}

/// One line per file: status, path, line counts, and attribution in words.
fn file_line(review: &Review, file: &FileReview, escape: fn(&str) -> String) -> String {
    let path = escape(&file.path);
    let letter = file.status.letter();
    if file.kind != FileKindInfo::Text {
        return format!("{letter} {path}  not diffed: {}", file.kind.as_str());
    }
    let added: usize = file.hunks.iter().map(|hunk| hunk.target_count).sum();
    let removed: usize = file.hunks.iter().map(|hunk| hunk.base_count).sum();
    let mut introduced = 0;
    let mut replaced = 0;
    let mut other = Tally::default();
    let mut classes = BTreeSet::new();
    let mut sessions = BTreeSet::new();
    for span in file.hunks.iter().flat_map(|hunk| &hunk.spans) {
        match &span.outcome {
            Outcome::Attributed { edits } => {
                match span.side {
                    SpanSide::Target => introduced += span.count,
                    SpanSide::Base => replaced += span.count,
                }
                for edit in edits {
                    classes.extend(edit.classes.iter().copied());
                    if let Some(info) = review.edits.get(edit.edit.as_str()) {
                        sessions.insert(info.session.clone());
                    }
                }
            }
            outcome => other.add(outcome, span.count),
        }
    }
    let mut parts = Vec::new();
    if introduced > 0 {
        parts.push(format!("{introduced} introduced"));
    }
    if replaced > 0 {
        parts.push(format!("{replaced} replaced"));
    }
    if let Some(class) = classes.iter().min() {
        parts.push(class_words(*class).to_owned());
        let count = sessions.len();
        parts.push(format!(
            "{count} {}",
            if count == 1 { "session" } else { "sessions" }
        ));
    }
    parts.extend(other.words(escape));
    if other.unattributed > 0 && !file.command_changes.is_empty() {
        let commands = file.command_changes.iter().map(|c| c.command.as_str());
        parts.push(format!("changed by {}", recorded_commands(commands)));
    }
    if let Some(info) = file
        .attribution
        .as_ref()
        .and_then(|a| a.unavailable.as_ref())
    {
        parts.push(format!("attribution unavailable: {}", escape(info)));
    }
    if file.coarse {
        parts.push("coarse diff".to_owned());
    }
    let words = if parts.is_empty() {
        "no line changes".to_owned()
    } else {
        parts.join(", ")
    };
    format!("{letter} {path}  +{added} -{removed}  {words}")
}

/// `"1 recorded command"` or `"{count} recorded commands"`. Counts each
/// id in `commands` once: a command can appear once per reported file,
/// per worktree, for a path.
pub(crate) fn recorded_commands<'a>(commands: impl IntoIterator<Item = &'a str>) -> String {
    let count = commands.into_iter().collect::<BTreeSet<_>>().len();
    if count == 1 {
        "1 recorded command".to_owned()
    } else {
        format!("{count} recorded commands")
    }
}

/// The first line of a command's text, cut to 100 characters, with how many
/// lines follow it.
pub(crate) fn command_line(text: &str, escape: fn(&str) -> String) -> String {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    let mut shown: String = first.chars().take(100).collect();
    if shown.len() < first.len() {
        shown.push('…');
    }
    let mut line = escape(&shown);
    match lines.count() {
        0 => {}
        1 => line.push_str(" … (1 more line)"),
        more => {
            let _ = write!(line, " … ({more} more lines)");
        }
    }
    line
}

/// Counts of non-attributed outcomes on one file, and the first
/// unattributed reason.
#[derive(Default)]
struct Tally {
    whitespace: usize,
    realigned: usize,
    line_ending: usize,
    content_match: usize,
    content_match_search_incomplete: usize,
    ambiguous: usize,
    unresolved: usize,
    unattributed: usize,
    reason: Option<UnattributedReason>,
}

impl Tally {
    fn add(&mut self, outcome: &Outcome, count: usize) {
        match outcome {
            Outcome::WhitespaceOnly { .. } => self.whitespace += count,
            Outcome::Realigned { .. } => self.realigned += count,
            Outcome::LineEndingChanged => self.line_ending += count,
            Outcome::ContentMatch {
                search_complete: true,
                ..
            } => self.content_match += count,
            Outcome::ContentMatch {
                search_complete: false,
                ..
            } => self.content_match_search_incomplete += count,
            Outcome::Ambiguous { .. } => self.ambiguous += count,
            Outcome::Unresolved { .. } => self.unresolved += count,
            Outcome::Unattributed { reason } => {
                self.unattributed += count;
                self.reason.get_or_insert_with(|| reason.clone());
            }
            Outcome::Attributed { .. } => {}
        }
    }

    fn words(&self, escape: fn(&str) -> String) -> Vec<String> {
        let mut words = count_phrases([
            (self.whitespace, "whitespace-only"),
            (self.realigned, "realigned"),
            (self.line_ending, "line ending changed"),
            (self.content_match, content_match_words(true)),
            (
                self.content_match_search_incomplete,
                content_match_words(false),
            ),
            (self.ambiguous, "ambiguous"),
            (self.unresolved, "unresolved (search incomplete)"),
        ]);
        if self.unattributed > 0 {
            let reason = self.reason.as_ref().map_or_else(String::new, |reason| {
                format!(": {}", reason_words(reason, escape))
            });
            words.push(format!("{} unattributed{reason}", self.unattributed));
        }
        words
    }
}

/// The evidence notes that are not gate items: coarse diffs, attribution
/// unavailable, capture limitations, conflicting edits, transcripts removed
/// before they could be read, scope and origin notes, and what was not
/// reconciled.
fn notes(review: &Review, escape: fn(&str) -> String) -> Vec<String> {
    let summary = &review.unattributed;
    let evidence = &review.evidence;
    let mut notes = Vec::new();
    for path in &summary.coarse {
        notes.push(format!("{}: coarse diff, too large to align", escape(path)));
    }
    for path in &summary.attribution_unavailable {
        notes.push(format!("{}: attribution unavailable", escape(path)));
    }
    notes.extend(capture_lines(evidence, escape));
    for conflict in &evidence.conflicting_edits {
        let records: Vec<String> = conflict
            .records
            .iter()
            .map(|record| escape(record.as_str()))
            .collect();
        notes.push(format!(
            "edit {} excluded: its sightings in records {} disagree",
            escape(conflict.edit.as_str()),
            records.join(", ")
        ));
    }
    for transcript in &evidence.removed_transcripts {
        notes.push(format!(
            "transcript {} removed before it could be read",
            escape(transcript)
        ));
    }
    notes.extend(scope_notes(review, escape));
    notes.extend(unreconciled_lines(&review.records, evidence, escape));
    notes
}

/// Notes on the review's scope and on the origins of its commits.
fn scope_notes(review: &Review, escape: fn(&str) -> String) -> Vec<String> {
    let scope = &review.scope;
    let mut notes = Vec::new();
    if !scope.paths.is_empty() {
        let paths: Vec<String> = scope.paths.iter().map(|p| escape(p)).collect();
        notes.push(format!("scope: limited to {}", paths.join(", ")));
    }
    if let Some(note) = &scope.path_note {
        notes.push(format!("scope: {}", escape(note)));
    }
    for path in &scope.include_untracked {
        notes.push(format!("scope: untracked {} included", escape(path)));
    }
    let excluded = scope.untracked_excluded.len();
    if excluded > 0 {
        notes.push(format!("scope: {excluded} untracked files excluded"));
    }
    for entry in &scope.not_captured {
        notes.push(format!(
            "scope: {} not captured ({}, {} on disk)",
            escape(&entry.path),
            entry.cause,
            entry.on_disk
        ));
    }
    if scope.skip_worktree_absent > 0 {
        notes.push(format!(
            "scope: {} skip-worktree paths absent from disk keep their index state",
            scope.skip_worktree_absent
        ));
    }
    for path in &scope.unmerged {
        notes.push(format!(
            "scope: {} unmerged, taken as on disk",
            escape(path)
        ));
    }
    if let Some(note) = &scope.ancestry_note {
        notes.push(format!("scope: {}", escape(note)));
    } else if review.base.ancestry == AncestryInfo::NotAncestor {
        notes.push(format!("scope: {ANCESTRY_NOTE}"));
    }
    let origins = &review.origins;
    let reviewed = origins.commits.len() + origins.without_origin.len();
    if reviewed > 0 {
        notes.push(format!(
            "origins: {reviewed} reviewed commits, {} without an origin worktree",
            origins.without_origin.len()
        ));
    }
    for unavailable in &origins.unavailable {
        notes.push(format!(
            "origins: {}: origin evidence unavailable: {}",
            escape(&unavailable.worktree),
            escape(&unavailable.reason)
        ));
    }
    notes
}

pub(crate) const fn class_words(class: ChainClass) -> &'static str {
    match class {
        ChainClass::ExactFromBase => "exact from the base",
        ChainClass::ExactFromStart => "exact from its start",
        ChainClass::Consistent => "consistent with the recorded edits, not verified",
    }
}

/// What the terminal calls a content match: one whose search ran out of
/// budget may have more covering edits than it names.
pub(crate) const fn content_match_words(search_complete: bool) -> &'static str {
    if search_complete {
        "content match"
    } else {
        "content match (search incomplete)"
    }
}

pub(crate) fn reason_words(reason: &UnattributedReason, escape: fn(&str) -> String) -> String {
    match reason {
        UnattributedReason::NoSurvivingChain { .. } => "no surviving chain".to_owned(),
        UnattributedReason::GapBeforeChain => "gap before the chain's start".to_owned(),
        UnattributedReason::ContentMatchIncomplete => {
            "content matching ran out of budget; a covering edit may exist".to_owned()
        }
        UnattributedReason::AttributionUnavailable { reason } => {
            format!("attribution unavailable: {}", escape(reason))
        }
    }
}

pub(crate) const fn relation_words(relation: Relation) -> &'static str {
    match relation {
        Relation::Introduced => "introduced",
        Relation::Replaced => "replaced",
    }
}

/// Sums the counts of every capture limitation of one transcript. They come
/// from one record and cover disjoint line ranges, so no problem is counted
/// twice.
pub(crate) fn total_counts(limitations: &[CaptureLimitation]) -> CaptureCounts {
    limitations
        .iter()
        .map(|limitation| &limitation.counts)
        .sum()
}

fn count_words(counts: &CaptureCounts) -> Vec<String> {
    let unknown: u64 = counts.unknown_records.values().sum();
    count_phrases([
        (unknown, "unknown records"),
        (counts.malformed_lines, "malformed lines"),
        (counts.abandoned_tool_uses, "tool uses without a result"),
        (counts.failed_tool_uses, "failed editing tool uses"),
        (
            counts.outside_checkout,
            "edits or commands outside the checkout",
        ),
        (counts.conflicting_tool_results, "conflicting tool results"),
        (counts.unmatched_tool_results, "unmatched tool results"),
        (counts.session_mismatch, "records of another session"),
        (counts.unnamed_tool_uses, "tool uses without an id"),
        (counts.unsupported_tool_uses, "unsupported tool uses"),
        (counts.unconfirmed_shell_writes, "unconfirmed shell writes"),
    ])
}

/// `"{count} {label}"` for each pair whose count is not zero.
pub(crate) fn count_phrases<'a, T: Display + Default + PartialEq>(
    items: impl IntoIterator<Item = (T, &'a str)>,
) -> Vec<String> {
    items
        .into_iter()
        .filter(|(count, _)| *count != T::default())
        .map(|(count, label)| format!("{count} {label}"))
        .collect()
}

/// Appends `line` and a newline to `out`.
pub(crate) fn push(out: &mut String, line: &str) {
    let _ = writeln!(out, "{line}");
}
