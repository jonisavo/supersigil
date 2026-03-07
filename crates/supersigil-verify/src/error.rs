use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerifyError {
    #[error(transparent)]
    Git(#[from] crate::git::GitError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),
}
