use std::io::{self, Write};
use std::path::Path;

use crate::commands::ContextArgs;
use crate::error::CliError;
use crate::format::{OutputFormat, write_json, write_tasks};
use crate::loader;

/// Run the `context` command: show structured view of a document.
///
/// # Errors
///
/// Returns `CliError` if the graph cannot be loaded, the document is not
/// found, or output fails.
pub fn run(args: &ContextArgs, config_path: &Path) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;
    let ctx = graph.context(&args.id)?;

    match args.format {
        OutputFormat::Json => write_json(&ctx)?,
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();

            let doc = &ctx.document;
            let doc_type = doc.frontmatter.doc_type.as_deref().unwrap_or("document");
            let status = doc.frontmatter.status.as_deref().unwrap_or("(no status)");

            writeln!(out, "# {doc_type}: {}", doc.frontmatter.id)?;
            writeln!(out, "Status: {status}")?;

            if !ctx.criteria.is_empty() {
                writeln!(out, "\n## Criteria:")?;
                for crit in &ctx.criteria {
                    let body = crit.body_text.as_deref().unwrap_or("(no description)");
                    writeln!(out, "- {}: {body}", crit.id)?;
                    for vref in &crit.validated_by {
                        let vstatus = vref.status.as_deref().unwrap_or("?");
                        writeln!(out, "  -> Validated by: {} ({vstatus})", vref.doc_id)?;
                    }
                    for illus in &crit.illustrated_by {
                        writeln!(out, "  -> Illustrated by: {illus}")?;
                    }
                }
            }

            if !ctx.implemented_by.is_empty() {
                writeln!(out, "\n## Implemented by:")?;
                for imp in &ctx.implemented_by {
                    let imp_status = imp.status.as_deref().unwrap_or("?");
                    writeln!(out, "- {} ({imp_status})", imp.doc_id)?;
                }
            }

            if !ctx.illustrated_by.is_empty() {
                writeln!(out, "\n## Illustrated by:")?;
                for illus in &ctx.illustrated_by {
                    writeln!(out, "- {illus}")?;
                }
            }

            if !ctx.tasks.is_empty() {
                writeln!(out, "\n## Tasks (in dependency order):")?;
                write_tasks(&mut out, &ctx.tasks)?;
            }
        }
    }

    Ok(())
}
