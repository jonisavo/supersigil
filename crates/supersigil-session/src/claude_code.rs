//! Claude Code JSONL transcript parser.
//!
//! Every line is one record with a `type`. `user` and `assistant` records
//! are turns; tool uses inside assistant records pair with tool results in
//! later user records. The parser is tolerant: unknown record types and
//! malformed lines are counted, never fatal.

pub mod content;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use supersigil_record::observations::{
    Command, CommandCategory, Edit, FileState, Hunk, Material, Observation, Outcome, Role, Turn,
};
use supersigil_record::{ContentId, EventId, SessionId, Timestamp, TurnId};

/// Characters kept from an agent turn's text.
const AGENT_EXCERPT_CHARS: usize = 240;
/// Characters kept from the end of a command's output streams.
const OUTPUT_TAIL_CHARS: usize = 2000;
/// Record types the parser knows and deliberately does not turn into
/// observations.
const IGNORED_TYPES: &[&str] = &[
    "system",
    "attachment",
    "summary",
    "file-history-snapshot",
    "file-history-delta",
    "last-prompt",
    "mode",
    "permission-mode",
];

/// Result of parsing a byte range of a transcript.
#[derive(Debug, Default)]
pub struct ParseOutcome {
    /// Observations for every record below `consumed`.
    pub observations: Vec<Observation>,
    /// Byte offset up to which records were fully turned into observations.
    pub consumed: u64,
    /// Ordinal for the first record at or after `consumed`.
    pub next_ordinal: u64,
    /// Session id from the first record that carried one.
    pub session: Option<SessionId>,
    /// Working directory from the first record that carried one.
    pub checkout: Option<PathBuf>,
    /// Branch from the first record that carried one.
    pub branch: Option<String>,
    /// Timestamp of the first record that carried one.
    pub first_time: Option<Timestamp>,
    /// Known record types that were skipped, by type.
    pub ignored_records: BTreeMap<String, u64>,
    /// Unknown record types that were skipped, by type.
    pub unknown_records: BTreeMap<String, u64>,
    /// Lines that were not valid JSON objects.
    pub malformed_lines: u64,
    /// Whether the input ended in a line without a newline.
    pub trailing_partial: bool,
    /// Edits whose path was outside the record's working directory.
    pub outside_checkout: u64,
    /// Tool uses the agent moved past without a recorded result.
    pub abandoned_tool_uses: u64,
    /// Editing tool uses whose result the harness flagged as an error; they
    /// produce no edit.
    pub failed_tool_uses: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRecord {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    parent_uuid: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    git_branch: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    is_sidechain: bool,
    #[serde(default)]
    is_meta: bool,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    message: Option<RawMessage>,
    #[serde(default)]
    tool_use_result: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    #[serde(default)]
    content: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawHunk {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    #[serde(default)]
    lines: Vec<String>,
}

#[derive(Debug)]
struct PendingTool {
    id: String,
    name: String,
    input: Value,
    turn: TurnId,
    ordinal: u64,
    time: Timestamp,
    cwd: PathBuf,
    record_index: usize,
}

#[derive(Debug)]
struct Record {
    index: usize,
    ordinal: u64,
    raw: RawRecord,
}

/// Complete lines of the input, split into records.
struct Lines {
    records: Vec<Record>,
    starts: Vec<usize>,
    complete_end: usize,
}

/// Splits `bytes` into complete lines and parses each as a record. Lines
/// that are not JSON objects are counted; a trailing line without a newline
/// is flagged and left out.
fn split_records(bytes: &[u8], start_ordinal: u64, outcome: &mut ParseOutcome) -> Lines {
    let mut lines = Lines {
        records: Vec::new(),
        starts: Vec::new(),
        complete_end: 0,
    };
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let Some(len) = bytes[cursor..].iter().position(|b| *b == b'\n') else {
            outcome.trailing_partial = true;
            break;
        };
        let line = &bytes[cursor..cursor + len];
        let index = lines.starts.len();
        lines.starts.push(cursor);
        cursor += len + 1;
        match serde_json::from_slice::<RawRecord>(line) {
            Ok(raw) => lines.records.push(Record {
                index,
                ordinal: start_ordinal + index as u64,
                raw,
            }),
            Err(_) => outcome.malformed_lines += 1,
        }
    }
    lines.complete_end = cursor;
    lines
}

/// Mutable state while walking the records in order.
struct Walk {
    outcome: ParseOutcome,
    staged: Vec<(usize, Observation)>,
    pending: Vec<PendingTool>,
    session: Option<SessionId>,
}

impl Walk {
    /// Handles one record: counts skipped types, then stages its turn and
    /// pairs its tool uses and results.
    fn record(&mut self, record: &Record) {
        let raw = &record.raw;
        match raw.kind.as_str() {
            "user" | "assistant" => {}
            other if IGNORED_TYPES.contains(&other) => {
                *self
                    .outcome
                    .ignored_records
                    .entry(other.to_owned())
                    .or_insert(0) += 1;
                return;
            }
            other => {
                *self
                    .outcome
                    .unknown_records
                    .entry(other.to_owned())
                    .or_insert(0) += 1;
                return;
            }
        }

        if self.session.is_none()
            && let Some(id) = &raw.session_id
        {
            self.session = Some(SessionId::new(id.clone()));
            self.outcome.session.clone_from(&self.session);
            self.outcome.checkout = raw.cwd.as_ref().map(PathBuf::from);
            self.outcome.branch.clone_from(&raw.git_branch);
            self.outcome.first_time = raw.timestamp.as_ref().map(|t| Timestamp::new(t.clone()));
        }
        let Some(session_id) = self.session.clone() else {
            self.count_unknown("no-session".to_owned());
            return;
        };
        let Some(uuid) = &raw.uuid else {
            self.count_unknown(format!("{}-without-uuid", raw.kind));
            return;
        };
        let turn_id = TurnId::new(uuid.clone());
        let time = Timestamp::new(raw.timestamp.clone().unwrap_or_default());
        let content = raw.message.as_ref().map(|m| &m.content);

        let role = classify_role(raw, content);
        // The agent moved on: a later assistant record or a human message
        // abandons unresolved tool uses. Tool results and meta records never
        // do, since parallel tool calls get one user record per result.
        if role == Role::Agent || role == Role::Human {
            self.abandon_pending(&session_id);
        }
        self.staged.push((
            record.index,
            Observation::Turn(Turn {
                id: turn_id.clone(),
                session: session_id.clone(),
                parent: raw.parent_uuid.clone().map(TurnId::new),
                role,
                time: time.clone(),
                sidechain: raw.is_sidechain,
                agent_id: raw.agent_id.clone(),
                excerpt: excerpt_for(role, content),
                source_ordinal: record.ordinal,
            }),
        ));

        if raw.kind == "assistant" {
            self.queue_tool_uses(record, &turn_id, &time);
        } else {
            self.resolve_results(record, &session_id, &time);
        }
    }

