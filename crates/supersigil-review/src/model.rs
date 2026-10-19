//! The JSON review model and the per-file assembly of hunks and spans.
//!
//! JSON fields start with the saved basis, worktree, base, and target, then
//! status, gate, and unattributed summary. Scope, origins, records, evidence,
//! and files follow, then edits, keyed by the IDs other sections use. The
//! decisions behind the change, the corrections anchored to none of them,
//! and each candidate session's annotation come last.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;
use supersigil_anchor::lines::split_lines;
use supersigil_anchor::{Chain, ChainEnd, Conflict, PathStatus, StopReason};
use supersigil_record::interpretations::{
    AlternativeStatus, AnnotationRevision, Observed, Source, Speaker,
};
use supersigil_record::observations::{
    CaptureLimitation, ChangeKind, EditOperation, Hunk, Material, Role,
};
use supersigil_record::reviews::{
    Basis, BasisId, CorrectionKind, DecisionRef, Fingerprint, GateDetail, GateItem, ItemId,
    Judgment, JudgmentKind, Origin, PathBytes, Subject, Who,
};
use supersigil_record::time::UtcTime;
use supersigil_record::{EventId, JudgmentId, RecordId, SessionId};

use crate::decisions::interpretation_changes;
use crate::diff::{DiffHunk, diff_lines};
use crate::judging::{
    Earlier, ItemChange, ItemState, JudgedItem, Relation, explanations_differ, uncovered_items,
};
use crate::names::ItemNames;
use crate::outcome::{
    AttributionState, Outcome, UnattributedReason, Verdict, base_line_verdict, target_line_verdict,
};

/// Schema string of the review JSON; a version marker, not a promise, while
/// the pivot is unreleased.
pub const REVIEW_SCHEMA: &str = "supersigil.review/3";
/// Schema string of the `why` JSON.
pub const WHY_SCHEMA: &str = "supersigil.why/2";

/// A review of the change between a base and a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Review {
    /// [`REVIEW_SCHEMA`].
    pub schema: &'static str,
    /// The saved basis judgments bind to, or `None` if saving failed.
    pub basis: Option<SavedBasis>,
    /// Why saving the basis failed, serialized as the reason text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis_unsaved: Option<Unsaved>,
    /// The reviewed worktree.
    pub worktree: String,
    /// The base snapshot.
    pub base: BaseInfo,
    /// The target snapshot.
    pub target: TargetInfo,
    /// Whether the review is complete.
    pub status: StatusInfo,
    /// Gate items and states, placed after status so readers see what needs
    /// judgment before review details.
    pub gate: GateInfo,
    /// Everything no recorded edit explains, by kind.
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
    /// The decisions citing an edit that contributes to a line of the
    /// change, sorted by name.
    pub decisions: Vec<DecisionInfo>,
    /// The standing corrections in scope that anchor to no decision in
    /// view, oldest first. Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unanchored_corrections: Vec<CorrectionInfo>,
    /// Per edit id in the change, the sorted names of the decisions citing
    /// it. Omitted when empty. Kept apart from `edits`, whose entries gate
    /// fingerprints hash, so an annotation never changes evidence.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub edit_decisions: BTreeMap<String, Vec<String>>,
    /// Each candidate session's annotation, sorted by session.
    pub annotations: Vec<AnnotationInfo>,
}

/// A review's basis as the review store saved it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SavedBasis {
    /// The basis's id, which names the saved review.
    pub id: BasisId,
    /// The basis.
    pub document: Basis,
}

/// Why a review's basis was not saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsaved {
    /// The review store stayed busy; running the review again may save it.
    Busy,
    /// A basis path, such as the worktree, is not valid UTF-8. The store's
    /// JSON cannot hold it, so rerunning cannot save this review.
    PathNotUtf8,
}

impl Unsaved {
    /// Returns the reason text used by the summary and JSON.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Busy => "the review store is busy",
            Self::PathNotUtf8 => {
                "it names a path that is not valid UTF-8, which the review store cannot hold"
            }
        }
    }

    /// Whether running the review again may save it.
    #[must_use]
    pub const fn may_save_later(self) -> bool {
        matches!(self, Self::Busy)
    }
}

