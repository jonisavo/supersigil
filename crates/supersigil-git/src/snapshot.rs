//! Captures the working tree as a git tree through a temporary index.
//!
//! The real index is copied (keeping its modification time), `git add -u`
//! and the listed untracked paths are applied to the copy, and `write-tree`
//! writes the result. The real index, the worktree, and every ref stay
//! untouched; the only writes are new objects, a refreshed modification time
//! on a split index's shared file (git does that on every read), and
//! whatever configured clean filters do on their own, exactly as `git add`
//! would. Paths whose on-disk state this procedure does not capture are
//! listed, never silently dropped.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::error::GitError;
use crate::oid::{ObjectId, parse_output};
use crate::path::{RepoPath, path_from_git};
use crate::repo::Repo;
use crate::run::{Git, io_error};

/// What to include beyond tracked changes, and which paths the listings cover.
#[derive(Debug, Clone, Default)]
pub struct SnapshotOptions {
    /// Untracked paths to include, as given with `--include-untracked`.
    pub include_untracked: Vec<RepoPath>,
    /// Literal path prefixes that limit the listings; empty for everything.
    /// The tree itself always covers the whole worktree.
    pub pathspecs: Vec<RepoPath>,
}

/// Whether a path whose contents were not captured exists on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnDisk {
    /// The file exists; its contents were not captured.
    Present,
    /// The file is missing; its deletion was not captured.
    Missing,
}

/// Why a tracked path's on-disk state is not in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotCapturedCause {
    /// The index entry is flagged assume-unchanged, so git does not look at
    /// the file.
    AssumeUnchanged,
    /// The index entry is flagged skip-worktree and the file exists.
    SkipWorktree,
    /// `git add -u` left it out for another reason, reported by `diff-files`
    /// (for example a file outside the sparse-checkout patterns whose
    /// skip-worktree bit git cleared, or a submodule configured
    /// `ignore = all`).
    NotStaged,
}

/// A tracked path whose on-disk state the tree does not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCaptured {
    /// The path.
    pub path: RepoPath,
    /// Whether the file exists.
    pub on_disk: OnDisk,
    /// Why it was not captured.
    pub cause: NotCapturedCause,
}

/// The working tree written as a tree, with everything the capture left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingTreeSnapshot {
    /// The tree holding the captured state.
    pub tree: ObjectId,
    /// Tracked paths whose on-disk state is not in the tree.
    pub not_captured: Vec<NotCaptured>,
    /// Skip-worktree entries whose file is absent; they keep their index
    /// state, which is the normal state in a sparse checkout.
    pub skip_worktree_absent: usize,
    /// Paths unmerged in the real index; the tree holds their on-disk form,
    /// conflict markers included.
    pub unmerged: Vec<RepoPath>,
    /// Untracked, non-ignored paths that were not included.
    pub untracked: Vec<RepoPath>,
}

/// Writes the working tree of `repo` as a tree, as `git add -u` plus
/// `options.include_untracked` would stage it, without touching the real
/// index.
///
/// # Errors
///
/// Returns [`GitError::Untracked`] if an included path is missing or
/// ignored, [`GitError::Io`] if the temporary index cannot be prepared, or
/// the errors of the git commands it runs.
pub fn snapshot_working_tree(
    repo: &Repo,
    options: &SnapshotOptions,
) -> Result<WorkingTreeSnapshot, GitError> {
    let pathspecs: Vec<OsString> = options
        .pathspecs
        .iter()
        .filter_map(RepoPath::literal_pathspec)
        .collect();
    // Read from the real index before anything else, without the override.
    let unmerged = unmerged_paths(repo, &pathspecs)?;
    let index = real_index_path(repo)?;

    let temp = tempfile::Builder::new()
        .prefix("supersigil-snapshot-")
        .tempdir()
        .map_err(|e| io_error("cannot create a temporary directory", e))?;
    let temp_index = temp.path().join("index");
    let hooks = temp.path().join("hooks");
    std::fs::create_dir(&hooks)
        .map_err(|e| io_error("cannot create an empty hooks directory", e))?;
    if index.exists() {
        copy_index(&index, &temp_index)?;
    }
    // A missing temporary index reads as an empty one: a fresh repository.
    let staging = Staging {
        git: repo.git().clone().with_env("GIT_INDEX_FILE", &temp_index),
        config: isolation_config(&hooks),
    };

    staging.run(&["add".into(), "-u".into()])?;
    for path in &options.include_untracked {
        include(repo, &staging, path)?;
    }
    let mut not_captured = not_staged(repo, &staging, &pathspecs)?;
    let (flagged, skip_worktree_absent) = flagged_entries(repo, &staging, &pathspecs)?;
    not_captured.extend(flagged);
    let untracked = untracked_paths(&staging, &pathspecs)?;
    let tree = parse_output(&staging.run(&["write-tree".into()])?, repo.format())?;
    Ok(WorkingTreeSnapshot {
        tree,
        not_captured,
        skip_worktree_absent,
        unmerged,
        untracked,
    })
}

