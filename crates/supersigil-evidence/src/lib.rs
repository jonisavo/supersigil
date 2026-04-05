//! Shared, language-agnostic normalized evidence types for supersigil ecosystem plugins.
//!
//! This crate provides the evidence model consumed by `supersigil-verify` and
//! implemented by ecosystem plugins such as `supersigil-rust`. It does not
//! contain any ecosystem-specific parsing or discovery logic.

mod plugin;
mod provenance;
mod repository;
mod types;

#[cfg(test)]
mod tests;

pub use plugin::{
    EcosystemPlugin, PluginDiagnostic, PluginDiscoveryResult, PluginError, PluginErrorDetails,
};
pub use provenance::{EvidenceConflict, PluginProvenance};
pub use repository::{RepositoryInfo, WorkspaceMetadata, parse_repository_url};
// Re-export from supersigil-core so downstream crates can use a single import path.
pub use supersigil_core::RepositoryProvider;
pub use types::{
    EvidenceId, EvidenceKind, ProjectScope, SourceLocation, TestIdentity, TestKind, VerifiableRef,
    VerificationEvidenceRecord, VerificationTargets,
};
