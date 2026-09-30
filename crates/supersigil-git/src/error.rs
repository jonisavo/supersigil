//! Errors from running git and reading its output.

use std::path::PathBuf;

use crate::run::GitVersion;

/// Failure to run git, or git output this crate cannot use.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// The `git` program could not be started.
    #[error("git was not found on PATH")]
    NotFound(#[source] std::io::Error),
    /// The installed git is older than [`crate::MIN_VERSION`].
    #[error("git {found} is too old; supersigil needs git {required} or newer")]
    TooOld {
        /// Version reported by `git version`.
        found: GitVersion,
        /// Oldest supported version.
        required: GitVersion,
    },
    /// Git exited unsuccessfully.
    #[error("`git {args}` failed ({}): {stderr}", status_text(*.status))]
    Failed {
        /// Arguments passed to git, joined with spaces.
        args: String,
        /// Exit code, or `None` when git was ended by a signal.
        status: Option<i32>,
        /// Standard error, trimmed.
        stderr: String,
    },
    /// An I/O operation around a git call failed.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Git printed something this crate does not understand.
    #[error("unexpected git output: {0}")]
    Parse(String),
    /// The directory is not inside a git worktree.
    #[error("{} is not inside a git worktree", .0.display())]
    NotAWorktree(PathBuf),
    /// The directory is a bare repository, which has no worktree to review.
    #[error("{} is a bare repository; review needs a worktree", .0.display())]
    Bare(PathBuf),
    /// A revision did not resolve to a commit.
    #[error("unknown revision: {0}")]
    UnknownRevision(String),
    /// A path given to include as untracked could not be included.
    #[error("cannot include untracked path {path}: {reason}")]
    Untracked {
        /// The path as given.
        path: String,
        /// Why it cannot be included.
        reason: String,
    },
}

/// Describes an exit status for an error message.
fn status_text(status: Option<i32>) -> String {
    status.map_or_else(
        || "killed by a signal".to_owned(),
        |code| format!("exit {code}"),
    )
}
