//! `supersigil review`: the change between a base and a target, each changed
//! line attributed to the recorded edits that produced it, or the reason it
//! is not. Every review is saved in the repository's review store, so
//! judgments can bind to exactly what it showed, and shows its gate with
//! the judgments recorded on it or carried from related reviews that saw an
//! item's same evidence, what changed since the most recent related
//! completion, and the command to run next.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write as _};
use std::path::Path;

use supersigil_anchor::PathAttribution;
use supersigil_git::bytes::{Blob, read_blobs_within};
use supersigil_git::changes::{
    Change, FileKind, MAX_DIFF_BYTES, changed_paths, classify_bytes, classify_change,
};
use supersigil_git::{Ancestry, ObjectId, RepoPath, ResolvedTarget, TargetSpec};
use supersigil_record::derivations::ALGORITHM_VERSION;
use supersigil_record::reviews::{
    self, Algorithms, BASIS_SCHEMA, Basis, BasisBase, BasisCheckout, BasisId, BasisRecord,
    BasisScope, BasisTarget, Judgment, PathBytes, ReviewStore, TargetKind,
};
use supersigil_record::time::UtcTime;
use supersigil_review::gate::{capture, explanations, gate_items};
use supersigil_review::judging::{GateIndex, GateStates, Ledger, related_bases};
use supersigil_review::model::{
    ANCESTRY_NOTE, BytesStatus, CommandChange, EditInfo, FileInput, FileReview, FileStatus,
    GateInfo, NotCapturedInfo, RepoPathInfo, SavedBasis, ScopeInfo, StatusInfo, Unsaved,
    UntrackedInfo, file_review, unattributed_summary,
};
use supersigil_review::outcome::Outcome;
use supersigil_review::summary::{Addressing, render_summary};
use supersigil_review::{REVIEW_SCHEMA, Review};

use crate::agent::in_agent_session;
use crate::commands::ReviewArgs;
use crate::error::CliError;
use crate::evidence::{CandidateCommands, add_referenced_edits};
use crate::format::{OutputFormat, escape_control, write_json};
use crate::gate_inputs::gate_inputs;
use crate::item_names::item_names;
use crate::pipeline::{
    BaseChoice, Gathered, PipelineArgs, all_conflicts, attribute_path, attribution_state,
    base_info, cause_word, claude_home, conflicts_for, display_from, evidence_info, file_kind,
    file_status, gather, mode_text, on_disk_word, origins_info, path_bytes, records_info,
    target_info,
};
use crate::reconcile::LOCK_WAIT;
use crate::record_dir;
use crate::reopening::read_changes;
use crate::review_dir::{
    begin_within, canonical_common_dir, judged_bases, open_review_store_within, resolve_review_dir,
};

/// Runs `supersigil review`.
///
/// # Errors
///
/// Returns [`CliError`] when the repository, a revision, a blob, or a record
/// cannot be read (the errors of [`gather`]), when the review store cannot
/// be read or written for a reason other than a held lock, or when writing
/// the output fails.
pub fn run(args: &ReviewArgs) -> Result<(), CliError> {
    let reviews_dir = resolve_review_dir(args.review_dir.as_deref())?;
    let g = gather(&PipelineArgs {
        checkout: record_dir::canonical_checkout(args.checkout.as_deref())?,
        cwd: record_dir::canonical_checkout(None)?,
        records_dir: record_dir::resolve_record_dir(args.record_dir.as_deref())?,
        claude_home: claude_home(args.claude_home.as_deref()),
        base: BaseChoice::Rev(args.base.clone()),
        target: args
            .target
            .clone()
            .map_or(TargetSpec::WorkingTree, TargetSpec::Commit),
        include_untracked: args.include_untracked.clone(),
        paths: args.paths.clone(),
        hash_logs: true,
    })?;
    let mut review = build(&g)?;
    let document = basis(&g, &review);
    let mut addressing = Addressing {
        review: None,
        checkout: args.checkout.clone(),
        review_dir: args.review_dir.clone(),
        retry: retry_flags(args),
        retry_paths: args.paths.clone(),
    };
    // The id the store saves the basis under. An unsaved review shows the
    // judgments on the same content saved before, which has the same id. A
    // basis naming a path that is not UTF-8 has none: the store's JSON
    // cannot hold it, so it is never saved and no judgment applies to it.
    match document.basis_id() {
        Ok(id) => {
            let stored = store(&g, &reviews_dir, &document, &id)?;
            (review.status, review.gate) = judged(&g, &review, &id, &document, &stored)?;
            addressing.review =
                (stored.selection.as_ref() != Some(&id)).then(|| id.short().to_owned());
            if stored.saved {
                review.basis = Some(SavedBasis { id, document });
            } else {
                review.basis_unsaved = Some(Unsaved::Busy);
            }
        }
        Err(_not_utf8) => {
            let names = item_names(&document, g.ignore_case, &g.worktree, &g.cwd);
            review.gate = GateInfo::unjudged(&document.gate, &names);
            review.basis_unsaved = Some(Unsaved::PathNotUtf8);
        }
    }
    match args.format.resolve() {
        OutputFormat::Json => write_json(&review)?,
        OutputFormat::Terminal => io::stdout()
            .lock()
            .write_all(render_summary(&review, &addressing, escape_control).as_bytes())?,
    }
    Ok(())
}

