use std::collections::HashMap;
use std::fmt::Write;

use crate::emit::emit_front_matter;
use crate::ids::{deduplicate_ids, make_task_id};
use crate::parse::tasks::{ParsedSubTask, ParsedTasks, TaskRefs, TaskStatus};

/// Build the attribute string for a `<Task>` opening tag.
fn task_attrs(
    id: &str,
    status: &TaskStatus,
    depends: Option<&str>,
    implements: Option<&str>,
) -> String {
    let mut attrs = format!("id=\"{id}\" status=\"{}\"", status.as_str());
    if let Some(dep) = depends {
        let _ = write!(attrs, " depends=\"{dep}\"");
    }
    if let Some(imp) = implements {
        let _ = write!(attrs, " implements=\"{imp}\"");
    }
    attrs
}

/// Emit the body content of a task (title, description, optional marker, comment).
fn emit_task_body(
    out: &mut String,
    title: &str,
    description: &[String],
    is_optional: bool,
    refs: &TaskRefs,
    ambiguity_count: &mut usize,
    indent: &str,
) {
    let _ = writeln!(out, "{indent}  {title}");

    if is_optional {
        out.push('\n');
        let marker = format!(
            "{indent}  <!-- TODO(supersigil-import): This task was marked as optional in Kiro; \
             supersigil <Task> has no optional attribute -->"
        );
        let _ = writeln!(out, "{marker}");
        *ambiguity_count += 1;
    }

    for desc in description {
        let trimmed = desc.trim();
        if !trimmed.is_empty() {
            out.push('\n');
            let _ = writeln!(out, "{indent}  {trimmed}");
        }
    }

    if let TaskRefs::Comment(comment) = refs {
        out.push('\n');
        let _ = writeln!(out, "{indent}  <!-- {comment} -->");
    }
}

/// Emit a tasks MDX document from parsed Kiro tasks.
///
/// `resolved_implements` maps raw task IDs (e.g., `"task-1"`, `"task-1-2"`) to
/// resolved criterion ref strings. `ambiguity_markers` are pre-computed markers
/// from the resolution phase.
///
/// Returns `(mdx_content, ambiguity_count)`.
#[must_use]
#[allow(clippy::implicit_hasher, reason = "public API always uses std HashMap")]
pub fn emit_tasks_mdx(
    parsed: &ParsedTasks,
    doc_id: &str,
    resolved_implements: &HashMap<String, Vec<String>>,
    feature_title: &str,
    ambiguity_markers: &[String],
) -> (String, usize) {
    let mut out = String::new();
    let mut ambiguity_count = ambiguity_markers.len();

    emit_front_matter(&mut out, doc_id, "tasks", feature_title);

    // Preamble prose
    for preamble in &parsed.preamble {
        let trimmed = preamble.trim();
        if !trimmed.is_empty() {
            out.push_str(trimmed);
            out.push_str("\n\n");
        }
    }

    // Collect all raw task IDs in order for deduplication (Req 9.3, 9.4)
    let mut raw_ids: Vec<String> = Vec::new();
    for task in &parsed.tasks {
        raw_ids.push(make_task_id(&task.number, None));
        for sub in &task.sub_tasks {
            raw_ids.push(make_task_id(&task.number, Some(&sub.number)));
        }
    }
    let (deduped_ids, dedup_markers) = deduplicate_ids(&raw_ids);
    ambiguity_count += dedup_markers.len();

    // Task components — use positional index for implements lookup to avoid
    // collisions when multiple tasks share the same number.
    let mut id_cursor = 0;
    let mut prev_top_id: Option<&str> = None;

    for task in &parsed.tasks {
        let task_id = &deduped_ids[id_cursor];
        let pos_key = id_cursor.to_string();
        id_cursor += 1;

        // Look up implements using positional index (collision-safe key)
        let implements = resolved_implements.get(&pos_key).map(|r| r.join(", "));

        let attrs = task_attrs(task_id, &task.status, prev_top_id, implements.as_deref());
        let _ = writeln!(out, "<Task {attrs}>");

        emit_task_body(
            &mut out,
            &task.title,
            &task.description,
            task.is_optional,
            &task.requirement_refs,
            &mut ambiguity_count,
            "",
        );

        // Emit unresolvable ref markers for this task
        emit_unresolved_markers(
            &mut out,
            &task.requirement_refs,
            task_id,
            &pos_key,
            resolved_implements,
            &mut ambiguity_count,
            "  ",
        );

        // Sub-tasks
        emit_sub_tasks_deduped(
            &mut out,
            &task.sub_tasks,
            &deduped_ids,
            &mut id_cursor,
            resolved_implements,
            &mut ambiguity_count,
        );

        let _ = writeln!(out, "</Task>");
        out.push('\n');

        prev_top_id = Some(task_id);
    }

    // Postamble prose (e.g., ## Notes sections after the task list)
    for postamble in &parsed.postamble {
        let trimmed = postamble.trim();
        if !trimmed.is_empty() {
            out.push_str(trimmed);
            out.push_str("\n\n");
        }
    }

    // Append pre-computed ambiguity markers from resolution phase
    for marker in ambiguity_markers {
        let _ = writeln!(out, "{marker}");
    }

    // Append dedup markers at end of document
    for marker in &dedup_markers {
        let _ = writeln!(out, "{marker}");
    }

    (out, ambiguity_count)
}

