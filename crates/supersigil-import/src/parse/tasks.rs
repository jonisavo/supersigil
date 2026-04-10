use regex::Regex;
use std::sync::LazyLock;

use super::RawRef;
use crate::refs::parse_requirement_refs;

/// Parsed tasks.md (implementation plan).
#[derive(Debug, Clone)]
pub struct ParsedTasks {
    /// Document title extracted from the heading.
    pub title: Option<String>,
    /// Overview/preamble text before the task list.
    pub preamble: Vec<String>,
    /// Top-level tasks.
    pub tasks: Vec<ParsedTask>,
    /// Sections after the task list (e.g., `## Notes`), preserved as top-level prose.
    pub postamble: Vec<String>,
}

/// A parsed top-level task.
#[derive(Debug, Clone)]
pub struct ParsedTask {
    /// Task number (e.g., `1`, `2`).
    pub number: String,
    /// Task title text.
    pub title: String,
    /// Completion status.
    pub status: TaskStatus,
    /// Whether this task was marked as optional.
    pub is_optional: bool,
    /// Description lines following the task heading.
    pub description: Vec<String>,
    /// Requirement references from metadata lines.
    pub requirement_refs: TaskRefs,
    /// Child sub-tasks.
    pub sub_tasks: Vec<ParsedSubTask>,
}

/// A parsed sub-task within a top-level task.
#[derive(Debug, Clone)]
pub struct ParsedSubTask {
    /// Parent task number.
    pub parent_number: String,
    /// Sub-task number within the parent.
    pub number: String,
    /// Sub-task title text.
    pub title: String,
    /// Completion status.
    pub status: TaskStatus,
    /// Whether this sub-task was marked as optional.
    pub is_optional: bool,
    /// Description lines following the sub-task heading.
    pub description: Vec<String>,
    /// Requirement references from metadata lines.
    pub requirement_refs: TaskRefs,
}

/// Completion status of a task or sub-task.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    /// Task is complete (`[x]`).
    Done,
    /// Task is ready to start (`[ ]`).
    Ready,
    /// Task is in progress (`[-]`).
    InProgress,
    /// Task is in draft state (`[~]`).
    Draft,
}

/// Requirement references attached to a task via metadata lines.
#[derive(Debug, Clone)]
pub enum TaskRefs {
    /// Parsed requirement references.
    Refs(Vec<RawRef>),
    /// Unparseable metadata preserved as a comment.
    Comment(String),
    /// No requirement references.
    None,
}

// Regex patterns per the design document's parsing strategy table.

// `# Implementation Plan: Title` or `# Tasks: Title`
static DOC_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^# (?:Implementation Plan|Tasks)(?:: (.+))?$").expect("valid regex")
});

// Top-level task: `- [x] 1. Task title` or `- [x]* 1. Optional task`
static TASK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^- \[([x ~\-])\](\*)?\s*(\d+)\.\s+(.+)$").expect("valid regex"));

// Sub-task: `  - [x] 1.2 Sub-task title` or `  - [x]* 1.2 Optional sub-task`
static SUB_TASK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s+- \[([x ~\-])\](\*)?\s*(\d+)\.(\d+)\s+(.+)$").expect("valid regex")
});

// Italic metadata: `_Requirements: X.Y, Z.W_` or `_Validates: Requirements X.Y_`
// Also matches list-prefixed form: `- _Requirements: X.Y_` (common in real Kiro specs).
static META_ITALIC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:-\s+)?_(?:Requirements|Validates):\s*(.+)_\s*$").expect("valid regex")
});

// Bold metadata: `**Requirements: X.Y, Z.W**` or `**Validates: Requirements X.Y**`
// Also matches list-prefixed form: `- **Requirements: X.Y**`.
static META_BOLD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:-\s+)?\*\*(?:Requirements|Validates):\s*(.+)\*\*\s*$").expect("valid regex")
});

// Section heading (## level)
static SECTION_HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^## (.+)$").expect("valid regex"));

