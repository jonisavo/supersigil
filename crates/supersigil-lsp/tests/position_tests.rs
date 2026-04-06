use supersigil_core::SourcePosition;
use supersigil_lsp::position::{source_to_lsp_utf16, utf16_to_byte};
use supersigil_rust_macros::verifies;

#[test]
#[verifies("lsp-server/req#req-8-3")]
fn ascii_line_and_column() {
    // SourcePosition is 1-based; LSP Position is 0-based.
    // Line 3, column 6 in "line1\nline2\nline3content"
    let sp = SourcePosition {
        byte_offset: 17,
        line: 3,
        column: 6,
    };
    let content = "line1\nline2\nline3content";
    let pos = source_to_lsp_utf16(&sp, content);
    assert_eq!(pos.line, 2); // 3 - 1
    assert_eq!(pos.character, 5); // 6 - 1 (ASCII: byte == UTF-16 unit)
}

#[test]
fn first_position() {
    let sp = SourcePosition {
        byte_offset: 0,
        line: 1,
        column: 1,
    };
    let pos = source_to_lsp_utf16(&sp, "hello");
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 0);
}

#[test]
fn end_of_first_line() {
    let sp = SourcePosition {
        byte_offset: 5,
        line: 1,
        column: 6,
    };
    let pos = source_to_lsp_utf16(&sp, "hello\nworld");
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 5);
}

#[test]
fn start_of_second_line() {
    let sp = SourcePosition {
        byte_offset: 6,
        line: 2,
        column: 1,
    };
    let pos = source_to_lsp_utf16(&sp, "hello\nworld");
    assert_eq!(pos.line, 1);
    assert_eq!(pos.character, 0);
}

#[test]
#[verifies("lsp-server/req#req-8-3")]
fn multibyte_utf8() {
    // "aéb" = 'a' (1 byte, 1 UTF-16) + 'é' (2 bytes UTF-8, 1 UTF-16 unit) + 'b' (1 byte)
    // Column 4 means byte offset 3 within line (0-based: 3), which is after 'a'(1) + 'é'(2) = 3 bytes
    // That's 2 UTF-16 code units (a + é)
    let sp = SourcePosition {
        byte_offset: 3,
        line: 1,
        column: 4,
    };
    let content = "a\u{00E9}b";
    let pos = source_to_lsp_utf16(&sp, content);
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 2); // 'a' + 'é' = 2 UTF-16 units before byte 3
}

#[test]
#[verifies("lsp-server/req#req-8-3")]
fn emoji_surrogate_pair() {
    // "a😀b" = 'a' (1 byte, 1 UTF-16) + '😀' (4 bytes UTF-8, 2 UTF-16 units) + 'b' (1 byte)
    // Column 6 means byte offset 5 within line, which is after 'a'(1) + '😀'(4) = 5 bytes
    // That's 3 UTF-16 code units (a=1 + 😀=2)
    let sp = SourcePosition {
        byte_offset: 5,
        line: 1,
        column: 6,
    };
    let content = "a\u{1F600}b";
    let pos = source_to_lsp_utf16(&sp, content);
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 3); // 'a'(1) + '😀'(2) = 3 UTF-16 units
}

// ---------------------------------------------------------------------------
// utf16_to_byte (incoming: UTF-16 → byte offset)
// ---------------------------------------------------------------------------

#[test]
#[verifies("lsp-server/req#req-8-3")]
fn utf16_to_byte_ascii() {
    let content = "hello world";
    // UTF-16 offset 5 → byte 5 (ASCII is 1:1)
    assert_eq!(utf16_to_byte(content, 0, 5), 5);
}

#[test]
#[verifies("lsp-server/req#req-8-3")]
fn utf16_to_byte_multibyte() {
    // "aéb" = 'a' (1 byte, 1 UTF-16) + 'é' (2 bytes, 1 UTF-16) + 'b' (1 byte, 1 UTF-16)
    let content = "a\u{00E9}b";
    // UTF-16 offset 0 → byte 0 ('a')
    assert_eq!(utf16_to_byte(content, 0, 0), 0);
    // UTF-16 offset 1 → byte 1 ('é')
    assert_eq!(utf16_to_byte(content, 0, 1), 1);
    // UTF-16 offset 2 → byte 3 ('b', after 2-byte é)
    assert_eq!(utf16_to_byte(content, 0, 2), 3);
}

#[test]
#[verifies("lsp-server/req#req-8-3")]
fn utf16_to_byte_emoji() {
    // "a😀b" = 'a' (1 byte, 1 UTF-16) + '😀' (4 bytes, 2 UTF-16) + 'b' (1 byte, 1 UTF-16)
    let content = "a\u{1F600}b";
    // UTF-16 offset 1 → byte 1 ('😀')
    assert_eq!(utf16_to_byte(content, 0, 1), 1);
    // UTF-16 offset 3 → byte 5 ('b', after 1-byte a + 4-byte 😀)
    assert_eq!(utf16_to_byte(content, 0, 3), 5);
}

#[test]
fn utf16_to_byte_second_line() {
    let content = "first\na\u{00E9}b";
    // Line 1: "aéb", UTF-16 offset 2 → byte 3
    assert_eq!(utf16_to_byte(content, 1, 2), 3);
}

#[test]
fn utf16_to_byte_past_end() {
    let content = "abc";
    // UTF-16 offset 10 → clamps to line length (3)
    assert_eq!(utf16_to_byte(content, 0, 10), 3);
}

#[test]
fn utf16_to_byte_invalid_line() {
    let content = "abc";
    // Line 5 doesn't exist → falls back to returning the offset as-is
    assert_eq!(utf16_to_byte(content, 5, 7), 7);
}
