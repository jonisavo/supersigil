//! The plain terminal summary of a review.
//!
//! Deterministic text with a header, gate rows, file rows, evidence notes,
//! and next commands. The header identifies the saved review or why saving
//! failed, worktree, base, target, and completion status or changes since the
//! most recent related completion. Diff text remains in JSON.
//!
//! Every string that comes from outside the tool (paths, ids, reasons,
//! names, transcript text) uses the caller's `escape` function. The CLI
//! supplies `format::escape_control` so path escape sequences cannot rewrite
//! the terminal.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::{self, Display, Write as _};
use std::path::PathBuf;

use supersigil_anchor::ChainClass;
use supersigil_record::observations::{CaptureCounts, CaptureLimitation};
use supersigil_record::reviews::{BasisId, GateDetail, JudgmentKind, PathBytes, Subject};

use crate::judging::Relation as ReviewRelation;
use crate::model::{
    ANCESTRY_NOTE, AncestryInfo, ChangedEvidence, EvidenceInfo, FileKindInfo, FileReview, GateInfo,
    GateItemInfo, ItemStateInfo, RecordInfo, Review, SpanSide, StatusInfo, TargetKindInfo, Unsaved,
};
use crate::outcome::{Outcome, Relation, UnattributedReason};

/// Maximum gate rows before the summary suggests a pager.
const LONG_GATE: usize = 20;

/// Status text when both the changed-file list and gate are empty, with
/// nothing to judge or complete.
pub const NOTHING_TO_REVIEW: &str = "Nothing to review: no changes in scope.";

/// Renders `review` as the terminal summary, escaping untrusted text with
/// `escape`. Addresses commands using `addressing` and adds
/// [`Addressing::note`] after the last. With no changed files and no gate
/// items, prints [`NOTHING_TO_REVIEW`] and suggests no judgment.
#[must_use]
pub fn render_summary(
    review: &Review,
    addressing: &Addressing,
    escape: fn(&str) -> String,
) -> String {
    let mut out = String::new();
    push(&mut out, &header_line(review));
    push(&mut out, &worktree_line(&review.worktree, escape));
    push(
        &mut out,
        &base_line(review.base.commit.as_deref(), &review.base.tree),
    );
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

    for line in gate(review, escape) {
        push(&mut out, &line);
    }

    // Collect commands for the note after the last.
    let mut commands = Vec::new();
    push(&mut out, "");
    push(&mut out, "files:");
    if review.files.is_empty() {
        let (line, command) = no_changes_line(review.target.kind, addressing);
        push(&mut out, &escape(&line));
        commands.extend(command);
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
            let retry = addressing.retry();
            let line = escape(&format!("Next: {retry}"));
            commands.push(retry);
            vec![line]
        } else {
            Vec::new()
        }
    } else {
        let open = review.gate.open_names();
        let (next, next_commands) = next_lines(&open, review.status.complete, addressing, escape);
        commands.extend(next_commands);
        next
    };
    // Add the note after all commands, including the file list's commit review.
    last.extend(addressing.note(&commands));
    if review.gate.items.len() > LONG_GATE {
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
    review_line(&saved.id, &compared(target, review.base.commit.as_deref()))
}

/// Builds a saved review's first header line from `id`'s first 8 digits and
/// its `compared` description ([`compared`]).
#[must_use]
pub fn review_line(id: &BasisId, compared: &str) -> String {
    format!("Review {}: {compared}.", id.short())
}

/// Builds the worktree header line, escaping `worktree` with `escape`.
#[must_use]
pub fn worktree_line(worktree: &str, escape: fn(&str) -> String) -> String {
    format!("  worktree: {}", escape(worktree))
}

/// Builds the base header line with `commit` and `tree`, or the empty tree.
#[must_use]
pub fn base_line(commit: Option<&str>, tree: &str) -> String {
    match commit {
        Some(commit) => format!("  base: {commit} (tree {tree})"),
        None => format!("  base: empty tree {tree}"),
    }
}

/// Describes the working tree or `target_commit` over the base commit or
/// empty tree for the first summary line. Abbreviates commit IDs.
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

/// Checks whether rerunning may save the review, unless its paths cannot
/// serialize in the store.
fn saves_later(review: &Review) -> bool {
    review.basis_unsaved.is_none_or(Unsaved::may_save_later)
}

fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

/// Shows who completed the review and when, naming the earlier review if
/// completion carries. Otherwise shows changes since the first related
/// review's standing completion, if any, and reports an empty open gate.
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

/// Lists changed file explanations, new observations by session and path,
/// changed transcript capture limitations, and gone or rewritten records,
/// one line per kind.
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

/// Displays repository `path` with lossy UTF-8 conversion and `escape`.
#[must_use]
pub fn shown_path(path: &PathBytes, escape: fn(&str) -> String) -> String {
    escape(&String::from_utf8_lossy(path.as_bytes()))
}

fn gate(review: &Review, escape: fn(&str) -> String) -> Vec<String> {
    let incomplete: BTreeMap<&PathBytes, usize> = review
        .files
        .iter()
        .map(|file| (&file.path_bytes, incomplete_content_matches(file)))
        .collect();
    gate_lines(&review.gate, |item| {
        let incomplete = match &item.subject {
            Subject::Path(path) => incomplete.get(path).copied().unwrap_or_default(),
            _ => 0,
        };
        gate_row(item, incomplete, escape)
    })
}

/// Renders `gate`'s open and closed counts, a blank line, and indented item
/// rows in gate order. Aligns columns from `row` with [`aligned`]. For an
/// empty gate, reports no items.
#[must_use]
pub fn gate_lines(gate: &GateInfo, row: impl Fn(&GateItemInfo) -> [String; 3]) -> Vec<String> {
    if gate.items.is_empty() {
        return vec!["Gate: no items.".to_owned()];
    }
    let closed = gate.items.len() - gate.open;
    let mut lines = vec![
        format!("Gate: {} open, {closed} closed.", gate.open),
        String::new(),
    ];
    let rows = aligned(gate.items.iter().map(row).collect());
    lines.extend(rows.into_iter().map(|row| format!("  {row}")));
    lines
}

/// Builds `item`'s name, state, and attention or closing-judgment columns.
/// `incomplete` counts content-matched lines whose search did not complete.
/// Escapes untrusted text with `escape`.
#[must_use]
pub fn gate_row(item: &GateItemInfo, incomplete: usize, escape: fn(&str) -> String) -> [String; 3] {
    [
        escape(&item.name),
        state_word(item).to_owned(),
        attention(item, incomplete, escape),
    ]
}

/// Formats `rows` as lines with two spaces between columns. Pads each
/// column except the last to its widest cell and trims trailing spaces.
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

/// Carried items show the carried judgment's closing kind.
fn state_word(item: &GateItemInfo) -> &'static str {
    match item.state {
        ItemStateInfo::Open => "open",
        ItemStateInfo::Acknowledged => "acknowledged",
        ItemStateInfo::Dismissed => "dismissed",
        ItemStateInfo::Reopened => "reopened",
        ItemStateInfo::Carried => item
            .judgment
            .as_ref()
            .map_or("acknowledged", |judgment| closed_word(judgment.kind)),
    }
}

/// What a judgment of `kind` is called in a sentence.
#[must_use]
pub const fn noun(kind: JudgmentKind) -> &'static str {
    match kind {
        JudgmentKind::Acknowledge => "acknowledgment",
        JudgmentKind::Dismiss => "dismissal",
        JudgmentKind::Comment => "comment",
        JudgmentKind::Complete => "completion",
        JudgmentKind::Undo => "undo",
    }
}

/// Returns `dismissed` for a dismissal and `acknowledged` for any other
/// `kind`. Callers use this for closing judgments.
#[must_use]
pub const fn closed_word(kind: JudgmentKind) -> &'static str {
    match kind {
        JudgmentKind::Dismiss => "dismissed",
        _ => "acknowledged",
    }
}

/// Builds the text after a row's state, describing needed attention, who
/// closed it and when, or why it reopened. `incomplete` counts the item's
/// content-matched lines whose search did not complete.
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
        // Only an acknowledgment or a dismissal reopens an item.
        ItemStateInfo::Reopened => format!(
            "{} since your {}",
            item.reopen_reasons.join("; "),
            noun(judgment.kind)
        ),
        _ => by,
    }
}

/// `incomplete` counts content-matched lines whose search did not complete.
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

