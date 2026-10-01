//! The steps `review` and `why` share (design section 1, steps 1 to 7):
//! resolve the change, find the originating worktrees, reconcile and pin
//! every involved record, capture the working tree after the pin, and map
//! the recorded edits onto worktrees. Also the conversions from git and
//! record data to the review model's plain types.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use supersigil_anchor::walk::{AcceptedEdit, dedup};
use supersigil_anchor::{
    CandidateEdit, Conflict, DEFAULT_BUDGET_BYTES, PathAttribution, Request, State, TargetKind,
    attribute,
};
use supersigil_git::bytes::{Conversion, worktree_form};
use supersigil_git::changes::{ChangeStatus, FileKind, Mode};
use supersigil_git::origin::{Origins, UnavailableOrigin, find_origins};
use supersigil_git::snapshot::{
    NotCaptured, NotCapturedCause, OnDisk, SnapshotOptions, WorkingTreeSnapshot,
    snapshot_working_tree,
};
use supersigil_git::worktree::{Worktree, list_worktrees};
use supersigil_git::{
    Ancestry, Base, Git, ObjectId, Repo, RepoPath, ResolvedRange, ResolvedTarget, TargetSpec,
    resolve_range,
};
use supersigil_record::observations::Observation;
use supersigil_record::{EventId, RecordId, Revision};
use supersigil_review::mapping::lines_correspond;
use supersigil_review::model::{
    AncestryInfo, BaseInfo, BytesStatus, CommitOriginInfo, EvidenceInfo, FileKindInfo, FileStatus,
    OriginsInfo, RecordInfo, TargetInfo, TargetKindInfo, UnavailableInfo, UnreconciledInfo,
};
use supersigil_session::checkout::{Placement, canonical, placement};

use crate::error::CliError;
use crate::evidence::Evidence;
use crate::mapping::{MappedEdit, map_edit};
use crate::reconcile::{InvolvedRecord, LOCK_WAIT, UnreconciledCheckout, reconcile};

/// The base a pipeline run reviews against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseChoice {
    /// `--base`, or the target's default base when `None`.
    Rev(Option<String>),
    /// The empty tree, with every commit reachable from the target in range:
    /// `why` explains a line back to the file's creation.
    EmptyTree,
}

/// Inputs to [`gather`].
#[derive(Debug, Clone)]
pub struct PipelineArgs {
    /// Directory that selects the repository and the reviewed worktree.
    pub checkout: PathBuf,
    /// Canonical current directory, which typed paths are relative to.
    pub cwd: PathBuf,
    /// Records directory.
    pub records_dir: PathBuf,
    /// Claude home used for discovery, if any.
    pub claude_home: Option<PathBuf>,
    /// The base.
    pub base: BaseChoice,
    /// The target.
    pub target: TargetSpec,
    /// Untracked paths to include, as typed.
    pub include_untracked: Vec<String>,
    /// Path filters, as typed.
    pub paths: Vec<String>,
}

/// An edit whose file lies in a candidate worktree.
#[derive(Debug, Clone)]
pub struct MappedCandidate {
    /// The edit, its record, and its worktree, as anchor takes it.
    pub candidate: CandidateEdit,
    /// The file's path relative to its worktree, components joined with `/`.
    pub path: String,
}

/// An edit that survived deduplication across every involved record, with
/// the path its file has in its worktree.
#[derive(Debug, Clone)]
pub struct MappedAccepted {
    /// The deduplicated edit and every record that holds a sighting of it.
    pub accepted: AcceptedEdit,
    /// The file's path relative to its worktree, components joined with `/`.
    pub path: String,
}

/// An edit id whose sightings disagree, and every path its sightings in
/// candidate worktrees map to.
#[derive(Debug, Clone)]
pub struct PathConflict {
    /// The conflicting edit and the records that hold its sightings.
    pub conflict: Conflict,
    /// The paths its sightings in candidate worktrees touched, relative to
    /// their worktrees. A sighting elsewhere names a file outside the
    /// review, so its path is never matched against a reviewed one.
    pub paths: BTreeSet<String>,
}

/// One involved record, pinned at the revision the review reads.
#[derive(Debug)]
pub struct PinnedRecord {
    /// The record's id.
    pub record_id: RecordId,
    /// The pinned revision.
    pub revision: Revision,
    /// Why reconcile did not bring the record up to date, if it did not.
    pub not_reconciled: Option<String>,
    /// The record's checkout associations at the pinned revision.
    pub associations: Vec<PathBuf>,
    /// Every session's observations at the pinned revision.
    pub observations: Vec<Observation>,
}

