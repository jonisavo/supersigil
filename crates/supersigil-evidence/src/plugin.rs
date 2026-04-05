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
    pub evidence: Vec<VerificationEvidenceRecord>,
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
    pub message: String,
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
    pub path: Option<PathBuf>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub code: Option<String>,
    pub suggestion: Option<String>,
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin {plugin}: failed to parse {file}: {message}")]
    ParseFailure {
        plugin: String,
        file: PathBuf,
        message: String,
    },
    #[error("plugin {plugin}: {message}")]
    Discovery {
        plugin: String,
        message: String,
        details: Option<Box<PluginErrorDetails>>,
    },
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
