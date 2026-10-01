//! The content-match fallback: whole edit texts found verbatim, for the
//! lines no chain explains.
//!
//! It runs only for the lines whose combined outcome needs it: a target line
//! with an unexplained origin (which includes a path with no chain) and a
//! base line with no fate. An edit's non-empty `new_text` (a Write's
//! content) is looked for in the target, its `old_text` in the base, as a
//! sequence of lines: every distinct line gets an id once, the block's
//! interior lines are found as a run of ids with a linear-time search
//! (KMP), and its partial first and last lines are checked by bytes. A
//! block covers only the whole lines it contains, a line counting as whole
//! apart from its terminator; a block inside part of a line explains
//! nothing, and a block with no letter or digit never matches.
//!
//! Matching has its own byte budget. Reading each side and each block is
//! charged before it is read, indexing each line the bookkeeping it adds,
//! the search each line it compares, and each place a block may occur the
//! bytes it checks there. The lines a block's matches cover are merged into
//! one union, charged once and marked once, however many matches overlap.
//! When the budget runs out, `incomplete` is set and the lines not yet
//! matched stay unmatched. Matches are evidence of a different kind than a
//! chain, and several stay a list.

use std::collections::{BTreeSet, HashMap};

use supersigil_record::EventId;
use supersigil_record::observations::EditOperation;

use crate::input::State;
use crate::lines::lines;
use crate::result::{BaseLineOutcome, ContentMatches, LineOutcome, Origin};
use crate::step::Budget;
use crate::walk::AcceptedEdit;

/// Finds whole-block matches of the edits' retained texts for the lines
/// that need the fallback, within `budget_bytes` of matching work.
#[must_use]
pub fn content_matches(
    base: &State,
    target: &State,
    edits: &[AcceptedEdit],
    target_outcomes: &[LineOutcome],
    base_outcomes: &[BaseLineOutcome],
    budget_bytes: u64,
) -> ContentMatches {
    let mut budget = Budget::new(budget_bytes);
    let target_wanted: Vec<bool> = target_outcomes.iter().map(needs_target).collect();
    let base_wanted: Vec<bool> = base_outcomes.iter().map(needs_base).collect();
    let mut matches = ContentMatches {
        target: vec![BTreeSet::new(); target_wanted.len()],
        base: vec![BTreeSet::new(); base_wanted.len()],
        incomplete: false,
    };
    let target_index = target_wanted
        .contains(&true)
        .then(|| Index::build(target.bytes().unwrap_or_default(), &mut budget))
        .flatten();
    let base_index = base_wanted
        .contains(&true)
        .then(|| Index::build(base.bytes().unwrap_or_default(), &mut budget))
        .flatten();
    for accepted in edits {
        if budget.exhausted() {
            break;
        }
        let edit = &accepted.edit;
        if let (Some(index), Some(block)) = (&target_index, edit.new_text.retained()) {
            let mut cover = Cover {
                wanted: &target_wanted,
                lines: &mut matches.target,
                id: &edit.id,
            };
            index.find(block, &mut cover, &mut budget);
        }
        if edit.operation == EditOperation::Write {
            continue;
        }
        if let (Some(index), Some(block)) = (&base_index, edit.old_text.retained()) {
            let mut cover = Cover {
                wanted: &base_wanted,
                lines: &mut matches.base,
                id: &edit.id,
            };
            index.find(block, &mut cover, &mut budget);
        }
    }
    matches.incomplete = budget.exhausted();
    matches
}

fn needs_target(outcome: &LineOutcome) -> bool {
    matches!(
        outcome,
        LineOutcome::Agreed { provenance, .. } if provenance.origins.contains(&Origin::Unexplained)
    )
}

fn needs_base(outcome: &BaseLineOutcome) -> bool {
    matches!(outcome, BaseLineOutcome::Agreed { fates, .. } if fates.is_empty())
}

/// Where a block's matches are recorded: only on the wanted lines.
struct Cover<'a> {
    wanted: &'a [bool],
    lines: &'a mut [BTreeSet<EventId>],
    id: &'a EventId,
}

impl Cover<'_> {
    /// Records the edit on `line` when it is wanted, charging the copy of
    /// its id first; `false` when the charge is refused.
    fn mark(&mut self, line: usize, budget: &mut Budget) -> bool {
        if !self.wanted.get(line).copied().unwrap_or(false) {
            return true;
        }
        if !budget.charge(size_of::<EventId>() + self.id.as_str().len()) {
            return false;
        }
        self.lines[line].insert(self.id.clone());
        true
    }
}

/// What indexing one line adds: its start, its id, and its place in the
/// lists of lines by id and by content.
const LINE_COST: usize = 4 * size_of::<usize>();

