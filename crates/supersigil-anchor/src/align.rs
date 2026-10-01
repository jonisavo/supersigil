//! Aligns the lines of an edit's before- and after-states from the byte
//! ranges forward execution replaced, never from a diff.
//!
//! Bytes outside the replaced ranges are carried unchanged, shifted by the
//! accumulated difference in range lengths. A line is kept when its whole
//! span lies outside the ranges on its side and the image of that span on
//! the other side is a whole line there; kept lines pair with their images.
//! Inside a replacement, whole lines common to the start of the old and new
//! text, and whole lines common to their end, are kept too; when the two
//! overlap, each replacement chooses independently which to prefer. Every
//! other line belongs to a changed region: the lines between two
//! consecutive kept pairs, before lines on one side and after lines on the
//! other.

use std::ops::Range;

use crate::input::State;
use crate::lines::line_starts;
use crate::step::Replacement;

/// Lines of the before-state and after-state that changed together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Line indices in the before-state.
    pub before: Range<usize>,
    /// Line indices in the after-state.
    pub after: Range<usize>,
}

/// A line correspondence between the before- and after-state of one edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment {
    /// Kept lines as (before line, after line), increasing on both sides.
    pub kept: Vec<(usize, usize)>,
    /// Changed regions, in order.
    pub regions: Vec<Region>,
}

/// Aligns `before` and `after` from the replacements forward execution made.
///
/// When the whole-line context common to the start and to the end of a
/// replacement overlaps (for example `x\nx\n` replaced by `x\n`), that
/// replacement has two mappings, prefix-first and suffix-first: equal bytes
/// do not settle which old line survived. Each such replacement chooses
/// independently (a `replace_all` has one replacement per occurrence), so a
/// step with n of them has 2^n alignments. The returned iterator generates
/// them one at a time, every choice prefix-first first; callers charge each
/// to their budget before using it.
///
/// `replacements` are in increasing byte order, as [`Forward`] holds them.
///
/// # Panics
///
/// Panics when a replacement's `before` range ends past `before`'s bytes,
/// or its `after` range past `after`'s bytes (an absent state has none).
/// The replacements [`execute_forward`] makes from `before`, with `after`
/// its result, always lie within them.
///
/// [`Forward`]: crate::step::Forward
/// [`execute_forward`]: crate::step::execute_forward
#[must_use]
pub fn align(before: &State, after: &State, replacements: &[Replacement]) -> Alignments {
    let old = Side::new(before.bytes().unwrap_or_default());
    let new = Side::new(after.bytes().unwrap_or_default());
    let mut fixed = carried_pairs(&old, &new, replacements);
    let mut choices = Vec::new();
    for replacement in replacements {
        let prefix = context_pairs(&old, &new, replacement, Prefer::Prefix);
        let suffix = context_pairs(&old, &new, replacement, Prefer::Suffix);
        if prefix == suffix {
            fixed.extend(prefix);
        } else {
            choices.push([prefix, suffix]);
        }
    }
    Alignments {
        counter: Some(vec![false; choices.len()]),
        fixed,
        choices,
        old_count: old.count(),
        new_count: new.count(),
    }
}

/// The alignments of one step, generated lazily: one per combination of
/// the independent context choices.
#[derive(Debug, Clone)]
pub struct Alignments {
    /// Kept pairs every alignment shares.
    fixed: Vec<(usize, usize)>,
    /// Per replacement with a choice: its prefix-first and suffix-first pairs.
    choices: Vec<[Vec<(usize, usize)>; 2]>,
    /// The next combination (`true` = suffix-first), `None` when done.
    counter: Option<Vec<bool>>,
    old_count: usize,
    new_count: usize,
}

impl Iterator for Alignments {
    type Item = Alignment;

    fn next(&mut self) -> Option<Alignment> {
        let counter = self.counter.as_mut()?;
        let mut kept = self.fixed.clone();
        for (&suffix, options) in counter.iter().zip(&self.choices) {
            kept.extend(options[usize::from(suffix)].iter().copied());
        }
        kept.sort_unstable();
        if !advance(counter) {
            self.counter = None;
        }
        Some(regions(kept, self.old_count, self.new_count))
    }
}

