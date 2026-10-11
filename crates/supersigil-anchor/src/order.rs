//! Orders the edits of one session's transcripts by what the transcripts
//! record about delegation ([`Delegation`]), never by time.
//!
//! Positions are (transcript, ordinal). An edge from (T1, s) to (T2, t)
//! says that everything T1 recorded below s happened before everything T2
//! recorded at or after t. Within a transcript, a lower ordinal comes first.
//!
//! - A start leaves its issuer at its send point and arrives at the receive
//!   it is matched with: a dispatch with the agent's dispatch receive, a
//!   resume with the agent's resume receive carrying the same message hash,
//!   each sent in the same issuer prompt as the receive records. Matching
//!   is by content and prompt, never by position, so a missing record
//!   leaves only its own start unmatched. A dispatch's link is proven: its
//!   result names the agent. A resume's is inferred: nothing in the
//!   receiving record names the sender, so a resume is matched only when
//!   its issuer is a root transcript (one that receives no work) and no
//!   other recorded transcript could have sent in that prompt by then (a
//!   transcript sends in a prompt only after receiving work in it). That
//!   no sender whose records are missing (a transcript not synced, or the
//!   part of one not yet synced) sent the same agent the identical message
//!   in the same prompt is assumed. A receive is matched only when no other
//!   recorded start, in any session, could have started it (a resume whose
//!   message is unknown could have started any resume of its agent, a
//!   worker fork not yet named any dispatch of its session), and a message
//!   received twice matches nothing.
//! - A stop names the agent and the start whose work ended. It leaves the
//!   agent at the first receive of any kind after that start's receive (or
//!   never, when there is none yet: sync appends each transcript in order,
//!   so the agent recorded nothing after an unrecorded receive) and arrives
//!   at the stop's position in the issuer.
//!
//! Steps that cannot be placed are ignored, which removes orders and never
//! adds one. A session gets no order at all when its steps contradict each
//! other (among them one agent's receives in two transcripts, or two
//! agents' in one) or lack a transcript, when two sightings of one step
//! differ (a transcript rewritten and read again from its start, or records
//! that disagree: keeping either could pair a stop with the wrong work), or
//! when its clocks would exceed [`MAX_CLOCK_POSITIONS`].

use std::collections::{BTreeMap, BTreeSet};

use supersigil_record::observations::{Delegation, DelegationStep, Edit, ReceiveKind, StartKind};
use supersigil_record::{ContentId, EventId, SessionId};

/// The most clock positions one session's order may hold: its edges times
/// its transcripts. Building the order costs about that much work and
/// memory; a larger session gets no order.
pub const MAX_CLOCK_POSITIONS: usize = 1 << 22;

/// The recorded order between the transcripts of each session.
#[derive(Debug, Clone, Default)]
pub struct Order {
    sessions: BTreeMap<SessionId, SessionOrder>,
}

