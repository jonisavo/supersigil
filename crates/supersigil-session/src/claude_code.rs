//! Claude Code JSONL transcript parser.
//!
//! Every line is one record with a `type`. `user` and `assistant` records
//! are turns; tool uses inside assistant records pair with tool results in
//! later user records. Claude Code writes each content block of one
//! assistant message as its own record, so consecutive assistant records do
//! not mean the agent moved past a tool use. A tool use still unresolved
//! when a typed message (a person's, or a parent agent's delegation)
//! arrives is abandoned; one still unresolved at the
//! end of the input is left for the next parse. The parser is tolerant:
//! unknown record types and malformed lines are counted, never fatal.

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

/// Characters kept from an agent turn's text.
const AGENT_EXCERPT_CHARS: usize = 240;
/// Characters kept from the end of a command's output streams.
const OUTPUT_TAIL_CHARS: usize = 2000;
/// Record types the parser knows and deliberately does not turn into
/// observations. From `ai-title` on, they are Claude Code UI state, which
/// carries no turn, edit, or command.
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

/// Result of parsing a byte range of a transcript.
#[derive(Debug, Default)]
pub struct ParseOutcome {
    /// Observations for every record below `consumed`.
    pub observations: Vec<Observation>,
    /// Byte offset up to which records were fully turned into observations.
    ///
    /// A tool use and its result are consumed together: consumption stops at
    /// the issuing record of a tool use still waiting for its result, and
    /// further back at the issuing record of any tool use whose result lies
    /// beyond that point.
    ///
    /// Zero when no session is known and `counts` is not empty: what was
    /// lost cannot be recorded without a session, so nothing is consumed and
    /// the same lines are read again on the next call, until a record names
    /// the session and the counts can be attached to it.
    pub consumed: u64,
    /// Ordinal for the first record at or after `consumed`.
    pub next_ordinal: u64,
    /// The session given to [`parse_transcript_with_session`], else the
    /// session id from the first record of any type that carried a
    /// non-empty one. Like `checkout`, `branch`, `first_time`, and
    /// `sidechain`, it is learned only from records below `consumed`: a
    /// record the next parse reads again may still change.
    pub session: Option<SessionId>,
    /// Working directory from the first record of the session that carried
    /// one, below `consumed`.
    pub checkout: Option<PathBuf>,
    /// Branch from the first record of the session that carried one, below
    /// `consumed`.
    pub branch: Option<String>,
    /// Timestamp of the first record of the session that carried one, below
    /// `consumed`.
    pub first_time: Option<Timestamp>,
    /// Whether the record that named the session, and so began supplying
    /// `session`, `checkout`, `branch`, and `first_time`, was a sidechain
    /// (subagent) record. A subagent transcript's metadata describes the
    /// subagent's start, not the session's. False when the session was given.
    pub sidechain: bool,
    /// Known record types that were skipped, by type.
    pub ignored_records: BTreeMap<String, u64>,
    /// Whether the input ended in a line without a newline.
    pub trailing_partial: bool,
    /// What the parse could not turn into evidence below `consumed`: unknown
    /// record types, malformed lines, edits outside the checkout, abandoned
    /// tool uses, editing tool uses the harness flagged as failed, editing
    /// tool uses whose input and result name different files, tool results
    /// that match no tool use, turns of another session, tool uses without an
    /// id, and editing tools the capture does not read. When no session is
    /// known, what was
    /// seen before the cutoff is still counted, though `consumed` is zero.
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
    ordinal: u64,
    time: Timestamp,
    cwd: PathBuf,
    /// Subagent of the issuing record, when inside a subagent.
    agent_id: Option<String>,
    record_index: usize,
}

/// A per-record tally toward [`ParseOutcome`]'s counts.
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
    /// A turn whose session id is not the established session's.
    SessionMismatch,
    /// A tool use without a non-empty id.
    Unnamed,
    /// A resolved tool use of an editing tool the capture does not read.
    Unsupported,
}

/// What one line contributed, kept with the line's index so that effects of
/// lines beyond the consumption cutoff can be dropped.
#[derive(Debug)]
enum Effect {
    Observation(Box<Observation>),
    Count(Count),
}

/// Line index of the record each piece of session metadata was learned
/// from, so metadata from lines beyond the consumption cutoff can be
/// dropped.
#[derive(Debug, Default)]
struct LearnedAt {
    session: Option<usize>,
    checkout: Option<usize>,
    branch: Option<usize>,
    first_time: Option<usize>,
}

