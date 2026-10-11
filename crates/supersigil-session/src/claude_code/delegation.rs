//! Reads how a transcript hands work to other agents: the calls that start
//! an agent's work and the notifications that prove it stopped.
//!
//! Claude Code starts an agent with an `Agent` call, whose result names the
//! agent, and resumes one with a `SendMessage` call, whose result names
//! `resumedAgentId`. When an agent stops, the harness writes a
//! task-notification naming the agent (`<task-id>`), the call whose work
//! ended (`<tool-use-id>`), and a `<status>`. The issuer's transcript holds
//! it in a delivered `user` record marked as a task-notification or a
//! `queued_command` attachment in `task-notification` mode, each beginning
//! with the notification's envelope. An earlier `queue-operation` enqueue of
//! the same text proves nothing: typed messages are enqueued the same way. A
//! hand-back message is no proof either: the agent sends it by calling its
//! hand-back tool, before it stops, and its report can quote any text, a
//! notification included.

use serde_json::Value;
use supersigil_record::observations::{
    Delegation, DelegationStep, Observation, StartKind, StopStatus,
};
use supersigil_record::{ContentId, EventId, SessionId};

/// What the harness writes before a task-notification it delivers into an
/// agent's own transcript.
const NOTIFICATION_WARNING: &str = "[SYSTEM NOTIFICATION - NOT USER INPUT]";

/// Tools whose calls can start an agent's work.
pub(super) const STARTING_TOOLS: &[&str] = &["Agent", "Task", "SendMessage"];

/// The header of a task-notification.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Notification<'a> {
    /// The task that stopped: an agent, or a background shell task.
    pub task: &'a str,
    /// The tool-use ID of the call whose work ended, if named.
    pub call: Option<&'a str>,
    /// The stated status, if any.
    pub status: Option<&'a str>,
}

/// Reads the header of the task-notification `text` begins with, after
/// any leading whitespace. Only the part before the first `<summary>`,
/// `<note>`, or `<result>` is read: the summary quotes the call's
/// description, and the result the agent's own report, either of which can
/// hold any text. `None` when `text` does not begin with the envelope or it
/// names no task.
pub(super) fn notification(text: &str) -> Option<Notification<'_>> {
    let rest = text.trim_start().strip_prefix("<task-notification>")?;
    let end = ["<summary>", "<note>", "<result>"]
        .iter()
        .filter_map(|open| rest.find(open))
        .min()
        .unwrap_or(rest.len());
    let header = &rest[..end];
    Some(Notification {
        task: tag(header, "task-id")?,
        call: tag(header, "tool-use-id"),
        status: tag(header, "status"),
    })
}

/// The envelope of a delivered notification record's `text`: the text
/// itself, or in an agent's transcript what follows the harness's fixed
/// warning.
pub(super) fn delivered_envelope(text: &str) -> &str {
    text.strip_prefix(NOTIFICATION_WARNING)
        .and_then(|rest| rest.find("<task-notification>").map(|at| &rest[at..]))
        .unwrap_or(text)
}

/// The trimmed text of the first `<name>` element in `text`.
fn tag<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let from = text.find(&open)? + open.len();
    let length = text[from..].find(&format!("</{name}>"))?;
    Some(text[from..from + length].trim()).filter(|value| !value.is_empty())
}

/// The status a stop records. `None` for every status but completed and
/// failed: an interrupted agent may have had a tool call in flight.
pub(super) fn stop_status(status: Option<&str>) -> Option<StopStatus> {
    match status {
        Some("completed") => Some(StopStatus::Completed),
        Some("failed") => Some(StopStatus::Failed),
        _ => None,
    }
}

/// The agent a dispatch's result names: `toolUseResult.agentId`. Result
/// text is never read for it: a synchronous agent's report can quote any
/// ID.
pub(super) fn dispatched_agent(structured: Option<&Value>) -> Option<String> {
    structured
        .and_then(|s| s.get("agentId"))
        .and_then(Value::as_str)
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
}

/// A start of `call`'s work in the issuer's `prompt`, placed at `ordinal`.
pub(super) fn start(
    session: &SessionId,
    call: &str,
    agent: Option<String>,
    kind: StartKind,
    message: Option<ContentId>,
    prompt: Option<&str>,
    ordinal: usize,
) -> Observation {
    Observation::Delegation(Delegation {
        id: EventId::derive("agent-start", session, call),
        session: session.clone(),
        step: DelegationStep::Start {
            agent,
            call: call.to_owned(),
            kind,
            message,
            prompt: prompt.map(str::to_owned),
        },
        source_ordinal: ordinal as u64,
        transcript: None,
    })
}

/// The proof, at `ordinal`, that `agent` stopped the work `call` started.
pub(super) fn stop(
    session: &SessionId,
    call: &str,
    agent: &str,
    status: StopStatus,
    ordinal: usize,
) -> Observation {
    Observation::Delegation(Delegation {
        id: EventId::derive("agent-stop", session, call),
        session: session.clone(),
        step: DelegationStep::Stop {
            agent: agent.to_owned(),
            call: call.to_owned(),
            status,
        },
        source_ordinal: ordinal as u64,
        transcript: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notification_header_ignores_the_report_it_carries() {
        let text = "<task-notification>\n<task-id>a1</task-id>\n<status>completed</status>\n\
                    <summary>Agent \"<tool-use-id>x</tool-use-id>\" finished</summary>\n\
                    <result><tool-use-id>forged</tool-use-id></result></task-notification>";
        assert_eq!(
            notification(text),
            Some(Notification {
                task: "a1",
                call: None,
                status: Some("completed"),
            })
        );
        assert_eq!(notification("no notification"), None);
        assert_eq!(notification("<task-notification><status>x</status>"), None);
    }

    #[test]
    fn only_a_text_that_begins_with_the_envelope_is_a_notification() {
        let envelope = "<task-notification><task-id>a1</task-id></task-notification>";
        assert!(notification(&format!("\n {envelope}")).is_some());
        assert_eq!(notification(&format!("I saw: {envelope}")), None);
        let warned = format!("{NOTIFICATION_WARNING} This is an automated event.\n\n{envelope}");
        assert_eq!(delivered_envelope(&warned), envelope);
        assert_eq!(delivered_envelope(envelope), envelope);
    }

    #[test]
    fn a_dispatched_agent_is_read_from_the_result() {
        let structured = serde_json::json!({"agentId": "a1"});
        assert_eq!(dispatched_agent(Some(&structured)), Some("a1".to_owned()));
        let empty = serde_json::json!({"agentId": ""});
        assert_eq!(dispatched_agent(Some(&empty)), None);
        assert_eq!(dispatched_agent(None), None);
    }
}
