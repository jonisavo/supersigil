//! CLI error types.

/// Top-level CLI error type.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Record store error.
    #[error("{0}")]
    Record(#[from] supersigil_record::StoreError),
    /// Session sync error.
    #[error("{0}")]
    Session(#[from] supersigil_session::SyncError),
    /// I/O error.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Generic command failure with a message.
    #[error("{0}")]
    CommandFailed(String),
}
