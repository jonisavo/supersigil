use std::io::{self, Write};
use std::path::Path;

use supersigil_core::PlanQuery;

use crate::commands::PlanArgs;
use crate::error::CliError;
use crate::format::{OutputFormat, write_json, write_tasks};
use crate::loader;

/// Run the `plan` command: show outstanding work for a document, prefix,
/// or the whole project.
///
/// # Errors
///
/// Returns `CliError::Query` if the ID/prefix matches no documents,
/// `CliError::Parse` or `CliError::Graph` if loading fails, or
/// `CliError::Io` if writing output fails.
pub fn run(args: &PlanArgs, config_path: &Path) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;

    let query = PlanQuery::parse(args.id_or_prefix.as_deref(), &graph)?;
    let plan = graph.plan(&query)?;

    match args.format {
        OutputFormat::Json => write_json(&plan)?,
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();

            if !plan.outstanding_criteria.is_empty() {
                writeln!(out, "## Outstanding criteria:")?;
                for crit in &plan.outstanding_criteria {
                    let body = crit.body_text.as_deref().unwrap_or("(no description)");
                    writeln!(out, "- {}#{}: {body}", crit.doc_id, crit.criterion_id)?;
                }
            }

            if !plan.pending_tasks.is_empty() {
                writeln!(out, "\n## Pending tasks (in dependency order):")?;
                write_tasks(&mut out, &plan.pending_tasks)?;
            }

            if !plan.illustrated_by.is_empty() {
                writeln!(out, "\n## Illustrated by:")?;
                for illus in &plan.illustrated_by {
                    let frag = illus
                        .target_fragment
                        .as_ref()
                        .map_or(String::new(), |f| format!("#{f}"));
                    writeln!(
                        out,
                        "- {} (illustrates {}{})",
                        illus.doc_id, illus.target_doc_id, frag
                    )?;
                }
            }

            if !plan.completed_tasks.is_empty() {
                writeln!(out, "\n## Completed:")?;
                for task in &plan.completed_tasks {
                    writeln!(out, "- {} (done)", task.task_id)?;
                }
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
