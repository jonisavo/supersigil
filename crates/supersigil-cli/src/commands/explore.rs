//! Generate a self-contained HTML spec explorer and open it in the browser.

use std::io;
use std::path::Path;

use supersigil_core::{Config, DocumentGraph};
use supersigil_evidence::RepositoryInfo;
use supersigil_verify::document_components::{
    BuildComponentsInput, DocumentComponentsResult, build_document_components,
};

use crate::commands::ExploreArgs;
use crate::error::CliError;
use crate::format::ColorConfig;
use crate::loader;
use crate::plugins;

use super::graph::json::build_graph_json;

const TEMPLATE_HTML: &str = include_str!("explore_template.html");
const TOKENS_CSS: &str = include_str!("explore_assets/landing-tokens.css");
const STYLES_CSS: &str = include_str!("explore_assets/styles.css");
const EXPLORER_JS: &str = include_str!("explore_assets/explore-standalone.js");
const PREVIEW_CSS: &str = include_str!("explore_assets/supersigil-preview.css");
const PREVIEW_JS: &str = include_str!("explore_assets/supersigil-preview.js");
const RENDER_IIFE_JS: &str = include_str!("explore_assets/render-iife.js");

/// Build render data (component trees with verification) for all documents.
fn build_render_data(
    config: &Config,
    graph: &DocumentGraph,
    project_root: &Path,
) -> Result<Vec<DocumentComponentsResult>, CliError> {
    let inputs = supersigil_verify::VerifyInputs::resolve(config, project_root);
    let (artifact_graph, _plugin_findings) =
        plugins::build_evidence(config, graph, project_root, None, &inputs);

    let evidence_index = &artifact_graph.evidence_by_target;

    let mut results: Vec<DocumentComponentsResult> = Vec::new();

    for (_doc_id, doc) in graph.documents() {
        let file_path = project_root.join(&doc.path);
        let content = std::fs::read_to_string(&file_path).map_err(|e| {
            CliError::Io(io::Error::new(
                e.kind(),
                format!("failed to read {}: {e}", file_path.display()),
            ))
        })?;

        let result = build_document_components(&BuildComponentsInput {
            doc,
            stale: false,
            content: &content,
            graph,
            evidence_by_target: if evidence_index.is_empty() {
                None
            } else {
                Some(evidence_index)
            },
            evidence_records: if artifact_graph.evidence.is_empty() {
                None
            } else {
                Some(&artifact_graph.evidence)
            },
            project_root,
        });

        results.push(result);
    }

    results.sort_by(|a, b| a.document_id.cmp(&b.document_id));
    Ok(results)
}

/// Build the self-contained HTML string for the spec explorer.
pub(crate) fn build_html(
    graph: &DocumentGraph,
    render_data: &[DocumentComponentsResult],
    repository_info: Option<&RepositoryInfo>,
) -> Result<String, CliError> {
    let graph_json = build_graph_json(graph);
    let json_str = serde_json::to_string_pretty(&graph_json)
        .map_err(std::io::Error::other)?
        .replace("</", "<\\/");

    let render_json = serde_json::to_string_pretty(render_data)
        .map_err(std::io::Error::other)?
        .replace("</", "<\\/");

    let repo_json = match repository_info {
        Some(info) => serde_json::to_string(info)
            .map_err(std::io::Error::other)?
            .replace("</", "<\\/"),
        None => "null".to_string(),
    };

    let html = TEMPLATE_HTML
        .replace("{{TOKENS}}", TOKENS_CSS)
        .replace("{{STYLES}}", STYLES_CSS)
        .replace("{{PREVIEW_CSS}}", PREVIEW_CSS)
        .replace("{{RENDER_IIFE_JS}}", RENDER_IIFE_JS)
        .replace("{{PREVIEW_JS}}", PREVIEW_JS)
        .replace("{{GRAPH_DATA}}", &json_str)
        .replace("{{RENDER_DATA}}", &render_json)
        .replace("{{REPOSITORY_INFO}}", &repo_json)
        .replace("{{EXPLORER_JS}}", EXPLORER_JS);

    Ok(html)
}

