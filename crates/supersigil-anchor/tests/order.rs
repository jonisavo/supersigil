//! The order between one session's transcripts, built from recorded
//! dispatches, resumes, stops, and receives.

mod common;

use std::time::Duration;

use common::{finishes_within, replace};
use supersigil_anchor::Order;
use supersigil_record::observations::{
    Delegation, DelegationStep, Edit, ReceiveKind, StartKind, StopStatus,
};
use supersigil_record::{ContentId, EventId, SessionId};

const MAIN: &str = "main.jsonl";
/// The issuer's prompt every step below is sent in, unless a test says.
const PROMPT: &str = "p";

/// A transcript of agent `agent`.
fn of(agent: &str) -> String {
    format!("agent-{agent}.jsonl")
}

fn delegation(id: &str, transcript: &str, ordinal: u64, step: DelegationStep) -> Delegation {
    Delegation {
        id: EventId::new(id),
        session: SessionId::new("s1"),
        step,
        source_ordinal: ordinal,
        transcript: Some(transcript.to_owned()),
    }
}

/// `transcript` dispatching `agent` with call `call` at `ordinal`.
fn dispatch(transcript: &str, ordinal: u64, agent: &str, call: &str) -> Delegation {
    delegation(
        &format!("start-{call}"),
        transcript,
        ordinal,
        DelegationStep::Start {
            agent: Some(agent.to_owned()),
            call: call.to_owned(),
            kind: StartKind::Dispatch,
            message: None,
            prompt: Some(PROMPT.to_owned()),
        },
    )
}

/// `transcript` resuming `agent` with `message` by call `call` at `ordinal`.
fn resume(transcript: &str, ordinal: u64, agent: &str, call: &str, message: &str) -> Delegation {
    delegation(
        &format!("start-{call}"),
        transcript,
        ordinal,
        DelegationStep::Start {
            agent: Some(agent.to_owned()),
            call: call.to_owned(),
            kind: StartKind::Resume,
            message: Some(ContentId::of(message.as_bytes())),
            prompt: Some(PROMPT.to_owned()),
        },
    )
}

/// `step` sent, or received, in the issuer's `prompt` instead.
fn in_prompt(mut step: Delegation, prompt: &str) -> Delegation {
    match &mut step.step {
        DelegationStep::Start { prompt: p, .. } | DelegationStep::Receive { prompt: p, .. } => {
            *p = Some(prompt.to_owned());
        }
        DelegationStep::Stop { .. } => {}
    }
    step
}

/// `transcript` learning at `ordinal` that `agent` stopped `call`'s work.
fn stop(transcript: &str, ordinal: u64, agent: &str, call: &str) -> Delegation {
    delegation(
        &format!("stop-{call}"),
        transcript,
        ordinal,
        DelegationStep::Stop {
            agent: agent.to_owned(),
            call: call.to_owned(),
            status: StopStatus::Completed,
        },
    )
}

/// `agent`'s transcript receiving work of `kind` at `ordinal`.
fn receive(agent: &str, ordinal: u64, kind: ReceiveKind, message: Option<&str>) -> Delegation {
    delegation(
        &format!("receive-{agent}-{ordinal}"),
        &of(agent),
        ordinal,
        DelegationStep::Receive {
            agent: agent.to_owned(),
            kind,
            message: message.map(|m| ContentId::of(m.as_bytes())),
            prompt: Some(PROMPT.to_owned()),
        },
    )
}

/// An edit made in `transcript` at `ordinal`.
fn edit(name: &str, transcript: &str, ordinal: u64) -> Edit {
    replace(name, transcript, ordinal, "a\n", "b\n")
}

fn order(delegations: &[Delegation]) -> Order {
    Order::new(delegations)
}

/// [`order`], for tests whose bindings shadow its name.
fn order_of(delegations: &[Delegation]) -> Order {
    order(delegations)
}

/// Main dispatches agent `a` at 1 and learns it stopped at 5; then
/// dispatches `b` at 6 and learns it stopped at 9.
fn sequential() -> Vec<Delegation> {
    vec![
        dispatch(MAIN, 1, "a", "ca"),
        receive("a", 0, ReceiveKind::Dispatch, None),
        stop(MAIN, 5, "a", "ca"),
        dispatch(MAIN, 6, "b", "cb"),
        receive("b", 0, ReceiveKind::Dispatch, None),
        stop(MAIN, 9, "b", "cb"),
    ]
}

