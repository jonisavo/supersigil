//! Walks backward from the target through recorded edits and collects every
//! chain the evidence supports.
//!
//! Edits are grouped by (worktree, transcript); within a group, edits with
//! equal ordinals form an unordered bucket and buckets are ordered by
//! ordinal. A chain starts at a head, continues backward through its group
//! without skipping an edit, and may enter another group only through a
//! verified link: an edit whose recorded after-hash equals the hash of the
//! current bytes. It ends at the first state equal to the base, or where no
//! predecessor is accepted. Time never decides.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use supersigil_record::observations::{Edit, EditOperation};
use supersigil_record::{ContentId, EventId, RecordId};

use crate::align::{Alignment, Alignments, advance, align};
use crate::input::{CandidateEdit, Request, State, TargetKind};
use crate::result::{Chain, ChainClass, ChainEnd, Conflict, PathStatus, StopReason};
use crate::step::{Budget, Reversal, Reversed, is_known, reverse};

/// An edit that survived deduplication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedEdit {
    /// The edit, with a known operation when any sighting had one.
    pub edit: Edit,
    /// Root of the worktree the edit's file lies in.
    pub worktree: PathBuf,
    /// Every record holding a sighting of it.
    pub records: Vec<RecordId>,
}

/// Applies the record's event-identity rule.
///
/// Within one record the first sighting of an edit id wins, as in derive.
/// Sightings in different records are one edit when their payloads and
/// worktrees are equal, or differ only in that one has `operation: unknown`
/// where the other has a known operation (sync never rewrites an older log);
/// the known operation is kept and every record is listed. Any other
/// difference excludes the edit as a [`Conflict`]; nothing is resolved by
/// order. Accepted edits keep the order of their first sighting.
#[must_use]
pub fn dedup(edits: Vec<CandidateEdit>) -> (Vec<AcceptedEdit>, Vec<Conflict>) {
    struct Merge {
        accepted: AcceptedEdit,
        conflict: bool,
    }
    let mut first_in_record: BTreeSet<(RecordId, EventId)> = BTreeSet::new();
    let mut position: BTreeMap<EventId, usize> = BTreeMap::new();
    let mut merged: Vec<Merge> = Vec::new();
    for candidate in edits {
        let id = candidate.edit.id.clone();
        if !first_in_record.insert((candidate.record.clone(), id.clone())) {
            continue;
        }
        let Some(&at) = position.get(&id) else {
            position.insert(id, merged.len());
            merged.push(Merge {
                accepted: AcceptedEdit {
                    edit: candidate.edit,
                    worktree: candidate.worktree,
                    records: vec![candidate.record],
                },
                conflict: false,
            });
            continue;
        };
        let merge = &mut merged[at];
        merge.accepted.records.push(candidate.record);
        if merge.conflict {
            continue;
        }
        match compatible(
            &merge.accepted.edit,
            &merge.accepted.worktree,
            &candidate.edit,
            &candidate.worktree,
        ) {
            Some(edit) => merge.accepted.edit = edit,
            None => merge.conflict = true,
        }
    }
    let mut accepted = Vec::new();
    let mut conflicts = Vec::new();
    for merge in merged {
        if merge.conflict {
            conflicts.push(Conflict {
                edit: merge.accepted.edit.id,
                records: merge.accepted.records,
            });
        } else {
            accepted.push(merge.accepted);
        }
    }
    (accepted, conflicts)
}

/// Merges two sightings of one edit, or `None` when they conflict.
fn compatible(a: &Edit, a_worktree: &Path, b: &Edit, b_worktree: &Path) -> Option<Edit> {
    if a_worktree != b_worktree {
        return None;
    }
    if a == b {
        return Some(a.clone());
    }
    let (known, unknown) = match (a.operation, b.operation) {
        (EditOperation::Unknown, op) if op != EditOperation::Unknown => (b, a),
        (op, EditOperation::Unknown) if op != EditOperation::Unknown => (a, b),
        _ => return None,
    };
    let mut lifted = unknown.clone();
    lifted.operation = known.operation;
    (lifted == *known).then(|| known.clone())
}

/// One reversed step of a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStep {
    /// The edit.
    pub edit: EventId,
    /// The state before the edit.
    pub before: State,
    /// The state after the edit.
    pub after: State,
    /// The line alignment this branch uses for the step.
    pub alignment: Alignment,
}

