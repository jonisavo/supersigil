//! `supersigil why <file>:<line>`: where one line of a file came from,
//! explained back to the file's creation when the evidence reaches that far.

use std::collections::BTreeMap;
use std::io::{self, Write as _};

use supersigil_anchor::lines::split_lines;
use supersigil_git::bytes::{blob_sizes, read_blobs};
use supersigil_git::changes::{
    Change, FileKind, MAX_DIFF_BYTES, Mode, changed_paths, classify_bytes,
};
use supersigil_git::snapshot::{SnapshotOptions, snapshot_working_tree};
use supersigil_git::{ObjectId, RepoPath, ResolvedTarget, TargetSpec};
use supersigil_review::WHY_SCHEMA;
use supersigil_review::diff::diff_lines;
use supersigil_review::mapping::lines_correspond;
use supersigil_review::model::EditInfo;
use supersigil_review::outcome::AttributionState;
use supersigil_review::why::{
    OnDiskCheck, Tristate, Why, WhyLine, WhyTarget, render_why, why_line,
};

use crate::commands::WhyArgs;
use crate::error::CliError;
use crate::evidence::edit_info;
use crate::format::{OutputFormat, escape_control, write_json};
use crate::pipeline::{
    BaseChoice, Gathered, PipelineArgs, attribute_path, claude_home, conflicts_for, evidence_info,
    gather, not_captured_reason, path_bytes, records_info, repo_path,
};
use crate::record_dir;

/// Runs `supersigil why`.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] for a malformed location, a path that
/// is not a regular text file of at most [`MAX_DIFF_BYTES`] bytes in the
/// captured working tree, or a line beyond the end of the file; otherwise
/// the errors of [`gather`].
pub fn run(args: &WhyArgs) -> Result<(), CliError> {
    let (typed, line) = parse_location(&args.location)?;
    let cwd = record_dir::canonical_checkout(None)?;
    let g = gather(&PipelineArgs {
        checkout: record_dir::canonical_checkout(args.checkout.as_deref())?,
        cwd: cwd.clone(),
        records_dir: record_dir::resolve_record_dir(args.record_dir.as_deref())?,
        claude_home: claude_home(args.claude_home.as_deref()),
        base: BaseChoice::EmptyTree,
        target: TargetSpec::WorkingTree,
        include_untracked: Vec::new(),
        paths: Vec::new(),
    })?;
    let path = repo_path(&g.worktree, &cwd, &typed)?.ok_or_else(|| {
        CliError::CommandFailed(format!("{typed} names the worktree, not a file"))
    })?;
    let (tree, blob) = target_blob(&g, &path, &typed)?;
    // The size is asked of git first, so the limit bounds what is read.
    if too_large(&g, &blob)? {
        return Err(CliError::CommandFailed(format!(
            "{typed} is larger than {MAX_DIFF_BYTES} bytes; why explains lines of text"
        )));
    }
    let bytes = read_one(&g, &blob)?;
    if classify_bytes(None, Some(&bytes)) != FileKind::Text {
        return Err(CliError::CommandFailed(format!(
            "{typed} is not a text file; why explains lines of text"
        )));
    }
    let side = path_bytes(&g.repo, &path, None, Some((&blob, &bytes)))?;
    let attribution = attribute_path(&g, &path, &side);
    let on_disk = on_disk_check(&g, &path, side.target_worktree_form(), &bytes);
    let target_lines = split_lines(&bytes);
    let analysis = if on_disk == OnDiskCheck::Captured {
        let index = line
            .checked_sub(1)
            .filter(|index| *index < target_lines.len())
            .ok_or_else(|| {
                CliError::CommandFailed(format!(
                    "line {line} is beyond the end of {typed} ({} lines)",
                    target_lines.len()
                ))
            })?;
        let head = head_blob(&g, &path)?;
        let (differs, reason) =
            differs_from_head(head.as_deref().map_err(String::as_str), &bytes, index);
        let state = match &attribution {
            Ok(found) => AttributionState::Available(found),
            Err(reason) => AttributionState::Unavailable {
                reason: reason.clone(),
            },
        };
        Some(why_line(&state, index, &target_lines, &[], differs, reason))
    } else {
        None
    };
    // Only the edits the line's analysis names: none when no line was
    // analyzed.
    let referenced = analysis
        .as_ref()
        .map(WhyLine::referenced_edits)
        .unwrap_or_default();
    let edits: BTreeMap<String, EditInfo> = match &attribution {
        Ok(found) => found
            .accepted
            .iter()
            .filter(|e| referenced.contains(&e.edit.id))
            .map(|e| (e.edit.id.as_str().to_owned(), edit_info(e, &g.evidence)))
            .collect(),
        Err(_) => BTreeMap::new(),
    };
    // Conflicts touching this path, whether or not attribution ran.
    let conflicts = conflicts_for(&g, &path);
    let why = Why {
        schema: WHY_SCHEMA,
        path: path.display(),
        target: WhyTarget {
            worktree: g.worktree.display().to_string(),
            tree: tree.to_string(),
            blob: Some(blob.to_string()),
            attribution_bytes: side.status.target.clone(),
        },
        records: records_info(&g),
        evidence: evidence_info(&g, conflicts.clone()),
        on_disk,
        line: analysis,
        edits,
        conflicting_edits: conflicts,
    };
    match args.format.resolve() {
        OutputFormat::Json => write_json(&why)?,
        OutputFormat::Terminal => io::stdout()
            .lock()
            .write_all(render_why(&why, escape_control).as_bytes())?,
    }
    Ok(())
}