#[test]
fn an_agent_started_after_another_stopped_comes_after_it() {
    let order = order(&sequential());
    let (a, b) = (edit("a", &of("a"), 3), edit("b", &of("b"), 2));
    assert!(order.before(&a, &b));
    assert!(!order.before(&b, &a));
    // The issuer's own edits: before its dispatch, and after both stops.
    let first = edit("m0", MAIN, 0);
    let last = edit("m10", MAIN, 10);
    for agent in [&a, &b] {
        assert!(order.before(&first, agent));
        assert!(order.before(agent, &last));
    }
    // Between the two agents, main's edit at 5.5 sits after a and before b.
    let between = edit("m6", MAIN, 6);
    assert!(order.before(&a, &between));
    assert!(
        !order.before(&between, &b),
        "issued at the dispatch's own line"
    );
}

#[test]
fn agents_running_at_once_stay_unordered() {
    let order = order(&[
        dispatch(MAIN, 1, "a", "ca"),
        receive("a", 0, ReceiveKind::Dispatch, None),
        dispatch(MAIN, 2, "b", "cb"),
        receive("b", 0, ReceiveKind::Dispatch, None),
        stop(MAIN, 5, "a", "ca"),
        stop(MAIN, 6, "b", "cb"),
    ]);
    let (a, b) = (edit("a", &of("a"), 3), edit("b", &of("b"), 2));
    assert!(!order.before(&a, &b));
    assert!(!order.before(&b, &a));
}

#[test]
fn a_resumed_agent_splits_into_segments_at_its_receives() {
    // a works, stops; b works, stops; a is resumed and works again.
    // Each delivered notification starts a new issuer prompt: the resume
    // is sent in a later one than the dispatches.
    let mut delegations = sequential();
    delegations.extend([
        in_prompt(resume(MAIN, 10, "a", "ra", "Fix it."), "p2"),
        in_prompt(receive("a", 20, ReceiveKind::Resume, Some("Fix it.")), "p2"),
        stop(MAIN, 30, "a", "ra"),
    ]);
    let order = order(&delegations);
    let (a1, b, a2) = (
        edit("a1", &of("a"), 3),
        edit("b", &of("b"), 2),
        edit("a2", &of("a"), 22),
    );
    assert!(order.before(&a1, &b));
    assert!(order.before(&b, &a2));
    assert!(!order.before(&a2, &b));
    assert!(order.before(&a2, &edit("m31", MAIN, 31)));
}

#[test]
fn work_after_an_input_that_could_wake_the_agent_is_not_before_the_stop() {
    let mut delegations = sequential();
    delegations.push(receive("a", 7, ReceiveKind::Input, None));
    let order = order(&delegations);
    let b = edit("b", &of("b"), 2);
    assert!(order.before(&edit("a3", &of("a"), 3), &b));
    assert!(!order.before(&edit("a9", &of("a"), 9), &b));
}

#[test]
fn resumes_match_by_message_not_by_position() {
    // Two resumes; the agent's transcript holds only the second's receive.
    // Matching by position would pair the first resume with it.
    let mut delegations = sequential();
    delegations.extend([
        in_prompt(resume(MAIN, 10, "a", "r1", "First."), "p2"),
        in_prompt(resume(MAIN, 12, "a", "r2", "Second."), "p2"),
        in_prompt(receive("a", 20, ReceiveKind::Resume, Some("Second.")), "p2"),
    ]);
    let order = order(&delegations);
    let late = edit("a21", &of("a"), 21);
    assert!(order.before(&edit("m11", MAIN, 11), &late));
    // Main's edit at 12 is not before the agent's: only edits below the
    // second resume's send point are.
    assert!(!order.before(&edit("m12", MAIN, 12), &late));
}

#[test]
fn a_message_sent_twice_matches_neither_resume() {
    let fresh = |step: Delegation| in_prompt(step, "p2");
    let (m9, m11, a21) = (
        edit("m9", MAIN, 9),
        edit("m11", MAIN, 11),
        edit("a21", &of("a"), 21),
    );
    // Sent once in a prompt of its own, the message is matched.
    let mut once = sequential();
    once.extend([
        fresh(resume(MAIN, 10, "a", "r1", "Again.")),
        fresh(receive("a", 20, ReceiveKind::Resume, Some("Again."))),
    ]);
    assert!(order(&once).before(&m9, &a21));
    // Sent twice, it matches neither resume, whether it is received once
    // or twice.
    let mut twice = once.clone();
    twice.push(fresh(resume(MAIN, 12, "a", "r2", "Again.")));
    let mut received_twice = twice.clone();
    received_twice.push(fresh(receive("a", 25, ReceiveKind::Resume, Some("Again."))));
    for delegations in [twice, received_twice] {
        let order = order(&delegations);
        assert!(!order.before(&m9, &a21));
        assert!(!order.before(&m11, &a21));
    }
}

