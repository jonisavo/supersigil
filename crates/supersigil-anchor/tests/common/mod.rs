//! Builders for edits and requests shared by the anchor tests.
#![allow(
    dead_code,
    reason = "each test binary uses a different subset of these helpers"
)]

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use supersigil_anchor::{CandidateEdit, DEFAULT_BUDGET_BYTES, Order, Request, State, TargetKind};
use supersigil_record::observations::{
    Delegation, DelegationStep, Edit, EditOperation, EditOrigin, FileState, Hunk, Material,
    ReceiveKind, StartKind, StopStatus,
};
use supersigil_record::{ContentId, EventId, RecordId, SessionId, Timestamp, TurnId};

/// The reviewed worktree in tests.
pub const WT: &str = "/work/repo";

/// An event id named `name`.
pub fn id(name: &str) -> EventId {
    EventId::new(name)
}

/// Event ids named `names`, in order.
pub fn ids(names: &[&str]) -> Vec<EventId> {
    names.iter().map(|n| id(n)).collect()
}

#[expect(
    clippy::too_many_arguments,
    reason = "a private builder behind the named helpers below"
)]
fn edit(
    id: &str,
    transcript: &str,
    ordinal: u64,
    operation: EditOperation,
    old: Material<String>,
    new: Material<String>,
    replace_all: bool,
    before: FileState,
    after: FileState,
) -> Edit {
    Edit {
        id: EventId::new(id),
        turn: TurnId::new(format!("turn-{id}")),
        session: SessionId::new("s1"),
        path: PathBuf::from("f.txt"),
        before,
        after,
        patch: Material::unavailable("test"),
        old_text: old,
        new_text: new,
        replace_all,
        operation,
        origin: EditOrigin::Tool,
        checkout: PathBuf::from(WT),
        time: Timestamp::new("2026-09-29T10:00:00.000Z"),
        source_ordinal: ordinal,
        agent_id: None,
        transcript: Some(transcript.to_owned()),
    }
}

fn text(value: &str) -> Material<String> {
    Material::Retained(value.to_owned())
}

fn known(content: &str) -> FileState {
    FileState::known(ContentId::of(content.as_bytes()))
}

/// An Edit-tool replacement with unknown before- and after-states.
pub fn replace(id: &str, transcript: &str, ordinal: u64, old: &str, new: &str) -> Edit {
    edit(
        id,
        transcript,
        ordinal,
        EditOperation::Replace,
        text(old),
        text(new),
        false,
        FileState::unknown(),
        FileState::unknown(),
    )
}

/// An Edit-tool replacement of every occurrence, with unknown states.
pub fn replace_all(id: &str, transcript: &str, ordinal: u64, old: &str, new: &str) -> Edit {
    let mut e = replace(id, transcript, ordinal, old, new);
    e.replace_all = true;
    e
}

/// A Write that creates the file: before absent, after known.
pub fn create(id: &str, transcript: &str, ordinal: u64, content: &str) -> Edit {
    edit(
        id,
        transcript,
        ordinal,
        EditOperation::Write,
        Material::unavailable("write replaces the whole file"),
        text(content),
        false,
        FileState::Absent,
        known(content),
    )
}

/// A Write that overwrites the file: before unknown, after known.
pub fn overwrite(id: &str, transcript: &str, ordinal: u64, content: &str) -> Edit {
    edit(
        id,
        transcript,
        ordinal,
        EditOperation::Write,
        Material::unavailable("write replaces the whole file"),
        text(content),
        false,
        FileState::unknown(),
        known(content),
    )
}

/// An append to an existing file, with unknown before- and after-states.
pub fn append(id: &str, transcript: &str, ordinal: u64, text_added: &str) -> Edit {
    edit(
        id,
        transcript,
        ordinal,
        EditOperation::Append,
        Material::unavailable("append"),
        text(text_added),
        false,
        FileState::unknown(),
        FileState::unknown(),
    )
}