/// The flags of the call `args` describes, other than `--checkout` and
/// `--review-dir`, each with the value it was given, for the command that
/// runs the review again. Not repeated: the output format and the record
/// and Claude home directories.
fn retry_flags(args: &ReviewArgs) -> Vec<(&'static str, Option<String>)> {
    let mut flags = Vec::new();
    if let Some(base) = &args.base {
        flags.push(("--base", Some(base.clone())));
    }
    if let Some(target) = &args.target {
        flags.push(("--target", Some(target.clone())));
    }
    if args.working_tree {
        flags.push(("--working-tree", None));
    }
    for path in &args.include_untracked {
        flags.push(("--include-untracked", Some(path.clone())));
    }
    flags
}

/// What the review store holds of a review after [`store`] saved it.
struct Stored {
    /// Whether the basis was saved; not when the store stayed busy.
    saved: bool,
    /// Every judgment in the store, in the order recorded.
    judgments: Vec<Judgment>,
    /// The review the reviewed worktree selects, if any.
    selection: Option<BasisId>,
    /// The basis of every other review with a judgment that relates to
    /// it, by id.
    judged: BTreeMap<BasisId, Basis>,
}

/// Saves `basis`, whose id is `id`, in the review store in `reviews_dir`
/// of `g`'s repository (see [`save`]), then reads the store without a
/// lock: its judgments, the reviewed worktree's selection, and the other
/// reviews judged that relate to it. When the store's directory stayed
/// busy, the store is only looked up; no store yet has no judgments.
///
/// # Errors
///
/// Returns [`CliError::Git`] if the repository's common directory cannot be
/// resolved, the errors of [`save`], and the store errors of finding the
/// store and reading its manifest, judgments, and bases.
fn store(
    g: &Gathered,
    reviews_dir: &Path,
    basis: &Basis,
    id: &BasisId,
) -> Result<Stored, CliError> {
    let repository = canonical_common_dir(&g.repo)?;
    let (store, saved) = match open_review_store_within(reviews_dir, &repository, LOCK_WAIT)? {
        Some(store) => {
            let saved = save(g, &store, basis)?;
            (Some(store), saved)
        }
        None => (reviews::find(reviews_dir, &repository)?, false),
    };
    let Some(snapshot) = store.as_ref().map(ReviewStore::snapshot).transpose()? else {
        return Ok(Stored {
            saved,
            judgments: Vec::new(),
            selection: None,
            judged: BTreeMap::new(),
        });
    };
    let judgments = snapshot.judgments()?;
    Ok(Stored {
        saved,
        judged: judged_bases(&snapshot, &judgments, id, basis)?,
        selection: snapshot.manifest().selections.get(&g.worktree).cloned(),
        judgments,
    })
}