#[test]
fn a_nested_agent_is_ordered_through_its_issuer() {
    // Main dispatches a; a dispatches c and learns c stopped; a then stops.
    let order = order(&[
        dispatch(MAIN, 1, "a", "ca"),
        receive("a", 0, ReceiveKind::Dispatch, None),
        dispatch(&of("a"), 4, "c", "cc"),
        receive("c", 0, ReceiveKind::Dispatch, None),
        stop(&of("a"), 8, "c", "cc"),
        stop(MAIN, 20, "a", "ca"),
    ]);
    let c = edit("c", &of("c"), 2);
    assert!(order.before(&edit("m0", MAIN, 0), &c));
    assert!(order.before(&c, &edit("a9", &of("a"), 9)));
    assert!(order.before(&c, &edit("m21", MAIN, 21)));
    assert!(!order.before(&edit("a5", &of("a"), 5), &c));
}

#[test]
fn a_worker_fork_takes_its_agent_from_its_stop() {
    let order = order(&[
        delegation(
            "start-cf",
            MAIN,
            1,
            DelegationStep::Start {
                agent: None,
                call: "cf".to_owned(),
                kind: StartKind::Dispatch,
                message: None,
                prompt: Some(PROMPT.to_owned()),
            },
        ),
        receive("f", 2, ReceiveKind::Dispatch, None),
        stop(MAIN, 5, "f", "cf"),
    ]);
    assert!(order.before(&edit("f", &of("f"), 4), &edit("m6", MAIN, 6)));
    assert!(order.before(&edit("m0", MAIN, 0), &edit("f", &of("f"), 4)));
}

#[test]
fn missing_records_remove_orders_without_inventing_any() {
    let (a, b) = (edit("a", &of("a"), 3), edit("b", &of("b"), 2));
    // The issuer synced before it learned that a stopped.
    let mut no_stop = sequential();
    no_stop.retain(|d| d.id != EventId::new("stop-ca"));
    assert!(!order(&no_stop).before(&a, &b));
    // The agent's transcript was not synced: nothing places its edits.
    let mut no_receive = sequential();
    no_receive.retain(|d| d.id != EventId::new("receive-a-0"));
    assert!(!order(&no_receive).before(&a, &b));
}

#[test]
fn a_contradictory_session_gets_no_order() {
    let (a, b) = (edit("a", &of("a"), 3), edit("b", &of("b"), 2));
    // A stop placed before the dispatch it ends.
    let mut early = sequential();
    early.retain(|d| d.id != EventId::new("stop-ca"));
    early.push(stop(MAIN, 0, "a", "ca"));
    assert!(!order(&early).before(&a, &b));
    // Two transcripts receiving one agent's work.
    let mut twice = sequential();
    twice.push(delegation(
        "receive-a-elsewhere",
        "other.jsonl",
        0,
        DelegationStep::Receive {
            agent: "a".to_owned(),
            kind: ReceiveKind::Dispatch,
            message: None,
            prompt: Some(PROMPT.to_owned()),
        },
    ));
    assert!(!order(&twice).before(&a, &b));
    // A step without a transcript cannot be placed.
    let mut unplaced = sequential();
    unplaced[4].transcript = None;
    assert!(!order(&unplaced).before(&a, &b));
}

#[test]
fn a_transcript_whose_receives_name_two_agents_leaves_its_session_unordered() {
    // Two parses of a rewritten agent transcript: one read a's dispatch at
    // its first line, the other b's. Their event IDs differ, so both are
    // kept, but one transcript cannot hold both agents' work.
    const SHARED: &str = "agent.jsonl";
    let received = |agent: &str| {
        delegation(
            &format!("receive-{agent}-0"),
            SHARED,
            0,
            DelegationStep::Receive {
                agent: agent.to_owned(),
                kind: ReceiveKind::Dispatch,
                message: None,
                prompt: Some(PROMPT.to_owned()),
            },
        )
    };
    let (m0, m4, x2) = (
        edit("m0", MAIN, 0),
        edit("m4", MAIN, 4),
        edit("x2", SHARED, 2),
    );
    // With a's receive alone, the transcript's work comes after main's
    // edits before a's dispatch, and only those.
    let one = order(&[
        dispatch(MAIN, 1, "a", "ca"),
        received("a"),
        dispatch(MAIN, 5, "b", "cb"),
    ]);
    assert!(one.before(&m0, &x2));
    assert!(!one.before(&m4, &x2));
    // With b's too, nothing orders the transcript: either agent's work
    // could be the one it holds.
    let both = order(&[
        dispatch(MAIN, 1, "a", "ca"),
        received("a"),
        dispatch(MAIN, 5, "b", "cb"),
        received("b"),
    ]);
    assert!(!both.before(&m4, &x2));
    assert!(!both.before(&m0, &x2));
}