impl Serialize for Unsaved {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.reason())
    }
}

/// Whether the review is complete, and what changed since the earlier
/// judged reviews related to it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct StatusInfo {
    /// Whether a standing completion covers the review.
    pub complete: bool,
    /// The review's own standing completion or one carried from a review of
    /// the same content.
    pub completion: Option<JudgmentInfo>,
    /// Related judged reviews in store order, except the review with the
    /// most recent carried completion comes first. If none carries, the most
    /// recent standing completion comes first. Omitted when empty, including
    /// for `judge`, which reads no records and relates no reviews here.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedInfo>,
    /// Where the completion comes from. Not in the JSON, where
    /// `completion.review` names the review it was recorded on.
    #[serde(skip)]
    pub source: CompletionSource,
}

/// Where a review's completion comes from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CompletionSource {
    /// Nothing completes the review, with the changes since related reviews
    /// read, or no judgment applies to it.
    #[default]
    Absent,
    /// The review's own completion.
    Own,
    /// A completion carried from this related review of the same content.
    Carried(BasisId),
    /// No own completion counts, and whether one carries was not checked,
    /// since the changes since related reviews were not read.
    NotChecked,
}

/// Code and record changes since a related review, read by the CLI beyond
/// its basis.
#[derive(Debug, Clone, Default)]
pub struct RelatedReading {
    /// Sorted paths that differ between its target tree and the current one,
    /// within scope. `None` if the earlier tree is unavailable, for example
    /// after deleting its retention ref and pruning it with git.
    pub changed_paths: Option<Vec<PathBytes>>,
    /// The observations recorded since it that edit or change a reviewed
    /// path, by session.
    pub new: Vec<NewObservations>,
    /// The candidate transcripts with a capture limitation recorded since
    /// it, sorted.
    pub capture: Vec<String>,
    /// Sorted records no longer involved, whose logs no longer start with
    /// the earlier bytes, or whose logs changed during this review's read.
    pub gone_or_rewritten: Vec<RecordId>,
}

/// An earlier judged review related to this one, and what changed since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedInfo {
    /// The related review's id.
    pub review: BasisId,
    /// How it relates: the same content, or another target tree of the
    /// same worktree.
    pub relation: Relation,
    /// Its most recent standing completion, if any.
    pub completion: Option<JudgmentInfo>,
    /// The paths that differ between its target tree and this one's,
    /// within the scope; null when its target tree is no longer in the
    /// repository.
    pub changed_paths: Option<Vec<PathBytes>>,
    /// Evidence changes since the earlier review. Omits explanation changes
    /// when changed paths are unavailable, since code changes also change
    /// explanations.
    pub changed_evidence: ChangedEvidence,
    /// The sessions whose annotation this review reads differs from the one
    /// it read, sorted by session. Omitted when empty. A changed
    /// interpretation blocks nothing: it is neither evidence nor a gate
    /// item.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub interpretation: Vec<InterpretationChange>,
    /// Current gate items absent from its gate or with changed fingerprints,
    /// in gate order.
    pub uncovered: Vec<UncoveredInfo>,
}

impl RelatedInfo {
    /// Builds details of related review `related` from `reading` and its
    /// most recent standing `completion`, using current `basis` and item
    /// `names`.
    pub(crate) fn new(
        basis: &Basis,
        related: Earlier<'_>,
        reading: RelatedReading,
        completion: Option<&Judgment>,
        names: &ItemNames<'_>,
    ) -> Self {
        let earlier = related.basis();
        let changed_evidence = ChangedEvidence {
            explanations: reading
                .changed_paths
                .as_ref()
                .map(|changed| explanations_differ(basis, earlier, changed))
                .unwrap_or_default(),
            new: reading.new,
            capture: reading.capture,
            gone_or_rewritten: reading.gone_or_rewritten,
        };
        let uncovered = uncovered_items(&basis.gate, &earlier.gate)
            .into_iter()
            .map(|uncovered| UncoveredInfo {
                id: uncovered.item.id.clone(),
                name: name_of(names, &uncovered.item.id),
                reasons: match uncovered.change {
                    ItemChange::New => vec!["new"],
                    ItemChange::Changed(reasons) => reasons,
                },
            })
            .collect();
        Self {
            review: related.id().clone(),
            relation: related.relation(),
            completion: completion.map(JudgmentInfo::new),
            changed_paths: reading.changed_paths,
            changed_evidence,
            interpretation: interpretation_changes(&earlier.annotations, &basis.annotations),
            uncovered,
        }
    }

