//! Rules an annotation document meets before the record stores it.
//!
//! [`validate`] checks an [`AnnotationDocument`] against what the importer
//! read: the record and session the document claims, the session's current
//! annotation revision, the record's revision, and the session's
//! observations. It checks every rule and lists every [`Problem`], each at
//! its JSON path, in document order:
//!
//! - `schema` is [`ANNOTATION_SCHEMA`]. `session` is the session imported
//!   for, and it has observations. `based_on` is its current revision, or
//!   `None` when it has none.
//! - `prepared` names this record, a revision no later than the one read,
//!   and no more observations than the session has. Observation logs only
//!   grow, so a smaller count names a prefix that still exists.
//! - `produced_by`, titles and texts are not blank and hold at most
//!   [`MAX_PRODUCED_BY_CHARS`], [`MAX_TITLE_CHARS`] and [`MAX_TEXT_CHARS`]
//!   characters.
//! - Decision and open-question ids are 1 to [`ID_MAX_CHARS`] characters of
//!   `A-Z`, `a-z`, `0-9`, `_` and `-`, unique within their list.
//! - Cites, an alternative's edits, and a question's `about` name edits or
//!   commands of the session. A decision may cite nothing.
//! - An explicit rationale, alternative reason or implication has a source.
//!   The annotator's rationale is inference, never explicit.
//! - A source names a turn of the session, and its quote of 1 to
//!   [`MAX_QUOTE_CHARS`] characters occurs verbatim in the retained text of
//!   a sighting of that turn whose role fits the entry: a human turn for
//!   speaker `human`; an agent or delegation turn for `agent`; any of those
//!   for an explicit implication; and a report as well for the annotator's
//!   inference. A report is harness-relayed text, not known to be the
//!   agent's, so it backs no explicit statement. A rewritten transcript
//!   sights a turn id again with other text; a quote may come from any
//!   sighting.
//! - A proposed alternative has no edits; an attempted one has at least one.
//! - `corrections_seen` holds judgment ids in the form the review store
//!   generates.
//! - Lists hold at most their bound of entries: [`MAX_CORRECTIONS_SEEN`]
//!   corrections, [`MAX_DECISIONS`] decisions, [`MAX_QUESTIONS`] questions,
//!   and [`MAX_PER_DECISION_LIST`] in each list of a decision and in a
//!   question's `about`. A longer list is one problem at its path, and its
//!   entries are checked as well.
//!
//! A quote shows the words were in the turn, not that they explain the
//! cited edits.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt;

use supersigil_record::interpretations::{
    ANNOTATION_SCHEMA, Alternative, AlternativeStatus, AnnotationDocument, AnnotationRevision,
    Decision, Implication, OpenQuestion, Rationale, Source, Speaker,
};
use supersigil_record::observations::{Role, Turn};
use supersigil_record::{EventId, JudgmentId, Observation, RecordId, Revision, SessionId, TurnId};

/// Most characters in a rationale, alternative reason, implication or
/// question text.
pub const MAX_TEXT_CHARS: usize = 2_000;
/// Most characters in a decision's or alternative's title.
pub const MAX_TITLE_CHARS: usize = 200;
/// Most characters in a source's quote: a passage, not a reply.
pub const MAX_QUOTE_CHARS: usize = 2_000;
/// Most characters in `produced_by`.
pub const MAX_PRODUCED_BY_CHARS: usize = 200;
/// Most decisions in a document.
pub const MAX_DECISIONS: usize = 10_000;
/// Most open questions in a document.
pub const MAX_QUESTIONS: usize = 10_000;
/// Most entries in each list of a decision: its cites, rationale,
/// alternatives and implications, and each alternative's edits; and in an
/// open question's `about`.
pub const MAX_PER_DECISION_LIST: usize = 1_000;
/// Most judgment ids in `corrections_seen`.
pub const MAX_CORRECTIONS_SEEN: usize = 10_000;
/// Most characters in a decision or open-question id.
pub const ID_MAX_CHARS: usize = 64;

/// What the importer read about the record and session a document claims.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The record imported into.
    pub record: &'a RecordId,
    /// The session imported for.
    pub session: &'a SessionId,
    /// The session's current annotation revision, `None` when it has none.
    pub current: Option<AnnotationRevision>,
    /// The record's revision read.
    pub record_revision: Revision,
    /// The session's observations at that revision, in log order. A turn
    /// id appears once per sighting.
    pub observations: &'a [Observation],
}