/// Run the `explore` command: generate an HTML explorer and open it.
///
/// # Errors
///
/// Returns `CliError` if loading fails or the file cannot be written/opened.
pub fn run(args: &ExploreArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);
    let render_data = build_render_data(&config, &graph, project_root)?;
    let assembled_plugins = plugins::assemble_plugins(&config);
    let repo_info =
        plugins::resolve_repository_info(&config, &assembled_plugins, project_root, color);
    let html = build_html(&graph, &render_data, repo_info.as_ref())?;

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
        let html = build_html(&graph, &[], None).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_graph_json_inline() {
        let graph = sample_graph();
        let html = build_html(&graph, &[], None).unwrap();
        assert!(html.contains("\"req/auth\""));
        assert!(html.contains("\"design/auth\""));
        // Graph data is inlined directly in the template's <script> tag
        assert!(html.contains("SupersigilExplorer.mount"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_d3_cdn() {
        let graph = sample_graph();
        let html = build_html(&graph, &[], None).unwrap();
        assert!(html.contains("https://cdn.jsdelivr.net/npm/d3@7/dist/d3.min.js"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_explorer_js() {
        let graph = sample_graph();
        let html = build_html(&graph, &[], None).unwrap();
        assert!(html.contains("SupersigilExplorer"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_styles() {
        let graph = sample_graph();
        let html = build_html(&graph, &[], None).unwrap();
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
        let html = build_html(&graph, &[], None).unwrap();
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
        assert!(!html.contains("{{PREVIEW_CSS}}"));
        assert!(!html.contains("{{RENDER_IIFE_JS}}"));
        assert!(!html.contains("{{PREVIEW_JS}}"));
        assert!(!html.contains("{{RENDER_DATA}}"));
        assert!(!html.contains("{{REPOSITORY_INFO}}"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_preview_assets() {
        let graph = sample_graph();
        let html = build_html(&graph, &[], None).unwrap();
        // Preview CSS
        assert!(html.contains("--supersigil-verified"));
        assert!(html.contains(".supersigil-block"));
        // Render IIFE
        assert!(html.contains("__supersigilRender"));
        assert!(html.contains("renderComponentTree"));
        // Preview JS (interactivity)
        assert!(html.contains("supersigil-evidence-toggle"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-1")]
    fn html_contains_render_data() {
        let graph = sample_graph();
        let html = build_html(&graph, &[], None).unwrap();
        // Render data should be inlined as an empty JSON array
        assert!(html.contains("var renderData = []"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-2-2")]
    fn output_flag_writes_to_file() {
        // This test verifies the --output path writes successfully
        let graph = sample_graph();
        let html = build_html(&graph, &[], None).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let output_path = dir.path().join("explore.html");
        std::fs::write(&output_path, &html).unwrap();

        let contents = std::fs::read_to_string(&output_path).unwrap();
        assert!(contents.starts_with("<!DOCTYPE html>"));
        assert!(contents.contains("\"req/auth\""));
    }

    #[test]
    #[verifies("graph-explorer/req#req-11-1")]
    fn html_without_repository_info_contains_null() {
        let graph = sample_graph();
        let html = build_html(&graph, &[], None).unwrap();
        assert!(html.contains("var repositoryInfo = null;"));
    }

    #[test]
    #[verifies("graph-explorer/req#req-11-1")]
    fn html_with_repository_info_contains_json() {
        use supersigil_evidence::{RepositoryInfo, RepositoryProvider};

        let graph = sample_graph();
        let repo = RepositoryInfo {
            provider: RepositoryProvider::GitHub,
            repo: "owner/repo".into(),
            host: "github.com".into(),
            main_branch: "main".into(),
        };
        let html = build_html(&graph, &[], Some(&repo)).unwrap();
        assert!(html.contains("var repositoryInfo = {"));
        assert!(html.contains(r#""provider":"github""#));
        assert!(html.contains(r#""repo":"owner/repo""#));
        assert!(html.contains(r#""host":"github.com""#));
        assert!(html.contains(r#""mainBranch":"main""#));
        // Should not contain the raw placeholder
        assert!(!html.contains("{{REPOSITORY_INFO}}"));
    }
}
