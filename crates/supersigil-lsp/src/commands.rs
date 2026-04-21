//! Custom LSP command handlers.

use serde::{Deserialize, Serialize};

/// The command name for the supersigil verify command.
pub const VERIFY_COMMAND: &str = "supersigil.verify";

/// The command name for the document list command.
///
/// Returns the full document list from the loaded graph. This
/// duplicates the `supersigil/documentList` custom request but is
/// also available via `workspace/executeCommand` for LSP clients
/// that cannot send custom JSON-RPC requests (e.g. `IntelliJ`'s
/// built-in LSP client).
pub const DOCUMENT_LIST_COMMAND: &str = "supersigil.documentList";

/// The command name for the document components command.
///
/// Returns the component tree for a single document with verification
/// status. This duplicates the `supersigil/documentComponents` custom
/// request but is also available via `workspace/executeCommand` for LSP
/// clients that cannot send custom JSON-RPC requests.
pub const DOCUMENT_COMPONENTS_COMMAND: &str = "supersigil.documentComponents";

/// The command name for the explorer snapshot command.
///
/// Returns the lazy graph explorer shell payload. This mirrors the
/// `supersigil/explorerSnapshot` custom request for clients that only support
/// `workspace/executeCommand`.
pub const EXPLORER_SNAPSHOT_COMMAND: &str = "supersigil.explorerSnapshot";

/// The command name for the explorer document command.
///
/// Returns lazy detail-panel payload for one explorer document. This mirrors
/// the `supersigil/explorerDocument` custom request for clients that only
/// support `workspace/executeCommand`.
pub const EXPLORER_DOCUMENT_COMMAND: &str = "supersigil.explorerDocument";

/// The command name for the interactive create-document command.
///
/// Used when the target project is ambiguous (multi-project mode) and the
/// server needs to ask the user which project to place the file in.
pub const CREATE_DOCUMENT_COMMAND: &str = "supersigil.createDocument";

/// Parameters for the interactive create-document command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateDocumentParams {
    /// Feature prefix used to resolve the destination path.
    pub feature: String,
    /// Full document reference to create.
    #[serde(rename = "ref")]
    pub target_ref: String,
    /// Long-form document type used for scaffolding.
    #[serde(rename = "type")]
    pub full_type: String,
}