#[test]
fn differing_sightings_of_one_step_void_the_session() {
    let (a, b) = (edit("a", &of("a"), 3), edit("b", &of("b"), 2));
    let delegations = sequential();
    // The same steps seen twice, in two records or replayed in one, agree.
    let twice: Vec<&Delegation> = delegations.iter().chain(&delegations).collect();
    assert!(Order::new(twice).before(&a, &b));
    // A second sighting places one step elsewhere: two records disagree, or
    // a rewritten transcript was read again from its start. Keeping either
    // could pair a stop with the wrong work.
    let mut moved = delegations[2].clone();
    moved.source_ordinal = 4;
    let differing: Vec<&Delegation> = delegations.iter().chain([&moved]).collect();
    assert!(!Order::new(differing).before(&a, &b));
}

#[test]
fn edits_of_other_sessions_or_without_a_transcript_are_never_ordered() {
    let order = order(&sequential());
    let a = edit("a", &of("a"), 3);
    let mut elsewhere = edit("b", &of("b"), 2);
    elsewhere.session = SessionId::new("s2");
    assert!(!order.before(&a, &elsewhere));
    let mut unplaced = edit("b", &of("b"), 2);
    unplaced.transcript = None;
    assert!(!order.before(&a, &unplaced));
    // Within one transcript, ordinals order edits.
    assert!(order.before(&edit("x", MAIN, 1), &edit("y", MAIN, 2)));
    assert!(!order.before(&edit("x", MAIN, 2), &edit("y", MAIN, 2)));
}

#[test]
fn an_agent_whose_start_is_not_recorded_yet_is_not_ordered() {
    // Reviewing a live session: the issuer's Agent call still awaits its
    // result, so no start is recorded, but the agent's transcript is.
    let mut live = sequential();
    live.retain(|d| d.id != EventId::new("start-cb"));
    let order = order(&live);
    let b = edit("b", &of("b"), 2);
    assert!(!order.before(&edit("a", &of("a"), 3), &b));
    assert!(!order.before(&edit("m0", MAIN, 0), &b));
}

#[test]
fn a_resume_sent_from_another_session_matches_nothing() {
    // A later session continuing this one resumes agent a; its start is
    // another session's, so a's resumed work is not placed after b.
    let mut delegations = sequential();
    let mut elsewhere = resume("later.jsonl", 10, "a", "ra", "Fix it.");
    elsewhere.session = SessionId::new("s2");
    delegations.extend([
        elsewhere,
        receive("a", 20, ReceiveKind::Resume, Some("Fix it.")),
    ]);
    let order = order(&delegations);
    assert!(!order.before(&edit("b", &of("b"), 2), &edit("a22", &of("a"), 22)));
    assert!(order.before(&edit("a3", &of("a"), 3), &edit("b", &of("b"), 2)));
}

#[test]
fn a_resume_message_another_session_also_sent_matches_nothing() {
    // A later session resumed a with "Fix it." (its receive is at 20);
    // then this session resumed a with the same message at 10, and a's
    // transcript is synced only up to the first receive. The receive could
    // be either start's, so it is matched with neither.
    let mut delegations = sequential();
    let mut later = resume("later.jsonl", 3, "a", "rl", "Fix it.");
    later.session = SessionId::new("s2");
    delegations.extend([
        later,
        receive("a", 20, ReceiveKind::Resume, Some("Fix it.")),
        resume(MAIN, 10, "a", "rs", "Fix it."),
    ]);
    let order = order(&delegations);
    assert!(!order.before(&edit("m9", MAIN, 9), &edit("a21", &of("a"), 21)));
}