    /// Checks for a completion of the same content whose gate covers the
    /// current gate, with no evidence changes since.
    pub(crate) fn carries(&self) -> bool {
        self.completion.is_some()
            && self.relation == Relation::SameContent
            && self.uncovered.is_empty()
            && self.changed_evidence.is_empty()
    }
}

/// Evidence changes since an earlier review. Excludes conversation turns,
/// commands whose change reports list no reviewed paths, including mentions,
/// and capture limitations recorded before that review.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChangedEvidence {
    /// Reviewed files whose explanation differs while their content does
    /// not.
    pub explanations: Vec<PathBytes>,
    /// The observations recorded since that edit or change a reviewed
    /// path, by session.
    pub new: Vec<NewObservations>,
    /// Candidate transcripts with a capture limitation recorded since.
    pub capture: Vec<String>,
    /// Records it read that are gone or rewritten.
    pub gone_or_rewritten: Vec<RecordId>,
}

impl ChangedEvidence {
    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.explanations.is_empty()
            && self.new.is_empty()
            && self.capture.is_empty()
            && self.gone_or_rewritten.is_empty()
    }
}

/// A session's observations since an earlier review that affect reviewed
/// paths. Includes edits mapping to those paths and commands whose change
/// reports list them. Command text that only names a path does not count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewObservations {
    /// The session.
    pub session: String,
    /// The reviewed paths they edit or change, sorted.
    pub paths: Vec<PathBytes>,
}

/// A current gate item absent from an earlier gate or with a changed fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UncoveredInfo {
    /// The item's id.
    pub id: ItemId,
    /// Its name, as the gate shows it.
    pub name: String,
    /// The reopen reason of each fingerprint part that differs, or `new`
    /// when the earlier gate did not hold the item.
    pub reasons: Vec<&'static str>,
}

/// Gate items a person judges before completing the review and their states.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GateInfo {
    /// How many items need a judgment: those open or reopened.
    pub open: usize,
    /// Every item, in gate order.
    pub items: Vec<GateItemInfo>,
}

