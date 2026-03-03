//! Hard errors produced during graph construction.

use std::path::PathBuf;

use crate::SourcePosition;

/// Hard errors produced during graph construction.
///
/// All variants are fatal — they indicate structural integrity failures
/// that prevent downstream consumers from operating correctly.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error(
        "duplicate document ID `{id}`: found in {}",
        paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    )]
    DuplicateId { id: String, paths: Vec<PathBuf> },

    #[error(
        "{doc_id}: duplicate component ID `{component_id}` at positions {}",
        positions.iter().map(|p| format!("{}:{}", p.line, p.column)).collect::<Vec<_>>().join(", ")
    )]
    DuplicateComponentId {
        doc_id: String,
        component_id: String,
        positions: Vec<SourcePosition>,
    },

    #[error(
        "{doc_id}:{}:{}: broken ref `{ref_str}`: {reason}",
        position.line,
        position.column
    )]
    BrokenRef {
        doc_id: String,
        ref_str: String,
        reason: String,
        position: SourcePosition,
    },

    #[error(
        "dependency cycle in tasks document `{doc_id}`: {}",
        cycle.join(" → ")
    )]
    TaskDependencyCycle { doc_id: String, cycle: Vec<String> },

    #[error("dependency cycle in document graph: {}", cycle.join(" → "))]
    DocumentDependencyCycle { cycle: Vec<String> },
}
