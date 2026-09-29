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
    /// The session given to [`parse_transcript_with_session`], else the
    /// session id from the first record that carried one.
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

/// A per-record tally, kept with the line it came from so that counts for
/// lines beyond the consumption cutoff can be dropped.
#[derive(Debug)]
enum Count {
    Ignored(String),
    Unknown(String),
    Malformed,
    Outside,
    Abandoned,
    Failed,
}

/// Complete lines of the input, split into records.
struct Lines {
    records: Vec<Record>,
    malformed: Vec<usize>,
    starts: Vec<usize>,
    complete_end: usize,
}

/// Splits `bytes` into complete lines and parses each as a record. Lines
/// that are not JSON objects are counted; a trailing line without a newline
/// is flagged and left out.
fn split_records(bytes: &[u8], start_ordinal: u64, outcome: &mut ParseOutcome) -> Lines {
    let mut lines = Lines {
        records: Vec::new(),
        malformed: Vec::new(),
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
            Err(_) => lines.malformed.push(index),
        }
    }
    lines.complete_end = cursor;
    lines
}

/// Mutable state while walking the records in order.
struct Walk {
    outcome: ParseOutcome,
    staged: Vec<(usize, Observation)>,
    counts: Vec<(usize, Count)>,
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
                self.counts
                    .push((record.index, Count::Ignored(other.to_owned())));
                return;
            }
            other => {
                self.counts
                    .push((record.index, Count::Unknown(other.to_owned())));
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
            self.count_unknown(record.index, "no-session".to_owned());
            return;
        };
        let Some(uuid) = &raw.uuid else {
            self.count_unknown(record.index, format!("{}-without-uuid", raw.kind));
            return;
        };
        let turn_id = TurnId::new(uuid.clone());
        let time = Timestamp::new(raw.timestamp.clone().unwrap_or_default());
        let content = raw.message.as_ref().map(|m| &m.content);

        let role = classify_role(raw, content);
        // The agent moved on: a later assistant record abandons unresolved
        // tool uses before it issues its own.
        if role == Role::Agent {
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
            // A human message abandons what is still unresolved, but only
            // after its own tool results are matched: a typed message can
            // share a record with the results it follows. Tool results and
            // meta records never abandon, since parallel tool calls get one
            // user record per result.
            self.resolve_results(record, &session_id, &time);
            if role == Role::Human {
                self.abandon_pending(&session_id);
            }
        }
    }

    fn count_unknown(&mut self, index: usize, key: String) {
        self.counts.push((index, Count::Unknown(key)));
    }

    /// Counts every unresolved tool use as abandoned and emits shell
    /// commands among them with an unavailable result.
    fn abandon_pending(&mut self, session: &SessionId) {
        for tool in self.pending.drain(..) {
            self.counts.push((tool.record_index, Count::Abandoned));
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
                &Resolution {
                    structured,
                    block: result,
                    is_error,
                    ended: time,
                },
                &mut Tally {
                    index: record.index,
                    counts: &mut self.counts,
                },
            ) {
                self.staged.push((record.index, observation));
            }
        }
    }
}

/// Parses complete lines of a transcript starting at ordinal `start_ordinal`.
///
/// See the module docs and [`ParseOutcome`] for what is and is not consumed.
/// Equivalent to [`parse_transcript_with_session`] without a known session.
#[must_use]
pub fn parse_transcript(bytes: &[u8], start_ordinal: u64) -> ParseOutcome {
    parse_transcript_with_session(bytes, start_ordinal, None)
}