impl GateInfo {
    /// Builds gate items from judged `items` with their names and states.
    /// Items absent from `names` use their full IDs.
    pub(crate) fn judged(items: &[JudgedItem<'_>], names: &ItemNames<'_>) -> Self {
        Self::of(
            items.iter().map(|judged| (judged.item, &judged.state)),
            names,
        )
    }

    /// Builds an open gate using `names` for an unsaved review that no
    /// judgment can apply to.
    #[must_use]
    pub fn unjudged(gate: &[GateItem], names: &ItemNames<'_>) -> Self {
        Self::of(gate.iter().map(|item| (item, &ItemState::Open)), names)
    }

    /// Returns names of items counted by [`Self::open`], in gate order.
    #[must_use]
    pub fn open_names(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter(|item| item.state.is_open())
            .map(|item| item.name.as_str())
            .collect()
    }

    fn of<'s, 'j: 's>(
        items: impl Iterator<Item = (&'s GateItem, &'s ItemState<'j>)>,
        names: &ItemNames<'_>,
    ) -> Self {
        let items: Vec<GateItemInfo> = items
            .map(|(item, state)| {
                let (state, judgment, reopen_reasons) = match state {
                    ItemState::Acknowledged(judgment) => {
                        (ItemStateInfo::Acknowledged, Some(*judgment), Vec::new())
                    }
                    ItemState::Dismissed(judgment) => {
                        (ItemStateInfo::Dismissed, Some(*judgment), Vec::new())
                    }
                    ItemState::Carried(judgment) => {
                        (ItemStateInfo::Carried, Some(*judgment), Vec::new())
                    }
                    ItemState::Reopened { judgment, reasons } => {
                        (ItemStateInfo::Reopened, Some(*judgment), reasons.clone())
                    }
                    ItemState::Open => (ItemStateInfo::Open, None, Vec::new()),
                };
                GateItemInfo {
                    id: item.id.clone(),
                    name: name_of(names, &item.id),
                    origin: item.origin(),
                    subject: item.subject.clone(),
                    detail: item.detail.clone(),
                    fingerprint: item.fingerprint.clone(),
                    state,
                    judgment: judgment.map(JudgmentInfo::new),
                    reopen_reasons,
                }
            })
            .collect();
        Self {
            open: items.iter().filter(|item| item.state.is_open()).count(),
            items,
        }
    }
}

fn name_of(names: &ItemNames<'_>, id: &ItemId) -> String {
    names
        .name(id)
        .map_or_else(|| format!("id:{id}"), str::to_owned)
}

/// One gate item and its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateItemInfo {
    /// The item's id, the same in every review that has the item.
    pub id: ItemId,
    /// The name a person types for it from the current directory.
    pub name: String,
    /// Its origin.
    pub origin: Origin,
    /// What it is about.
    pub subject: Subject,
    /// What its row says, by origin.
    pub detail: GateDetail,
    /// What a judgment of it saw.
    pub fingerprint: Fingerprint,
    /// Whether a judgment closes it.
    pub state: ItemStateInfo,
    /// Its own or carried closing judgment, or the earlier judgment whose
    /// fingerprint differs for a reopened item. Null when open.
    pub judgment: Option<JudgmentInfo>,
    /// Reopen reasons for fingerprint parts that differ from the earlier
    /// judgment's evidence.
    pub reopen_reasons: Vec<&'static str>,
}

/// Whether a gate item is closed, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStateInfo {
    /// No standing acknowledgment or dismissal closes or reopens it.
    Open,
    /// Acknowledged on this review.
    Acknowledged,
    /// Dismissed on this review.
    Dismissed,
    /// Closed by a related review's judgment of the same evidence.
    Carried,
    /// A related review's judgment names it, but its evidence changed
    /// since.
    Reopened,
}

impl ItemStateInfo {
    /// Whether the item needs a judgment: it is open or reopened.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open | Self::Reopened)
    }
}

/// A recorded judgment, as the review shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JudgmentInfo {
    /// The judgment's id.
    pub id: JudgmentId,
    /// What it records.
    pub kind: JudgmentKind,
    /// The full id of the review it was made on.
    pub review: BasisId,
    /// The reason given with it, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Optional note text from a comment or completion, or a correction's
    /// constraint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// For a correction, the decision it addresses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionRef>,
    /// For a correction, its kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correction: Option<CorrectionKind>,
    /// Who recorded it.
    pub who: Who,
    /// When, in UTC.
    pub at: UtcTime,
}

impl JudgmentInfo {
    /// Builds the review's displayed details for `judgment`.
    #[must_use]
    pub fn new(judgment: &Judgment) -> Self {
        Self {
            id: judgment.id.clone(),
            kind: judgment.kind,
            review: judgment.basis.clone(),
            reason: judgment.reason.clone(),
            text: judgment.text.clone(),
            decision: judgment.decision.clone(),
            correction: judgment.correction,
            who: judgment.who.clone(),
            at: judgment.at,
        }
    }
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
    /// Untracked files a recorded edit names but the scope excluded.
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
    /// Counts `file`'s lines and returns their spans, excluding attributed,
    /// whitespace-only, and realigned lines.
    pub(crate) fn of(file: &FileReview) -> (Self, Vec<&Span>) {
        let mut counts = Self {
            path: file.path.clone(),
            unattributed: 0,
            content_match_incomplete: 0,
            unresolved: 0,
            ambiguous: 0,
            line_ending: 0,
            content_match_only: 0,
        };
        let mut counted = Vec::new();
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
            counted.push(span);
        }
        (counts, counted)
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

impl ScopeInfo {
    /// Returns excluded untracked files named by recorded edits, each a
    /// gate item, in listed order.
    pub fn recorded_untracked(&self) -> impl Iterator<Item = &UntrackedInfo> {
        self.untracked_excluded
            .iter()
            .filter(|untracked| untracked.recorded_edit)
    }
}

/// An untracked file the scope excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UntrackedInfo {
    /// The file, lossy when not UTF-8.
    pub path: String,
    /// Reversible escaped form, present only when the path is not UTF-8.
    pub path_escaped: Option<String>,
    /// The path's raw bytes, which name its gate item.
    #[serde(skip)]
    pub path_bytes: PathBytes,
    /// Whether a candidate edit in the reviewed worktree names this path.
    /// A shell-inferred edit does not establish that its statement ran.
    pub recorded_edit: bool,
    /// The flag that includes it, for example `--include-untracked src/new.rs`.
    pub include_flag: String,
}