/// A chain with the states and alignments its walk established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkedChain {
    /// The chain.
    pub chain: Chain,
    /// The state before its oldest step.
    pub start: State,
    /// Its steps, oldest first.
    pub steps: Vec<ChainStep>,
}

/// Everything the walk of one path found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
    /// Whether the search completed and found anything.
    pub status: PathStatus,
    /// Chains whose readings are combined line by line.
    pub chains: Vec<WalkedChain>,
    /// Alternatives that assume an unrecorded change: chains that do not
    /// end at the base, covered by one that does (see [`walk`]).
    pub set_aside: Vec<WalkedChain>,
    /// The deduplicated edits.
    pub accepted: Vec<AcceptedEdit>,
    /// Edits excluded as conflicting evidence.
    pub conflicts: Vec<Conflict>,
}

/// Walks backward from `request.target` and returns every chain found.
///
/// Heads are the edits whose reversal from the target is accepted: for a
/// working-tree target, the edits of the last bucket of each group in the
/// reviewed worktree (every pinned observation predates the target, so a
/// group's later edits supersede its earlier states) and any edit of a
/// group in another worktree; for a commit target, any edit. Every order of
/// a bucket's edits is tried. At every state both the local predecessors
/// (the rest of the current bucket, or else the previous bucket of the
/// group) and every verified link into a group not yet entered are
/// explored; leaving through a link mid-bucket abandons the bucket's other
/// edits. Chains are classed by what verifies them.
///
/// A chain that does not end at the base is set aside when a chain that
/// does end there entered, in the same order, every group the first one
/// entered, possibly followed by more, and used, in each, every edit the
/// first one used: the two differ only in unrecorded choices (replacement
/// locations, bucket orders, alignments) and in how far back the recorded
/// evidence reaches, and the first holds no recorded edit the second
/// lacks. It may use fewer, when a recorded check (an after-hash, a
/// creation's content) stopped it early, or when the base-reaching chain
/// went on through a verified link into a group the first never reached.
/// A chain holding an edit that no base-reaching chain holds is never set
/// aside, so no recorded evidence is dropped.
///
/// Subsets and orders of a bucket are generated one at a time, never
/// materialized. Every generated branch is charged 1 KiB, and the bucket
/// indices read to generate it, before it is explored; every entry of a
/// state's list of links the group index read to check it, entered or not;
/// every reversal candidate its bytes (see [`reverse`]); and every
/// alignment the step's bytes. Chains found on one path share it while it
/// is explored: each copy of a step into a chain after the first is
/// charged what it holds.
/// Setting chains aside compares their groups and edits, each distinct
/// pair once, and each comparison is charged the indices it reads.
/// Enumeration stops the moment the budget is exhausted. The status is then
/// [`PathStatus::SearchIncomplete`]: the chains found so far are returned,
/// and a chain whose comparison was refused is not set aside.
#[must_use]
pub fn walk(request: &Request) -> Walk {
    let (accepted, conflicts) = dedup(request.edits.clone());
    let groups = group(&accepted);
    let mut search = Search::new(request, &accepted, &groups);
    search.run();
    let Search {
        mut budget,
        found,
        head_failures,
        ..
    } = search;
    let (chains, set_aside) = settle(found, &mut budget);
    let status = if budget.exhausted() {
        PathStatus::SearchIncomplete
    } else if !chains.is_empty() {
        PathStatus::Composed
    } else if !groups.is_empty() {
        PathStatus::NotComposed {
            reasons: head_failures.into_iter().collect(),
        }
    } else {
        PathStatus::NotComposed {
            reasons: vec![StopReason::NoPredecessor],
        }
    };
    Walk {
        status,
        chains,
        set_aside,
        accepted,
        conflicts,
    }
}

/// The edits of one file from one transcript.
struct Group {
    worktree: PathBuf,
    /// Indices into the accepted edits, one bucket per ordinal, oldest first.
    buckets: Vec<Vec<usize>>,
}

fn group(accepted: &[AcceptedEdit]) -> Vec<Group> {
    let mut by_key: BTreeMap<(PathBuf, Option<String>), Vec<usize>> = BTreeMap::new();
    for (i, a) in accepted.iter().enumerate() {
        by_key
            .entry((a.worktree.clone(), a.edit.transcript.clone()))
            .or_default()
            .push(i);
    }
    by_key
        .into_iter()
        .map(|((worktree, _), mut indices)| {
            indices.sort_by_key(|&i| accepted[i].edit.source_ordinal);
            let buckets = indices
                .chunk_by(|&a, &b| {
                    accepted[a].edit.source_ordinal == accepted[b].edit.source_ordinal
                })
                .map(<[usize]>::to_vec)
                .collect();
            Group { worktree, buckets }
        })
        .collect()
}

