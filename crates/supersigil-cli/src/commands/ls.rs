use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use crate::commands::LsArgs;
use crate::error::CliError;
use crate::format::{OutputFormat, write_json};
use crate::loader;

#[derive(Serialize)]
struct DocEntry {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    doc_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    path: String,
}

/// Run the `ls` command: list documents with optional filters.
///
/// # Errors
///
/// Returns `CliError` if the graph cannot be loaded or output fails.
pub fn run(args: &LsArgs, config_path: &Path) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;

    let mut entries: Vec<DocEntry> = graph
        .documents()
        .filter(|(_, doc)| {
            if let Some(ref t) = args.doc_type
                && doc.frontmatter.doc_type.as_deref() != Some(t.as_str())
            {
                return false;
            }
            if let Some(ref s) = args.status
                && doc.frontmatter.status.as_deref() != Some(s.as_str())
            {
                return false;
            }
            if let Some(ref p) = args.project
                && graph.doc_project(&doc.frontmatter.id) != Some(p.as_str())
            {
                return false;
            }
            true
        })
        .map(|(_, doc)| DocEntry {
            id: doc.frontmatter.id.clone(),
            doc_type: doc.frontmatter.doc_type.clone(),
            status: doc.frontmatter.status.clone(),
            path: doc.path.display().to_string(),
        })
        .collect();

    // Sort for stable output
    entries.sort_by(|a, b| a.id.cmp(&b.id));

    match args.format {
        OutputFormat::Json => write_json(&entries)?,
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for entry in &entries {
                let doc_type = entry.doc_type.as_deref().unwrap_or("-");
                let status = entry.status.as_deref().unwrap_or("-");
                writeln!(
                    out,
                    "{}  {}  {}  {}",
                    entry.id, doc_type, status, entry.path
                )?;
            }
        }
    }

    Ok(())
}
