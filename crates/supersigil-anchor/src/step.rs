//! Reverses one recorded edit and validates every candidate by executing
//! the edit forward.
//!
//! Reversal only proposes before-states. A candidate is accepted when the
//! current bytes match the edit's recorded after-hash (when known), the
//! candidate matches its recorded before-hash (when known), and executing
//! the edit forward on the candidate, with the recorder's semantics,
//! reproduces the current bytes exactly.
//!
//! When the edit's `structuredPatch` is retained, it is a consistency check
//! where no hash decides. Claude Code shows every tab in it as two spaces,
//! so its text is never bytes; but each hunk's old side (context and `-`
//! lines) is the before-file's lines at `old_start`, and its new side
//! (context and `+` lines) the after-file's lines at `new_start`, once the
//! file's tabs are shown the same way. While the after-hash is unknown, the
//! current bytes must show every hunk's new side; while the before-hash is
//! unknown, a candidate must show every hunk's old side. A known hash
//! decides alone and the hunks are not consulted for it. A hunk holding a
//! literal tab cannot come from that display, so an edit with one is
//! reversed as if no patch were retained. Hunk line numbers also order the
//! candidates, nearest first.
//!
//! The check proposes nothing and changes no candidate; it only removes
//! candidates and steps, and every accepted candidate still passes forward
//! execution and every known hash. Under a display rule it does not
//! recognize it could reject the true candidate of a hashless edit: less
//! attribution, and a competing reading that survives alone.

use std::collections::BTreeSet;
use std::iter::Peekable;
use std::ops::Range;

use supersigil_record::observations::{Content, Edit, EditOperation, FileState, Hunk, Material};

use crate::input::State;
use crate::lines::{line_of, line_starts, without_terminator};
use crate::result::StopReason;

/// One replacement an edit made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// Byte range of the replaced text in the before-state.
    pub before: Range<usize>,
    /// Byte range of the replacement text in the after-state.
    pub after: Range<usize>,
}

/// The result of executing an edit forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward {
    /// The after-state.
    pub after: State,
    /// The replacements made, in increasing byte order.
    pub replacements: Vec<Replacement>,
}

/// Executes `edit` on `before` with the recorder's semantics.
///
/// An Edit (`operation: replace`) replaces the first occurrence of its
/// `old_text`, or every non-overlapping occurrence from left to right when
/// `replace_all` is set, as `supersigil-session`'s `claude_code/content.rs`
/// does. It needs both texts retained, a non-empty `old_text`, a present
/// before-state, and at least one occurrence. A Write's result is its
/// retained content whatever the before-state was, and its one replacement
/// spans both whole files. Returns `None` when the edit cannot execute,
/// which includes every `Unknown` operation.
#[must_use]
pub fn execute_forward(edit: &Edit, before: &State) -> Option<Forward> {
    match edit.operation {
        EditOperation::Replace => {
            let old = retained(&edit.old_text)?;
            let new = retained(&edit.new_text)?;
            let bytes = before.bytes()?;
            let limit = if edit.replace_all { usize::MAX } else { 1 };
            let mut after = Vec::with_capacity(bytes.len() + new.len());
            let mut replacements = Vec::new();
            let mut last = 0;
            for position in Matches::new(bytes, old, Reading::Leftmost).take(limit) {
                after.extend_from_slice(&bytes[last..position]);
                let start = after.len();
                after.extend_from_slice(new);
                replacements.push(Replacement {
                    before: position..position + old.len(),
                    after: start..after.len(),
                });
                last = position + old.len();
            }
            if replacements.is_empty() {
                return None;
            }
            after.extend_from_slice(&bytes[last..]);
            Some(Forward {
                after: State::Present(after),
                replacements,
            })
        }
        EditOperation::Write => {
            let content = retained(&edit.new_text)?;
            Some(Forward {
                after: State::Present(content.to_vec()),
                replacements: vec![Replacement {
                    before: 0..before.byte_len(),
                    after: 0..content.len(),
                }],
            })
        }
        EditOperation::Unknown => None,
    }
}

/// An accepted candidate before-state of one edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reversal {
    /// The before-state.
    pub before: State,
    /// The replacements forward execution made from `before`.
    pub replacements: Vec<Replacement>,
    /// Whether this is one of several accepted candidate positions: an
    /// unrecorded choice the walk must branch on.
    pub location_choice: bool,
}