/// Everything `review` and `why` need after the shared steps.
#[derive(Debug)]
pub struct Gathered {
    /// The repository, opened in the reviewed worktree.
    pub repo: Repo,
    /// Canonical root of the reviewed worktree.
    pub worktree: PathBuf,
    /// Canonical current directory.
    pub cwd: PathBuf,
    /// Base, target, and ancestry.
    pub range: ResolvedRange,
    /// The target tree: the commit's, or the captured working tree.
    pub target_tree: ObjectId,
    /// The working-tree snapshot, for a working-tree target.
    pub snapshot: Option<WorkingTreeSnapshot>,
    /// Registered worktrees, with paths canonical when they exist.
    pub worktrees: Vec<Worktree>,
    /// The commits in range and the worktrees that originated them, and
    /// every worktree or record checkout whose origin evidence is
    /// unavailable.
    pub origins: Origins,
    /// Origin worktrees, plus the reviewed worktree for a working-tree target.
    pub candidate_worktrees: Vec<PathBuf>,
    /// Every involved record, pinned.
    pub records: Vec<PinnedRecord>,
    /// Checkouts whose transcripts no record could take.
    pub unreconciled: Vec<UnreconciledCheckout>,
    /// Every sighting of an edit whose file lies in a candidate worktree.
    pub candidates: Vec<MappedCandidate>,
    /// The edits whose file lies in a candidate worktree, after
    /// deduplicating every sighting in every involved record, wherever it
    /// maps: the only edits attribution sees.
    pub accepted: Vec<MappedAccepted>,
    /// Edit ids excluded because their sightings disagree, each with a
    /// sighting in a candidate worktree.
    pub conflicts: Vec<PathConflict>,
    /// Edit sightings no registered worktree contains.
    pub unplaced_edits: usize,
    /// Whether the repository sets `core.ignorecase`.
    pub ignore_case: bool,
    /// `--include-untracked`, relative to the worktree.
    pub include_untracked: Vec<RepoPath>,
    /// Path filters, relative to the worktree; empty when none was given or
    /// one named the worktree root.
    pub paths: Vec<RepoPath>,
    /// Whether a path selector named the worktree root, which selects
    /// everything.
    pub whole_worktree: bool,
    /// The pinned records' evidence, indexed.
    pub evidence: Evidence,
}

/// Runs the shared steps and returns what they found.
///
/// # Errors
///
/// Returns [`CliError::Git`] if git fails or is too old, the checkout is not
/// in a worktree, or a revision does not resolve;
/// [`CliError::CommandFailed`] if a typed path lies outside the worktree or
/// is not UTF-8; [`CliError::Io`] if the worktree root cannot be resolved;
/// and the errors of [`reconcile`].
pub fn gather(args: &PipelineArgs) -> Result<Gathered, CliError> {
    // 1. The reviewed worktree, the base, and the target kind.
    let repo = Repo::open(Git::new(&args.checkout))?;
    let worktree = canonical(repo.root())?;
    let include_untracked = untracked_paths(&worktree, &args.cwd, &args.include_untracked)?;
    let (paths, whole_worktree) = path_filters(&worktree, &args.cwd, &args.paths)?;
    let range = resolve(&repo, &args.base, &args.target)?;

    // 2. The commits in range and the worktrees that originated them.
    let commits = range.commits(&repo)?;
    let worktrees = registered_worktrees(&repo)?;
    let mut origins = find_origins(&repo, &worktrees, &commits)?;
    let candidate_worktrees = candidates_for(&range, &origins, &worktree);

    // 3 and 4. Every involved record, reconciled with bounded lock waits.
    //    `list_worktrees` lists the main worktree first; discovery reads the
    //    project directories of a nested worktree's ancestors up to it.
    let main_worktree = worktrees
        .first()
        .map_or_else(|| worktree.clone(), |main| main.path.clone());
    let reconciliation = reconcile(
        &args.records_dir,
        args.claude_home.as_deref(),
        &candidate_worktrees,
        &main_worktree,
        LOCK_WAIT,
    )?;

    // 5. One pinned snapshot per record. Their checkouts that no
    //    registered worktree holds have no origin evidence either.
    let records = pin(reconciliation.records)?;
    origins
        .unavailable
        .extend(unavailable_associations(&records, &worktrees)?);

    // 6. The working tree, captured after the pin, so every pinned
    //    observation predates it.
    let (target_tree, snapshot) = match &range.target {
        ResolvedTarget::WorkingTree { .. } => {
            let options = SnapshotOptions {
                include_untracked: include_untracked.clone(),
                pathspecs: paths.clone(),
            };
            let snapshot = snapshot_working_tree(&repo, &options)?;
            (snapshot.tree.clone(), Some(snapshot))
        }
        ResolvedTarget::Commit { tree, .. } => (tree.clone(), None),
    };

    // 7. Every recorded edit, mapped onto the registered worktrees and
    //    deduplicated once across all of them, before anything is filtered
    //    to candidate worktrees or partitioned by path: an edit id whose
    //    sightings disagree is excluded from every path, even when only one
    //    of its sightings lies in a candidate worktree.
    let ignore_case = repo.config_bool("core.ignorecase")?.unwrap_or(false);
    let sightings = map_sightings(&records, &worktrees, ignore_case);
    let unplaced_edits = sightings.iter().filter(|s| s.mapped.is_none()).count();
    let candidates = in_candidate_worktrees(&sightings, &candidate_worktrees);
    let (accepted, conflicts) = deduplicate(&sightings, &candidate_worktrees);
    let evidence = Evidence::index(records.iter().flat_map(|r| &r.observations));
    Ok(Gathered {
        repo,
        worktree,
        cwd: args.cwd.clone(),
        range,
        target_tree,
        snapshot,
        worktrees,
        origins,
        candidate_worktrees,
        records,
        unreconciled: reconciliation.unreconciled,
        candidates,
        accepted,
        conflicts,
        unplaced_edits,
        ignore_case,
        include_untracked,
        paths,
        whole_worktree,
        evidence,
    })
}