/// A rule a document breaks, at the JSON path of the value breaking it.
///
/// Displays as `<path>: <message>`. Messages quote document values
/// unescaped, so terminal output escapes them as it does every error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The JSON path in the document: field names joined by `.`, list
    /// positions in brackets, such as `decisions[2].rationale[0].text`.
    pub path: String,
    /// What is wrong there.
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// Checks `document` against `context` by the [module's rules](self).
///
/// Indexes the session's turns, edits and commands once, so the time taken
/// is linear in the document plus the quotes' substring searches.
///
/// # Errors
///
/// Returns every [`Problem`] found, in document order, when the document
/// breaks any rule.
pub fn validate(context: &Context<'_>, document: &AnnotationDocument) -> Result<(), Vec<Problem>> {
    let mut check = Check::new(context.observations);
    check.header(context, document);
    check.corrections(&document.corrections_seen);
    check.decisions(&document.decisions);
    check.questions(&document.open_questions);
    if check.problems.0.is_empty() {
        Ok(())
    } else {
        Err(check.problems.0)
    }
}

/// A JSON path in the document, built into a string only for a problem.
#[derive(Clone, Copy)]
enum JsonPath<'p> {
    /// The document itself.
    Root,
    /// A field of the value at the parent path.
    Field(&'p JsonPath<'p>, &'static str),
    /// An entry of the list at the parent path.
    Index(&'p JsonPath<'p>, usize),
}

impl<'p> JsonPath<'p> {
    /// The path of field `name` of the value here.
    const fn field(&'p self, name: &'static str) -> Self {
        Self::Field(self, name)
    }

    /// The path of entry `index` of the list here.
    const fn index(&'p self, index: usize) -> Self {
        Self::Index(self, index)
    }
}

impl fmt::Display for JsonPath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Root => Ok(()),
            Self::Field(&Self::Root, name) => f.write_str(name),
            Self::Field(parent, name) => write!(f, "{parent}.{name}"),
            Self::Index(parent, index) => write!(f, "{parent}[{index}]"),
        }
    }
}

/// The problems found so far, in document order.
#[derive(Default)]
struct Problems(Vec<Problem>);

impl Problems {
    /// Adds the problem `message` at `path`.
    fn add(&mut self, path: JsonPath<'_>, message: impl Into<String>) {
        self.0.push(Problem {
            path: path.to_string(),
            message: message.into(),
        });
    }
}

/// The session's observations, indexed, and the problems found.
struct Check<'a> {
    /// Every sighting of each turn, in log order.
    turns: HashMap<&'a TurnId, Vec<&'a Turn>>,
    /// The ids of the session's edits and commands.
    events: HashSet<&'a EventId>,
    /// The problems found so far.
    problems: Problems,
}

impl<'a> Check<'a> {
    /// Indexes `observations`.
    fn new(observations: &'a [Observation]) -> Self {
        let mut turns: HashMap<_, Vec<_>> = HashMap::new();
        let mut events = HashSet::new();
        for observation in observations {
            match observation {
                Observation::Turn(turn) => turns.entry(&turn.id).or_default().push(turn),
                Observation::Edit(edit) => {
                    events.insert(&edit.id);
                }
                Observation::Command(command) => {
                    events.insert(&command.id);
                }
                _ => {}
            }
        }
        Self {
            turns,
            events,
            problems: Problems::default(),
        }
    }

