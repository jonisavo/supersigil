//! Parses Claude Code JSONL records into turns, edits, and commands.
//!
//! Each line contains a record with a `type`. The parser pairs tool calls in
//! `assistant` records with results in later `user` records. Consecutive
//! assistant records may be blocks of the same message, so they do not end
//! pending tool calls.
//!
//! A new human or delegation turn marks pending calls as abandoned. Calls
//! still awaiting results at the end of the input are left for the next parse.
//! Unknown record types and malformed lines are counted without stopping the parse.

pub mod content;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use supersigil_record::observations::{
    CaptureCounts, Command, CommandCategory, Edit, FileState, Hunk, Material, Observation, Outcome,
    Role, Turn,
};
use supersigil_record::{ContentId, EventId, SessionId, Timestamp, TurnId};

/// Maximum Unicode characters retained from the start of agent and delegation text.
const AGENT_EXCERPT_CHARS: usize = 240;
/// Maximum Unicode characters retained from the end of each command output stream.
const OUTPUT_TAIL_CHARS: usize = 2000;
/// Recognized record types that do not produce turn, edit, or command observations.
const IGNORED_TYPES: &[&str] = &[
    "system",
    "attachment",
    "summary",
    "file-history-snapshot",
    "file-history-delta",
    "last-prompt",
    "mode",
    "permission-mode",
    "ai-title",
    "atis-latch",
    "bridge-session",
    "cost-state",
    "pr-link",
    "queue-operation",
    "relocated",
    "worktree-state",
];

/// Captured events, session metadata, and the next position to read in a transcript.
///
/// Session metadata comes only from records before [`Self::consumed`]. Records
/// at or after that byte offset will be read again and may still change.
#[derive(Debug, Default)]
pub struct ParseOutcome {
    /// Turns, edits, and commands produced from the requested line position
    /// up to `next_ordinal`, excluding that final position.
    pub observations: Vec<Observation>,
    /// Byte offset at which the next parse should resume, measured from the file start.
    ///
    /// Stops before the earliest tool call still awaiting a result. Moves
    /// further back if needed to keep every completed call and its result
    /// together on the same side of that boundary.
    ///
    /// If capture problems were counted but no session is known, stays at
    /// the requested starting position. The next parse retries those lines
    /// so the counts can be saved once a session ID is available.
    pub consumed: u64,
    /// Zero-based line position corresponding to `consumed`.
    pub next_ordinal: u64,
    /// First non-empty session ID found before `consumed`, from any record type.
    pub session: Option<SessionId>,
    /// First working directory supplied by a record of this session before `consumed`.
    pub checkout: Option<PathBuf>,
    /// First branch name supplied by a record of this session before `consumed`.
    pub branch: Option<String>,
    /// First timestamp supplied by a record of this session before `consumed`.
    pub first_time: Option<Timestamp>,
    /// Whether the record that first identified the session was marked as a
    /// subagent record. `false` when no session is known.
    pub sidechain: bool,
    /// Number of intentionally skipped records for each recognized type.
    pub ignored_records: BTreeMap<String, u64>,
    /// Whether the input ends with a line that has no newline and was left unparsed.
    pub trailing_partial: bool,
    /// Capture problems in the returned range, such as malformed records or failed edits.
    /// When no session is known, includes counts for lines held back for retry
    /// even though `consumed` has not advanced.
    pub counts: CaptureCounts,
}

impl ParseOutcome {
    fn add(&mut self, count: Count) {
        match count {
            Count::Ignored(kind) => *self.ignored_records.entry(kind).or_insert(0) += 1,
            Count::Unknown(kind) => *self.counts.unknown_records.entry(kind).or_insert(0) += 1,
            Count::Malformed => self.counts.malformed_lines += 1,
            Count::Outside => self.counts.outside_checkout += 1,
            Count::Abandoned => self.counts.abandoned_tool_uses += 1,
            Count::Failed => self.counts.failed_tool_uses += 1,
            Count::Conflicting => self.counts.conflicting_tool_results += 1,
            Count::Unmatched => self.counts.unmatched_tool_results += 1,
            Count::SessionMismatch => self.counts.session_mismatch += 1,
            Count::Unnamed => self.counts.unnamed_tool_uses += 1,
            Count::Unsupported => self.counts.unsupported_tool_uses += 1,
        }
    }
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
    time: Timestamp,
    cwd: PathBuf,
    /// Subagent ID from the record that issued the tool call, if available.
    agent_id: Option<String>,
    record_index: usize,
}

/// One skipped record or tool call to add to [`ParseOutcome`]'s counts.
#[derive(Debug)]
enum Count {
    Ignored(String),
    Unknown(String),
    Malformed,
    Outside,
    Abandoned,
    Failed,
    Conflicting,
    Unmatched,
    /// A turn whose session ID differs from the transcript's session.
    SessionMismatch,
    /// A tool call with a missing or empty ID.
    Unnamed,
    /// A completed call to an unsupported editing tool.
    Unsupported,
}