/// Resolves the range, replacing the base with the empty tree for `why`.
///
/// # Errors
///
/// Returns the git errors of [`resolve_range`] and [`Repo::empty_tree`].
fn resolve(repo: &Repo, base: &BaseChoice, target: &TargetSpec) -> Result<ResolvedRange, CliError> {
    match base {
        BaseChoice::Rev(rev) => Ok(resolve_range(repo, rev.as_deref(), target)?),
        BaseChoice::EmptyTree => {
            let mut range = resolve_range(repo, None, target)?;
            range.base = Base {
                commit: None,
                tree: repo.empty_tree()?,
            };
            range.ancestry = Ancestry::Unavailable;
            Ok(range)
        }
    }
}

/// Registered worktrees, with the paths of those that exist canonicalized.
///
/// # Errors
///
/// Returns the git errors of [`list_worktrees`].
fn registered_worktrees(repo: &Repo) -> Result<Vec<Worktree>, CliError> {
    let mut worktrees = list_worktrees(repo)?;
    for registered in &mut worktrees {
        if let Ok(path) = canonical(&registered.path) {
            registered.path = path;
        }
    }
    Ok(worktrees)
}

/// The checkouts of `records` whose origin evidence is unavailable.
/// [`find_origins`] sees only git, so a record checkout that no longer
/// exists or lies in no registered worktree (a worktree already pruned) is
/// listed here.
///
/// # Errors
///
/// Returns [`CliError::Io`] when a checkout cannot be inspected for a
/// reason other than not existing: it is never taken for a missing one.
fn unavailable_associations(
    records: &[PinnedRecord],
    worktrees: &[Worktree],
) -> Result<Vec<UnavailableOrigin>, CliError> {
    let mut unavailable = Vec::new();
    for association in records.iter().flat_map(|r| &r.associations) {
        let registered = worktrees.iter().any(|w| {
            placement(association, &w.path) != Placement::Outside
                || placement(&w.path, association) != Placement::Outside
        });
        if !exists(association)? || !registered {
            unavailable.push(UnavailableOrigin {
                worktree: association.clone(),
                reason: "origin evidence unavailable: worktree no longer registered".to_owned(),
            });
        }
    }
    Ok(unavailable)
}

/// Whether `path` exists. A path that does not is `false`; any other
/// failure to look at it is an error, not an absence.
///
/// # Errors
///
/// Returns [`CliError::Io`] when `path` cannot be inspected for a reason
/// other than not existing.
fn exists(path: &Path) -> Result<bool, CliError> {
    match std::fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(std::io::Error::new(
            e.kind(),
            format!("cannot inspect {}: {e}", path.display()),
        )
        .into()),
    }
}

