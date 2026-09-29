//! Fixture helpers shared by the parser and sync tests.

use std::path::Path;

/// Session ID used in the fixture transcript.
pub const SESSION: &str = "11111111-1111-4111-8111-111111111111";

/// Reads the fixture transcript as bytes.
pub fn fixture() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slice.jsonl")).unwrap()
}

/// Returns each line's starting byte offset, including zero for empty input.
pub fn line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && i + 1 < bytes.len() {
            starts.push(i + 1);
        }
    }
    starts
}
