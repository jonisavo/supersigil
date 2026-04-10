//! Hard errors produced during graph construction.

use std::path::PathBuf;

use crate::{ComponentDefError, SourcePosition};

/// Hard errors produced during graph construction.
///
/// All variants are fatal — they indicate structural integrity failures
/// that prevent downstream consumers from operating correctly.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// Two or more documents share the same ID.
    #[error(
        "duplicate document ID `{id}`: found in {}",
        paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    )]
    DuplicateId {
        /// The duplicated document ID.
        id: String,
        /// Paths of the files that share this ID.
        paths: Vec<PathBuf>,
    },

    /// Two or more components within the same document share the same ID.
    #[error(
        "{doc_id}: duplicate component ID `{component_id}` at positions {}",
        positions.iter().map(|p| format!("{}:{}", p.line, p.column)).collect::<Vec<_>>().join(", ")
    )]
    DuplicateComponentId {
        /// The document containing the duplicates.
        doc_id: String,
        /// The duplicated component ID.
        component_id: String,
        /// Source positions of each occurrence.
        positions: Vec<SourcePosition>,
    },

    /// A reference could not be resolved to any known target.
    #[error(
        "{doc_id}:{}:{}: broken ref `{ref_str}`: {reason}",
        position.line,
        position.column
    )]
    BrokenRef {
        /// The document containing the broken reference.
        doc_id: String,
        /// The raw reference string that could not be resolved.
        ref_str: String,
        /// Explanation of why the reference is broken.
        reason: String,
        /// Source position of the reference.
        position: SourcePosition,
    },

    /// A cycle was detected among task dependencies within a document.
    #[error(
        "dependency cycle in tasks document `{doc_id}`: {}",
        cycle.join(" → ")
    )]
    TaskDependencyCycle {
        /// The tasks document containing the cycle.
        doc_id: String,
        /// Task IDs forming the cycle.
        cycle: Vec<String>,
    },

    /// A cycle was detected among document-level `DependsOn` edges.
    #[error("dependency cycle in document graph: {}", cycle.join(" → "))]
    DocumentDependencyCycle {
        /// Document IDs forming the cycle.
        cycle: Vec<String>,
    },

    /// A component definition is invalid.
    #[error(transparent)]
    InvalidComponentDef(#[from] ComponentDefError),
}