/// Origin worktrees, plus the reviewed worktree for a working-tree target.
fn candidates_for(range: &ResolvedRange, origins: &Origins, reviewed: &Path) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = origins
        .origin_worktrees()
        .into_iter()
        .map(|path| canonical(&path).unwrap_or(path))
        .collect();
    if matches!(range.target, ResolvedTarget::WorkingTree { .. }) {
        candidates.push(reviewed.to_path_buf());
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

/// Pins one snapshot per involved record and reads every session from it.
///
/// # Errors
///
/// Returns a store error if a snapshot cannot be read.
fn pin(involved: Vec<InvolvedRecord>) -> Result<Vec<PinnedRecord>, CliError> {
    let mut records = Vec::new();
    for record in involved {
        let snapshot = record.store.snapshot()?;
        let mut observations = Vec::new();
        for session in snapshot.sessions() {
            observations.extend(snapshot.observations(&session)?);
        }
        records.push(PinnedRecord {
            record_id: record.record_id,
            revision: snapshot.revision(),
            not_reconciled: record.not_reconciled,
            associations: snapshot
                .manifest()
                .associations
                .iter()
                .map(|a| a.checkout.clone())
                .collect(),
            observations,
        });
    }
    Ok(records)
}

/// One record's sighting of a recorded edit, and where its file maps.
struct Sighting {
    /// The edit and its record. `worktree` is the worktree its file maps
    /// to or, when no registered worktree contains it, the edit's own
    /// checkout, so [`dedup`] has a key to compare for every sighting.
    candidate: CandidateEdit,
    /// Where the file maps, `None` when no registered worktree contains it.
    mapped: Option<MappedEdit>,
}

/// Maps every edit of every pinned record onto the registered worktrees,
/// keeping every sighting whether its worktree is a candidate, another
/// registered worktree, or none.
fn map_sightings(
    records: &[PinnedRecord],
    worktrees: &[Worktree],
    ignore_case: bool,
) -> Vec<Sighting> {
    let roots: Vec<PathBuf> = worktrees.iter().map(|w| w.path.clone()).collect();
    let mut sightings = Vec::new();
    for record in records {
        for observation in &record.observations {
            let Observation::Edit(edit) = observation else {
                continue;
            };
            let mapped = map_edit(edit, &roots, ignore_case);
            let worktree = mapped
                .as_ref()
                .map_or_else(|| edit.checkout.clone(), |m| m.worktree.clone());
            sightings.push(Sighting {
                candidate: CandidateEdit {
                    record: record.record_id.clone(),
                    worktree,
                    edit: edit.clone(),
                },
                mapped,
            });
        }
    }
    sightings
}

/// Where `sighting` maps, when that is a candidate worktree.
fn candidate_mapping<'a>(
    sighting: &'a Sighting,
    candidate_worktrees: &[PathBuf],
) -> Option<&'a MappedEdit> {
    sighting
        .mapped
        .as_ref()
        .filter(|m| candidate_worktrees.contains(&m.worktree))
}

/// The sightings whose file lies in a candidate worktree, before
/// deduplication.
fn in_candidate_worktrees(
    sightings: &[Sighting],
    candidate_worktrees: &[PathBuf],
) -> Vec<MappedCandidate> {
    sightings
        .iter()
        .filter_map(|sighting| {
            let mapped = candidate_mapping(sighting, candidate_worktrees)?;
            Some(MappedCandidate {
                candidate: sighting.candidate.clone(),
                path: mapped.path.clone(),
            })
        })
        .collect()
}

/// Applies the record's event-identity rule ([`dedup`]) once over every
/// sighting, wherever it maps, and only then filters to candidate
/// worktrees: each accepted edit whose file lies in one gets its path, and
/// each conflict with a sighting in one gets the paths those sightings
/// touched. A conflict whose other sighting lies outside the candidate
/// worktrees therefore still excludes the edit.
fn deduplicate(
    sightings: &[Sighting],
    candidate_worktrees: &[PathBuf],
) -> (Vec<MappedAccepted>, Vec<PathConflict>) {
    // As in `dedup`, only a record's first sighting of an edit id counts.
    let mut first_in_record: BTreeSet<(&RecordId, &EventId)> = BTreeSet::new();
    let mut paths: BTreeMap<&EventId, BTreeSet<String>> = BTreeMap::new();
    for sighting in sightings {
        let id = &sighting.candidate.edit.id;
        if !first_in_record.insert((&sighting.candidate.record, id)) {
            continue;
        }
        if let Some(mapped) = candidate_mapping(sighting, candidate_worktrees) {
            paths.entry(id).or_default().insert(mapped.path.clone());
        }
    }
    let (accepted, conflicts) = dedup(sightings.iter().map(|s| s.candidate.clone()).collect());
    let accepted = accepted
        .into_iter()
        .filter_map(|accepted| {
            // Accepted sightings agree on their payload, so on where they
            // map: one path when that is a candidate worktree, none otherwise.
            let path = paths.get(&accepted.edit.id)?.first()?.clone();
            Some(MappedAccepted { accepted, path })
        })
        .collect();
    let conflicts = conflicts
        .into_iter()
        .filter_map(|conflict| {
            let paths = paths.get(&conflict.edit)?.clone();
            Some(PathConflict { conflict, paths })
        })
        .collect();
    (accepted, conflicts)
}