/// The status and the gate of `review`, gathered in `g`, whose basis
/// `basis` has the id `id`: each item's state from the judgments `stored`
/// holds on it and on the earlier reviews related to it, named as typed
/// from the current directory, and the status from what changed since
/// each of those reviews.
///
/// # Errors
///
/// Returns the errors of [`read_changes`].
fn judged(
    g: &Gathered,
    review: &Review,
    id: &BasisId,
    basis: &Basis,
    stored: &Stored,
) -> Result<(StatusInfo, GateInfo), CliError> {
    let related = related_bases(id, basis, &stored.judged);
    let indexes = GateIndex::of(&related);
    let ledger = Ledger::new(&stored.judgments);
    let states = GateStates::new(id, basis, &related, &indexes, &ledger);
    let names = item_names(basis, g.ignore_case, &g.worktree, &g.cwd);
    let gate = GateInfo::new(&states, &names);
    let readings = read_changes(g, review, &related)?;
    Ok((StatusInfo::reviewed(&states, basis, readings, &names), gate))
}

/// The basis of `review`: what it compared and the evidence it read, as
/// gathered in `g`, the digests of what it shows, and its gate, every list
/// sorted. Salted when a gate item has an unknown fingerprint part, so such
/// a review is never taken for an earlier one.
fn basis(g: &Gathered, review: &Review) -> Basis {
    let inputs = gate_inputs(g, review);
    let gate = gate_items(review, &inputs);
    let salt = gate
        .iter()
        .any(|item| item.fingerprint.has_unknown())
        .then(|| uuid::Uuid::new_v4().simple().to_string());
    let mut records: Vec<BasisRecord> = g
        .records
        .iter()
        .map(|record| {
            let mut associations = record.associations.clone();
            associations.sort();
            BasisRecord {
                id: record.record_id.clone(),
                revision: record.revision,
                logs: record.logs.clone(),
                not_reconciled: record.not_reconciled.clone(),
                associations,
            }
        })
        .collect();
    records.sort_by(|a, b| a.id.cmp(&b.id));
    let mut unreconciled_checkouts: Vec<BasisCheckout> = g
        .unreconciled
        .iter()
        .map(|entry| BasisCheckout {
            checkout: entry.checkout.clone(),
            reason: entry.reason.clone(),
        })
        .collect();
    unreconciled_checkouts.sort_by(|a, b| (&a.checkout, &a.reason).cmp(&(&b.checkout, &b.reason)));
    let (kind, commit) = match &g.range.target {
        ResolvedTarget::WorkingTree { .. } => (TargetKind::WorkingTree, None),
        ResolvedTarget::Commit { commit, .. } => (TargetKind::Commit, Some(commit.to_string())),
    };
    Basis {
        schema: BASIS_SCHEMA.to_owned(),
        worktree: g.worktree.clone(),
        base: BasisBase {
            commit: g.range.base.commit.as_ref().map(ToString::to_string),
            tree: g.range.base.tree.to_string(),
        },
        target: BasisTarget {
            kind,
            commit,
            tree: g.target_tree.to_string(),
        },
        scope: BasisScope {
            include_untracked: sorted(&g.include_untracked),
            paths: sorted(&g.paths),
        },
        records,
        unreconciled_checkouts,
        algorithms: Algorithms {
            derivation: ALGORITHM_VERSION,
        },
        explanations: explanations(review, &inputs.edits),
        capture: capture(review),
        gate,
        salt,
    }
}

/// The bytes of `paths`, sorted, each once.
fn sorted(paths: &[RepoPath]) -> Vec<PathBytes> {
    let paths: BTreeSet<PathBytes> = paths.iter().map(|p| PathBytes::new(p.as_bytes())).collect();
    paths.into_iter().collect()
}

/// Saves `basis` in `store`, makes it the reviewed worktree's selection
/// unless an agent session runs the review, and cleans the store up.
/// Returns whether it saved: not when the store stayed busy, its writer
/// lock still held after [`LOCK_WAIT`].
///
/// # Errors
///
/// Returns the store errors of writing and committing the store.
fn save(g: &Gathered, store: &ReviewStore, basis: &Basis) -> Result<bool, CliError> {
    let Some(mut tx) = begin_within(store, LOCK_WAIT)? else {
        return Ok(false);
    };
    let now = UtcTime::now();
    let id = tx.save_basis(basis, now)?;
    if !in_agent_session(|name| std::env::var_os(name)) {
        tx.select(&g.worktree, &id);
    }
    tx.cleanup(now)?;
    tx.commit()?;
    Ok(true)
}