/// A path whose on-disk state the snapshot did not capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotCapturedInfo {
    /// The path, lossy when not UTF-8.
    pub path: String,
    /// Reversible escaped form, present only when the path is not UTF-8.
    pub path_escaped: Option<String>,
    /// The path's raw bytes, which name its gate item.
    #[serde(skip)]
    pub path_bytes: PathBytes,
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
    /// Sorted transcripts removed after discovery but before reading. Each
    /// leaves its record or checkout unreconciled.
    pub removed_transcripts: Vec<String>,
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
    /// The path's raw bytes, which name its gate items.
    #[serde(skip)]
    pub path_bytes: PathBytes,
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
    /// attribution), without those listed in `command_changes`.
    pub mentions: Vec<Mention>,
    /// Commands the harness reported as changing the file, newest first.
    /// These observe file changes during commands, never attribute lines.
    pub command_changes: Vec<CommandChange>,
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

/// A command the harness reported as changing a file.
///
/// The harness reports a file difference after the command, identifying
/// neither the responsible statement nor its bytes. This is never
/// attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandChange {
    /// Command event id.
    pub command: String,
    /// Session id.
    pub session: String,
    /// Turn that issued the command.
    pub turn: String,
    /// Canonical transcript path.
    pub transcript: Option<String>,
    /// Time the command was issued.
    pub time: String,
    /// Checkout the command ran in.
    pub checkout: String,
    /// Worktree containing the reported file. A command is listed once per
    /// reported file, per candidate worktree: a commit review has an entry
    /// for each worktree where it changed that path. With case ignored, two
    /// spellings of one file give two entries for the reviewed path, each
    /// with its own kind and diff.
    pub worktree: String,
    /// The command text.
    pub text: String,
    /// What the harness stated about the change.
    pub kind: ChangeKind,
    /// The harness's capped display hunks for this file over the whole
    /// command, or why none exist.
    pub patch: Material<Vec<Hunk>>,
    /// Changed files counted but unnamed in the report. Any may be a
    /// reviewed file missing from this command's listing.
    pub unlisted: u64,
    /// Files reported outside the command's checkout, which the record
    /// omits.
    pub outside: u64,
    /// Reported flag names, kept because their meaning is not established.
    pub flags: BTreeSet<String>,
}

/// How an edit reached the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditOriginInfo {
    /// A call to an editing tool.
    Tool,
    /// A quoted shell heredoc. The edit is inferred from command text. The
    /// harness confirms the command changed the file, not that this
    /// statement ran.
    Shell {
        /// The command's event id.
        command: String,
        /// The command's text, when the review's records hold the command.
        text: Option<String>,
    },
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
    /// How the edit reached the record.
    pub origin: EditOriginInfo,
    /// The nearest recorded ancestor message that is Human, Delegation or
    /// Summary.
    pub prompt: Option<PromptInfo>,
}

/// The message that preceded an edit in the conversation; not a cause or a
/// rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptInfo {
    /// Turn id of the message.
    pub turn: String,
    /// Human, Delegation, or Summary for a compaction summary.
    pub role: Role,
    /// The message text, or why it is not retained.
    pub text: Material<String>,
}

