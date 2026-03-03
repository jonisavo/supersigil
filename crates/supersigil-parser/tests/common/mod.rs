use std::path::PathBuf;

use supersigil_core::{ExtractedComponent, ParseError};
use supersigil_parser::{extract_components, parse_mdx_body};

pub fn dummy_path() -> PathBuf {
    PathBuf::from("test.mdx")
}

/// Parse MDX body and extract components using default component defs.
pub fn extract(body: &str, body_offset: usize) -> (Vec<ExtractedComponent>, Vec<ParseError>) {
    let ast = parse_mdx_body(body, &dummy_path()).expect("MDX should parse");
    let mut errors = Vec::new();
    let components = extract_components(&ast, body_offset, &dummy_path(), &mut errors);
    (components, errors)
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