/// Splits `<file>:<line>` at its last colon; the line is 1-based.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when there is no colon, the file is
/// empty, or the line is not a positive number.
fn parse_location(location: &str) -> Result<(String, usize), CliError> {
    let invalid = || CliError::CommandFailed(format!("expected <file>:<line>, got {location}"));
    let (file, line) = location.rsplit_once(':').ok_or_else(invalid)?;
    let line = line
        .parse::<usize>()
        .ok()
        .filter(|line| *line > 0)
        .ok_or_else(invalid)?;
    if file.is_empty() {
        return Err(invalid());
    }
    Ok((file.to_owned(), line))
}

/// The captured tree and the path's blob in it. A tracked path keeps the
/// snapshot `add -u` produced; only a path absent from it and present on
/// disk (the user named an untracked file) is captured again with that one
/// path included.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when the path is in neither capture
/// or is not a regular file there, or the git errors of the snapshot and
/// the diff.
fn target_blob(
    g: &Gathered,
    path: &RepoPath,
    typed: &str,
) -> Result<(ObjectId, ObjectId), CliError> {
    if let Some(entry) = entry_at(g, &g.target_tree, path)? {
        return Ok((g.target_tree.clone(), file_blob(entry, typed)?));
    }
    let on_disk = path.to_path().is_some_and(|p| g.worktree.join(p).is_file());
    if on_disk {
        let options = SnapshotOptions {
            include_untracked: vec![path.clone()],
            pathspecs: Vec::new(),
        };
        let snapshot = snapshot_working_tree(&g.repo, &options)?;
        if let Some(entry) = entry_at(g, &snapshot.tree, path)? {
            let blob = file_blob(entry, typed)?;
            return Ok((snapshot.tree, blob));
        }
    }
    Err(CliError::CommandFailed(format!(
        "{typed} is not in the captured working tree"
    )))
}

/// The entry `path` has in `tree`, found by diffing the empty tree against
/// it: its mode and object as the target side of an addition.
///
/// # Errors
///
/// Returns the git errors of the diff.
fn entry_at(g: &Gathered, tree: &ObjectId, path: &RepoPath) -> Result<Option<Change>, CliError> {
    let empty = g.repo.empty_tree()?;
    Ok(
        changed_paths(&g.repo, &empty, tree, std::slice::from_ref(path))?
            .into_iter()
            .find(|c| &c.path == path),
    )
}

/// The blob of `entry` when it is a regular file. A symbolic link's blob
/// holds its target path and a submodule's object is a commit: neither has
/// lines to explain, and neither is ever attributed.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] naming what the entry is otherwise.
fn file_blob(entry: Change, typed: &str) -> Result<ObjectId, CliError> {
    if entry.new_mode.is_file()
        && let Some(blob) = entry.new_blob
    {
        return Ok(blob);
    }
    let what = match entry.new_mode {
        Mode::SYMLINK => "a symbolic link",
        Mode::GITLINK => "a submodule",
        _ => "not a regular file",
    };
    Err(CliError::CommandFailed(format!(
        "{typed} is {what}; why explains lines of text"
    )))
}

/// Reads one blob.
///
/// # Errors
///
/// Returns the git errors of the read, or [`CliError::CommandFailed`] when
/// git returns nothing for the id.
fn read_one(g: &Gathered, id: &ObjectId) -> Result<Vec<u8>, CliError> {
    read_blobs(&g.repo, std::slice::from_ref(id))?
        .remove(id)
        .ok_or_else(|| CliError::CommandFailed(format!("blob {id} could not be read")))
}

