//! Core evidence types: `VerifiableRef`, `TestIdentity`, `VerificationEvidenceRecord`,
//! `ProjectScope`, `SourceLocation`, `EvidenceId`, `EvidenceKind`, and `TestKind`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;

use crate::provenance::PluginProvenance;

// ---------------------------------------------------------------------------
// EvidenceId
// ---------------------------------------------------------------------------

/// Opaque identifier for a single evidence record within an `ArtifactGraph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct EvidenceId(pub usize);

// ---------------------------------------------------------------------------
// SourceLocation
// ---------------------------------------------------------------------------

/// File-relative source location for evidence provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
}

// ---------------------------------------------------------------------------
// VerifiableRef
// ---------------------------------------------------------------------------

/// A reference to a specific verifiable target in a specific document.
///
/// Used as the normalized target in all evidence records, regardless of
/// the ecosystem that produced the evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct VerifiableRef {
    pub doc_id: String,
    pub target_id: String,
}

impl VerifiableRef {
    /// Parse a verifiable reference string like `"req/auth#crit-1"`.
    ///
    /// Returns `None` if the string does not contain a `#` separator.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let (doc_id, target_id) = s.split_once('#')?;
        Some(Self {
            doc_id: doc_id.to_string(),
            target_id: target_id.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// TestKind
// ---------------------------------------------------------------------------

/// Classification of the test that produced evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum TestKind {
    Unit,
    Async,
    Property,
    Snapshot,
    Unknown,
}

impl TestKind {
    /// Stable string representation for serialization and display.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Async => "async",
            Self::Property => "property",
            Self::Snapshot => "snapshot",
            Self::Unknown => "unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// TestIdentity
// ---------------------------------------------------------------------------

/// Identity of a single test function or test file that provides evidence.
///
/// This is the deduplication key for same-test evidence merging: two sources
/// refer to the "same test" only when they normalize to the same file and
/// test name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TestIdentity {
    pub file: PathBuf,
    pub name: String,
    pub kind: TestKind,
}

// ---------------------------------------------------------------------------
// EvidenceKind
// ---------------------------------------------------------------------------

/// How the evidence was originally authored or discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum EvidenceKind {
    Tag,
    FileGlob,
    RustAttribute,
}

impl EvidenceKind {
    /// Stable string representation for serialization and display.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FileGlob => "file-glob",
            Self::Tag => "tag",
            Self::RustAttribute => "rust-attribute",
        }
    }
}

// ---------------------------------------------------------------------------
// VerificationEvidenceRecord
// ---------------------------------------------------------------------------

/// A single normalized evidence record linking a test to one or more verifiable targets.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationEvidenceRecord {
    pub id: EvidenceId,
    pub targets: BTreeSet<VerifiableRef>,
    pub test: TestIdentity,
    pub source_location: SourceLocation,
    pub evidence_kind: EvidenceKind,
    pub provenance: Vec<PluginProvenance>,
    pub metadata: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// ProjectScope
// ---------------------------------------------------------------------------

/// Language-agnostic description of the supersigil project context used for
/// evidence discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectScope {
    pub project: Option<String>,
    pub project_root: PathBuf,
}
