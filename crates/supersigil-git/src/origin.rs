//! Which worktree created a reviewed commit, from each worktree's HEAD reflog.
//!
//! A worktree originates a commit when its own HEAD reflog has a
//! commit-family entry for it. The association is computed at review time
//! and never stored; an expired reflog or a pruned worktree simply leaves a
//! commit without an origin, which the review reports.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::GitError;
use crate::oid::ObjectId;
use crate::repo::Repo;
use crate::snapshot::nul_fields;
use crate::worktree::Worktree;

/// Actions of the sequencer (rebase, cherry-pick, revert, and pull with
/// rebase) that create a commit, as the parenthesized suffix git appends.
const SEQUENCER_SUFFIXES: [&str; 7] = [
    " (pick)",
    " (reword)",
    " (edit)",
    " (squash)",
    " (fixup)",
    " (continue)",
    " (merge)",
];

/// Returns whether a reflog subject records the creation of its commit.
///
/// The action is the text before the first `": "` (refnames cannot contain a
/// space, so a refspec such as `main:main` stays inside the action) and the
/// message is the rest. An entry is an origin when:
///
/// - its action is `commit` or `commit (…)`, or `am`, whatever the message
///   (a commit may be titled "Fast-forward handling fix");
/// - its action is `cherry-pick` or `revert`, or ends with a sequencer
///   suffix such as ` (pick)` whatever precedes it (`rebase`, `rebase -i`,
///   `pull --rebase origin main`), and its message is not exactly
///   `fast-forward`: the sequencer writes `<action>: fast-forward` when it
///   moves HEAD to an existing commit, and a commit whose whole title is
///   `fast-forward` is indistinguishable, so it is conservatively not an
///   origin;
/// - its action starts with `merge ` or `pull` and its message starts with
///   `Merge made by` (fast-forward merges and pulls never match).
///
/// Any other action, including a bare `rebase`, is not evidence. Git writes
/// these subjects in English regardless of locale.
#[must_use]
pub fn is_origin_subject(subject: &str) -> bool {
    let Some((action, message)) = subject.split_once(": ") else {
        return false;
    };
    if action == "commit"
        || action == "am"
        || (action.starts_with("commit (") && action.ends_with(')'))
    {
        return true;
    }
    let sequencer = action == "cherry-pick"
        || action == "revert"
        || SEQUENCER_SUFFIXES
            .iter()
            .any(|suffix| action.ends_with(suffix));
    if sequencer {
        return message != "fast-forward";
    }
    (action.starts_with("merge ") || action.starts_with("pull"))
        && message.starts_with("Merge made by")
}

/// One entry of a worktree's HEAD reflog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    /// The commit HEAD moved to.
    pub commit: ObjectId,
    /// The reflog subject, for example `commit: add greet`.
    pub subject: String,
}

/// Reads the HEAD reflog of the worktree at `worktree`, newest first. Each
/// linked worktree has its own. A worktree without a reflog (logging
/// disabled, or an unborn HEAD) has no entries.
///
/// # Errors
///
/// Returns the errors of [`crate::Git::raw`], or [`GitError::Parse`] for
/// entries this function does not understand.
pub fn head_reflog(repo: &Repo, worktree: &Path) -> Result<Vec<ReflogEntry>, GitError> {
    let output = repo.git().in_dir(worktree).raw([
        "log",
        "-g",
        "-z",
        "--no-abbrev",
        "--no-color",
        "--no-show-signature",
        "--format=%H%x1f%gs",
        "HEAD",
        "--",
    ])?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for record in nul_fields(&output.stdout) {
        let record = String::from_utf8_lossy(record);
        let record = record.trim_matches('\n');
        let (commit, subject) = record
            .split_once('\u{1f}')
            .ok_or_else(|| GitError::Parse(format!("reflog entry {record:?}")))?;
        entries.push(ReflogEntry {
            commit: ObjectId::parse(commit, repo.format())?,
            subject: subject.to_owned(),
        });
    }
    Ok(entries)
}

/// A reviewed commit and the worktrees that created it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitOrigin {
    /// The commit.
    pub commit: ObjectId,
    /// Worktrees whose HEAD reflog records creating it, sorted.
    pub worktrees: Vec<PathBuf>,
}

/// A worktree whose reflog could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableOrigin {
    /// The worktree's registered path.
    pub worktree: PathBuf,
    /// Why its evidence is unavailable.
    pub reason: String,
}