/// What a line not seen before adds: its map entry and its list of lines.
const NEW_LINE_COST: usize = size_of::<(&[u8], usize)>() + size_of::<Vec<usize>>();

/// What a line content not seen before adds: its map entry with its list.
const NEW_CONTENT_COST: usize = size_of::<(&[u8], Vec<usize>)>();

/// One side's lines, indexed once.
struct Index<'a> {
    bytes: &'a [u8],
    /// Where each line starts, and then the end of the bytes.
    starts: Vec<usize>,
    /// The id of each line: equal lines, terminators included, share one.
    ids: Vec<usize>,
    id_of: HashMap<&'a [u8], usize>,
    /// The lines with each id, in order.
    by_id: Vec<Vec<usize>>,
    /// The lines by their content without terminator, in order.
    by_content: HashMap<&'a [u8], Vec<usize>>,
}

impl<'a> Index<'a> {
    /// Indexes `bytes`. Reading them is charged first, and each line the
    /// bookkeeping it adds before it is added; `None` when the budget runs
    /// out.
    fn build(bytes: &'a [u8], budget: &mut Budget) -> Option<Self> {
        if !budget.charge(bytes.len()) {
            return None;
        }
        let mut index = Self {
            bytes,
            starts: Vec::new(),
            ids: Vec::new(),
            id_of: HashMap::new(),
            by_id: Vec::new(),
            by_content: HashMap::new(),
        };
        let mut start = 0;
        for line in lines(bytes) {
            let known = index.id_of.get(line).copied();
            let content = content(line);
            let seen = index.by_content.contains_key(content);
            let cost = LINE_COST
                + if known.is_none() { NEW_LINE_COST } else { 0 }
                + if seen { 0 } else { NEW_CONTENT_COST };
            if !budget.charge(cost) {
                return None;
            }
            let id = known.unwrap_or_else(|| {
                index.id_of.insert(line, index.by_id.len());
                index.by_id.push(Vec::new());
                index.by_id.len() - 1
            });
            let at = index.ids.len();
            index.ids.push(id);
            index.by_id[id].push(at);
            index.by_content.entry(content).or_default().push(at);
            index.starts.push(start);
            start += line.len();
        }
        index.starts.push(bytes.len());
        Some(index)
    }

    fn count(&self) -> usize {
        self.ids.len()
    }

    fn line(&self, at: usize) -> &'a [u8] {
        &self.bytes[self.starts[at]..self.starts[at + 1]]
    }

    /// Marks every wanted line `block` covers wherever it occurs.
    ///
    /// Reading the block is charged before it is read. Each place it may
    /// occur is charged the bytes checked there, and the union of the lines
    /// its matches cover is charged, then marked, once.
    fn find(&self, block: &str, cover: &mut Cover, budget: &mut Budget) {
        if !budget.charge(block.len()) {
            return;
        }
        if !block.chars().any(char::is_alphanumeric) {
            return;
        }
        let block = block.as_bytes();
        let (Some(first), Some(last)) = (lines(block).next(), lines(block).last()) else {
            return;
        };
        let segments = lines(block).count();
        let mut union = Union::default();
        if segments == 1 {
            let found = if first.ends_with(b"\n") {
                self.id_of.get(first).map(|&id| &self.by_id[id])
            } else {
                self.by_content.get(first)
            };
            for &line in found.into_iter().flatten() {
                if !union.add((line, line + 1), self, cover, budget) {
                    return;
                }
            }
            union.close(self, cover, budget);
            return;
        }
        let tail = (!last.ends_with(b"\n")).then_some(last);
        let checked = first.len() + tail.map_or(0, <[u8]>::len);
        let core = segments - 1 - usize::from(tail.is_some());
        if core == 0 {
            // Only a first line and a partial last one: check each pair.
            for start in 1..self.count() {
                if !budget.charge(checked) {
                    return;
                }
                if let Some(span) = self.covered(start, start, first, tail)
                    && !union.add(span, self, cover, budget)
                {
                    return;
                }
            }
        } else {
            // The interior line ids and the search's fallback table.
            if !budget.charge(core.saturating_mul(2 * size_of::<usize>())) {
                return;
            }
            let Some(pattern) = lines(block)
                .skip(1)
                .take(core)
                .map(|segment| self.id_of.get(segment).copied())
                .collect::<Option<Vec<usize>>>()
            else {
                return;
            };
            let mut search = Kmp::new(&pattern);
            while let Some(start) = search.next(self, budget) {
                if !budget.charge(checked) {
                    return;
                }
                if let Some(span) = self.covered(start, start + core, first, tail)
                    && !union.add(span, self, cover, budget)
                {
                    return;
                }
            }
            if budget.exhausted() {
                return;
            }
        }
        union.close(self, cover, budget);
    }

    /// The lines one occurrence covers, when the block's interior lines are
    /// lines `start..end`: the line before must end with the block's `first`
    /// line, and the line at `end` must begin with its partial `tail`. A
    /// partial line is covered only when the block holds all of it but its
    /// terminator. `None` when the block does not occur there.
    fn covered(
        &self,
        start: usize,
        end: usize,
        first: &[u8],
        tail: Option<&[u8]>,
    ) -> Option<(usize, usize)> {
        let before = self.line(start.checked_sub(1)?);
        if !before.ends_with(first) {
            return None;
        }
        let after = (end < self.count()).then(|| self.line(end));
        if let Some(tail) = tail
            && !after.is_some_and(|line| line.starts_with(tail))
        {
            return None;
        }
        let lo = if before.len() == first.len() {
            start - 1
        } else {
            start
        };
        let hi = match (tail, after) {
            (Some(tail), Some(line)) if content(line).len() == tail.len() => end + 1,
            _ => end,
        };
        Some((lo, hi))
    }

    /// Charges the bytes of lines `lo..hi`, then marks each of them; `false`
    /// when the budget runs out.
    fn mark(&self, (lo, hi): (usize, usize), cover: &mut Cover, budget: &mut Budget) -> bool {
        budget.charge(self.starts[hi] - self.starts[lo])
            && (lo..hi).all(|line| cover.mark(line, budget))
    }
}

