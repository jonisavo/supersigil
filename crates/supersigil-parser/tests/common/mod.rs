use std::path::PathBuf;

pub fn dummy_path() -> PathBuf {
    PathBuf::from("test.md")
}

/// Count CRLF pairs, bare CR, and bare LF in a byte string.
///
/// Returns `(crlf_pairs, bare_cr, bare_lf)`.
#[allow(dead_code, reason = "only used by property_tests, not unit_tests")]
pub fn count_line_endings(bytes: &[u8]) -> (usize, usize, usize) {
    let mut crlf = 0;
    let mut bare_cr = 0;
    let mut bare_lf = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            crlf += 1;
            i += 2;
        } else if bytes[i] == b'\r' {
            bare_cr += 1;
            i += 1;
        } else if bytes[i] == b'\n' {
            bare_lf += 1;
            i += 1;
        } else {
            i += 1;
        }
    }
    (crlf, bare_cr, bare_lf)
}
