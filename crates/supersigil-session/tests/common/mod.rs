//! Fixture helpers shared by the parser and sync tests.

use std::path::Path;

/// Session id of the slice fixture.
pub const SESSION: &str = "11111111-1111-4111-8111-111111111111";

/// Bytes of the slice fixture transcript.
pub fn fixture() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slice.jsonl")).unwrap()
}

/// Byte offset of every line in `bytes`.
pub fn line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && i + 1 < bytes.len() {
            starts.push(i + 1);
        }
    }
    starts
}