/// What reversing one edit produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reversed {
    /// The accepted candidates, distinct by before-state. Empty only when
    /// the budget ran out; check [`Budget::exhausted`].
    Candidates(Vec<Reversal>),
    /// No candidate could be proposed or accepted, and why.
    Stop(StopReason),
}

/// A work budget measured in bytes processed.
#[derive(Debug, Clone)]
pub struct Budget {
    remaining: u64,
    exhausted: bool,
}

impl Budget {
    /// Creates a budget allowing `bytes` bytes of work.
    #[must_use]
    pub fn new(bytes: u64) -> Self {
        Self {
            remaining: bytes,
            exhausted: false,
        }
    }

    /// Spends `bytes` of the budget. Returns `false`, and marks the budget
    /// exhausted, when not enough remains; nothing is spent then.
    pub fn charge(&mut self, bytes: usize) -> bool {
        let cost = u64::try_from(bytes).unwrap_or(u64::MAX);
        if self.exhausted || cost > self.remaining {
            self.exhausted = true;
            return false;
        }
        self.remaining -= cost;
        true
    }

    /// Returns whether a charge has ever been refused.
    #[must_use]
    pub fn exhausted(&self) -> bool {
        self.exhausted
    }
}

/// Proposes and validates before-states for `edit`, whose after-state is
/// `current`.
///
/// Candidates per operation:
///
/// - Edit: every occurrence of a non-empty `new_text` in `current`. For an
///   empty `new_text` (a deletion), every line start of `current` plus its
///   end when that is a line boundary, and only when `old_text` ends with a
///   line terminator. Hunk line numbers order the candidates, nearest
///   first.
/// - Edit with `replace_all`: one joint candidate replacing every
///   non-overlapping occurrence of `new_text`, only when the before-hash is
///   known. Forward execution would replace every occurrence of `old_text`
///   in it, so a candidate whose forward output would not have the current
///   bytes' length is rejected before that output is built.
/// - Write creating a file (recorded before-state absent): the before-state
///   is absent.
/// - Write overwriting a file: the before-state is `base`, only when the
///   recorded before-hash equals `base`'s hash.
///
/// With a retained patch, the step stops when the after-hash is unknown and
/// `current` does not show every hunk's new side, and a candidate is
/// rejected when the before-hash is unknown and it does not show every
/// hunk's old side, compared as the module documentation describes. A
/// known hash decides alone, and a patch holding a literal tab is ignored
/// entirely. An Edit's candidate is checked through the lines around its
/// splice before it is built, so a rejected one costs only those lines and
/// the hunk text.
///
/// Work is charged to `budget` before it is done: reading `current` (its
/// hash, its lines, the search for the edit's text), the retained hunks
/// where they are consulted, the table each search prepares, and each
/// candidate (the lines a patch check splices, then the before-state built
/// and the after-state forward execution makes from it; for a joint
/// inverse, also the search counting `old_text` in it). Candidates are
/// generated one at a time. When a charge is refused, the candidates
/// accepted so far are returned and [`Budget::exhausted`] is set.
#[must_use]
pub fn reverse(edit: &Edit, current: &State, base: &State, budget: &mut Budget) -> Reversed {
    if edit.operation == EditOperation::Unknown {
        return Reversed::Stop(StopReason::OperationUnknown {
            edit: edit.id.clone(),
        });
    }
    // Charged before reading, so a reversal that proposes nothing still
    // costs what it read: the walk tries one per branch.
    if !budget.charge(current.byte_len()) {
        return out_of_budget();
    }
    if current.matches(&edit.after) == Some(false) {
        return Reversed::Stop(StopReason::AfterHashMismatch {
            edit: edit.id.clone(),
        });
    }
    // The retained patch is read only where it is consulted: for the new
    // side while the after-hash is unknown, and for an ordinary Edit's
    // candidates. Its text is charged before it is read.
    let consulted =
        !is_known(&edit.after) || (edit.operation == EditOperation::Replace && !edit.replace_all);
    let patch = match edit.patch.retained() {
        Some(hunks) if consulted => {
            let bytes = hunks
                .iter()
                .flat_map(|hunk| &hunk.lines)
                .map(String::len)
                .sum();
            if !budget.charge(bytes) {
                return out_of_budget();
            }
            Patch::new(hunks, bytes, current)
        }
        _ => None,
    };
    if let Some(patch) = &patch
        && !is_known(&edit.after)
        && !patch.shown_after(&patch.current)
    {
        return Reversed::Stop(StopReason::AfterPatchMismatch {
            edit: edit.id.clone(),
        });
    }
    if edit.operation == EditOperation::Write {
        reverse_write(edit, current, base, budget)
    } else {
        reverse_replace(edit, current, patch.as_ref(), budget)
    }
}