/// An append that created the file: before absent, after unknown.
pub fn append_creating(id: &str, transcript: &str, ordinal: u64, text_added: &str) -> Edit {
    let mut e = append(id, transcript, ordinal, text_added);
    e.before = FileState::Absent;
    e
}

/// `edit` as the heredoc write at `index` in the shell command `command`.
pub fn written_by(mut edit: Edit, command: &str, index: u64) -> Edit {
    edit.origin = EditOrigin::Shell {
        command: id(command),
        index,
    };
    edit
}

/// A replacement whose operation was not recorded.
pub fn unknown_op(id: &str, transcript: &str, ordinal: u64, old: &str, new: &str) -> Edit {
    let mut e = replace(id, transcript, ordinal, old, new);
    e.operation = EditOperation::Unknown;
    e
}

/// Sets both recorded hashes from the given contents.
pub fn with_hashes(mut edit: Edit, before: &str, after: &str) -> Edit {
    edit.before = known(before);
    edit.after = known(after);
    edit
}

/// Retains the patch Claude Code records for an edit from `before` to
/// `after`, built as jsdiff builds it: one hunk around the lines between the
/// common leading and trailing lines, with three lines of context, line
/// terminators dropped, a marker after a last line that has none, and every
/// tab displayed as two spaces. Identical texts give no hunk.
pub fn with_patch(mut edit: Edit, before: &str, after: &str) -> Edit {
    edit.patch = Material::Retained(patch(before, after));
    edit
}

fn patch(before: &str, after: &str) -> Vec<Hunk> {
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    let limit = old.len().min(new.len());
    let prefix = (0..limit).take_while(|&k| old[k] == new[k]).count();
    if prefix == old.len() && prefix == new.len() {
        return Vec::new();
    }
    let suffix = (0..limit - prefix)
        .take_while(|&k| old[old.len() - 1 - k] == new[new.len() - 1 - k])
        .count();
    let start = prefix.saturating_sub(3);
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
    let context_end = (old_end + 3).min(old.len());
    let mut lines = Vec::new();
    let mut show = |sign: char, line: &str| {
        let text = line.strip_suffix('\n').unwrap_or(line);
        lines.push(format!("{sign}{}", text.replace('\t', "  ")));
        if !line.ends_with('\n') {
            lines.push("\\ No newline at end of file".to_owned());
        }
    };
    old[start..prefix].iter().for_each(|l| show(' ', l));
    old[prefix..old_end].iter().for_each(|l| show('-', l));
    new[prefix..new_end].iter().for_each(|l| show('+', l));
    old[old_end..context_end].iter().for_each(|l| show(' ', l));
    // Unified-diff convention: an empty range starts at the line before it.
    let range = |count: usize| {
        let first = if count == 0 { start } else { start + 1 };
        (u32::try_from(first).unwrap(), u32::try_from(count).unwrap())
    };
    let (old_start, old_lines) = range(context_end - start);
    let (new_start, new_lines) = range(context_end - old_end + new_end - start);
    vec![Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        lines,
    }]
}

/// Offers `edit` from record `r1` in `worktree`.
pub fn in_worktree(edit: Edit, worktree: &str) -> CandidateEdit {
    CandidateEdit {
        record: RecordId::new("r1"),
        worktree: PathBuf::from(worktree),
        edit,
    }
}

/// Offers `edit` from record `r1` in the reviewed worktree.
pub fn candidate(edit: Edit) -> CandidateEdit {
    in_worktree(edit, WT)
}

/// A present state holding `text`, or absent for `None`.
pub fn state(text: Option<&str>) -> State {
    text.map_or(State::Absent, |t| State::Present(t.as_bytes().to_vec()))
}

/// Runs `work` on another thread and returns its result, failing the test
/// when it takes longer than `limit` times [`time_limit_scale`]. A hang
/// becomes a failure instead of a stuck test run; the thread is left to the
/// process's exit.
pub fn finishes_within<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(work());
    });
    result
        .recv_timeout(limit * time_limit_scale())
        .expect("the work finishes within its time limit")
}