/// The group of each of `count` accepted edits.
fn group_of(groups: &[Group], count: usize) -> Vec<usize> {
    let mut of = vec![0; count];
    for (g, grp) in groups.iter().enumerate() {
        for &idx in grp.buckets.iter().flatten() {
            of[idx] = g;
        }
    }
    of
}

/// The verified links into each group, found by a state's hash, never by
/// reading every edit: for every recorded after-hash, a list of the edits
/// that recorded it, as (group, (bucket, edit) in walk order) entries in
/// group order.
#[derive(Default)]
struct Links {
    lists: Vec<Vec<LinkEntry>>,
    /// The position in `lists` of each after-hash's list.
    by_hash: BTreeMap<ContentId, usize>,
}

/// A group whose edits recorded one after-hash, with those edits as
/// (bucket, edit) in walk order.
type LinkEntry = (usize, Vec<(usize, usize)>);

impl Links {
    fn new(accepted: &[AcceptedEdit], groups: &[Group]) -> Self {
        let mut links = Self::default();
        for (g, grp) in groups.iter().enumerate() {
            for (b, bucket) in grp.buckets.iter().enumerate() {
                for &idx in bucket {
                    let Some(hash) = accepted[idx].edit.after.content_id() else {
                        continue;
                    };
                    let next = links.lists.len();
                    let slot = *links.by_hash.entry(hash.clone()).or_insert(next);
                    if slot == next {
                        links.lists.push(Vec::new());
                    }
                    let entries = &mut links.lists[slot];
                    match entries.last_mut() {
                        Some((last, edits)) if *last == g => edits.push((b, idx)),
                        _ => entries.push((g, vec![(b, idx)])),
                    }
                }
            }
        }
        links
    }
}

fn without(bucket: &[usize], idx: usize) -> Vec<usize> {
    bucket.iter().copied().filter(|&i| i != idx).collect()
}

/// The subsets of a bucket's other edits (which of them came before the
/// entry edit), generated one at a time and never materialized together.
struct Subsets {
    items: Vec<usize>,
    /// Membership of the next subset; `None` when every subset was given.
    next: Option<Vec<bool>>,
}

impl Subsets {
    fn new(items: Vec<usize>) -> Self {
        let next = Some(vec![false; items.len()]);
        Self { items, next }
    }
}

impl Iterator for Subsets {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Vec<usize>> {
        let member = self.next.as_mut()?;
        let subset = self
            .items
            .iter()
            .zip(member.iter())
            .filter(|(_, in_it)| **in_it)
            .map(|(&item, _)| item)
            .collect();
        if !advance(member) {
            self.next = None;
        }
        Some(subset)
    }
}

/// What generating one branch costs the budget before it is explored, so
/// that branches failing cheaply still exhaust it.
const BRANCH_COST: usize = 1024;

/// One predecessor to try: reversing edit `idx` next, in `bucket` of
/// `group`, with `remaining` of that bucket's edits still to reverse before
/// the chain leaves it.
struct Choice {
    idx: usize,
    group: usize,
    bucket: usize,
    remaining: Vec<usize>,
    /// How many edits of the bucket were read to name `remaining`.
    read: usize,
}

/// Which of a bucket's other edits may still precede the one reversed next.
#[derive(Clone, Copy)]
enum Rest {
    /// All of them: the chain passes through the whole bucket.
    All,
    /// Any subset of them: the chain enters the bucket at this edit (a head
    /// or a link), and the others may have run after it.
    AnySubset,
}

/// The predecessors one bucket offers, generated one at a time: each of
/// `edits` reversed next, with the other edits of `pool` that must or may
/// still precede it.
struct Tries {
    group: usize,
    bucket: usize,
    edits: Vec<usize>,
    pool: Vec<usize>,
    rest: Rest,
    /// The position in `edits` of the next edit to reverse.
    position: usize,
    /// The subsets still to give for that edit, under [`Rest::AnySubset`].
    subsets: Option<Subsets>,
}

