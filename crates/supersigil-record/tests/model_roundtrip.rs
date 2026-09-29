//! Tests observation JSON formats and serialization round trips.

use std::collections::BTreeMap;
use std::path::PathBuf;

use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, Command, CommandCategory, Content, Edit, EndReason,
    FileState, Hunk, Material, Observation, Outcome, Role, SessionEnd, SessionStart, Source, Turn,
};
use supersigil_record::{ContentId, EventId, SessionId, Timestamp, TurnId};

fn sample() -> Vec<Observation> {
    let session = SessionId::new("11111111-1111-4111-8111-111111111111");
    let checkout = PathBuf::from("/work/repo");
    vec![
        Observation::SessionStart(SessionStart {
            session: session.clone(),
            source: Source::ClaudeCode,
            source_ids: BTreeMap::from([("transcript".to_owned(), "slice.jsonl".to_owned())]),
            checkout: checkout.clone(),
            branch: Some("main".to_owned()),
            time: Timestamp::new("2026-09-28T10:00:00.000Z"),
            sidechain: false,
        }),
        Observation::Turn(Turn {
            id: TurnId::new("u1"),
            session: session.clone(),
            parent: None,
            role: Role::Human,
            time: Timestamp::new("2026-09-28T10:00:00.000Z"),
            sidechain: false,
            agent_id: None,
            excerpt: Material::Retained("Add a greeting function and clean up.".to_owned()),
            source_ordinal: 0,
            transcript: None,
        }),
        Observation::Edit(Edit {
            id: EventId::derive("edit", &session, "toolu_01"),
            turn: TurnId::new("a1"),
            session: session.clone(),
            path: PathBuf::from("src/lib.rs"),
            before: FileState::known(ContentId::of(b"old\n")),
            after: FileState::Present {
                content: Content::Unknown,
            },
            patch: Material::Retained(vec![Hunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: vec!["-old".to_owned(), "+new".to_owned()],
            }]),
            old_text: Material::Retained("old\n".to_owned()),
            new_text: Material::Withheld {
                policy: "capture.edit_text".to_owned(),
            },
            replace_all: false,
            checkout: checkout.clone(),
            time: Timestamp::new("2026-09-28T10:00:06.000Z"),
            source_ordinal: 2,
            agent_id: None,
            transcript: None,
        }),
        Observation::Command(Command {
            id: EventId::derive("command", &session, "toolu_07"),
            turn: TurnId::new("a7"),
            session: session.clone(),
            cmd: "cargo nextest run".to_owned(),
            exit: None,
            stdout_tail: Material::Retained("3 tests run: 3 passed".to_owned()),
            stderr_tail: Material::Unavailable {
                reason: "not captured".to_owned(),
            },
            category: CommandCategory::TestRun,
            reported_error: false,
            outcome: Some(Outcome::Passed),
            started: Timestamp::new("2026-09-28T10:01:00.000Z"),
            ended: Some(Timestamp::new("2026-09-28T10:01:02.000Z")),
            checkout: checkout.clone(),
            source_ordinal: 14,
            agent_id: None,
            transcript: None,
        }),
        Observation::CaptureLimitation(CaptureLimitation {
            session: session.clone(),
            transcript: "slice.jsonl".to_owned(),
            from_ordinal: 0,
            to_ordinal: 19,
            counts: CaptureCounts {
                unknown_records: BTreeMap::from([("ai-title".to_owned(), 1)]),
                malformed_lines: 0,
                abandoned_tool_uses: 0,
                failed_tool_uses: 0,
                outside_checkout: 0,
                conflicting_tool_results: 0,
                unmatched_tool_results: 0,
                session_mismatch: 0,
                unnamed_tool_uses: 0,
                unsupported_tool_uses: 0,
            },
        }),
        Observation::SessionEnd(SessionEnd {
            session,
            time: Timestamp::new("2026-09-28T10:02:00.000Z"),
            reason: EndReason::Unknown,
        }),
    ]
}

#[test]
fn observation_json_shape_is_stable() {
    insta::assert_yaml_snapshot!(sample());
}

#[test]
fn observations_round_trip_through_json_lines() {
    let original = sample();
    let lines: Vec<String> = original
        .iter()
        .map(|o| serde_json::to_string(o).unwrap())
        .collect();
    let parsed: Vec<Observation> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(parsed, original);
    assert_eq!(
        parsed[2].session().as_str(),
        "11111111-1111-4111-8111-111111111111"
    );
}

#[test]
fn every_material_and_content_shape_serializes() {
    // Internally tagged newtype variants cannot carry strings or sequences;
    // these are the payloads that must keep working.
    let cases: Vec<serde_json::Value> = vec![
        serde_json::to_value(Material::Retained("text".to_owned())).unwrap(),
        serde_json::to_value(Material::Retained(vec![Hunk {
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: 1,
            lines: vec!["+x".to_owned()],
        }]))
        .unwrap(),
        serde_json::to_value(Material::<String>::Withheld {
            policy: "p".to_owned(),
        })
        .unwrap(),
        serde_json::to_value(Content::Known(ContentId::of(b"x"))).unwrap(),
        serde_json::to_value(Content::Unknown).unwrap(),
    ];
    assert_eq!(cases[0]["state"], "retained");
    assert_eq!(cases[0]["value"], "text");
    assert_eq!(cases[1]["value"][0]["lines"][0], "+x");
    assert_eq!(cases[2]["value"]["policy"], "p");
    assert_eq!(cases[3]["kind"], "known");
    assert!(cases[3]["id"].as_str().unwrap().starts_with("sha256:"));
    assert_eq!(cases[4]["kind"], "unknown");
    let command_json = serde_json::to_value(&sample()[3]).unwrap();
    assert_eq!(command_json["kind"], "command");
    assert_eq!(command_json["category"], "test_run");
}

#[test]
fn file_state_helpers_distinguish_absent_unknown_and_known() {
    let known = FileState::known(ContentId::of(b"x"));
    assert!(matches!(
        known,
        FileState::Present {
            content: Content::Known(_)
        }
    ));
    assert!(matches!(
        FileState::unknown(),
        FileState::Present {
            content: Content::Unknown
        }
    ));
    assert!(matches!(FileState::Absent, FileState::Absent));
    let retained = Material::Retained(1);
    assert_eq!(retained.retained(), Some(&1));
    let withheld: Material<i32> = Material::Withheld {
        policy: "p".to_owned(),
    };
    assert_eq!(withheld.retained(), None);
}
