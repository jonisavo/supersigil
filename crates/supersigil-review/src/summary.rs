//! The plain terminal summary of a review.
//!
//! Deterministic text with a header, gate rows, file rows, evidence notes,
//! and next commands. The header identifies the saved review or why saving
//! failed, worktree, base, target, and completion status or changes since the
//! most recent related completion. Diff text remains in JSON.
//!
//! Lines are built from untrusted text (paths, ids, reasons, names,
//! transcript text) as is; [`Printed`] escapes each line once, so a path's
//! escape sequences cannot rewrite the terminal.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Write as _};

use supersigil_anchor::ChainClass;
use supersigil_record::observations::{CaptureCounts, CaptureLimitation};
use supersigil_record::reviews::{BasisId, GateDetail, JudgmentKind, PathBytes, Subject};

use crate::judging::Relation as ReviewRelation;
use crate::model::{
    ANCESTRY_NOTE, AncestryInfo, ChangedEvidence, CompletionSource, EvidenceInfo, FileKindInfo,
    FileReview, GateInfo, GateItemInfo, ItemStateInfo, RecordInfo, Review, SpanSide, StatusInfo,
    TargetKindInfo, Unsaved,
};
use crate::outcome::{Outcome, Relation, UnattributedReason};
use crate::printed::{Addressing, Printed, aligned};

/// Maximum gate rows before the summary suggests a pager.
const LONG_GATE: usize = 20;

/// Status text when both the changed-file list and gate are empty, with
/// nothing to judge or complete.
pub const NOTHING_TO_REVIEW: &str = "Nothing to review: no changes in scope.";