impl Tries {
    fn new(group: usize, bucket: usize, edits: Vec<usize>, pool: Vec<usize>, rest: Rest) -> Self {
        Self {
            group,
            bucket,
            edits,
            pool,
            rest,
            position: 0,
            subsets: None,
        }
    }
}

impl Iterator for Tries {
    type Item = Choice;

    fn next(&mut self) -> Option<Choice> {
        loop {
            let idx = *self.edits.get(self.position)?;
            let remaining = match self.rest {
                Rest::All => {
                    self.position += 1;
                    without(&self.pool, idx)
                }
                Rest::AnySubset => {
                    let pool = &self.pool;
                    let subsets = self
                        .subsets
                        .get_or_insert_with(|| Subsets::new(without(pool, idx)));
                    let Some(subset) = subsets.next() else {
                        self.subsets = None;
                        self.position += 1;
                        continue;
                    };
                    subset
                }
            };
            return Some(Choice {
                idx,
                group: self.group,
                bucket: self.bucket,
                remaining,
                read: self.pool.len(),
            });
        }
    }
}

/// Which predecessors of a state are still to be generated.
enum Cursor {
    /// At the target: the heads, from this group and bucket on.
    Heads { group: usize, bucket: usize },
    /// The rest of the current bucket, or else the previous bucket.
    Local,
    /// Verified links to the state's hash: from this entry of that hash's
    /// list in [`Links`] on, and this position in the entry's edits.
    Links {
        list: usize,
        entry: usize,
        position: usize,
    },
    /// Nothing more.
    Done,
}

/// Where a chain is: its group and bucket, and the edits of that bucket
/// still to reverse before it leaves.
struct At {
    group: usize,
    bucket: usize,
    remaining: Vec<usize>,
}

/// A state on the chain under construction whose predecessors are being
/// tried, one at a time.
struct Frame {
    /// `None` at the target, before any step.
    at: Option<At>,
    cursor: Cursor,
    /// The bucket whose predecessors are being generated.
    tries: Option<Tries>,
    /// The predecessor being tried: its accepted candidates, and the
    /// alignments of the current one, generated one at a time.
    trying: Option<Trying>,
    /// Whether any predecessor's reversal was accepted.
    progressed: bool,
    /// Why each rejected predecessor was rejected.
    reasons: BTreeSet<StopReason>,
}

impl Frame {
    fn new(at: Option<At>) -> Self {
        let cursor = if at.is_some() {
            Cursor::Local
        } else {
            Cursor::Heads {
                group: 0,
                bucket: 0,
            }
        };
        Self {
            at,
            cursor,
            tries: None,
            trying: None,
            progressed: false,
            reasons: BTreeSet::new(),
        }
    }
}

struct Trying {
    choice: Choice,
    candidates: std::vec::IntoIter<Reversal>,
    current: Option<(Reversal, Alignments)>,
}

impl Trying {
    /// The next accepted candidate with one of its alignments, charged the
    /// step's bytes; `None` when there is none or the charge is refused.
    fn next_child(
        &mut self,
        current: &State,
        accepted: &[AcceptedEdit],
        budget: &mut Budget,
    ) -> Option<Child> {
        loop {
            if let Some((reversal, alignments)) = &mut self.current
                && let Some(alignment) = alignments.next()
            {
                if !budget.charge(reversal.before.byte_len() + current.byte_len()) {
                    return None;
                }
                let choice = &self.choice;
                return Some(Child {
                    at: At {
                        group: choice.group,
                        bucket: choice.bucket,
                        remaining: choice.remaining.clone(),
                    },
                    step: PathStep {
                        idx: choice.idx,
                        step: ChainStep {
                            edit: accepted[choice.idx].edit.id.clone(),
                            before: reversal.before.clone(),
                            after: current.clone(),
                            alignment,
                        },
                        copied: false,
                    },
                });
            }
            let reversal = self.candidates.next()?;
            let alignments = align(&reversal.before, current, &reversal.replacements);
            self.current = Some((reversal, alignments));
        }
    }
}

/// A reversed step and where the chain is after it.
struct Child {
    at: At,
    step: PathStep,
}

/// A step of the chain under construction, with its accepted-edit index.
struct PathStep {
    idx: usize,
    step: ChainStep,
    /// Whether a finished chain already holds a copy of the step.
    copied: bool,
}

/// A finished chain before ids are assigned.
struct Found {
    /// Chains with equal keys differ only in unrecorded choices.
    key: Key,
    walked: WalkedChain,
}