/// Builds the empty file-list text and optional command. For a working
/// tree, suggests reviewing a commit just made, using `addressing`.
fn no_changes_line(target: TargetKindInfo, addressing: &Addressing) -> (String, Option<Command>) {
    match target {
        TargetKindInfo::WorkingTree => {
            let command = addressing.review("--target HEAD");
            let line =
                format!("  no changes in scope; to review a commit just made, run {command}");
            (line, Some(command))
        }
        TargetKindInfo::Commit => ("  no changes in scope".to_owned(), None),
    }
}

/// What a printed command shows in place of a flag value that cannot be
/// printed exactly.
const AS_GIVEN: &str = "<as given>";

/// What [`Command`] records for a path filter it shows as `<as given>`.
const PATH_FILTER: &str = "path filter";

/// What [`Command`] records for an item name it shows as `<as given>`.
const ITEM_NAME: &str = "item name";

/// The shells a printed command is written for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Shell {
    /// POSIX shells, which read single-quoted values literally.
    #[default]
    Posix,
    /// Windows shells. `cmd.exe` and PowerShell read quotes, `%`, and some
    /// other characters differently, so a value that needs quoting shows as
    /// `<as given>`: pasting a quoted file name could run part of it.
    Windows,
}

impl Shell {
    /// The shells of the platform this runs on.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }

    /// `word` as a command for these shells shows it: quoted with
    /// [`shell_word`] for POSIX shells; unchanged for Windows shells when
    /// it holds only ASCII letters, digits, and `_-./\:~+` and does not
    /// start with `~`, else `None`.
    #[must_use]
    pub fn word(self, word: &str) -> Option<String> {
        match self {
            Self::Posix => Some(shell_word(word)),
            Self::Windows => {
                let plain = !word.is_empty()
                    && !word.starts_with('~')
                    && word
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || r"_-./\:~+".contains(c));
                plain.then(|| word.to_owned())
            }
        }
    }

    /// Why a value shows as `<as given>`, for one value or several.
    const fn reasons(self) -> (&'static str, &'static str) {
        match self {
            Self::Posix => (
                "is not valid UTF-8 or holds a control character",
                "are not valid UTF-8 or hold a control character",
            ),
            Self::Windows => (
                "is not valid UTF-8, holds a control character, or needs quoting that Windows \
                 shells read differently",
                "are not valid UTF-8, hold a control character, or need quoting that Windows \
                 shells read differently",
            ),
        }
    }
}

/// A command rendered by [`Addressing`], tracking values replaced by
/// `<as given>` so output can add [`Addressing::note`] after all commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The shells it is written for.
    shell: Shell,
    /// The command as printed.
    text: String,
    /// The flag of each value it shows as `<as given>`, or `path filter`
    /// for a path filter, in the order shown.
    as_given: Vec<&'static str>,
}

impl Command {
    /// `program` without arguments, for `shell`.
    fn new(program: &str, shell: Shell) -> Self {
        Self {
            shell,
            text: program.to_owned(),
            as_given: Vec::new(),
        }
    }

    /// Appends a space and `words`, printed as given, unless `words` is
    /// empty.
    fn words(&mut self, words: &str) {
        if !words.is_empty() {
            self.text.push(' ');
            self.text.push_str(words);
        }
    }

    /// Appends `<flag>=<value>` using [`Self::value`] so leading `-` in a
    /// value cannot parse as a flag.
    fn flag(&mut self, flag: &'static str, value: &OsStr) {
        let value = self.value(flag, value);
        self.words(&format!("{flag}={value}"));
    }

