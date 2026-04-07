use std::collections::HashSet;
use std::io::{self, Write};
use std::path::Path;

use supersigil_core::{OutstandingTarget, PlanOutput, PlanQuery, TaskInfo};

use crate::commands::PlanArgs;
use crate::error::CliError;
use crate::format::{
    self, ColorConfig, OutputFormat, Token, write_completed_summary, write_dependency_graph,
    write_json, write_tasks,
};
use crate::loader;
use crate::plugins;

/// Run the `plan` command: show outstanding work for a document, prefix,
/// or the whole project.
///
/// # Errors
///
/// Returns `CliError::Query` if the ID/prefix matches no documents,
/// `CliError::Parse` or `CliError::Graph` if loading fails, or
/// `CliError::Io` if writing output fails.
pub fn run(args: &PlanArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);

    let query = match PlanQuery::parse(args.id_or_prefix.as_deref(), &graph) {
        Ok(q) => q,
        Err(e) => {
            format::hint(color, "Run `supersigil ls` to see available document IDs.");
            return Err(e.into());
        }
    };
    let mut plan = graph.plan(&query)?;

    // Filter outstanding criteria by evidence: criteria with verification
    // evidence in the ArtifactGraph are no longer outstanding.
    let inputs = supersigil_verify::VerifyInputs::resolve(&config, project_root);
    let (artifact_graph, plugin_findings) =
        plugins::build_evidence(&config, &graph, project_root, None, &inputs);
    plugins::warn_plugin_findings(&plugin_findings, color);
    plan.outstanding_targets
        .retain(|c| !artifact_graph.has_evidence(&c.doc_id, &c.target_id));
    let plan = plan;

    match args.format {
        OutputFormat::Json => write_json(&plan)?,
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            write_terminal(&mut out, &plan, args.full, color)?;
        }
    }

    Ok(())
}

fn write_terminal(
    out: &mut impl Write,
    plan: &PlanOutput,
    full: bool,
    color: ColorConfig,
) -> io::Result<()> {
    let c = color;

    // 1. Dependency graph (pending + completed tasks).
    let all_tasks: Vec<&TaskInfo> = plan
        .pending_tasks
        .iter()
        .chain(plan.completed_tasks.iter())
        .collect();
    if !all_tasks.is_empty() {
        writeln!(out, "{}", c.paint(Token::Header, "## Dependency graph:"))?;
        write_dependency_graph(out, &all_tasks, color)?;
    }

    // 2. Criteria section.
    if !plan.outstanding_targets.is_empty() {
        if full {
            writeln!(
                out,
                "\n{} ({})",
                c.paint(Token::Header, "## Outstanding work"),
                c.paint(Token::Count, &plan.outstanding_targets.len().to_string()),
            )?;
            let all_refs: Vec<&OutstandingTarget> = plan.outstanding_targets.iter().collect();
            write_targets_list(out, &all_refs, c)?;
        } else {
            let (actionable, blocked) = partition_targets(
                &plan.outstanding_targets,
                &plan.pending_tasks,
                &plan.completed_tasks,
            );
            let total = plan.outstanding_targets.len();
            writeln!(
                out,
                "\n{} ({} of {}):",
                c.paint(Token::Header, "## Actionable work"),
                c.paint(Token::Count, &actionable.len().to_string()),
                total,
            )?;
            write_targets_list(out, &actionable, c)?;
            if !blocked.is_empty() {
                writeln!(
                    out,
                    "  ({} more targets blocked by upstream tasks)",
                    blocked.len(),
                )?;
            }
        }
    }

    // 3. Task list (--full only).
    if full && !plan.pending_tasks.is_empty() {
        writeln!(
            out,
            "\n{}",
            c.paint(Token::Header, "## Pending tasks (in dependency order):"),
        )?;
        write_tasks(out, &plan.pending_tasks, color)?;
    }

    // 4. Completed summary.
    if !plan.completed_tasks.is_empty() {
        writeln!(out)?;
        write_completed_summary(out, &plan.completed_tasks, color)?;
    }

    if plan.outstanding_targets.is_empty()
        && plan.pending_tasks.is_empty()
        && plan.completed_tasks.is_empty()
    {
        writeln!(out, "No outstanding work.")?;
    }

    Ok(())
}

fn write_targets_list(
    out: &mut impl Write,
    criteria: &[&OutstandingTarget],
    color: ColorConfig,
) -> io::Result<()> {
    for crit in criteria {
        let body = crit.body_text.as_deref().unwrap_or("(no description)");
        let ref_str = format!("{}#{}", crit.doc_id, crit.target_id);
        writeln!(out, "- {}: {body}", color.paint(Token::DocId, &ref_str))?;
    }
    Ok(())
}