#[test]
fn a_session_too_large_to_order_gets_no_order() {
    // Agents in turn whose clocks would hold more positions than the bound:
    // the order is given up, not computed without limit.
    let large = finishes_within(Duration::from_secs(10), || {
        let mut delegations = Vec::new();
        for k in 0..1500u64 {
            let (agent, call) = (format!("x{k}"), format!("c{k}"));
            delegations.push(dispatch(MAIN, 10 * k + 1, &agent, &call));
            delegations.push(receive(&agent, 0, ReceiveKind::Dispatch, None));
            delegations.push(stop(MAIN, 10 * k + 5, &agent, &call));
        }
        order(&delegations)
    });
    let (first, last) = (edit("f", &of("x0"), 2), edit("l", &of("x1499"), 2));
    assert!(!large.before(&first, &last));
    // Within the bound, a long session is ordered.
    let long = finishes_within(Duration::from_secs(10), || {
        let mut delegations = Vec::new();
        for k in 0..500u64 {
            let (agent, call) = (format!("x{k}"), format!("c{k}"));
            delegations.push(dispatch(MAIN, 10 * k + 1, &agent, &call));
            delegations.push(receive(&agent, 0, ReceiveKind::Dispatch, None));
            delegations.push(stop(MAIN, 10 * k + 5, &agent, &call));
        }
        order(&delegations)
    });
    assert!(long.before(&edit("f", &of("x0"), 2), &edit("l", &of("x499"), 2)));
}

#[test]
fn every_sighting_of_a_start_contends_even_in_a_session_left_unordered() {
    // This session resumes a with "Fix it." in prompt pf, and a receives it
    // there: the resume is matched.
    let (m9, a21) = (edit("m9", MAIN, 9), edit("a21", &of("a"), 21));
    let mut delegations = sequential();
    delegations.extend([
        in_prompt(resume(MAIN, 10, "a", "rs", "Fix it."), "pf"),
        in_prompt(receive("a", 20, ReceiveKind::Resume, Some("Fix it.")), "pf"),
    ]);
    assert!(order(&delegations).before(&m9, &a21));
    // A later session's resume of a, seen with another message in another
    // prompt, could not have started that receive.
    let in_s2 = |mut step: Delegation| {
        step.session = SessionId::new("s2");
        step
    };
    let later = |message: &str, prompt: &str| {
        in_s2(in_prompt(
            resume("later.jsonl", 3, "a", "rl", message),
            prompt,
        ))
    };
    delegations.push(later("Other.", "pl"));
    assert!(order(&delegations).before(&m9, &a21));
    // Seen again with "Fix it." in pf, it leaves its session unordered; that
    // sighting still could have started a's receive at 20, so this
    // session's own resume is not matched with it.
    delegations.push(later("Fix it.", "pf"));
    assert!(!order(&delegations).before(&m9, &a21));
}

#[test]
fn a_resume_with_an_unknown_message_contends_for_every_resume_of_its_agent() {
    // This session resumes a with "Fix it." in prompt pf, and a receives it
    // there: the resume is matched.
    let (m9, a21) = (edit("m9", MAIN, 9), edit("a21", &of("a"), 21));
    let mut delegations = sequential();
    delegations.extend([
        in_prompt(resume(MAIN, 10, "a", "rs", "Fix it."), "pf"),
        in_prompt(receive("a", 20, ReceiveKind::Resume, Some("Fix it.")), "pf"),
    ]);
    assert!(order(&delegations).before(&m9, &a21));
    // A later session resumed a, in another prompt, with a message the
    // parser could not read: it contends for every resume receive of a.
    let mut unknown = delegation(
        "start-rl",
        "later.jsonl",
        3,
        DelegationStep::Start {
            agent: Some("a".to_owned()),
            call: "rl".to_owned(),
            kind: StartKind::Resume,
            message: None,
            prompt: Some(PROMPT.to_owned()),
        },
    );
    unknown.session = SessionId::new("s2");
    delegations.push(unknown);
    assert!(!order(&delegations).before(&m9, &a21));
}

#[test]
fn a_fork_not_yet_named_contends_for_every_dispatch_of_its_session() {
    // A worker fork's start names no agent until its stop is recorded.
    let mut delegations = sequential();
    delegations.push(delegation(
        "start-cf",
        MAIN,
        7,
        DelegationStep::Start {
            agent: None,
            call: "cf".to_owned(),
            kind: StartKind::Dispatch,
            message: None,
            prompt: Some(PROMPT.to_owned()),
        },
    ));
    let order = order(&delegations);
    assert!(!order.before(&edit("a", &of("a"), 3), &edit("b", &of("b"), 2)));
}

