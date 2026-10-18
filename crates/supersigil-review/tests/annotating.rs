//! Tests annotation validation against a constructed session: each rule's
//! path and message, every problem listed in document order, quotes checked
//! against each sighting's role and retained text, and the time a large
//! document takes.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::{eid, finishes_within};
use proptest::prelude::*;
use supersigil_record::interpretations::{
    ANNOTATION_SCHEMA, Alternative, AlternativeStatus, AnnotationDocument, AnnotationRevision,
    Decision, Implication, Observed, OpenQuestion, Prepared, Rationale, Source, Speaker,
};
use supersigil_record::observations::{
    Command, CommandCategory, Edit, EditOperation, EditOrigin, FileState, Material, Observation,
    Role, Turn,
};
use supersigil_record::{JudgmentId, RecordId, Revision, SessionId, Timestamp, TurnId};
use supersigil_review::annotating::{
    Context, ID_MAX_CHARS, MAX_CORRECTIONS_SEEN, MAX_DECISIONS, MAX_LIST_ENTRIES,
    MAX_PRODUCED_BY_CHARS, MAX_QUESTIONS, MAX_QUOTE_CHARS, MAX_TEXT_CHARS, MAX_TITLE_CHARS,
    Problem, validate,
};

const SESSION: &str = "3f2a9c1e-5b7d-4e2f-9a1c-0d8e7f6a5b4c";
const RECORD: &str = "r1";
const EDIT: &str = "edit:0123456789abcdef";
const COMMAND: &str = "command:fedcba9876543210";
const JUDGMENT: &str = "0b5f8e4c-3d2a-4f1b-9c8d-7e6f5a4b3c2d";

/// What the human turn says.
const HUMAN_SAYS: &str = "make the sync plan inspectable";
/// What the agent turn's first sighting says.
const AGENT_SAYS: &str = "syncing after each file costs a round trip";
/// What the agent turn's second sighting, from a rewritten transcript,
/// says.
const AGENT_REWRITTEN: &str = "one sync per plan keeps it cheap";
/// What the delegation turn says.
const DELEGATION_SAYS: &str = "publish both stores through one plan";
/// What the relayed report says.
const REPORT_SAYS: &str = "all tests pass";

/// No problem at all.
const NO_PROBLEMS: [Problem; 0] = [];

/// The `n`th record revision.
fn revision(n: u64) -> Revision {
    (0..n).fold(Revision::ZERO, |revision, _| revision.next())
}

/// A sighting of turn `id` of the session with `role` and `text`.
fn sighting(id: &str, role: Role, text: Material<String>) -> Observation {
    Observation::Turn(Turn {
        id: TurnId::new(id),
        session: SessionId::new(SESSION),
        parent: None,
        role,
        time: Timestamp::new("2026-10-15T14:00:00.000Z"),
        sidechain: false,
        agent_id: None,
        text,
        source_ordinal: 0,
        transcript: None,
    })
}

/// A sighting of turn `id` with `role` that retains `text`.
fn turn(id: &str, role: Role, text: &str) -> Observation {
    sighting(id, role, Material::Retained(text.to_owned()))
}

/// An edit `id` issued by `turn`.
fn edit(id: &str, turn: &str) -> Observation {
    Observation::Edit(Edit {
        id: eid(id),
        turn: TurnId::new(turn),
        session: SessionId::new(SESSION),
        path: PathBuf::from("src/files.rs"),
        before: FileState::Absent,
        after: FileState::unknown(),
        patch: Material::unavailable("not needed here"),
        old_text: Material::unavailable("not needed here"),
        new_text: Material::unavailable("not needed here"),
        replace_all: false,
        operation: EditOperation::Write,
        origin: EditOrigin::Tool,
        checkout: PathBuf::from("/work/repo"),
        time: Timestamp::new("2026-10-15T14:00:01.000Z"),
        source_ordinal: 0,
        agent_id: None,
        transcript: None,
    })
}

/// A command `id` issued by `turn`.
fn command(id: &str, turn: &str) -> Observation {
    Observation::Command(Command {
        id: eid(id),
        turn: TurnId::new(turn),
        session: SessionId::new(SESSION),
        cmd: "cargo nextest run".to_owned(),
        exit: Some(0),
        stdout_tail: Material::unavailable("not needed here"),
        stderr_tail: Material::unavailable("not needed here"),
        category: CommandCategory::TestRun,
        reported_error: false,
        outcome: None,
        started: Timestamp::new("2026-10-15T14:00:02.000Z"),
        ended: None,
        checkout: PathBuf::from("/work/repo"),
        changes: Material::unavailable("not needed here"),
        source_ordinal: 0,
        agent_id: None,
        transcript: None,
    })
}

