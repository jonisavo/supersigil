//! CLI error types.

/// Top-level CLI error type. Messages are not escaped; see
/// [`Untrusted`](supersigil_review::printed::Untrusted).
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Record store error.
    #[error("{0}")]
    Record(#[from] supersigil_record::StoreError),
    /// Session sync error.
    #[error("{0}")]
    Session(#[from] supersigil_session::SyncError),
    /// Git error: git missing or too old, not a worktree, a revision that
    /// does not resolve, or a git command that failed.
    #[error("{0}")]
    Git(#[from] supersigil_git::GitError),
    /// I/O error.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Generic command failure with a message.
    #[error("{0}")]
    CommandFailed(String),
    /// A refused call's printable lines, with untrusted text already escaped:
    /// a `judge` call or an `annotate import`. `judge` write command refusals
    /// start with "No judgment recorded.".
    #[error("{0}")]
    Refused(String),
}
