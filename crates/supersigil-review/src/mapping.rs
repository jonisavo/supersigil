//! Whether anchor's lines, over worktree-form bytes, map one to one onto the
//! reviewed diff's lines, over blob bytes.

use supersigil_anchor::lines::{split_lines, without_terminator};

/// Returns whether `attribution` bytes correspond line for line to `blob`
/// bytes: they are identical, or they have the same number of lines and each
/// line is equal once its terminator and a trailing `\r` are removed (a
/// conversion that changed only line endings).
///
/// When this is false the conversion changed line structure (for example an
/// LFS pointer against its content), and the file's attribution is reported
/// unavailable rather than mapped onto the wrong lines.
#[must_use]
pub fn lines_correspond(blob: &[u8], attribution: &[u8]) -> bool {
    if blob == attribution {
        return true;
    }
    let blob_lines = split_lines(blob);
    let attribution_lines = split_lines(attribution);
    blob_lines.len() == attribution_lines.len()
        && blob_lines
            .iter()
            .zip(&attribution_lines)
            .all(|(a, b)| without_terminator(a) == without_terminator(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_bytes_correspond() {
        assert!(lines_correspond(b"a\nb\n", b"a\nb\n"));
        assert!(lines_correspond(b"", b""));
    }

    #[test]
    fn line_ending_conversion_corresponds() {
        assert!(lines_correspond(b"a\nb\n", b"a\r\nb\r\n"));
        assert!(lines_correspond(b"a\r\nb", b"a\nb"));
    }

    #[test]
    fn changed_content_does_not_correspond() {
        assert!(!lines_correspond(b"a\nb\n", b"a\nc\n"));
    }

    #[test]
    fn changed_line_structure_does_not_correspond() {
        let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:ab\nsize 3\n";
        assert!(!lines_correspond(pointer, b"abc"));
        assert!(!lines_correspond(b"a\nb\n", b"a b\n"));
    }
}
