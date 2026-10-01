//! The JSON review model and the per-file assembly of hunks and spans.
//!
//! JSON field order puts identity first, followed by the prominent
//! unattributed summary, then scope, origins, records, evidence, files, and
//! the edits every other section refers to by id.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;
use supersigil_anchor::lines::split_lines;
use supersigil_anchor::{Chain, ChainEnd, Conflict, PathStatus, StopReason};
use supersigil_record::EventId;
use supersigil_record::observations::{CaptureLimitation, EditOperation, Material, Role};

use crate::diff::{DiffHunk, diff_lines};
use crate::outcome::{
    AttributionState, Outcome, UnattributedReason, Verdict, base_line_verdict, target_line_verdict,
};

/// Schema string of the review JSON; a version marker, not a promise, while
/// the pivot is unreleased.
pub const REVIEW_SCHEMA: &str = "supersigil.review/1";
/// Schema string of the `why` JSON.
pub const WHY_SCHEMA: &str = "supersigil.why/1";

/// A review of the change between a base and a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Review {
    /// [`REVIEW_SCHEMA`].
    pub schema: &'static str,
    /// The reviewed worktree.
    pub worktree: String,
    /// The base snapshot.
    pub base: BaseInfo,
    /// The target snapshot.
    pub target: TargetInfo,
    /// Everything no recorded edit explains, first so a reader meets it
    /// before any detail.
    pub unattributed: UnattributedSummary,
    /// The options used and the snapshot's full listing.
    pub scope: ScopeInfo,
    /// Which worktrees originated the reviewed commits.
    pub origins: OriginsInfo,
    /// Every involved record, with its pinned revision.
    pub records: Vec<RecordInfo>,
    /// Transcripts and capture limitations behind the attribution.
    pub evidence: EvidenceInfo,
    /// One entry per changed path in scope, sorted by path bytes.
    pub files: Vec<FileReview>,
    /// Every accepted edit referenced above, keyed by edit id.
    pub edits: BTreeMap<String, EditInfo>,
}

/// The base snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaseInfo {
    /// Base commit, or null for the empty tree.
    pub commit: Option<String>,
    /// Base tree id.
    pub tree: String,
    /// Whether the base is an ancestor of the target commit.
    pub ancestry: AncestryInfo,
}

/// Whether the base is an ancestor of the target commit; the same variants
/// as `supersigil_git::Ancestry`, which the CLI converts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AncestryInfo {
    /// The base is an ancestor of the target commit.
    Ancestor,
    /// It is not: the diff includes changes made on the base side.
    NotAncestor,
    /// Not checked: an empty-tree base or an unborn HEAD.
    Unavailable,
}

/// The target snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetInfo {
    /// Working tree or commit.
    pub kind: TargetKindInfo,
    /// The commit, or for a working tree the HEAD it was built over; null
    /// for a working tree over an unborn HEAD.
    pub commit: Option<String>,
    /// Target tree id.
    pub tree: String,
}

/// Kind of target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKindInfo {
    /// The reviewed worktree's on-disk state.
    WorkingTree,
    /// A commit.
    Commit,
}

/// Git status of a changed path; mirrors `supersigil_git::changes::ChangeStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    /// Only in the target.
    Added,
    /// Only in the base.
    Deleted,
    /// In both, with different contents or mode.
    Modified,
    /// A file became a symlink or gitlink, or the reverse.
    TypeChanged,
}

impl FileStatus {
    /// The one-letter code git uses for the status.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Modified => 'M',
            Self::TypeChanged => 'T',
        }
    }
}

/// How a changed path is handled; mirrors `supersigil_git::changes::FileKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKindInfo {
    /// Diffed and attributed.
    Text,
    /// Listed, not diffed.
    Binary,
    /// Over the size limit; listed, not diffed.
    TooLarge,
    /// Same blob, different mode; listed.
    ModeOnly,
    /// A symlink; listed, not diffed.
    Symlink,
    /// A submodule commit; listed, not diffed.
    Gitlink,
    /// A type change; listed, not diffed.
    TypeChange,
    /// A path that is not valid UTF-8; listed.
    UnsupportedPath,
}

impl FileKindInfo {
    /// The snake-case name the JSON uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Binary => "binary",
            Self::TooLarge => "too_large",
            Self::ModeOnly => "mode_only",
            Self::Symlink => "symlink",
            Self::Gitlink => "gitlink",
            Self::TypeChange => "type_change",
            Self::UnsupportedPath => "unsupported_path",
        }
    }
}