/// Origins of the reviewed commits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Origins {
    /// Commits with at least one origin, in the order they were given.
    pub commits: Vec<CommitOrigin>,
    /// Commits no readable reflog records creating, in the order given.
    pub without_origin: Vec<ObjectId>,
    /// Registered worktrees whose directory is missing.
    pub unavailable: Vec<UnavailableOrigin>,
}

impl Origins {
    /// Returns every worktree that originated at least one reviewed commit.
    #[must_use]
    pub fn origin_worktrees(&self) -> BTreeSet<PathBuf> {
        self.commits
            .iter()
            .flat_map(|origin| origin.worktrees.iter().cloned())
            .collect()
    }
}

/// Returns whether `path` is a directory. A path that does not exist is
/// `false`; any other failure to look at it is an error, not an absence.
fn directory_exists(path: &Path) -> Result<bool, GitError> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(GitError::Io {
            context: format!("checking worktree directory {}", path.display()),
            source,
        }),
    }
}

/// Finds, for each of `commits`, the worktrees whose own HEAD reflog has a
/// commit-family entry for it ([`is_origin_subject`]). Bare entries are
/// skipped; a prunable worktree or one whose directory is missing is listed
/// as unavailable.
///
/// # Errors
///
/// Returns the errors of [`head_reflog`], or [`GitError::Io`] when a
/// worktree directory cannot be inspected for a reason other than not
/// existing.
pub fn find_origins(
    repo: &Repo,
    worktrees: &[Worktree],
    commits: &[ObjectId],
) -> Result<Origins, GitError> {
    let wanted: BTreeSet<&ObjectId> = commits.iter().collect();
    let mut found: BTreeMap<ObjectId, BTreeSet<PathBuf>> = BTreeMap::new();
    let mut unavailable = Vec::new();
    for worktree in worktrees.iter().filter(|w| !w.bare) {
        if worktree.prunable || !directory_exists(&worktree.path)? {
            unavailable.push(UnavailableOrigin {
                worktree: worktree.path.clone(),
                reason: "worktree directory is missing".to_owned(),
            });
            continue;
        }
        for entry in head_reflog(repo, &worktree.path)? {
            if wanted.contains(&entry.commit) && is_origin_subject(&entry.subject) {
                found
                    .entry(entry.commit)
                    .or_default()
                    .insert(worktree.path.clone());
            }
        }
    }
    let mut origins = Origins {
        unavailable,
        ..Origins::default()
    };
    let mut seen = BTreeSet::new();
    for commit in commits.iter().filter(|c| seen.insert(*c)) {
        match found.get(commit) {
            Some(worktrees) => origins.commits.push(CommitOrigin {
                commit: commit.clone(),
                worktrees: worktrees.iter().cloned().collect(),
            }),
            None => origins.without_origin.push(commit.clone()),
        }
    }
    Ok(origins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_subjects_follow_the_grammar() {
        let cases = [
            ("commit: add greet", true),
            ("commit (initial): init", true),
            ("commit (amend): add greet", true),
            ("commit (merge): Merge branch 'x'", true),
            ("commit: Fast-forward handling fix", true),
            ("commit: fast-forward", true),
            ("am: add greet", true),
            ("cherry-pick: add greet", true),
            ("cherry-pick: fast-forward", false),
            ("revert: Revert \"add greet\"", true),
            ("revert: fast-forward", false),
            ("rebase (pick): add greet", true),
            ("rebase -i (pick): add greet", true),
            ("rebase (reword): add greet", true),
            ("rebase (edit): add greet", true),
            ("rebase (squash): add greet", true),
            ("rebase (fixup): add greet", true),
            ("rebase (continue): add greet", true),
            ("rebase (merge): Merge branch 'x'", true),
            ("rebase (pick): fast-forward", false),
            ("pull --rebase origin main (pick): local", true),
            ("pull -q --rebase origin main (pick): local", true),
            ("merge feat: Merge made by the 'ort' strategy.", true),
            ("merge feat: Fast-forward", false),
            (
                "pull -q --no-rebase --no-edit origin main: Merge made by the 'ort' strategy.",
                true,
            ),
            (
                "pull origin main:main: Merge made by the 'ort' strategy.",
                true,
            ),
            ("pull: Fast-forward", false),
            ("pull origin main: Fast-forward", false),
            ("rebase (start): checkout main", false),
            ("rebase (finish): returning to refs/heads/feat", false),
            ("rebase: fast-forward", false),
            ("checkout: moving from main to feat", false),
            ("reset: moving to HEAD~1", false),
            ("branch: Created from HEAD", false),
            ("commit", false),
            ("", false),
        ];
        for (subject, expected) in cases {
            assert_eq!(is_origin_subject(subject), expected, "{subject:?}");
        }
    }
}