impl LearnedAt {
    /// Forgets in `outcome` whatever was learned at or beyond `cutoff`. The
    /// checkout, branch, and first time belong to the learned session, so
    /// they go with it.
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

/// Mutable state while walking the records in order.
struct Walk {
    outcome: ParseOutcome,
    /// Whether the caller gave the session, so none is learned from records.
    session_given: bool,
    learned_at: LearnedAt,
    start_ordinal: u64,
    effects: Vec<(usize, Effect)>,
    pending: Vec<PendingTool>,
    /// Line indexes of each resolved tool use's issuing record and of the
    /// record carrying its result.
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

    /// Learns the session and its metadata from a record of any type, so a
    /// transcript that opens with records the parser skips still has a
    /// session to attach their capture limitation to. The first non-empty
    /// `sessionId` names the session; the checkout, branch, and first time
    /// come from the first record of that session carrying each. An empty
    /// `sessionId` is no session id at all. A session given by the caller
    /// is fixed: only the checkout and branch are still learned, from records
    /// that carry no session id or the given one. Whether the naming record
    /// was a sidechain record is kept with the metadata.
    fn learn_session(&mut self, index: usize, raw: &RawRecord) {
        let id = raw.session_id.as_deref().filter(|id| !id.is_empty());
        if self.session_given {
            let given = self.outcome.session.as_ref().map(SessionId::as_str);
            if id.is_none_or(|id| Some(id) == given) {
                self.learn_place(index, raw);
            }
            return;
        }
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
        self.learn_place(index, raw);
        if self.outcome.first_time.is_none() && raw.timestamp.is_some() {
            self.outcome.first_time = raw.timestamp.as_ref().map(|t| Timestamp::new(t.clone()));
            self.learned_at.first_time = Some(index);
        }
    }

    /// Learns the checkout and branch from the record on line `index`, where
    /// not yet known.
    fn learn_place(&mut self, index: usize, raw: &RawRecord) {
        if self.outcome.checkout.is_none() && raw.cwd.is_some() {
            self.outcome.checkout = raw.cwd.as_ref().map(PathBuf::from);
            self.learned_at.checkout = Some(index);
        }
        if self.outcome.branch.is_none() && raw.git_branch.is_some() {
            self.outcome.branch.clone_from(&raw.git_branch);
            self.learned_at.branch = Some(index);
        }
    }

    /// Handles the record on line `index`: learns the session, counts
    /// skipped types, then stages its turn and pairs its tool uses and
    /// results. A turn naming a session other than the established one
    /// (learned or given) is counted and not staged, so one session's
    /// evidence never lands in another. Only a typed message abandons
    /// unresolved tool uses; an assistant record never does, since one
    /// message's blocks arrive as consecutive assistant records.
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
            return;
        };
        let turn_id = TurnId::new(uuid.clone());
        let time = Timestamp::new(raw.timestamp.clone().unwrap_or_default());
        let ordinal = self.start_ordinal + index as u64;
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

    /// Counts every unresolved tool use as abandoned and emits shell
    /// commands among them with an unavailable result. Called when a typed
    /// message arrives, after that record's own results are matched.
    fn abandon_pending(&mut self, session: &SessionId) {
        for tool in std::mem::take(&mut self.pending) {
            self.count(tool.record_index, Count::Abandoned);
            if let Some(observation) = build_abandoned(&tool, session) {
                self.observe(tool.record_index, observation);
            }
        }
    }

