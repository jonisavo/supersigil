//! Custom LSP command handlers.

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

/// The command name for the graph data command.
///
/// Returns the full document graph as JSON matching the `GraphJson` schema.
/// This duplicates the `supersigil/graphData` custom request but is also
/// available via `workspace/executeCommand` for LSP clients that cannot send
/// custom JSON-RPC requests.
pub const GRAPH_DATA_COMMAND: &str = "supersigil.graphData";

/// The command name for the interactive create-document command.
///
/// Used when the target project is ambiguous (multi-project mode) and the
/// server needs to ask the user which project to place the file in.
pub const CREATE_DOCUMENT_COMMAND: &str = "supersigil.createDocument";