/// Advances `bits` to the next combination like a binary counter, lowest
/// bit first. Returns `false` when it wraps around to all `false`, that is
/// when every combination has been given; an empty counter has one.
pub(crate) fn advance(bits: &mut [bool]) -> bool {
    for bit in bits.iter_mut() {
        *bit = !*bit;
        if *bit {
            return true;
        }
    }
    false
}

/// The bytes of one state and where its lines start.
struct Side<'a> {
    bytes: &'a [u8],
    starts: Vec<usize>,
}

impl<'a> Side<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            starts: line_starts(bytes),
        }
    }

    fn count(&self) -> usize {
        self.starts.len()
    }

    fn span(&self, line: usize) -> Range<usize> {
        let end = self
            .starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.bytes.len());
        self.starts[line]..end
    }

    fn line_at(&self, offset: usize) -> Option<usize> {
        self.starts.binary_search(&offset).ok()
    }

    /// Whether `range` is exactly one whole line.
    fn whole_line(&self, range: &Range<usize>) -> Option<usize> {
        let line = self.line_at(range.start)?;
        (self.span(line) == *range).then_some(line)
    }
}

fn overlaps(span: &Range<usize>, range: &Range<usize>) -> bool {
    span.start < range.end && range.start < span.end
}

/// Kept pairs of lines lying wholly outside the replaced ranges.
///
/// Lines and replacements are both in increasing byte order, so one pass
/// over each suffices: the replacements ending at or before a line's start
/// are behind it, and only the first one ahead can overlap it. A byte past
/// the last replacement behind it is carried unchanged, at the same
/// distance from that replacement's end on the other side.
fn carried_pairs(old: &Side, new: &Side, replacements: &[Replacement]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut ahead = 0;
    // Where the last replacement behind the line ends, on each side.
    let (mut before_end, mut after_end) = (0, 0);
    for line in 0..old.count() {
        let span = old.span(line);
        while let Some(behind) = replacements
            .get(ahead)
            .filter(|r| r.before.end <= span.start)
        {
            (before_end, after_end) = (behind.before.end, behind.after.end);
            ahead += 1;
        }
        if replacements
            .get(ahead)
            .is_some_and(|r| overlaps(&span, &r.before))
        {
            continue;
        }
        let start = after_end + (span.start - before_end);
        let image = start..start + span.len();
        if let Some(target) = new.whole_line(&image) {
            pairs.push((line, target));
        }
    }
    pairs
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prefer {
    Prefix,
    Suffix,
}

/// The whole lines of `range` in `side`, as (line index, bytes); `None`
/// for a segment that is not a whole line of the file.
fn segments<'a>(side: &Side<'a>, range: &Range<usize>) -> Vec<Option<(usize, &'a [u8])>> {
    let mut out = Vec::new();
    let mut start = range.start;
    while start < range.end {
        let end = side.bytes[start..range.end]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(range.end, |i| start + i + 1);
        let whole = side.whole_line(&(start..end));
        out.push(whole.map(|line| (line, &side.bytes[start..end])));
        start = end;
    }
    out
}

/// Kept pairs from whole lines common to the start and the end of one
/// replacement, taking the preferred side first and the other up to what
/// remains.
fn context_pairs(old: &Side, new: &Side, r: &Replacement, prefer: Prefer) -> Vec<(usize, usize)> {
    let before = segments(old, &r.before);
    let after = segments(new, &r.after);
    let common = |a: &Option<(usize, &[u8])>, b: &Option<(usize, &[u8])>| match (a, b) {
        (Some((x, left)), Some((y, right))) if left == right => Some((*x, *y)),
        _ => None,
    };
    let limit = before.len().min(after.len());
    let prefix_max = (0..limit)
        .take_while(|&k| common(&before[k], &after[k]).is_some())
        .count();
    let suffix_max = (0..limit)
        .take_while(|&k| {
            common(&before[before.len() - 1 - k], &after[after.len() - 1 - k]).is_some()
        })
        .count();
    let (prefix, suffix) = match prefer {
        Prefer::Prefix => (prefix_max, suffix_max.min(limit - prefix_max)),
        Prefer::Suffix => (prefix_max.min(limit - suffix_max), suffix_max),
    };
    let mut pairs: Vec<(usize, usize)> = (0..prefix)
        .filter_map(|k| common(&before[k], &after[k]))
        .collect();
    pairs.extend(
        (0..suffix)
            .rev()
            .filter_map(|k| common(&before[before.len() - 1 - k], &after[after.len() - 1 - k])),
    );
    pairs
}

