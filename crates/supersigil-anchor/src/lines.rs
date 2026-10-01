//! Splits bytes into lines that keep their terminators.
//!
//! A line ends right after `\n`; the last line may lack one. Empty input has
//! no lines. Every other module counts lines this way, so a line index means
//! the same thing to the walk, the alignment, and the review.

/// Returns the byte offset where each line starts.
///
/// `b"a\nb"` has lines starting at `[0, 2]`, `b"a\n"` at `[0]`, and empty
/// input has none.
#[must_use]
pub fn line_starts(bytes: &[u8]) -> Vec<usize> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut starts = vec![0];
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' && i + 1 < bytes.len() {
            starts.push(i + 1);
        }
    }
    starts
}

/// Iterates the lines of `bytes`, each keeping its terminator, without
/// collecting them.
pub fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes.split_inclusive(|&b| b == b'\n')
}

/// Splits `bytes` into lines, each keeping its terminator.
#[must_use]
pub fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    lines(bytes).collect()
}

/// `line` without its `\n` and then without a trailing `\r`.
#[must_use]
pub fn without_terminator(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Returns the index of the line containing `offset`, given [`line_starts`].
///
/// An offset at or past the end belongs to the last line; with no lines the
/// result is 0.
#[must_use]
pub fn line_of(starts: &[usize], offset: usize) -> usize {
    starts
        .partition_point(|&start| start <= offset)
        .saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_end_after_newlines_and_the_last_may_lack_one() {
        assert_eq!(line_starts(b""), Vec::<usize>::new());
        assert_eq!(line_starts(b"a\n"), vec![0]);
        assert_eq!(line_starts(b"a\nb"), vec![0, 2]);
        assert_eq!(line_starts(b"a\r\nb\n"), vec![0, 3]);
        assert_eq!(
            split_lines(b"a\r\nb\nc"),
            vec![&b"a\r\n"[..], &b"b\n"[..], &b"c"[..]]
        );
    }

    #[test]
    fn offsets_belong_to_the_line_that_contains_them() {
        let starts = line_starts(b"ab\ncd\n");
        assert_eq!(line_of(&starts, 0), 0);
        assert_eq!(line_of(&starts, 2), 0);
        assert_eq!(line_of(&starts, 3), 1);
        assert_eq!(line_of(&starts, 6), 1);
        assert_eq!(line_of(&[], 4), 0);
    }
}