/// The prominent summary of everything no recorded edit explains.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UnattributedSummary {
    /// Per file with any such line, the counts by outcome.
    pub files: Vec<FileCounts>,
    /// Files whose diff is one coarse hunk.
    pub coarse: Vec<String>,
    /// Files whose attribution could not be computed.
    pub attribution_unavailable: Vec<String>,
    /// Files listed but not diffed.
    pub not_diffed: Vec<NotDiffed>,
    /// Untracked files a recorded edit wrote but the scope excluded.
    pub untracked_with_recorded_edits: Vec<String>,
    /// Paths whose on-disk state the snapshot did not capture.
    pub not_captured: Vec<String>,
}

/// Lines of one file by outcome that no chain attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileCounts {
    /// The file.
    pub path: String,
    /// Lines with the *unattributed* outcome, except those counted in
    /// `content_match_incomplete`.
    pub unattributed: usize,
    /// Lines with the *unattributed* outcome because content matching ran
    /// out of budget before finding a covering edit.
    pub content_match_incomplete: usize,
    /// Lines with the *unresolved* outcome.
    pub unresolved: usize,
    /// Lines with the *ambiguous* outcome.
    pub ambiguous: usize,
    /// Lines with the *line ending changed* outcome.
    pub line_ending: usize,
    /// Lines explained only by a content match, whether or not its search
    /// completed: each line's outcome says.
    pub content_match_only: usize,
}

impl FileCounts {
    fn empty(path: String) -> Self {
        Self {
            path,
            unattributed: 0,
            content_match_incomplete: 0,
            unresolved: 0,
            ambiguous: 0,
            line_ending: 0,
            content_match_only: 0,
        }
    }

    const fn total(&self) -> usize {
        self.unattributed
            + self.content_match_incomplete
            + self.unresolved
            + self.ambiguous
            + self.line_ending
            + self.content_match_only
    }
}

/// A file listed but not diffed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotDiffed {
    /// The file.
    pub path: String,
    /// Its kind, as [`FileKindInfo::as_str`] spells it.
    pub kind: String,
}

/// The note a review carries when the base is not an ancestor of the target.
pub const ANCESTRY_NOTE: &str =
    "the base is not an ancestor of the target; the diff includes changes made on the base side";

/// The options used and the snapshot's listing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScopeInfo {
    /// Untracked paths included with `--include-untracked`.
    pub include_untracked: Vec<String>,
    /// Literal path prefixes the review is limited to.
    pub paths: Vec<String>,
    /// Untracked, non-ignored files the scope excluded.
    pub untracked_excluded: Vec<UntrackedInfo>,
    /// Paths whose on-disk state was not captured.
    pub not_captured: Vec<NotCapturedInfo>,
    /// Skip-worktree paths absent from disk that keep their index state.
    pub skip_worktree_absent: usize,
    /// Paths unmerged in the real index, taken in their on-disk form.
    pub unmerged: Vec<String>,
    /// Set (to [`ANCESTRY_NOTE`]) when the base is not an ancestor of the
    /// target.
    pub ancestry_note: Option<String>,
    /// Set when a path selector named the worktree root, so the review has
    /// no path filter.
    pub path_note: Option<String>,
}

/// An untracked file the scope excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UntrackedInfo {
    /// The file.
    pub path: String,
    /// Whether a candidate edit in the reviewed worktree wrote this path.
    pub recorded_edit: bool,
    /// The flag that includes it, for example `--include-untracked src/new.rs`.
    pub include_flag: String,
}

/// A path whose on-disk state the snapshot did not capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotCapturedInfo {
    /// The path.
    pub path: String,
    /// `present` or `missing`.
    pub on_disk: String,
    /// `assume_unchanged`, `skip_worktree`, or `not_staged`.
    pub cause: String,
}

/// Which worktrees originated the reviewed commits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OriginsInfo {
    /// Per reviewed commit with an origin, its origin worktrees.
    pub commits: Vec<CommitOriginInfo>,
    /// Reviewed commits no worktree's reflog originates.
    pub without_origin: Vec<String>,
    /// Worktrees whose origin evidence is unavailable.
    pub unavailable: Vec<UnavailableInfo>,
}