    /// Checks the fields before the lists against `context`.
    fn header(&mut self, context: &Context<'_>, document: &AnnotationDocument) {
        let root = JsonPath::Root;
        if document.schema != ANNOTATION_SCHEMA {
            self.problems.add(
                root.field("schema"),
                format!(
                    "{:?}, but this build reads {ANNOTATION_SCHEMA}",
                    document.schema
                ),
            );
        }
        if document.session != *context.session {
            self.problems.add(
                root.field("session"),
                format!(
                    "{:?}, but this import is for session {}",
                    document.session.as_str(),
                    context.session
                ),
            );
        } else if context.observations.is_empty() {
            self.problems.add(
                root.field("session"),
                format!(
                    "session {} has no observations in this record",
                    context.session
                ),
            );
        }
        if document.based_on != context.current {
            self.problems.add(
                root.field("based_on"),
                format!(
                    "{}, but the current revision is {}",
                    revision(document.based_on),
                    revision(context.current)
                ),
            );
        }
        let prepared = &document.prepared;
        let path = root.field("prepared");
        if prepared.record != *context.record {
            self.problems.add(
                path.field("record"),
                format!(
                    "{:?}, but this is record {}",
                    prepared.record.as_str(),
                    context.record
                ),
            );
        }
        if prepared.observation_revision > context.record_revision {
            self.problems.add(
                path.field("observation_revision"),
                format!(
                    "{}, but the record is at revision {}",
                    prepared.observation_revision.get(),
                    context.record_revision.get()
                ),
            );
        }
        let observed = context.observations.len() as u64;
        if prepared.observed.count > observed {
            self.problems.add(
                path.field("observed").field("count"),
                format!(
                    "{}, but the session has {observed} observations",
                    prepared.observed.count
                ),
            );
        }
        self.text(
            root.field("produced_by"),
            &document.produced_by,
            MAX_PRODUCED_BY_CHARS,
        );
    }

    /// Checks `corrections_seen`.
    fn corrections(&mut self, ids: &[JudgmentId]) {
        let root = JsonPath::Root;
        self.list(
            root.field("corrections_seen"),
            ids,
            MAX_CORRECTIONS_SEEN,
            |check, path, id| {
                if !is_judgment_id(id.as_str()) {
                    check
                        .problems
                        .add(path, format!("{:?} is not a judgment id", id.as_str()));
                }
            },
        );
    }

    /// Checks `decisions`.
    fn decisions(&mut self, decisions: &[Decision]) {
        let root = JsonPath::Root;
        let list = root.field("decisions");
        self.bound(list, decisions.len(), MAX_DECISIONS);
        let mut ids = HashMap::new();
        for (index, decision) in decisions.iter().enumerate() {
            self.id(list, index, &decision.id, &mut ids);
            let path = list.index(index);
            self.text(path.field("title"), &decision.title, MAX_TITLE_CHARS);
            self.list(
                path.field("cites"),
                &decision.cites,
                MAX_PER_DECISION_LIST,
                Self::event,
            );
            self.list(
                path.field("rationale"),
                &decision.rationale,
                MAX_PER_DECISION_LIST,
                Self::rationale,
            );
            self.list(
                path.field("alternatives"),
                &decision.alternatives,
                MAX_PER_DECISION_LIST,
                Self::alternative,
            );
            self.list(
                path.field("implications"),
                &decision.implications,
                MAX_PER_DECISION_LIST,
                Self::implication,
            );
        }
    }

    /// Checks `open_questions`.
    fn questions(&mut self, questions: &[OpenQuestion]) {
        let root = JsonPath::Root;
        let list = root.field("open_questions");
        self.bound(list, questions.len(), MAX_QUESTIONS);
        let mut ids = HashMap::new();
        for (index, question) in questions.iter().enumerate() {
            self.id(list, index, &question.id, &mut ids);
            let path = list.index(index);
            self.text(path.field("text"), &question.text, MAX_TEXT_CHARS);
            self.list(
                path.field("about"),
                &question.about,
                MAX_PER_DECISION_LIST,
                Self::event,
            );
        }
    }