/// Partition criteria into actionable and blocked.
///
/// A criterion is "actionable" if:
/// - At least one task implementing it is unblocked (all deps completed or absent), OR
/// - No pending task implements it at all (uncovered).
///
/// Returns `(actionable, blocked)`.
fn partition_targets<'a>(
    criteria: &'a [OutstandingTarget],
    pending_tasks: &[TaskInfo],
    completed_tasks: &[TaskInfo],
) -> (Vec<&'a OutstandingTarget>, Vec<&'a OutstandingTarget>) {
    let completed_ids: HashSet<&str> = completed_tasks.iter().map(|t| t.task_id.as_str()).collect();
    let pending_ids: HashSet<&str> = pending_tasks.iter().map(|t| t.task_id.as_str()).collect();

    // Unblocked tasks: pending tasks where all depends_on are either completed
    // or not in the pending set.
    let unblocked_tasks: HashSet<&str> = pending_tasks
        .iter()
        .filter(|t| {
            t.depends_on.iter().all(|dep| {
                completed_ids.contains(dep.as_str()) || !pending_ids.contains(dep.as_str())
            })
        })
        .map(|t| t.task_id.as_str())
        .collect();

    // Criteria refs covered by unblocked tasks.
    let actionable_refs: HashSet<(&str, &str)> = pending_tasks
        .iter()
        .filter(|t| unblocked_tasks.contains(t.task_id.as_str()))
        .flat_map(|t| {
            t.implements
                .iter()
                .map(|(doc_id, crit_id)| (doc_id.as_str(), crit_id.as_str()))
        })
        .collect();

    // All criteria refs covered by any pending task.
    let all_pending_refs: HashSet<(&str, &str)> = pending_tasks
        .iter()
        .flat_map(|t| {
            t.implements
                .iter()
                .map(|(doc_id, crit_id)| (doc_id.as_str(), crit_id.as_str()))
        })
        .collect();

    let mut actionable = Vec::new();
    let mut blocked = Vec::new();

    for crit in criteria {
        let key = (crit.doc_id.as_str(), crit.target_id.as_str());
        if actionable_refs.contains(&key) || !all_pending_refs.contains(&key) {
            actionable.push(crit);
        } else {
            blocked.push(crit);
        }
    }

    (actionable, blocked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crit(doc_id: &str, crit_id: &str) -> OutstandingTarget {
        OutstandingTarget {
            doc_id: doc_id.to_owned(),
            target_id: crit_id.to_owned(),
            body_text: Some(format!("Body for {crit_id}")),
        }
    }

    fn task_info(task_id: &str, implements: &[(&str, &str)], depends_on: &[&str]) -> TaskInfo {
        TaskInfo {
            tasks_doc_id: "tasks/test".to_owned(),
            task_id: task_id.to_owned(),
            status: Some("pending".to_owned()),
            body_text: None,
            implements: implements
                .iter()
                .map(|(d, c)| (d.to_string(), c.to_string()))
                .collect(),
            depends_on: depends_on.iter().map(ToString::to_string).collect(),
        }
    }

    fn done_task_info(task_id: &str) -> TaskInfo {
        TaskInfo {
            tasks_doc_id: "tasks/test".to_owned(),
            task_id: task_id.to_owned(),
            status: Some("done".to_owned()),
            body_text: None,
            implements: vec![],
            depends_on: vec![],
        }
    }

    #[test]
    fn partition_all_actionable_when_no_deps() {
        let criteria = vec![crit("req", "c1"), crit("req", "c2")];
        let pending = vec![
            task_info("t1", &[("req", "c1")], &[]),
            task_info("t2", &[("req", "c2")], &[]),
        ];
        let (actionable, blocked) = partition_targets(&criteria, &pending, &[]);
        assert_eq!(actionable.len(), 2);
        assert!(blocked.is_empty());
    }

    #[test]
    fn partition_blocked_by_upstream() {
        let criteria = vec![crit("req", "c1"), crit("req", "c2")];
        let pending = vec![
            task_info("t1", &[("req", "c1")], &[]),
            task_info("t2", &[("req", "c2")], &["t1"]),
        ];
        let (actionable, blocked) = partition_targets(&criteria, &pending, &[]);
        assert_eq!(actionable.len(), 1);
        assert_eq!(actionable[0].target_id, "c1");
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].target_id, "c2");
    }

    #[test]
    fn partition_unblocked_when_dep_completed() {
        let criteria = vec![crit("req", "c2")];
        let pending = vec![task_info("t2", &[("req", "c2")], &["t1"])];
        let completed = vec![done_task_info("t1")];
        let (actionable, blocked) = partition_targets(&criteria, &pending, &completed);
        assert_eq!(actionable.len(), 1);
        assert!(blocked.is_empty());
    }

    #[test]
    fn partition_uncovered_criteria_are_actionable() {
        let criteria = vec![crit("req", "c1")];
        // No tasks implement c1
        let pending = vec![task_info("t1", &[("req", "other")], &[])];
        let (actionable, blocked) = partition_targets(&criteria, &pending, &[]);
        assert_eq!(actionable.len(), 1);
        assert_eq!(actionable[0].target_id, "c1");
        assert!(blocked.is_empty());
    }

    #[test]
    fn partition_no_tasks_all_actionable() {
        let criteria = vec![crit("req", "c1"), crit("req", "c2")];
        let (actionable, blocked) = partition_targets(&criteria, &[], &[]);
        assert_eq!(actionable.len(), 2);
        assert!(blocked.is_empty());
    }
}