impl Order {
    /// Builds the order from the delegation steps of every record. Equal
    /// sightings of one event ID are one step; differing sightings, in one
    /// record or several, leave the step's session unordered, yet every
    /// sighting of a start still contends for the receives it could have
    /// started.
    #[must_use]
    pub fn new<'a>(steps: impl IntoIterator<Item = &'a Delegation>) -> Self {
        let mut kept: BTreeMap<&EventId, &Delegation> = BTreeMap::new();
        let mut sightings: Vec<&Delegation> = Vec::new();
        let mut disputed: BTreeSet<&SessionId> = BTreeSet::new();
        for step in steps {
            match kept.get(&step.id) {
                None => {
                    kept.insert(&step.id, step);
                    sightings.push(step);
                }
                Some(first) if *first == step => {}
                Some(first) => {
                    disputed.insert(&first.session);
                    disputed.insert(&step.session);
                    sightings.push(step);
                }
            }
        }
        let contenders = Contenders::new(&sightings);
        let senders = Senders::new(&sightings, &contenders);
        let mut by_session: BTreeMap<&SessionId, Vec<&Delegation>> = BTreeMap::new();
        for step in kept.into_values() {
            by_session.entry(&step.session).or_default().push(step);
        }
        let sessions = by_session
            .into_iter()
            .filter(|(session, _)| !disputed.contains(session))
            .filter_map(|(session, steps)| {
                Some((
                    session.clone(),
                    SessionOrder::new(session, &steps, &contenders, &senders)?,
                ))
            })
            .collect();
        Self { sessions }
    }

    /// Whether the record shows that `a` happened before `b`.
    ///
    /// Edits of one transcript of one session are ordered by their
    /// ordinals, whether or not the session got an order: this restates the
    /// walk's rule for one transcript. Edits of different transcripts of one
    /// session are ordered by the recorded delegation. Edits of different
    /// sessions, or without a transcript, never are.
    #[must_use]
    pub fn before(&self, a: &Edit, b: &Edit) -> bool {
        if a.session != b.session {
            return false;
        }
        let (Some(from), Some(to)) = (&a.transcript, &b.transcript) else {
            return false;
        };
        if from == to {
            return a.source_ordinal < b.source_ordinal;
        }
        let Some(order) = self.sessions.get(&a.session) else {
            return false;
        };
        let (Some(&from), Some(&to)) = (order.transcripts.get(from), order.transcripts.get(to))
        else {
            return false;
        };
        order
            .known(to, b.source_ordinal)
            .is_some_and(|clock| clock[from] > a.source_ordinal)
    }
}

