//! CLI error types.

/// Top-level CLI error type.
///
/// Messages may carry paths, session ids, and arguments from outside the
/// tool unescaped; the binary escapes the rendered message once, with
/// [`escape_control`](crate::format::escape_control), before it reaches a
/// terminal.
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