/// Parses complete lines of a transcript starting at ordinal `start_ordinal`,
/// continuing a transcript whose session is already known.
///
/// Sync resumes mid-file, where appended records need not repeat the
/// session id. With `session` given, records without a `sessionId` belong to
/// it and [`ParseOutcome::session`] is that session, so parsing a file in
/// chunks yields the same observations as parsing it whole. The session,
/// checkout, branch, and first time are only read from records when no
/// session is given.
#[must_use]
pub fn parse_transcript_with_session(
    bytes: &[u8],
    start_ordinal: u64,
    session: Option<&SessionId>,
) -> ParseOutcome {
    let mut outcome = ParseOutcome {
        session: session.cloned(),
        ..ParseOutcome::default()
    };
    let lines = split_records(bytes, start_ordinal, &mut outcome);
    let mut walk = Walk {
        outcome,
        staged: Vec::new(),
        counts: lines
            .malformed
            .iter()
            .map(|i| (*i, Count::Malformed))
            .collect(),
        pending: Vec::new(),
        session: session.cloned(),
    };
    for record in &lines.records {
        walk.record(record);
    }

    // A tool use still waiting for its result at the end blocks consumption
    // from its record onward, so the next parse sees it with its result.
    let Walk {
        mut outcome,
        mut staged,
        counts,
        pending,
        ..
    } = walk;
    let mut cutoff = lines.starts.len();
    if let Some(cut) = pending.iter().map(|p| p.record_index).min() {
        cutoff = cut;
        outcome.consumed = lines.starts[cut] as u64;
        outcome.next_ordinal = start_ordinal + cut as u64;
        staged.retain(|(index, _)| *index < cut);
    } else {
        outcome.consumed = lines.complete_end as u64;
        outcome.next_ordinal = start_ordinal + lines.starts.len() as u64;
    }
    outcome.observations = staged.into_iter().map(|(_, o)| o).collect();
    for (_, count) in counts.into_iter().filter(|(index, _)| *index < cutoff) {
        match count {
            Count::Ignored(kind) => *outcome.ignored_records.entry(kind).or_insert(0) += 1,
            Count::Unknown(kind) => *outcome.unknown_records.entry(kind).or_insert(0) += 1,
            Count::Malformed => outcome.malformed_lines += 1,
            Count::Outside => outcome.outside_checkout += 1,
            Count::Abandoned => outcome.abandoned_tool_uses += 1,
            Count::Failed => outcome.failed_tool_uses += 1,
        }
    }
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

/// Why a text field is not retained when neither the result nor the input
/// carries it.
const FIELD_MISSING: &str = "field missing";
/// Why a command stream is not retained when the source does not carry it.
const NOT_CAPTURED: &str = "not captured";

/// One field of an editing tool, read from the tool result first (it
/// reflects what was applied) and from the input otherwise.
struct Field<T> {
    value: Option<T>,
    /// Both sides carry the field and they disagree.
    conflict: bool,
}

impl<T: PartialEq> Field<T> {
    fn reconcile(from_result: Option<T>, from_input: Option<T>) -> Self {
        let conflict = matches!((&from_result, &from_input), (Some(r), Some(i)) if r != i);
        Self {
            value: from_result.or(from_input),
            conflict,
        }
    }

    /// Present on at least one side and not contradicted by the other.
    fn settled(&self) -> Option<&T> {
        if self.conflict {
            None
        } else {
            self.value.as_ref()
        }
    }
}

fn retained_or_missing(text: Option<&str>) -> Material<String> {
    text.map_or_else(
        || Material::Unavailable {
            reason: FIELD_MISSING.to_owned(),
        },
        |t| Material::Retained(t.to_owned()),
    )
}

/// What one editing tool did to a file, apart from the path and timing.
struct EditParts {
    before: FileState,
    after: FileState,
    old_text: Material<String>,
    new_text: Material<String>,
    replace_all: bool,
}

/// An `Edit`. The after-state is computed only when the original and all
/// three fields are known and neither side contradicts the other; a missing
/// field is never read as an empty string or a default.
fn edit_parts(tool: &PendingTool, result: Option<&Value>) -> EditParts {
    let input = Some(&tool.input);
    let original = string_field(result, "originalFile");
    let old = Field::reconcile(
        string_field(result, "oldString"),
        string_field(input, "old_string"),
    );
    let new = Field::reconcile(
        string_field(result, "newString"),
        string_field(input, "new_string"),
    );
    let replace_all = Field::reconcile(
        result.and_then(|r| r.get("replaceAll")?.as_bool()),
        tool.input.get("replace_all").and_then(Value::as_bool),
    );
    let after = match (
        original,
        old.settled(),
        new.settled(),
        replace_all.settled(),
    ) {
        (Some(original), Some(old), Some(new), Some(all)) => {
            content::apply_edit(original, old, new, *all)
        }
        _ => None,
    };
    EditParts {
        before: state_of(original),
        after: state_of(after.as_deref()),
        old_text: retained_or_missing(old.value),
        new_text: retained_or_missing(new.value),
        replace_all: replace_all.value.unwrap_or(false),
    }
}

/// A `Write`. The written content comes from the result, else the input;
/// without either the after-state is unknown.
fn write_parts(tool: &PendingTool, result: Option<&Value>) -> EditParts {
    let text =
        string_field(result, "content").or_else(|| string_field(Some(&tool.input), "content"));
    let before = if string_field(result, "type") == Some("create") {
        FileState::Absent
    } else {
        state_of(string_field(result, "originalFile"))
    };
    EditParts {
        before,
        after: state_of(text),
        old_text: Material::Unavailable {
            reason: "write replaces the whole file".to_owned(),
        },
        new_text: retained_or_missing(text),
        replace_all: false,
    }
}

/// A `MultiEdit`. Without any edits, or with an edit missing its old or new
/// text, the after-state is unknown rather than a no-op or a deletion.
fn multi_edit_parts(tool: &PendingTool, result: Option<&Value>) -> EditParts {
    let original = string_field(result, "originalFile");
    let edits: Option<Vec<(String, String, bool)>> = tool
        .input
        .get("edits")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .and_then(|items| {
            items
                .iter()
                .map(|e| {
                    Some((
                        string_field(Some(e), "old_string")?.to_owned(),
                        string_field(Some(e), "new_string")?.to_owned(),
                        e.get("replace_all")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    ))
                })
                .collect()
        });
    let after = original
        .zip(edits.as_deref())
        .and_then(|(o, edits)| content::apply_multi_edit(o, edits));
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

/// Where a resolved tool use records its counts.
struct Tally<'a> {
    index: usize,
    counts: &'a mut Vec<(usize, Count)>,
}

/// The result side of a resolved tool use.
struct Resolution<'a> {
    /// The record's `toolUseResult`, when it can belong to this tool use.
    structured: Option<&'a Value>,
    /// The `tool_result` content block.
    block: &'a Value,
    /// Whether the harness flagged the result as an error.
    is_error: bool,
    /// Time of the record carrying the result.
    ended: &'a Timestamp,
}

