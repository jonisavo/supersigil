//! Re-exports document component types from supersigil-verify and adds the LSP request wrapper.

pub use supersigil_verify::document_components::*;

use lsp_types::request::Request;

/// LSP request type for per-document component trees.
#[derive(Debug)]
pub struct DocumentComponentsRequest;

impl Request for DocumentComponentsRequest {
    type Params = DocumentComponentsParams;
    type Result = DocumentComponentsResult;
    const METHOD: &'static str = "supersigil/documentComponents";
}