#[test]
fn many_resumes_are_matched_promptly() {
    // One agent resumed 50,000 times, each in turn: half with one shared
    // message, which matches nothing, half with their own.
    let order = finishes_within(Duration::from_secs(10), || {
        let mut delegations = vec![
            dispatch(MAIN, 1, "a", "ca"),
            receive("a", 0, ReceiveKind::Dispatch, None),
            stop(MAIN, 5, "a", "ca"),
        ];
        for k in 1..=50_000u64 {
            let message = if k % 2 == 0 {
                "Continue.".to_owned()
            } else {
                format!("Step {k}.")
            };
            let call = format!("r{k}");
            delegations.push(resume(MAIN, 10 * k + 1, "a", &call, &message));
            delegations.push(receive("a", 10 * k, ReceiveKind::Resume, Some(&message)));
            delegations.push(stop(MAIN, 10 * k + 5, "a", &call));
        }
        order(&delegations)
    });
    // The last odd resume is matched: work after its receive comes after
    // main's edit before it was sent.
    assert!(order.before(&edit("m", MAIN, 499_990), &edit("a", &of("a"), 499_992)));
}

#[test]
fn a_start_is_matched_only_with_a_receive_of_its_prompt() {
    // A later session resumed a with "Fix it." in its prompt pl, and its
    // start is not recorded yet; this session's own "Fix it." resume, sent
    // in prompt ps, is not matched with the receive pl sent.
    let mut delegations = sequential();
    delegations.extend([
        in_prompt(receive("a", 20, ReceiveKind::Resume, Some("Fix it.")), "pl"),
        in_prompt(resume(MAIN, 10, "a", "rs", "Fix it."), "ps"),
    ]);
    let order = order(&delegations);
    assert!(!order.before(&edit("m9", MAIN, 9), &edit("a21", &of("a"), 21)));
    // With the same prompt it is matched.
    let mut same = sequential();
    same.extend([
        in_prompt(receive("a", 20, ReceiveKind::Resume, Some("Fix it.")), "ps"),
        in_prompt(resume(MAIN, 10, "a", "rs", "Fix it."), "ps"),
    ]);
    assert!(order_of(&same).before(&edit("m9", MAIN, 9), &edit("a21", &of("a"), 21)));
    // A dispatch received in another prompt is not matched either.
    let mut other = sequential();
    other[1] = in_prompt(other[1].clone(), "elsewhere");
    let order = order_of(&other);
    assert!(!order.before(&edit("m0", MAIN, 0), &edit("a3", &of("a"), 3)));
}

#[test]
fn conflicting_sightings_of_a_forks_stop_are_counted_promptly() {
    // One unnamed fork start seen 50,000 ways, and its stop seen naming
    // 50,000 agents: the fork names no single agent, so it contends for
    // every dispatch of its session, without visiting every pair.
    let order = finishes_within(Duration::from_secs(10), || {
        let mut delegations = sequential();
        for k in 0..50_000u64 {
            delegations.push(delegation(
                "start-cf",
                MAIN,
                100 + k,
                DelegationStep::Start {
                    agent: None,
                    call: "cf".to_owned(),
                    kind: StartKind::Dispatch,
                    message: None,
                    prompt: Some(PROMPT.to_owned()),
                },
            ));
            let mut end = stop(MAIN, 200_000 + k, &format!("f{k}"), "cf");
            end.id = EventId::new("stop-cf");
            delegations.push(end);
        }
        order(&delegations)
    });
    assert!(!order.before(&edit("a", &of("a"), 3), &edit("b", &of("b"), 2)));
}

/// Main resumes agent a with "Fix it." at 10 in prompt pm, and a receives
/// it at 20; `others` are added.
fn resumed_in_pm(others: &[Delegation]) -> Order {
    let mut delegations = sequential();
    delegations.extend([
        in_prompt(resume(MAIN, 10, "a", "rs", "Fix it."), "pm"),
        in_prompt(receive("a", 20, ReceiveKind::Resume, Some("Fix it.")), "pm"),
    ]);
    delegations.extend(others.iter().cloned());
    order_of(&delegations)
}

