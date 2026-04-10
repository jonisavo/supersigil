//! Core evidence types: `VerifiableRef`, `TestIdentity`, `VerificationEvidenceRecord`,
//! `VerificationTargets`, `ProjectScope`, `SourceLocation`, `EvidenceId`,
//! `EvidenceKind`, and `TestKind`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use serde::Serialize;

use crate::provenance::PluginProvenance;

// ---------------------------------------------------------------------------
// EvidenceId
// ---------------------------------------------------------------------------

/// Opaque identifier for a single evidence record within an `ArtifactGraph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct EvidenceId(usize);

impl EvidenceId {
    /// Create a new evidence identifier.
    #[must_use]
    pub fn new(id: usize) -> Self {
        Self(id)
    }
}

// ---------------------------------------------------------------------------
// SourceLocation
// ---------------------------------------------------------------------------

/// File-relative source location for evidence provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLocation {
    /// File path relative to the project root.
    pub file: PathBuf,
    /// One-based line number.
    pub line: usize,
    /// One-based column number.
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
    /// Document identifier (e.g. `"req/auth"`).
    pub doc_id: String,
    /// Criterion identifier within the document (e.g. `"crit-1"`).
    pub target_id: String,
}

impl VerifiableRef {
    /// Parse a verifiable reference string like `"req/auth#crit-1"`.
    ///
    /// Returns `None` unless the string matches the `document-id#criterion-id`
    /// form with non-empty fragments and no extra `#` characters.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let (doc_id, target_id) = supersigil_core::split_criterion_ref(s)?;
        Some(Self {
            doc_id: doc_id.to_string(),
            target_id: target_id.to_string(),
        })
    }
}

impl fmt::Display for VerifiableRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.doc_id, self.target_id)
    }
}

// ---------------------------------------------------------------------------
// VerificationTargets
// ---------------------------------------------------------------------------

/// Non-empty set of criterion targets backed by a single evidence record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct VerificationTargets(BTreeSet<VerifiableRef>);

impl VerificationTargets {
    /// Construct a non-empty target set.
    #[must_use]
    pub fn new(targets: BTreeSet<VerifiableRef>) -> Option<Self> {
        if targets.is_empty() {
            return None;
        }
        Some(Self(targets))
    }

    /// Construct a target set containing exactly one criterion ref.
    #[must_use]
    pub fn single(target: VerifiableRef) -> Self {
        Self(BTreeSet::from([target]))
    }

    /// Borrow the underlying target set.
    #[must_use]
    pub fn as_set(&self) -> &BTreeSet<VerifiableRef> {
        &self.0
    }

    /// Iterate over the targeted criteria.
    pub fn iter(&self) -> std::collections::btree_set::Iter<'_, VerifiableRef> {
        self.0.iter()
    }

    /// Number of criterion targets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Target sets are guaranteed to be non-empty by construction.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Consume the wrapper and return the underlying set.
    #[must_use]
    pub fn into_set(self) -> BTreeSet<VerifiableRef> {
        self.0
    }
}

impl<'a> IntoIterator for &'a VerificationTargets {
    type Item = &'a VerifiableRef;
    type IntoIter = std::collections::btree_set::Iter<'a, VerifiableRef>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl IntoIterator for VerificationTargets {
    type Item = VerifiableRef;
    type IntoIter = std::collections::btree_set::IntoIter<VerifiableRef>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl PartialEq<BTreeSet<VerifiableRef>> for VerificationTargets {
    fn eq(&self, other: &BTreeSet<VerifiableRef>) -> bool {
        self.0 == *other
    }
}

impl PartialEq<VerificationTargets> for BTreeSet<VerifiableRef> {
    fn eq(&self, other: &VerificationTargets) -> bool {
        *self == other.0
    }
}

// ---------------------------------------------------------------------------
// TestKind
// ---------------------------------------------------------------------------

/// Classification of the test that produced evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum TestKind {
    /// Synchronous unit test.
    Unit,
    /// Asynchronous (tokio/async-std) test.
    Async,
    /// Property-based (e.g. proptest) test.
    Property,
    /// Snapshot test.
    Snapshot,
    /// Test kind could not be determined.
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
    /// Path to the test file relative to the project root.
    pub file: PathBuf,
    /// Fully qualified test function name.
    pub name: String,
    /// Classification of the test.
    pub kind: TestKind,
}

// ---------------------------------------------------------------------------
// EvidenceKind
// ---------------------------------------------------------------------------

/// How the evidence was originally authored or discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum EvidenceKind {
    /// Evidence from a `verified-by` tag in a spec document.
    Tag,
    /// Evidence from a file-glob pattern match.
    FileGlob,
    /// Evidence from a `#[verified_by(...)]` Rust attribute.
    RustAttribute,
    /// Evidence from a JavaScript/TypeScript `verifies(...)` annotation.
    JsVerifies,
}

impl EvidenceKind {
    /// Stable string representation for serialization and display.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FileGlob => "file-glob",
            Self::Tag => "tag",
            Self::RustAttribute => "rust-attribute",
            Self::JsVerifies => "js-verifies",
        }
    }
}

// ---------------------------------------------------------------------------
// VerificationEvidenceRecord
// ---------------------------------------------------------------------------

/// A single normalized evidence record linking a test to one or more criterion targets.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationEvidenceRecord {
    /// Unique identifier within the artifact graph.
    pub id: EvidenceId,
    /// Criterion targets this evidence covers.
    pub targets: VerificationTargets,
    /// Identity of the test that produced this evidence.
    pub test: TestIdentity,
    /// Source location where the evidence annotation appears.
    pub source_location: SourceLocation,
    /// Chain of provenance entries explaining how this evidence was discovered.
    pub provenance: Vec<PluginProvenance>,
    /// Arbitrary key-value metadata attached by the plugin.
    pub metadata: BTreeMap<String, String>,
}

impl VerificationEvidenceRecord {
    /// Derive the evidence kind from the first provenance entry.
    #[must_use]
    pub fn kind(&self) -> Option<EvidenceKind> {
        self.provenance.first().map(PluginProvenance::kind)
    }
}

// ---------------------------------------------------------------------------
// ProjectScope
// ---------------------------------------------------------------------------

/// Language-agnostic description of the supersigil project context used for
/// evidence discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectScope {
    /// Optional project name from the supersigil configuration.
    pub project: Option<String>,
    /// Filesystem root of the project being verified.
    pub project_root: PathBuf,
}