/// The whole number in `SUPERSIGIL_TIME_LIMIT_SCALE`, or 1 when it is unset,
/// so a slow CI runner can allow more time while others keep the limits.
///
/// # Panics
///
/// Panics if the variable is set to anything but a positive whole number.
fn time_limit_scale() -> u32 {
    std::env::var("SUPERSIGIL_TIME_LIMIT_SCALE").map_or(1, |scale| {
        scale
            .parse()
            .ok()
            .filter(|&scale| scale > 0)
            .expect("SUPERSIGIL_TIME_LIMIT_SCALE is a positive whole number")
    })
}

/// Lines of a state, counted the way anchor counts them.
pub fn line_count(state: &State) -> usize {
    supersigil_anchor::lines::line_starts(state.bytes().unwrap_or_default()).len()
}

/// A request with the default budget, reviewing [`WT`].
pub fn request(
    base: Option<&str>,
    target: Option<&str>,
    kind: TargetKind,
    edits: Vec<CandidateEdit>,
) -> Request {
    let (edits, conflicts) = supersigil_anchor::walk::dedup(edits);
    Request {
        base: state(base),
        target: state(target),
        target_kind: kind,
        reviewed_worktree: PathBuf::from(WT),
        edits,
        conflicts,
        budget_bytes: DEFAULT_BUDGET_BYTES,
        order: Arc::default(),
    }
}

/// `request` with `order` as the recorded order of its session.
pub fn ordered(mut request: Request, order: Order) -> Request {
    request.order = Arc::new(order);
    request
}

/// The order of session `s1` when a main transcript dispatched each of
/// `transcripts` as an agent and learned it had stopped before dispatching
/// the next.
pub fn in_turn(transcripts: &[&str]) -> Order {
    delegated(transcripts, true)
}

/// The order of session `s1` when a main transcript dispatched every one of
/// `transcripts` as an agent before learning that any had stopped.
pub fn at_once(transcripts: &[&str]) -> Order {
    delegated(transcripts, false)
}

fn delegated(transcripts: &[&str], in_turn: bool) -> Order {
    let count = transcripts.len() as u64;
    let mut steps = Vec::new();
    for (k, &agent) in (0u64..).zip(transcripts) {
        let (started, stopped) = if in_turn {
            (10 * k + 1, 10 * k + 5)
        } else {
            (k + 1, 10 * count + k)
        };
        steps.extend(dispatched("main", started, agent));
        steps.push(delegation(
            &format!("stop-{agent}"),
            "main",
            stopped,
            DelegationStep::Stop {
                agent: agent.to_owned(),
                call: format!("call-{agent}"),
                status: StopStatus::Completed,
            },
        ));
    }
    Order::new(&steps)
}

/// A delegation step of session `s1`, recorded at `ordinal` of
/// `transcript`.
pub fn delegation(id: &str, transcript: &str, ordinal: u64, step: DelegationStep) -> Delegation {
    Delegation {
        id: EventId::new(id),
        session: SessionId::new("s1"),
        step,
        source_ordinal: ordinal,
        transcript: Some(transcript.to_owned()),
    }
}

/// `issuer` dispatching `agent` at `ordinal` with call `call-<agent>` in
/// prompt `p`, and the agent's transcript, named after it, receiving the
/// dispatch on its first line.
pub fn dispatched(issuer: &str, ordinal: u64, agent: &str) -> [Delegation; 2] {
    let prompt = Some("p".to_owned());
    [
        delegation(
            &format!("start-{agent}"),
            issuer,
            ordinal,
            DelegationStep::Start {
                agent: Some(agent.to_owned()),
                call: format!("call-{agent}"),
                kind: StartKind::Dispatch,
                message: None,
                prompt: prompt.clone(),
            },
        ),
        delegation(
            &format!("receive-{agent}"),
            agent,
            0,
            DelegationStep::Receive {
                agent: agent.to_owned(),
                kind: ReceiveKind::Dispatch,
                message: None,
                prompt,
            },
        ),
    ]
}