/// Observation or count produced by one transcript line.
/// Stored with its line index so results beyond the resume position can be discarded.
#[derive(Debug)]
enum Effect {
    Observation(Box<Observation>),
    Count(Count),
}

/// Line indexes where session metadata was first found.
/// Used to discard metadata from records that the next parse will read again.
#[derive(Debug, Default)]
struct LearnedAt {
    session: Option<usize>,
    checkout: Option<usize>,
    branch: Option<usize>,
    first_time: Option<usize>,
}

impl LearnedAt {
    /// Clears metadata read at or after `cutoff`.
    /// Clearing the session also clears its checkout, branch, time, and subagent flag.
    fn truncate(&self, outcome: &mut ParseOutcome, cutoff: usize) {
        let beyond = |at: Option<usize>| at.is_some_and(|at| at >= cutoff);
        if beyond(self.session) {
            outcome.session = None;
            outcome.sidechain = false;
            outcome.checkout = None;
            outcome.branch = None;
            outcome.first_time = None;
            return;
        }
        if beyond(self.checkout) {
            outcome.checkout = None;
        }
        if beyond(self.branch) {
            outcome.branch = None;
        }
        if beyond(self.first_time) {
            outcome.first_time = None;
        }
    }
}

/// Parser state accumulated while reading records in transcript order.
struct Walk {
    outcome: ParseOutcome,
    learned_at: LearnedAt,
    effects: Vec<(usize, Effect)>,
    pending: Vec<PendingTool>,
    /// Pairs of line indexes for completed tool calls and their results.
    resolved: Vec<(usize, usize)>,
}

impl Walk {
    fn count(&mut self, index: usize, count: Count) {
        self.effects.push((index, Effect::Count(count)));
    }

    fn observe(&mut self, index: usize, observation: Observation) {
        self.effects
            .push((index, Effect::Observation(Box::new(observation))));
    }

    /// Reads session metadata before filtering by record type.
    ///
    /// The first non-empty `sessionId` identifies the session and supplies its
    /// subagent flag. For that session, keeps the first checkout, branch, and
    /// timestamp found. Records without that session ID supply no metadata.
    fn learn_session(&mut self, index: usize, raw: &RawRecord) {
        let id = raw.session_id.as_deref().filter(|id| !id.is_empty());
        let Some(id) = id else {
            return;
        };
        match &self.outcome.session {
            None => {
                self.outcome.session = Some(SessionId::new(id));
                self.outcome.sidechain = raw.is_sidechain;
                self.learned_at.session = Some(index);
            }
            Some(known) if known.as_str() == id => {}
            Some(_) => return,
        }
        if self.outcome.checkout.is_none() && raw.cwd.is_some() {
            self.outcome.checkout = raw.cwd.as_ref().map(PathBuf::from);
            self.learned_at.checkout = Some(index);
        }
        if self.outcome.branch.is_none() && raw.git_branch.is_some() {
            self.outcome.branch.clone_from(&raw.git_branch);
            self.learned_at.branch = Some(index);
        }
        if self.outcome.first_time.is_none() && raw.timestamp.is_some() {
            self.outcome.first_time = raw.timestamp.as_ref().map(|t| Timestamp::new(t.clone()));
            self.learned_at.first_time = Some(index);
        }
    }

    /// Reads session metadata, records the turn, and matches tool calls with results.
    ///
    /// Counts and skips unsupported record types and turns from another session.
    /// Human and delegation turns abandon pending calls after their own tool
    /// results are matched. Assistant records leave pending calls open.
    fn record(&mut self, index: usize, mut raw: RawRecord) {
        self.learn_session(index, &raw);
        match raw.kind.as_str() {
            "user" | "assistant" => {}
            other if IGNORED_TYPES.contains(&other) => {
                self.count(index, Count::Ignored(other.to_owned()));
                return;
            }
            other => {
                self.count(index, Count::Unknown(other.to_owned()));
                return;
            }
        }

        let Some(session_id) = self.outcome.session.clone() else {
            self.count(index, Count::Unknown("no-session".to_owned()));
            return;
        };
        if raw
            .session_id
            .as_deref()
            .is_some_and(|id| !id.is_empty() && id != session_id.as_str())
        {
            self.count(index, Count::SessionMismatch);
            return;
        }
        let Some(uuid) = &raw.uuid else {
            self.count(index, Count::Unknown(format!("{}-without-uuid", raw.kind)));
            // No turn without an id, but the tool results it carries still
            // pair: left unresolved, their tool uses would hold the cursor
            // at their issuing records forever.
            if raw.kind == "user" {
                self.resolve_results(index, &raw, &session_id);
            }
            return;
        };
        let turn_id = TurnId::new(uuid.clone());
        let time = Timestamp::new(raw.timestamp.clone().unwrap_or_default());
        let ordinal = index as u64;
        let content = raw.message.as_ref().map(|m| &m.content);

        let role = classify_role(&raw, content);
        self.observe(
            index,
            Observation::Turn(Turn {
                id: turn_id.clone(),
                session: session_id.clone(),
                parent: raw.parent_uuid.clone().map(TurnId::new),
                role,
                time: time.clone(),
                sidechain: raw.is_sidechain,
                agent_id: raw.agent_id.clone(),
                excerpt: excerpt_for(role, content),
                source_ordinal: ordinal,
                transcript: None,
            }),
        );

        if raw.kind == "assistant" {
            self.queue_tool_uses(index, &mut raw, &turn_id, &time);
        } else {
            // A typed message, from a person or a delegating parent agent,
            // abandons what is still unresolved, but only after its own tool
            // results are matched: a typed message can share a record with
            // the results it follows. Tool results and meta records never
            // abandon, since parallel tool calls get one user record per
            // result.
            self.resolve_results(index, &raw, &session_id);
            if matches!(role, Role::Human | Role::Delegation) {
                self.abandon_pending(&session_id);
            }
        }
    }

