//! `EcosystemPlugin` trait and `PluginError` error surface.

use std::path::PathBuf;

use supersigil_core::DocumentGraph;
use thiserror::Error;

use crate::types::{ProjectScope, VerificationEvidenceRecord};

// ---------------------------------------------------------------------------
// PluginError
// ---------------------------------------------------------------------------

/// Error surface for ecosystem plugin discovery failures.
///
/// Plugin failures become verification findings rather than hard errors.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin {plugin}: failed to parse {file}: {message}")]
    ParseFailure {
        plugin: String,
        file: PathBuf,
        message: String,
    },
    #[error("plugin {plugin}: {message}")]
    Discovery { plugin: String, message: String },
    #[error("plugin {plugin}: I/O error on {path}: {source}")]
    Io {
        plugin: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

// ---------------------------------------------------------------------------
// EcosystemPlugin
// ---------------------------------------------------------------------------

/// Extension contract for built-in and future ecosystem integrations.
///
/// Implementations return normalized evidence records. Plugin failures are
/// reported through `PluginError`, which `supersigil-verify` turns into
/// verification findings.
pub trait EcosystemPlugin {
    /// Human-readable plugin name (e.g. `"rust"`).
    fn name(&self) -> &'static str;

    /// Discover evidence records from the given source files.
    ///
    /// # Errors
    ///
    /// Returns `PluginError` if the plugin encounters a fatal discovery failure.
    fn discover(
        &self,
        files: &[PathBuf],
        scope: &ProjectScope,
        documents: &DocumentGraph,
    ) -> Result<Vec<VerificationEvidenceRecord>, PluginError>;
}
