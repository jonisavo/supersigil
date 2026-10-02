//! `supersigil review`: the change between a base and a target, each changed
//! line attributed to the recorded edits that produced it, or the reason it
//! is not.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write as _};

use supersigil_anchor::PathAttribution;
use supersigil_git::bytes::{Blob, read_blobs_within};
use supersigil_git::changes::{
    Change, FileKind, MAX_DIFF_BYTES, changed_paths, classify_bytes, classify_change,
};
use supersigil_git::{Ancestry, ObjectId, RepoPath, TargetSpec};
use supersigil_review::model::{
    ANCESTRY_NOTE, BytesStatus, CommandChange, EditInfo, FileInput, FileReview, FileStatus,
    NotCapturedInfo, RepoPathInfo, ScopeInfo, UntrackedInfo, file_review, unattributed_summary,
};
use supersigil_review::outcome::Outcome;
use supersigil_review::summary::render_summary;
use supersigil_review::{REVIEW_SCHEMA, Review};

use crate::commands::ReviewArgs;
use crate::error::CliError;
use crate::evidence::{CandidateCommands, add_referenced_edits};
use crate::format::{OutputFormat, escape_control, write_json};
use crate::pipeline::{
    BaseChoice, Gathered, PipelineArgs, all_conflicts, attribute_path, attribution_state,
    base_info, cause_word, claude_home, conflicts_for, display_from, evidence_info, file_kind,
    file_status, gather, mode_text, on_disk_word, origins_info, path_bytes, records_info,
    same_path, target_info,
};
use crate::record_dir;

/// Runs `supersigil review`.
///
/// # Errors
///
/// Returns [`CliError`] when the repository, a revision, a blob, or a record
/// cannot be read (the errors of [`gather`]), or when writing the output
/// fails.
pub fn run(args: &ReviewArgs) -> Result<(), CliError> {
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
    })?;
    let review = build(&g)?;
    match args.format.resolve() {
        OutputFormat::Json => write_json(&review)?,
        OutputFormat::Terminal => io::stdout()
            .lock()
            .write_all(render_summary(&review, escape_control).as_bytes())?,
    }
    Ok(())
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
        worktree: g.worktree.display().to_string(),
        base: base_info(g),
        target: target_info(g),
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
    let path = RepoPathInfo {
        display: change.path.display(),
        escaped: change
            .path
            .to_str()
            .is_none()
            .then(|| change.path.escaped()),
    };
    let command_changes: Vec<CommandChange> =
        g.command_changes.touching(&change.path).cloned().collect();
    // A command the harness reported as changing the file is listed there,
    // not again as a mention of its path.
    let mut mentions = change
        .path
        .to_str()
        .map(|p| commands.mentions(p))
        .unwrap_or_default();
    let changing: BTreeSet<&str> = command_changes.iter().map(|c| c.command.as_str()).collect();
    mentions.retain(|m| !changing.contains(m.command.as_str()));
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

/// Whether any span of `file` is unattributed.
fn has_unattributed(file: &FileReview) -> bool {
    file.hunks
        .iter()
        .flat_map(|h| &h.spans)
        .any(|s| matches!(s.outcome, Outcome::Unattributed { .. }))
}

/// The scope block: options, untracked files (those a recorded edit wrote
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
        let recorded_edit = g
            .candidates
            .iter()
            .any(|m| m.candidate.worktree == g.worktree && same_path(&m.path, path, g.ignore_case));
        info.untracked_excluded.push(UntrackedInfo {
            path: path.display(),
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
        .map(|n| NotCapturedInfo {
            path: n.path.display(),
            on_disk: on_disk_word(n.on_disk).to_owned(),
            cause: cause_word(n.cause).to_owned(),
        })
        .collect();
    info.skip_worktree_absent = snapshot.skip_worktree_absent;
    info.unmerged = snapshot.unmerged.iter().map(RepoPath::display).collect();
    info
}