/// Git pointed at the temporary index, with the configuration that keeps it
/// from writing anything but the temporary index and objects.
struct Staging {
    git: Git,
    config: Vec<OsString>,
}

impl Staging {
    /// Runs git with the isolation configuration before `args`.
    fn run(&self, args: &[OsString]) -> Result<Vec<u8>, GitError> {
        self.git.output(self.config.iter().chain(args))
    }

    /// Like [`Staging::run`], but returns the raw output whatever the status.
    fn raw(&self, args: &[OsString]) -> Result<std::process::Output, GitError> {
        self.git.raw(self.config.iter().chain(args))
    }
}

/// `-c` options for every command on the temporary index: no split index (so
/// no shared-index files are written into the git directory), no fsmonitor
/// daemon state, and an empty hooks directory, because writing any index,
/// including an alternate one, runs the `post-index-change` hook.
fn isolation_config(hooks: &Path) -> Vec<OsString> {
    let mut hooks_path = OsString::from("core.hooksPath=");
    hooks_path.push(hooks);
    vec![
        "-c".into(),
        "core.splitIndex=false".into(),
        "-c".into(),
        "core.fsmonitor=false".into(),
        "-c".into(),
        hooks_path,
    ]
}

/// Resolves the real index path. `rev-parse --git-path` honors
/// `GIT_INDEX_FILE`, which [`Git::command`] removes, so this is the real one.
fn real_index_path(repo: &Repo) -> Result<PathBuf, GitError> {
    let output = repo.git().output(["rev-parse", "--git-path", "index"])?;
    // `rev-parse --git-path` has no `-z` form; strip only its line terminator,
    // because a path may end in spaces.
    let path = path_from_git(output.strip_suffix(b"\n").unwrap_or(&output))
        .ok_or_else(|| GitError::Parse("index path not representable".to_owned()))?;
    Ok(if path.is_absolute() {
        path
    } else {
        repo.root().join(path)
    })
}

/// Copies the index and gives the copy the source's modification time, read
/// from the same open file. Git decides whether a cached stat entry is racily
/// clean by comparing it with the index file's own mtime; a newer copy would
/// let a file changed in the same instant as the last index write pass as
/// clean.
pub(crate) fn copy_index(source: &Path, target: &Path) -> Result<(), GitError> {
    let mut from = File::open(source).map_err(|e| io_error("cannot open the index", e))?;
    let modified = from
        .metadata()
        .and_then(|m| m.modified())
        .map_err(|e| io_error("cannot read the index's modification time", e))?;
    let mut to =
        File::create(target).map_err(|e| io_error("cannot create the temporary index", e))?;
    std::io::copy(&mut from, &mut to).map_err(|e| io_error("cannot copy the index", e))?;
    to.set_modified(modified)
        .map_err(|e| io_error("cannot set the temporary index's modification time", e))
}

/// Adds one listed untracked path to the temporary index.
fn include(repo: &Repo, staging: &Staging, path: &RepoPath) -> Result<(), GitError> {
    let refuse = |reason: &str| GitError::Untracked {
        path: path.display(),
        reason: reason.to_owned(),
    };
    let (Some(relative), Some(spec)) = (path.to_path(), path.literal_pathspec()) else {
        return Err(refuse("not representable on this platform"));
    };
    if repo.root().join(&relative).symlink_metadata().is_err() {
        return Err(refuse("no such file"));
    }
    let ignored = staging.raw(&[
        "check-ignore".into(),
        "-q".into(),
        "--".into(),
        relative.into(),
    ])?;
    if ignored.status.success() {
        return Err(refuse("ignored"));
    }
    staging.run(&["add".into(), "--".into(), spec])?;
    Ok(())
}

/// Paths `diff-files` still reports after staging: tracked content whose
/// on-disk state `add -u` did not capture, whatever the reason. The flag
/// matters for submodules configured `ignore = all`, whose changed gitlink
/// plain `diff-files` would hide.
fn not_staged(
    repo: &Repo,
    staging: &Staging,
    pathspecs: &[OsString],
) -> Result<Vec<NotCaptured>, GitError> {
    let mut args: Vec<OsString> = vec![
        "diff-files".into(),
        "-z".into(),
        "--name-status".into(),
        "--ignore-submodules=dirty".into(),
        "--".into(),
    ];
    args.extend(pathspecs.iter().cloned());
    let output = staging.run(&args)?;
    let fields: Vec<&[u8]> = nul_fields(&output).collect();
    let mut listed = Vec::new();
    for pair in fields.chunks(2) {
        let [status, path] = pair else {
            return Err(GitError::Parse(
                "diff-files output ends mid-record".to_owned(),
            ));
        };
        let path = RepoPath::new(path.to_vec());
        let on_disk = if status.first() == Some(&b'D') {
            OnDisk::Missing
        } else {
            on_disk(repo, &path)
        };
        listed.push(NotCaptured {
            path,
            on_disk,
            cause: NotCapturedCause::NotStaged,
        });
    }
    Ok(listed)
}