/// Builds the review model from the gathered inputs.
///
/// # Errors
///
/// Returns [`CliError::Git`] if the diff, a blob, or a conversion cannot be
/// read.
fn build(g: &Gathered) -> Result<Review, CliError> {
    let mut changes = changed_paths(&g.repo, &g.range.base.tree, &g.target_tree, &g.paths)?;
    changes.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let blobs = diffable_blobs(g, &changes)?;
    let mention_worktrees = g.observed_worktrees();
    let commands = g
        .evidence
        .candidate_commands(&g.candidate_transcripts, &mention_worktrees);
    let mut files = Vec::new();
    let mut edits: BTreeMap<String, EditInfo> = BTreeMap::new();
    for change in &changes {
        let (file, attribution) = review_file(g, change, &blobs, &commands)?;
        // Only the edits the file's analysis names.
        if let Some(attribution) = attribution {
            add_referenced_edits(
                &mut edits,
                &attribution.accepted,
                &file.referenced_edits(),
                &g.evidence,
            );
        }
        files.push(file);
    }
    let scope = scope_info(g);
    Ok(Review {
        schema: REVIEW_SCHEMA,
        basis: None,
        basis_unsaved: None,
        worktree: g.worktree.display().to_string(),
        base: base_info(g),
        target: target_info(g),
        status: StatusInfo::default(),
        gate: GateInfo::default(),
        unattributed: unattributed_summary(&files, &scope),
        scope,
        origins: origins_info(g),
        records: records_info(g),
        evidence: evidence_info(g, all_conflicts(g)),
        files,
        edits,
    })
}

/// Every side that can be diffed (not classified away by mode or path):
/// its bytes when within [`MAX_DIFF_BYTES`], its size otherwise.
///
/// # Errors
///
/// Returns [`CliError::Git`] if the sizes or the blobs cannot be read.
fn diffable_blobs(g: &Gathered, changes: &[Change]) -> Result<BTreeMap<ObjectId, Blob>, CliError> {
    let ids: Vec<ObjectId> = changes
        .iter()
        .filter(|c| classify_change(c).is_none())
        .flat_map(|c| [c.old_blob.clone(), c.new_blob.clone()])
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(read_blobs_within(&g.repo, &ids, MAX_DIFF_BYTES)?)
}

/// Reviews one changed path; returns anchor's result with it when the path
/// was attributed.
///
/// # Errors
///
/// Returns [`CliError::Git`] if a conversion cannot be run.
fn review_file(
    g: &Gathered,
    change: &Change,
    blobs: &BTreeMap<ObjectId, Blob>,
    commands: &CandidateCommands<'_>,
) -> Result<(FileReview, Option<PathAttribution>), CliError> {
    // Both references borrow from the map, so they outlive the lookup key.
    let side = |id: Option<&ObjectId>| match id.and_then(|id| blobs.get_key_value(id)) {
        Some((id, Blob::Read(bytes))) => Some((id, bytes.as_slice())),
        _ => None,
    };
    let (old, new) = (
        side(change.old_blob.as_ref()),
        side(change.new_blob.as_ref()),
    );
    let too_large = [&change.old_blob, &change.new_blob]
        .into_iter()
        .flatten()
        .any(|id| matches!(blobs.get(id), Some(Blob::TooLarge(_))));
    let kind = classify_change(change).unwrap_or_else(|| {
        if too_large {
            FileKind::TooLarge
        } else {
            classify_bytes(old.map(|(_, b)| b), new.map(|(_, b)| b))
        }
    });
    let text = kind == FileKind::Text;
    // Listed on every file its sightings touched, whatever the file's kind
    // and whether or not attribution ran.
    let conflicts = conflicts_for(g, &change.path);
    let (bytes_status, attribution) = if text {
        let bytes = path_bytes(&g.repo, &change.path, old, new)?;
        let attribution = attribute_path(g, &change.path, bytes.base, bytes.target, &conflicts);
        (bytes.status, attribution)
    } else {
        let not_read = BytesStatus {
            base: "not read".to_owned(),
            target: "not read".to_owned(),
        };
        (
            not_read,
            Err(format!("not diffed: {}", file_kind(kind).as_str())),
        )
    };
    let path = path_info(&change.path);
    let command_changes: Vec<CommandChange> =
        g.command_changes.touching(&change.path).cloned().collect();
    let mentions = change
        .path
        .to_str()
        .map(|p| commands.mentions(p))
        .unwrap_or_default();
    let mut file = file_review(FileInput {
        path: &path,
        status: file_status(change.status),
        old_mode: mode_text(change.old_mode),
        new_mode: mode_text(change.new_mode),
        old_blob: change.old_blob.as_ref().map(ToString::to_string),
        new_blob: change.new_blob.as_ref().map(ToString::to_string),
        kind: file_kind(kind),
        bytes_status,
        base_blob: old.filter(|_| text).map(|(_, b)| b),
        target_blob: new.filter(|_| text).map(|(_, b)| b),
        attribution: attribution_state(&attribution),
        mentions,
        command_changes,
        conflicting_edits: conflicts,
    });
    // Keep mentions for deletions and files with unattributed spans.
    if file.status != FileStatus::Deleted && !has_unattributed(&file) {
        file.mentions.clear();
    }
    Ok((file, attribution.ok()))
}