/// A depth-first search over the chains, run on an explicit stack of
/// [`Frame`]s over one shared path: a branch costs only its own step, and
/// no call stack grows with the chain.
struct Search<'a> {
    request: &'a Request,
    accepted: &'a [AcceptedEdit],
    groups: &'a [Group],
    /// The group of each accepted edit.
    group_of: Vec<usize>,
    /// The verified links, by after-hash.
    links: Links,
    budget: Budget,
    /// The chain under construction, newest step first.
    path: Vec<PathStep>,
    /// How many steps of `path` lie in each group; a group with any has
    /// been entered.
    entered: Vec<usize>,
    found: Vec<Found>,
    head_failures: BTreeSet<StopReason>,
}

impl<'a> Search<'a> {
    fn new(request: &'a Request, accepted: &'a [AcceptedEdit], groups: &'a [Group]) -> Self {
        Self {
            request,
            accepted,
            groups,
            group_of: group_of(groups, accepted.len()),
            links: Links::new(accepted, groups),
            budget: Budget::new(request.budget_bytes),
            path: Vec::new(),
            entered: vec![0; groups.len()],
            found: Vec::new(),
            head_failures: BTreeSet::new(),
        }
    }

    /// The current state: the before-state of the newest reversed step, or
    /// the target before any.
    fn state(&self) -> &State {
        self.path
            .last()
            .map_or(&self.request.target, |p| &p.step.before)
    }

    fn push(&mut self, step: PathStep) {
        self.entered[self.group_of[step.idx]] += 1;
        self.path.push(step);
    }

    fn pop(&mut self) {
        if let Some(step) = self.path.pop() {
            self.entered[self.group_of[step.idx]] -= 1;
        }
    }

    /// Explores every chain depth first. A chain is finished at the first
    /// state equal to the base, or once every predecessor of its oldest
    /// state was tried and none was accepted.
    fn run(&mut self) {
        let mut frames = vec![Frame::new(None)];
        while let Some(frame) = frames.last_mut() {
            if let Some(child) = self.next_child(frame) {
                self.push(child.step);
                if *self.state() == self.request.base {
                    self.finish(ChainEnd::Base);
                    self.pop();
                } else {
                    frames.push(Frame::new(Some(child.at)));
                }
                continue;
            }
            if self.budget.exhausted() {
                return;
            }
            let frame = frames.pop().expect("the loop holds a frame");
            if frame.at.is_none() {
                self.head_failures = frame.reasons;
                return;
            }
            if !frame.progressed {
                let mut reasons = frame.reasons;
                if reasons.is_empty() {
                    reasons.insert(StopReason::NoPredecessor);
                }
                self.finish(ChainEnd::Stopped {
                    reasons: reasons.into_iter().collect(),
                });
            }
            self.pop();
        }
    }

    /// Generates the next child of `frame`'s state: an accepted predecessor
    /// with one alignment. Each predecessor is charged [`BRANCH_COST`], and
    /// the bucket indices read to generate it, before it is reversed. `None`
    /// once every predecessor was tried or the budget ran out.
    fn next_child(&mut self, frame: &mut Frame) -> Option<Child> {
        loop {
            if self.budget.exhausted() {
                return None;
            }
            if let Some(trying) = &mut frame.trying {
                let current = self
                    .path
                    .last()
                    .map_or(&self.request.target, |p| &p.step.before);
                if let Some(child) = trying.next_child(current, self.accepted, &mut self.budget) {
                    return Some(child);
                }
                frame.trying = None;
                continue;
            }
            let choice = self.next_choice(frame)?;
            let read = choice.read.saturating_mul(size_of::<usize>());
            if !self.budget.charge(BRANCH_COST.saturating_add(read)) {
                return None;
            }
            let current = self
                .path
                .last()
                .map_or(&self.request.target, |p| &p.step.before);
            let edit = &self.accepted[choice.idx].edit;
            match reverse(edit, current, &self.request.base, &mut self.budget) {
                Reversed::Stop(reason) => {
                    frame.reasons.insert(reason);
                }
                Reversed::Candidates(candidates) => {
                    frame.progressed |= !candidates.is_empty();
                    frame.trying = Some(Trying {
                        choice,
                        candidates: candidates.into_iter(),
                        current: None,
                    });
                }
            }
        }
    }