/// Converts `-- <path>` selectors with [`repo_path`]. A selector that
/// names the worktree root selects everything: the result is then no filter
/// at all, with `true` so the scope listing can say why. Every selector is
/// still converted, so one that is invalid on its own is never hidden by a
/// root selector.
///
/// # Errors
///
/// Returns the errors of [`repo_path`].
pub fn path_filters(
    root: &Path,
    cwd: &Path,
    typed: &[String],
) -> Result<(Vec<RepoPath>, bool), CliError> {
    let mut paths = Vec::new();
    let mut whole_worktree = false;
    for text in typed {
        match repo_path(root, cwd, text)? {
            Some(path) => paths.push(path),
            None => whole_worktree = true,
        }
    }
    if whole_worktree {
        paths.clear();
    }
    Ok((paths, whole_worktree))
}

/// Converts `--include-untracked` paths with [`repo_path`]. They name
/// files, so the worktree root is never a selector here.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] for a path that names the worktree
/// root, and the errors of [`repo_path`].
pub fn untracked_paths(
    root: &Path,
    cwd: &Path,
    typed: &[String],
) -> Result<Vec<RepoPath>, CliError> {
    typed
        .iter()
        .map(|text| {
            repo_path(root, cwd, text)?.ok_or_else(|| {
                CliError::CommandFailed(format!(
                    "--include-untracked takes files; {text} names the worktree root"
                ))
            })
        })
        .collect()
}

/// `typed`, relative to `cwd` unless absolute, as a path relative to the
/// worktree `root`; `None` when it names `root` itself. Resolved lexically:
/// `.` and `..` apply to the spelling and nothing is read from disk.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when the path lies outside `root` or
/// is not valid UTF-8.
pub fn repo_path(root: &Path, cwd: &Path, typed: &str) -> Result<Option<RepoPath>, CliError> {
    let mut normal = PathBuf::new();
    for component in cwd.join(typed).components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    let relative = normal.strip_prefix(root).map_err(|_outside| {
        CliError::CommandFailed(format!(
            "{typed} is outside the worktree {}",
            root.display()
        ))
    })?;
    let parts = relative
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| CliError::CommandFailed(format!("{typed} is not valid UTF-8")))?;
    Ok((!parts.is_empty()).then(|| RepoPath::from_utf8(&parts.join("/"))))
}

/// Whether a mapped edit path names `path`: exactly, or ignoring ASCII case
/// when the repository sets `core.ignorecase`.
#[must_use]
pub fn same_path(mapped: &str, path: &RepoPath, ignore_case: bool) -> bool {
    path.to_str()
        .is_some_and(|p| p == mapped || (ignore_case && p.eq_ignore_ascii_case(mapped)))
}

/// The Claude home: the flag, then `$HOME/.claude`.
#[must_use]
pub fn claude_home(flag: Option<&Path>) -> Option<PathBuf> {
    flag.map(Path::to_path_buf)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")))
}

/// `path` as the user would type it from `cwd`: relative when `cwd` is at or
/// above the file inside the worktree, absolute otherwise.
#[must_use]
pub fn display_from(worktree: &Path, cwd: &Path, path: &RepoPath) -> String {
    let display = path.display();
    let absolute = || worktree.join(&display).display().to_string();
    let Ok(below) = cwd.strip_prefix(worktree) else {
        return absolute();
    };
    let prefix: Vec<String> = below
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if prefix.is_empty() {
        return display;
    }
    display
        .strip_prefix(&format!("{}/", prefix.join("/")))
        .map_or_else(absolute, str::to_owned)
}

/// Both sides' attribution bytes for one path, and the status the output
/// shows for each.
#[derive(Debug, Clone)]
pub struct PathBytes {
    /// Status of each side's attribution bytes.
    pub status: BytesStatus,
    /// The base side's state, or why attribution is unavailable.
    pub base: Result<State, String>,
    /// The target side's state, or why attribution is unavailable.
    pub target: Result<State, String>,
    /// The target's converted bytes when they do not map line for line onto
    /// its blob, so `target` holds that reason instead of them.
    pub target_unmapped: Option<Vec<u8>>,
}

impl PathBytes {
    /// The target in worktree form whenever its conversion succeeded,
    /// whether or not it maps onto the blob lines: what the file on disk
    /// holds when the capture is current.
    #[must_use]
    pub fn target_worktree_form(&self) -> Option<&[u8]> {
        match &self.target {
            Ok(state) => state.bytes(),
            Err(_) => self.target_unmapped.as_deref(),
        }
    }
}