/// Whether a recorded state can be checked: absent, or present with a known
/// content hash.
pub(crate) fn is_known(state: &FileState) -> bool {
    !matches!(
        state,
        FileState::Present {
            content: Content::Unknown
        }
    )
}

/// A retained patch and the lines of the current bytes it is checked with.
struct Patch<'a> {
    hunks: Vec<Sides<'a>>,
    /// Bytes of hunk text, charged for each check of a candidate.
    bytes: usize,
    /// The 0-based line where the first hunk shows its first change, used
    /// only to order candidates.
    hint: Option<usize>,
    current: Whole<'a>,
}

impl<'a> Patch<'a> {
    /// A retained patch of `bytes` bytes of hunk text, checked against
    /// `current`. `None` when a hunk holds a literal tab: Claude Code shows
    /// every tab as two spaces, so such a hunk is in another display and the
    /// whole patch is ignored.
    fn new(hunks: &'a [Hunk], bytes: usize, current: &'a State) -> Option<Self> {
        if hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .any(|line| line.contains('\t'))
        {
            return None;
        }
        Some(Self {
            hunks: sides(hunks),
            bytes,
            hint: hint_line(hunks),
            current: Whole::new(current.bytes().unwrap_or_default()),
        })
    }
}

/// One hunk's old side (context and `-` lines) and new side (context and
/// `+` lines), without their prefixes.
struct Sides<'a> {
    old_start: u32,
    old: Vec<&'a str>,
    new_start: u32,
    new: Vec<&'a str>,
}

fn sides(hunks: &[Hunk]) -> Vec<Sides<'_>> {
    hunks
        .iter()
        .map(|hunk| {
            let (mut old, mut new) = (Vec::new(), Vec::new());
            for line in &hunk.lines {
                // `\ No newline at end of file` markers are skipped.
                match line.as_bytes().first() {
                    Some(b' ') => {
                        old.push(&line[1..]);
                        new.push(&line[1..]);
                    }
                    Some(b'-') => old.push(&line[1..]),
                    Some(b'+') => new.push(&line[1..]),
                    _ => {}
                }
            }
            Sides {
                old_start: hunk.old_start,
                old,
                new_start: hunk.new_start,
                new,
            }
        })
        .collect()
}

impl Patch<'_> {
    fn shown_before(&self, lines: &impl Lines) -> bool {
        self.hunks
            .iter()
            .all(|hunk| shows(lines, hunk.old_start, &hunk.old))
    }

    fn shown_after(&self, lines: &impl Lines) -> bool {
        self.hunks
            .iter()
            .all(|hunk| shows(lines, hunk.new_start, &hunk.new))
    }
}

/// Whether `lines` shows `shown` from the 1-based line `start`; for an
/// empty side, `start` is the line before it (0 before the first), as in a
/// unified diff. Lines are compared without their terminator and a trailing
/// `\r` on either side, and with every tab of the file shown as two spaces,
/// the way Claude Code displays them.
fn shows(lines: &impl Lines, start: u32, shown: &[&str]) -> bool {
    let Ok(start) = usize::try_from(start) else {
        return false;
    };
    if shown.is_empty() {
        return start <= lines.count();
    }
    let Some(first) = start.checked_sub(1) else {
        return false;
    };
    first + shown.len() <= lines.count()
        && shown
            .iter()
            .enumerate()
            .all(|(k, text)| displays(lines.line(first + k), text))
}

fn displays(line: &[u8], shown: &str) -> bool {
    let mut rest = without_terminator(shown.as_bytes());
    for byte in without_terminator(line) {
        let display: &[u8] = if *byte == b'\t' {
            b"  "
        } else {
            std::slice::from_ref(byte)
        };
        let Some(after) = rest.strip_prefix(display) else {
            return false;
        };
        rest = after;
    }
    rest.is_empty()
}

