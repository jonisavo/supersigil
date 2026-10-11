//! Tests how delegation between agents is parsed: the turn roles of relayed
//! records, and the starts, stops, and receives that order one session's
//! transcripts.

use serde_json::{Value, json};
use supersigil_record::observations::{
    Delegation, DelegationStep, Material, Observation, Role, StartKind, StopStatus, Turn,
};
use supersigil_record::{ContentId, EventId, SessionId};
use supersigil_session::claude_code::{ParseOutcome, parse_transcript};

const SESSION: &str = "s";
/// The prompt every test record belongs to (`promptId`).
const PROMPT: &str = "p1";

fn delegations(outcome: &ParseOutcome) -> Vec<&Delegation> {
    outcome
        .observations
        .iter()
        .filter_map(|o| match o {
            Observation::Delegation(d) => Some(d),
            _ => None,
        })
        .collect()
}

/// Each delegation step with the ordinal it is placed at.
fn steps(outcome: &ParseOutcome) -> Vec<(u64, DelegationStep)> {
    delegations(outcome)
        .into_iter()
        .map(|d| (d.source_ordinal, d.step.clone()))
        .collect()
}

fn start(
    agent: Option<&str>,
    call: &str,
    kind: StartKind,
    message: Option<&str>,
) -> DelegationStep {
    DelegationStep::Start {
        agent: agent.map(str::to_owned),
        call: call.to_owned(),
        kind,
        message: message.map(|m| ContentId::of(m.as_bytes())),
        prompt: Some(PROMPT.to_owned()),
    }
}

fn stop(agent: &str, call: &str, status: StopStatus) -> DelegationStep {
    DelegationStep::Stop {
        agent: agent.to_owned(),
        call: call.to_owned(),
        status,
    }
}

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
        "isSidechain": false, "promptId": PROMPT, "message": {"role": kind}
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

/// A call `id` to tool `name` issued by the main transcript.
fn call(uuid: &str, name: &str, id: &str, input: Value) -> Value {
    let mut record = main_record(
        "assistant",
        uuid,
        json!([{"type": "tool_use", "id": id, "name": name}]),
    );
    record["message"]["content"][0]["input"] = input;
    record
}

/// An Agent call `id` dispatching an agent in the background.
fn dispatch(uuid: &str, id: &str) -> Value {
    call(
        uuid,
        "Agent",
        id,
        json!({"description": "Task 1", "prompt": "Do task 1.", "subagent_type": "general-purpose"}),
    )
}

/// The result of call `id`: `text` as its content, `structured` as its
/// `toolUseResult`, marked as an error when `error`.
fn result(uuid: &str, id: &str, text: &str, structured: Option<Value>, error: bool) -> Value {
    let mut record = main_record(
        "user",
        uuid,
        json!([{"type": "tool_result", "tool_use_id": id, "content": text, "is_error": error}]),
    );
    if let Some(structured) = structured {
        record["toolUseResult"] = structured;
    }
    record
}

/// The result of an Agent call `id` that launched `agent` in the
/// background, as Claude Code records it.
fn launched(uuid: &str, id: &str, agent: &str) -> Value {
    let text = format!(
        "Async agent launched successfully. agentId: {agent} (internal ID - do not mention to user.)"
    );
    result(
        uuid,
        id,
        &text,
        Some(json!({"status": "async_launched", "agentId": agent, "isAsync": true})),
        false,
    )
}

/// A `SendMessage` call `id` to `to` carrying `message`.
fn send(uuid: &str, id: &str, to: &str, message: &str) -> Value {
    call(
        uuid,
        "SendMessage",
        id,
        json!({"to": to, "summary": "Fix round", "message": message}),
    )
}

/// The result of a `SendMessage` call `id` that resumed `agent`.
fn resumed(uuid: &str, id: &str, agent: &str) -> Value {
    result(
        uuid,
        id,
        "Resuming agent",
        Some(json!({"success": true, "message": "Resuming agent", "resumedAgentId": agent})),
        false,
    )
}

/// The `queue-operation` record that enqueues `text` for the main
/// transcript: a notification, a hand-back, or a message typed meanwhile
/// alike.
fn enqueue(text: &str) -> Value {
    json!({
        "type": "queue-operation", "operation": "enqueue",
        "timestamp": "2026-10-02T10:00:00.000Z", "sessionId": SESSION, "content": text
    })
}

