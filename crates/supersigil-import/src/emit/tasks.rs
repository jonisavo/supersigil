use std::fmt::Write;

use crate::emit::emit_front_matter;
use crate::ids::{deduplicate_ids, make_task_id};
use crate::parse::tasks::{ParsedSubTask, ParsedTasks, TaskRefs, TaskStatus};
use crate::refs::{self, RequirementIndex};

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
#[derive(Clone, Copy)]
struct TaskBodyCtx<'a> {
    task_id: &'a str,
    is_optional: bool,
    refs: &'a TaskRefs,
    indent: &'a str,
}

fn emit_task_body(
    out: &mut String,
    title: &str,
    description: &[String],
    ctx: TaskBodyCtx<'_>,
    ambiguity_count: &mut usize,
    task_markers: &mut Vec<String>,
) {
    let _ = writeln!(out, "{}  {title}", ctx.indent);

    if ctx.is_optional {
        let marker = format!(
            "<!-- TODO(supersigil-import): This task was marked as optional in Kiro; \
             supersigil <Task> has no optional attribute (task: {}) -->",
            ctx.task_id
        );
        task_markers.push(marker);
        *ambiguity_count += 1;
    }

    for desc in description {
        let trimmed = desc.trim();
        if !trimmed.is_empty() {
            out.push('\n');
            for line in trimmed.lines() {
                let _ = writeln!(out, "{}  {}", ctx.indent, line.trim());
            }
        }
    }

    if let TaskRefs::Comment(comment) = ctx.refs {
        let comment = comment.replace("*/", "* /");
        out.push('\n');
        let _ = writeln!(out, "{}  {{/* {comment} */}}", ctx.indent);
    }
}

/// Resolve requirement refs for a task into an implements attribute value.
///
/// Returns `(implements_string, ambiguity_markers, validates_resolved_count)`.
fn resolve_task_implements(
    task_refs: &TaskRefs,
    task_id: &str,
    req_index: Option<&RequirementIndex<'_>>,
    req_doc_id: &str,
) -> (Option<String>, Vec<String>, usize) {
    let raw_refs = match task_refs {
        TaskRefs::Refs(r) if !r.is_empty() => r,
        _ => return (None, Vec::new(), 0),
    };

    let Some(index) = req_index else {
        let refs_str = format_raw_refs(raw_refs);
        let marker = format!(
            "<!-- TODO(supersigil-import): Could not resolve implements references \
             '{refs_str}' for task {task_id} -->"
        );
        return (None, vec![marker], 0);
    };

    let (resolved, mut markers) = refs::resolve_refs(raw_refs, index, req_doc_id);
    let resolved_count = resolved.len();

    if resolved_count < raw_refs.len() {
        let refs_str = format_raw_refs(raw_refs);
        let marker = if resolved_count == 0 {
            format!(
                "<!-- TODO(supersigil-import): Could not resolve implements references \
                 '{refs_str}' for task {task_id} -->"
            )
        } else {
            format!(
                "<!-- TODO(supersigil-import): Only {resolved_count} of {} implements \
                 references resolved for task {task_id} (from '{refs_str}') -->",
                raw_refs.len()
            )
        };
        markers.push(marker);
    }

    let implements = if resolved.is_empty() {
        None
    } else {
        Some(resolved.join(", "))
    };

    (implements, markers, resolved_count)
}