/// Entries flagged assume-unchanged or skip-worktree, from `ls-files -v`:
/// `diff-files` trusts both flags just as `add -u` does, so they are listed
/// from the flags. Returns the listed entries and the number of
/// skip-worktree entries whose file is absent.
fn flagged_entries(
    repo: &Repo,
    staging: &Staging,
    pathspecs: &[OsString],
) -> Result<(Vec<NotCaptured>, usize), GitError> {
    let mut args: Vec<OsString> = vec!["ls-files".into(), "-v".into(), "-z".into(), "--".into()];
    args.extend(pathspecs.iter().cloned());
    let output = staging.run(&args)?;
    let mut listed = Vec::new();
    let mut absent = 0;
    for entry in nul_fields(&output) {
        let [tag, b' ', path @ ..] = entry else {
            return Err(GitError::Parse(format!(
                "ls-files -v entry {:?}",
                String::from_utf8_lossy(entry)
            )));
        };
        let path = RepoPath::new(path.to_vec());
        if tag.is_ascii_lowercase() {
            listed.push(NotCaptured {
                on_disk: on_disk(repo, &path),
                path,
                cause: NotCapturedCause::AssumeUnchanged,
            });
        } else if *tag == b'S' {
            match on_disk(repo, &path) {
                OnDisk::Present => listed.push(NotCaptured {
                    path,
                    on_disk: OnDisk::Present,
                    cause: NotCapturedCause::SkipWorktree,
                }),
                OnDisk::Missing => absent += 1,
            }
        }
    }
    Ok((listed, absent))
}

/// Paths with an unmerged entry in the real index.
fn unmerged_paths(repo: &Repo, pathspecs: &[OsString]) -> Result<Vec<RepoPath>, GitError> {
    let mut args: Vec<OsString> = vec!["ls-files".into(), "-u".into(), "-z".into(), "--".into()];
    args.extend(pathspecs.iter().cloned());
    let output = repo.git().output(&args)?;
    let mut paths = BTreeSet::new();
    for entry in nul_fields(&output) {
        let tab = entry
            .iter()
            .position(|&b| b == b'\t')
            .ok_or_else(|| GitError::Parse("ls-files -u entry without a tab".to_owned()))?;
        paths.insert(RepoPath::new(entry[tab + 1..].to_vec()));
    }
    Ok(paths.into_iter().collect())
}

/// Untracked, non-ignored paths still untracked after staging.
fn untracked_paths(staging: &Staging, pathspecs: &[OsString]) -> Result<Vec<RepoPath>, GitError> {
    let mut args: Vec<OsString> = vec![
        "ls-files".into(),
        "--others".into(),
        "--exclude-standard".into(),
        "-z".into(),
        "--".into(),
    ];
    args.extend(pathspecs.iter().cloned());
    let output = staging.run(&args)?;
    Ok(nul_fields(&output)
        .map(|path| RepoPath::new(path.to_vec()))
        .collect())
}

/// Whether `path` exists under the worktree root (a dangling symlink exists).
fn on_disk(repo: &Repo, path: &RepoPath) -> OnDisk {
    let exists = path
        .to_path()
        .is_some_and(|relative| repo.root().join(relative).symlink_metadata().is_ok());
    if exists {
        OnDisk::Present
    } else {
        OnDisk::Missing
    }
}

/// Splits NUL-terminated output into its non-empty fields.
pub(crate) fn nul_fields(output: &[u8]) -> impl Iterator<Item = &[u8]> {
    output.split(|&b| b == 0).filter(|field| !field.is_empty())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn copied_index_keeps_the_source_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("index");
        std::fs::write(&source, b"DIRC").unwrap();
        let past = SystemTime::now() - Duration::from_secs(3600);
        File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_modified(past)
            .unwrap();
        let target = dir.path().join("copy");
        copy_index(&source, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"DIRC");
        let modified = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        assert_eq!(modified(&target), modified(&source));
    }

    #[test]
    fn nul_fields_skip_the_terminator() {
        let fields: Vec<&[u8]> = nul_fields(b"a\0b c\0").collect();
        assert_eq!(fields, [&b"a"[..], &b"b c"[..]]);
        assert_eq!(nul_fields(b"").count(), 0);
    }
}
