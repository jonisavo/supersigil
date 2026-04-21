use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git2::{DiffOptions, Repository};
use thiserror::Error;

/// Errors from git operations used in affected-document detection.
#[derive(Debug, Error)]
pub enum GitError {
    /// An underlying `git2` library error.
    #[error("git error: {0}")]
    Git2(#[from] git2::Error),
    /// The given ref could not be resolved.
    #[error("cannot resolve ref '{ref_str}'")]
    UnresolvableRef {
        /// The unresolvable ref string.
        ref_str: String,
    },
}

/// Compute changed files since a reference commit.
///
/// - `committed_only`: only include committed changes (exclude staged + unstaged)
/// - `use_merge_base`: use merge-base of `since_ref` and HEAD instead of `since_ref` directly
///
/// # Errors
///
/// Returns [`GitError::UnresolvableRef`] if `since_ref` cannot be resolved, or
/// [`GitError::Git2`] for other git operations failures.
pub fn changed_files(
    repo_path: &Path,
    since_ref: &str,
    committed_only: bool,
    use_merge_base: bool,
) -> Result<Vec<PathBuf>, GitError> {
    let repo = Repository::discover(repo_path)?;
    let since_obj =
        repo.revparse_single(since_ref)
            .map_err(|_git2_err| GitError::UnresolvableRef {
                ref_str: since_ref.to_owned(),
            })?;

    let base_oid = if use_merge_base {
        let head = repo.head()?.peel_to_commit()?.id();
        repo.merge_base(since_obj.id(), head)?
    } else {
        since_obj.id()
    };

    let base_tree = repo.find_commit(base_oid)?.tree()?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let head_tree = head_commit.tree()?;

    let mut paths = HashSet::new();

    // Committed changes: base..HEAD
    let diff = repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)?;
    collect_diff_paths(&diff, &mut paths);

    if !committed_only {
        // Staged changes: HEAD..index
        let index_diff = repo.diff_tree_to_index(Some(&head_tree), None, None)?;
        collect_diff_paths(&index_diff, &mut paths);

        // Unstaged changes: index..workdir
        let mut opts = DiffOptions::new();
        opts.include_untracked(true);
        let workdir_diff = repo.diff_index_to_workdir(None, Some(&mut opts))?;
        collect_diff_paths(&workdir_diff, &mut paths);
    }

    Ok(paths.into_iter().collect())
}

fn collect_diff_paths(diff: &git2::Diff<'_>, paths: &mut HashSet<PathBuf>) {
    for delta in diff.deltas() {
        if let Some(p) = delta.new_file().path() {
            paths.insert(p.to_owned());
        }
        if let Some(p) = delta.old_file().path() {
            paths.insert(p.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{git_commit, init_repo, sanitize_git_env};

    #[test]
    fn detects_committed_changes_since_ref() {
        let (dir, initial) = init_repo();
        std::fs::write(dir.path().join("new.rs"), "fn main() {}").unwrap();
        git_commit(&dir);

        let files = changed_files(dir.path(), &initial, false, false).unwrap();
        assert!(files.iter().any(|p| p.ends_with("new.rs")));
    }

    #[test]
    fn includes_unstaged_changes_by_default() {
        let (dir, initial) = init_repo();
        std::fs::write(dir.path().join("unstaged.txt"), "hello").unwrap();
        let files = changed_files(dir.path(), &initial, false, false).unwrap();
        assert!(files.iter().any(|p| p.ends_with("unstaged.txt")));
    }

    #[test]
    fn committed_only_excludes_working_tree() {
        let (dir, initial) = init_repo();
        std::fs::write(dir.path().join("unstaged.txt"), "hello").unwrap();
        let files = changed_files(dir.path(), &initial, true, false).unwrap();
        assert!(!files.iter().any(|p| p.ends_with("unstaged.txt")));
    }

    #[test]
    fn merge_base_diffs_against_branch_point() {
        let (dir, _initial) = init_repo();

        // Create a branch from the initial commit
        let git = |args: &[&str]| {
            let mut cmd = std::process::Command::new("git");
            sanitize_git_env(
                cmd.args(args)
                    .current_dir(dir.path())
                    .env("GIT_AUTHOR_NAME", "Test")
                    .env("GIT_AUTHOR_EMAIL", "test@test.com")
                    .env("GIT_COMMITTER_NAME", "Test")
                    .env("GIT_COMMITTER_EMAIL", "test@test.com"),
            )
            .output()
            .expect("git command")
        };

        // Create a feature branch
        git(&["checkout", "-b", "feature"]);
        std::fs::write(dir.path().join("feature.rs"), "fn feature() {}").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "feature work"]);

        // Go back to main and advance it (so `main` moves past the branch point)
        git(&["checkout", "main"]);
        std::fs::write(dir.path().join("main-only.rs"), "fn main_only() {}").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "main advance"]);

        // Back on feature
        git(&["checkout", "feature"]);

        // Without merge-base: diff is main..feature, which sees main-only.rs
        // as "removed" and feature.rs as "added" — both show up
        let files_direct = changed_files(dir.path(), "main", false, false).unwrap();
        assert!(
            files_direct.iter().any(|p| p.ends_with("main-only.rs")),
            "direct diff should include main-only.rs"
        );

        // With merge-base: diff is (merge-base of main and feature)..feature
        // The merge-base is the initial commit, so only feature.rs shows up
        // (main-only.rs was committed on main after the branch point)
        let files_mb = changed_files(dir.path(), "main", false, true).unwrap();
        assert!(
            files_mb.iter().any(|p| p.ends_with("feature.rs")),
            "merge-base diff should include feature.rs"
        );
        // main-only.rs should NOT appear because the merge-base is the initial
        // commit (before main advanced), and feature branch never touched it
        assert!(
            !files_mb.iter().any(|p| p.ends_with("main-only.rs")),
            "merge-base diff should NOT include main-only.rs"
        );
    }

    #[test]
    fn unresolvable_ref_returns_error() {
        let (dir, _) = init_repo();
        let result = changed_files(dir.path(), "nonexistent-ref", false, false);
        result.unwrap_err();
    }
}