/// A reviewed commit and its origin worktrees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitOriginInfo {
    /// The commit.
    pub commit: String,
    /// Worktrees whose own HEAD reflog has a commit-family entry for it.
    pub worktrees: Vec<String>,
}

/// A worktree whose origin evidence is unavailable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnavailableInfo {
    /// The worktree.
    pub worktree: String,
    /// Why, for example "directory missing".
    pub reason: String,
}

/// An involved record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordInfo {
    /// Record id.
    pub id: String,
    /// Pinned revision, null when the record could not be read.
    pub revision: Option<u64>,
    /// Whether reconcile ran for it.
    pub reconciled: bool,
    /// Why not, when it did not.
    pub reason: Option<String>,
}

/// Transcripts and capture limitations behind the attribution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EvidenceInfo {
    /// Every candidate transcript, whether or not its edits were attributed.
    pub candidate_transcripts: Vec<TranscriptInfo>,
    /// Edits whose checkout could not be placed in any worktree.
    pub unplaced_edits: usize,
    /// Checkouts whose discovered transcripts were not reconciled.
    pub unreconciled_checkouts: Vec<UnreconciledInfo>,
    /// Edits excluded because their sightings in several records disagree.
    pub conflicting_edits: Vec<Conflict>,
}

/// A candidate transcript and its capture limitations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptInfo {
    /// Canonical transcript path.
    pub transcript: String,
    /// Session id.
    pub session: String,
    /// The capture limitations of the involved record whose reports reach
    /// furthest into the transcript. One record's reports cover disjoint
    /// line ranges, so their counts add up; another record's copies of the
    /// same lines are left out rather than counted again.
    pub capture_limitations: Vec<CaptureLimitation>,
    /// Always false: a limitation names a transcript range, not a path.
    pub localized: bool,
}

/// A checkout whose discovered transcripts were not reconciled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnreconciledInfo {
    /// The checkout.
    pub checkout: String,
    /// How many transcripts were discovered for it.
    pub transcripts: usize,
    /// Why they were not reconciled.
    pub reason: String,
}

/// One changed path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileReview {
    /// The path, lossy when not UTF-8.
    pub path: String,
    /// Reversible escaped form, present only when the path is not UTF-8.
    pub path_escaped: Option<String>,
    /// Git status.
    pub status: FileStatus,
    /// Base mode as octal text, `000000` when absent.
    pub old_mode: String,
    /// Target mode as octal text, `000000` when absent.
    pub new_mode: String,
    /// Base blob id.
    pub old_blob: Option<String>,
    /// Target blob id.
    pub new_blob: Option<String>,
    /// How the path is handled.
    pub kind: FileKindInfo,
    /// Attribution-bytes status per side.
    pub attribution_bytes: BytesStatus,
    /// Anchor's status and chains; null when the file is not diffed.
    pub attribution: Option<AttributionInfo>,
    /// Edits excluded from this file as conflicting evidence, whether or
    /// not the file was diffed or attributed: an edit id whose sightings in
    /// different records disagree is excluded from every file any sighting
    /// touched, and listed on each of them.
    pub conflicting_edits: Vec<Conflict>,
    /// Whether the diff is one coarse hunk.
    pub coarse: bool,
    /// Changed regions with their spans.
    pub hunks: Vec<HunkReview>,
    /// Commands whose text mentions the path (textual evidence, never
    /// attribution).
    pub mentions: Vec<Mention>,
}

impl FileReview {
    /// Every edit id this file's analysis names, outside its conflict
    /// references: the edits of its chains and set-aside chains, the edits
    /// its status and chain ends stopped at, and the edits its line outcomes
    /// name (a content match can name an edit no chain holds). A line's
    /// provenance names only chain edits, so it adds none. The review's
    /// `edits` map holds exactly these.
    #[must_use]
    pub fn referenced_edits(&self) -> BTreeSet<EventId> {
        let mut ids = BTreeSet::new();
        if let Some(info) = &self.attribution {
            analysis_edits(
                info.status.as_ref(),
                &info.chains,
                &info.set_aside,
                &mut ids,
            );
        }
        for span in self.hunks.iter().flat_map(|hunk| &hunk.spans) {
            ids.extend(outcome_edits(&span.outcome).into_iter().cloned());
        }
        ids
    }
}