    /// Marks pending tool calls as abandoned when a human or delegation turn arrives.
    /// Records pending shell commands with unavailable results, then clears the queue.
    fn abandon_pending(&mut self, session: &SessionId) {
        for tool in std::mem::take(&mut self.pending) {
            self.count(tool.record_index, Count::Abandoned);
            if let Some(observation) = build_abandoned(&tool, session) {
                self.observe(tool.record_index, observation);
            }
        }
    }

    /// Moves tool-call inputs from `raw` into the pending queue.
    /// Counts calls with missing or empty IDs instead of queuing them.
    fn queue_tool_uses(
        &mut self,
        index: usize,
        raw: &mut RawRecord,
        turn: &TurnId,
        time: &Timestamp,
    ) {
        // A record without `cwd` ran where the transcript already was.
        let cwd = raw
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| self.outcome.checkout.clone())
            .unwrap_or_default();
        let Some(items) = raw.message.as_mut().and_then(|m| m.content.as_array_mut()) else {
            return;
        };
        for block in items.iter_mut().filter(|b| is_block(b, "tool_use")) {
            let Some(id) = string_field(Some(block), "id")
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
            else {
                self.count(index, Count::Unnamed);
                continue;
            };
            let name = string_field(Some(block), "name")
                .unwrap_or_default()
                .to_owned();
            self.pending.push(PendingTool {
                id,
                name,
                input: block.get_mut("input").map(Value::take).unwrap_or_default(),
                turn: turn.clone(),
                time: time.clone(),
                cwd: cwd.clone(),
                agent_id: raw.agent_id.clone(),
                record_index: index,
            });
        }
    }

    /// Matches tool results to pending calls by ID.
    /// Uses the result record's timestamp as the completion time when available.
    fn resolve_results(&mut self, index: usize, raw: &RawRecord, session: &SessionId) {
        let ended = raw.timestamp.as_deref().map(Timestamp::new);
        let content = raw.message.as_ref().map(|m| &m.content);
        let results = || blocks(content).filter(|b| is_block(b, "tool_result"));
        let structured = if results().count() == 1 {
            raw.tool_use_result.as_ref()
        } else {
            None
        };
        for result in results() {
            let id = result
                .get("tool_use_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty());
            let is_error = result
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            // A result for no awaited tool use (its issuing record is
            // missing or malformed, a typed message abandoned it, or it
            // names no tool use at all) cannot be paired, so what it
            // reported is dropped and counted.
            let Some(position) = id.and_then(|id| self.pending.iter().position(|p| p.id == id))
            else {
                self.count(index, Count::Unmatched);
                continue;
            };
            let tool = self.pending.remove(position);
            self.resolved.push((tool.record_index, index));
            let resolution = Resolution {
                structured,
                block: result,
                is_error,
                ended: ended.as_ref(),
            };
            match build_resolved(&tool, session, &resolution) {
                Some(Ok(observation)) => self.observe(index, observation),
                Some(Err(count)) => self.count(index, count),
                None => {}
            }
        }
    }
}

