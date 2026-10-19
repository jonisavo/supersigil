//! Tests the annotation document's JSON form: field names, omitted empty
//! lists and sources, refused unknown fields, and the stored revision.

use serde_json::{Value, json};
use supersigil_record::interpretations::{
    ANNOTATION_SCHEMA, Alternative, AlternativeStatus, Annotation, AnnotationDocument,
    AnnotationRevision, Decision, Implication, Observed, OpenQuestion, Prepared, Rationale, Source,
    Speaker,
};
use supersigil_record::{EventId, JudgmentId, RecordId, Revision, SessionId, TurnId};

/// A document without decisions, questions or corrections, built on no
/// revision.
fn minimal_document() -> AnnotationDocument {
    AnnotationDocument {
        schema: ANNOTATION_SCHEMA.to_owned(),
        session: SessionId::new("s1"),
        based_on: None,
        prepared: Prepared {
            record: RecordId::new("r1"),
            observation_revision: Revision::ZERO.next(),
            observed: Observed {
                count: 0,
                last: None,
            },
        },
        produced_by: "claude-code/supersigil-annotate".to_owned(),
        corrections_seen: Vec::new(),
        decisions: Vec::new(),
        open_questions: Vec::new(),
    }
}

/// A document using every speaker and alternative status, with a source on
/// each kind of entry that takes one.
fn full_document() -> AnnotationDocument {
    let quoted = |speaker, turn: &str, quote: &str| Rationale {
        text: quote.to_owned(),
        speaker,
        explicit: true,
        source: Some(Source {
            turn: TurnId::new(turn),
            quote: quote.to_owned(),
        }),
    };
    AnnotationDocument {
        based_on: Some(AnnotationRevision::FIRST.next()),
        prepared: Prepared {
            record: RecordId::new("r1"),
            observation_revision: Revision::ZERO.next(),
            observed: Observed {
                count: 100,
                last: Some("2026-10-15T14:02:11Z".parse().unwrap()),
            },
        },
        corrections_seen: vec![JudgmentId::new("j1")],
        decisions: vec![Decision {
            id: "d1".to_owned(),
            title: "Publish both stores through one sync plan".to_owned(),
            cites: vec![
                EventId::new("edit:0123456789abcdef"),
                EventId::new("command:fedcba9876543210"),
            ],
            rationale: vec![
                quoted(Speaker::Human, "u1", "make the sync plan inspectable"),
                Rationale {
                    text: "one plan is easier to review".to_owned(),
                    speaker: Speaker::Annotator,
                    explicit: false,
                    source: None,
                },
            ],
            alternatives: vec![
                Alternative {
                    title: "Per-store syncs".to_owned(),
                    status: AlternativeStatus::Attempted,
                    edits: vec![EventId::new("edit:1111111111111111")],
                    reason: Some(quoted(Speaker::Agent, "a1", "syncing each file costs more")),
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
                    text: "every new file kind needs a step".to_owned(),
                    explicit: false,
                    source: None,
                },
                Implication {
                    text: "readers keep their plan".to_owned(),
                    explicit: true,
                    source: Some(Source {
                        turn: TurnId::new("a1"),
                        quote: "readers keep their plan".to_owned(),
                    }),
                },
            ],
        }],
        open_questions: vec![OpenQuestion {
            id: "q1".to_owned(),
            text: "Why keep the old plan?".to_owned(),
            about: vec![EventId::new("edit:0123456789abcdef")],
        }],
        ..minimal_document()
    }
}

#[test]
fn a_document_with_every_variant_round_trips_in_the_documented_shape() {
    let document = full_document();
    let value = serde_json::to_value(&document).unwrap();
    assert_eq!(
        value,
        json!({
            "schema": "supersigil.annotation/1",
            "session": "s1",
            "based_on": 2,
            "prepared": {
                "record": "r1",
                "observation_revision": 1,
                "observed": { "count": 100, "last": "2026-10-15T14:02:11Z" }
            },
            "produced_by": "claude-code/supersigil-annotate",
            "corrections_seen": ["j1"],
            "decisions": [{
                "id": "d1",
                "title": "Publish both stores through one sync plan",
                "cites": ["edit:0123456789abcdef", "command:fedcba9876543210"],
                "rationale": [
                    { "text": "make the sync plan inspectable", "speaker": "human",
                      "explicit": true,
                      "source": { "turn": "u1", "quote": "make the sync plan inspectable" } },
                    { "text": "one plan is easier to review", "speaker": "annotator",
                      "explicit": false }
                ],
                "alternatives": [
                    { "title": "Per-store syncs", "status": "attempted",
                      "edits": ["edit:1111111111111111"],
                      "reason": { "text": "syncing each file costs more", "speaker": "agent",
                                  "explicit": true,
                                  "source": { "turn": "a1",
                                              "quote": "syncing each file costs more" } } },
                    { "title": "No sync at all", "status": "proposed" }
                ],
                "implications": [
                    { "text": "every new file kind needs a step", "explicit": false },
                    { "text": "readers keep their plan", "explicit": true,
                      "source": { "turn": "a1", "quote": "readers keep their plan" } }
                ]
            }],
            "open_questions": [
                { "id": "q1", "text": "Why keep the old plan?",
                  "about": ["edit:0123456789abcdef"] }
            ]
        })
    );
    assert_eq!(
        serde_json::from_value::<AnnotationDocument>(value).unwrap(),
        document
    );
}

