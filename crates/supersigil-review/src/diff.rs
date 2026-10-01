//! The reviewed diff: a line diff of blob bytes, bounded without a clock.
//!
//! Lines are split after `\n` and compared byte for byte, terminators
//! included, so a change in line ending or in the final newline is a change.
//! Common leading and trailing lines are removed first; when more than
//! [`COARSE_LINES`] remain on either side, the file gets one coarse hunk
//! instead of an aligned diff, so the work never depends on how different the
//! two sides are.

use similar::{Algorithm, DiffTag, capture_diff_slices};
use supersigil_anchor::lines::split_lines;

/// Lines left on either side after trimming common lines, above which a
/// file gets one coarse hunk instead of a Myers alignment.
pub const COARSE_LINES: usize = 5_000;

/// One changed region, as 0-based line ranges on each side.
///
/// An empty range sits where the other side's lines go: `base_start` of an
/// insertion is the index of the base line the added lines precede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    /// First removed base line, 0-based.
    pub base_start: usize,
    /// Number of removed base lines.
    pub base_count: usize,
    /// First added target line, 0-based.
    pub target_start: usize,
    /// Number of added target lines.
    pub target_count: usize,
}

/// The reviewed diff of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// Changed regions in order, without context lines.
    pub hunks: Vec<DiffHunk>,
    /// Whether the file was too large to align and got one coarse hunk
    /// that removes every remaining base line and adds every remaining
    /// target line.
    pub coarse: bool,
}

/// Diffs `base` against `target` line by line with Myers, git's default.
///
/// Consecutive changed lines form one hunk; hunks carry no context.
#[must_use]
pub fn diff_lines(base: &[u8], target: &[u8]) -> FileDiff {
    let old = split_lines(base);
    let new = split_lines(target);
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];
    if old_mid.is_empty() && new_mid.is_empty() {
        return FileDiff {
            hunks: Vec::new(),
            coarse: false,
        };
    }
    if old_mid.len() > COARSE_LINES || new_mid.len() > COARSE_LINES {
        return FileDiff {
            hunks: vec![DiffHunk {
                base_start: prefix,
                base_count: old_mid.len(),
                target_start: prefix,
                target_count: new_mid.len(),
            }],
            coarse: true,
        };
    }
    let mut hunks = Vec::new();
    let mut open: Option<DiffHunk> = None;
    for op in capture_diff_slices(Algorithm::Myers, old_mid, new_mid) {
        let (tag, old_range, new_range) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            hunks.extend(open.take());
            continue;
        }
        let hunk = open.get_or_insert(DiffHunk {
            base_start: prefix + old_range.start,
            base_count: 0,
            target_start: prefix + new_range.start,
            target_count: 0,
        });
        hunk.base_count += old_range.len();
        hunk.target_count += new_range.len();
    }
    hunks.extend(open);
    FileDiff {
        hunks,
        coarse: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(
        base_start: usize,
        base_count: usize,
        target_start: usize,
        target_count: usize,
    ) -> DiffHunk {
        DiffHunk {
            base_start,
            base_count,
            target_start,
            target_count,
        }
    }

    #[test]
    fn identical_files_have_no_hunks() {
        let diff = diff_lines(b"a\nb\n", b"a\nb\n");
        assert_eq!(diff.hunks, Vec::new());
        assert!(!diff.coarse);
    }

    #[test]
    fn a_changed_line_is_one_hunk() {
        let diff = diff_lines(b"a\nb\nc\n", b"a\nB\nc\n");
        assert_eq!(diff.hunks, vec![hunk(1, 1, 1, 1)]);
    }

    #[test]
    fn an_insertion_has_an_empty_base_range_where_it_goes() {
        let diff = diff_lines(b"a\nc\n", b"a\nb\nc\n");
        assert_eq!(diff.hunks, vec![hunk(1, 0, 1, 1)]);
        let at_start = diff_lines(b"b\n", b"a\nb\n");
        assert_eq!(at_start.hunks, vec![hunk(0, 0, 0, 1)]);
    }

    #[test]
    fn an_added_file_adds_every_line() {
        let diff = diff_lines(b"", b"x\ny\n");
        assert_eq!(diff.hunks, vec![hunk(0, 0, 0, 2)]);
    }

    #[test]
    fn a_deleted_file_removes_every_line() {
        let diff = diff_lines(b"x\ny\n", b"");
        assert_eq!(diff.hunks, vec![hunk(0, 2, 0, 0)]);
    }

    #[test]
    fn terminators_are_part_of_the_line() {
        assert_eq!(diff_lines(b"x\n", b"x").hunks, vec![hunk(0, 1, 0, 1)]);
        assert_eq!(diff_lines(b"x\n", b"x\r\n").hunks, vec![hunk(0, 1, 0, 1)]);
    }

    #[test]
    fn separate_changes_are_separate_hunks() {
        let diff = diff_lines(b"a\nb\nc\nd\ne\n", b"A\nb\nc\nd\nE\n");
        assert_eq!(diff.hunks, vec![hunk(0, 1, 0, 1), hunk(4, 1, 4, 1)]);
    }

    fn numbered(prefix: &str, count: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for i in 0..count {
            bytes.extend_from_slice(format!("{prefix}{i}\n").as_bytes());
        }
        bytes
    }

    #[test]
    fn exactly_coarse_lines_remaining_is_still_aligned() {
        // First and last lines differ, so nothing is trimmed and 5,000
        // lines remain on each side; the equal middle keeps Myers cheap.
        let middle = numbered("x", COARSE_LINES - 2);
        let base = [b"a\n".as_slice(), &middle, b"z\n"].concat();
        let target = [b"A\n".as_slice(), &middle, b"Z\n"].concat();
        let diff = diff_lines(&base, &target);
        assert!(!diff.coarse);
        assert_eq!(
            diff.hunks,
            vec![
                hunk(0, 1, 0, 1),
                hunk(COARSE_LINES - 1, 1, COARSE_LINES - 1, 1)
            ]
        );
    }

    #[test]
    fn more_than_coarse_lines_remaining_is_one_coarse_hunk() {
        let base = [b"same\n".as_slice(), &numbered("b", COARSE_LINES + 1)].concat();
        let target = [b"same\n".as_slice(), &numbered("t", 3)].concat();
        let diff = diff_lines(&base, &target);
        assert!(diff.coarse);
        assert_eq!(diff.hunks, vec![hunk(1, COARSE_LINES + 1, 1, 3)]);
    }
}