    fn count_unknown(&mut self, key: String) {
        *self.outcome.unknown_records.entry(key).or_insert(0) += 1;
    }

    /// Counts every unresolved tool use as abandoned and emits shell
    /// commands among them with an unavailable result.
    fn abandon_pending(&mut self, session: &SessionId) {
        for tool in self.pending.drain(..) {
            self.outcome.abandoned_tool_uses += 1;
            if let Some(observation) = build_abandoned(&tool, session) {
                self.staged.push((tool.record_index, observation));
            }
        }
    }

    fn queue_tool_uses(&mut self, record: &Record, turn: &TurnId, time: &Timestamp) {
        let raw = &record.raw;
        let cwd = PathBuf::from(raw.cwd.clone().unwrap_or_default());
        let content = raw.message.as_ref().map(|m| &m.content);
        for block in blocks(content) {
            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                self.pending.push(PendingTool {
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    input: block.get("input").cloned().unwrap_or(Value::Null),
                    turn: turn.clone(),
                    ordinal: record.ordinal,
                    time: time.clone(),
                    cwd: cwd.clone(),
                    record_index: record.index,
                });
            }
        }
    }

    fn resolve_results(&mut self, record: &Record, session: &SessionId, time: &Timestamp) {
        let raw = &record.raw;
        let content = raw.message.as_ref().map(|m| &m.content);
        let results: Vec<&Value> = blocks(content)
            .into_iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            .collect();
        let structured = if results.len() == 1 {
            raw.tool_use_result.as_ref()
        } else {
            None
        };
        for result in results {
            let id = result
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let is_error = result
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let Some(position) = self.pending.iter().position(|p| p.id == id) else {
                continue;
            };
            let tool = self.pending.remove(position);
            if let Some(observation) = build_resolved(
                &tool,
                session,
                structured,
                is_error,
                time,
                &mut self.outcome,
            ) {
                self.staged.push((record.index, observation));
            }
        }
    }
}

