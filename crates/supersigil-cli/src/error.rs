use std::path::PathBuf;

/// Top-level CLI error type.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(
        "config file not found (searched upward from {start_dir}). Run `supersigil init` to create one."
    )]
    ConfigNotFound { start_dir: PathBuf },
    #[error("config errors:\n{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Config(Vec<supersigil_core::ConfigError>),
    #[error("parse errors:\n{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Parse(Vec<supersigil_core::ParseError>),
    #[error("graph errors:\n{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Graph(Vec<supersigil_core::GraphError>),
    #[error("{0}")]
    Query(#[from] supersigil_core::QueryError),
    #[error("{0}")]
    Import(#[from] supersigil_import::ImportError),
    #[error("{0}")]
    Verify(#[from] supersigil_verify::VerifyError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("lint errors found")]
    LintFailed,
    #[error("{0}")]
    CommandFailed(String),
}
