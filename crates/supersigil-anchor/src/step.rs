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

use std::ops::Range;

use supersigil_record::observations::{Content, Edit, EditOperation, FileState, Hunk, Material};

use crate::input::State;
use crate::lines::{line_of, line_starts};
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
            if old.is_empty() {
                return None;
            }
            let positions = if edit.replace_all {
                non_overlapping(bytes, old)
            } else {
                find(bytes, old).into_iter().collect()
            };
            if positions.is_empty() {
                return None;
            }
            let mut after = Vec::with_capacity(bytes.len() + new.len());
            let mut replacements = Vec::with_capacity(positions.len());
            let mut last = 0;
            for position in positions {
                after.extend_from_slice(&bytes[last..position]);
                let start = after.len();
                after.extend_from_slice(new);
                replacements.push(Replacement {
                    before: position..position + old.len(),
                    after: start..after.len(),
                });
                last = position + old.len();
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
///   known.
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
/// Reading `current` (its hash, its lines, the search for the edit's text)
/// is charged to `budget` first, then each candidate; when a charge is
/// refused, the candidates accepted so far are returned and
/// [`Budget::exhausted`] is set.
#[must_use]
pub fn reverse(edit: &Edit, current: &State, base: &State, budget: &mut Budget) -> Reversed {
    if edit.operation == EditOperation::Unknown {
        return stop(StopReason::OperationUnknown {
            edit: edit.id.clone(),
        });
    }
    // Charged before reading, so a reversal that proposes nothing still
    // costs what it read: the walk tries one per branch.
    if !budget.charge(current.byte_len()) {
        return Reversed::Candidates(Vec::new());
    }
    if current.matches(&edit.after) == Some(false) {
        return stop(StopReason::AfterHashMismatch {
            edit: edit.id.clone(),
        });
    }
    let patch = Patch::new(edit, current);
    if let Some(patch) = &patch
        && !is_known(&edit.after)
        && !patch.shown_after(&patch.current)
    {
        return stop(StopReason::AfterPatchMismatch {
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
    /// The edit's retained patch, checked against `current`. `None` when no
    /// patch was retained, or when a hunk holds a literal tab: Claude Code
    /// shows every tab as two spaces, so such a hunk is in another display
    /// and the whole patch is ignored.
    fn new(edit: &'a Edit, current: &'a State) -> Option<Self> {
        let hunks = edit.patch.retained()?;
        let text = || hunks.iter().flat_map(|hunk| &hunk.lines);
        if text().any(|line| line.contains('\t')) {
            return None;
        }
        Some(Self {
            hunks: sides(hunks),
            bytes: text().map(String::len).sum(),
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

fn without_terminator(bytes: &[u8]) -> &[u8] {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    bytes.strip_suffix(b"\r").unwrap_or(bytes)
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
    fn new(outer: &'a Whole<'a>, at: &Range<usize>, with: &[u8]) -> Self {
        let head = line_of(&outer.starts, at.start);
        let from = outer.starts.get(head).copied().unwrap_or(0);
        // The first line starting after the replaced range: the line holding
        // its end is touched even when the range ends at that line's start.
        let tail = outer.starts.partition_point(|&start| start <= at.end);
        let to = outer.starts.get(tail).copied().unwrap_or(outer.bytes.len());
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

fn stop(reason: StopReason) -> Reversed {
    Reversed::Stop(reason)
}

fn retained(material: &Material<String>) -> Option<&[u8]> {
    material.retained().map(String::as_bytes)
}

/// Returns the first position where `needle` occurs.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    Matches::new(haystack, needle, false).next()
}

/// Returns the non-overlapping occurrences of `needle`, left to right, as
/// `str::replace` finds them.
fn non_overlapping(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    Matches::new(haystack, needle, false).collect()
}

/// Returns every position where `needle` occurs, overlapping ones included.
fn occurrences(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    Matches::new(haystack, needle, true).collect()
}

/// The positions where a needle occurs in a haystack, left to right, found
/// in time linear in both lengths (Knuth-Morris-Pratt): every haystack byte
/// is read once, so repetitive text costs no more than any other. An empty
/// needle occurs nowhere.
struct Matches<'a> {
    haystack: &'a [u8],
    needle: &'a [u8],
    /// For each `i`, the length of the longest proper prefix of
    /// `needle[..=i]` that is also its suffix.
    border: Vec<usize>,
    /// Whether a match may start inside the previous one.
    overlapping: bool,
    /// The next haystack byte to read.
    at: usize,
    /// How many leading needle bytes the bytes before `at` end with.
    matched: usize,
}

impl<'a> Matches<'a> {
    fn new(haystack: &'a [u8], needle: &'a [u8], overlapping: bool) -> Self {
        let mut border = vec![0; needle.len()];
        let mut k = 0;
        for i in 1..needle.len() {
            while k > 0 && needle[i] != needle[k] {
                k = border[k - 1];
            }
            if needle[i] == needle[k] {
                k += 1;
            }
            border[i] = k;
        }
        Self {
            haystack,
            needle,
            border,
            overlapping,
            at: 0,
            matched: 0,
        }
    }
}

impl Iterator for Matches<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        if self.needle.is_empty() {
            return None;
        }
        while let Some(&byte) = self.haystack.get(self.at) {
            self.at += 1;
            while self.matched > 0 && self.needle[self.matched] != byte {
                self.matched = self.border[self.matched - 1];
            }
            if self.needle[self.matched] == byte {
                self.matched += 1;
            }
            if self.matched == self.needle.len() {
                self.matched = if self.overlapping {
                    self.border[self.matched - 1]
                } else {
                    0
                };
                return Some(self.at - self.needle.len());
            }
        }
        None
    }
}

/// Where a deleted block could have been: every line start, plus the end
/// when the bytes are empty or end with a newline.
fn deletion_positions(bytes: &[u8]) -> Vec<usize> {
    let mut positions = line_starts(bytes);
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        positions.push(bytes.len());
    }
    positions
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

/// Orders positions by line distance from the hint, then by position.
fn order_by_hint(mut positions: Vec<usize>, starts: &[usize], hint: Option<usize>) -> Vec<usize> {
    if let Some(hint) = hint {
        positions.sort_by_key(|&p| (line_of(starts, p).abs_diff(hint), p));
    }
    positions
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
        return stop(StopReason::TextUnavailable { edit: id() });
    };
    let Some(bytes) = current.bytes() else {
        return stop(StopReason::NotLocatable { edit: id() });
    };
    if old.is_empty() {
        return stop(StopReason::NotLocatable { edit: id() });
    }
    if edit.replace_all {
        return reverse_replace_all(edit, current, bytes, old, new, budget);
    }
    let positions = if new.is_empty() {
        if !old.ends_with(b"\n") {
            return stop(StopReason::NotLocatable { edit: id() });
        }
        deletion_positions(bytes)
    } else {
        occurrences(bytes, new)
    };
    if positions.is_empty() {
        return stop(StopReason::NotLocatable { edit: id() });
    }
    let positions = match patch {
        Some(patch) => order_by_hint(positions, &patch.current.starts, patch.hint),
        None => positions,
    };
    // A known before-hash decides alone; the old side is consulted only
    // without one.
    let check = patch.filter(|_| !is_known(&edit.before));
    let mut accepted: Vec<Reversal> = Vec::new();
    for position in positions {
        if let Some(patch) = check {
            let spliced = Spliced::new(&patch.current, &(position..position + new.len()), old);
            if !budget.charge(spliced.touched.len() + patch.bytes) {
                break;
            }
            if !patch.shown_before(&spliced) {
                continue;
            }
        }
        if !budget.charge(bytes.len() + old.len()) {
            break;
        }
        let before = State::Present(splice(bytes, position..position + new.len(), old));
        if accepted.iter().any(|r| r.before == before) {
            continue;
        }
        if let Some(reversal) = validate(edit, before, current) {
            accepted.push(reversal);
        }
    }
    if accepted.is_empty() && !budget.exhausted() {
        return stop(StopReason::NoAcceptedCandidate { edit: id() });
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
        stop(StopReason::ReplaceAllUnverified {
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
    let positions = non_overlapping(bytes, new);
    if positions.is_empty() {
        return stop(StopReason::NotLocatable {
            edit: edit.id.clone(),
        });
    }
    if !budget.charge(2 * bytes.len()) {
        return Reversed::Candidates(Vec::new());
    }
    let mut before = Vec::with_capacity(bytes.len());
    let mut last = 0;
    for position in positions {
        before.extend_from_slice(&bytes[last..position]);
        before.extend_from_slice(old);
        last = position + new.len();
    }
    before.extend_from_slice(&bytes[last..]);
    match validate(edit, State::Present(before), current) {
        Some(reversal) => Reversed::Candidates(vec![reversal]),
        None => unverified(),
    }
}

fn reverse_write(edit: &Edit, current: &State, base: &State, budget: &mut Budget) -> Reversed {
    let id = || edit.id.clone();
    let Some(content) = retained(&edit.new_text) else {
        return stop(StopReason::TextUnavailable { edit: id() });
    };
    if !budget.charge(current.byte_len() + base.byte_len()) {
        return Reversed::Candidates(Vec::new());
    }
    if current.bytes() != Some(content) {
        return stop(StopReason::NoAcceptedCandidate { edit: id() });
    }
    let before = match &edit.before {
        FileState::Absent => State::Absent,
        FileState::Present {
            content: Content::Known(recorded),
        } if base.content_id().as_ref() == Some(recorded) => base.clone(),
        FileState::Present { .. } => return stop(StopReason::WholeFileWrite { edit: id() }),
    };
    match validate(edit, before, current) {
        Some(reversal) => Reversed::Candidates(vec![reversal]),
        None => stop(StopReason::NoAcceptedCandidate { edit: id() }),
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
        for haystack in words(0..=9) {
            for needle in words(1..=4) {
                let (h, n) = (haystack.as_bytes(), needle.as_bytes());
                let every: Vec<usize> = (0..haystack.len())
                    .filter(|&i| haystack[i..].starts_with(&needle))
                    .collect();
                let greedy: Vec<usize> = haystack.match_indices(&needle).map(|(i, _)| i).collect();
                assert_eq!(occurrences(h, n), every, "{haystack} {needle}");
                assert_eq!(non_overlapping(h, n), greedy, "{haystack} {needle}");
                assert_eq!(find(h, n), haystack.find(&needle), "{haystack} {needle}");
            }
            assert_eq!(occurrences(haystack.as_bytes(), b""), Vec::<usize>::new());
        }
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
