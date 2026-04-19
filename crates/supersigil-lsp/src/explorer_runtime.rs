//! LSP request and notification wrappers for the explorer runtime contract.

use lsp_types::{notification::Notification, request::Request};

pub use supersigil_verify::explorer_runtime::{
    BuildExplorerDocumentInput, BuildExplorerSnapshotInput, CoverageSummary, ExplorerChangedEvent,
    ExplorerDocument, ExplorerDocumentFingerprint, ExplorerDocumentParams, ExplorerDocumentSummary,
    ExplorerEdge, ExplorerGraphComponent, ExplorerSnapshot, build_explorer_document,
    build_explorer_snapshot, diff_explorer_documents, diff_explorer_snapshots,
    fingerprint_document_components,
};

/// Request type for `supersigil/explorerSnapshot`.
#[derive(Debug)]
pub struct ExplorerSnapshotRequest;

impl Request for ExplorerSnapshotRequest {
    type Params = serde_json::Value;
    type Result = ExplorerSnapshot;
    const METHOD: &'static str = "supersigil/explorerSnapshot";
}

/// Request type for `supersigil/explorerDocument`.
#[derive(Debug)]
pub struct ExplorerDocumentRequest;

impl Request for ExplorerDocumentRequest {
    type Params = ExplorerDocumentParams;
    type Result = ExplorerDocument;
    const METHOD: &'static str = "supersigil/explorerDocument";
}

/// Notification type for `supersigil/explorerChanged`.
#[derive(Debug)]
pub struct ExplorerChangedNotification;

impl Notification for ExplorerChangedNotification {
    type Params = ExplorerChangedEvent;
    const METHOD: &'static str = "supersigil/explorerChanged";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_methods_match_spec() {
        assert_eq!(
            ExplorerSnapshotRequest::METHOD,
            "supersigil/explorerSnapshot"
        );
        assert_eq!(
            ExplorerDocumentRequest::METHOD,
            "supersigil/explorerDocument"
        );
        assert_eq!(
            ExplorerChangedNotification::METHOD,
            "supersigil/explorerChanged"
        );
    }
}