/// Lines of a state, each with its terminator.
trait Lines {
    fn count(&self) -> usize;
    fn line(&self, index: usize) -> &[u8];
}

/// The lines of bytes in hand.
struct Whole<'a> {
    bytes: &'a [u8],
    starts: Vec<usize>,
}

impl<'a> Whole<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            starts: line_starts(bytes),
        }
    }
}

impl Lines for Whole<'_> {
    fn count(&self) -> usize {
        self.starts.len()
    }

    fn line(&self, index: usize) -> &[u8] {
        let end = self
            .starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.bytes.len());
        &self.bytes[self.starts[index]..end]
    }
}

/// The lines of `outer` with a byte range replaced, without building them:
/// only the lines the replacement touches are copied.
struct Spliced<'a> {
    outer: &'a Whole<'a>,
    /// Lines of `outer` before the touched ones.
    head: usize,
    /// The touched lines, after the replacement.
    touched: Vec<u8>,
    touched_starts: Vec<usize>,
    /// The first line of `outer` after the touched ones.
    tail: usize,
}

impl<'a> Spliced<'a> {
    /// Builds the splice after charging `budget` for the touched lines it
    /// copies; `None`, with nothing built, when the charge is refused.
    fn charged(
        outer: &'a Whole<'a>,
        at: &Range<usize>,
        with: &[u8],
        budget: &mut Budget,
    ) -> Option<Self> {
        let (_, from, _, to) = Self::bounds(outer, at);
        let len = (to - from - at.len()).saturating_add(with.len());
        budget.charge(len).then(|| Self::new(outer, at, with))
    }

    /// The touched lines of `outer` around `at`: the first one's index and
    /// start, and the index and start of the first line after them.
    fn bounds(outer: &Whole, at: &Range<usize>) -> (usize, usize, usize, usize) {
        let head = line_of(&outer.starts, at.start);
        let from = outer.starts.get(head).copied().unwrap_or(0);
        // The first line starting after the replaced range: the line holding
        // its end is touched even when the range ends at that line's start.
        let tail = outer.starts.partition_point(|&start| start <= at.end);
        let to = outer.starts.get(tail).copied().unwrap_or(outer.bytes.len());
        (head, from, tail, to)
    }

    fn new(outer: &'a Whole<'a>, at: &Range<usize>, with: &[u8]) -> Self {
        let (head, from, tail, to) = Self::bounds(outer, at);
        let mut touched = Vec::with_capacity(to - from - at.len() + with.len());
        touched.extend_from_slice(&outer.bytes[from..at.start]);
        touched.extend_from_slice(with);
        touched.extend_from_slice(&outer.bytes[at.end..to]);
        let touched_starts = line_starts(&touched);
        Self {
            outer,
            head,
            touched,
            touched_starts,
            tail,
        }
    }
}

impl Lines for Spliced<'_> {
    fn count(&self) -> usize {
        self.head + self.touched_starts.len() + self.outer.count() - self.tail
    }

    fn line(&self, index: usize) -> &[u8] {
        if index < self.head {
            return self.outer.line(index);
        }
        let local = index - self.head;
        match self.touched_starts.get(local) {
            Some(&start) => {
                let end = self
                    .touched_starts
                    .get(local + 1)
                    .copied()
                    .unwrap_or(self.touched.len());
                &self.touched[start..end]
            }
            None => self
                .outer
                .line(local - self.touched_starts.len() + self.tail),
        }
    }
}

/// The result when the budget refused a charge: no candidates, with
/// [`Budget::exhausted`] set.
const fn out_of_budget() -> Reversed {
    Reversed::Candidates(Vec::new())
}

fn retained(material: &Material<String>) -> Option<&[u8]> {
    material.retained().map(String::as_bytes)
}

/// How [`Matches`] reads its haystack.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// Non-overlapping occurrences, left to right, as `str::replace` finds
    /// them.
    Leftmost,
    /// Every occurrence, overlapping ones included, left to right.
    Every,
    /// Every occurrence, overlapping ones included, right to left.
    EveryBackward,
}

