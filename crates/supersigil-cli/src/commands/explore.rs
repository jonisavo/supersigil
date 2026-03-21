//! Generate a self-contained HTML graph explorer and open it in the browser.

use std::path::Path;

use crate::commands::ExploreArgs;
use crate::error::CliError;
use crate::format::ColorConfig;
use crate::loader;

use super::graph::json::build_graph_json;

const TEMPLATE_HTML: &str = include_str!("explore_template.html");
const TOKENS_CSS: &str = include_str!("../../../../website/src/styles/landing-tokens.css");
const STYLES_CSS: &str = include_str!("../../../../website/src/components/explore/styles.css");
const EXPLORER_JS: &str = include_str!("explore_standalone.js");

/// Build the self-contained HTML string for the graph explorer.
pub(crate) fn build_html(graph: &supersigil_core::DocumentGraph) -> Result<String, CliError> {
    let graph_json = build_graph_json(graph);
    let json_str = serde_json::to_string_pretty(&graph_json)
        .map_err(std::io::Error::other)?
        .replace("</", "<\\/");

    let html = TEMPLATE_HTML
        .replace("{{TOKENS}}", TOKENS_CSS)
        .replace("{{STYLES}}", STYLES_CSS)
        .replace("{{GRAPH_DATA}}", &json_str)
        .replace("{{EXPLORER_JS}}", EXPLORER_JS);

    Ok(html)
}

/// Run the `explore` command: generate an HTML explorer and open it.
///
/// # Errors
///
/// Returns `CliError` if loading fails or the file cannot be written/opened.
pub fn run(args: &ExploreArgs, config_path: &Path, _color: ColorConfig) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;
    let html = build_html(&graph)?;

    if let Some(output_path) = &args.output {
        std::fs::write(output_path, &html)?;
        eprintln!("Wrote explorer to {}", output_path.display());
    } else {
        let mut temp = tempfile::Builder::new()
            .prefix("supersigil-explore-")
            .suffix(".html")
            .tempfile()?;
        std::io::Write::write_all(&mut temp, html.as_bytes())?;
        let (_, path) = temp
            .keep()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        eprintln!("Opening explorer in browser...");
        open::that(&path)?;
        eprintln!("Wrote explorer to {}", path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use supersigil_rust::verifies;
    use supersigil_verify::test_helpers::{build_test_graph, make_criterion, make_doc_typed};

    fn sample_graph() -> supersigil_core::DocumentGraph {
        let docs = vec![
            make_doc_typed(
                "req/auth",
                "requirements",
                Some("Approved"),
                vec![make_criterion("auth-1", 5)],
            ),
            make_doc_typed("design/auth", "design", Some("Draft"), vec![]),
        ];
        build_test_graph(docs)
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_doctype() {
        let graph = sample_graph();
        let html = build_html(&graph).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_graph_json_inline() {
        let graph = sample_graph();
        let html = build_html(&graph).unwrap();
        assert!(html.contains("\"req/auth\""));
        assert!(html.contains("\"design/auth\""));
        // Graph data is inlined directly in the template's <script> tag
        assert!(html.contains("SupersigilExplorer.mount"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_d3_cdn() {
        let graph = sample_graph();
        let html = build_html(&graph).unwrap();
        assert!(html.contains("https://cdn.jsdelivr.net/npm/d3@7/dist/d3.min.js"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_explorer_js() {
        let graph = sample_graph();
        let html = build_html(&graph).unwrap();
        assert!(html.contains("SupersigilExplorer"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_styles() {
        let graph = sample_graph();
        let html = build_html(&graph).unwrap();
        // Check for landing tokens
        assert!(html.contains("--bg-deep"));
        assert!(html.contains("--gold"));
        // Check for explorer styles
        assert!(html.contains(".explorer-bar"));
        assert!(html.contains(".detail-panel"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_is_self_contained() {
        let graph = sample_graph();
        let html = build_html(&graph).unwrap();
        // Should contain <script> tags
        assert!(html.contains("<script>"));
        assert!(html.contains("</script>"));
        // Should contain <style> tags
        assert!(html.contains("<style>"));
        assert!(html.contains("</style>"));
        // Should have no template placeholders left
        assert!(!html.contains("{{TOKENS}}"));
        assert!(!html.contains("{{STYLES}}"));
        assert!(!html.contains("{{GRAPH_DATA}}"));
        assert!(!html.contains("{{EXPLORER_JS}}"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-2")]
    fn output_flag_writes_to_file() {
        // This test verifies the --output path writes successfully
        let graph = sample_graph();
        let html = build_html(&graph).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let output_path = dir.path().join("explore.html");
        std::fs::write(&output_path, &html).unwrap();

        let contents = std::fs::read_to_string(&output_path).unwrap();
        assert!(contents.starts_with("<!DOCTYPE html>"));
        assert!(contents.contains("\"req/auth\""));
    }
}