/// Reads both sides' attribution bytes for `path`: each blob converted to
/// worktree form, kept only when it maps line for line onto the blob.
///
/// # Errors
///
/// Returns [`CliError::Git`] if a conversion cannot be run.
pub fn path_bytes(
    repo: &Repo,
    path: &RepoPath,
    base: Option<(&ObjectId, &[u8])>,
    target: Option<(&ObjectId, &[u8])>,
) -> Result<PathBytes, CliError> {
    let base = side_bytes(repo, path, base)?;
    let target = side_bytes(repo, path, target)?;
    Ok(PathBytes {
        status: BytesStatus {
            base: base.status,
            target: target.status,
        },
        base: base.state,
        target: target.state,
        target_unmapped: target.unmapped,
    })
}

/// One side of [`PathBytes`].
struct Side {
    status: String,
    state: Result<State, String>,
    unmapped: Option<Vec<u8>>,
}

/// One side of [`path_bytes`].
///
/// # Errors
///
/// Returns [`CliError::Git`] if the conversion cannot be run.
fn side_bytes(
    repo: &Repo,
    path: &RepoPath,
    blob: Option<(&ObjectId, &[u8])>,
) -> Result<Side, CliError> {
    let Some((id, bytes)) = blob else {
        return Ok(Side {
            status: "absent".to_owned(),
            state: Ok(State::Absent),
            unmapped: None,
        });
    };
    Ok(match worktree_form(repo, path, id, bytes)? {
        Conversion::Identical => Side {
            status: "identical".to_owned(),
            state: Ok(State::Present(bytes.to_vec())),
            unmapped: None,
        },
        Conversion::Converted(converted) if lines_correspond(bytes, &converted) => Side {
            status: "converted".to_owned(),
            state: Ok(State::Present(converted)),
            unmapped: None,
        },
        Conversion::Converted(converted) => Side {
            status: "converted".to_owned(),
            state: Err("conversion changed line structure".to_owned()),
            unmapped: Some(converted),
        },
        Conversion::Failed { stderr_tail, .. } => Side {
            status: format!("failed: {stderr_tail}"),
            state: Err(format!("conversion failed: {stderr_tail}")),
            unmapped: None,
        },
    })
}

/// Runs anchor for `path` with the accepted edits mapped to it, each passed
/// as one candidate carrying its first record, and attaches the conflicts
/// whose sightings touched the path.
///
/// # Errors
///
/// Returns why attribution is unavailable when either side's bytes are.
pub fn attribute_path(
    g: &Gathered,
    path: &RepoPath,
    bytes: &PathBytes,
) -> Result<PathAttribution, String> {
    let base = bytes.base.clone()?;
    let target = bytes.target.clone()?;
    let edits = g
        .accepted
        .iter()
        .filter(|m| same_path(&m.path, path, g.ignore_case))
        .filter_map(|m| {
            Some(CandidateEdit {
                record: m.accepted.records.first()?.clone(),
                worktree: m.accepted.worktree.clone(),
                edit: m.accepted.edit.clone(),
            })
        })
        .collect();
    let target_kind = match g.range.target {
        ResolvedTarget::WorkingTree { .. } => TargetKind::WorkingTree,
        ResolvedTarget::Commit { .. } => TargetKind::Commit,
    };
    let mut attribution = attribute(Request {
        base,
        target,
        target_kind,
        reviewed_worktree: g.worktree.clone(),
        edits,
        budget_bytes: DEFAULT_BUDGET_BYTES,
    });
    attribution.conflicts = conflicts_for(g, path);
    Ok(attribution)
}

/// The conflicts any of whose sightings touched `path`.
#[must_use]
pub fn conflicts_for(g: &Gathered, path: &RepoPath) -> Vec<Conflict> {
    g.conflicts
        .iter()
        .filter(|c| c.paths.iter().any(|p| same_path(p, path, g.ignore_case)))
        .map(|c| c.conflict.clone())
        .collect()
}

/// Every conflict with a sighting in a candidate worktree, for the evidence
/// block.
#[must_use]
pub fn all_conflicts(g: &Gathered) -> Vec<Conflict> {
    g.conflicts.iter().map(|c| c.conflict.clone()).collect()
}

/// The candidate transcripts of `g` (design section 1, step 7).
#[must_use]
pub fn candidate_transcripts(g: &Gathered) -> BTreeSet<String> {
    g.evidence.candidate_transcripts(
        g.candidates.iter().map(|m| &m.candidate.edit),
        &g.candidate_worktrees,
    )
}

/// The review model's base block.
#[must_use]
pub fn base_info(g: &Gathered) -> BaseInfo {
    BaseInfo {
        commit: g.range.base.commit.as_ref().map(ToString::to_string),
        tree: g.range.base.tree.to_string(),
        ancestry: match g.range.ancestry {
            Ancestry::Ancestor => AncestryInfo::Ancestor,
            Ancestry::NotAncestor => AncestryInfo::NotAncestor,
            Ancestry::Unavailable => AncestryInfo::Unavailable,
        },
    }
}