    /// Shows `value` as its shells read it ([`Shell::word`]), or
    /// substitutes `<as given>` and records `what` if [`printed`] cannot
    /// represent it exactly or the shells cannot read it safely.
    fn value(&mut self, what: &'static str, value: &OsStr) -> String {
        printed(value)
            .and_then(|text| self.shell.word(text))
            .unwrap_or_else(|| {
                self.as_given.push(what);
                AS_GIVEN.to_owned()
            })
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Addresses printed commands. `judge` commands name the review when it
/// differs from the worktree's selection. Every command repeats the supplied
/// `--checkout` and `--review-dir`, and `review` commands the supplied
/// `--record-dir` and `--claude-home`, which choose the evidence read.
///
/// Commands replace non-UTF-8 values or control characters with `<as given>`
/// because lossy or escaped values could name another path, and for Windows
/// shells also values that need quoting. Output adds [`Addressing::note`]
/// once after commands containing these placeholders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Addressing {
    /// The review's first 8 ID digits for an agent session or `--review`
    /// override, when `judge` would otherwise apply to another review.
    pub review: Option<String>,
    /// `--checkout` as the call was given it.
    pub checkout: Option<PathBuf>,
    /// `--review-dir` as the call was given it.
    pub review_dir: Option<PathBuf>,
    /// `--record-dir` as the `review` call was given it.
    pub record_dir: Option<PathBuf>,
    /// `--claude-home` as the `review` call was given it.
    pub claude_home: Option<PathBuf>,
    /// The flags of the `review` call other than the addressing and
    /// evidence flags, each with the value it was given (`None` for a flag
    /// without one), for the command that runs it again.
    pub retry: Vec<(&'static str, Option<String>)>,
    /// The path filters of the `review` call as it was given them.
    pub retry_paths: Vec<String>,
    /// The shells commands are written for.
    pub shell: Shell,
}

impl Addressing {
    /// Builds `supersigil review` with the supplied arguments. Emits
    /// [`retry`](Self::retry) flags as `<flag>=<value>`, then the evidence,
    /// `--checkout`, and `--review-dir` flags, then `--` and any
    /// [`retry_paths`](Self::retry_paths). Values that cannot print exactly
    /// become `<as given>`.
    #[must_use]
    pub fn retry(&self) -> Command {
        let mut command = Command::new("supersigil review", self.shell);
        for (flag, value) in &self.retry {
            match value {
                None => command.words(flag),
                Some(value) => command.flag(flag, OsStr::new(value)),
            }
        }
        self.evidence(&mut command);
        self.address(&mut command);
        self.paths(&mut command);
        command
    }

    /// Builds `supersigil judge <verb>` with `names` as its shells read them,
    /// then literal `options` and review address flags. If any name starts
    /// with `-`, puts all names last after `--` to prevent flag parsing.
    #[must_use]
    pub fn judge(&self, verb: &str, names: &[&str], options: &str) -> Command {
        let dashed = names.iter().any(|name| name.starts_with('-'));
        let mut command = Command::new(&format!("supersigil judge {verb}"), self.shell);
        let names = names
            .iter()
            .map(|name| command.value(ITEM_NAME, OsStr::new(name)))
            .collect::<Vec<_>>()
            .join(" ");
        if !dashed {
            command.words(&names);
        }
        command.words(options);
        if let Some(review) = &self.review {
            command.flag("--review", OsStr::new(review));
        }
        self.address(&mut command);
        if dashed {
            command.words("--");
            command.words(&names);
        }
        command
    }

    /// Builds `supersigil review <words>` with literal `words`, then the
    /// supplied evidence, `--checkout`, and `--review-dir` flags, then `--`
    /// and any [`retry_paths`](Self::retry_paths).
    #[must_use]
    pub fn review(&self, words: &str) -> Command {
        let mut command = Command::new("supersigil review", self.shell);
        command.words(words);
        self.evidence(&mut command);
        self.address(&mut command);
        self.paths(&mut command);
        command
    }

    /// Builds `supersigil review <flag>=<value>`, then as [`Self::review`].
    #[must_use]
    pub fn review_flag(&self, flag: &'static str, value: &str) -> Command {
        let mut command = Command::new("supersigil review", self.shell);
        command.flag(flag, OsStr::new(value));
        self.evidence(&mut command);
        self.address(&mut command);
        self.paths(&mut command);
        command
    }

    /// Builds the note after printed `commands`, naming values shown as
    /// `<as given>` and telling the reader to pass them unchanged. Returns
    /// `None` when every value prints exactly.
    #[must_use]
    pub fn note<'c>(&self, commands: impl IntoIterator<Item = &'c Command>) -> Option<String> {
        let shown: BTreeSet<&str> = commands
            .into_iter()
            .flat_map(|command| command.as_given.iter().copied())
            .collect();
        // List review address flags, retry flags, other flags, item names,
        // then path filters.
        let order = ["--review", "--checkout", "--review-dir"]
            .into_iter()
            .chain(self.retry.iter().map(|&(flag, _)| flag))
            .chain([
                "--include-untracked",
                "--record-dir",
                "--claude-home",
                ITEM_NAME,
                PATH_FILTER,
            ]);
        let mut flags: Vec<&str> = Vec::new();
        for flag in order {
            if shown.contains(flag) && !flags.contains(&flag) {
                flags.push(flag);
            }
        }
        let (one, several) = self.shell.reasons();
        match flags.as_slice() {
            [] => None,
            [flag] => Some(format!(
                "The {flag} value {one}, so commands show {AS_GIVEN} for it: pass it as given."
            )),
            flags => Some(format!(
                "The {} values {several}, so commands show {AS_GIVEN} for them: pass them as \
                 given.",
                flags.join(" and ")
            )),
        }
    }

    /// Appends `--` and the [`retry_paths`](Self::retry_paths), if any, to
    /// `command`.
    fn paths(&self, command: &mut Command) {
        if !self.retry_paths.is_empty() {
            command.words("--");
            for path in &self.retry_paths {
                let path = command.value(PATH_FILTER, OsStr::new(path));
                command.words(&path);
            }
        }
    }

    /// Appends supplied `--record-dir` and `--claude-home` values to `command`.
    fn evidence(&self, command: &mut Command) {
        let flags = [
            ("--record-dir", &self.record_dir),
            ("--claude-home", &self.claude_home),
        ];
        for (flag, value) in flags {
            if let Some(value) = value {
                command.flag(flag, value.as_os_str());
            }
        }
    }

    /// Appends supplied `--checkout` and `--review-dir` values to `command`.
    fn address(&self, command: &mut Command) {
        let flags = [
            ("--checkout", &self.checkout),
            ("--review-dir", &self.review_dir),
        ];
        for (flag, value) in flags {
            if let Some(value) = value {
                command.flag(flag, value.as_os_str());
            }
        }
    }
}

/// Returns `value`'s exact text for commands or names, or `None` for
/// non-UTF-8 values or control characters that output would escape.
#[must_use]
pub fn printed(value: &OsStr) -> Option<&str> {
    value
        .to_str()
        .filter(|text| !text.chars().any(char::is_control))
}

/// Maximum open items named in a `Next:` command before using a placeholder
/// that refers to the item list above.
const MOST_NAMED: usize = 20;

/// Builds next-step lines and their commands for a saved review, escaping
/// lines with `escape`. Suggests acknowledging `open` items or dismissing
/// placeholder names, then completing; suggests only completion with no open
/// items; emits nothing for a complete review. Prints item names once to keep
/// long gates readable. Above 20 open items, acknowledgment also uses a
/// placeholder and refers to the item list above.
#[must_use]
pub fn next_lines(
    open: &[&str],
    complete: bool,
    addressing: &Addressing,
    escape: fn(&str) -> String,
) -> (Vec<String>, Vec<Command>) {
    let (lines, commands) = if complete {
        (Vec::new(), Vec::new())
    } else if open.is_empty() {
        let complete = addressing.judge("complete", &[], "");
        (vec![format!("Next: {complete}")], vec![complete])
    } else {
        let (acknowledge, shown) = if open.len() > MOST_NAMED {
            let acknowledge = addressing.judge("acknowledge", &[], "<name>...");
            let shown = format!("{acknowledge} ({} open items, listed above)", open.len());
            (acknowledge, shown)
        } else {
            let acknowledge = addressing.judge("acknowledge", open, "");
            let shown = acknowledge.to_string();
            (acknowledge, shown)
        };
        let dismiss = addressing.judge("dismiss", &[], "<name>... --reason <text>");
        let complete = addressing.judge("complete", &[], "");
        let lines = vec![
            format!("Next: {shown}"),
            format!("  or: {dismiss}"),
            format!("  then: {complete}"),
        ];
        (lines, vec![acknowledge, dismiss, complete])
    };
    (lines.iter().map(|line| escape(line)).collect(), commands)
}

/// Quotes `word` for a POSIX shell, leaving safe nonempty words unchanged
/// and single-quoting the rest.
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

/// Counts `file`'s content-matched lines with incomplete searches from its
/// spans, since gate counts do not separate them.
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

/// Builds evidence notes outside gate items for coarse diffs, unavailable
/// attribution, capture limitations, conflicting edits, removed transcripts,
/// scope, origins, and unreconciled evidence.
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