/// Adds the edits a path's status, chains, and set-aside chains name.
pub(crate) fn analysis_edits(
    status: Option<&PathStatus>,
    chains: &[Chain],
    set_aside: &[Chain],
    ids: &mut BTreeSet<EventId>,
) {
    if let Some(PathStatus::NotComposed { reasons }) = status {
        ids.extend(reasons.iter().filter_map(stop_edit).cloned());
    }
    for chain in chains.iter().chain(set_aside) {
        ids.insert(chain.head.clone());
        ids.extend(chain.edits.iter().cloned());
        if let ChainEnd::Stopped { reasons } = &chain.end {
            ids.extend(reasons.iter().filter_map(stop_edit).cloned());
        }
    }
}

/// The edits an outcome names: introducing or replacing, whitespace-only,
/// or covering by content match. Ambiguous and unresolved readings are
/// chain provenance, which names only chain edits.
pub(crate) fn outcome_edits(outcome: &Outcome) -> Vec<&EventId> {
    match outcome {
        Outcome::Attributed { edits } => edits.iter().map(|e| &e.edit).collect(),
        Outcome::ContentMatch { edits, .. } | Outcome::WhitespaceOnly { edits } => {
            edits.iter().collect()
        }
        Outcome::Unresolved { .. }
        | Outcome::Ambiguous { .. }
        | Outcome::Unattributed { .. }
        | Outcome::LineEndingChanged
        | Outcome::Realigned { .. } => Vec::new(),
    }
}

/// The edit a stop reason names, if any.
const fn stop_edit(reason: &StopReason) -> Option<&EventId> {
    match reason {
        StopReason::OperationUnknown { edit }
        | StopReason::TextUnavailable { edit }
        | StopReason::NotLocatable { edit }
        | StopReason::WholeFileWrite { edit }
        | StopReason::ReplaceAllUnverified { edit }
        | StopReason::AfterHashMismatch { edit }
        | StopReason::AfterPatchMismatch { edit }
        | StopReason::NoAcceptedCandidate { edit } => Some(edit),
        StopReason::NoPredecessor => None,
    }
}

/// Attribution-bytes status per side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BytesStatus {
    /// `identical`, `converted`, `too large` (the worktree form is over the
    /// size limit, and reading it stopped there), `failed (exit <code>):
    /// <stderr tail>` (or `failed (killed by a signal): ...`), `absent`, or
    /// `not read` (the file's bytes are not read).
    pub base: String,
    /// Same values as `base`.
    pub target: String,
}

/// Anchor's result for a file, without the per-line detail spans carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttributionInfo {
    /// Anchor's status; null when attribution is unavailable.
    pub status: Option<PathStatus>,
    /// The chains not set aside.
    pub chains: Vec<Chain>,
    /// Alternatives that assume an unrecorded change.
    pub set_aside: Vec<Chain>,
    /// Why attribution is unavailable, when it is.
    pub unavailable: Option<String>,
}

/// One changed region with its spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HunkReview {
    /// First removed base line, 1-based; for an empty range, the line
    /// before it (0 means before the first line).
    pub base_start: usize,
    /// Number of removed base lines.
    pub base_count: usize,
    /// First added target line, 1-based, with the same rule.
    pub target_start: usize,
    /// Number of added target lines.
    pub target_count: usize,
    /// Removed lines with their terminators.
    pub removed: Vec<String>,
    /// Added lines with their terminators.
    pub added: Vec<String>,
    /// Removed-line spans, then added-line spans, each in line order.
    pub spans: Vec<Span>,
}

/// Which side of the diff a span covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanSide {
    /// Removed base lines.
    Base,
    /// Added target lines.
    Target,
}

/// Consecutive diff lines on one side with the same outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    /// Which side.
    pub side: SpanSide,
    /// First line, 1-based.
    pub start: usize,
    /// Number of lines.
    pub count: usize,
    /// The shared outcome.
    pub outcome: Outcome,
    /// Each line's full provenance, in order.
    pub provenance: Vec<Value>,
}

/// A command whose text mentions a file's path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mention {
    /// Command event id.
    pub command: String,
    /// Session id.
    pub session: String,
    /// Turn that issued the command.
    pub turn: String,
    /// Checkout the command ran in.
    pub checkout: String,
    /// The command text.
    pub text: String,
    /// The recorded result, when one was recorded.
    pub result: Option<MentionResult>,
}

/// A mentioned command's recorded result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MentionResult {
    /// Exit status, when reported.
    pub exit: Option<i32>,
    /// `passed` or `failed`, when inferred.
    pub outcome: Option<String>,
}