#[test]
fn empty_lists_are_omitted_and_read_back_empty() {
    let document = AnnotationDocument {
        decisions: vec![Decision {
            id: "d1".to_owned(),
            title: "History only".to_owned(),
            cites: Vec::new(),
            rationale: Vec::new(),
            alternatives: Vec::new(),
            implications: Vec::new(),
        }],
        open_questions: vec![OpenQuestion {
            id: "q1".to_owned(),
            text: "Was this intended?".to_owned(),
            about: Vec::new(),
        }],
        ..minimal_document()
    };
    let value = serde_json::to_value(&document).unwrap();
    assert_eq!(
        value,
        json!({
            "schema": "supersigil.annotation/1",
            "session": "s1",
            "based_on": null,
            "prepared": {
                "record": "r1",
                "observation_revision": 1,
                "observed": { "count": 0, "last": null }
            },
            "produced_by": "claude-code/supersigil-annotate",
            "decisions": [{ "id": "d1", "title": "History only" }],
            "open_questions": [{ "id": "q1", "text": "Was this intended?" }]
        })
    );
    assert_eq!(
        serde_json::from_value::<AnnotationDocument>(value).unwrap(),
        document
    );
    let bare = json!({
        "schema": "supersigil.annotation/1",
        "session": "s1",
        "based_on": null,
        "prepared": {
            "record": "r1",
            "observation_revision": 1,
            "observed": { "count": 0, "last": null }
        },
        "produced_by": "claude-code/supersigil-annotate"
    });
    assert_eq!(
        serde_json::from_value::<AnnotationDocument>(bare).unwrap(),
        minimal_document()
    );
}

#[test]
fn an_unknown_field_is_refused_anywhere_in_the_document() {
    let objects = [
        "",
        "/prepared",
        "/prepared/observed",
        "/decisions/0",
        "/decisions/0/rationale/0",
        "/decisions/0/rationale/0/source",
        "/decisions/0/alternatives/0",
        "/decisions/0/alternatives/0/reason",
        "/decisions/0/implications/1",
        "/decisions/0/implications/1/source",
        "/open_questions/0",
    ];
    for pointer in objects {
        let mut value = serde_json::to_value(full_document()).unwrap();
        let object = value.pointer_mut(pointer).and_then(Value::as_object_mut);
        object.unwrap().insert("extra".to_owned(), json!(1));
        let error = serde_json::from_value::<AnnotationDocument>(value).unwrap_err();
        assert!(
            error.to_string().contains("unknown field `extra`"),
            "{pointer}: {error}"
        );
    }
}

#[test]
fn revisions_count_from_one_and_are_written_as_numbers() {
    let second = AnnotationRevision::FIRST.next();
    assert_eq!(AnnotationRevision::FIRST.get(), 1);
    assert_eq!(second.get(), 2);
    assert!(AnnotationRevision::FIRST < second);
    assert_eq!(second.to_string(), "2");
    assert_eq!(serde_json::to_value(second).unwrap(), json!(2));
    assert_eq!(
        serde_json::from_value::<AnnotationRevision>(json!(2)).unwrap(),
        second
    );
}

#[test]
fn the_stored_form_holds_the_document_unchanged_beside_its_revision_and_time() {
    let annotation = Annotation {
        revision: AnnotationRevision::FIRST,
        at: "2026-10-15T14:05:00Z".parse().unwrap(),
        document: full_document(),
    };
    let value = serde_json::to_value(&annotation).unwrap();
    assert_eq!(
        value,
        json!({
            "revision": 1,
            "at": "2026-10-15T14:05:00Z",
            "document": serde_json::to_value(full_document()).unwrap()
        })
    );
    assert_eq!(
        serde_json::from_value::<Annotation>(value).unwrap(),
        annotation
    );
}
