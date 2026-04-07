use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;
use supersigil_verify::document_components::{
    BuildComponentsInput, DocumentComponentsResult, build_document_components,
};

use crate::commands::{RenderArgs, RenderFormat};
use crate::error::CliError;
use crate::format::{self, ColorConfig, Token};
use crate::loader;
use crate::plugins;

#[derive(Serialize)]
struct RenderOutput {
    documents: Vec<DocumentComponentsResult>,
}

/// Run the `render` command: output component trees with verification data.
///
/// Iterates all documents in the graph, builds fence-grouped component trees
/// with verification status, and outputs a JSON object containing
/// `DocumentComponentsResult` entries.
///
/// # Errors
///
/// Returns `CliError` if loading fails or an I/O error occurs.
pub fn run(args: &RenderArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);

    // Build evidence for verification status enrichment.
    let inputs = supersigil_verify::VerifyInputs::resolve(&config, project_root);
    let (artifact_graph, plugin_findings) =
        plugins::build_evidence(&config, &graph, project_root, None, &inputs);

    if !plugin_findings.is_empty() {
        plugins::warn_plugin_findings(&plugin_findings, color);
    }

    let evidence_index = &artifact_graph.evidence_by_target;

    // Build component trees for all documents.
    let mut results: Vec<DocumentComponentsResult> = Vec::new();

    for (_doc_id, doc) in graph.documents() {
        // Read the file content from disk.
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
            graph: &graph,
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

    // Sort by document_id for deterministic output.
    results.sort_by(|a, b| a.document_id.cmp(&b.document_id));

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let doc_count = results.len();

    match args.format {
        RenderFormat::Json => {
            let json = serde_json::to_string_pretty(&RenderOutput { documents: results })
                .map_err(|e| CliError::Io(io::Error::other(e)))?;
            writeln!(out, "{json}")?;
        }
    }

    // Summary on stderr so it doesn't pollute piped JSON output.
    eprintln!(
        "{} {} documents rendered",
        color.paint(Token::Header, "Render:"),
        color.paint(Token::Count, &doc_count.to_string()),
    );
    format::hint(
        color,
        "Pipe to a file, e.g. `supersigil render --format json > components.json`.",
    );

    Ok(())
}
