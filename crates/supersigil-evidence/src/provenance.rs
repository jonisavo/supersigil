//! Provenance and conflict types: `PluginProvenance` and `EvidenceConflict`.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::types::{EvidenceKind, SourceLocation, TestIdentity, VerifiableRef};

// ---------------------------------------------------------------------------
// PluginProvenance
// ---------------------------------------------------------------------------

/// How a piece of evidence was discovered or authored.
///
/// The design keeps explicit authored evidence and plugin evidence comparable
/// without hiding where they came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PluginProvenance {
    /// Evidence from a `verified-by` tag in a spec document.
    VerifiedByTag {
        /// Document that contains the tag.
        doc_id: String,
        /// Tag value from the document.
        tag: String,
    },
    /// Evidence from a file-glob pattern in a spec document.
    VerifiedByFileGlob {
        /// Document that contains the glob pattern.
        doc_id: String,
        /// Resolved file paths matching the glob.
        paths: Vec<String>,
    },
    /// Evidence from a `#[verified_by(...)]` Rust attribute.
    RustAttribute {
        /// Source location of the attribute.
        attribute_span: SourceLocation,
    },
    /// Evidence from a JavaScript/TypeScript `verifies(...)` annotation.
    JsVerifies {
        /// Source location of the annotation.
        annotation_span: SourceLocation,
    },
}

impl PluginProvenance {
    /// Map the provenance variant to its corresponding evidence classification.
    #[must_use]
    pub fn kind(&self) -> EvidenceKind {
        match self {
            Self::VerifiedByTag { .. } => EvidenceKind::Tag,
            Self::VerifiedByFileGlob { .. } => EvidenceKind::FileGlob,
            Self::RustAttribute { .. } => EvidenceKind::RustAttribute,
            Self::JsVerifies { .. } => EvidenceKind::JsVerifies,
        }
    }
}

// ---------------------------------------------------------------------------
// EvidenceConflict
// ---------------------------------------------------------------------------

/// A conflict where two evidence sources disagree on the effective criterion
/// set for the same test.
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceConflict {
    /// The test whose evidence is in conflict.
    pub test: TestIdentity,
    /// Criterion set from the first source.
    pub left: BTreeSet<VerifiableRef>,
    /// Criterion set from the second source.
    pub right: BTreeSet<VerifiableRef>,
    /// Provenance entries from all conflicting sources.
    pub sources: Vec<PluginProvenance>,
}