/// Renders `review` as the terminal summary. Addresses commands using
/// `addressing` and adds the note on values shown as `<as given>` after the
/// last. With no changed files and no gate items, prints
/// [`NOTHING_TO_REVIEW`] and suggests no judgment.
#[must_use]
pub fn render_summary(review: &Review, addressing: &Addressing) -> String {
    let mut out = Printed::new();
    out.line(header_line(review));
    out.line(worktree_line(&review.worktree));
    out.line(base_line(review.base.commit.as_deref(), &review.base.tree));
    let target = match (review.target.kind, &review.target.commit) {
        (TargetKindInfo::Commit, Some(commit)) => format!("  target: commit {commit}"),
        (TargetKindInfo::WorkingTree, Some(commit)) => {
            format!("  target: working tree over {commit}")
        }
        (_, None) => "  target: working tree over an unborn HEAD".to_owned(),
    };
    out.line(format!("{target} (tree {})", review.target.tree));
    let nothing = review.files.is_empty() && review.gate.items.is_empty();
    if nothing {
        out.line(NOTHING_TO_REVIEW);
    } else {
        out.lines(status_lines(&review.status, review.gate.open));
    }

    out.lines(gate(review));

    out.lines(["", "files:"]);
    if review.files.is_empty() {
        let line = no_changes_line(review.target.kind, addressing, &mut out);
        out.line(line);
    }
    for file in &review.files {
        out.line(format!("  {}", file_line(review, file)));
    }

    let notes = notes(review);
    if !notes.is_empty() {
        out.lines(["", "Not gate items:"]);
        out.lines(notes.into_iter().map(|note| format!("  {note}")));
    }

    let mut last = if nothing {
        Vec::new()
    } else if review.basis.is_none() {
        if saves_later(review) {
            let retry = out.command(addressing.retry());
            vec![format!("Next: {retry}")]
        } else {
            Vec::new()
        }
    } else {
        let open = review.gate.open_names();
        next_lines(&open, review.status.complete, addressing, &mut out)
    };
    // The note follows all commands, including the file list's commit review.
    last.extend(out.take_note());
    if review.gate.items.len() > LONG_GATE {
        last.push(
            "Long output: run the same review command with --format terminal and pipe it to \
             less."
                .to_owned(),
        );
    }
    if !last.is_empty() {
        out.line("");
        out.lines(last);
    }
    out.text()
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

/// Builds the worktree header line.
#[must_use]
pub fn worktree_line(worktree: &str) -> String {
    format!("  worktree: {worktree}")
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
fn status_lines(status: &StatusInfo, open: usize) -> Vec<String> {
    if let Some(completion) = &status.completion {
        let by = format!(
            "Review complete: completed on {} by {}",
            completion.at.date(),
            completion.who.name
        );
        return vec![if matches!(status.source, CompletionSource::Carried(_)) {
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
                lines.extend(paths.iter().map(|path| format!("  {}", shown_path(path))));
            }
            None => lines.push(
                "  changed paths unavailable: the reviewed snapshot is no longer in the \
                 repository"
                    .to_owned(),
            ),
        }
    }
    let evidence = evidence_lines(&related.changed_evidence);
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
                .map(|item| format!("  {}: {}", item.name, item.reasons.join("; "))),
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
fn evidence_lines(evidence: &ChangedEvidence) -> Vec<String> {
    let paths = |paths: &[PathBytes]| paths.iter().map(shown_path).collect::<Vec<_>>().join(", ");
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
            .map(|new| format!("session {}: {}", new.session, paths(&new.paths)))
            .collect();
        lines.push(format!("  new: {}", sessions.join("; ")));
    }
    if !evidence.capture.is_empty() {
        lines.push(format!(
            "  capture limitations changed: {}",
            evidence.capture.join(", ")
        ));
    }
    if !evidence.gone_or_rewritten.is_empty() {
        let records: Vec<&str> = evidence
            .gone_or_rewritten
            .iter()
            .map(supersigil_record::RecordId::as_str)
            .collect();
        lines.push(format!("  gone or rewritten: {}", records.join(", ")));
    }
    lines
}

/// Displays repository `path` with lossy UTF-8 conversion.
#[must_use]
pub fn shown_path(path: &PathBytes) -> String {
    String::from_utf8_lossy(path.as_bytes()).into_owned()
}

fn gate(review: &Review) -> Vec<String> {
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
        gate_row(item, incomplete)
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
#[must_use]
pub fn gate_row(item: &GateItemInfo, incomplete: usize) -> [String; 3] {
    [
        item.name.clone(),
        state_word(item).to_owned(),
        attention(item, incomplete),
    ]
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
fn attention(item: &GateItemInfo, incomplete: usize) -> String {
    let Some(judgment) = &item.judgment else {
        return needs(&item.detail, incomplete as u64);
    };
    let by = format!("by {} on {}", judgment.who.name, judgment.at.date());
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
fn needs(detail: &GateDetail, incomplete: u64) -> String {
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
        GateDetail::NotDiffed { kind } => format!("not diffed: {kind}"),
        GateDetail::UntrackedRecorded { .. } => {
            "untracked, a recorded edit names it; not included".to_owned()
        }
        GateDetail::NotCaptured { cause, .. } => {
            format!("on-disk state not captured: {cause}")
        }
        GateDetail::CommitWithoutOrigin => "commit has no recorded origin".to_owned(),
        GateDetail::Discontinuity { gaps } => {
            let gaps_word = if *gaps == 1 { "gap" } else { "gaps" };
            format!("{gaps} {gaps_word} between recorded edits")
        }
        GateDetail::Unreconciled { reason } => format!("evidence not read: {reason}"),
    }
}

/// Builds the empty file-list text. For a working tree, suggests reviewing
/// a commit just made, using `addressing`, and records that command in
/// `out`.
fn no_changes_line(target: TargetKindInfo, addressing: &Addressing, out: &mut Printed) -> String {
    match target {
        TargetKindInfo::WorkingTree => {
            let command = out.command(addressing.review("--target HEAD"));
            format!("  no changes in scope; to review a commit just made, run {command}")
        }
        TargetKindInfo::Commit => "  no changes in scope".to_owned(),
    }
}

/// Maximum open items named in a `Next:` command before using a placeholder
/// that refers to the item list above.
const MOST_NAMED: usize = 20;

/// Builds next-step lines for a saved review, recording their commands in
/// `out`. Suggests acknowledging `open` items or dismissing placeholder
/// names, then completing; suggests only completion with no open items;
/// emits nothing for a complete review. Prints item names once to keep long
/// gates readable. Above 20 open items, acknowledgment also uses a
/// placeholder and refers to the item list above.
#[must_use]
pub fn next_lines(
    open: &[&str],
    complete: bool,
    addressing: &Addressing,
    out: &mut Printed,
) -> Vec<String> {
    if complete {
        return Vec::new();
    }
    if open.is_empty() {
        let complete = out.command(addressing.judge("complete", &[], ""));
        return vec![format!("Next: {complete}")];
    }
    let acknowledge = if open.len() > MOST_NAMED {
        let acknowledge = out.command(addressing.judge("acknowledge", &[], "<name>..."));
        format!("{acknowledge} ({} open items, listed above)", open.len())
    } else {
        out.command(addressing.judge("acknowledge", open, ""))
    };
    let dismiss = out.command(addressing.judge("dismiss", &[], "<name>... --reason <text>"));
    let complete = out.command(addressing.judge("complete", &[], ""));
    vec![
        format!("Next: {acknowledge}"),
        format!("  or: {dismiss}"),
        format!("  then: {complete}"),
    ]
}

/// Header lines shared with `why`: records and checkouts not reconciled,
/// then one line per candidate transcript with capture limitations.
pub(crate) fn context_lines(records: &[RecordInfo], evidence: &EvidenceInfo) -> Vec<String> {
    let mut lines = unreconciled_lines(records, evidence);
    lines.extend(capture_lines(evidence));
    lines
}

/// One line per record and per checkout not reconciled, with why.
fn unreconciled_lines(records: &[RecordInfo], evidence: &EvidenceInfo) -> Vec<String> {
    let mut lines = Vec::new();
    for record in records.iter().filter(|record| !record.reconciled) {
        lines.push(format!(
            "record {} not reconciled: {}",
            record.id,
            record.reason.as_deref().unwrap_or("unknown reason")
        ));
    }
    for checkout in &evidence.unreconciled_checkouts {
        lines.push(format!(
            "checkout {} not reconciled ({} transcripts): {}",
            checkout.checkout, checkout.transcripts, checkout.reason
        ));
    }
    lines
}

fn capture_lines(evidence: &EvidenceInfo) -> Vec<String> {
    let mut lines = Vec::new();
    for transcript in &evidence.candidate_transcripts {
        let counts = total_counts(&transcript.capture_limitations);
        let words = count_words(&counts);
        if !words.is_empty() {
            lines.push(format!(
                "capture limitations in {} (session {}): {}; not localized to a path",
                transcript.transcript,
                transcript.session,
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
fn file_line(review: &Review, file: &FileReview) -> String {
    let path = &file.path;
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
    parts.extend(other.words());
    if other.unattributed > 0 && !file.command_changes.is_empty() {
        let commands = file.command_changes.iter().map(|c| c.command.as_str());
        parts.push(format!("changed by {}", recorded_commands(commands)));
    }
    if let Some(info) = file
        .attribution
        .as_ref()
        .and_then(|a| a.unavailable.as_ref())
    {
        parts.push(format!("attribution unavailable: {info}"));
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
pub(crate) fn command_line(text: &str) -> String {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    let mut shown: String = first.chars().take(100).collect();
    if shown.len() < first.len() {
        shown.push('…');
    }
    let mut line = shown;
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

    fn words(&self) -> Vec<String> {
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
            let reason = self
                .reason
                .as_ref()
                .map_or_else(String::new, |reason| format!(": {}", reason_words(reason)));
            words.push(format!("{} unattributed{reason}", self.unattributed));
        }
        words
    }
}

/// Builds evidence notes outside gate items for coarse diffs, unavailable
/// attribution, capture limitations, conflicting edits, removed transcripts,
/// scope, origins, and unreconciled evidence.
fn notes(review: &Review) -> Vec<String> {
    let summary = &review.unattributed;
    let evidence = &review.evidence;
    let mut notes = Vec::new();
    for path in &summary.coarse {
        notes.push(format!("{path}: coarse diff, too large to align"));
    }
    for path in &summary.attribution_unavailable {
        notes.push(format!("{path}: attribution unavailable"));
    }
    notes.extend(capture_lines(evidence));
    for conflict in &evidence.conflicting_edits {
        let records: Vec<&str> = conflict
            .records
            .iter()
            .map(supersigil_record::RecordId::as_str)
            .collect();
        notes.push(format!(
            "edit {} excluded: its sightings in records {} disagree",
            conflict.edit.as_str(),
            records.join(", ")
        ));
    }
    for transcript in &evidence.removed_transcripts {
        notes.push(format!(
            "transcript {transcript} removed before it could be read"
        ));
    }
    notes.extend(scope_notes(review));
    notes.extend(unreconciled_lines(&review.records, evidence));
    notes
}

/// Notes on the review's scope and on the origins of its commits.
fn scope_notes(review: &Review) -> Vec<String> {
    let scope = &review.scope;
    let mut notes = Vec::new();
    if !scope.paths.is_empty() {
        notes.push(format!("scope: limited to {}", scope.paths.join(", ")));
    }
    if let Some(note) = &scope.path_note {
        notes.push(format!("scope: {note}"));
    }
    for path in &scope.include_untracked {
        notes.push(format!("scope: untracked {path} included"));
    }
    let excluded = scope.untracked_excluded.len();
    if excluded > 0 {
        notes.push(format!("scope: {excluded} untracked files excluded"));
    }
    for entry in &scope.not_captured {
        notes.push(format!(
            "scope: {} not captured ({}, {} on disk)",
            entry.path, entry.cause, entry.on_disk
        ));
    }
    if scope.skip_worktree_absent > 0 {
        notes.push(format!(
            "scope: {} skip-worktree paths absent from disk keep their index state",
            scope.skip_worktree_absent
        ));
    }
    for path in &scope.unmerged {
        notes.push(format!("scope: {path} unmerged, taken as on disk"));
    }
    if let Some(note) = &scope.ancestry_note {
        notes.push(format!("scope: {note}"));
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
            unavailable.worktree, unavailable.reason
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

pub(crate) fn reason_words(reason: &UnattributedReason) -> String {
    match reason {
        UnattributedReason::NoSurvivingChain { .. } => "no surviving chain".to_owned(),
        UnattributedReason::GapBeforeChain => "gap before the chain's start".to_owned(),
        UnattributedReason::ContentMatchIncomplete => {
            "content matching ran out of budget; a covering edit may exist".to_owned()
        }
        UnattributedReason::AttributionUnavailable { reason } => {
            format!("attribution unavailable: {reason}")
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