/// Parses complete lines of a transcript starting at ordinal `start_ordinal`.
///
/// See the module docs and [`ParseOutcome`] for what is and is not consumed.
#[must_use]
pub fn parse_transcript(bytes: &[u8], start_ordinal: u64) -> ParseOutcome {
    let mut outcome = ParseOutcome::default();
    let lines = split_records(bytes, start_ordinal, &mut outcome);
    let mut walk = Walk {
        outcome,
        staged: Vec::new(),
        pending: Vec::new(),
        session: None,
    };
    for record in &lines.records {
        walk.record(record);
    }

    // A tool use still waiting for its result at the end blocks consumption
    // from its record onward, so the next parse sees it with its result.
    let Walk {
        mut outcome,
        mut staged,
        pending,
        ..
    } = walk;
    if let Some(cut) = pending.iter().map(|p| p.record_index).min() {
        outcome.consumed = lines.starts[cut] as u64;
        outcome.next_ordinal = start_ordinal + cut as u64;
        staged.retain(|(index, _)| *index < cut);
    } else {
        outcome.consumed = lines.complete_end as u64;
        outcome.next_ordinal = start_ordinal + lines.starts.len() as u64;
    }
    outcome.observations = staged.into_iter().map(|(_, o)| o).collect();
    outcome
}

