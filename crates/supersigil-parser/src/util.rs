//! Shared utility functions for the parser crate.

/// Compute 1-based (line, column) from a byte offset in `content`.
pub(crate) fn line_col(content: &str, byte_offset: usize) -> (usize, usize) {
    let pos = byte_offset.min(content.len());
    let mut line = 1;
    let mut col = 1;
    for &b in &content.as_bytes()[..pos] {
        if b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Returns `true` if the name starts with an uppercase ASCII letter (`PascalCase`).
pub(crate) fn is_pascal_case(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
}