fn build_edit(
    tool: &PendingTool,
    session: &SessionId,
    resolution: &Resolution,
    tally: &mut Tally,
) -> Option<Observation> {
    if resolution.is_error {
        // The harness says the tool failed. Whatever the input asked for did
        // not necessarily reach the disk, so there is no edit and no hash to
        // compute.
        tally.counts.push((tally.index, Count::Failed));
        return None;
    }
    let result = resolution.structured;
    let Some(path) = relative_path(tool, result) else {
        tally.counts.push((tally.index, Count::Outside));
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
        time: resolution.ended.clone(),
        source_ordinal: tool.ordinal,
    }))
}

/// Text of a `tool_result` block's content: the string itself, or its text
/// blocks joined by newlines. `None` when the block carries no text.
fn block_text(block: &Value) -> Option<String> {
    match block.get("content")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let texts: Vec<&str> = items
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect();
            (!texts.is_empty()).then(|| texts.join("\n"))
        }
        _ => None,
    }
}

fn stream_tail(text: Option<&str>) -> Material<String> {
    text.map_or_else(
        || Material::Unavailable {
            reason: NOT_CAPTURED.to_owned(),
        },
        |t| Material::Retained(tail_chars(t, OUTPUT_TAIL_CHARS)),
    )
}

fn build_command(tool: &PendingTool, session: &SessionId, resolution: &Resolution) -> Observation {
    let result = resolution.structured;
    let cmd = string_field(Some(&tool.input), "command")
        .unwrap_or_default()
        .to_owned();
    let category = classify_command(&cmd);
    // Without structured streams, the tool result's own content is the
    // command's output as the harness showed it to the agent.
    let stdout = string_field(result, "stdout")
        .map(str::to_owned)
        .or_else(|| block_text(resolution.block));
    let stderr = string_field(result, "stderr");
    let interrupted = result
        .and_then(|r| r.get("interrupted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // A reported error is failure evidence and outranks any success line in
    // the output, for example a passing binary before a compilation error.
    let outcome_value = if category != CommandCategory::TestRun || interrupted {
        None
    } else if resolution.is_error {
        Some(Outcome::Failed)
    } else {
        test_outcome(
            stdout.as_deref().unwrap_or_default(),
            stderr.unwrap_or_default(),
        )
    };
    Observation::Command(Command {
        id: EventId::derive("command", session, &tool.id),
        turn: tool.turn.clone(),
        session: session.clone(),
        cmd,
        exit: None,
        stdout_tail: stream_tail(stdout.as_deref()),
        stderr_tail: stream_tail(stderr),
        category,
        reported_error: resolution.is_error,
        outcome: outcome_value,
        started: tool.time.clone(),
        ended: Some(resolution.ended.clone()),
        checkout: tool.cwd.clone(),
        source_ordinal: tool.ordinal,
    })
}

fn build_resolved(
    tool: &PendingTool,
    session: &SessionId,
    resolution: &Resolution,
    tally: &mut Tally,
) -> Option<Observation> {
    match tool.name.as_str() {
        "Edit" | "Write" | "MultiEdit" => build_edit(tool, session, resolution, tally),
        "Bash" => Some(build_command(tool, session, resolution)),
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
/// failed run. Every nextest summary line is inspected, so a failing suite
/// after a passing one is a failed run too. `None` means no verdict could be
/// read, not success.
#[must_use]
pub fn test_outcome(stdout: &str, stderr: &str) -> Option<Outcome> {
    let text = format!("{stdout}\n{stderr}");
    let nextest_summaries: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("tests run:") || l.contains("test run:"))
        .collect();
    let failed = text.contains("test result: FAILED")
        || nextest_summaries.iter().any(|l| summary_has_failures(l))
        || text.contains("FAILED");
    if failed {
        return Some(Outcome::Failed);
    }
    if text.contains("test result: ok") || !nextest_summaries.is_empty() {
        return Some(Outcome::Passed);
    }
    None
}

/// Whether a nextest summary line reports a non-zero failed count, such as
/// `2 passed, 1 failed`. `0 failed` is not a failure.
fn summary_has_failures(line: &str) -> bool {
    let tokens: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect();
    tokens
        .windows(2)
        .any(|w| w[1] == "failed" && w[0].parse::<u64>().is_ok_and(|n| n > 0))
}