/// A path's display forms and raw bytes.
fn path_info(path: &RepoPath) -> RepoPathInfo {
    RepoPathInfo {
        display: path.display(),
        escaped: path.to_str().is_none().then(|| path.escaped()),
        bytes: PathBytes::new(path.as_bytes()),
    }
}

/// Whether any span of `file` is unattributed.
fn has_unattributed(file: &FileReview) -> bool {
    file.hunks
        .iter()
        .flat_map(|h| &h.spans)
        .any(|s| matches!(s.outcome, Outcome::Unattributed { .. }))
}

/// The scope block: options, untracked files (those a recorded edit names
/// first, each with the flag that includes it), paths whose on-disk state
/// was not captured, unmerged paths, and the ancestry note.
fn scope_info(g: &Gathered) -> ScopeInfo {
    let mut info = ScopeInfo {
        include_untracked: g.include_untracked.iter().map(RepoPath::display).collect(),
        paths: g.paths.iter().map(RepoPath::display).collect(),
        untracked_excluded: Vec::new(),
        not_captured: Vec::new(),
        skip_worktree_absent: 0,
        unmerged: Vec::new(),
        path_note: g.whole_worktree.then(|| {
            "a path selector names the worktree root, so the review covers the whole worktree"
                .to_owned()
        }),
        ancestry_note: (g.range.ancestry == Ancestry::NotAncestor)
            .then(|| ANCESTRY_NOTE.to_owned()),
    };
    let Some(snapshot) = &g.snapshot else {
        return info;
    };
    for path in &snapshot.untracked {
        let recorded_edit = g.sightings_of(path).next().is_some();
        let forms = path_info(path);
        info.untracked_excluded.push(UntrackedInfo {
            path: forms.display,
            path_escaped: forms.escaped,
            path_bytes: forms.bytes,
            recorded_edit,
            include_flag: format!(
                "--include-untracked {}",
                display_from(&g.worktree, &g.cwd, path)
            ),
        });
    }
    info.untracked_excluded.sort_by(|a, b| {
        b.recorded_edit
            .cmp(&a.recorded_edit)
            .then_with(|| a.path.cmp(&b.path))
    });
    info.not_captured = snapshot
        .not_captured
        .iter()
        .map(|n| {
            let forms = path_info(&n.path);
            NotCapturedInfo {
                path: forms.display,
                path_escaped: forms.escaped,
                path_bytes: forms.bytes,
                on_disk: on_disk_word(n.on_disk).to_owned(),
                cause: cause_word(n.cause).to_owned(),
            }
        })
        .collect();
    info.skip_worktree_absent = snapshot.skip_worktree_absent;
    info.unmerged = snapshot.unmerged.iter().map(RepoPath::display).collect();
    info
}