    /// The next predecessor of `frame`'s state to try, or `None`.
    fn next_choice(&mut self, frame: &mut Frame) -> Option<Choice> {
        loop {
            if let Some(choice) = frame.tries.as_mut().and_then(Iterator::next) {
                return Some(choice);
            }
            frame.tries = Some(self.open(frame.at.as_ref(), &mut frame.cursor)?);
        }
    }

    /// The next bucket of predecessors, advancing `cursor`: the heads at
    /// the target; elsewhere the rest of the current bucket or the previous
    /// one, then every verified link. `None` when there is none left or the
    /// budget ran out.
    fn open(&mut self, at: Option<&At>, cursor: &mut Cursor) -> Option<Tries> {
        loop {
            match cursor {
                Cursor::Heads { group, bucket } => {
                    let grp = self.groups.get(*group)?;
                    let g = *group;
                    if self.request.target_kind == TargetKind::WorkingTree
                        && grp.worktree == self.request.reviewed_worktree
                    {
                        // Its later edits supersede its earlier states.
                        *group += 1;
                        let last = grp.buckets.len() - 1;
                        let edits = grp.buckets[last].clone();
                        return Some(Tries::new(g, last, edits.clone(), edits, Rest::All));
                    }
                    if let Some(edits) = grp.buckets.get(*bucket) {
                        let b = *bucket;
                        *bucket += 1;
                        return Some(Tries::new(
                            g,
                            b,
                            edits.clone(),
                            edits.clone(),
                            Rest::AnySubset,
                        ));
                    }
                    *group += 1;
                    *bucket = 0;
                }
                Cursor::Local => {
                    let at = at.expect("a state after a step has a place");
                    // Hashing the state, and finding its list of links, is
                    // part of the step's charged bytes.
                    let list = if self.links.lists.is_empty() {
                        None
                    } else {
                        let hash = self.state().content_id();
                        hash.and_then(|hash| self.links.by_hash.get(&hash).copied())
                    };
                    *cursor = list.map_or(Cursor::Done, |list| Cursor::Links {
                        list,
                        entry: 0,
                        position: 0,
                    });
                    if !at.remaining.is_empty() {
                        let edits = at.remaining.clone();
                        return Some(Tries::new(
                            at.group,
                            at.bucket,
                            edits.clone(),
                            edits,
                            Rest::All,
                        ));
                    }
                    if at.bucket > 0 {
                        let edits = self.groups[at.group].buckets[at.bucket - 1].clone();
                        return Some(Tries::new(
                            at.group,
                            at.bucket - 1,
                            edits.clone(),
                            edits,
                            Rest::All,
                        ));
                    }
                }
                Cursor::Links {
                    list,
                    entry,
                    position,
                } => {
                    let Some((g, edits)) = self.links.lists[*list].get(*entry) else {
                        *cursor = Cursor::Done;
                        continue;
                    };
                    if *position == 0 {
                        // Each entry reached is charged the group index read
                        // to check it, before an entered group is passed
                        // over: a chain through many groups that recorded
                        // one state passes over every one it entered.
                        if !self.budget.charge(size_of::<usize>()) {
                            return None;
                        }
                        // A group entered once is never entered again.
                        if self.entered[*g] > 0 {
                            *entry += 1;
                            continue;
                        }
                    }
                    let Some(&(b, idx)) = edits.get(*position) else {
                        (*entry, *position) = (*entry + 1, 0);
                        continue;
                    };
                    *position += 1;
                    let pool = self.groups[*g].buckets[b].clone();
                    return Some(Tries::new(*g, b, vec![idx], pool, Rest::AnySubset));
                }
                Cursor::Done => return None,
            }
        }
    }

