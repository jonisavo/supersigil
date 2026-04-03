//! Custom LSP command handlers.

use supersigil_core::DiagnosticsTier;

use crate::parse_tier;

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

/// The command name for the interactive create-document command.
///
/// Used when the target project is ambiguous (multi-project mode) and the
/// server needs to ask the user which project to place the file in.
pub const CREATE_DOCUMENT_COMMAND: &str = "supersigil.createDocument";

/// Parse an optional tier argument from command arguments.
///
/// Expected: `["lint"]`, `["verify"]`, or `[]` (use default).
///
/// Returns `None` if no arguments are provided or if the argument is not a
/// recognised tier name.
#[must_use]
pub fn parse_verify_tier(arguments: &[serde_json::Value]) -> Option<DiagnosticsTier> {
    arguments
        .first()
        .and_then(|v| v.as_str())
        .and_then(parse_tier)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_verify_tier_lint() {
        assert_eq!(
            parse_verify_tier(&[json!("lint")]),
            Some(DiagnosticsTier::Lint),
        );
    }

    #[test]
    fn parse_verify_tier_verify() {
        assert_eq!(
            parse_verify_tier(&[json!("verify")]),
            Some(DiagnosticsTier::Verify),
        );
    }

    #[test]
    fn parse_verify_tier_full_is_now_unknown() {
        assert_eq!(parse_verify_tier(&[json!("full")]), None);
    }

    #[test]
    fn parse_verify_tier_empty_returns_none() {
        assert_eq!(parse_verify_tier(&[]), None);
    }

    #[test]
    fn parse_verify_tier_invalid_returns_none() {
        assert_eq!(parse_verify_tier(&[json!("invalid")]), None);
    }
}
