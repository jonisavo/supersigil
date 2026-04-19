//! Contract tests for the explorer runtime LSP surface.

use lsp_types::{notification::Notification, request::Request};

use supersigil_lsp::commands::{EXPLORER_DOCUMENT_COMMAND, EXPLORER_SNAPSHOT_COMMAND};
use supersigil_lsp::explorer_runtime::{
    ExplorerChangedNotification, ExplorerDocumentRequest, ExplorerSnapshotRequest,
};
use supersigil_rust::verifies;

#[test]
#[verifies("graph-explorer-runtime/req#req-1-1")]
fn explorer_runtime_request_methods_and_commands_match_spec() {
    assert_eq!(
        ExplorerSnapshotRequest::METHOD,
        "supersigil/explorerSnapshot"
    );
    assert_eq!(
        ExplorerDocumentRequest::METHOD,
        "supersigil/explorerDocument"
    );
    assert_eq!(EXPLORER_SNAPSHOT_COMMAND, "supersigil.explorerSnapshot");
    assert_eq!(EXPLORER_DOCUMENT_COMMAND, "supersigil.explorerDocument");
}

#[test]
#[verifies("graph-explorer-runtime/req#req-1-4")]
fn explorer_changed_notification_method_matches_spec() {
    assert_eq!(
        ExplorerChangedNotification::METHOD,
        "supersigil/explorerChanged"
    );
}