/// Parses a full transcript and returns new events starting at `from_ordinal`.
///
/// `from_ordinal` is a zero-based line position, normally the previous result's
/// [`ParseOutcome::next_ordinal`]. Earlier lines are read for session metadata
/// and tool-call context but produce no returned observations or counts.
/// Lines without a final newline and calls still awaiting results are left
/// for a later parse. [`ParseOutcome::consumed`] identifies where to resume.
///
/// Malformed records and unsupported tool calls are counted in the result.
/// This function reads only `bytes`; it does not access files or run commands.
#[must_use]
pub fn parse_transcript(bytes: &[u8], from_ordinal: u64) -> ParseOutcome {
    let mut walk = Walk {
        outcome: ParseOutcome::default(),
        learned_at: LearnedAt::default(),
        effects: Vec::new(),
        pending: Vec::new(),
        resolved: Vec::new(),
    };
    // One complete line at a time: parse it, walk it, drop it. A line that
    // is not a JSON object is counted; a trailing line without a newline is
    // flagged and left out.
    let mut starts = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let Some(len) = bytes[cursor..].iter().position(|b| *b == b'\n') else {
            walk.outcome.trailing_partial = true;
            break;
        };
        let line = &bytes[cursor..cursor + len];
        let index = starts.len();
        starts.push(cursor);
        cursor += len + 1;
        match serde_json::from_slice::<RawRecord>(line) {
            Ok(raw) => walk.record(index, raw),
            Err(_) => walk.count(index, Count::Malformed),
        }
    }
    let complete_end = cursor;

    let Walk {
        mut outcome,
        mut effects,
        pending,
        resolved,
        learned_at,
    } = walk;
    let from = usize::try_from(from_ordinal)
        .unwrap_or(usize::MAX)
        .min(starts.len());
    let mut cutoff = consumption_cutoff(starts.len(), &pending, &resolved).max(from);
    learned_at.truncate(&mut outcome, cutoff);
    effects.retain(|(index, _)| (from..cutoff).contains(index));
    for (_, effect) in effects {
        match effect {
            Effect::Observation(observation) => outcome.observations.push(*observation),
            Effect::Count(count) => outcome.add(count),
        }
    }
    // Counts without a session cannot be recorded; hold the lines back
    // until a record names the session.
    if outcome.session.is_none() && !outcome.counts.is_empty() {
        cutoff = from;
    }
    outcome.consumed = starts.get(cutoff).copied().unwrap_or(complete_end) as u64;
    outcome.next_ordinal = cutoff as u64;
    outcome
}

/// Finds the first line to leave for the next parse.
///
/// Starts at the earliest pending tool call. Moves backward across completed
/// call/result pairs until none crosses the boundary. This lets the next
/// parse handle each call together with its result.
fn consumption_cutoff(lines: usize, pending: &[PendingTool], resolved: &[(usize, usize)]) -> usize {
    let mut cutoff = pending
        .iter()
        .map(|p| p.record_index)
        .min()
        .unwrap_or(lines);
    while let Some(issued) = resolved
        .iter()
        .filter(|(issued, result)| *issued < cutoff && *result >= cutoff)
        .map(|(issued, _)| *issued)
        .min()
    {
        cutoff = issued;
    }
    cutoff
}

/// Checks whether `block` has the content type `ty`.
fn is_block(block: &Value, ty: &str) -> bool {
    block.get("type").and_then(Value::as_str) == Some(ty)
}

/// Iterates over an array of content blocks, yielding nothing for other values.
fn blocks(content: Option<&Value>) -> impl Iterator<Item = &Value> {
    content.and_then(Value::as_array).into_iter().flatten()
}
/// Classifies the record as agent text, metadata, tool output, delegation, or human text.
///
/// Assistant records are agent text. User records are metadata when marked
/// `isMeta`, or tool output when their non-empty content contains only tool
/// results. Remaining user records are delegation in subagent transcripts
/// and human text in main transcripts.
fn classify_role(raw: &RawRecord, content: Option<&Value>) -> Role {
    if raw.kind == "assistant" {
        return Role::Agent;
    }
    if raw.is_meta {
        return Role::Meta;
    }
    let mut items = blocks(content).peekable();
    if items.peek().is_some() && items.all(|b| is_block(b, "tool_result")) {
        return Role::Tool;
    }
    if raw.is_sidechain {
        return Role::Delegation;
    }
    Role::Human
}

/// Extracts message text, returning an empty string when there is none.
fn text_of(content: Option<&Value>) -> String {
    content_text(content).unwrap_or_default()
}

/// Extracts a content string or joins an array's text blocks with newlines.
/// Returns `None` if no text is present and `Some("")` for explicitly empty text.
fn content_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let texts: Vec<&str> = items
                .iter()
                .filter(|b| is_block(b, "text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect();
            (!texts.is_empty()).then(|| texts.join("\n"))
        }
        _ => None,
    }
}