/// The `queued_command` attachment delivering `text` into a running turn,
/// with the harness's `commandMode`: `task-notification` for a
/// notification, `prompt` for a hand-back or a message typed meanwhile.
fn queued(uuid: &str, mode: &str, text: &str) -> Value {
    json!({
        "type": "attachment", "uuid": uuid, "parentUuid": null, "sessionId": SESSION,
        "cwd": "/work/repo", "timestamp": "2026-10-02T10:00:00.000Z", "isSidechain": false,
        "attachment": {"type": "queued_command", "prompt": text, "commandMode": mode}
    })
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

#[test]
fn a_background_dispatch_starts_its_agent_at_the_call() {
    let text = lines(&[
        main_record("user", "u0", json!("Start.")),
        dispatch("a1", "toolu_d"),
        launched("r1", "toolu_d", "ag1"),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    assert_eq!(
        steps(&outcome),
        vec![(1, start(Some("ag1"), "toolu_d", StartKind::Dispatch, None))]
    );
    let d = delegations(&outcome)[0];
    assert_eq!(
        d.id,
        EventId::derive("agent-start", &SessionId::new(SESSION), "toolu_d")
    );
    assert_eq!(d.session, SessionId::new(SESSION));
    assert_eq!(d.transcript, None);
}

#[test]
fn a_synchronous_dispatch_starts_and_stops_at_its_result() {
    let completed = result(
        "r1",
        "toolu_d",
        "Done.",
        Some(
            json!({"status": "completed", "agentId": "ag1", "content": [{"type": "text", "text": "Done."}]}),
        ),
        false,
    );
    let text = lines(&[dispatch("a1", "toolu_d"), completed]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    assert_eq!(
        steps(&outcome),
        vec![
            (0, start(Some("ag1"), "toolu_d", StartKind::Dispatch, None)),
            (1, stop("ag1", "toolu_d", StopStatus::Completed)),
        ]
    );
    let ids: Vec<&EventId> = delegations(&outcome).iter().map(|d| &d.id).collect();
    assert_eq!(
        ids[1],
        &EventId::derive("agent-stop", &SessionId::new(SESSION), "toolu_d")
    );
}

#[test]
fn an_agent_named_only_in_the_result_text_is_not_started() {
    // Only `toolUseResult.agentId` names the agent: result text can quote
    // any ID, as a synchronous agent's report does.
    for (text, structured) in [
        (
            "Async agent launched successfully. agentId: a5d1578534c664e12 (internal ID)",
            None,
        ),
        (
            "The log contains agentId: a5d1578534c664e12.",
            Some(json!({"status": "completed"})),
        ),
    ] {
        let text = lines(&[
            dispatch("a1", "toolu_d"),
            result("r1", "toolu_d", text, structured, false),
        ]);
        let outcome = parse_transcript(text.as_bytes(), 0);
        assert_eq!(steps(&outcome), vec![]);
    }
}

#[test]
fn a_worker_fork_starts_without_a_named_agent() {
    let fork = call(
        "a1",
        "Agent",
        "toolu_f",
        json!({"description": "Fork A", "prompt": "Write tasks.", "subagent_type": "fork"}),
    );
    let text = lines(&[
        fork,
        result(
            "r1",
            "toolu_f",
            "Fork started — processing in background",
            None,
            false,
        ),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    assert_eq!(
        steps(&outcome),
        vec![(0, start(None, "toolu_f", StartKind::Dispatch, None))]
    );
}

#[test]
fn an_agent_call_that_names_no_agent_or_failed_starts_nothing() {
    let failed = lines(&[
        dispatch("a1", "toolu_d"),
        result(
            "r1",
            "toolu_d",
            "Agent terminated early due to an API error",
            None,
            true,
        ),
    ]);
    assert!(steps(&parse_transcript(failed.as_bytes(), 0)).is_empty());
    let unnamed = lines(&[
        dispatch("a1", "toolu_d"),
        result(
            "r1",
            "toolu_d",
            "Something else",
            Some(json!({"status": "x"})),
            false,
        ),
    ]);
    assert!(steps(&parse_transcript(unnamed.as_bytes(), 0)).is_empty());
}

#[test]
fn a_resume_starts_its_agent_with_the_message_hash() {
    let text = lines(&[
        send("a1", "toolu_s", "ag1", "Fix round 1."),
        resumed("r1", "toolu_s", "ag1"),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    assert_eq!(
        steps(&outcome),
        vec![(
            0,
            start(
                Some("ag1"),
                "toolu_s",
                StartKind::Resume,
                Some("Fix round 1.")
            )
        )]
    );
}

#[test]
fn a_message_queued_for_a_running_agent_starts_nothing() {
    let text = lines(&[
        send("a1", "toolu_s", "ag1", "One more thing."),
        result(
            "r1",
            "toolu_s",
            "Message queued for delivery to ag1 at its next tool round.",
            Some(
                json!({"success": true, "message": "Message queued for delivery to ag1 at its next tool round."}),
            ),
            false,
        ),
    ]);
    assert!(steps(&parse_transcript(text.as_bytes(), 0)).is_empty());
}

#[test]
fn a_start_moves_back_to_an_edit_still_in_flight() {
    // One message issues an Edit and then an Agent call; the Edit's result
    // comes after the Agent call, so the edit may not have finished before
    // the agent started.
    let edit = call(
        "a1",
        "Edit",
        "toolu_e",
        json!({"file_path": "/work/repo/f.txt", "old_string": "a", "new_string": "b"}),
    );
    let text = lines(&[
        edit,
        dispatch("a2", "toolu_d"),
        launched("r1", "toolu_d", "ag1"),
        result("r2", "toolu_e", "updated", None, false),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    assert_eq!(
        steps(&outcome),
        vec![(0, start(Some("ag1"), "toolu_d", StartKind::Dispatch, None))]
    );
}

#[test]
fn a_stop_is_placed_at_its_first_delivered_notification() {
    // The enqueue comes first but proves nothing: a message typed while the
    // issuer is busy is enqueued the same way. The harness marks only the
    // deliveries as notifications.
    let notice = notification_text("ag1", Some("toolu_d"), "completed");
    let text = lines(&[
        dispatch("a1", "toolu_d"),
        launched("r1", "toolu_d", "ag1"),
        enqueue(&notice),
        queued("q1", "task-notification", &notice),
        delivered_notification("n1", "ag1", Some("toolu_d"), "completed"),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    assert_eq!(
        steps(&outcome),
        vec![
            (0, start(Some("ag1"), "toolu_d", StartKind::Dispatch, None)),
            (3, stop("ag1", "toolu_d", StopStatus::Completed)),
        ]
    );
    // A later parse that starts past the first delivery records no second
    // stop.
    let later = parse_transcript(text.as_bytes(), 4);
    assert!(steps(&later).is_empty());
}

#[test]
fn each_form_of_a_notification_proves_a_stop() {
    let notice = notification_text("ag1", Some("toolu_d"), "failed");
    let head = || vec![dispatch("a1", "toolu_d"), launched("r1", "toolu_d", "ag1")];
    for (form, record) in [
        ("attachment", queued("q1", "task-notification", &notice)),
        (
            "delivered",
            delivered_notification("n1", "ag1", Some("toolu_d"), "failed"),
        ),
    ] {
        let mut records = head();
        records.push(record);
        let outcome = parse_transcript(lines(&records).as_bytes(), 0);
        assert_eq!(
            steps(&outcome)[1..],
            [(2, stop("ag1", "toolu_d", StopStatus::Failed))],
            "{form}"
        );
    }
}

#[test]
fn a_resumed_segment_stops_under_its_resume_call() {
    let text = lines(&[
        send("a1", "toolu_s", "ag1", "Fix it."),
        resumed("r1", "toolu_s", "ag1"),
        delivered_notification("n1", "ag1", Some("toolu_s"), "completed"),
    ]);
    let outcome = parse_transcript(text.as_bytes(), 0);
    assert_eq!(
        steps(&outcome)[1..],
        [(2, stop("ag1", "toolu_s", StopStatus::Completed))]
    );
}

#[test]
fn notifications_that_prove_no_usable_stop_record_nothing() {
    let head = || vec![dispatch("a1", "toolu_d"), launched("r1", "toolu_d", "ag1")];
    let cases = [
        // An interrupted agent may have had a tool call in flight, and the
        // first proof decides: a later notification of that call adds
        // nothing.
        (
            "killed",
            vec![
                delivered_notification("n1", "ag1", Some("toolu_d"), "killed"),
                delivered_notification("n2", "ag1", Some("toolu_d"), "completed"),
            ],
        ),
        // A woken agent notifies again without naming its start.
        (
            "no call",
            vec![delivered_notification("n1", "ag1", None, "completed")],
        ),
        // An enqueue alone: a typed message is enqueued the same way.
        (
            "enqueued",
            vec![enqueue(&notification_text(
                "ag1",
                Some("toolu_d"),
                "completed",
            ))],
        ),
        // A background shell task names its Bash call.
        (
            "shell task",
            vec![
                bash_call("b1", "toolu_b"),
                bash_result("b2", "toolu_b"),
                delivered_notification("n1", "bg1", Some("toolu_b"), "completed"),
            ],
        ),
        (
            "unknown call",
            vec![delivered_notification(
                "n1",
                "ag1",
                Some("toolu_x"),
                "completed",
            )],
        ),
        // A hand-back proves only that the agent called the hand-back tool,
        // even when its report quotes a notification.
        ("hand-back", vec![hand_back("h1", "ag1")]),
        (
            "quoted in a hand-back",
            vec![queued(
                "q1",
                "prompt",
                &format!(
                    "<agent-message from=\"ag1\"> [Subagent hand-back] I saw: {}</agent-message>",
                    notification_text("ag1", Some("toolu_d"), "completed")
                ),
            )],
        ),
        // A message typed while the issuer was busy is no notification.
        (
            "typed prompt",
            vec![queued(
                "q1",
                "prompt",
                &notification_text("ag1", Some("toolu_d"), "completed"),
            )],
        ),
    ];
    for (case, tail) in cases {
        let mut records = head();
        records.extend(tail);
        let outcome = parse_transcript(lines(&records).as_bytes(), 0);
        assert_eq!(steps(&outcome).len(), 1, "{case}: {:?}", steps(&outcome));
    }
}

#[test]
fn a_notification_in_an_agent_transcript_stops_its_nested_agent() {
    let mut nested = call(
        "x1",
        "Agent",
        "toolu_n",
        json!({"description": "Nested", "prompt": "Go.", "subagent_type": "general-purpose"}),
    );
    nested["isSidechain"] = json!(true);
    nested["agentId"] = json!("a1");
    let mut launch = launched("x2", "toolu_n", "ag2");
    launch["isSidechain"] = json!(true);
    launch["agentId"] = json!("a1");
    let mut notice = agent_notification("a1", "x3", "ag2");
    let text = notice["message"]["content"]
        .as_str()
        .unwrap()
        .replace("toolu_bg", "toolu_n");
    notice["message"]["content"] = json!(text);
    let records = [
        agent_record("a1", "user", "p0", json!("Do the task.")),
        nested,
        launch,
        notice,
    ];
    let outcome = parse_transcript(lines(&records).as_bytes(), 0);
    assert!(
        steps(&outcome).contains(&(3, stop("ag2", "toolu_n", StopStatus::Completed))),
        "{:?}",
        steps(&outcome)
    );
}

#[test]
fn a_start_carries_the_prompt_of_its_result() {
    let mut launch = launched("r1", "toolu_d", "ag1");
    launch["promptId"] = json!("p7");
    let outcome = parse_transcript(lines(&[dispatch("a1", "toolu_d"), launch]).as_bytes(), 0);
    let DelegationStep::Start { prompt, .. } = &delegations(&outcome)[0].step else {
        panic!("a start: {:?}", steps(&outcome));
    };
    assert_eq!(prompt.as_deref(), Some("p7"));
}

#[test]
fn an_empty_prompt_is_no_prompt() {
    let mut launch = launched("r1", "toolu_d", "ag1");
    launch["promptId"] = json!("");
    let outcome = parse_transcript(lines(&[dispatch("a1", "toolu_d"), launch]).as_bytes(), 0);
    let DelegationStep::Start { prompt, .. } = &delegations(&outcome)[0].step else {
        panic!("a start: {:?}", steps(&outcome));
    };
    assert_eq!(*prompt, None);
}

#[test]
fn many_calls_in_flight_are_placed_promptly() {
    // One record issuing 200,000 calls: finding the earliest call in flight
    // by scanning them all for each call would take minutes.
    let blocks: Vec<Value> = (0..200_000)
        .map(|i| {
            json!({"type": "tool_use", "id": format!("toolu_{i}"), "name": "Bash",
            "input": {"command": "true"}})
        })
        .collect();
    let text = lines(&[main_record("assistant", "a1", json!(blocks))]);
    let (done, parsed) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(parse_transcript(text.as_bytes(), 0).observations.len());
    });
    parsed
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("parsing finishes within its time limit");
}