    /// Queues the record's tool uses, taking their inputs out of `raw`. A
    /// tool use without a non-empty id can never be paired with its result,
    /// so it is counted instead of queued.
    fn queue_tool_uses(
        &mut self,
        index: usize,
        raw: &mut RawRecord,
        turn: &TurnId,
        time: &Timestamp,
    ) {
        let cwd = PathBuf::from(raw.cwd.as_deref().unwrap_or_default());
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
                ordinal: self.start_ordinal + index as u64,
                time: time.clone(),
                cwd: cwd.clone(),
                agent_id: raw.agent_id.clone(),
                record_index: index,
            });
        }
    }

    /// Pairs the record's tool results with awaited tool uses. The record's
    /// own timestamp, when it has one, is when the results were recorded.
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
/// chunks yields the same observations as parsing it whole. With a session
/// given, the checkout and branch are still read from records of that
/// session, so every chunk can be checked against the checkout; the first
/// time is only read when no session is given.
#[must_use]
pub fn parse_transcript_with_session(
    bytes: &[u8],
    start_ordinal: u64,
    session: Option<&SessionId>,
) -> ParseOutcome {
    let mut walk = Walk {
        outcome: ParseOutcome {
            session: session.cloned(),
            ..ParseOutcome::default()
        },
        session_given: session.is_some(),
        learned_at: LearnedAt::default(),
        start_ordinal,
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
        ..
    } = walk;
    let cutoff = consumption_cutoff(starts.len(), &pending, &resolved);
    let consumed = starts.get(cutoff).copied().unwrap_or(complete_end);
    learned_at.truncate(&mut outcome, cutoff);
    effects.retain(|(index, _)| *index < cutoff);
    for (_, effect) in effects {
        match effect {
            Effect::Observation(observation) => outcome.observations.push(*observation),
            Effect::Count(count) => outcome.add(count),
        }
    }
    // Counts without a session cannot be recorded; hold the lines back
    // until a record names the session.
    if outcome.session.is_none() && !outcome.counts.is_empty() {
        outcome.consumed = 0;
        outcome.next_ordinal = start_ordinal;
    } else {
        outcome.consumed = consumed as u64;
        outcome.next_ordinal = start_ordinal + cutoff as u64;
    }
    outcome
}

/// The index of the first line not to consume. A tool use still waiting for
/// its result blocks consumption from its issuing record onward, so the next
/// parse sees it with its result. Lowering the cutoff can leave a resolved
/// tool use's issuing record below it and its result at or beyond it; the
/// cutoff then drops to that issuing record too, until no pair is split.
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

/// Whether `block` is a content block of type `ty`.
fn is_block(block: &Value, ty: &str) -> bool {
    block.get("type").and_then(Value::as_str) == Some(ty)
}

/// The content blocks of a message; none unless the content is an array.
fn blocks(content: Option<&Value>) -> impl Iterator<Item = &Value> {
    content.and_then(Value::as_array).into_iter().flatten()
}
/// The speaker of a record. A `user` record is harness metadata when marked
/// meta, tool output when it holds only tool results, and otherwise a typed
/// message: from a person in a main transcript, and from the parent agent
/// in a sidechain (subagent) transcript, where it is a delegation rather
/// than human intent.
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

/// Text of a message's content, empty when it carries none.
fn text_of(content: Option<&Value>) -> String {
    content_text(content).unwrap_or_default()
}

/// Text of message or `tool_result` content: the string itself, or its text
/// blocks joined by newlines. `None` when it carries no text, which differs
/// from carrying empty text.
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

/// The edit's path relative to the checkout, read from both the input's
/// `file_path` and the result's `filePath`. A path on one side only is used
/// as it is. When both sides carry one, each is made relative to the
/// checkout and they must agree; if they do not, which file was edited is
/// uncertain and the edit is counted as [`Count::Conflicting`] rather than
/// recorded under either path. A path that cannot be shown to lie inside
/// the checkout is [`Count::Outside`].
fn relative_path(tool: &PendingTool, result: Option<&Value>) -> Result<PathBuf, Count> {
    let from_input = string_field(Some(&tool.input), "file_path").map(|p| contained(&tool.cwd, p));
    let from_result = string_field(result, "filePath").map(|p| contained(&tool.cwd, p));
    match (from_input, from_result) {
        (Some(input), Some(result)) if input != result => Err(Count::Conflicting),
        (Some(path), _) | (None, Some(path)) => path.ok_or(Count::Outside),
        (None, None) => Err(Count::Outside),
    }
}

/// `path` relative to `checkout`, or `None` when it cannot be shown to lie
/// inside it. Containment is lexical: the checkout must be non-empty, the
/// path must start with it, and the remainder may contain only normal
/// components. `/work/repo/../secret` strips to `../secret` and is rejected
/// here. Symlinks are not resolved in this plan; plan 2 checks paths against
/// the git tree, which never contains one that escapes.
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

/// Why a text field is not retained when neither the result nor the input
/// carries it.
const FIELD_MISSING: &str = "field missing";
/// Why a command stream is not retained when the source does not carry it.
const NOT_CAPTURED: &str = "not captured";
/// Why a command's streams are not retained when it has no result.
const NO_RESULT: &str = "no result recorded";

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

/// The text as retained material, else unavailable for `reason`.
fn retained_or(text: Option<&str>, reason: &str) -> Material<String> {
    text.map_or_else(
        || Material::unavailable(reason),
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
        old_text: retained_or(old.value, FIELD_MISSING),
        new_text: retained_or(new.value, FIELD_MISSING),
        replace_all: replace_all.value.unwrap_or(false),
    }
}