fn excerpt_for(role: Role, content: Option<&Value>) -> Material<String> {
    match role {
        Role::Human => Material::Retained(text_of(content)),
        Role::Agent | Role::Delegation => {
            Material::Retained(truncate_chars(&text_of(content), AGENT_EXCERPT_CHARS))
        }
        Role::Tool => Material::unavailable("tool result"),
        Role::Meta => Material::unavailable("harness metadata"),
        Role::Summary => Material::unavailable("compaction summary"),
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

/// Reads the edited path from the input's `file_path` and the result's `filePath`.
///
/// Converts each supplied path to a checkout-relative path with [`contained`].
/// Uses whichever is present, requiring equal results when both are supplied.
///
/// # Errors
///
/// Returns [`Count::Conflicting`] if the converted paths differ, including when
/// only one is inside the checkout. Returns [`Count::Outside`] if neither
/// supplies a path inside the checkout.
fn relative_path(tool: &PendingTool, result: Option<&Value>) -> Result<PathBuf, Count> {
    let from_input = string_field(Some(&tool.input), "file_path").map(|p| contained(&tool.cwd, p));
    let from_result = string_field(result, "filePath").map(|p| contained(&tool.cwd, p));
    match (from_input, from_result) {
        (Some(input), Some(result)) if input != result => Err(Count::Conflicting),
        (Some(path), _) | (None, Some(path)) => path.ok_or(Count::Outside),
        (None, None) => Err(Count::Outside),
    }
}

/// Returns the part of `path` below `checkout`, without accessing the file system.
///
/// Requires a non-empty checkout prefix and a non-empty remainder containing
/// only normal path components. For example, `/work/repo/../secret` is rejected
/// under `/work/repo`. Returns `None` if these checks fail. Does not resolve symlinks.
fn contained(checkout: &Path, path: &str) -> Option<PathBuf> {
    if checkout.as_os_str().is_empty() {
        return None;
    }
    let relative = Path::new(path).strip_prefix(checkout).ok()?;
    let normal = relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    (normal && !relative.as_os_str().is_empty()).then(|| relative.to_path_buf())
}

fn hunks_of(result: Option<&Value>) -> Material<Vec<Hunk>> {
    match result.and_then(|r| r.get("structuredPatch")) {
        Some(value) => match Vec::<RawHunk>::deserialize(value) {
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
            Err(_) => Material::unavailable("unreadable structured patch"),
        },
        None => Material::unavailable("no structured patch"),
    }
}

fn state_of(text: Option<&str>) -> FileState {
    text.map_or_else(FileState::unknown, |t| {
        FileState::known(ContentId::of(t.as_bytes()))
    })
}

/// Reason recorded when text is missing from both the input and result.
const FIELD_MISSING: &str = "field missing";
/// Reason recorded when a command output stream is missing.
const NOT_CAPTURED: &str = "not captured";
/// Reason recorded when a command has no result.
const NO_RESULT: &str = "no result recorded";

/// Editing-tool field taken from the result, falling back to the input.
/// Tracks disagreement when both provide values.
struct Field<T> {
    value: Option<T>,
    /// Whether the input and result both supplied values that differ.
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

    /// Returns the value if present and the input and result do not disagree.
    fn settled(&self) -> Option<&T> {
        if self.conflict {
            None
        } else {
            self.value.as_ref()
        }
    }
}

/// Stores supplied text as retained material, or records `reason` when it is absent.
fn retained_or(text: Option<&str>, reason: &str) -> Material<String> {
    text.map_or_else(
        || Material::unavailable(reason),
        |t| Material::Retained(t.to_owned()),
    )
}

/// File states and replacement text extracted from an editing tool call.
struct EditParts {
    before: FileState,
    after: FileState,
    old_text: Material<String>,
    new_text: Material<String>,
    replace_all: bool,
}

/// Reconstructs the file states and replacement text for an `Edit` call.
///
/// Computes after-content only when the original text, old text, new text,
/// and `replace_all` are available and the input and result do not disagree.
/// If any required value is missing or replacement fails, the after-state is unknown.
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
        old_text: retained_or(old.value, FIELD_MISSING),
        new_text: retained_or(new.value, FIELD_MISSING),
        replace_all: replace_all.value.unwrap_or(false),
    }
}

/// Reconstructs file states for a `Write` call.
///
/// Uses content from the result, falling back to the input. Missing or
/// conflicting content leaves the after-state unknown. Conflicting result
/// text is still retained for inspection.
fn write_parts(tool: &PendingTool, result: Option<&Value>) -> EditParts {
    let content = Field::reconcile(
        string_field(result, "content"),
        string_field(Some(&tool.input), "content"),
    );
    let before = if string_field(result, "type") == Some("create") {
        FileState::Absent
    } else {
        state_of(string_field(result, "originalFile"))
    };
    EditParts {
        before,
        after: state_of(content.settled().copied()),
        old_text: Material::unavailable("write replaces the whole file"),
        new_text: retained_or(content.value, FIELD_MISSING),
        replace_all: false,
    }
}

/// Reconstructs file states by applying a `MultiEdit` call's replacements in order.
/// Missing original text, empty or incomplete edits, or a failed replacement
/// leave the after-state unknown.
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
        old_text: Material::unavailable("multi-edit"),
        new_text: Material::unavailable("multi-edit"),
        replace_all: false,
    }
}