/// Turns kept pairs into an alignment: the lines between consecutive kept
/// pairs form one changed region.
fn regions(kept: Vec<(usize, usize)>, old_count: usize, new_count: usize) -> Alignment {
    let mut out = Vec::new();
    let (mut before, mut after) = (0, 0);
    for &(b, a) in kept.iter().chain(std::iter::once(&(old_count, new_count))) {
        if b > before || a > after {
            out.push(Region {
                before: before..b,
                after: after..a,
            });
        }
        before = b + 1;
        after = a + 1;
    }
    Alignment { kept, regions: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The replacements the recorder's semantics make, through `str`.
    fn replacements(before: &str, old: &str, new: &str, all: bool) -> (String, Vec<Replacement>) {
        let (mut after, mut made, mut last) = (String::new(), Vec::new(), 0);
        for (at, _) in before
            .match_indices(old)
            .take(if all { usize::MAX } else { 1 })
        {
            after.push_str(&before[last..at]);
            let start = after.len();
            after.push_str(new);
            made.push(Replacement {
                before: at..at + old.len(),
                after: start..after.len(),
            });
            last = at + old.len();
        }
        after.push_str(&before[last..]);
        (after, made)
    }

    /// Kept pairs found by mapping each line through every replacement.
    fn carried_one_by_one(old: &Side, new: &Side, made: &[Replacement]) -> Vec<(usize, usize)> {
        (0..old.count())
            .filter_map(|line| {
                let span = old.span(line);
                if made.iter().any(|r| overlaps(&span, &r.before)) {
                    return None;
                }
                let behind = made.iter().filter(|r| r.before.end <= span.start);
                let grown: usize = behind.clone().map(|r| r.after.len()).sum();
                let shrunk: usize = behind.map(|r| r.before.len()).sum();
                let start = span.start + grown - shrunk;
                new.whole_line(&(start..start + span.len()))
                    .map(|target| (line, target))
            })
            .collect()
    }

    #[test]
    fn carried_lines_are_those_mapped_one_by_one() {
        // Every string over `a`, `b`, and a newline, up to six bytes.
        let (mut befores, mut longest) = (vec![String::new()], vec![String::new()]);
        for _ in 0..6 {
            longest = longest
                .iter()
                .flat_map(|b| ["a", "b", "\n"].map(|c| format!("{b}{c}")))
                .collect();
            befores.extend(longest.iter().cloned());
        }
        let edits = [
            ("a", "b"),
            ("a", ""),
            ("a", "bb"),
            ("a\n", "b\n"),
            ("a\n", ""),
            ("\n", ""),
            ("\n", "\n\n"),
            ("a", "a\n"),
            ("ab", "\n"),
            ("b\na", "x"),
        ];
        for before in &befores {
            let mut cases: Vec<(String, Vec<Replacement>)> = edits
                .iter()
                .flat_map(|&(old, new)| {
                    [false, true].map(|all| replacements(before, old, new, all))
                })
                .collect();
            for written in ["", "a\n", "x"] {
                let whole = vec![Replacement {
                    before: 0..before.len(),
                    after: 0..written.len(),
                }];
                cases.push((written.to_owned(), whole));
            }
            for (after, made) in cases {
                let (old, new) = (Side::new(before.as_bytes()), Side::new(after.as_bytes()));
                assert_eq!(
                    carried_pairs(&old, &new, &made),
                    carried_one_by_one(&old, &new, &made),
                    "{before:?} -> {after:?}"
                );
            }
        }
    }

    #[test]
    fn advance_gives_every_combination_once() {
        let mut bits = [false; 3];
        let mut seen = vec![bits];
        while advance(&mut bits) {
            seen.push(bits);
        }
        assert_eq!(seen.len(), 8);
        assert_eq!(seen[1], [true, false, false]);
        assert_eq!(seen[2], [false, true, false]);
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 8);
        assert_eq!(bits, [false; 3]);
        assert!(!advance(&mut []));
    }
}