/// Parse a Kiro `tasks.md` file into a structured IR.
///
/// Uses line-by-line processing with regex patterns. Handles:
/// - Document title from `# Implementation Plan: Title` or `# Tasks: Title`
/// - Preamble/overview text before the task list
/// - Top-level tasks with status markers, numbers, titles
/// - Sub-tasks with parent.child numbering
/// - Description lines under tasks/sub-tasks
/// - Metadata lines in both italic and bold forms
/// - Optional task markers (`*` after status bracket)
/// - `N/A` and non-ref sentinel values
#[must_use]
pub fn parse_tasks(content: &str) -> ParsedTasks {
    let mut title: Option<String> = None;
    let mut preamble: Vec<String> = Vec::new();
    let mut tasks: Vec<ParsedTask> = Vec::new();

    let mut section = Section::Before;
    let mut preamble_lines: Vec<String> = Vec::new();
    let mut postamble_lines: Vec<String> = Vec::new();
    let mut current_task: Option<TaskBuilder> = None;
    let mut current_sub: Option<SubTaskBuilder> = None;

    for line in content.lines() {
        // Check for document title
        if let Some(caps) = DOC_TITLE_RE.captures(line) {
            title = caps.get(1).map(|m| m.as_str().trim().to_string());
            continue;
        }

        // Check for section headings
        if let Some(caps) = SECTION_HEADING_RE.captures(line) {
            let heading = caps[1].trim();
            if heading.eq_ignore_ascii_case("Tasks") {
                // Flush preamble
                flush_preamble(&preamble_lines, &mut preamble);
                preamble_lines.clear();
                section = Section::Tasks;
                continue;
            }
            // Any other ## heading before Tasks is preamble content
            if section == Section::Before || section == Section::Preamble {
                section = Section::Preamble;
                preamble_lines.push(line.to_string());
                continue;
            }
            // ## heading inside Tasks section — flush current task/sub and switch to postamble
            if section == Section::Tasks {
                flush_sub(&mut current_sub, &mut current_task);
                flush_task(&mut current_task, &mut tasks);
                section = Section::Postamble;
                postamble_lines.push(line.to_string());
                continue;
            }
            // ## heading inside Postamble — just collect it
            if section == Section::Postamble {
                postamble_lines.push(line.to_string());
                continue;
            }
        }

        match section {
            Section::Before => {
                // Lines before any ## heading — skip blank lines at start
                if !line.trim().is_empty() {
                    section = Section::Preamble;
                    preamble_lines.push(line.to_string());
                }
            }
            Section::Preamble => {
                preamble_lines.push(line.to_string());
            }
            Section::Tasks => {
                parse_task_line(line, &mut tasks, &mut current_task, &mut current_sub);
            }
            Section::Postamble => {
                postamble_lines.push(line.to_string());
            }
        }
    }

    // Flush any remaining builders
    flush_sub(&mut current_sub, &mut current_task);
    flush_task(&mut current_task, &mut tasks);

    // Flush preamble if we never hit ## Tasks
    if !preamble_lines.is_empty() && preamble.is_empty() {
        flush_preamble(&preamble_lines, &mut preamble);
    }

    // Flush postamble
    let mut postamble: Vec<String> = Vec::new();
    flush_preamble(&postamble_lines, &mut postamble);

    ParsedTasks {
        title,
        preamble,
        tasks,
        postamble,
    }
}

#[derive(Debug, PartialEq)]
enum Section {
    Before,
    Preamble,
    Tasks,
    Postamble,
}

struct TaskBuilder {
    number: String,
    title: String,
    status: TaskStatus,
    is_optional: bool,
    description_lines: Vec<String>,
    requirement_refs: TaskRefs,
    sub_tasks: Vec<ParsedSubTask>,
}

struct SubTaskBuilder {
    parent_number: String,
    number: String,
    title: String,
    status: TaskStatus,
    is_optional: bool,
    description_lines: Vec<String>,
    requirement_refs: TaskRefs,
}

impl TaskStatus {
    /// Parse a checkbox marker character into a status.
    #[must_use]
    pub fn from_marker(marker: &str) -> Self {
        match marker {
            "x" => Self::Done,
            "-" => Self::InProgress,
            "~" => Self::Draft,
            // " " and any unrecognized marker default to Ready
            _ => Self::Ready,
        }
    }

    /// Return the canonical checkbox marker character for this status.
    #[must_use]
    pub fn marker_char(&self) -> char {
        match self {
            Self::Done => 'x',
            Self::Ready => ' ',
            Self::InProgress => '-',
            Self::Draft => '~',
        }
    }

    /// Return the supersigil status string for spec document output.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Ready => "ready",
            Self::InProgress => "in-progress",
            Self::Draft => "draft",
        }
    }
}