/// Tool-result data matched to a pending call.
struct Resolution<'a> {
    /// Structured `toolUseResult`, when it can be assigned to this call.
    structured: Option<&'a Value>,
    /// The `tool_result` content block.
    block: &'a Value,
    /// Whether the source marked the result as an error.
    is_error: bool,
    /// Timestamp of the record carrying the result, if available.
    ended: Option<&'a Timestamp>,
}

/// Builds an edit observation from a supported editing call and its result.
/// Uses the result timestamp, falling back to the call timestamp when absent.
///
/// # Errors
///
/// Returns [`Count::Failed`] if the tool reported an error, or the path error
/// from [`relative_path`].
fn build_edit(
    tool: &PendingTool,
    session: &SessionId,
    resolution: &Resolution,
) -> Result<Observation, Count> {
    if resolution.is_error {
        // The harness says the tool failed. Whatever the input asked for did
        // not necessarily reach the disk, so there is no edit and no hash to
        // compute.
        return Err(Count::Failed);
    }
    let result = resolution.structured;
    let path = relative_path(tool, result)?;
    let parts = match tool.name.as_str() {
        "Edit" => edit_parts(tool, result),
        "Write" => write_parts(tool, result),
        _ => multi_edit_parts(tool, result),
    };
    Ok(Observation::Edit(Edit {
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
        time: resolution
            .ended
            .cloned()
            .unwrap_or_else(|| tool.time.clone()),
        source_ordinal: tool.record_index as u64,
        agent_id: tool.agent_id.clone(),
        transcript: None,
    }))
}

fn stream_tail(text: Option<&str>) -> Material<String> {
    text.map_or_else(
        || Material::unavailable(NOT_CAPTURED),
        |t| Material::Retained(tail_chars(t, OUTPUT_TAIL_CHARS)),
    )
}

/// Builds a command from its tool call, before any result is known.
/// Output is unavailable, `reported_error` is false, and outcome and end time are absent.
fn command_base(tool: &PendingTool, session: &SessionId) -> Command {
    let cmd = string_field(Some(&tool.input), "command")
        .unwrap_or_default()
        .to_owned();
    Command {
        id: EventId::derive("command", session, &tool.id),
        turn: tool.turn.clone(),
        session: session.clone(),
        category: classify_command(&cmd),
        cmd,
        exit: None,
        stdout_tail: Material::unavailable(NO_RESULT),
        stderr_tail: Material::unavailable(NO_RESULT),
        reported_error: false,
        outcome: None,
        started: tool.time.clone(),
        ended: None,
        checkout: tool.cwd.clone(),
        source_ordinal: tool.record_index as u64,
        agent_id: tool.agent_id.clone(),
        transcript: None,
    }
}

