use std::fmt::Write;

use crate::emit::{emit_front_matter, format_marker, xml_escape};
use crate::ids::{deduplicate_ids, make_task_id};
use crate::parse::tasks::{ParsedSubTask, ParsedTasks, TaskRefs, TaskStatus};
use crate::refs::{self, RequirementIndex};
use crate::{AmbiguityBreakdown, AmbiguityKind};

/// Return the length of the longest run of consecutive backtick characters in `text`.
fn max_backtick_run(text: &str) -> usize {
    let mut max = 0;
    let mut current = 0;
    for c in text.chars() {
        if c == '`' {
            current += 1;
            max = max.max(current);
        } else {
            current = 0;
        }
    }
    max
}

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
    breakdown: &mut AmbiguityBreakdown,
    task_markers: &mut Vec<String>,
) {
    let _ = writeln!(out, "{}  {}", ctx.indent, xml_escape(title));

    if ctx.is_optional {
        let marker = format_marker(&format!(
            "This task was marked as optional in Kiro; \
             supersigil <Task> has no optional attribute (task: {})",
            ctx.task_id
        ));
        task_markers.push(marker);
        breakdown.record(AmbiguityKind::UnsupportedFeature);
    }

    for desc in description {
        let trimmed = desc.trim();
        if !trimmed.is_empty() {
            out.push('\n');
            for line in trimmed.lines() {
                let _ = writeln!(out, "{}  {}", ctx.indent, xml_escape(line.trim()));
            }
        }
    }

    if let TaskRefs::Comment(comment) = ctx.refs {
        let marker = format_marker(&format!(
            "Kiro metadata for task {}: {comment}",
            ctx.task_id
        ));
        task_markers.push(marker);
        breakdown.record(AmbiguityKind::UnsupportedFeature);
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
        let marker = format_marker(&format!(
            "Could not resolve implements references '{refs_str}' for task {task_id}"
        ));
        return (None, vec![marker], 0);
    };

    let (resolved, mut markers) = refs::resolve_refs(raw_refs, index, req_doc_id);
    let resolved_count = resolved.len();

    if resolved_count < raw_refs.len() {
        let refs_str = format_raw_refs(raw_refs);
        let marker = if resolved_count == 0 {
            format_marker(&format!(
                "Could not resolve implements references '{refs_str}' for task {task_id}"
            ))
        } else {
            format_marker(&format!(
                "Only {resolved_count} of {} implements \
                 references resolved for task {task_id} (from '{refs_str}')",
                raw_refs.len()
            ))
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

/// Emit a tasks spec document from parsed Kiro tasks.
///
/// When `req_index` is provided, task requirement refs are resolved inline
/// against the index. When absent, unresolvable-ref markers are emitted.
///
/// Returns `(md_content, ambiguity_breakdown, validates_resolved)`.
#[must_use]
pub fn emit_tasks_md(
    parsed: &ParsedTasks,
    doc_id: &str,
    req_index: Option<&RequirementIndex<'_>>,
    req_doc_id: &str,
    feature_title: &str,
) -> (String, AmbiguityBreakdown, usize) {
    let mut out = String::new();
    let mut breakdown = AmbiguityBreakdown::default();
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
    for _ in &dedup_markers {
        breakdown.record(AmbiguityKind::DuplicateId);
    }

    // Task components
    let mut id_cursor = 0;
    let mut prev_top_id: Option<&str> = None;

    for task in &parsed.tasks {
        let task_id = &deduped_ids[id_cursor];
        id_cursor += 1;

        let (implements, impl_markers, resolved_count) =
            resolve_task_implements(&task.requirement_refs, task_id, req_index, req_doc_id);
        validates_resolved += resolved_count;
        for _ in &impl_markers {
            breakdown.record(AmbiguityKind::UnresolvedRef);
        }
        task_markers.extend(impl_markers);

        let attrs = task_attrs(task_id, &task.status, prev_top_id, implements.as_deref());

        // Compute fence length: scan all content that will appear inside the fence.
        let mut max_run = max_backtick_run(&task.title);
        for desc in &task.description {
            max_run = max_run.max(max_backtick_run(desc));
        }
        for sub in &task.sub_tasks {
            max_run = max_run.max(max_backtick_run(&sub.title));
            for desc in &sub.description {
                max_run = max_run.max(max_backtick_run(desc));
            }
        }
        let fence_len = max_run + 1;
        let fence_len = fence_len.max(3);
        let fence = "`".repeat(fence_len);

        let _ = writeln!(out, "{fence}supersigil-xml");
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
            &mut breakdown,
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
            &mut breakdown,
            &mut validates_resolved,
            &mut task_markers,
        );

        let _ = writeln!(out, "</Task>");
        let _ = writeln!(out, "{fence}");
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

    (out, breakdown, validates_resolved)
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
    breakdown: &mut AmbiguityBreakdown,
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
        for _ in &impl_markers {
            breakdown.record(AmbiguityKind::UnresolvedRef);
        }
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
            breakdown,
            task_markers,
        );

        let _ = writeln!(out, "  </Task>");

        prev_sub_id = Some(sub_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::tasks::ParsedTask;

    fn simple_task(title: &str, description: Vec<String>) -> ParsedTask {
        ParsedTask {
            number: "1".to_string(),
            title: title.to_string(),
            status: TaskStatus::Ready,
            is_optional: false,
            description,
            requirement_refs: TaskRefs::None,
            sub_tasks: vec![],
        }
    }

    fn parsed_tasks_with(task: ParsedTask) -> ParsedTasks {
        ParsedTasks {
            title: None,
            preamble: vec![],
            tasks: vec![task],
            postamble: vec![],
        }
    }

    #[test]
    fn max_backtick_run_empty() {
        assert_eq!(max_backtick_run(""), 0);
    }

    #[test]
    fn max_backtick_run_no_backticks() {
        assert_eq!(max_backtick_run("hello world"), 0);
    }

    #[test]
    fn max_backtick_run_single() {
        assert_eq!(max_backtick_run("a`b"), 1);
    }

    #[test]
    fn max_backtick_run_triple() {
        assert_eq!(max_backtick_run("some ```triple``` backticks"), 3);
    }

    #[test]
    fn max_backtick_run_mixed_runs() {
        // longest run wins
        assert_eq!(max_backtick_run("```` then `` then `"), 4);
    }

    #[test]
    fn emit_uses_triple_fence_when_no_backticks() {
        let task = simple_task("Do something", vec![]);
        let parsed = parsed_tasks_with(task);
        let (output, _, _) = emit_tasks_md(&parsed, "feat/tasks", None, "", "Feature");

        assert!(
            output.contains("```supersigil-xml"),
            "expected triple-backtick fence when no backticks in content"
        );
        // Ensure the fence is exactly 3 backticks, not more
        assert!(
            !output.contains("````supersigil-xml"),
            "fence should be exactly 3 backticks when no backticks in content"
        );
    }

    #[test]
    fn emit_uses_quadruple_fence_when_description_has_triple_backticks() {
        let desc = "Use this code:\n```rust\nfn main() {}\n```\nDone.".to_string();
        let task = simple_task("Implement feature", vec![desc]);
        let parsed = parsed_tasks_with(task);
        let (output, _, _) = emit_tasks_md(&parsed, "feat/tasks", None, "", "Feature");

        // The outer fence must be 4 backticks
        assert!(
            output.contains("````supersigil-xml"),
            "expected 4-backtick fence when description contains triple backticks;\noutput:\n{output}"
        );
        // Must also close with 4 backticks
        // The closing fence is a line with exactly 4 backticks
        let has_closing_fence = output.lines().any(|l| l == "````");
        assert!(
            has_closing_fence,
            "expected closing 4-backtick fence line;\noutput:\n{output}"
        );
        // Must NOT have a fence line that is exactly 3 backticks (would be broken by inner ```)
        let has_triple_only_fence = output.lines().any(|l| l == "```supersigil-xml");
        assert!(
            !has_triple_only_fence,
            "outer fence must not be exactly 3 backticks;\noutput:\n{output}"
        );
    }

    #[test]
    fn emit_uses_five_fence_when_description_has_quadruple_backticks() {
        let desc = "See ```` for details.".to_string();
        let task = simple_task("Use quad backticks", vec![desc]);
        let parsed = parsed_tasks_with(task);
        let (output, _, _) = emit_tasks_md(&parsed, "feat/tasks", None, "", "Feature");

        assert!(
            output.contains("`````supersigil-xml"),
            "expected 5-backtick fence when content has 4 consecutive backticks;\noutput:\n{output}"
        );
    }

    #[test]
    fn emit_uses_quadruple_fence_when_subtask_description_has_triple_backticks() {
        let sub_desc = "Run: ```shell\nls\n```".to_string();
        let sub = ParsedSubTask {
            parent_number: "1".to_string(),
            number: "1".to_string(),
            title: "Sub-task".to_string(),
            status: TaskStatus::Ready,
            is_optional: false,
            description: vec![sub_desc],
            requirement_refs: TaskRefs::None,
        };
        let task = ParsedTask {
            number: "1".to_string(),
            title: "Parent task".to_string(),
            status: TaskStatus::Ready,
            is_optional: false,
            description: vec![],
            requirement_refs: TaskRefs::None,
            sub_tasks: vec![sub],
        };
        let parsed = parsed_tasks_with(task);
        let (output, _, _) = emit_tasks_md(&parsed, "feat/tasks", None, "", "Feature");

        assert!(
            output.contains("````supersigil-xml"),
            "expected 4-backtick fence when subtask description contains triple backticks;\noutput:\n{output}"
        );
    }

    #[test]
    fn emit_uses_quadruple_fence_when_title_has_triple_backticks() {
        // A task title with triple backticks (unusual but must be handled)
        let task = simple_task("Use ```code``` in title", vec![]);
        let parsed = parsed_tasks_with(task);
        let (output, _, _) = emit_tasks_md(&parsed, "feat/tasks", None, "", "Feature");

        assert!(
            output.contains("````supersigil-xml"),
            "expected 4-backtick fence when title contains triple backticks;\noutput:\n{output}"
        );
    }
}