    /// Checks that the list at `path` holds at most `max` entries, then
    /// checks each entry with `check`.
    fn list<T>(
        &mut self,
        path: JsonPath<'_>,
        entries: &[T],
        max: usize,
        mut check: impl FnMut(&mut Self, JsonPath<'_>, &T),
    ) {
        self.bound(path, entries.len(), max);
        for (index, entry) in entries.iter().enumerate() {
            check(self, path.index(index), entry);
        }
    }

    /// Checks that the list at `path`, of `len` entries, holds at most
    /// `max`.
    fn bound(&mut self, path: JsonPath<'_>, len: usize, max: usize) {
        if len > max {
            self.problems
                .add(path, format!("has {len} entries; at most {max}"));
        }
    }

    /// Checks that the id `id` of entry `index` of `list` is well formed
    /// and no earlier entry's. `seen` holds each well-formed id's first
    /// entry.
    fn id<'e>(
        &mut self,
        list: JsonPath<'_>,
        index: usize,
        id: &'e str,
        seen: &mut HashMap<&'e str, usize>,
    ) {
        let entry = list.index(index);
        let path = entry.field("id");
        let chars = id.chars().count();
        if chars == 0 {
            self.problems.add(
                path,
                format!("is empty; an id has 1 to {ID_MAX_CHARS} characters"),
            );
        } else if chars > ID_MAX_CHARS {
            self.problems.add(
                path,
                format!("has {chars} characters; an id has at most {ID_MAX_CHARS}"),
            );
        } else if let Some(other) = id
            .chars()
            .find(|&c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        {
            self.problems.add(
                path,
                format!("{id:?} holds {other:?}; an id holds only A-Z, a-z, 0-9, _ and -"),
            );
        } else {
            match seen.entry(id) {
                Entry::Occupied(first) => self.problems.add(
                    path,
                    format!("{id:?} repeats {}", list.index(*first.get()).field("id")),
                ),
                Entry::Vacant(vacant) => {
                    vacant.insert(index);
                }
            }
        }
    }

    /// Checks that `text` at `path` is not blank and holds at most `max`
    /// characters.
    fn text(&mut self, path: JsonPath<'_>, text: &str, max: usize) {
        if text.trim().is_empty() {
            self.problems.add(path, "is blank");
            return;
        }
        let chars = text.chars().count();
        if chars > max {
            self.problems
                .add(path, format!("has {chars} characters; at most {max}"));
        }
    }

    /// Checks that `id` at `path` names an edit or command of the session.
    fn event(&mut self, path: JsonPath<'_>, id: &EventId) {
        if !self.events.contains(id) {
            self.problems.add(
                path,
                format!(
                    "{:?} is not an edit or command of this session",
                    id.as_str()
                ),
            );
        }
    }

    /// Checks a rationale, or an alternative's reason, at `path`.
    fn rationale(&mut self, path: JsonPath<'_>, rationale: &Rationale) {
        self.text(path.field("text"), &rationale.text, MAX_TEXT_CHARS);
        if rationale.speaker == Speaker::Annotator && rationale.explicit {
            self.problems.add(
                path.field("explicit"),
                "true, but speaker annotator is never explicit",
            );
        }
        self.sourced(
            path,
            rationale.explicit,
            rationale.source.as_ref(),
            Claim::Rationale(rationale.speaker),
        );
    }

    /// Checks an alternative at `path`.
    fn alternative(&mut self, path: JsonPath<'_>, alternative: &Alternative) {
        self.text(path.field("title"), &alternative.title, MAX_TITLE_CHARS);
        let mismatch = match (alternative.status, alternative.edits.is_empty()) {
            (AlternativeStatus::Proposed, false) => {
                Some("proposed, but it lists edits; a proposed alternative has none")
            }
            (AlternativeStatus::Attempted, true) => {
                Some("attempted, but it lists no edits; an attempted alternative has at least one")
            }
            (AlternativeStatus::Proposed, true) | (AlternativeStatus::Attempted, false) => None,
        };
        if let Some(message) = mismatch {
            self.problems.add(path.field("status"), message);
        }
        self.list(
            path.field("edits"),
            &alternative.edits,
            MAX_PER_DECISION_LIST,
            Self::event,
        );
        if let Some(reason) = &alternative.reason {
            self.rationale(path.field("reason"), reason);
        }
    }

    /// Checks an implication at `path`.
    fn implication(&mut self, path: JsonPath<'_>, implication: &Implication) {
        self.text(path.field("text"), &implication.text, MAX_TEXT_CHARS);
        self.sourced(
            path,
            implication.explicit,
            implication.source.as_ref(),
            Claim::Implication {
                explicit: implication.explicit,
            },
        );
    }

    /// Checks the source of the entry at `path`: present when the entry is
    /// `explicit`, and when present, quoting a sighting that fits `claim`.
    fn sourced(
        &mut self,
        path: JsonPath<'_>,
        explicit: bool,
        source: Option<&Source>,
        claim: Claim,
    ) {
        let path = path.field("source");
        match source {
            Some(source) => self.source(path, source, claim),
            None if explicit => self
                .problems
                .add(path, "is missing; an explicit statement needs a source"),
            None => {}
        }
    }

    /// Checks that `source` at `path` names a turn of the session and
    /// quotes 1 to [`MAX_QUOTE_CHARS`] characters of a sighting of it that
    /// fits `claim`.
    fn source(&mut self, path: JsonPath<'_>, source: &Source, claim: Claim) {
        let turn = source.turn.as_str();
        let sightings = self.turns.get(&source.turn);
        if sightings.is_none() {
            self.problems.add(
                path.field("turn"),
                format!("{turn:?} is not a turn of this session"),
            );
        }
        let path = path.field("quote");
        let chars = source.quote.chars().count();
        if chars == 0 {
            self.problems.add(
                path,
                format!("is empty; a quote has 1 to {MAX_QUOTE_CHARS} characters"),
            );
        } else if chars > MAX_QUOTE_CHARS {
            self.problems.add(
                path,
                format!("has {chars} characters; at most {MAX_QUOTE_CHARS}"),
            );
        } else if let Some(message) =
            sightings.and_then(|sightings| misquoted(sightings, turn, &source.quote, claim))
        {
            self.problems.add(path, message);
        }
    }
}

/// What a source backs, which decides the sightings it may quote.
#[derive(Clone, Copy)]
enum Claim {
    /// A rationale by this speaker, stated or inferred.
    Rationale(Speaker),
    /// An implication.
    Implication {
        /// Whether a turn states it.
        explicit: bool,
    },
}

impl Claim {
    /// Whether a sighting with `role` can back it. A report backs only an
    /// inference.
    const fn fits(self, role: Role) -> bool {
        match self {
            Self::Rationale(Speaker::Human) => matches!(role, Role::Human),
            Self::Rationale(Speaker::Agent) => matches!(role, Role::Agent | Role::Delegation),
            Self::Implication { explicit: true } => {
                matches!(role, Role::Human | Role::Agent | Role::Delegation)
            }
            Self::Rationale(Speaker::Annotator) | Self::Implication { explicit: false } => {
                matches!(
                    role,
                    Role::Human | Role::Agent | Role::Delegation | Role::Report
                )
            }
        }
    }