/// The positions where a needle occurs in a haystack, given one at a time
/// and found in time linear in both lengths (Knuth-Morris-Pratt): every
/// haystack byte is read once, so repetitive text costs no more than any
/// other. Preparing the search costs the needle's length, except that a
/// needle that is empty or longer than the haystack occurs nowhere and
/// prepares nothing.
struct Matches<'a> {
    haystack: &'a [u8],
    needle: &'a [u8],
    reading: Reading,
    /// For each `i`, the length of the longest proper prefix of the first
    /// `i + 1` needle bytes in reading order that is also their suffix.
    border: Vec<usize>,
    /// How many haystack bytes have been read.
    read: usize,
    /// How many leading needle bytes, in reading order, the bytes read end
    /// with.
    matched: usize,
}

impl<'a> Matches<'a> {
    fn new(haystack: &'a [u8], needle: &'a [u8], reading: Reading) -> Self {
        let mut matches = Self {
            haystack,
            needle,
            reading,
            border: Vec::new(),
            read: 0,
            matched: 0,
        };
        if needle.is_empty() || needle.len() > haystack.len() {
            matches.read = haystack.len();
            return matches;
        }
        let mut border = vec![0; needle.len()];
        let mut k = 0;
        for i in 1..needle.len() {
            let byte = matches.needle_at(i);
            while k > 0 && byte != matches.needle_at(k) {
                k = border[k - 1];
            }
            if byte == matches.needle_at(k) {
                k += 1;
            }
            border[i] = k;
        }
        matches.border = border;
        matches
    }

    /// The `i`-th needle byte in reading order.
    fn needle_at(&self, i: usize) -> u8 {
        if self.reading == Reading::EveryBackward {
            self.needle[self.needle.len() - 1 - i]
        } else {
            self.needle[i]
        }
    }
}

impl Iterator for Matches<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        let (len, width) = (self.haystack.len(), self.needle.len());
        let backward = self.reading == Reading::EveryBackward;
        while self.read < len {
            let byte = if backward {
                self.haystack[len - 1 - self.read]
            } else {
                self.haystack[self.read]
            };
            self.read += 1;
            while self.matched > 0 && self.needle_at(self.matched) != byte {
                self.matched = self.border[self.matched - 1];
            }
            if self.needle_at(self.matched) == byte {
                self.matched += 1;
            }
            if self.matched == width {
                self.matched = if self.reading == Reading::Leftmost {
                    0
                } else {
                    self.border[width - 1]
                };
                return Some(if backward {
                    len - self.read
                } else {
                    self.read - width
                });
            }
        }
        None
    }
}

/// The candidate positions of an Edit's `new_text` in the current bytes,
/// generated one at a time: every occurrence of a non-empty `new_text`, or
/// for a deletion (an empty one) every line start and the end when that is
/// a line boundary. With a hint (the current bytes' line starts and a
/// 0-based line), positions on lines nearer the hint come first; without
/// one, positions come in byte order.
fn candidate_positions<'a>(
    bytes: &'a [u8],
    new: &'a [u8],
    hint: Option<(&'a [usize], usize)>,
) -> Box<dyn Iterator<Item = usize> + 'a> {
    let Some((starts, line)) = hint else {
        return positions_from(bytes, new, 0);
    };
    let from = starts.get(line).copied().unwrap_or(bytes.len());
    Box::new(Outward {
        starts,
        line,
        before: positions_before(bytes, new, from).peekable(),
        after: positions_from(bytes, new, from).peekable(),
    })
}

/// Whether a deleted block could have been at `position`: a line start, or
/// the end when the bytes are empty or end with a line terminator.
fn deletable_at(bytes: &[u8], position: usize) -> bool {
    position == 0 || bytes[position - 1] == b'\n'
}

/// Candidate positions at or after `from`, in increasing order.
fn positions_from<'a>(
    bytes: &'a [u8],
    new: &'a [u8],
    from: usize,
) -> Box<dyn Iterator<Item = usize> + 'a> {
    if new.is_empty() {
        Box::new((from..=bytes.len()).filter(move |&p| deletable_at(bytes, p)))
    } else {
        Box::new(Matches::new(&bytes[from..], new, Reading::Every).map(move |p| p + from))
    }
}

/// Candidate positions before `to`, in decreasing order. An occurrence
/// starting before `to` may end after it, so the search reads up to the
/// end of the last one that could.
fn positions_before<'a>(
    bytes: &'a [u8],
    new: &'a [u8],
    to: usize,
) -> Box<dyn Iterator<Item = usize> + 'a> {
    if new.is_empty() {
        Box::new((0..to).rev().filter(move |&p| deletable_at(bytes, p)))
    } else {
        let end = to
            .checked_sub(1)
            .map_or(0, |last| (last + new.len()).min(bytes.len()));
        Box::new(Matches::new(&bytes[..end], new, Reading::EveryBackward))
    }
}