/// A session with a human, agent, delegation, report and tool turn, the
/// agent turn sighted twice with different text, an edit and a command.
fn observations() -> Vec<Observation> {
    vec![
        turn("t-human", Role::Human, &format!("Please {HUMAN_SAYS}.")),
        turn("t-agent", Role::Agent, &format!("I think {AGENT_SAYS}.")),
        edit(EDIT, "t-agent"),
        command(COMMAND, "t-agent"),
        turn(
            "t-delegation",
            Role::Delegation,
            &format!("Please {DELEGATION_SAYS}."),
        ),
        turn(
            "t-report",
            Role::Report,
            &format!("<task-notification>Agent finished: {REPORT_SAYS}</task-notification>"),
        ),
        sighting("t-tool", Role::Tool, Material::unavailable("tool result")),
        turn(
            "t-agent",
            Role::Agent,
            &format!("On reflection, {AGENT_REWRITTEN}."),
        ),
    ]
}

/// What the importer read: record r1 at revision 2, the session without
/// an annotation, and its observations.
struct Read {
    record: RecordId,
    session: SessionId,
    current: Option<AnnotationRevision>,
    record_revision: Revision,
    observations: Vec<Observation>,
}

impl Read {
    fn new() -> Self {
        Self {
            record: RecordId::new(RECORD),
            session: SessionId::new(SESSION),
            current: None,
            record_revision: revision(2),
            observations: observations(),
        }
    }

    fn context(&self) -> Context<'_> {
        Context {
            record: &self.record,
            session: &self.session,
            current: self.current,
            record_revision: self.record_revision,
            observations: &self.observations,
        }
    }

    /// The problems `validate` lists for `document`, none when it passes.
    fn problems(&self, document: &AnnotationDocument) -> Vec<Problem> {
        validate(&self.context(), document)
            .err()
            .unwrap_or_default()
    }
}

/// The problems `validate` lists for `document` against [`Read::new`].
fn problems(document: &AnnotationDocument) -> Vec<Problem> {
    Read::new().problems(document)
}

fn problem(path: &str, message: &str) -> Problem {
    Problem {
        path: path.to_owned(),
        message: message.to_owned(),
    }
}

fn source(turn: &str, quote: &str) -> Source {
    Source {
        turn: TurnId::new(turn),
        quote: quote.to_owned(),
    }
}

/// An explicit rationale by `speaker`, quoting `quote` from `turn`.
fn stated(speaker: Speaker, turn: &str, quote: &str) -> Rationale {
    Rationale {
        text: "the speaker said so".to_owned(),
        speaker,
        explicit: true,
        source: Some(source(turn, quote)),
    }
}

/// The annotator's inference, from `source` if given.
fn inferred(source: Option<Source>) -> Rationale {
    Rationale {
        text: "the author preferred one plan".to_owned(),
        speaker: Speaker::Annotator,
        explicit: false,
        source,
    }
}

/// A document every rule accepts, using each speaker, both alternative
/// statuses, sourced and unsourced entries, and a question.
fn valid() -> AnnotationDocument {
    AnnotationDocument {
        schema: ANNOTATION_SCHEMA.to_owned(),
        session: SessionId::new(SESSION),
        based_on: None,
        prepared: Prepared {
            record: RecordId::new(RECORD),
            observation_revision: revision(2),
            observed: Observed {
                count: 8,
                last: None,
            },
        },
        produced_by: "claude-code/supersigil-annotate".to_owned(),
        corrections_seen: vec![JudgmentId::new(JUDGMENT)],
        decisions: vec![Decision {
            id: "d1".to_owned(),
            title: "Publish both stores through one sync plan".to_owned(),
            cites: vec![eid(EDIT), eid(COMMAND)],
            rationale: vec![
                stated(Speaker::Human, "t-human", HUMAN_SAYS),
                stated(Speaker::Agent, "t-agent", AGENT_SAYS),
                inferred(Some(source("t-report", REPORT_SAYS))),
                inferred(None),
            ],
            alternatives: vec![
                Alternative {
                    title: "Per-store syncs".to_owned(),
                    status: AlternativeStatus::Attempted,
                    edits: vec![eid(EDIT)],
                    reason: Some(stated(Speaker::Agent, "t-delegation", DELEGATION_SAYS)),
                },
                Alternative {
                    title: "No sync at all".to_owned(),
                    status: AlternativeStatus::Proposed,
                    edits: Vec::new(),
                    reason: None,
                },
            ],
            implications: vec![
                Implication {
                    text: "the plan stays inspectable".to_owned(),
                    explicit: true,
                    source: Some(source("t-human", HUMAN_SAYS)),
                },
                Implication {
                    text: "every new file kind needs a step in the plan".to_owned(),
                    explicit: false,
                    source: None,
                },
            ],
        }],
        open_questions: vec![OpenQuestion {
            id: "q1".to_owned(),
            text: "Why keep the old plan?".to_owned(),
            about: vec![eid(COMMAND)],
        }],
    }
}