fn build_command(tool: &PendingTool, session: &SessionId, resolution: &Resolution) -> Observation {
    let base = command_base(tool, session);
    let result = resolution.structured;
    // Without structured streams, the tool result's own content is the
    // command's output as the harness showed it to the agent.
    let stdout = string_field(result, "stdout")
        .map(str::to_owned)
        .or_else(|| content_text(resolution.block.get("content")));
    let stderr = string_field(result, "stderr");
    let interrupted = result
        .and_then(|r| r.get("interrupted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // A reported error is failure evidence and outranks any success line in
    // the output, for example a passing binary before a compilation error.
    let outcome = if base.category != CommandCategory::TestRun || interrupted {
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
        stdout_tail: stream_tail(stdout.as_deref()),
        stderr_tail: stream_tail(stderr),
        reported_error: resolution.is_error,
        outcome,
        ended: resolution.ended.cloned(),
        ..base
    })
}

/// Builds an observation for `Edit`, `Write`, `MultiEdit`, or `Bash`.
/// Returns `None` for other tools except `NotebookEdit`, which is counted as unsupported.
///
/// # Errors
///
/// Returns `Some(Err(...))` for a `NotebookEdit` call or an editing call rejected
/// by [`build_edit`].
fn build_resolved(
    tool: &PendingTool,
    session: &SessionId,
    resolution: &Resolution,
) -> Option<Result<Observation, Count>> {
    match tool.name.as_str() {
        "Edit" | "Write" | "MultiEdit" => Some(build_edit(tool, session, resolution)),
        "Bash" => Some(Ok(build_command(tool, session, resolution))),
        "NotebookEdit" => Some(Err(Count::Unsupported)),
        _ => None,
    }
}

/// Records an abandoned `Bash` call with no result. Returns `None` for other tools.
fn build_abandoned(tool: &PendingTool, session: &SessionId) -> Option<Observation> {
    (tool.name == "Bash").then(|| Observation::Command(command_base(tool, session)))
}

/// Classifies a shell command line by recognized program names and subcommands.
///
/// Splits commands at unquoted `;`, `&`, `|`, and newlines, including `&&`
/// and `||`. Redirections such as `2>&1` do not split commands. Skips comments
/// and heredoc bodies, stopping if a heredoc terminator is missing.
///
/// Before identifying the program, skips leading assignments, `sudo`, `time`,
/// `env`, and `mise exec ... --`. For example, `RUST_LOG=debug cargo test` is
/// a test run, while `echo "cargo test"` and `cat nextest-results.txt` are `Other`.
///
/// When several commands match, the result uses this priority: `TestRun`,
/// `Build`, `Git`, then `Other`. This recognizes specific command forms;
/// it does not evaluate shell syntax or execute the command.
#[must_use]
pub fn classify_command(cmd: &str) -> CommandCategory {
    simple_commands(cmd)
        .iter()
        .map(|words| category_of(program_words(words)))
        .max_by_key(|category| rank(*category))
        .unwrap_or(CommandCategory::Other)
}

/// Returns a category's priority when a line contains several commands.
const fn rank(category: CommandCategory) -> u8 {
    match category {
        CommandCategory::Other => 0,
        CommandCategory::Git => 1,
        CommandCategory::Build => 2,
        CommandCategory::TestRun => 3,
    }
}

/// Classifies words starting with a program name and followed by its arguments.
fn category_of(words: &[String]) -> CommandCategory {
    let word = |i: usize| words.get(i).map(String::as_str);
    // A program named by path, such as `/usr/bin/cargo`, is still `cargo`.
    let program = word(0).map(|p| p.rsplit('/').next().unwrap_or(p));
    match (program, word(1), word(2)) {
        (Some("cargo"), Some("test" | "nextest"), _)
        | (Some("go" | "npm" | "pnpm"), Some("test"), _)
        | (Some("pytest" | "vitest" | "jest"), _, _) => CommandCategory::TestRun,
        (Some("cargo"), Some("build" | "check" | "clippy"), _)
        | (Some("tsc"), _, _)
        | (Some("pnpm"), Some("build"), _)
        | (Some("npm"), Some("run"), Some("build")) => CommandCategory::Build,
        (Some("git" | "gh"), _, _) => CommandCategory::Git,
        _ => CommandCategory::Other,
    }
}

/// Removes leading assignments and `sudo`, `time`, `env`, or `mise exec ... --` prefixes.
fn program_words(words: &[String]) -> &[String] {
    let mut rest = words;
    loop {
        match rest.first().map(String::as_str) {
            Some("sudo" | "time" | "env") => rest = &rest[1..],
            Some(word) if is_assignment(word) => rest = &rest[1..],
            Some("mise") if rest.get(1).is_some_and(|w| w == "exec") => {
                match rest.iter().position(|w| w == "--") {
                    Some(dashes) => rest = &rest[dashes + 1..],
                    None => return rest,
                }
            }
            _ => return rest,
        }
    }
}

/// Checks for a shell variable assignment such as `RUST_LOG=debug`.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
            && !name.is_empty()
    })
}

/// Splits a command line into word lists, removing quotes and backslash escapes.
/// Uses the separators and exclusions described in [`classify_command`].
fn simple_commands(line: &str) -> Vec<Vec<String>> {
    let mut split = Splitter::default();
    let mut chars = line.char_indices().peekable();
    let mut quote: Option<char> = None;
    let mut prev: Option<char> = None;
    while let Some((i, c)) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => split.word.extend(chars.next().map(|(_, c)| c)),
            (Some(_), c) => split.word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                split.in_word = true;
            }
            (None, '\\') => {
                split.word.extend(chars.next().map(|(_, c)| c));
                split.in_word = true;
            }
            // A comment runs to the end of its line.
            (None, '#') if !split.in_word => while chars.next_if(|&(_, c)| c != '\n').is_some() {},
            (None, '<') if chars.next_if(|&(_, c)| c == '<').is_some() => {
                // `<<<` is a here-string, whose word follows as an argument;
                // `<<` opens a heredoc whose body starts on the next line.
                if chars.next_if(|&(_, c)| c == '<').is_none() {
                    split.end_word();
                    let Some(heredoc) = heredoc_delimiter(&mut chars) else {
                        // An unclosed quote: the rest is of unknown extent.
                        return split.finish();
                    };
                    split.heredocs.push(heredoc);
                }
            }
            // `2>&1`, `>&2`, and `&>file` redirect; they do not split.
            (None, '&')
                if matches!(prev, Some('>' | '<'))
                    || chars.peek().is_some_and(|&(_, c)| c == '>') =>
            {
                split.word.push(c);
                split.in_word = true;
            }
            (None, ';' | '&' | '|' | '\n') => {
                split.end_command();
                if c == '\n' && !split.heredocs.is_empty() {
                    let Some(resume) = skip_heredoc_bodies(line, i + 1, &mut split.heredocs) else {
                        // No terminator: the rest is data of unknown extent.
                        return split.finish();
                    };
                    while chars.next_if(|&(j, _)| j < resume).is_some() {}
                }
            }
            (None, c) if c.is_whitespace() => split.end_word(),
            (None, c) => {
                split.word.push(c);
                split.in_word = true;
            }
        }
        prev = Some(c);
    }
    split.finish()
}