/// Positions before a hint line, nearest first, and from it on, nearest
/// first, merged by their lines' distance from the hint line; a tie goes
/// to the earlier line.
struct Outward<'a> {
    starts: &'a [usize],
    line: usize,
    before: Peekable<Box<dyn Iterator<Item = usize> + 'a>>,
    after: Peekable<Box<dyn Iterator<Item = usize> + 'a>>,
}

impl Iterator for Outward<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        let (starts, line) = (self.starts, self.line);
        let distance = |position: usize| line_of(starts, position).abs_diff(line);
        match (self.before.peek(), self.after.peek()) {
            (Some(&before), Some(&after)) if distance(after) < distance(before) => {
                self.after.next()
            }
            (Some(_), _) => self.before.next(),
            (None, _) => self.after.next(),
        }
    }
}

/// The 0-based line where the first hunk shows its first change, used only
/// to order candidates.
fn hint_line(hunks: &[Hunk]) -> Option<usize> {
    let hunk = hunks.first()?;
    let context = hunk
        .lines
        .iter()
        .take_while(|line| line.starts_with(' '))
        .count();
    let one_based = usize::try_from(hunk.new_start).ok()? + context;
    Some(one_based.saturating_sub(1))
}

/// What a candidate before-state of `len` bytes costs before it is built:
/// building it, and executing the edit forward on it into an after-state
/// of `current` bytes that is compared with the current ones. Saturates, so
/// that a cost too large to represent is refused.
fn candidate_cost(len: usize, current: usize) -> usize {
    len.saturating_add(current)
}

/// Checks a candidate before-state against the recorded before-hash and by
/// forward execution.
fn validate(edit: &Edit, before: State, current: &State) -> Option<Reversal> {
    if before.matches(&edit.before) == Some(false) {
        return None;
    }
    let forward = execute_forward(edit, &before)?;
    (forward.after == *current).then_some(Reversal {
        before,
        replacements: forward.replacements,
        location_choice: false,
    })
}

fn splice(bytes: &[u8], at: Range<usize>, with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() - at.len() + with.len());
    out.extend_from_slice(&bytes[..at.start]);
    out.extend_from_slice(with);
    out.extend_from_slice(&bytes[at.end..]);
    out
}

fn reverse_replace(
    edit: &Edit,
    current: &State,
    patch: Option<&Patch>,
    budget: &mut Budget,
) -> Reversed {
    let id = || edit.id.clone();
    let (Some(old), Some(new)) = (retained(&edit.old_text), retained(&edit.new_text)) else {
        return Reversed::Stop(StopReason::TextUnavailable { edit: id() });
    };
    let Some(bytes) = current.bytes() else {
        return Reversed::Stop(StopReason::NotLocatable { edit: id() });
    };
    if old.is_empty() {
        return Reversed::Stop(StopReason::NotLocatable { edit: id() });
    }
    if edit.replace_all {
        return reverse_replace_all(edit, current, bytes, old, new, budget);
    }
    if (new.is_empty() && !old.ends_with(b"\n")) || new.len() > bytes.len() {
        return Reversed::Stop(StopReason::NotLocatable { edit: id() });
    }
    let hint = patch.and_then(|patch| Some((patch.current.starts.as_slice(), patch.hint?)));
    // Each search for `new_text` (one each way from a hint) prepares a table
    // as long as it.
    let searches = if hint.is_some() { 2 } else { 1 };
    if !budget.charge(new.len().saturating_mul(searches)) {
        return out_of_budget();
    }
    // A known before-hash decides alone; the old side is consulted only
    // without one.
    let check = patch.filter(|_| !is_known(&edit.before));
    let mut proposed = false;
    let mut accepted: Vec<Reversal> = Vec::new();
    // Forward execution's replacement start for each accepted candidate. An
    // accepted before-state is the current bytes with `new_text` at that
    // start put back to `old_text`, so equal starts mean equal states.
    let mut starts = BTreeSet::new();
    for position in candidate_positions(bytes, new, hint) {
        proposed = true;
        let at = position..position + new.len();
        if let Some(patch) = check {
            if !budget.charge(patch.bytes) {
                break;
            }
            let Some(spliced) = Spliced::charged(&patch.current, &at, old, budget) else {
                break;
            };
            if !patch.shown_before(&spliced) {
                continue;
            }
        }
        let len = (bytes.len() - new.len()).saturating_add(old.len());
        if !budget.charge(candidate_cost(len, bytes.len())) {
            break;
        }
        let before = State::Present(splice(bytes, at, old));
        if let Some(reversal) = validate(edit, before, current)
            && reversal
                .replacements
                .first()
                .is_some_and(|replaced| starts.insert(replaced.before.start))
        {
            accepted.push(reversal);
        }
    }
    if !proposed {
        return Reversed::Stop(StopReason::NotLocatable { edit: id() });
    }
    if accepted.is_empty() && !budget.exhausted() {
        return Reversed::Stop(StopReason::NoAcceptedCandidate { edit: id() });
    }
    let choice = accepted.len() > 1;
    for reversal in &mut accepted {
        reversal.location_choice = choice;
    }
    Reversed::Candidates(accepted)
}