/// A document without decisions or questions, prepared before the session
/// had observations.
fn empty() -> AnnotationDocument {
    let mut document = AnnotationDocument {
        decisions: Vec::new(),
        open_questions: Vec::new(),
        ..valid()
    };
    document.prepared.observed.count = 0;
    document
}

#[test]
fn a_full_valid_document_passes() {
    assert_eq!(validate(&Read::new().context(), &valid()), Ok(()));
    assert_eq!(problems(&empty()), NO_PROBLEMS);
}

#[test]
fn a_problem_displays_as_its_path_and_message() {
    let document = AnnotationDocument {
        produced_by: String::new(),
        ..valid()
    };
    let shown: Vec<String> = problems(&document).iter().map(Problem::to_string).collect();
    assert_eq!(shown, ["produced_by: is blank"]);
}

#[test]
fn the_schema_must_be_the_annotation_schema() {
    let document = AnnotationDocument {
        schema: "supersigil.annotation/2".to_owned(),
        ..valid()
    };
    assert_eq!(
        problems(&document),
        [problem(
            "schema",
            r#""supersigil.annotation/2", but this build reads supersigil.annotation/1"#
        )]
    );
}

#[test]
fn the_session_must_be_the_one_imported_and_have_observations() {
    let document = AnnotationDocument {
        session: SessionId::new("9c0d1e2f"),
        ..valid()
    };
    assert_eq!(
        problems(&document),
        [problem(
            "session",
            &format!(r#""9c0d1e2f", but this import is for session {SESSION}"#)
        )]
    );
    let unobserved = Read {
        observations: Vec::new(),
        ..Read::new()
    };
    assert_eq!(
        unobserved.problems(&empty()),
        [problem(
            "session",
            &format!("session {SESSION} has no observations in this record")
        )]
    );
}

#[test]
fn based_on_must_be_the_current_revision() {
    let second = AnnotationRevision::FIRST.next();
    let annotated = Read {
        current: Some(second),
        ..Read::new()
    };
    assert_eq!(
        annotated.problems(&valid()),
        [problem("based_on", "none, but the current revision is 2")]
    );
    let on_first = AnnotationDocument {
        based_on: Some(AnnotationRevision::FIRST),
        ..valid()
    };
    assert_eq!(
        annotated.problems(&on_first),
        [problem("based_on", "1, but the current revision is 2")]
    );
    assert_eq!(
        problems(&on_first),
        [problem("based_on", "1, but the current revision is none")]
    );
    let on_current = AnnotationDocument {
        based_on: Some(second),
        ..valid()
    };
    assert_eq!(annotated.problems(&on_current), NO_PROBLEMS);
}

#[test]
fn prepared_must_name_this_record_and_no_later_state() {
    let mut document = valid();
    document.prepared = Prepared {
        record: RecordId::new("r2"),
        observation_revision: revision(3),
        observed: Observed {
            count: 9,
            last: None,
        },
    };
    assert_eq!(
        problems(&document),
        [
            problem("prepared.record", r#""r2", but this is record r1"#),
            problem(
                "prepared.observation_revision",
                "3, but the record is at revision 2"
            ),
            problem(
                "prepared.observed.count",
                "9, but the session has 8 observations"
            ),
        ]
    );
    // An earlier state names a prefix that still exists.
    document.prepared = Prepared {
        record: RecordId::new(RECORD),
        observation_revision: revision(1),
        observed: Observed {
            count: 3,
            last: None,
        },
    };
    assert_eq!(problems(&document), NO_PROBLEMS);
}

#[test]
fn produced_by_must_be_non_blank_and_short() {
    let blank = AnnotationDocument {
        produced_by: " \n\t".to_owned(),
        ..valid()
    };
    assert_eq!(problems(&blank), [problem("produced_by", "is blank")]);
    let long = AnnotationDocument {
        produced_by: "é".repeat(MAX_PRODUCED_BY_CHARS + 1),
        ..valid()
    };
    assert_eq!(
        problems(&long),
        [problem("produced_by", "has 201 characters; at most 200")]
    );
    // Characters are counted, not bytes.
    let at_bound = AnnotationDocument {
        produced_by: "é".repeat(MAX_PRODUCED_BY_CHARS),
        ..valid()
    };
    assert_eq!(problems(&at_bound), NO_PROBLEMS);
}

#[test]
fn ids_are_short_plain_and_unique_within_their_list() {
    let mut document = valid();
    let decision = document.decisions[0].clone();
    let long = "x".repeat(ID_MAX_CHARS + 1);
    let longest = "Az09_-".repeat(11)[..ID_MAX_CHARS].to_owned();
    document.decisions = ["d1", "", "d 1", &long, "d1", &longest, "d1"]
        .into_iter()
        .map(|id| Decision {
            id: id.to_owned(),
            ..decision.clone()
        })
        .collect();
    let question = document.open_questions[0].clone();
    // A question may share a decision's id.
    document.open_questions = ["q1", "d1", "q1", "é"]
        .into_iter()
        .map(|id| OpenQuestion {
            id: id.to_owned(),
            ..question.clone()
        })
        .collect();
    assert_eq!(
        problems(&document),
        [
            problem("decisions[1].id", "is empty; an id has 1 to 64 characters"),
            problem(
                "decisions[2].id",
                r#""d 1" holds ' '; an id holds only A-Z, a-z, 0-9, _ and -"#
            ),
            problem("decisions[3].id", "has 65 characters; an id has at most 64"),
            problem("decisions[4].id", r#""d1" repeats decisions[0].id"#),
            problem("decisions[6].id", r#""d1" repeats decisions[0].id"#),
            problem(
                "open_questions[2].id",
                r#""q1" repeats open_questions[0].id"#
            ),
            problem(
                "open_questions[3].id",
                r#""é" holds 'é'; an id holds only A-Z, a-z, 0-9, _ and -"#
            ),
        ]
    );
}

#[test]
fn cited_ids_name_edits_or_commands_of_the_session() {
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.cites = vec![eid(EDIT), eid("t-agent"), eid("edit:ffffffffffffffff")];
    decision.alternatives[0].edits = vec![eid(COMMAND), eid("command:0000000000000000")];
    document.open_questions[0].about = vec![eid("t-human")];
    assert_eq!(
        problems(&document),
        [
            problem(
                "decisions[0].cites[1]",
                r#""t-agent" is not an edit or command of this session"#
            ),
            problem(
                "decisions[0].cites[2]",
                r#""edit:ffffffffffffffff" is not an edit or command of this session"#
            ),
            problem(
                "decisions[0].alternatives[0].edits[1]",
                r#""command:0000000000000000" is not an edit or command of this session"#
            ),
            problem(
                "open_questions[0].about[0]",
                r#""t-human" is not an edit or command of this session"#
            ),
        ]
    );
    // A decision may cite nothing: it is history.
    let mut document = valid();
    document.decisions[0].cites.clear();
    assert_eq!(problems(&document), NO_PROBLEMS);
}

#[test]
fn titles_and_texts_are_non_blank_and_within_their_bounds() {
    let over = "é".repeat(MAX_TEXT_CHARS + 1);
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.title = "  ".to_owned();
    decision.rationale[0].text.clone_from(&over);
    decision.alternatives[1].title = "x".repeat(MAX_TITLE_CHARS + 1);
    decision.alternatives[0].reason.as_mut().unwrap().text = "\n".to_owned();
    decision.implications[1].text = String::new();
    document.open_questions[0].text.clone_from(&over);
    assert_eq!(
        problems(&document),
        [
            problem("decisions[0].title", "is blank"),
            problem(
                "decisions[0].rationale[0].text",
                "has 2001 characters; at most 2000"
            ),
            problem("decisions[0].alternatives[0].reason.text", "is blank"),
            problem(
                "decisions[0].alternatives[1].title",
                "has 201 characters; at most 200"
            ),
            problem("decisions[0].implications[1].text", "is blank"),
            problem(
                "open_questions[0].text",
                "has 2001 characters; at most 2000"
            ),
        ]
    );
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.title = "é".repeat(MAX_TITLE_CHARS);
    decision.alternatives[0].title = "é".repeat(MAX_TITLE_CHARS);
    decision.rationale[3].text = "é".repeat(MAX_TEXT_CHARS);
    decision.implications[1].text = "é".repeat(MAX_TEXT_CHARS);
    document.open_questions[0].text = "é".repeat(MAX_TEXT_CHARS);
    assert_eq!(problems(&document), NO_PROBLEMS);
}

#[test]
fn an_explicit_statement_needs_a_source() {
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.rationale[0].source = None;
    decision.alternatives[0].reason.as_mut().unwrap().source = None;
    decision.implications[0].source = None;
    let missing = "is missing; an explicit statement needs a source";
    assert_eq!(
        problems(&document),
        [
            problem("decisions[0].rationale[0].source", missing),
            problem("decisions[0].alternatives[0].reason.source", missing),
            problem("decisions[0].implications[0].source", missing),
        ]
    );
}

#[test]
fn the_annotators_rationale_is_never_explicit() {
    let mut document = valid();
    let decision = &mut document.decisions[0];
    // Sourced from the report, which fits an inference.
    decision.rationale[2].explicit = true;
    decision.rationale[3].explicit = true;
    decision.alternatives[1].reason = Some(Rationale {
        explicit: true,
        ..inferred(Some(source("t-human", HUMAN_SAYS)))
    });
    let never = "true, but speaker annotator is never explicit";
    assert_eq!(
        problems(&document),
        [
            problem("decisions[0].rationale[2].explicit", never),
            problem("decisions[0].rationale[3].explicit", never),
            problem(
                "decisions[0].rationale[3].source",
                "is missing; an explicit statement needs a source"
            ),
            problem("decisions[0].alternatives[1].reason.explicit", never),
        ]
    );
}

#[test]
fn a_source_names_a_turn_of_the_session() {
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.rationale[0] = stated(Speaker::Human, "t-gone", HUMAN_SAYS);
    decision.implications[0].source = Some(source(EDIT, HUMAN_SAYS));
    assert_eq!(
        problems(&document),
        [
            problem(
                "decisions[0].rationale[0].source.turn",
                r#""t-gone" is not a turn of this session"#
            ),
            problem(
                "decisions[0].implications[0].source.turn",
                r#""edit:0123456789abcdef" is not a turn of this session"#
            ),
        ]
    );
}

#[test]
fn a_quote_has_one_to_the_most_characters() {
    let mut read = Read::new();
    let longest = "é".repeat(MAX_QUOTE_CHARS);
    read.observations
        .push(turn("t-long", Role::Human, &format!("{longest}é")));
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.rationale[0] = stated(Speaker::Human, "t-human", "");
    decision.rationale[1] = stated(Speaker::Human, "t-long", &format!("{longest}é"));
    // Both problems of a source are listed.
    decision.implications[0].source = Some(source("t-gone", ""));
    assert_eq!(
        read.problems(&document),
        [
            problem(
                "decisions[0].rationale[0].source.quote",
                "is empty; a quote has 1 to 2000 characters"
            ),
            problem(
                "decisions[0].rationale[1].source.quote",
                "has 2001 characters; at most 2000"
            ),
            problem(
                "decisions[0].implications[0].source.turn",
                r#""t-gone" is not a turn of this session"#
            ),
            problem(
                "decisions[0].implications[0].source.quote",
                "is empty; a quote has 1 to 2000 characters"
            ),
        ]
    );
    let mut document = valid();
    document.decisions[0].rationale[0] = stated(Speaker::Human, "t-long", &longest);
    assert_eq!(read.problems(&document), NO_PROBLEMS);
}

#[test]
fn a_report_backs_an_inference_but_no_explicit_statement() {
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.rationale[1] = stated(Speaker::Agent, "t-report", REPORT_SAYS);
    decision.implications[0].source = Some(source("t-report", REPORT_SAYS));
    let relayed = "a report is harness-relayed text, not known to be the agent's";
    assert_eq!(
        problems(&document),
        [
            problem(
                "decisions[0].rationale[1].source.quote",
                &format!(
                    r#"found only in report text of turn "t-report", but speaker agent needs agent or delegation text; {relayed}"#
                )
            ),
            problem(
                "decisions[0].implications[0].source.quote",
                &format!(
                    r#"found only in report text of turn "t-report", but an explicit implication needs human, agent or delegation text; {relayed}"#
                )
            ),
        ]
    );
    // The valid document's third rationale is the annotator's inference
    // from the same report; an inferred implication may cite it too.
    let mut document = valid();
    assert_eq!(
        document.decisions[0].rationale[2].source,
        Some(source("t-report", REPORT_SAYS))
    );
    document.decisions[0].implications[1].source = Some(source("t-report", REPORT_SAYS));
    assert_eq!(problems(&document), NO_PROBLEMS);
}

#[test]
fn a_quote_may_come_from_any_sighting_of_a_rewritten_turn() {
    let mut document = valid();
    document.decisions[0].rationale[1] = stated(Speaker::Agent, "t-agent", AGENT_REWRITTEN);
    assert_eq!(problems(&document), NO_PROBLEMS);
    // A quote spanning the two sightings is in neither.
    document.decisions[0].rationale[1] =
        stated(Speaker::Agent, "t-agent", "round trip. On reflection");
    assert_eq!(
        problems(&document),
        [problem(
            "decisions[0].rationale[1].source.quote",
            r#"not found verbatim in the retained text of turn "t-agent""#
        )]
    );
}

#[test]
fn a_quote_from_a_turn_without_retained_text_is_refused() {
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.rationale[0] = stated(Speaker::Human, "t-tool", "tool result");
    decision.rationale[2] = inferred(Some(source("t-tool", "tool result")));
    let unretained = r#"the text of turn "t-tool" is not retained"#;
    assert_eq!(
        problems(&document),
        [
            problem("decisions[0].rationale[0].source.quote", unretained),
            problem("decisions[0].rationale[2].source.quote", unretained),
        ]
    );
}

#[test]
fn a_quote_must_come_from_a_turn_whose_role_fits_the_speaker() {
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.rationale[0] = stated(Speaker::Agent, "t-human", HUMAN_SAYS);
    decision.rationale[1] = stated(Speaker::Human, "t-agent", AGENT_SAYS);
    decision.alternatives[0].reason = Some(stated(Speaker::Human, "t-delegation", DELEGATION_SAYS));
    assert_eq!(
        problems(&document),
        [
            problem(
                "decisions[0].rationale[0].source.quote",
                r#"found only in human text of turn "t-human", but speaker agent needs agent or delegation text"#
            ),
            problem(
                "decisions[0].rationale[1].source.quote",
                r#"found only in agent text of turn "t-agent", but speaker human needs human text"#
            ),
            problem(
                "decisions[0].alternatives[0].reason.source.quote",
                r#"found only in delegation text of turn "t-delegation", but speaker human needs human text"#
            ),
        ]
    );
}

#[test]
fn a_quote_must_occur_verbatim() {
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.rationale[0] = stated(Speaker::Human, "t-human", "Make the sync plan inspectable");
    decision.implications[0].source = Some(source("t-agent", "syncing  after"));
    assert_eq!(
        problems(&document),
        [
            problem(
                "decisions[0].rationale[0].source.quote",
                r#"not found verbatim in the retained text of turn "t-human""#
            ),
            problem(
                "decisions[0].implications[0].source.quote",
                r#"not found verbatim in the retained text of turn "t-agent""#
            ),
        ]
    );
}

#[test]
fn an_alternatives_status_matches_its_edits() {
    let mut document = valid();
    let decision = &mut document.decisions[0];
    decision.alternatives[0].edits.clear();
    decision.alternatives[1].edits = vec![eid(EDIT)];
    assert_eq!(
        problems(&document),
        [
            problem(
                "decisions[0].alternatives[0].status",
                "attempted, but it lists no edits; an attempted alternative has at least one"
            ),
            problem(
                "decisions[0].alternatives[1].status",
                "proposed, but it lists edits; a proposed alternative has none"
            ),
        ]
    );
}

#[test]
fn corrections_seen_are_judgment_ids_as_the_store_generates_them() {
    let refused = [
        "j1",
        "0B5F8E4C-3D2A-4F1B-9C8D-7E6F5A4B3C2D",
        "0b5f8e4c3d2a4f1b9c8d7e6f5a4b3c2d",
        "{0b5f8e4c-3d2a-4f1b-9c8d-7e6f5a4b3c2d}",
        "0b5f8e4c-3d2a-1f1b-9c8d-7e6f5a4b3c2d",
        "0b5f8e4c-3d2a-4f1b-7c8d-7e6f5a4b3c2d",
        "0b5f8e4c-3d2a-4f1b-9c8d-7e6f5a4b3c2",
        "",
    ];
    let document = AnnotationDocument {
        corrections_seen: [JUDGMENT]
            .into_iter()
            .chain(refused)
            .map(JudgmentId::new)
            .collect(),
        ..valid()
    };
    let expected: Vec<Problem> = refused
        .iter()
        .enumerate()
        .map(|(index, id)| {
            problem(
                &format!("corrections_seen[{}]", index + 1),
                &format!("{id:?} is not a judgment id"),
            )
        })
        .collect();
    assert_eq!(problems(&document), expected);
    let generated = AnnotationDocument {
        corrections_seen: (0..100).map(|_| JudgmentId::generate()).collect(),
        ..valid()
    };
    assert_eq!(problems(&generated), NO_PROBLEMS);
}

/// A document holding `more` entries past every list's bound: corrections,
/// decisions and questions, and in the first decision its cites,
/// rationale, alternatives, the first alternative's edits, and
/// implications, and in the first question its `about`. Every entry is
/// valid.
fn lists(more: usize) -> AnnotationDocument {
    let template = valid();
    let decision = &template.decisions[0];
    let per_decision = MAX_LIST_ENTRIES + more;
    let attempted = Alternative {
        edits: vec![eid(EDIT); per_decision],
        ..decision.alternatives[0].clone()
    };
    let first = Decision {
        id: "d0".to_owned(),
        cites: vec![eid(EDIT); per_decision],
        rationale: vec![inferred(None); per_decision],
        alternatives: [attempted]
            .into_iter()
            .chain(vec![decision.alternatives[1].clone(); per_decision - 1])
            .collect(),
        implications: vec![decision.implications[1].clone(); per_decision],
        ..decision.clone()
    };
    let decisions = [first]
        .into_iter()
        .chain((1..MAX_DECISIONS + more).map(|n| Decision {
            id: format!("d{n}"),
            ..decision.clone()
        }))
        .collect();
    let open_questions = (0..MAX_QUESTIONS + more)
        .map(|n| OpenQuestion {
            id: format!("q{n}"),
            about: if n == 0 {
                vec![eid(COMMAND); per_decision]
            } else {
                template.open_questions[0].about.clone()
            },
            ..template.open_questions[0].clone()
        })
        .collect();
    AnnotationDocument {
        corrections_seen: vec![JudgmentId::new(JUDGMENT); MAX_CORRECTIONS_SEEN + more],
        decisions,
        open_questions,
        ..template
    }
}

#[test]
fn lists_at_their_bounds_pass() {
    assert_eq!(problems(&lists(0)), NO_PROBLEMS);
}

#[test]
fn a_list_over_its_bound_is_one_problem_at_its_path() {
    assert_eq!(
        problems(&lists(1)),
        [
            problem("corrections_seen", "has 10001 entries; at most 10000"),
            problem("decisions", "has 10001 entries; at most 10000"),
            problem("decisions[0].cites", "has 1001 entries; at most 1000"),
            problem("decisions[0].rationale", "has 1001 entries; at most 1000"),
            problem(
                "decisions[0].alternatives",
                "has 1001 entries; at most 1000"
            ),
            problem(
                "decisions[0].alternatives[0].edits",
                "has 1001 entries; at most 1000"
            ),
            problem(
                "decisions[0].implications",
                "has 1001 entries; at most 1000"
            ),
            problem("open_questions", "has 10001 entries; at most 10000"),
            problem("open_questions[0].about", "has 1001 entries; at most 1000"),
        ]
    );
}

#[test]
fn every_problem_is_listed_in_document_order() {
    let mut document = valid();
    // Broken in another order than the document's.
    document.open_questions[0].id = "q 1".to_owned();
    document.decisions[0].rationale[1] = stated(Speaker::Agent, "t-human", HUMAN_SAYS);
    document.produced_by = " ".to_owned();
    document.decisions[0].title = String::new();
    document.schema = "supersigil.annotation/0".to_owned();
    let paths: Vec<String> = problems(&document)
        .into_iter()
        .map(|problem| problem.path)
        .collect();
    assert_eq!(
        paths,
        [
            "schema",
            "produced_by",
            "decisions[0].title",
            "decisions[0].rationale[1].source.quote",
            "open_questions[0].id",
        ]
    );
}

/// An explicit statement, which a quote must back.
#[derive(Debug, Clone, Copy)]
enum Statement {
    /// A rationale with speaker `human`.
    Human,
    /// A rationale with speaker `agent`.
    Agent,
    /// An implication.
    Implication,
}

impl Statement {
    /// Whether a sighting with `role` can back it, as the rules state.
    fn fits(self, role: Role) -> bool {
        match self {
            Self::Human => role == Role::Human,
            Self::Agent => matches!(role, Role::Agent | Role::Delegation),
            Self::Implication => matches!(role, Role::Human | Role::Agent | Role::Delegation),
        }
    }
}

fn statement() -> impl Strategy<Value = Statement> {
    prop_oneof![
        Just(Statement::Human),
        Just(Statement::Agent),
        Just(Statement::Implication),
    ]
}

/// A sighting's role, and its text when retained.
fn generated_sighting() -> impl Strategy<Value = (Role, Option<String>)> {
    let role = prop_oneof![
        Just(Role::Human),
        Just(Role::Agent),
        Just(Role::Delegation),
        Just(Role::Report),
        Just(Role::Tool),
    ];
    (role, proptest::option::of("[abé ]{0,12}"))
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    /// An explicit statement is accepted exactly when its quote occurs in
    /// the retained text of a sighting of its turn whose role fits it.
    #[test]
    fn an_explicit_statement_is_accepted_only_when_a_fitting_sighting_holds_its_quote(
        sightings in proptest::collection::vec(generated_sighting(), 1..4),
        quote in "[abé ]{1,4}",
        statement in statement(),
    ) {
        let read = Read {
            observations: sightings
                .iter()
                .map(|(role, text)| {
                    let text = text
                        .clone()
                        .map_or_else(|| Material::unavailable("generated"), Material::Retained);
                    sighting("t", *role, text)
                })
                .collect(),
            ..Read::new()
        };
        let mut decision = Decision {
            cites: Vec::new(),
            rationale: Vec::new(),
            alternatives: Vec::new(),
            implications: Vec::new(),
            ..valid().decisions[0].clone()
        };
        match statement {
            Statement::Human => decision.rationale.push(stated(Speaker::Human, "t", &quote)),
            Statement::Agent => decision.rationale.push(stated(Speaker::Agent, "t", &quote)),
            Statement::Implication => decision.implications.push(Implication {
                text: "it follows".to_owned(),
                explicit: true,
                source: Some(source("t", &quote)),
            }),
        }
        let document = AnnotationDocument {
            decisions: vec![decision],
            ..empty()
        };
        let fits = sightings.iter().any(|(role, text)| {
            statement.fits(*role) && text.as_ref().is_some_and(|text| text.contains(&quote))
        });
        let problems = read.problems(&document);
        prop_assert_eq!(problems.is_empty(), fits, "{:?}", problems);
        if !fits {
            prop_assert_eq!(problems.len(), 1);
            prop_assert!(problems[0].path.ends_with(".source.quote"), "{:?}", problems);
        }
    }
}

#[test]
fn ten_thousand_sourced_decisions_validate_within_the_time_limit() {
    const TURNS: usize = 2_000;
    const DECISIONS: usize = 10_000;
    let filler = "x".repeat(1_000);
    let mut observations = Vec::new();
    for n in 0..TURNS {
        let id = format!("t{n}");
        observations.push(turn(&id, Role::Agent, &format!("{filler} reason {n}.")));
        observations.push(edit(&format!("edit:{n:016x}"), &id));
    }
    let template = valid().decisions[0].clone();
    let decisions = (0..DECISIONS)
        .map(|index| {
            let n = index % TURNS;
            Decision {
                id: format!("d{index}"),
                cites: vec![eid(&format!("edit:{n:016x}"))],
                rationale: vec![stated(
                    Speaker::Agent,
                    &format!("t{n}"),
                    &format!("reason {n}."),
                )],
                alternatives: Vec::new(),
                implications: Vec::new(),
                ..template.clone()
            }
        })
        .collect();
    let document = AnnotationDocument {
        decisions,
        ..empty()
    };
    let read = Read {
        observations,
        ..Read::new()
    };
    let problems = finishes_within(Duration::from_secs(20), move || read.problems(&document));
    assert_eq!(problems, NO_PROBLEMS);
}