/// A `Write`. The written content comes from the result, else the input.
/// Without either, or when both carry it and they differ, the after-state
/// is unknown; the result's text is still retained.
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
        old_text: Material::unavailable("multi-edit"),
        new_text: Material::unavailable("multi-edit"),
        replace_all: false,
    }
}

/// The result side of a resolved tool use.
struct Resolution<'a> {
    /// The record's `toolUseResult`, when it can belong to this tool use.
    structured: Option<&'a Value>,
    /// The `tool_result` content block.
    block: &'a Value,
    /// Whether the harness flagged the result as an error.
    is_error: bool,
    /// Time of the record carrying the result, when it has one.
    ended: Option<&'a Timestamp>,
}

/// The edit a resolved editing tool made, or what to count instead. The
/// edit's time is when its result was recorded, or the issuing record's time
/// when the result record carries none.
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
        source_ordinal: tool.ordinal,
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

/// The shell command a tool use ran, as known before any result: its
/// streams unavailable, no error, no outcome, and no end time.
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
        source_ordinal: tool.ordinal,
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

/// What a resolved tool use contributes: an edit, a command, or a count.
/// An editing tool the capture does not read is counted rather than passed
/// over, so the missing edit evidence is visible. Other tools contribute
/// nothing.
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

/// A shell command still without a result when a typed message arrived.
fn build_abandoned(tool: &PendingTool, session: &SessionId) -> Option<Observation> {
    (tool.name == "Bash").then(|| Observation::Command(command_base(tool, session)))
}

/// Coarse classification of a shell command line, read from the programs it
/// runs rather than from any text it mentions.
///
/// The line is split into simple commands at `;`, `&`, `|`, and newlines
/// outside quotes (so `&&` and `||` split as well, but the `&` of a
/// redirection such as `2>&1` does not), and each into words with quotes
/// removed. Leading variable assignments (`RUST_LOG=debug`), `sudo`, `time`,
/// `env`, and a `mise exec ... --` prefix are skipped; what remains names
/// the program, and for `cargo`, `go`, `npm`, and `pnpm` its subcommand
/// too. A test run in any simple command makes the line a
/// [`CommandCategory::TestRun`], else a build a
/// [`CommandCategory::Build`], else git or `gh` a [`CommandCategory::Git`].
/// So `cat nextest-results.txt` and `echo "cargo test"` are
/// [`CommandCategory::Other`].
#[must_use]
pub fn classify_command(cmd: &str) -> CommandCategory {
    simple_commands(cmd)
        .iter()
        .map(|words| category_of(program_words(words)))
        .max_by_key(|category| rank(*category))
        .unwrap_or(CommandCategory::Other)
}

/// Precedence of a category when a line runs several commands.
const fn rank(category: CommandCategory) -> u8 {
    match category {
        CommandCategory::Other => 0,
        CommandCategory::Git => 1,
        CommandCategory::Build => 2,
        CommandCategory::TestRun => 3,
    }
}

/// The category of one simple command, given its words from the program on.
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

/// `words` from the program on: leading assignments, `sudo`, `time`, `env`,
/// and `mise exec ... --` are skipped.
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

/// Whether `word` is a shell variable assignment such as `RUST_LOG=debug`.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
            && !name.is_empty()
    })
}

/// Splits a command line into simple commands, each a list of words with
/// quotes and backslash escapes removed. See [`classify_command`] for where
/// it splits.
fn simple_commands(line: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    // Whether a word has begun, which an empty quoted word also does.
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut prev: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => word.extend(chars.next()),
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                word.extend(chars.next());
                in_word = true;
            }
            // `2>&1`, `>&2`, and `&>file` redirect; they do not split.
            (None, '&') if matches!(prev, Some('>' | '<')) || chars.peek() == Some(&'>') => {
                word.push(c);
                in_word = true;
            }
            (None, ';' | '&' | '|' | '\n') => {
                end_word(&mut words, &mut word, &mut in_word);
                if !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
            }
            (None, c) if c.is_whitespace() => end_word(&mut words, &mut word, &mut in_word),
            (None, c) => {
                word.push(c);
                in_word = true;
            }
        }
        prev = Some(c);
    }
    end_word(&mut words, &mut word, &mut in_word);
    if !words.is_empty() {
        commands.push(words);
    }
    commands
}

/// Moves a begun word into `words`.
fn end_word(words: &mut Vec<String>, word: &mut String, in_word: &mut bool) {
    if std::mem::take(in_word) {
        words.push(std::mem::take(word));
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
