//! Conversion between `SourcePosition` (1-based, byte offset) and
//! LSP `Position` (0-based, UTF-16 character offset).

use std::path::Path;

use lsp_types::{Position, Range};
use supersigil_core::SourcePosition;

/// A zero-width range at a single position.
#[must_use]
pub fn zero_range(pos: Position) -> Range {
    Range {
        start: pos,
        end: pos,
    }
}

/// Convert a 1-based (line, column) pair to a 0-based LSP Position.
///
/// Uses saturating subtraction so that (0, 0) inputs map to (0, 0) rather
/// than wrapping.
#[must_use]
#[allow(clippy::cast_possible_truncation, reason = "line/column fit in u32")]
pub fn raw_to_lsp(line: usize, column: usize) -> Position {
    Position {
        line: line.saturating_sub(1) as u32,
        character: column.saturating_sub(1) as u32,
    }
}

/// Convert a [`SourcePosition`] to a 0-based LSP Position.
#[must_use]
pub fn source_to_lsp(sp: &SourcePosition) -> Position {
    raw_to_lsp(sp.line, sp.column)
}

/// Convert a [`SourcePosition`] to an LSP [`Position`] with proper UTF-16
/// character offset computation.
///
/// Requires the full file content to compute the UTF-16 character offset
/// from the byte-based column.
#[must_use]
pub fn source_to_lsp_utf16(sp: &SourcePosition, content: &str) -> Position {
    #[allow(clippy::cast_possible_truncation, reason = "line numbers fit in u32")]
    let line = sp.line.saturating_sub(1) as u32;

    let line_start: usize = content
        .split('\n')
        .take(sp.line.saturating_sub(1))
        .map(|l| l.len() + 1)
        .sum();

    let byte_col = sp.column.saturating_sub(1);
    let line_content = &content[line_start..];

    #[allow(clippy::cast_possible_truncation, reason = "byte_col fits in u32")]
    let character = byte_to_utf16(line_content, byte_col as u32);

    Position { line, character }
}

/// Like [`source_to_lsp_utf16`] but reads the file content from disk.
///
/// Falls back to byte-based [`source_to_lsp`] if the file cannot be read.
#[must_use]
pub fn source_to_lsp_from_file(sp: &SourcePosition, path: &Path) -> Position {
    match std::fs::read_to_string(path) {
        Ok(content) => source_to_lsp_utf16(sp, &content),
        Err(_) => source_to_lsp(sp),
    }
}

/// Convert a byte offset within a line to a UTF-16 character offset.
#[must_use]
#[allow(clippy::cast_possible_truncation, reason = "len_utf16 returns 1 or 2")]
pub fn byte_to_utf16(line_str: &str, byte_offset: u32) -> u32 {
    line_str
        .char_indices()
        .take_while(|(i, _)| (*i as u32) < byte_offset)
        .map(|(_, c)| c.len_utf16() as u32)
        .sum()
}

/// Convert byte offsets within a line to an LSP `Range` with UTF-16 columns.
#[must_use]
#[allow(clippy::cast_possible_truncation, reason = "byte offsets fit in u32")]
pub fn byte_range_to_lsp(line_str: &str, line: u32, byte_start: usize, byte_end: usize) -> Range {
    let start_char = byte_to_utf16(line_str, byte_start as u32);
    let end_char = byte_to_utf16(line_str, byte_end as u32);
    Range {
        start: Position {
            line,
            character: start_char,
        },
        end: Position {
            line,
            character: end_char,
        },
    }
}

/// Convert a UTF-16 character offset to a byte offset within a line.
fn utf16_to_byte_offset(line_str: &str, utf16_offset: u32) -> usize {
    let mut utf16_count = 0u32;
    for (byte_idx, ch) in line_str.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_idx;
        }
        #[allow(clippy::cast_possible_truncation, reason = "len_utf16 returns 1 or 2")]
        {
            utf16_count += ch.len_utf16() as u32;
        }
    }
    line_str.len()
}

/// Convert a UTF-16 `character` offset to a byte offset given the full
/// document content and a 0-based line number.
///
/// Falls back to the original offset if the line is not found (e.g., line
/// number out of range).
#[must_use]
#[allow(clippy::cast_possible_truncation, reason = "byte offsets fit in u32")]
pub fn utf16_to_byte(content: &str, line: u32, utf16_char: u32) -> u32 {
    let Some(line_str) = content.lines().nth(line as usize) else {
        return utf16_char;
    };
    utf16_to_byte_offset(line_str, utf16_char) as u32
}