/// The union of the line spans one block's matches cover, given in
/// increasing order of their starts: a span that overlaps or touches the
/// open one extends it, and any other closes it, marking its lines.
#[derive(Default)]
struct Union {
    open: Option<(usize, usize)>,
}

impl Union {
    /// Adds a match's span `lo..hi`; `false` when marking a closed span ran
    /// out of budget.
    fn add(
        &mut self,
        (lo, hi): (usize, usize),
        index: &Index,
        cover: &mut Cover,
        budget: &mut Budget,
    ) -> bool {
        if lo >= hi {
            return true;
        }
        match &mut self.open {
            Some((_, end)) if lo <= *end => {
                *end = (*end).max(hi);
                true
            }
            open => open
                .replace((lo, hi))
                .is_none_or(|span| index.mark(span, cover, budget)),
        }
    }

    /// Marks the open span, if any.
    fn close(self, index: &Index, cover: &mut Cover, budget: &mut Budget) {
        if let Some(span) = self.open {
            index.mark(span, cover, budget);
        }
    }
}

/// A linear-time search for a run of line ids, one match at a time.
struct Kmp<'p> {
    pattern: &'p [usize],
    /// For each prefix of the pattern, the longest proper prefix that is
    /// also its suffix.
    fallback: Vec<usize>,
    /// The next line to visit.
    at: usize,
    /// How much of the pattern ends at the last line visited.
    matched: usize,
}

impl<'p> Kmp<'p> {
    /// Prepares the search for a non-empty `pattern`.
    fn new(pattern: &'p [usize]) -> Self {
        let mut fallback = vec![0; pattern.len()];
        let mut k = 0;
        for i in 1..pattern.len() {
            while k > 0 && pattern[i] != pattern[k] {
                k = fallback[k - 1];
            }
            if pattern[i] == pattern[k] {
                k += 1;
            }
            fallback[i] = k;
        }
        Self {
            pattern,
            fallback,
            at: 0,
            matched: 0,
        }
    }

    /// The next start of the pattern among `index`'s line ids, charging
    /// each line visited; `None` at the end, or when the budget runs out.
    fn next(&mut self, index: &Index, budget: &mut Budget) -> Option<usize> {
        while self.at < index.count() {
            let at = self.at;
            self.at += 1;
            if !budget.charge(index.starts[at + 1] - index.starts[at]) {
                return None;
            }
            let id = index.ids[at];
            while self.matched > 0 && id != self.pattern[self.matched] {
                self.matched = self.fallback[self.matched - 1];
            }
            if id == self.pattern[self.matched] {
                self.matched += 1;
            }
            if self.matched == self.pattern.len() {
                self.matched = self.fallback[self.matched - 1];
                return Some(at + 1 - self.pattern.len());
            }
        }
        None
    }
}

/// A line's content, before its `\n` or `\r\n` terminator. Unlike
/// [`without_terminator`](crate::lines::without_terminator), a lone trailing
/// `\r` on an unterminated last line stays part of the content.
fn content(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r\n")
        .or_else(|| line.strip_suffix(b"\n"))
        .unwrap_or(line)
}
