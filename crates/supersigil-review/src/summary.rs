//! The plain terminal summary of a review.
//!
//! Plain, deterministic text in four parts: a header (worktree, base,
//! target, anything not reconciled, and every candidate transcript's capture
//! limitations), the unattributed summary before anything else, one line per
//! file, and scope and origin notes. No diff text: the JSON carries it.
//!
//! Every string that comes from outside the tool (paths, ids, reasons,
//! transcript text) goes through the caller's `escape` function, which the
//! CLI supplies as its `format::escape_control`, so an escape sequence in a
//! path cannot rewrite the terminal.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Write as _};

use supersigil_anchor::ChainClass;
use supersigil_record::observations::{CaptureCounts, CaptureLimitation};

use crate::model::{
    ANCESTRY_NOTE, AncestryInfo, EvidenceInfo, FileKindInfo, FileReview, RecordInfo, Review,
    SpanSide, TargetKindInfo,
};
use crate::outcome::{Outcome, Relation, UnattributedReason};

/// Renders `review` as the terminal summary, escaping untrusted text with
/// `escape`.
#[must_use]
pub fn render_summary(review: &Review, escape: fn(&str) -> String) -> String {
    let mut out = String::new();
    push(&mut out, &format!("review of {}", escape(&review.worktree)));
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
    for line in context_lines(&review.records, &review.evidence, escape) {
        push(&mut out, &format!("  {line}"));
    }

    push(&mut out, "");
    let unattributed = unattributed_lines(review, escape);
    if unattributed.is_empty() {
        push(&mut out, "unattributed: nothing");
    } else {
        push(&mut out, "unattributed:");
        for line in unattributed {
            push(&mut out, &format!("  {line}"));
        }
    }

    push(&mut out, "");
    push(&mut out, "files:");
    if review.files.is_empty() {
        push(&mut out, "  no changes in scope");
    }
    for file in &review.files {
        push(&mut out, &format!("  {}", file_line(review, file, escape)));
    }

    let notes = notes(review, escape);
    if !notes.is_empty() {
        push(&mut out, "");
        for note in notes {
            push(&mut out, &note);
        }
    }
    out
}

/// Header lines shared with `why`: records and checkouts not reconciled,
/// then one line per candidate transcript with capture limitations.
pub(crate) fn context_lines(
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

fn unattributed_lines(review: &Review, escape: fn(&str) -> String) -> Vec<String> {
    let summary = &review.unattributed;
    let incomplete = incomplete_content_matches(&review.files);
    let mut lines = Vec::new();
    for untracked in review
        .scope
        .untracked_excluded
        .iter()
        .filter(|untracked| untracked.recorded_edit)
    {
        lines.push(format!(
            "{}: untracked, a recorded edit wrote this file; not included ({})",
            escape(&untracked.path),
            escape(&untracked.include_flag)
        ));
    }
    for counts in &summary.files {
        let search_incomplete = incomplete.get(counts.path.as_str()).map_or(0, |n| *n);
        let parts = count_phrases([
            (counts.unattributed, "unattributed"),
            (
                counts.content_match_incomplete,
                "unattributed, content matching incomplete",
            ),
            (counts.unresolved, "unresolved (search incomplete)"),
            (counts.ambiguous, "ambiguous"),
            (counts.line_ending, "line ending changed"),
            (
                counts.content_match_only.saturating_sub(search_incomplete),
                "content match only",
            ),
            (search_incomplete, "content match only (search incomplete)"),
        ]);
        lines.push(format!("{}: {}", escape(&counts.path), parts.join(", ")));
    }
    for path in &summary.attribution_unavailable {
        lines.push(format!("{}: attribution unavailable", escape(path)));
    }
    for path in &summary.coarse {
        lines.push(format!("{}: coarse diff, too large to align", escape(path)));
    }
    for entry in &summary.not_diffed {
        lines.push(format!(
            "{}: not diffed: {}",
            escape(&entry.path),
            entry.kind
        ));
    }
    for path in &summary.not_captured {
        lines.push(format!("{}: on-disk state not captured", escape(path)));
    }
    lines
}

/// Lines with a content match whose search did not complete, by file path.
/// The summary's counts do not tell them apart, so they come from the
/// spans.
fn incomplete_content_matches(files: &[FileReview]) -> BTreeMap<&str, usize> {
    let mut incomplete = BTreeMap::new();
    for file in files {
        let count: usize = file
            .hunks
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
            .sum();
        if count > 0 {
            *incomplete.entry(file.path.as_str()).or_default() += count;
        }
    }
    incomplete
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

fn notes(review: &Review, escape: fn(&str) -> String) -> Vec<String> {
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
fn total_counts(limitations: &[CaptureLimitation]) -> CaptureCounts {
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