    /// Records the chain on the path, which ends as `end`. The first chain
    /// to hold a step copies it for what exploring it was charged; every
    /// further copy is charged what it holds before it is made, so that
    /// many chains sharing a long path still exhaust the budget. Nothing is
    /// recorded when that charge is refused.
    fn finish(&mut self, end: ChainEnd) {
        let copies = self
            .path
            .iter()
            .filter(|p| p.copied)
            .map(|p| held(&p.step))
            .sum();
        if !self.budget.charge(copies) {
            return;
        }
        for step in &mut self.path {
            step.copied = true;
        }
        let class = if end == ChainEnd::Base {
            ChainClass::ExactFromBase
        } else if self.path.iter().all(|p| {
            let edit = &self.accepted[p.idx].edit;
            is_known(&edit.before) && is_known(&edit.after)
        }) {
            ChainClass::ExactFromStart
        } else {
            ChainClass::Consistent
        };
        // A chain passes through each group it entered in one run.
        let mut key: Key = Vec::new();
        for step in &self.path {
            let g = self.group_of[step.idx];
            match key.last_mut() {
                Some((last, edits)) if *last == g => edits.push(step.idx),
                _ => key.push((g, vec![step.idx])),
            }
        }
        for (_, edits) in &mut key {
            edits.sort_unstable();
        }
        let steps: Vec<ChainStep> = self.path.iter().rev().map(|p| p.step.clone()).collect();
        let head = steps
            .last()
            .map(|step| step.edit.clone())
            .expect("a finished chain has at least one step");
        self.found.push(Found {
            key,
            walked: WalkedChain {
                chain: Chain {
                    id: 0,
                    class,
                    head,
                    edits: steps.iter().map(|s| s.edit.clone()).collect(),
                    end,
                },
                start: self.state().clone(),
                steps,
            },
        });
    }
}

/// The bytes a copy of `step` holds: both states and the alignment.
fn held(step: &ChainStep) -> usize {
    step.before.byte_len()
        + step.after.byte_len()
        + size_of_val(step.alignment.kept.as_slice())
        + size_of_val(step.alignment.regions.as_slice())
}

/// A chain's groups, in the order it entered them, with the accepted edits
/// it used in each, sorted.
type Key = Vec<(usize, Vec<usize>)>;

/// Assigns ids in discovery order and sets aside every chain not ending at
/// the base whose key is covered by the key of one that does.
///
/// Siblings that differ only in unrecorded choices share their key, so
/// keys are compared, not chains: each distinct key of a chain not ending
/// at the base is compared once with each distinct key of a base-reaching
/// chain whose groups it begins, and every comparison is charged the
/// indices it reads before it is made. A chain whose comparison is refused
/// stays among the chains.
fn settle(found: Vec<Found>, budget: &mut Budget) -> (Vec<WalkedChain>, Vec<WalkedChain>) {
    // The distinct keys of base-reaching chains: their edits, by groups.
    let mut reaching: BTreeMap<Vec<usize>, BTreeSet<Vec<Vec<usize>>>> = BTreeMap::new();
    for f in &found {
        if f.walked.chain.end == ChainEnd::Base {
            let (groups, edits) = f.key.iter().cloned().unzip();
            reaching.entry(groups).or_default().insert(edits);
        }
    }
    let mut verdicts: BTreeMap<Key, bool> = BTreeMap::new();
    let mut chains = Vec::new();
    let mut set_aside = Vec::new();
    for (id, mut f) in found.into_iter().enumerate() {
        f.walked.chain.id = id;
        let aside = f.walked.chain.end != ChainEnd::Base
            && *verdicts
                .entry(f.key)
                .or_insert_with_key(|key| is_covered(&reaching, key, budget));
        if aside {
            set_aside.push(f.walked);
        } else {
            chains.push(f.walked);
        }
    }
    (chains, set_aside)
}

/// Whether a base-reaching chain entered, in the same order, every group
/// `key` entered, possibly followed by more, and used, in each, every edit
/// `key` used there. Each comparison is charged first; `false` once a
/// charge is refused.
fn is_covered(
    reaching: &BTreeMap<Vec<usize>, BTreeSet<Vec<Vec<usize>>>>,
    key: &Key,
    budget: &mut Budget,
) -> bool {
    let groups: Vec<usize> = key.iter().map(|(g, _)| *g).collect();
    // Group sequences that begin with `groups` follow it in order.
    for (_, all) in reaching
        .range(groups.clone()..)
        .take_while(|(by, _)| by.starts_with(&groups))
    {
        for edits in all {
            let read: usize = key
                .iter()
                .zip(edits)
                .map(|((_, used), by)| used.len() + by.len())
                .sum();
            if !budget.charge(read.saturating_mul(size_of::<usize>())) {
                return false;
            }
            if key
                .iter()
                .zip(edits)
                .all(|((_, used), by)| is_subset(used, by))
            {
                return true;
            }
        }
    }
    false
}

/// Whether every element of the sorted `small` is in the sorted `big`.
fn is_subset(small: &[usize], big: &[usize]) -> bool {
    let mut big = big.iter();
    small.iter().all(|x| big.any(|y| y == x))
}