fn blocks(content: Option<&Value>) -> Vec<&Value> {
    match content {
        Some(Value::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    }
}

fn classify_role(raw: &RawRecord, content: Option<&Value>) -> Role {
    if raw.kind == "assistant" {
        return Role::Agent;
    }
    if raw.is_meta {
        return Role::Meta;
    }
    let items = blocks(content);
    if !items.is_empty()
        && items
            .iter()
            .all(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
    {
        return Role::Tool;
    }
    Role::Human
}

fn text_of(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn excerpt_for(role: Role, content: Option<&Value>) -> Material<String> {
    match role {
        Role::Human => Material::Retained(text_of(content)),
        Role::Agent => Material::Retained(truncate_chars(&text_of(content), AGENT_EXCERPT_CHARS)),
        Role::Tool => Material::Unavailable {
            reason: "tool result".to_owned(),
        },
        Role::Meta => Material::Unavailable {
            reason: "harness metadata".to_owned(),
        },
        Role::Summary => Material::Unavailable {
            reason: "compaction summary".to_owned(),
        },
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(max)).collect()
}

fn string_field<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a str> {
    value?.get(key)?.as_str()
}

/// The edit's path relative to the checkout, or `None` when it cannot be
/// shown to lie inside it. Containment is lexical: the checkout must be
/// non-empty, the path must start with it, and the remainder may contain
/// only normal components. `/work/repo/../secret` strips to `../secret` and
/// is rejected here. Symlinks are not resolved in this plan; plan 2 checks
/// paths against the git tree, which never contains one that escapes.
fn relative_path(tool: &PendingTool, result: Option<&Value>) -> Option<PathBuf> {
    if tool.cwd.as_os_str().is_empty() {
        return None;
    }
    let path = string_field(Some(&tool.input), "file_path")
        .or_else(|| string_field(result, "filePath"))?;
    let relative = Path::new(path).strip_prefix(&tool.cwd).ok()?;
    let contained = relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    (contained && !relative.as_os_str().is_empty()).then(|| relative.to_path_buf())
}

fn hunks_of(result: Option<&Value>) -> Material<Vec<Hunk>> {
    match result.and_then(|r| r.get("structuredPatch")) {
        Some(value) => match serde_json::from_value::<Vec<RawHunk>>(value.clone()) {
            Ok(raw) => Material::Retained(
                raw.into_iter()
                    .map(|h| Hunk {
                        old_start: h.old_start,
                        old_lines: h.old_lines,
                        new_start: h.new_start,
                        new_lines: h.new_lines,
                        lines: h.lines,
                    })
                    .collect(),
            ),
            Err(_) => Material::Unavailable {
                reason: "unreadable structured patch".to_owned(),
            },
        },
        None => Material::Unavailable {
            reason: "no structured patch".to_owned(),
        },
    }
}

fn state_of(text: Option<&str>) -> FileState {
    text.map_or_else(FileState::unknown, |t| {
        FileState::known(ContentId::of(t.as_bytes()))
    })
}

/// What one editing tool did to a file, apart from the path and timing.
struct EditParts {
    before: FileState,
    after: FileState,
    old_text: Material<String>,
    new_text: Material<String>,
    replace_all: bool,
}

fn edit_parts(tool: &PendingTool, result: Option<&Value>) -> EditParts {
    let original = string_field(result, "originalFile");
    let old = string_field(Some(&tool.input), "old_string")
        .or_else(|| string_field(result, "oldString"))
        .unwrap_or_default();
    let new = string_field(Some(&tool.input), "new_string")
        .or_else(|| string_field(result, "newString"))
        .unwrap_or_default();
    let replace_all = tool
        .input
        .get("replace_all")
        .and_then(Value::as_bool)
        .or_else(|| result?.get("replaceAll")?.as_bool())
        .unwrap_or(false);
    let after = original.and_then(|o| content::apply_edit(o, old, new, replace_all));
    EditParts {
        before: state_of(original),
        after: state_of(after.as_deref()),
        old_text: Material::Retained(old.to_owned()),
        new_text: Material::Retained(new.to_owned()),
        replace_all,
    }
}

fn write_parts(tool: &PendingTool, result: Option<&Value>) -> EditParts {
    let text = string_field(Some(&tool.input), "content")
        .or_else(|| string_field(result, "content"))
        .unwrap_or_default();
    let before = if string_field(result, "type") == Some("create") {
        FileState::Absent
    } else {
        state_of(string_field(result, "originalFile"))
    };
    EditParts {
        before,
        after: state_of(Some(text)),
        old_text: Material::Unavailable {
            reason: "write replaces the whole file".to_owned(),
        },
        new_text: Material::Retained(text.to_owned()),
        replace_all: false,
    }
}

fn multi_edit_parts(tool: &PendingTool, result: Option<&Value>) -> EditParts {
    let original = string_field(result, "originalFile");
    let edits: Vec<(String, String, bool)> = tool
        .input
        .get("edits")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|e| {
                    (
                        string_field(Some(e), "old_string")
                            .unwrap_or_default()
                            .to_owned(),
                        string_field(Some(e), "new_string")
                            .unwrap_or_default()
                            .to_owned(),
                        e.get("replace_all")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let after = original.and_then(|o| content::apply_multi_edit(o, &edits));
    EditParts {
        before: state_of(original),
        after: state_of(after.as_deref()),
        old_text: Material::Unavailable {
            reason: "multi-edit".to_owned(),
        },
        new_text: Material::Unavailable {
            reason: "multi-edit".to_owned(),
        },
        replace_all: false,
    }
}

fn build_edit(
    tool: &PendingTool,
    session: &SessionId,
    result: Option<&Value>,
    is_error: bool,
    ended: &Timestamp,
    outcome: &mut ParseOutcome,
) -> Option<Observation> {
    if is_error {
        // The harness says the tool failed. Whatever the input asked for did
        // not necessarily reach the disk, so there is no edit and no hash to
        // compute.
        outcome.failed_tool_uses += 1;
        return None;
    }
    let Some(path) = relative_path(tool, result) else {
        outcome.outside_checkout += 1;
        return None;
    };
    let parts = match tool.name.as_str() {
        "Edit" => edit_parts(tool, result),
        "Write" => write_parts(tool, result),
        _ => multi_edit_parts(tool, result),
    };
    Some(Observation::Edit(Edit {
        id: EventId::derive("edit", session, &tool.id),
        turn: tool.turn.clone(),
        session: session.clone(),
        path,
        before: parts.before,
        after: parts.after,
        patch: hunks_of(result),
        old_text: parts.old_text,
        new_text: parts.new_text,
        replace_all: parts.replace_all,
        checkout: tool.cwd.clone(),
        time: ended.clone(),
        source_ordinal: tool.ordinal,
    }))
}

fn build_command(
    tool: &PendingTool,
    session: &SessionId,
    result: Option<&Value>,
    is_error: bool,
    ended: &Timestamp,
) -> Observation {
    let cmd = string_field(Some(&tool.input), "command")
        .unwrap_or_default()
        .to_owned();
    let category = classify_command(&cmd);
    let stdout = string_field(result, "stdout").unwrap_or_default();
    let stderr = string_field(result, "stderr").unwrap_or_default();
    let interrupted = result
        .and_then(|r| r.get("interrupted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let outcome_value = if category == CommandCategory::TestRun && !interrupted {
        test_outcome(stdout, stderr).or(is_error.then_some(Outcome::Failed))
    } else {
        None
    };
    Observation::Command(Command {
        id: EventId::derive("command", session, &tool.id),
        turn: tool.turn.clone(),
        session: session.clone(),
        cmd,
        exit: None,
        stdout_tail: Material::Retained(tail_chars(stdout, OUTPUT_TAIL_CHARS)),
        stderr_tail: Material::Retained(tail_chars(stderr, OUTPUT_TAIL_CHARS)),
        category,
        reported_error: is_error,
        outcome: outcome_value,
        started: tool.time.clone(),
        ended: Some(ended.clone()),
        checkout: tool.cwd.clone(),
        source_ordinal: tool.ordinal,
    })
}

fn build_resolved(
    tool: &PendingTool,
    session: &SessionId,
    result: Option<&Value>,
    is_error: bool,
    ended: &Timestamp,
    outcome: &mut ParseOutcome,
) -> Option<Observation> {
    match tool.name.as_str() {
        "Edit" | "Write" | "MultiEdit" => {
            build_edit(tool, session, result, is_error, ended, outcome)
        }
        "Bash" => Some(build_command(tool, session, result, is_error, ended)),
        _ => None,
    }
}

fn build_abandoned(tool: &PendingTool, session: &SessionId) -> Option<Observation> {
    if tool.name != "Bash" {
        return None;
    }
    let cmd = string_field(Some(&tool.input), "command")
        .unwrap_or_default()
        .to_owned();
    let category = classify_command(&cmd);
    let reason = "no result recorded".to_owned();
    Some(Observation::Command(Command {
        id: EventId::derive("command", session, &tool.id),
        turn: tool.turn.clone(),
        session: session.clone(),
        cmd,
        exit: None,
        stdout_tail: Material::Unavailable {
            reason: reason.clone(),
        },
        stderr_tail: Material::Unavailable { reason },
        category,
        reported_error: false,
        outcome: None,
        started: tool.time.clone(),
        ended: None,
        checkout: tool.cwd.clone(),
        source_ordinal: tool.ordinal,
    }))
}

const TESTS: &[&str] = &[
    "nextest",
    "cargo test",
    "vitest",
    "pnpm test",
    "npm test",
    "pytest",
    "go test",
    "jest",
];
const BUILDS: &[&str] = &[
    "cargo build",
    "cargo check",
    "cargo clippy",
    "tsc",
    "pnpm build",
    "npm run build",
];

/// Coarse classification of a shell command line.
#[must_use]
pub fn classify_command(cmd: &str) -> CommandCategory {
    let lower = cmd.to_lowercase();
    if TESTS.iter().any(|t| lower.contains(t)) {
        CommandCategory::TestRun
    } else if lower.starts_with("git ") || lower.contains("&& git ") || lower.starts_with("gh ") {
        CommandCategory::Git
    } else if BUILDS.iter().any(|b| lower.contains(b)) {
        CommandCategory::Build
    } else {
        CommandCategory::Other
    }
}

/// Reads a pass or fail verdict from cargo test or nextest output.
///
/// Failure takes precedence: a workspace run prints one `test result:` line
/// per binary, and one failing binary after several passing ones is a
/// failed run. `None` means no verdict could be read, not success.
#[must_use]
pub fn test_outcome(stdout: &str, stderr: &str) -> Option<Outcome> {
    let text = format!("{stdout}\n{stderr}");
    let nextest_summary = text
        .lines()
        .find(|l| l.contains("tests run:") || l.contains("test run:"));
    let failed = text.contains("test result: FAILED")
        || nextest_summary.is_some_and(|l| l.contains("failed"))
        || text.contains("FAILED");
    if failed {
        return Some(Outcome::Failed);
    }
    if text.contains("test result: ok") || nextest_summary.is_some() {
        return Some(Outcome::Passed);
    }
    None
}