/// Every recorded start, in any session, that could have started a receive:
/// how many name each agent with each kind, message, and prompt, and which
/// starts leave that open.
struct Contenders<'a> {
    /// The agents a stop of each call names, by session, issuing
    /// transcript, and call: how a worker fork's start is named.
    forks: BTreeMap<(&'a SessionId, &'a str, &'a str), BTreeSet<&'a str>>,
    /// Dispatches naming each agent.
    dispatches: BTreeMap<&'a str, usize>,
    /// Sessions with a dispatch that names no single agent: a worker fork
    /// whose stop is not recorded, or recorded naming several agents. It
    /// could have started any dispatch receive of its session.
    unnamed: BTreeSet<&'a SessionId>,
    /// Resumes naming each agent with each message, in each prompt.
    resumes: BTreeMap<(&'a str, &'a ContentId, &'a str), usize>,
    /// Agents with a resume whose message or prompt is not known. It could
    /// have started any resume receive of its agent.
    unknown: BTreeSet<&'a str>,
}

impl<'a> Contenders<'a> {
    fn new(sightings: &[&'a Delegation]) -> Self {
        let mut contenders = Self {
            forks: BTreeMap::new(),
            dispatches: BTreeMap::new(),
            unnamed: BTreeSet::new(),
            resumes: BTreeMap::new(),
            unknown: BTreeSet::new(),
        };
        for step in sightings {
            if let (DelegationStep::Stop { agent, call, .. }, Some(transcript)) =
                (&step.step, &step.transcript)
            {
                contenders
                    .forks
                    .entry((&step.session, transcript, call))
                    .or_default()
                    .insert(agent);
            }
        }
        for step in sightings {
            let DelegationStep::Start {
                kind,
                message,
                prompt,
                ..
            } = &step.step
            else {
                continue;
            };
            let Some(agent) = contenders.agent(step) else {
                contenders.unnamed.insert(&step.session);
                continue;
            };
            match (kind, message, named(prompt.as_ref())) {
                (StartKind::Dispatch, ..) => {
                    *contenders.dispatches.entry(agent).or_default() += 1;
                }
                (StartKind::Resume, Some(message), Some(prompt)) => {
                    *contenders
                        .resumes
                        .entry((agent, message, prompt))
                        .or_default() += 1;
                }
                (StartKind::Resume, ..) => {
                    contenders.unknown.insert(agent);
                }
            }
        }
        contenders
    }

    /// The one agent the start `step` names, or for a worker fork the one
    /// agent a stop of its call names; `None` when there is not exactly one.
    fn agent(&self, step: &'a Delegation) -> Option<&'a str> {
        let DelegationStep::Start { agent, call, .. } = &step.step else {
            return None;
        };
        if let Some(agent) = agent {
            return Some(agent);
        }
        let transcript = step.transcript.as_deref()?;
        let named = self
            .forks
            .get(&(&step.session, transcript, call.as_str()))?;
        let mut named = named.iter();
        match (named.next(), named.next()) {
            (Some(agent), None) => Some(agent),
            _ => None,
        }
    }

    /// How many starts could have started a dispatch receive of `agent` in
    /// `session`.
    fn dispatching(&self, session: &SessionId, agent: &str) -> usize {
        if self.unnamed.contains(session) {
            usize::MAX
        } else {
            self.dispatches.get(agent).copied().unwrap_or(0)
        }
    }

    /// How many starts could have started a resume receive of `agent`
    /// carrying `message`, sent in `prompt`.
    fn resuming(&self, agent: &str, message: &ContentId, prompt: &str) -> usize {
        if self.unknown.contains(agent) {
            usize::MAX
        } else {
            self.resumes
                .get(&(agent, message, prompt))
                .copied()
                .unwrap_or(0)
        }
    }
}

/// The order of one session.
#[derive(Debug, Clone)]
struct SessionOrder {
    /// The index of each transcript that holds a step.
    transcripts: BTreeMap<String, usize>,
    /// What is known at each arrival.
    arrivals: Arrivals,
}

/// For each transcript, its arrivals by position, each with what is known
/// there: for every transcript, the position below which all it recorded
/// happened before.
type Arrivals = Vec<Vec<(u64, Vec<u64>)>>;

/// Everything `from` recorded below `below` happened before everything
/// `to` recorded at or after `at`.
struct Edge {
    from: usize,
    below: u64,
    to: usize,
    at: u64,
}

/// A start, in its issuer's transcript.
struct Start<'a> {
    transcript: usize,
    /// The issuer's transcript, by name.
    issuer: &'a str,
    ordinal: u64,
    agent: &'a str,
    call: &'a str,
    kind: StartKind,
    message: Option<&'a ContentId>,
    prompt: Option<&'a str>,
}

/// The receives of one agent's transcript, indexed.
#[derive(Default)]
struct Received<'a> {
    /// The agent's transcript.
    home: usize,
    /// Every receive's position, sorted.
    all: Vec<u64>,
    /// The dispatch receives' positions, each with its prompt.
    dispatches: Vec<(u64, Option<&'a str>)>,
    /// The resume receives' positions, by message and prompt.
    resumes: BTreeMap<(&'a ContentId, &'a str), Vec<u64>>,
}

impl SessionOrder {
    /// The order of `session`'s steps, or `None` when they cannot all be
    /// placed, contradict each other, or are too many to order.
    fn new(
        session: &SessionId,
        steps: &[&Delegation],
        contenders: &Contenders,
        senders: &Senders,
    ) -> Option<Self> {
        let mut transcripts: BTreeMap<String, usize> = BTreeMap::new();
        for step in steps {
            let next = transcripts.len();
            transcripts.entry(step.transcript.clone()?).or_insert(next);
        }
        let index = |step: &Delegation| step.transcript.as_ref().map(|t| transcripts[t]);
        let mut starts: Vec<Start> = Vec::new();
        let mut ends: Vec<(usize, u64, &str, &str)> = Vec::new();
        let mut agents: BTreeMap<&str, Received> = BTreeMap::new();
        // The agent each transcript's receives name: one transcript holds
        // one agent's work, as each agent's work lies in one transcript.
        let mut homes: BTreeMap<usize, &str> = BTreeMap::new();
        for step in steps {
            let transcript = index(step)?;
            let name = step.transcript.as_deref()?;
            let ordinal = step.source_ordinal;
            match &step.step {
                DelegationStep::Start {
                    call,
                    kind,
                    message,
                    prompt,
                    ..
                } => {
                    if let Some(agent) = contenders.agent(step) {
                        starts.push(Start {
                            transcript,
                            issuer: name,
                            ordinal,
                            agent,
                            call,
                            kind: *kind,
                            message: message.as_ref(),
                            prompt: named(prompt.as_ref()),
                        });
                    }
                }
                DelegationStep::Stop { agent, call, .. } => {
                    ends.push((transcript, ordinal, agent, call));
                }
                DelegationStep::Receive {
                    agent,
                    kind,
                    message,
                    prompt,
                } => {
                    let received = agents.entry(agent).or_insert_with(|| Received {
                        home: transcript,
                        ..Received::default()
                    });
                    if received.home != transcript
                        || *homes.entry(transcript).or_insert(agent) != agent.as_str()
                    {
                        return None;
                    }
                    received.all.push(ordinal);
                    let prompt = named(prompt.as_ref());
                    match (kind, message, prompt) {
                        (ReceiveKind::Dispatch, ..) => {
                            received.dispatches.push((ordinal, prompt));
                        }
                        (ReceiveKind::Resume, Some(message), Some(prompt)) => {
                            received
                                .resumes
                                .entry((message, prompt))
                                .or_default()
                                .push(ordinal);
                        }
                        (ReceiveKind::Resume | ReceiveKind::Input, ..) => {}
                    }
                }
            }
        }
        for received in agents.values_mut() {
            received.all.sort_unstable();
            if received.dispatches.len() > 1 {
                return None;
            }
        }
        let (mut edges, matched) = match_starts(session, &starts, &agents, contenders, senders);
        for (transcript, ordinal, agent, call) in ends {
            let Some(&(started, issuer, begun)) = matched.get(call) else {
                continue;
            };
            if started != agent || issuer != transcript {
                continue;
            }
            let received = &agents[agent];
            let next = received.all.partition_point(|&o| o <= begun);
            edges.push(Edge {
                from: received.home,
                below: received.all.get(next).copied().unwrap_or(u64::MAX),
                to: transcript,
                at: ordinal,
            });
        }
        let arrivals = clocks(transcripts.len(), &edges)?;
        Some(Self {
            transcripts,
            arrivals,
        })
    }

    /// What is known at `ordinal` of `transcript`: the merged clocks of
    /// its arrivals at or before it, `None` when there are none.
    fn known(&self, transcript: usize, ordinal: u64) -> Option<&Vec<u64>> {
        let arrivals = &self.arrivals[transcript];
        let count = arrivals.partition_point(|(at, _)| *at <= ordinal);
        count.checked_sub(1).map(|last| &arrivals[last].1)
    }
}

/// Who could have sent work in each issuer prompt, as far as the records
/// of every session show: prompt IDs are unique, and an agent of one
/// session can be resumed from another. A transcript sends in prompt P
/// only after receiving work in P: an agent's records carry the prompt of
/// the message that last started it. A resume's receipt is matched only
/// when no other recorded transcript could have sent it; that a sender
/// whose records are missing did not send the same agent the identical
/// message in the same prompt is assumed, and stated in the docs.
struct Senders<'a> {
    /// The transcripts that receive work: the agents' own.
    homes: BTreeSet<&'a str>,
    /// For each prompt, the issuers with a start in it (`None` for a start
    /// without a transcript); at most two.
    issuers: BTreeMap<&'a str, Vec<Option<Issuer<'a>>>>,
    /// For each prompt and issuer, its earliest starts in that prompt.
    starts: BTreeMap<(&'a str, Issuer<'a>), Earliest<'a>>,
    /// For each prompt, agents with a receive of any kind in it, except an
    /// agent whose dispatch there is matched with its start; at most two.
    unexplained: BTreeMap<&'a str, Vec<&'a str>>,
    /// Agents with a start or a receive that names no prompt (`None` for a
    /// start naming no single agent): they may be working in any; at most
    /// two.
    unprompted: Vec<Option<&'a str>>,
}

impl<'a> Senders<'a> {
    fn new(sightings: &[&'a Delegation], contenders: &Contenders<'a>) -> Self {
        let mut senders = Self {
            homes: BTreeSet::new(),
            issuers: BTreeMap::new(),
            starts: BTreeMap::new(),
            unexplained: BTreeMap::new(),
            unprompted: Vec::new(),
        };
        let mut sorted: Vec<&Delegation> = sightings.to_vec();
        sorted.sort_by_key(|s| (s.transcript.as_deref(), s.source_ordinal));
        // Each agent's dispatch starts and dispatch receives, by session
        // and prompt; the first two, which tell one from many.
        let mut dispatched: BTreeMap<&str, Vec<(&SessionId, Option<&str>)>> = BTreeMap::new();
        let mut received: BTreeMap<&str, Vec<(&SessionId, Option<&str>)>> = BTreeMap::new();
        for step in &sorted {
            match &step.step {
                DelegationStep::Start { kind, prompt, .. } => {
                    let agent = contenders.agent(step);
                    let Some(prompt) = named(prompt.as_ref()) else {
                        note(&mut senders.unprompted, agent);
                        continue;
                    };
                    if let (StartKind::Dispatch, Some(agent)) = (kind, agent) {
                        first_two(
                            dispatched.entry(agent).or_default(),
                            (&step.session, Some(prompt)),
                        );
                    }
                    let issuer = step.transcript.as_deref().map(|t| (&step.session, t));
                    note(senders.issuers.entry(prompt).or_default(), issuer);
                    let Some(issuer) = issuer else {
                        continue;
                    };
                    let earliest = senders.starts.entry((prompt, issuer)).or_insert(Earliest {
                        at: step.source_ordinal,
                        agent,
                        other: None,
                    });
                    if (earliest.agent != agent || agent.is_none()) && earliest.other.is_none() {
                        earliest.other = Some(step.source_ordinal);
                    }
                }
                DelegationStep::Receive {
                    agent,
                    kind: ReceiveKind::Dispatch,
                    prompt,
                    ..
                } => {
                    first_two(
                        received.entry(agent).or_default(),
                        (&step.session, named(prompt.as_ref())),
                    );
                }
                DelegationStep::Receive { .. } | DelegationStep::Stop { .. } => {}
            }
        }
        // The prompt an agent's dispatch was matched in, as `match_starts`
        // matches it: one dispatch receive, one start that could have sent
        // it, both in one session and one prompt. The agent began working
        // there only after that start; whether the start came after the
        // resume is for `alone` to check.
        let explained = |agent: &str| -> Option<&str> {
            let &[(session, Some(prompt))] = received.get(agent)?.as_slice() else {
                return None;
            };
            let &[(by, Some(sent))] = dispatched.get(agent)?.as_slice() else {
                return None;
            };
            (by == session && sent == prompt && contenders.dispatching(session, agent) == 1)
                .then_some(prompt)
        };
        for step in &sorted {
            let DelegationStep::Receive { agent, prompt, .. } = &step.step else {
                continue;
            };
            if let Some(home) = step.transcript.as_deref() {
                senders.homes.insert(home);
            }
            match named(prompt.as_ref()) {
                None => note(&mut senders.unprompted, Some(agent.as_str())),
                Some(prompt) if Some(prompt) != explained(agent) => {
                    note(senders.unexplained.entry(prompt).or_default(), agent);
                }
                Some(_) => {}
            }
        }
        senders
    }

    /// Whether `issuer`, resuming `agent` at `ordinal` in `prompt`, is the
    /// only recorded transcript that could have sent `agent` work in that
    /// prompt by then: a root transcript (one that receives no work); no
    /// other transcript with a start in that prompt; no earlier start of
    /// another agent in it; and no other agent with a receive in it, or
    /// with a start or receive naming no prompt. A receive in the prompt is
    /// excused only for an agent whose dispatch there is matched with a
    /// start, which these checks place in `issuer` after the resume: it
    /// began working after the resume was sent.
    fn alone(&self, issuer: Issuer, ordinal: u64, prompt: &str, agent: &str) -> bool {
        if self.homes.contains(issuer.1) {
            return false;
        }
        if self
            .issuers
            .get(prompt)
            .is_some_and(|issuers| issuers.iter().any(|&t| t != Some(issuer)))
        {
            return false;
        }
        if let Some(earliest) = self.starts.get(&(prompt, issuer)) {
            let other = if earliest.agent == Some(agent) {
                earliest.other
            } else {
                Some(earliest.at)
            };
            if other.is_some_and(|at| at <= ordinal) {
                return false;
            }
        }
        !self
            .unexplained
            .get(prompt)
            .is_some_and(|agents| agents.iter().any(|&other| other != agent))
            && !self.unprompted.iter().any(|&other| other != Some(agent))
    }
}

/// An issuing transcript within one session. Its ordinals are comparable
/// only within that session: another session's lines in the same file may
/// come from a rewritten transcript.
type Issuer<'a> = (&'a SessionId, &'a str);

/// An issuer's earliest starts in one prompt.
struct Earliest<'a> {
    /// The first start's position.
    at: u64,
    /// The agent it names, `None` when it names no single agent.
    agent: Option<&'a str>,
    /// The position of the first start naming another agent, or none.
    other: Option<u64>,
}

/// Adds `item` to `list` unless `list` holds two already.
fn first_two<T>(list: &mut Vec<T>, item: T) {
    if list.len() < 2 {
        list.push(item);
    }
}

/// Adds `item` to `list` unless it is there or `list` holds two already:
/// two distinct items answer whether any differs from a given one.
fn note<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if list.len() < 2 && !list.contains(&item) {
        list.push(item);
    }
}