/// An accepted edit referenced by the review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditInfo {
    /// Session id.
    pub session: String,
    /// Canonical transcript path.
    pub transcript: Option<String>,
    /// Turn that issued the edit.
    pub turn: String,
    /// Time of the edit.
    pub time: String,
    /// Worktree the edit maps to.
    pub worktree: String,
    /// Recorded editing operation.
    pub operation: EditOperation,
    /// The nearest recorded ancestor message that is Human or Delegation.
    pub prompt: Option<PromptInfo>,
}

/// The message that preceded an edit in the conversation; not a cause or a
/// rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptInfo {
    /// Turn id of the message.
    pub turn: String,
    /// Human or Delegation.
    pub role: Role,
    /// The retained excerpt, or why it is not retained.
    pub excerpt: Material<String>,
}

/// A changed path's display forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPathInfo {
    /// Lossy display name.
    pub display: String,
    /// Reversible escaped form when the path is not UTF-8.
    pub escaped: Option<String>,
}

/// Everything [`file_review`] needs for one changed path.
#[derive(Debug, Clone)]
pub struct FileInput<'a> {
    /// The path.
    pub path: &'a RepoPathInfo,
    /// Git status.
    pub status: FileStatus,
    /// Base mode as octal text.
    pub old_mode: String,
    /// Target mode as octal text.
    pub new_mode: String,
    /// Base blob id.
    pub old_blob: Option<String>,
    /// Target blob id.
    pub new_blob: Option<String>,
    /// How the path is handled; only `Text` is diffed.
    pub kind: FileKindInfo,
    /// Attribution-bytes status per side.
    pub bytes_status: BytesStatus,
    /// Base blob bytes; `None` for an added file.
    pub base_blob: Option<&'a [u8]>,
    /// Target blob bytes; `None` for a deleted file.
    pub target_blob: Option<&'a [u8]>,
    /// Anchor's result, or why there is none.
    pub attribution: AttributionState<'a>,
    /// Commands mentioning the path.
    pub mentions: Vec<Mention>,
    /// Edits excluded from the path as conflicting evidence.
    pub conflicting_edits: Vec<Conflict>,
}

/// Builds one file's review: the reviewed diff, each changed line's outcome
/// grouped into spans, and anchor's status and chains.
///
/// Only `Text` files are diffed; any other kind gets no hunks and no
/// attribution.
#[must_use]
pub fn file_review(input: FileInput<'_>) -> FileReview {
    let (attribution, coarse, hunks) = if input.kind == FileKindInfo::Text {
        let base = input.base_blob.unwrap_or_default();
        let target = input.target_blob.unwrap_or_default();
        let diff = diff_lines(base, target);
        let base_lines = split_lines(base);
        let target_lines = split_lines(target);
        let hunks = diff
            .hunks
            .iter()
            .map(|hunk| hunk_review(hunk, &base_lines, &target_lines, &input.attribution))
            .collect();
        (
            Some(attribution_info(&input.attribution)),
            diff.coarse,
            hunks,
        )
    } else {
        (None, false, Vec::new())
    };
    FileReview {
        path: input.path.display.clone(),
        path_escaped: input.path.escaped.clone(),
        status: input.status,
        old_mode: input.old_mode,
        new_mode: input.new_mode,
        old_blob: input.old_blob,
        new_blob: input.new_blob,
        kind: input.kind,
        attribution_bytes: input.bytes_status,
        attribution,
        conflicting_edits: input.conflicting_edits,
        coarse,
        hunks,
        mentions: input.mentions,
    }
}

