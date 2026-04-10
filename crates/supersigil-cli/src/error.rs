use std::path::PathBuf;

/// Top-level CLI error type.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// No `supersigil.toml` found when searching upward from `start_dir`.
    #[error(
        "config file not found (searched upward from {start_dir}). Run `supersigil init` to create one."
    )]
    ConfigNotFound {
        /// Directory where the upward search started.
        start_dir: PathBuf,
    },
    /// One or more config validation errors.
    #[error("config errors:\n{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Config(Vec<supersigil_core::ConfigError>),
    /// One or more spec file parse errors.
    #[error("parse errors:\n{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Parse(Vec<supersigil_core::ParseError>),
    /// One or more document graph construction errors.
    #[error("graph errors:\n{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Graph(Vec<supersigil_core::GraphError>),
    /// One or more component definition errors.
    #[error("component definition errors:\n{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    ComponentDef(Vec<supersigil_core::ComponentDefError>),
    /// Graph query error.
    #[error("{0}")]
    Query(#[from] supersigil_core::QueryError),
    /// Import error from an external format.
    #[error("{0}")]
    Import(#[from] supersigil_import::ImportError),
    /// Verification engine error.
    #[error("{0}")]
    Verify(#[from] supersigil_verify::VerifyError),
    /// I/O error.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Generic command failure with a message.
    #[error("{0}")]
    CommandFailed(String),
}
