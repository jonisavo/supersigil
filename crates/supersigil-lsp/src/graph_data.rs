//! Custom LSP request type for the graph data endpoint.
//!
//! - `supersigil/graphData`: request returning the full document graph as JSON

use lsp_types::request::Request;

pub use supersigil_verify::graph_json::GraphJson;

/// LSP request type for `supersigil/graphData`.
#[derive(Debug)]
pub struct GraphDataRequest;

impl Request for GraphDataRequest {
    type Params = serde_json::Value;
    type Result = GraphJson;
    const METHOD: &'static str = "supersigil/graphData";
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_method_is_supersigil_graph_data() {
        assert_eq!(GraphDataRequest::METHOD, "supersigil/graphData");
    }
}
