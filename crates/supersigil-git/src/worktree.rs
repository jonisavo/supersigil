//! Lists a repository's worktrees.

use std::path::PathBuf;

use crate::error::GitError;
use crate::oid::ObjectId;
use crate::path::path_from_git;
use crate::repo::Repo;

/// One worktree registered with the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// The worktree's directory, as git prints it.
    pub path: PathBuf,
    /// The commit its HEAD points at, `None` when unborn.
    pub head: Option<ObjectId>,
    /// Whether this entry is the bare repository itself.
    pub bare: bool,
    /// Whether git reports the worktree prunable (its directory is missing).
    pub prunable: bool,
}

/// Lists every worktree of the repository, the main one first, from
/// `worktree list --porcelain -z`.
///
/// # Errors
///
/// Returns the errors of [`crate::Git::output`], or [`GitError::Parse`] for
/// output this function does not understand.
pub fn list_worktrees(repo: &Repo) -> Result<Vec<Worktree>, GitError> {
    let output = repo
        .git()
        .output(["worktree", "list", "--porcelain", "-z"])?;
    parse_porcelain(&output, repo)
}

/// Parses NUL-terminated porcelain attributes; an empty attribute ends a
/// worktree.
fn parse_porcelain(output: &[u8], repo: &Repo) -> Result<Vec<Worktree>, GitError> {
    let mut worktrees = Vec::new();
    let mut current: Option<Worktree> = None;
    for attribute in output.split(|&b| b == 0) {
        if attribute.is_empty() {
            worktrees.extend(current.take());
            continue;
        }
        let (key, value) = match attribute.iter().position(|&b| b == b' ') {
            Some(space) => (&attribute[..space], &attribute[space + 1..]),
            None => (attribute, &b""[..]),
        };
        if key == b"worktree" {
            worktrees.extend(current.take());
            let path = path_from_git(value).ok_or_else(|| {
                GitError::Parse("worktree path not representable on this platform".to_owned())
            })?;
            current = Some(Worktree {
                path,
                head: None,
                bare: false,
                prunable: false,
            });
            continue;
        }
        let Some(worktree) = current.as_mut() else {
            return Err(GitError::Parse(format!(
                "worktree attribute before a worktree: {}",
                String::from_utf8_lossy(attribute)
            )));
        };
        match key {
            b"HEAD" => {
                let id = ObjectId::parse(&String::from_utf8_lossy(value), repo.format())?;
                worktree.head = (!id.is_null()).then_some(id);
            }
            b"bare" => worktree.bare = true,
            b"prunable" => worktree.prunable = true,
            _ => {}
        }
    }
    worktrees.extend(current);
    Ok(worktrees)
}
