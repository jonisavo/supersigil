//! `EcosystemPlugin` trait and plugin discovery error/diagnostic surfaces.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use supersigil_core::DocumentGraph;
use thiserror::Error;

use crate::repository::WorkspaceMetadata;
use crate::types::{ProjectScope, VerificationEvidenceRecord};

// ---------------------------------------------------------------------------
// PluginDiscoveryResult
// ---------------------------------------------------------------------------

/// Structured output from a successful plugin discovery pass.
///
/// Plugins can emit evidence alongside non-fatal diagnostics for recoverable
/// per-file issues. Fatal discovery failures still use `PluginError`.
#[derive(Debug, Default)]
pub struct PluginDiscoveryResult {
    /// Evidence records discovered by the plugin.
    pub evidence: Vec<VerificationEvidenceRecord>,
    /// Non-fatal diagnostics emitted during discovery.
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl PluginDiscoveryResult {
    /// Build a result containing evidence with no diagnostics.
    #[must_use]
    pub fn from_evidence(evidence: Vec<VerificationEvidenceRecord>) -> Self {
        Self {
            evidence,
            diagnostics: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// PluginDiagnostic
// ---------------------------------------------------------------------------

/// Non-fatal plugin diagnostic emitted during a successful discovery pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDiagnostic {
    /// Human-readable diagnostic message.
    pub message: String,
    /// File path associated with the diagnostic, if any.
    pub path: Option<PathBuf>,
}

impl PluginDiagnostic {
    /// Create a warning diagnostic without an associated path.
    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: None,
        }
    }

    /// Create a warning diagnostic tied to a specific file path.
    #[must_use]
    pub fn warning_for_path(path: PathBuf, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: Some(path),
        }
    }
}

// ---------------------------------------------------------------------------
// PluginError
// ---------------------------------------------------------------------------

/// Error surface for ecosystem plugin discovery failures.
///
/// Plugin failures become verification findings rather than hard errors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginErrorDetails {
    /// File path where the error occurred.
    pub path: Option<PathBuf>,
    /// One-based line number of the error.
    pub line: Option<usize>,
    /// One-based column number of the error.
    pub column: Option<usize>,
    /// Machine-readable error code.
    pub code: Option<String>,
    /// Human-readable suggestion for fixing the error.
    pub suggestion: Option<String>,
}

/// Error surface for ecosystem plugin failures.
#[derive(Debug, Error)]
pub enum PluginError {
    /// A source file could not be parsed.
    #[error("plugin {plugin}: failed to parse {file}: {message}")]
    ParseFailure {
        /// Name of the plugin that failed.
        plugin: String,
        /// File that failed to parse.
        file: PathBuf,
        /// Human-readable parse error message.
        message: String,
    },
    /// A general discovery-phase failure.
    #[error("plugin {plugin}: {message}")]
    Discovery {
        /// Name of the plugin that failed.
        plugin: String,
        /// Human-readable error message.
        message: String,
        /// Optional structured error details.
        details: Option<Box<PluginErrorDetails>>,
    },
    /// An I/O error during plugin execution.
    #[error("plugin {plugin}: I/O error on {path}: {source}")]
    Io {
        /// Name of the plugin that failed.
        plugin: String,
        /// Path that triggered the I/O error.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

// ---------------------------------------------------------------------------
// EcosystemPlugin
// ---------------------------------------------------------------------------

/// Extension contract for built-in and future ecosystem integrations.
///
/// Implementations return normalized evidence records plus any non-fatal
/// diagnostics. Fatal plugin failures are reported through `PluginError`,
/// which `supersigil-verify` turns into verification findings.
pub trait EcosystemPlugin {
    /// Human-readable plugin name (e.g. `"rust"`).
    fn name(&self) -> &'static str;

    /// Plan plugin-specific discovery inputs from shared test files and project scope.
    ///
    /// The default implementation preserves the shared discovery inputs
    /// unchanged. Plugins can override this hook to add or replace files based
    /// on the project context before `discover` runs.
    #[must_use]
    fn plan_discovery_inputs<'a>(
        &self,
        test_files: &'a [PathBuf],
        _scope: &ProjectScope,
    ) -> Cow<'a, [PathBuf]> {
        Cow::Borrowed(test_files)
    }

    /// Extract workspace-level metadata from the ecosystem manifest at the given root.
    ///
    /// The default implementation returns empty metadata. Plugins that can
    /// read repository URLs or other workspace-wide information from their
    /// manifest files should override this method.
    ///
    /// # Errors
    ///
    /// Returns `PluginError` if the manifest file exists but cannot be parsed.
    fn workspace_metadata(&self, workspace_root: &Path) -> Result<WorkspaceMetadata, PluginError> {
        let _ = workspace_root;
        Ok(WorkspaceMetadata { repository: None })
    }

    /// Discover evidence records from the given source files.
    ///
    /// # Errors
    ///
    /// Returns `PluginError` if the plugin encounters a fatal discovery
    /// failure. Recoverable diagnostics should be returned in the successful
    /// `PluginDiscoveryResult`.
    fn discover(
        &self,
        files: &[PathBuf],
        scope: &ProjectScope,
        documents: &DocumentGraph,
    ) -> Result<PluginDiscoveryResult, PluginError>;
}