/// Parse a metadata value into `TaskRefs`.
///
/// Handles `N/A`, `N/A (...)`, non-ref annotations like `(test infrastructure)`,
/// and actual requirement ref lists.
fn parse_metadata_value(value: &str) -> TaskRefs {
    let trimmed = value.trim();

    // N/A with optional parenthetical
    if trimmed == "N/A" || trimmed.starts_with("N/A ") || trimmed.starts_with("N/A(") {
        return TaskRefs::None;
    }

    // Strip optional "Requirements " prefix for ref parsing
    let ref_body = trimmed.strip_prefix("Requirements ").unwrap_or(trimmed);

    // If it doesn't look like refs at all (no dots), treat as comment
    if !ref_body.contains('.') {
        return TaskRefs::Comment(trimmed.to_string());
    }

    let (refs, markers) = parse_requirement_refs(trimmed);
    if refs.is_empty() {
        TaskRefs::Comment(trimmed.to_string())
    } else if !markers.is_empty() {
        // Mixed parseable and unparseable tokens — treat as comment to avoid
        // incorrect implements links (e.g., `_Requirements: 1.2, TBD_`).
        TaskRefs::Comment(trimmed.to_string())
    } else {
        TaskRefs::Refs(refs)
    }
}

fn parse_task_line(
    line: &str,
    tasks: &mut Vec<ParsedTask>,
    current_task: &mut Option<TaskBuilder>,
    current_sub: &mut Option<SubTaskBuilder>,
) {
    // Try sub-task pattern first (indented)
    if let Some(caps) = SUB_TASK_RE.captures(line) {
        // Flush previous sub-task
        flush_sub(current_sub, current_task);

        let status = TaskStatus::from_marker(&caps[1]);
        let is_optional = caps.get(2).is_some();
        let parent_number = caps[3].to_string();
        let number = caps[4].to_string();
        let sub_title = caps[5].trim().to_string();

        *current_sub = Some(SubTaskBuilder {
            parent_number,
            number,
            title: sub_title,
            status,
            is_optional,
            description_lines: Vec::new(),
            requirement_refs: TaskRefs::None,
        });
        return;
    }

    // Try top-level task pattern
    if let Some(caps) = TASK_RE.captures(line) {
        // Flush previous sub-task and task
        flush_sub(current_sub, current_task);
        flush_task(current_task, tasks);

        let status = TaskStatus::from_marker(&caps[1]);
        let is_optional = caps.get(2).is_some();
        let number = caps[3].to_string();
        let task_title = caps[4].trim().to_string();

        *current_task = Some(TaskBuilder {
            number,
            title: task_title,
            status,
            is_optional,
            description_lines: Vec::new(),
            requirement_refs: TaskRefs::None,
            sub_tasks: Vec::new(),
        });
        return;
    }

    // Try metadata line (italic or bold) — applies to current sub-task or task
    if let Some(caps) = META_ITALIC_RE
        .captures(line)
        .or_else(|| META_BOLD_RE.captures(line))
    {
        let value = caps[1].to_string();
        let refs = parse_metadata_value(&value);

        if let Some(sub) = current_sub {
            sub.requirement_refs = refs;
        } else if let Some(task) = current_task {
            task.requirement_refs = refs;
        }
        return;
    }

    // Description line — indented content under a task or sub-task
    // Strip leading indentation but preserve trailing whitespace
    let stripped = line.trim_start();
    if !stripped.trim_end().is_empty() {
        if let Some(sub) = current_sub {
            sub.description_lines.push(stripped.to_string());
        } else if let Some(task) = current_task {
            task.description_lines.push(stripped.to_string());
        }
    }
}

fn join_description_lines(lines: &[String]) -> Vec<String> {
    if lines.is_empty() {
        Vec::new()
    } else {
        vec![lines.join("\n")]
    }
}

fn flush_sub(current_sub: &mut Option<SubTaskBuilder>, current_task: &mut Option<TaskBuilder>) {
    if let Some(sub) = current_sub.take() {
        let parsed_sub = ParsedSubTask {
            parent_number: sub.parent_number,
            number: sub.number,
            title: sub.title,
            status: sub.status,
            is_optional: sub.is_optional,
            description: join_description_lines(&sub.description_lines),
            requirement_refs: sub.requirement_refs,
        };
        if let Some(task) = current_task {
            task.sub_tasks.push(parsed_sub);
        }
    }
}

fn flush_task(current_task: &mut Option<TaskBuilder>, tasks: &mut Vec<ParsedTask>) {
    if let Some(task) = current_task.take() {
        tasks.push(ParsedTask {
            number: task.number,
            title: task.title,
            status: task.status,
            is_optional: task.is_optional,
            description: join_description_lines(&task.description_lines),
            requirement_refs: task.requirement_refs,
            sub_tasks: task.sub_tasks,
        });
    }
}

fn flush_preamble(lines: &[String], preamble: &mut Vec<String>) {
    // Collect each non-empty content line as a separate preamble block,
    // preserving section headings (## Overview, etc.) for structural fidelity.
    for line in lines {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            preamble.push(trimmed.to_string());
        }
    }
}