#[test]
fn a_resume_another_agent_working_in_its_prompt_could_have_sent_is_not_matched() {
    // Main dispatched l at 7 in the same prompt; l could have sent a the
    // same message, its start not yet recorded.
    let (m9, a21) = (edit("m9", MAIN, 9), edit("a21", &of("a"), 21));
    let l_first = [
        in_prompt(dispatch(MAIN, 7, "l", "cl"), "pm"),
        in_prompt(receive("l", 0, ReceiveKind::Dispatch, None), "pm"),
    ];
    assert!(!resumed_in_pm(&l_first).before(&m9, &a21));
    // Dispatched after the resume, l could not have sent it.
    let l_after = [
        in_prompt(dispatch(MAIN, 11, "l", "cl"), "pm"),
        in_prompt(receive("l", 0, ReceiveKind::Dispatch, None), "pm"),
    ];
    assert!(resumed_in_pm(&l_after).before(&m9, &a21));
}

#[test]
fn an_agent_working_in_a_prompt_without_a_recorded_start_blocks_its_resumes() {
    // Agent k received work in the prompt, but no start of it is recorded
    // (launched by a slash command, or its issuer not yet synced).
    let k = [in_prompt(
        receive("k", 0, ReceiveKind::Dispatch, None),
        "pm",
    )];
    assert!(!resumed_in_pm(&k).before(&edit("m9", MAIN, 9), &edit("a21", &of("a"), 21)));
}

#[test]
fn a_resume_issued_by_an_agent_is_not_matched() {
    // An agent's own sends share its issuer's prompt, which the issuer
    // could use too, unrecorded.
    let mut delegations = sequential();
    delegations.extend([
        in_prompt(resume(&of("b"), 3, "a", "rb", "Fix it."), "pb"),
        in_prompt(receive("a", 20, ReceiveKind::Resume, Some("Fix it.")), "pb"),
    ]);
    let order = order(&delegations);
    assert!(!order.before(&edit("b2", &of("b"), 2), &edit("a21", &of("a"), 21)));
}

#[test]
fn an_empty_prompt_matches_nothing() {
    let mut delegations = sequential();
    delegations[0] = in_prompt(delegations[0].clone(), "");
    delegations[1] = in_prompt(delegations[1].clone(), "");
    let order = order(&delegations);
    assert!(!order.before(&edit("m0", MAIN, 0), &edit("a3", &of("a"), 3)));
}

#[test]
fn a_receive_in_the_prompt_no_start_explains_blocks_resumes() {
    // Agent l received its dispatch in prompt pm with no dispatch naming
    // it (a slash command launched it); main's later resume of l, with
    // another message, does not explain that receive.
    let l = [
        in_prompt(receive("l", 0, ReceiveKind::Dispatch, None), "pm"),
        in_prompt(resume(MAIN, 12, "l", "rl", "Other."), "pm"),
    ];
    assert!(!resumed_in_pm(&l).before(&edit("m9", MAIN, 9), &edit("a21", &of("a"), 21)));
}

#[test]
fn a_start_or_receive_without_a_prompt_blocks_resumes() {
    // Main dispatched l and l received it, but neither names a prompt: l
    // may be working in pm.
    let mut start = dispatch(MAIN, 7, "l", "cl");
    let mut received = receive("l", 0, ReceiveKind::Dispatch, None);
    for step in [&mut start, &mut received] {
        if let DelegationStep::Start { prompt, .. } | DelegationStep::Receive { prompt, .. } =
            &mut step.step
        {
            *prompt = None;
        }
    }
    let (m9, a21) = (edit("m9", MAIN, 9), edit("a21", &of("a"), 21));
    assert!(!resumed_in_pm(&[start.clone()]).before(&m9, &a21));
    assert!(!resumed_in_pm(&[received]).before(&m9, &a21));
}

#[test]
fn a_later_identical_resume_does_not_explain_an_earlier_receive() {
    // l, dispatched in another prompt, received "Again." in pm from a
    // resume the parser could not read; main's later resume of l with the
    // same message in the same prompt did not cause that receive.
    let l = [
        in_prompt(dispatch(MAIN, 2, "l", "cl"), "q"),
        in_prompt(receive("l", 0, ReceiveKind::Dispatch, None), "q"),
        in_prompt(receive("l", 30, ReceiveKind::Resume, Some("Again.")), "pm"),
        in_prompt(resume(MAIN, 12, "l", "rl", "Again."), "pm"),
    ];
    assert!(!resumed_in_pm(&l).before(&edit("m9", MAIN, 9), &edit("a21", &of("a"), 21)));
}