/// The review model's target block.
#[must_use]
pub fn target_info(g: &Gathered) -> TargetInfo {
    TargetInfo {
        kind: match g.range.target {
            ResolvedTarget::WorkingTree { .. } => TargetKindInfo::WorkingTree,
            ResolvedTarget::Commit { .. } => TargetKindInfo::Commit,
        },
        commit: g.range.target_commit().map(ToString::to_string),
        tree: g.target_tree.to_string(),
    }
}

/// The origins block, with every worktree whose origin evidence is
/// unavailable, record associations included ([`unavailable_associations`]).
#[must_use]
pub fn origins_info(g: &Gathered) -> OriginsInfo {
    let mut unavailable: Vec<UnavailableInfo> = g
        .origins
        .unavailable
        .iter()
        .map(|u| UnavailableInfo {
            worktree: u.worktree.display().to_string(),
            reason: u.reason.clone(),
        })
        .collect();
    unavailable.sort_by(|a, b| a.worktree.cmp(&b.worktree));
    unavailable.dedup();
    OriginsInfo {
        commits: g
            .origins
            .commits
            .iter()
            .map(|c| CommitOriginInfo {
                commit: c.commit.to_string(),
                worktrees: c
                    .worktrees
                    .iter()
                    .map(|w| w.display().to_string())
                    .collect(),
            })
            .collect(),
        without_origin: g
            .origins
            .without_origin
            .iter()
            .map(ToString::to_string)
            .collect(),
        unavailable,
    }
}

/// The records block: each involved record, its pinned revision, and
/// whether it was reconciled.
#[must_use]
pub fn records_info(g: &Gathered) -> Vec<RecordInfo> {
    g.records
        .iter()
        .map(|r| RecordInfo {
            id: r.record_id.as_str().to_owned(),
            revision: Some(r.revision.get()),
            reconciled: r.not_reconciled.is_none(),
            reason: r.not_reconciled.clone(),
        })
        .collect()
}

/// The evidence block: candidate transcripts with every capture limitation,
/// unplaced edits, unreconciled checkouts, and `conflicting_edits`.
#[must_use]
pub fn evidence_info(g: &Gathered, conflicting_edits: Vec<Conflict>) -> EvidenceInfo {
    EvidenceInfo {
        candidate_transcripts: g.evidence.transcripts(&candidate_transcripts(g)),
        unplaced_edits: g.unplaced_edits,
        unreconciled_checkouts: g
            .unreconciled
            .iter()
            .map(|u| UnreconciledInfo {
                checkout: u.checkout.display().to_string(),
                transcripts: u.transcripts,
                reason: u.reason.clone(),
            })
            .collect(),
        conflicting_edits,
    }
}

/// The review model's file status.
#[must_use]
pub fn file_status(status: ChangeStatus) -> FileStatus {
    match status {
        ChangeStatus::Added => FileStatus::Added,
        ChangeStatus::Deleted => FileStatus::Deleted,
        ChangeStatus::Modified => FileStatus::Modified,
        ChangeStatus::TypeChanged => FileStatus::TypeChanged,
    }
}

/// The review model's file kind.
#[must_use]
pub fn file_kind(kind: FileKind) -> FileKindInfo {
    match kind {
        FileKind::Text => FileKindInfo::Text,
        FileKind::Binary => FileKindInfo::Binary,
        FileKind::TooLarge => FileKindInfo::TooLarge,
        FileKind::ModeOnly => FileKindInfo::ModeOnly,
        FileKind::Symlink => FileKindInfo::Symlink,
        FileKind::Gitlink => FileKindInfo::Gitlink,
        FileKind::TypeChange => FileKindInfo::TypeChange,
        FileKind::UnsupportedPath => FileKindInfo::UnsupportedPath,
    }
}

/// A mode as six octal digits, `000000` when absent.
#[must_use]
pub fn mode_text(mode: Mode) -> String {
    format!("{:06o}", mode.0)
}

/// The JSON word for a path's presence on disk.
#[must_use]
pub fn on_disk_word(on_disk: OnDisk) -> &'static str {
    match on_disk {
        OnDisk::Present => "present",
        OnDisk::Missing => "missing",
    }
}

/// The JSON word for why a path's on-disk state was not captured.
#[must_use]
pub fn cause_word(cause: NotCapturedCause) -> &'static str {
    match cause {
        NotCapturedCause::AssumeUnchanged => "assume_unchanged",
        NotCapturedCause::SkipWorktree => "skip_worktree",
        NotCapturedCause::NotStaged => "not_staged",
    }
}