/// The path's bytes at HEAD, or why there is no text to compare with: HEAD
/// is unborn, the path is not in it, it is not a regular file there (a
/// symbolic link's or a submodule's object is no text baseline), or its
/// blob is over [`MAX_DIFF_BYTES`], which is checked before it is read.
///
/// # Errors
///
/// Returns the git errors of reading HEAD's tree, the blob's size, and the
/// blob.
fn head_blob(g: &Gathered, path: &RepoPath) -> Result<Result<Vec<u8>, String>, CliError> {
    let ResolvedTarget::WorkingTree { head: Some(head) } = &g.range.target else {
        return Ok(Err("HEAD is unborn".to_owned()));
    };
    let tree = g.repo.commit_tree(head)?;
    let Some(entry) = entry_at(g, &tree, path)? else {
        return Ok(Err("the file is not in HEAD".to_owned()));
    };
    let Some(id) = entry.new_blob.filter(|_| entry.new_mode.is_file()) else {
        return Ok(Err("the path is not a regular file in HEAD".to_owned()));
    };
    if too_large(g, &id)? {
        return Ok(Err(format!(
            "the file in HEAD is larger than {MAX_DIFF_BYTES} bytes"
        )));
    }
    Ok(Ok(read_one(g, &id)?))
}

/// Whether `blob` is larger than [`MAX_DIFF_BYTES`], asked of git before
/// anything is read.
///
/// # Errors
///
/// Returns the git errors of reading the size, or
/// [`CliError::CommandFailed`] when git reports none for the blob.
fn too_large(g: &Gathered, blob: &ObjectId) -> Result<bool, CliError> {
    let sizes = blob_sizes(&g.repo, std::slice::from_ref(blob))?;
    let size = sizes
        .get(blob)
        .ok_or_else(|| CliError::CommandFailed(format!("blob {blob} has no size")))?;
    Ok(*size > MAX_DIFF_BYTES)
}

/// Whether target line `index` lies in a changed hunk against HEAD, with the
/// reason when that is unknown. A coarse hunk is not aligned, so a line
/// inside it may equal a HEAD line: unknown, never yes.
fn differs_from_head(
    head: Result<&[u8], &str>,
    target: &[u8],
    index: usize,
) -> (Tristate, Option<String>) {
    let head = match head {
        Ok(head) => head,
        Err(reason) => return (Tristate::Unknown, Some(reason.to_owned())),
    };
    let diff = diff_lines(head, target);
    let changed = diff
        .hunks
        .iter()
        .any(|h| (h.target_start..h.target_start + h.target_count).contains(&index));
    match (changed, diff.coarse) {
        (true, true) => (Tristate::Unknown, Some("coarse diff".to_owned())),
        (true, false) => (Tristate::Yes, None),
        (false, _) => (Tristate::No, None),
    }
}

/// Checks that the captured line is what is on disk: a path the snapshot
/// listed as not captured never is; otherwise the file on disk must equal
/// the target in worktree form (`worktree_form`, or the blob when conversion
/// failed) up to line endings. Whether those bytes map onto the blob lines
/// is attribution's concern, not this check's.
fn on_disk_check(
    g: &Gathered,
    path: &RepoPath,
    worktree_form: Option<&[u8]>,
    blob: &[u8],
) -> OnDiskCheck {
    let listed = g
        .snapshot
        .as_ref()
        .and_then(|s| s.not_captured.iter().find(|n| &n.path == path));
    if let Some(entry) = listed {
        return OnDiskCheck::NotCaptured {
            reason: not_captured_reason(entry),
        };
    }
    let expected = worktree_form.unwrap_or(blob);
    let Some(file) = path.to_path().map(|p| g.worktree.join(p)) else {
        return OnDiskCheck::NotCaptured {
            reason: "the path cannot be represented on this platform".to_owned(),
        };
    };
    match std::fs::read(&file) {
        Ok(disk) if lines_correspond(&disk, expected) => OnDiskCheck::Captured,
        Ok(_) => OnDiskCheck::NotCaptured {
            reason: "the file on disk differs from the captured target: its on-disk state was not captured, or it changed during the command".to_owned(),
        },
        Err(e) => OnDiskCheck::NotCaptured {
            reason: format!("the file cannot be read from disk: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::parse_location;

    #[test]
    fn locations_split_at_the_last_colon() {
        assert_eq!(
            parse_location("src/lib.rs:7").unwrap(),
            ("src/lib.rs".to_owned(), 7)
        );
        assert_eq!(
            parse_location(r"C:\repo\a.rs:3").unwrap(),
            (r"C:\repo\a.rs".to_owned(), 3)
        );
        parse_location("src/lib.rs").unwrap_err();
        parse_location("src/lib.rs:0").unwrap_err();
        parse_location(":3").unwrap_err();
        parse_location("a.rs:x").unwrap_err();
    }
}