#[test]
fn an_input_in_the_prompt_or_naming_none_blocks_resumes() {
    // l, dispatched in another prompt, received an input in pm, or one
    // whose prompt cannot be read (a malformed resume record): it may be
    // working in pm.
    let (m9, a21) = (edit("m9", MAIN, 9), edit("a21", &of("a"), 21));
    let in_q = [
        in_prompt(dispatch(MAIN, 2, "l", "cl"), "q"),
        in_prompt(receive("l", 0, ReceiveKind::Dispatch, None), "q"),
    ];
    let mut unread = receive("l", 4, ReceiveKind::Input, None);
    if let DelegationStep::Receive { prompt, .. } = &mut unread.step {
        *prompt = None;
    }
    for input in [
        in_prompt(receive("l", 4, ReceiveKind::Input, None), "pm"),
        unread,
    ] {
        let mut others = in_q.to_vec();
        others.push(input);
        assert!(!resumed_in_pm(&others).before(&m9, &a21));
    }
    // An input in another prompt does not.
    let mut others = in_q.to_vec();
    others.push(in_prompt(receive("l", 4, ReceiveKind::Input, None), "q"));
    assert!(resumed_in_pm(&others).before(&m9, &a21));
    // Nor does one in pm of an agent dispatched there after the resume:
    // it began working only after the resume was sent.
    let after = [
        in_prompt(dispatch(MAIN, 11, "l", "cl"), "pm"),
        in_prompt(receive("l", 0, ReceiveKind::Dispatch, None), "pm"),
        in_prompt(receive("l", 4, ReceiveKind::Input, None), "pm"),
    ];
    assert!(resumed_in_pm(&after).before(&m9, &a21));
}

#[test]
fn a_receive_in_the_prompt_recorded_in_another_session_blocks_resumes() {
    // l, an agent of an earlier session, was resumed in pm by a call the
    // parser could not read; its receive is recorded under its own session.
    let mut l = in_prompt(receive("l", 30, ReceiveKind::Resume, Some("Again.")), "pm");
    l.session = SessionId::new("s2");
    assert!(!resumed_in_pm(&[l]).before(&edit("m9", MAIN, 9), &edit("a21", &of("a"), 21)));
}

#[test]
fn a_contended_dispatch_does_not_excuse_its_agent() {
    // Main dispatched l in pm after the resume, but another session's
    // start names l too: the dispatch is not matched, so nothing shows that
    // l began working after the resume.
    let mut other = in_prompt(dispatch("other.jsonl", 1, "l", "co"), "q");
    other.session = SessionId::new("s2");
    let l = [
        in_prompt(dispatch(MAIN, 12, "l", "cl"), "pm"),
        in_prompt(receive("l", 0, ReceiveKind::Dispatch, None), "pm"),
        other,
    ];
    assert!(!resumed_in_pm(&l).before(&edit("m9", MAIN, 9), &edit("a21", &of("a"), 21)));
}

#[test]
fn a_start_of_another_session_in_the_same_transcript_blocks_resumes() {
    // A session recorded in the same main transcript (rewritten, or read
    // again) dispatched l in pm at a later line; its lines are not this
    // session's, so nothing shows l began after the resume.
    let in_s2 = |mut step: Delegation| {
        step.session = SessionId::new("s2");
        step
    };
    let l = [
        in_s2(in_prompt(dispatch(MAIN, 12, "l", "cl"), "pm")),
        in_s2(in_prompt(
            receive("l", 0, ReceiveKind::Dispatch, None),
            "pm",
        )),
    ];
    assert!(!resumed_in_pm(&l).before(&edit("m9", MAIN, 9), &edit("a21", &of("a"), 21)));
}

#[test]
fn many_issuers_in_one_prompt_are_checked_promptly() {
    // 100,000 issuers each resume a with its own message in one prompt,
    // and one more starts b there: no resume is matched, and checking
    // each against the others takes no quadratic work.
    let order = finishes_within(Duration::from_secs(10), || {
        let mut delegations = sequential();
        for k in 0..100_000u64 {
            let issuer = format!("issuer-{k:06}.jsonl");
            let message = format!("Step {k}.");
            delegations.push(in_prompt(
                resume(&issuer, 1, "a", &format!("r{k}"), &message),
                "pm",
            ));
            delegations.push(in_prompt(
                receive("a", 100 + k, ReceiveKind::Resume, Some(&message)),
                "pm",
            ));
        }
        // Its event ID sorts last, so a scan would reach it last.
        delegations.push(in_prompt(dispatch("zz.jsonl", 1, "b2", "zzz"), "pm"));
        order_of(&delegations)
    });
    assert!(!order.before(
        &edit("i", "issuer-000000.jsonl", 0),
        &edit("a", &of("a"), 101)
    ));
}