/// A decision an annotator recorded, shown because an edit it cites
/// contributes to a line of the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecisionInfo {
    /// Its name in this review: its id, or `<session short>:<id>` when
    /// another annotated session has a decision with the same id.
    pub name: String,
    /// The annotated session.
    pub session: SessionId,
    /// The record chosen for the session, which holds the annotation.
    pub record: RecordId,
    /// The annotation revision read.
    pub revision: AnnotationRevision,
    /// The annotator's id for it.
    pub id: String,
    /// What was decided, as the annotator put it.
    pub title: String,
    /// Whether a standing correction disputes its rationale or grouping.
    pub disputed: bool,
    /// The standing corrections anchored to it, oldest first.
    pub corrections: Vec<CorrectionInfo>,
    /// Why it was decided, each line labelled by who gave it. A
    /// correction's constraint comes first.
    pub rationale: Vec<RationaleInfo>,
    /// Other courses proposed or attempted.
    pub alternatives: Vec<AlternativeInfo>,
    /// What it means for later work.
    pub implications: Vec<ImplicationInfo>,
    /// The sorted names of the session's open-question items in this review
    /// whose `about` names an id this decision cites.
    pub open_questions: Vec<String>,
    /// Its cited ids, split by whether they contribute to a line of the
    /// change.
    pub cites: CitesInfo,
    /// Each span a cited edit contributes to, once, in file order then span
    /// order, saying how the edit contributes.
    pub explains: Vec<ExplainedSpan>,
}

impl DecisionInfo {
    /// The full reference naming this decision: its record, session,
    /// revision and id.
    #[must_use]
    pub fn reference(&self) -> DecisionRef {
        DecisionRef {
            record: self.record.clone(),
            session: self.session.clone(),
            revision: self.revision,
            id: self.id.clone(),
        }
    }
}

/// A standing correction of a decision, as a decision it anchors to shows
/// it, or as a review lists it when it anchors to none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CorrectionInfo {
    /// The correction judgment.
    pub judgment: JudgmentId,
    /// The decision it addressed, which need not be the one showing it.
    pub decision: DecisionRef,
    /// What it says about the decision.
    pub kind: CorrectionKind,
    /// Why, for a `wrong` or `not_together` correction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The actual constraint, for a `constraint` correction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Who recorded it.
    pub who: Who,
    /// When, in UTC.
    pub at: UtcTime,
    /// The revision it addressed, when it anchors through the edits that
    /// revision's decision cited rather than to that decision itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addressed: Option<AnnotationRevision>,
    /// Whether the shown revision of its session lists it among the
    /// corrections its annotator saw; `false` when the review shows no
    /// revision of that session.
    pub seen: bool,
    /// The full id of the review it was recorded on.
    pub review: BasisId,
}

/// One line of a decision's rationale, labelled by who gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RationaleInfo {
    /// The reason.
    pub text: String,
    /// Who gave it.
    pub speaker: Speaker,
    /// Whether the speaker stated it. Import checked an explicit line's
    /// quote against the turn; any other line is the annotator's inference.
    pub explicit: bool,
    /// The passage it rests on, if any.
    pub source: Option<Source>,
    /// The correction whose constraint this line is, or `None` for a line
    /// the annotator wrote.
    pub correction: Option<JudgmentId>,
}

/// A course the session proposed or attempted instead of a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AlternativeInfo {
    /// What the alternative was.
    pub title: String,
    /// How far it went.
    pub status: AlternativeStatus,
    /// The edits that attempted it, none for a proposal.
    pub edits: Vec<EventId>,
    /// Whether the session's derivations show every edit of the attempt
    /// restored. Never true for an alternative without edits.
    pub restored: bool,
    /// Why it was not taken, if known.
    pub reason: Option<RationaleInfo>,
}

/// What a decision means for later work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImplicationInfo {
    /// The implication.
    pub text: String,
    /// Whether a turn states it; otherwise it is the annotator's.
    pub explicit: bool,
    /// The passage it rests on, if any.
    pub source: Option<Source>,
}

/// A decision's cited ids, each list sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CitesInfo {
    /// The cited edits that contribute to a line of the change.
    pub in_change: Vec<EventId>,
    /// The other cited edits and commands.
    pub elsewhere: Vec<EventId>,
}

