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
    VerifiedByTag { doc_id: String, tag: String },
    VerifiedByFileGlob { doc_id: String, paths: Vec<String> },
    RustAttribute { attribute_span: SourceLocation },
    JsVerifies { annotation_span: SourceLocation },
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
    pub test: TestIdentity,
    pub left: BTreeSet<VerifiableRef>,
    pub right: BTreeSet<VerifiableRef>,
    pub sources: Vec<PluginProvenance>,
}