/// Each matched call with its agent, its issuer, and the position of the
/// receive it was matched with.
type Matched<'a> = BTreeMap<&'a str, (&'a str, usize, u64)>;

/// The edges from `session`'s starts to the receives they match, and the
/// matched calls. A start is matched with a receive only when that is the
/// one receive of its kind, message, and prompt in the agent's transcript
/// and no other start, in any session, could have started it.
fn match_starts<'a>(
    session: &SessionId,
    starts: &[Start<'a>],
    agents: &BTreeMap<&'a str, Received<'a>>,
    contenders: &Contenders,
    senders: &Senders,
) -> (Vec<Edge>, Matched<'a>) {
    let mut edges = Vec::new();
    let mut matched = BTreeMap::new();
    for start in starts {
        let Some(received) = agents.get(start.agent) else {
            continue;
        };
        let Some(prompt) = start.prompt else {
            continue;
        };
        let at = match (start.kind, start.message) {
            (StartKind::Dispatch, _) => match received.dispatches.as_slice() {
                &[(at, Some(received))]
                    if received == prompt && contenders.dispatching(session, start.agent) == 1 =>
                {
                    Some(at)
                }
                _ => None,
            },
            (StartKind::Resume, Some(message)) => {
                match received.resumes.get(&(message, prompt)).map(Vec::as_slice) {
                    Some(&[at])
                        if contenders.resuming(start.agent, message, prompt) == 1
                            && !contenders.unnamed.contains(session)
                            && senders.alone(
                                (session, start.issuer),
                                start.ordinal,
                                prompt,
                                start.agent,
                            ) =>
                    {
                        Some(at)
                    }
                    _ => None,
                }
            }
            (StartKind::Resume, None) => None,
        };
        if let Some(at) = at {
            edges.push(Edge {
                from: start.transcript,
                below: start.ordinal,
                to: received.home,
                at,
            });
            matched.insert(start.call, (start.agent, start.transcript, at));
        }
    }
    (edges, matched)
}

/// For each transcript, its arrivals by position with what is known at
/// each: for every transcript, the position below which all it recorded
/// happened before. What an edge carries is its source's position and
/// what was known at its source before it left; what is known at an
/// arrival adds that to what was known at the arrival before it.
///
/// Each arrival depends on at most two others, so the work is about edges
/// times transcripts. `None` when that exceeds [`MAX_CLOCK_POSITIONS`],
/// when the edges form a cycle, or when an edge would place its
/// destination's later records before its own arrival: the steps
/// contradict each other.
fn clocks(transcripts: usize, edges: &[Edge]) -> Option<Arrivals> {
    if edges.len().saturating_mul(transcripts) > MAX_CLOCK_POSITIONS {
        return None;
    }
    let mut arrivals: Vec<Vec<usize>> = vec![Vec::new(); transcripts];
    for (i, edge) in edges.iter().enumerate() {
        arrivals[edge.to].push(i);
    }
    for list in &mut arrivals {
        list.sort_by_key(|&i| (edges[i].at, i));
    }
    let mut place = vec![0; edges.len()];
    for list in &arrivals {
        for (j, &i) in list.iter().enumerate() {
            place[i] = j;
        }
    }
    // What each arrival needs first: the arrival before it at its
    // destination, and the last arrival at its source before it left.
    let needs: Vec<[Option<usize>; 2]> = edges
        .iter()
        .enumerate()
        .map(|(i, edge)| {
            let previous = place[i].checked_sub(1).map(|j| arrivals[edge.to][j]);
            let at_source = &arrivals[edge.from];
            let left = at_source.partition_point(|&k| edges[k].at < edge.below);
            [previous, left.checked_sub(1).map(|j| at_source[j])]
        })
        .collect();
    let mut waiting: Vec<usize> = needs.iter().map(|n| n.iter().flatten().count()).collect();
    let mut needed_by: Vec<Vec<usize>> = vec![Vec::new(); edges.len()];
    for (i, need) in needs.iter().enumerate() {
        for &j in need.iter().flatten() {
            needed_by[j].push(i);
        }
    }
    let mut ready: Vec<usize> = (0..edges.len()).filter(|&i| waiting[i] == 0).collect();
    let mut known: Vec<Option<Vec<u64>>> = vec![None; edges.len()];
    while let Some(i) = ready.pop() {
        let edge = &edges[i];
        let [previous, left] = needs[i];
        let mut clock = vec![0; transcripts];
        clock[edge.from] = edge.below;
        if let Some(j) = left {
            merge(&mut clock, known[j].as_ref()?);
        }
        if clock[edge.to] > edge.at {
            return None;
        }
        if let Some(j) = previous {
            merge(&mut clock, known[j].as_ref()?);
        }
        known[i] = Some(clock);
        for &k in &needed_by[i] {
            waiting[k] -= 1;
            if waiting[k] == 0 {
                ready.push(k);
            }
        }
    }
    arrivals
        .into_iter()
        .map(|list| {
            list.into_iter()
                .map(|i| Some((edges[i].at, known[i].take()?)))
                .collect()
        })
        .collect()
}

/// A recorded prompt, `None` when it is missing or empty: an empty prompt
/// identifies nothing.
fn named(prompt: Option<&String>) -> Option<&str> {
    prompt
        .map(String::as_str)
        .filter(|prompt| !prompt.is_empty())
}

/// Raises each position of `into` to `from`'s where `from`'s is higher.
fn merge(into: &mut [u64], from: &[u64]) {
    for (mine, theirs) in into.iter_mut().zip(from) {
        *mine = (*mine).max(*theirs);
    }
}
