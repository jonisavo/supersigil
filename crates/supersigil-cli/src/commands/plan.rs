use std::io::{self, Write};
use std::path::Path;

use supersigil_core::PlanQuery;

use crate::commands::PlanArgs;
use crate::error::CliError;
use crate::format::{
    self, ColorConfig, OutputFormat, Token, write_completed_summary, write_json, write_tasks,
};
use crate::loader;

/// Run the `plan` command: show outstanding work for a document, prefix,
/// or the whole project.
///
/// # Errors
///
/// Returns `CliError::Query` if the ID/prefix matches no documents,
/// `CliError::Parse` or `CliError::Graph` if loading fails, or
/// `CliError::Io` if writing output fails.
pub fn run(args: &PlanArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;

    let query = match PlanQuery::parse(args.id_or_prefix.as_deref(), &graph) {
        Ok(q) => q,
        Err(e) => {
            format::hint(color, "Run `supersigil ls` to see available document IDs.");
            return Err(e.into());
        }
    };
    let plan = graph.plan(&query)?;

    match args.format {
        OutputFormat::Json => write_json(&plan)?,
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();

            let c = color;
            if !plan.outstanding_criteria.is_empty() {
                writeln!(
                    out,
                    "{}",
                    c.paint(Token::Header, "## Outstanding criteria:"),
                )?;
                for crit in &plan.outstanding_criteria {
                    let body = crit.body_text.as_deref().unwrap_or("(no description)");
                    let ref_str = format!("{}#{}", crit.doc_id, crit.criterion_id);
                    writeln!(out, "- {}: {body}", c.paint(Token::DocId, &ref_str))?;
                }
            }

            if !plan.pending_tasks.is_empty() {
                writeln!(
                    out,
                    "\n{}",
                    c.paint(Token::Header, "## Pending tasks (in dependency order):"),
                )?;
                write_tasks(&mut out, &plan.pending_tasks, color)?;
            }

            if !plan.illustrated_by.is_empty() {
                writeln!(out, "\n{}", c.paint(Token::Header, "## Illustrated by:"))?;
                for illus in &plan.illustrated_by {
                    let frag = illus
                        .target_fragment
                        .as_ref()
                        .map_or(String::new(), |f| format!("#{f}"));
                    writeln!(
                        out,
                        "- {} (illustrates {}{})",
                        c.paint(Token::DocId, &illus.doc_id),
                        c.paint(Token::DocId, &illus.target_doc_id),
                        frag,
                    )?;
                }
            }

            if !plan.completed_tasks.is_empty() {
                writeln!(out)?;
                write_completed_summary(&mut out, &plan.completed_tasks, color)?;
            }

            if plan.outstanding_criteria.is_empty()
                && plan.pending_tasks.is_empty()
                && plan.completed_tasks.is_empty()
            {
                writeln!(out, "No outstanding work.")?;
            }
        }
    }

    Ok(())
}