fn reverse_replace_all(
    edit: &Edit,
    current: &State,
    bytes: &[u8],
    old: &[u8],
    new: &[u8],
    budget: &mut Budget,
) -> Reversed {
    let unverified = || {
        Reversed::Stop(StopReason::ReplaceAllUnverified {
            edit: edit.id.clone(),
        })
    };
    let known_before = matches!(
        edit.before,
        FileState::Present {
            content: Content::Known(_)
        }
    );
    if !known_before || new.is_empty() {
        return unverified();
    }
    let not_locatable = || {
        Reversed::Stop(StopReason::NotLocatable {
            edit: edit.id.clone(),
        })
    };
    if new.len() > bytes.len() {
        return not_locatable();
    }
    if !budget.charge(new.len()) {
        return out_of_budget();
    }
    let count = Matches::new(bytes, new, Reading::Leftmost).count();
    if count == 0 {
        return not_locatable();
    }
    // The joint inverse puts every occurrence back to `old_text`. Its
    // length is charged, with its validation, the second search that
    // builds it, and the search that counts `old_text` in it, before
    // anything is built. The occurrences do not overlap, so they fit in the
    // bytes.
    let len = count
        .checked_mul(old.len())
        .and_then(|restored| (bytes.len() - count * new.len()).checked_add(restored))
        .unwrap_or(usize::MAX);
    let cost = candidate_cost(len, bytes.len())
        .saturating_add(bytes.len())
        .saturating_add(new.len())
        .saturating_add(len)
        .saturating_add(old.len());
    if !budget.charge(cost) {
        return out_of_budget();
    }
    let mut before = Vec::with_capacity(len);
    let mut last = 0;
    for position in Matches::new(bytes, new, Reading::Leftmost) {
        before.extend_from_slice(&bytes[last..position]);
        before.extend_from_slice(old);
        last = position + new.len();
    }
    before.extend_from_slice(&bytes[last..]);
    // Forward execution replaces every occurrence of `old_text` in the
    // inverse, also those the current bytes held unchanged, so its output
    // can be far longer than the current bytes it was charged as. Its
    // length follows from the count: an inverse whose forward output
    // cannot be the current bytes' length is rejected before it is built.
    let replaced = Matches::new(&before, old, Reading::Leftmost).count();
    let forward_len = replaced
        .checked_mul(new.len())
        .and_then(|added| (len - replaced * old.len()).checked_add(added));
    if forward_len != Some(bytes.len()) {
        return unverified();
    }
    match validate(edit, State::Present(before), current) {
        Some(reversal) => Reversed::Candidates(vec![reversal]),
        None => unverified(),
    }
}