/// Collects the prominent summary from the files and the scope listing.
#[must_use]
pub fn unattributed_summary(files: &[FileReview], scope: &ScopeInfo) -> UnattributedSummary {
    let mut summary = UnattributedSummary::default();
    for file in files {
        let mut counts = FileCounts::empty(file.path.clone());
        for span in file.hunks.iter().flat_map(|hunk| &hunk.spans) {
            let slot = match span.outcome {
                Outcome::Unattributed {
                    reason: UnattributedReason::ContentMatchIncomplete,
                } => &mut counts.content_match_incomplete,
                Outcome::Unattributed { .. } => &mut counts.unattributed,
                Outcome::Unresolved { .. } => &mut counts.unresolved,
                Outcome::Ambiguous { .. } => &mut counts.ambiguous,
                Outcome::LineEndingChanged => &mut counts.line_ending,
                Outcome::ContentMatch { .. } => &mut counts.content_match_only,
                Outcome::Attributed { .. }
                | Outcome::WhitespaceOnly { .. }
                | Outcome::Realigned { .. } => continue,
            };
            *slot += span.count;
        }
        if counts.total() > 0 {
            summary.files.push(counts);
        }
        if file.coarse {
            summary.coarse.push(file.path.clone());
        }
        if file
            .attribution
            .as_ref()
            .is_some_and(|info| info.unavailable.is_some())
        {
            summary.attribution_unavailable.push(file.path.clone());
        }
        if file.kind != FileKindInfo::Text {
            summary.not_diffed.push(NotDiffed {
                path: file.path.clone(),
                kind: file.kind.as_str().to_owned(),
            });
        }
    }
    summary.untracked_with_recorded_edits = scope
        .untracked_excluded
        .iter()
        .filter(|untracked| untracked.recorded_edit)
        .map(|untracked| untracked.path.clone())
        .collect();
    summary.not_captured = scope
        .not_captured
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    summary
}

fn attribution_info(state: &AttributionState<'_>) -> AttributionInfo {
    match state {
        AttributionState::Available(attr) => AttributionInfo {
            status: Some(attr.status.clone()),
            chains: attr.chains.clone(),
            set_aside: attr.set_aside.clone(),
            unavailable: None,
        },
        AttributionState::Unavailable { reason } => AttributionInfo {
            status: None,
            chains: Vec::new(),
            set_aside: Vec::new(),
            unavailable: Some(reason.clone()),
        },
    }
}

fn hunk_review(
    hunk: &DiffHunk,
    base_lines: &[&[u8]],
    target_lines: &[&[u8]],
    attribution: &AttributionState<'_>,
) -> HunkReview {
    let removed = hunk.base_start..hunk.base_start + hunk.base_count;
    let added = hunk.target_start..hunk.target_start + hunk.target_count;
    let mut spans = Vec::new();
    push_spans(
        &mut spans,
        SpanSide::Base,
        hunk.base_start,
        removed
            .clone()
            .map(|line| base_line_verdict(attribution, line, base_lines, target_lines)),
    );
    push_spans(
        &mut spans,
        SpanSide::Target,
        hunk.target_start,
        added
            .clone()
            .map(|line| target_line_verdict(attribution, line, base_lines, target_lines)),
    );
    HunkReview {
        base_start: one_based_start(hunk.base_start, hunk.base_count),
        base_count: hunk.base_count,
        target_start: one_based_start(hunk.target_start, hunk.target_count),
        target_count: hunk.target_count,
        removed: base_lines[removed].iter().map(|line| text(line)).collect(),
        added: target_lines[added].iter().map(|line| text(line)).collect(),
        spans,
    }
}

/// Groups consecutive lines with equal verdicts, starting at 0-based
/// `first`. Each span's outcome is built once, from its lines' shared
/// verdict, so what every line of a path shares is copied once per span.
fn push_spans<'s>(
    spans: &mut Vec<Span>,
    side: SpanSide,
    first: usize,
    verdicts: impl Iterator<Item = (Verdict<'s>, Value)>,
) {
    // The open span: its verdict, first line (1-based), and provenance.
    let mut current: Option<(Verdict<'s>, usize, Vec<Value>)> = None;
    let close = |(verdict, start, provenance): (Verdict<'_>, usize, Vec<Value>)| Span {
        side,
        start,
        count: provenance.len(),
        outcome: verdict.into_outcome(),
        provenance,
    };
    for (offset, (verdict, provenance)) in verdicts.enumerate() {
        if let Some((_, _, lines)) = current.as_mut().filter(|(open, ..)| *open == verdict) {
            lines.push(provenance);
            continue;
        }
        spans.extend(current.take().map(close));
        current = Some((verdict, first + offset + 1, vec![provenance]));
    }
    spans.extend(current.map(close));
}

/// Unified-diff numbering: 1-based, and for an empty range the line before.
const fn one_based_start(start: usize, count: usize) -> usize {
    if count == 0 { start } else { start + 1 }
}

/// A line's text with its terminator; text files are UTF-8, so this is
/// lossless for every diffed file.
pub(crate) fn text(line: &[u8]) -> String {
    String::from_utf8_lossy(line).into_owned()
}