/// The sentence for a path whose on-disk state was not captured.
#[must_use]
pub fn not_captured_reason(entry: &NotCaptured) -> String {
    let what = match entry.on_disk {
        OnDisk::Present => "present on disk; contents not captured",
        OnDisk::Missing => "missing on disk; deletion not captured",
    };
    format!("{what} ({})", cause_word(entry.cause))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use supersigil_git::RepoPath;
    use supersigil_git::changes::Mode;
    use supersigil_git::worktree::Worktree;
    use supersigil_record::{RecordId, Revision};
    use supersigil_session::checkout::canonical;

    use super::{
        PinnedRecord, display_from, mode_text, path_filters, repo_path, same_path,
        unavailable_associations,
    };

    fn pinned(associations: Vec<PathBuf>) -> PinnedRecord {
        PinnedRecord {
            record_id: RecordId::new("r1"),
            revision: Revision::ZERO,
            not_reconciled: None,
            associations,
            observations: Vec::new(),
        }
    }

    fn registered(path: &Path) -> Worktree {
        Worktree {
            path: path.to_path_buf(),
            head: None,
            bare: false,
            prunable: false,
        }
    }

    #[test]
    fn associations_outside_every_registered_worktree_are_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path()).unwrap();
        let repo = root.join("repo");
        let elsewhere = root.join("elsewhere");
        let pruned = repo.join(".claude/worktrees/pruned");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let records = [pinned(vec![
            repo.clone(),
            repo.join("src"),
            elsewhere.clone(),
            pruned.clone(),
        ])];

        let unavailable = unavailable_associations(&records, &[registered(&repo)]).unwrap();

        // The worktree and a directory in it keep their origin evidence; a
        // directory no registered worktree holds, and a missing one (a
        // nested worktree already pruned), have none.
        let paths: Vec<&Path> = unavailable.iter().map(|u| u.worktree.as_path()).collect();
        assert_eq!(paths, [elsewhere.as_path(), pruned.as_path()]);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_association_is_an_error_not_a_missing_one() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path()).unwrap();
        let repo = root.join("repo");
        let locked = repo.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let probe = locked.join("inner");
        let denied = std::fs::metadata(&probe)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied);

        let result = unavailable_associations(&[pinned(vec![probe])], &[registered(&repo)]);

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if denied {
            assert!(result.is_err(), "{result:?}");
        } else {
            eprintln!("skipped: this process ignores file permissions");
        }
    }

    #[test]
    fn typed_paths_resolve_relative_to_the_current_directory() {
        let root = Path::new("/work/repo");
        let path = |cwd: &str, typed: &str| {
            repo_path(root, Path::new(cwd), typed)
                .unwrap()
                .map(|p| p.display())
        };
        assert_eq!(
            path("/work/repo", "src/lib.rs"),
            Some("src/lib.rs".to_owned())
        );
        assert_eq!(
            path("/work/repo/src", "lib.rs"),
            Some("src/lib.rs".to_owned())
        );
        assert_eq!(
            path("/work/repo/src", "../README.md"),
            Some("README.md".to_owned())
        );
        assert_eq!(
            path("/elsewhere", "/work/repo/a.rs"),
            Some("a.rs".to_owned())
        );
        assert_eq!(path("/work/repo", "."), None);
        repo_path(root, Path::new("/work/repo"), "../other/a.rs").unwrap_err();
    }

    #[test]
    fn a_root_selector_still_checks_every_other_selector() {
        let root = Path::new("/work/repo");
        let typed =
            |paths: &[&str]| -> Vec<String> { paths.iter().map(|p| (*p).to_owned()).collect() };
        let (paths, whole) = path_filters(root, root, &typed(&["src", "."])).unwrap();
        assert!(paths.is_empty());
        assert!(whole);
        path_filters(root, root, &typed(&[".", "../other/a.rs"])).unwrap_err();
    }

    #[test]
    fn paths_match_ignoring_case_only_when_asked() {
        let path = RepoPath::from_utf8("src/Lib.rs");
        assert!(same_path("src/Lib.rs", &path, false));
        assert!(!same_path("src/lib.rs", &path, false));
        assert!(same_path("src/lib.rs", &path, true));
    }

    #[test]
    fn displayed_paths_are_relative_to_the_current_directory() {
        let root = Path::new("/work/repo");
        let path = RepoPath::from_utf8("src/new.rs");
        assert_eq!(display_from(root, root, &path), "src/new.rs");
        assert_eq!(display_from(root, &root.join("src"), &path), "new.rs");
        assert_eq!(
            display_from(root, &root.join("docs"), &path),
            root.join("src/new.rs").display().to_string()
        );
    }

    #[test]
    fn modes_print_as_six_octal_digits() {
        assert_eq!(mode_text(Mode::REGULAR), "100644");
        assert_eq!(mode_text(Mode::ABSENT), "000000");
    }
}