    /// The text it needs, as a message says it.
    const fn needs(self) -> &'static str {
        match self {
            Self::Rationale(Speaker::Human) => "speaker human needs human text",
            Self::Rationale(Speaker::Agent) => "speaker agent needs agent or delegation text",
            Self::Implication { explicit: true } => {
                "an explicit implication needs human, agent or delegation text"
            }
            Self::Rationale(Speaker::Annotator) | Self::Implication { explicit: false } => {
                "an inference needs human, agent, delegation or report text"
            }
        }
    }
}

/// Why no sighting of turn `turn` that fits `claim` holds `quote` in its
/// retained text: the quote is only in sightings of other roles, in none,
/// or no sighting retains text. `None` when a fitting one holds it.
fn misquoted(sightings: &[&Turn], turn: &str, quote: &str, claim: Claim) -> Option<String> {
    let mut retained = false;
    let mut holding: Vec<Role> = Vec::new();
    for sighting in sightings {
        let Some(text) = sighting.text.retained() else {
            continue;
        };
        retained = true;
        if text.contains(quote) {
            if claim.fits(sighting.role) {
                return None;
            }
            if !holding.contains(&sighting.role) {
                holding.push(sighting.role);
            }
        }
    }
    if holding.is_empty() {
        return Some(if retained {
            format!("not found verbatim in the retained text of turn {turn:?}")
        } else {
            format!("the text of turn {turn:?} is not retained")
        });
    }
    let roles: Vec<&str> = holding.iter().map(|&role| role_name(role)).collect();
    let mut message = format!(
        "found only in {} text of turn {turn:?}, but {}",
        roles.join(" and "),
        claim.needs()
    );
    if holding.contains(&Role::Report) {
        message.push_str("; a report is harness-relayed text, not known to be the agent's");
    }
    Some(message)
}

/// The name of `role` as observations write it.
const fn role_name(role: Role) -> &'static str {
    match role {
        Role::Human => "human",
        Role::Agent => "agent",
        Role::Delegation => "delegation",
        Role::Report => "report",
        Role::Tool => "tool",
        Role::Meta => "meta",
        Role::Summary => "summary",
    }
}

/// A revision number, or `none`, as a message says it.
fn revision(revision: Option<AnnotationRevision>) -> String {
    revision.map_or_else(|| "none".to_owned(), |revision| revision.to_string())
}

/// Whether `id` has the form [`JudgmentId::generate`] gives it: a version 4
/// UUID in lowercase hexadecimal, hyphenated 8-4-4-4-12.
fn is_judgment_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            14 => byte == b'4',
            19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
            _ => matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
        })
}