fn emit_sub_tasks_deduped(
    out: &mut String,
    sub_tasks: &[ParsedSubTask],
    deduped_ids: &[String],
    id_cursor: &mut usize,
    resolved_implements: &HashMap<String, Vec<String>>,
    ambiguity_count: &mut usize,
) {
    let mut prev_sub_id: Option<&str> = None;

    for sub in sub_tasks {
        let sub_id = &deduped_ids[*id_cursor];
        let pos_key = id_cursor.to_string();
        *id_cursor += 1;

        let implements = resolved_implements.get(&pos_key).map(|r| r.join(", "));

        out.push('\n');
        let attrs = task_attrs(sub_id, &sub.status, prev_sub_id, implements.as_deref());
        let _ = writeln!(out, "  <Task {attrs}>");

        emit_task_body(
            out,
            &sub.title,
            &sub.description,
            sub.is_optional,
            &sub.requirement_refs,
            ambiguity_count,
            "  ",
        );

        emit_unresolved_markers(
            out,
            &sub.requirement_refs,
            sub_id,
            &pos_key,
            resolved_implements,
            ambiguity_count,
            "    ",
        );

        let _ = writeln!(out, "  </Task>");

        prev_sub_id = Some(sub_id);
    }
}

/// Emit ambiguity markers for task refs that couldn't be resolved.
///
/// `task_id` is the (possibly deduped) ID used in the output.
/// `raw_task_id` is the original ID used as key in `resolved_implements`.
fn emit_unresolved_markers(
    out: &mut String,
    refs: &TaskRefs,
    task_id: &str,
    raw_task_id: &str,
    resolved_implements: &HashMap<String, Vec<String>>,
    ambiguity_count: &mut usize,
    indent: &str,
) {
    let raw_refs = match refs {
        TaskRefs::Refs(r) if !r.is_empty() => r,
        _ => return,
    };

    let resolved_count = resolved_implements.get(raw_task_id).map_or(0, Vec::len);

    if resolved_count >= raw_refs.len() {
        // All refs resolved — nothing to report
        return;
    }

    let refs_str: String = raw_refs
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    let marker = if resolved_count == 0 {
        format!(
            "{indent}<!-- TODO(supersigil-import): Could not resolve implements references \
             '{refs_str}' for task {task_id} -->"
        )
    } else {
        // Partial resolution — some resolved, some didn't. We can't know exactly
        // which resolved without re-doing resolution, so note the count discrepancy.
        format!(
            "{indent}<!-- TODO(supersigil-import): Only {resolved_count} of {} implements \
             references resolved for task {task_id} (from '{refs_str}') -->",
            raw_refs.len()
        )
    };
    let _ = writeln!(out, "{marker}");
    *ambiguity_count += 1;
}