/// A span a decision explains, and how its cited edit contributes to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainedSpan {
    /// The file.
    pub path: PathBytes,
    /// Which side of the diff.
    pub side: SpanSide,
    /// First line, 1-based.
    pub start: usize,
    /// Number of lines.
    pub count: usize,
    /// How the span's outcome names the cited edit.
    pub association: Association,
    /// For a content match, whether its search completed. Omitted
    /// otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_complete: Option<bool>,
}

/// How a span's outcome names an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Association {
    /// The edit introduced or replaced the lines.
    Attributed,
    /// The edit changed only whitespace in the lines.
    WhitespaceOnly,
    /// No chain explains the lines; the edit is one of the whole-edit
    /// blocks covering them. This is never attribution.
    ContentMatch,
}

/// What a review read of one candidate session's annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnnotationInfo {
    /// The candidate session.
    pub session: SessionId,
    /// The record chosen for the session, or `None` when no involved record
    /// holds it.
    pub record: Option<RecordId>,
    /// The checkout through which the review found that record: one whose
    /// review reads it, among whose records `annotate prepare` finds the
    /// session. It is a candidate worktree that is or lies in one of the
    /// record's associations, the reviewed one first, so a command naming
    /// it reads the review's own repository; else the most specific
    /// association lying in a candidate worktree; else the record's first
    /// association. `None` without a record.
    pub checkout: Option<PathBuf>,
    /// The session's current annotation revision in that record, or `None`
    /// when it has none.
    pub revision: Option<AnnotationRevision>,
    /// How much of the session that revision's annotator saw, its
    /// `prepared.observed`; `None` without a revision.
    pub observed: Option<Observed>,
    /// How many observations of the session the chosen record holds.
    pub observations_now: u64,
    /// How many involved records hold an annotation of the session.
    pub annotated_records: usize,
}

/// A session whose annotation differs between an earlier review and this
/// one.
///
/// Revisions are numbered per record: when the record chosen for the
/// session differs, `from` and `to` count in different records and may be
/// equal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterpretationChange {
    /// The session.
    pub session: SessionId,
    /// The revision the earlier review read, or `None` when it read none.
    pub from: Option<AnnotationRevision>,
    /// The revision this review reads, or `None` when it reads none.
    pub to: Option<AnnotationRevision>,
}

/// A changed path's display forms and raw bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPathInfo {
    /// Lossy display name.
    pub display: String,
    /// Reversible escaped form when the path is not UTF-8.
    pub escaped: Option<String>,
    /// The raw bytes.
    pub bytes: PathBytes,
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
    /// Commands mentioning the path. Those also in `command_changes` are
    /// left out of the review.
    pub mentions: Vec<Mention>,
    /// Commands the harness reported as changing the path, newest first.
    pub command_changes: Vec<CommandChange>,
    /// Edits excluded from the path as conflicting evidence.
    pub conflicting_edits: Vec<Conflict>,
}

/// Builds one file's review: the reviewed diff, each changed line's outcome
/// grouped into spans, and anchor's status and chains.
///
/// Only `Text` files are diffed; any other kind gets no hunks and no
/// attribution. A command the harness reported as changing the file is
/// listed under `command_changes`, not again as a mention of its path.
#[must_use]
pub fn file_review(mut input: FileInput<'_>) -> FileReview {
    let changing: BTreeSet<&str> = input
        .command_changes
        .iter()
        .map(|change| change.command.as_str())
        .collect();
    input
        .mentions
        .retain(|mention| !changing.contains(mention.command.as_str()));
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
        path_bytes: input.path.bytes.clone(),
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
        command_changes: input.command_changes,
    }
}

/// Collects the prominent summary from the files and the scope listing.
#[must_use]
pub fn unattributed_summary(files: &[FileReview], scope: &ScopeInfo) -> UnattributedSummary {
    let mut summary = UnattributedSummary::default();
    for file in files {
        let (counts, _) = FileCounts::of(file);
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
        .recorded_untracked()
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