fn reverse_write(edit: &Edit, current: &State, base: &State, budget: &mut Budget) -> Reversed {
    let id = || edit.id.clone();
    let Some(content) = retained(&edit.new_text) else {
        return Reversed::Stop(StopReason::TextUnavailable { edit: id() });
    };
    if !budget.charge(candidate_cost(base.byte_len(), current.byte_len())) {
        return out_of_budget();
    }
    if current.bytes() != Some(content) {
        return Reversed::Stop(StopReason::NoAcceptedCandidate { edit: id() });
    }
    let before = match &edit.before {
        FileState::Absent => State::Absent,
        FileState::Present {
            content: Content::Known(recorded),
        } if base.content_id().as_ref() == Some(recorded) => base.clone(),
        FileState::Present { .. } => {
            return Reversed::Stop(StopReason::WholeFileWrite { edit: id() });
        }
    };
    match validate(edit, before, current) {
        Some(reversal) => Reversed::Candidates(vec![reversal]),
        None => Reversed::Stop(StopReason::NoAcceptedCandidate { edit: id() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(lines: &impl Lines) -> Vec<&[u8]> {
        (0..lines.count()).map(|i| lines.line(i)).collect()
    }

    #[test]
    fn spliced_lines_are_the_lines_of_the_spliced_bytes() {
        let outers: [&[u8]; 5] = [b"", b"a\n", b"a\nb", b"ab\ncd\nef\n", b"\n\n"];
        let withs: [&[u8]; 5] = [b"", b"x", b"x\n", b"\ny", b"x\ny\n"];
        for outer in outers {
            let whole = Whole::new(outer);
            for start in 0..=outer.len() {
                for end in start..=outer.len() {
                    for with in withs {
                        let spliced = Spliced::new(&whole, &(start..end), with);
                        let built = splice(outer, start..end, with);
                        assert_eq!(
                            all(&spliced),
                            all(&Whole::new(&built)),
                            "{outer:?} {start}..{end} {with:?}"
                        );
                        // The charge made before building is what is built.
                        let cost = u64::try_from(spliced.touched.len()).unwrap();
                        if let Some(less) = cost.checked_sub(1) {
                            let mut short = Budget::new(less);
                            assert!(
                                Spliced::charged(&whole, &(start..end), with, &mut short).is_none()
                            );
                        }
                        let mut exact = Budget::new(cost);
                        let charged = Spliced::charged(&whole, &(start..end), with, &mut exact);
                        assert_eq!(charged.map(|s| s.touched), Some(spliced.touched));
                        assert!(!exact.charge(1));
                    }
                }
            }
        }
    }

    /// Every string over `a` and `b` of each length in `lengths`.
    fn words(lengths: std::ops::RangeInclusive<u32>) -> Vec<String> {
        lengths
            .flat_map(|len| {
                (0..1_u32 << len).map(move |bits| {
                    (0..len)
                        .map(|k| if bits >> k & 1 == 1 { 'b' } else { 'a' })
                        .collect()
                })
            })
            .collect()
    }

    #[test]
    fn matches_are_the_occurrences_str_finds() {
        let found = |h: &str, n: &str, reading| -> Vec<usize> {
            Matches::new(h.as_bytes(), n.as_bytes(), reading).collect()
        };
        for haystack in words(0..=9) {
            for needle in words(1..=4) {
                let every: Vec<usize> = (0..haystack.len())
                    .filter(|&i| haystack[i..].starts_with(&needle))
                    .collect();
                let leftmost: Vec<usize> =
                    haystack.match_indices(&needle).map(|(i, _)| i).collect();
                let backward: Vec<usize> = every.iter().rev().copied().collect();
                assert_eq!(found(&haystack, &needle, Reading::Every), every);
                assert_eq!(found(&haystack, &needle, Reading::Leftmost), leftmost);
                assert_eq!(found(&haystack, &needle, Reading::EveryBackward), backward);
            }
            for reading in [Reading::Leftmost, Reading::Every, Reading::EveryBackward] {
                assert_eq!(found(&haystack, "", reading), Vec::<usize>::new());
            }
        }
    }

    #[test]
    fn a_search_that_cannot_match_prepares_nothing() {
        let needle = vec![b'a'; 64];
        let matches = Matches::new(b"aaa", &needle, Reading::Every);
        assert!(matches.border.is_empty());
        assert_eq!(matches.count(), 0);
    }

    #[test]
    fn a_line_displays_without_terminators_and_with_tabs_as_two_spaces() {
        assert!(displays(b"\tx\r\n", "  x"));
        assert!(displays(b"a", "a\r"));
        assert!(!displays(b"\tx\n", "\tx"));
        assert!(!displays(b"\tx\n", "    x"));
        assert!(!displays(b"ab\n", "a"));
        assert!(!displays(b"a\n", "ab"));
    }
}