fn format_raw_refs(refs: &[crate::parse::RawRef]) -> String {
    refs.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Emit a tasks MDX document from parsed Kiro tasks.
///
/// When `req_index` is provided, task requirement refs are resolved inline
/// against the index. When absent, unresolvable-ref markers are emitted.
///
/// Returns `(mdx_content, ambiguity_count, validates_resolved)`.
#[must_use]
pub fn emit_tasks_mdx(
    parsed: &ParsedTasks,
    doc_id: &str,
    req_index: Option<&RequirementIndex<'_>>,
    req_doc_id: &str,
    feature_title: &str,
) -> (String, usize, usize) {
    let mut out = String::new();
    let mut ambiguity_count = 0;
    let mut validates_resolved = 0;
    let mut task_markers: Vec<String> = Vec::new();

    emit_front_matter(&mut out, doc_id, "tasks", feature_title);

    // Preamble prose
    for preamble in &parsed.preamble {
        let trimmed = preamble.trim();
        if !trimmed.is_empty() {
            out.push_str(trimmed);
            out.push_str("\n\n");
        }
    }

    // Collect all raw task IDs in order for deduplication
    let mut raw_ids: Vec<String> = Vec::new();
    for task in &parsed.tasks {
        raw_ids.push(make_task_id(&task.number, None));
        for sub in &task.sub_tasks {
            raw_ids.push(make_task_id(&task.number, Some(&sub.number)));
        }
    }
    let (deduped_ids, dedup_markers) = deduplicate_ids(&raw_ids);
    ambiguity_count += dedup_markers.len();

    // Task components
    let mut id_cursor = 0;
    let mut prev_top_id: Option<&str> = None;

    for task in &parsed.tasks {
        let task_id = &deduped_ids[id_cursor];
        id_cursor += 1;

        let (implements, impl_markers, resolved_count) =
            resolve_task_implements(&task.requirement_refs, task_id, req_index, req_doc_id);
        validates_resolved += resolved_count;
        ambiguity_count += impl_markers.len();
        task_markers.extend(impl_markers);

        let attrs = task_attrs(task_id, &task.status, prev_top_id, implements.as_deref());
        let _ = writeln!(out, "<Task {attrs}>");

        emit_task_body(
            &mut out,
            &task.title,
            &task.description,
            TaskBodyCtx {
                task_id,
                is_optional: task.is_optional,
                refs: &task.requirement_refs,
                indent: "",
            },
            &mut ambiguity_count,
            &mut task_markers,
        );

        // Sub-tasks
        emit_sub_tasks(
            &mut out,
            &task.sub_tasks,
            &deduped_ids,
            &mut id_cursor,
            req_index,
            req_doc_id,
            &mut ambiguity_count,
            &mut validates_resolved,
            &mut task_markers,
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

    for marker in &task_markers {
        let _ = writeln!(out, "{marker}");
    }

    // Append dedup markers at end of document
    for marker in &dedup_markers {
        let _ = writeln!(out, "{marker}");
    }

    (out, ambiguity_count, validates_resolved)
}

#[allow(
    clippy::too_many_arguments,
    reason = "internal helper, params are all distinct concerns"
)]
fn emit_sub_tasks(
    out: &mut String,
    sub_tasks: &[ParsedSubTask],
    deduped_ids: &[String],
    id_cursor: &mut usize,
    req_index: Option<&RequirementIndex<'_>>,
    req_doc_id: &str,
    ambiguity_count: &mut usize,
    validates_resolved: &mut usize,
    task_markers: &mut Vec<String>,
) {
    let mut prev_sub_id: Option<&str> = None;

    for sub in sub_tasks {
        let sub_id = &deduped_ids[*id_cursor];
        *id_cursor += 1;

        let (implements, impl_markers, resolved_count) =
            resolve_task_implements(&sub.requirement_refs, sub_id, req_index, req_doc_id);
        *validates_resolved += resolved_count;
        *ambiguity_count += impl_markers.len();
        task_markers.extend(impl_markers);

        out.push('\n');
        let attrs = task_attrs(sub_id, &sub.status, prev_sub_id, implements.as_deref());
        let _ = writeln!(out, "  <Task {attrs}>");

        emit_task_body(
            out,
            &sub.title,
            &sub.description,
            TaskBodyCtx {
                task_id: sub_id,
                is_optional: sub.is_optional,
                refs: &sub.requirement_refs,
                indent: "  ",
            },
            ambiguity_count,
            task_markers,
        );

        let _ = writeln!(out, "  </Task>");

        prev_sub_id = Some(sub_id);
    }
}
