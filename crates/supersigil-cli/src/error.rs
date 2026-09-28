//! CLI error types.

/// Top-level CLI error type.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// I/O error.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Generic command failure with a message.
    #[error("{0}")]
    CommandFailed(String),
}
