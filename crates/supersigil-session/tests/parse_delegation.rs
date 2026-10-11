//! Tests how delegation between agents is parsed: the turn roles of relayed
//! records, and the starts, stops, and receives that order one session's
//! transcripts.

use serde_json::{Value, json};
use supersigil_record::observations::{Material, Observation, Role, Turn};
use supersigil_session::claude_code::{ParseOutcome, parse_transcript};

const SESSION: &str = "s";

fn turns(outcome: &ParseOutcome) -> Vec<&Turn> {
    outcome
        .observations
        .iter()
        .filter_map(|o| match o {
            Observation::Turn(t) => Some(t),
            _ => None,
        })
        .collect()
}

/// The JSONL text of `records`, one per line.
fn lines(records: &[Value]) -> String {
    let mut text = String::new();
    for record in records {
        text.push_str(&record.to_string());
        text.push('\n');
    }
    text
}

/// A record of `kind` in the main transcript, with `content` as its message.
fn main_record(kind: &str, uuid: &str, content: Value) -> Value {
    let mut record = json!({
        "type": kind, "uuid": uuid, "parentUuid": null, "sessionId": SESSION,
        "cwd": "/work/repo", "timestamp": "2026-10-02T10:00:00.000Z",
        "isSidechain": false, "message": {"role": kind}
    });
    record["message"]["content"] = content;
    record
}

/// A record of `kind` in the transcript of agent `agent`.
fn agent_record(agent: &str, kind: &str, uuid: &str, content: Value) -> Value {
    let mut record = main_record(kind, uuid, content);
    record["isSidechain"] = json!(true);
    record["agentId"] = json!(agent);
    record
}

/// A Bash call `id` issued by the main transcript.
fn bash_call(uuid: &str, id: &str) -> Value {
    main_record(
        "assistant",
        uuid,
        json!([{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": "ls"}}]),
    )
}

/// The result of the Bash call `id`.
fn bash_result(uuid: &str, id: &str) -> Value {
    let mut record = main_record(
        "user",
        uuid,
        json!([{"type": "tool_result", "tool_use_id": id, "content": "x"}]),
    );
    record["toolUseResult"] = json!({"stdout": "x", "stderr": "", "interrupted": false});
    record
}

/// The text of a task-notification for `task`, naming `call` when given.
fn notification_text(task: &str, call: Option<&str>, status: &str) -> String {
    let call = call.map_or_else(String::new, |c| format!("<tool-use-id>{c}</tool-use-id>\n"));
    format!(
        "<task-notification>\n<task-id>{task}</task-id>\n{call}<status>{status}</status>\n\
         <summary>Agent finished</summary>\n<result>Done.</result>\n</task-notification>"
    )
}

/// A delivered task-notification in the main transcript, as Claude Code
/// writes it when the issuer is idle.
fn delivered_notification(uuid: &str, task: &str, call: Option<&str>, status: &str) -> Value {
    let mut record = main_record("user", uuid, json!(notification_text(task, call, status)));
    record["origin"] = json!({"kind": "task-notification"});
    record
}

/// A hand-back in the main transcript: the agent's final report, relayed.
fn hand_back(uuid: &str, agent: &str) -> Value {
    let text = format!(
        "Another Claude session sent a message: <agent-message from=\"{agent}\"> \
         [Subagent hand-back] The report follows:\n  - Status: DONE</agent-message>"
    );
    let mut record = main_record("user", uuid, json!(text));
    record["isMeta"] = json!(true);
    record["origin"] = json!({"kind": "peer", "from": agent, "handback": true});
    record
}

/// The framing Claude Code puts around a message that resumes an agent.
fn coordinator_text(message: &str) -> String {
    format!(
        "The coordinator sent a message while you were working:\n{message}\n\n\
         Address this before completing your current task."
    )
}

/// A resume message as the resumed agent's transcript records it.
fn coordinator(agent: &str, uuid: &str, message: &str) -> Value {
    let mut record = agent_record(agent, "user", uuid, json!(coordinator_text(message)));
    record["isMeta"] = json!(true);
    record["origin"] = json!({"kind": "coordinator"});
    record
}

/// A task-notification as an agent's transcript records it: meta, with the
/// harness's warning prefix.
fn agent_notification(agent: &str, uuid: &str, task: &str) -> Value {
    let text = format!(
        "[SYSTEM NOTIFICATION - NOT USER INPUT] This is an automated background-task event.\n\n{}",
        notification_text(task, Some("toolu_bg"), "completed")
    );
    let mut record = agent_record(agent, "user", uuid, json!(text));
    record["isMeta"] = json!(true);
    record["origin"] = json!({"kind": "task-notification"});
    record
}

#[test]
fn relayed_reports_are_report_turns_with_truncated_excerpts() {
    let text = lines(&[
        main_record("user", "u0", json!("Start.")),
        delivered_notification("n1", "a1", Some("toolu_a"), "completed"),
        hand_back("h1", "a1"),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    let roles: Vec<Role> = turns(&outcome).iter().map(|t| t.role).collect();
    assert_eq!(roles, vec![Role::Human, Role::Report, Role::Report]);
    let report = turns(&outcome)[1];
    let Material::Retained(excerpt) = &report.excerpt else {
        panic!("a report keeps its text: {:?}", report.excerpt);
    };
    assert!(excerpt.starts_with("<task-notification>"), "{excerpt}");
    let long = "x".repeat(500);
    let mut record = main_record("user", "n2", json!(long));
    record["origin"] = json!({"kind": "task-notification"});
    let outcome = parse_transcript(lines(&[record]).as_bytes(), 0);
    assert_eq!(
        turns(&outcome)[0].excerpt,
        Material::Retained("x".repeat(240))
    );
}

#[test]
fn a_report_arriving_mid_turn_abandons_no_tool_call() {
    let text = lines(&[
        bash_call("a1", "t1"),
        delivered_notification("n1", "b1", Some("toolu_b"), "completed"),
        bash_result("r1", "t1"),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    assert_eq!(outcome.counts.abandoned_tool_uses, 0);
    assert_eq!(outcome.counts.unmatched_tool_results, 0);
    assert_eq!(outcome.consumed, text.len() as u64);
}

#[test]
fn a_notification_in_an_agent_transcript_is_a_report() {
    let text = lines(&[
        agent_record("a1", "user", "p0", json!("Do the task.")),
        agent_notification("a1", "n1", "bg1"),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    let roles: Vec<Role> = turns(&outcome).iter().map(|t| t.role).collect();
    assert_eq!(roles, vec![Role::Delegation, Role::Report]);
}

#[test]
fn a_resume_message_is_a_delegation_turn_with_its_excerpt() {
    let text = lines(&[
        agent_record("a1", "user", "p0", json!("Do the task.")),
        agent_record(
            "a1",
            "assistant",
            "x1",
            json!([{"type": "text", "text": "Done."}]),
        ),
        coordinator("a1", "c1", "Fix round 1."),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    let resume = turns(&outcome)[2];
    assert_eq!(resume.role, Role::Delegation);
    assert_eq!(
        resume.excerpt,
        Material::Retained(coordinator_text("Fix round 1."))
    );
}
