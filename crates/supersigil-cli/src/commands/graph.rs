pub(crate) mod json;

use std::io::{self, Write};
use std::path::Path;

use crate::commands::{GraphArgs, GraphFormat};
use crate::error::CliError;
use crate::format::{self, ColorConfig, Token};
use crate::loader;

/// Run the `graph` command: visualize document dependency graph.
///
/// # Errors
///
/// Returns `CliError` if loading fails.
pub fn run(args: &GraphArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let node_count = graph.documents().count();

    let edge_count = match args.format {
        GraphFormat::Mermaid => write_mermaid(&mut out, &graph)?,
        GraphFormat::Dot => write_dot(&mut out, &graph)?,
        GraphFormat::Json => write_json(&mut out, &graph, project_root)?,
    };

    // Summary on stderr so it doesn't pollute piped graph output.
    eprintln!(
        "{} {} nodes, {} edges",
        color.paint(Token::Header, "Graph:"),
        color.paint(Token::Count, &node_count.to_string()),
        color.paint(Token::Count, &edge_count.to_string()),
    );
    let hint_msg = match args.format {
        GraphFormat::Mermaid => "Pipe to a file or tool, e.g. `supersigil graph > deps.mmd`.",
        GraphFormat::Dot => {
            "Pipe to a file or tool, e.g. `supersigil graph --format dot > deps.dot`."
        }
        GraphFormat::Json => {
            "Pipe to a file or tool, e.g. `supersigil graph --format json > graph.json`."
        }
    };
    format::hint(color, hint_msg);

    Ok(())
}

fn node_label(id: &str, doc: &supersigil_core::SpecDocument) -> String {
    doc.frontmatter
        .doc_type
        .as_deref()
        .map_or_else(|| id.to_owned(), |t| format!("{id}\\n({t})"))
}

fn write_mermaid(
    out: &mut impl Write,
    graph: &supersigil_core::DocumentGraph,
) -> io::Result<usize> {
    writeln!(out, "graph TD")?;

    for (id, doc) in graph.documents() {
        let label = node_label(id, doc);
        let safe_id = mermaid_id(id);
        writeln!(out, "    {safe_id}[\"{label}\"]")?;
    }

    let mut edge_count = 0;
    for_each_edge(graph, |from, label, to| {
        edge_count += 1;
        writeln!(
            out,
            "    {} -->|{label}| {}",
            mermaid_id(from),
            mermaid_id(to)
        )
    })?;

    Ok(edge_count)
}

fn write_dot(out: &mut impl Write, graph: &supersigil_core::DocumentGraph) -> io::Result<usize> {
    writeln!(out, "digraph specs {{")?;
    writeln!(out, "    rankdir=TB;")?;

    for (id, doc) in graph.documents() {
        let label = node_label(id, doc);
        writeln!(out, "    \"{id}\" [label=\"{label}\"];")?;
    }

    let mut edge_count = 0;
    for_each_edge(graph, |from, label, to| {
        edge_count += 1;
        writeln!(out, "    \"{from}\" -> \"{to}\" [label=\"{label}\"];")
    })?;

    writeln!(out, "}}")?;
    Ok(edge_count)
}

/// Iterate all edges in the graph, calling `emit(from_id, edge_label, to_doc_id)`
/// for each resolved reference.
fn for_each_edge(
    graph: &supersigil_core::DocumentGraph,
    mut emit: impl FnMut(&str, &str, &str) -> io::Result<()>,
) -> io::Result<()> {
    for (id, doc) in graph.documents() {
        for (idx, comp) in doc.components.iter().enumerate() {
            if comp.attributes.contains_key("refs")
                && let Some(refs) = graph.resolved_refs(id, &[idx])
            {
                for rr in refs {
                    emit(id, &comp.name, &rr.target_doc_id)?;
                }
            }
        }
    }
    Ok(())
}

/// Convert a document ID to a valid Mermaid node identifier.
fn mermaid_id(id: &str) -> String {
    id.replace(['/', '-'], "_")
}

fn write_json(
    out: &mut impl Write,
    graph: &supersigil_core::DocumentGraph,
    project_root: &Path,
) -> io::Result<usize> {
    json::write_json(out, graph, project_root)
}
