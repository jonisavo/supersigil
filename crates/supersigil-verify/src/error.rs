use thiserror::Error;

/// Errors that can occur during the verification pipeline.
#[derive(Debug, Error)]
pub enum VerifyError {
    /// A git operation failed.
    #[error(transparent)]
    Git(#[from] crate::git::GitError),
    /// An I/O operation failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// A regex failed to compile.
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),
}