/// Words, commands, and heredoc delimiters accumulated by the command-line parser.
#[derive(Default)]
struct Splitter {
    commands: Vec<Vec<String>>,
    words: Vec<String>,
    word: String,
    /// Whether a word has started, including an empty quoted argument.
    in_word: bool,
    /// Heredoc delimiters opened on this line, paired with whether `<<-`
    /// allows leading tabs on the terminator line.
    heredocs: Vec<(String, bool)>,
}

impl Splitter {
    /// Adds the current word to the command if a word has started.
    fn end_word(&mut self) {
        if std::mem::take(&mut self.in_word) {
            self.words.push(std::mem::take(&mut self.word));
        }
    }

    /// Ends the current command, if it has any words.
    fn end_command(&mut self) {
        self.end_word();
        if !self.words.is_empty() {
            self.commands.push(std::mem::take(&mut self.words));
        }
    }

    fn finish(mut self) -> Vec<Vec<String>> {
        self.end_command();
        self.commands
    }
}

/// Reads the delimiter after `<<` and whether `-` enables tab stripping.
///
/// Skips spaces and tabs, then reads either a quoted string or an unquoted
/// word with quotes and backslashes removed. Returns `None` for an unclosed quote.
fn heredoc_delimiter(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Option<(String, bool)> {
    let strip_tabs = chars.next_if(|&(_, c)| c == '-').is_some();
    while chars.next_if(|&(_, c)| c == ' ' || c == '\t').is_some() {}
    if let Some((_, quote)) = chars.next_if(|&(_, c)| c == '\'' || c == '"') {
        let mut delimiter = String::new();
        for (_, c) in chars.by_ref() {
            if c == quote {
                return Some((delimiter, strip_tabs));
            }
            delimiter.push(c);
        }
        return None;
    }
    let mut delimiter = String::new();
    while let Some((_, c)) = chars.next_if(|&(_, c)| {
        !c.is_whitespace() && !matches!(c, ';' | '&' | '|' | '<' | '>' | '(' | ')')
    }) {
        if !matches!(c, '\'' | '"' | '\\') {
            delimiter.push(c);
        }
    }
    Some((delimiter, strip_tabs))
}

/// Skips heredoc bodies and terminator lines, starting at byte `start`.
///
/// A terminator must equal its delimiter, ignoring leading tabs for `<<-`.
/// Returns the byte position where commands resume, or `None` if a terminator is missing.
fn skip_heredoc_bodies(
    line: &str,
    start: usize,
    heredocs: &mut Vec<(String, bool)>,
) -> Option<usize> {
    let mut pos = start;
    for (delimiter, strip_tabs) in heredocs.drain(..) {
        loop {
            if pos >= line.len() {
                return None;
            }
            let end = line[pos..].find('\n').map_or(line.len(), |n| pos + n);
            let mut text = &line[pos..end];
            if strip_tabs {
                text = text.trim_start_matches('\t');
            }
            pos = end + 1;
            if text == delimiter {
                break;
            }
        }
    }
    Some(pos)
}

/// Infers pass or fail from `cargo test` or nextest markers in both output streams.
///
/// Returns `Failed` if either stream contains `FAILED` or a nextest summary
/// reports a non-zero failed count. Otherwise returns `Passed` for
/// `test result: ok` or a nextest summary. Failure takes priority even when
/// other binaries passed. Returns `None` when no result marker is recognized.
#[must_use]
pub fn test_outcome(stdout: &str, stderr: &str) -> Option<Outcome> {
    let text = format!("{stdout}\n{stderr}");
    let nextest_summaries: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("tests run:") || l.contains("test run:"))
        .collect();
    let failed =
        nextest_summaries.iter().any(|l| summary_has_failures(l)) || text.contains("FAILED");
    if failed {
        return Some(Outcome::Failed);
    }
    if text.contains("test result: ok") || !nextest_summaries.is_empty() {
        return Some(Outcome::Passed);
    }
    None
}

/// Checks for a positive number followed by `failed`, such as `1 failed`.
/// A `0 failed` count returns `false`.
fn summary_has_failures(line: &str) -> bool {
    let tokens: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect();
    tokens
        .windows(2)
        .any(|w| w[1] == "failed" && w[0].parse::<u64>().is_ok_and(|n| n > 0))
}
