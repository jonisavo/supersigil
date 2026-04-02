// Preprocessing stage tests

mod common;
use common::dummy_path;

use supersigil_parser::preprocess;

#[test]
fn valid_utf8_passthrough() {
    let input = b"hello world";
    let result = preprocess(input, &dummy_path()).unwrap();
    assert_eq!(result, "hello world");
}

#[test]
fn non_utf8_returns_io_error() {
    // 0xFF 0xFE is not valid UTF-8 (without being a BOM in UTF-8 context)
    let input: &[u8] = &[0x80, 0x81, 0x82];
    let result = preprocess(input, &dummy_path());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, supersigil_core::ParseError::IoError { .. }),
        "expected IoError, got {err:?}"
    );
}

#[test]
fn bom_stripped() {
    // UTF-8 BOM is EF BB BF
    let mut input = vec![0xEF, 0xBB, 0xBF];
    input.extend_from_slice(b"content after bom");
    let result = preprocess(&input, &dummy_path()).unwrap();
    assert_eq!(result, "content after bom");
}

#[test]
fn no_bom_content_unchanged() {
    let input = b"no bom here";
    let result = preprocess(input, &dummy_path()).unwrap();
    assert_eq!(result, "no bom here");
}

#[test]
fn file_with_only_bom_produces_empty_string() {
    let input: &[u8] = &[0xEF, 0xBB, 0xBF];
    let result = preprocess(input, &dummy_path()).unwrap();
    assert_eq!(result, "");
}

#[test]
fn bom_followed_by_front_matter_delimiter() {
    let mut input = vec![0xEF, 0xBB, 0xBF];
    input.extend_from_slice(b"---\nsupersigil:\n  id: test\n---\n");
    let result = preprocess(&input, &dummy_path()).unwrap();
    assert!(
        result.starts_with("---"),
        "BOM should be stripped, leaving --- at start"
    );
}

#[test]
fn crlf_normalized_to_lf() {
    let input = b"line1\r\nline2\r\nline3";
    let result = preprocess(input, &dummy_path()).unwrap();
    assert_eq!(result, "line1\nline2\nline3");
    assert!(!result.contains("\r\n"), "no CRLF should remain");
}

#[test]
fn mixed_crlf_and_lf_normalized_bare_cr_preserved() {
    // Mix of \r\n and \n, plus a bare \r
    let input = b"a\r\nb\nc\rd\r\ne";
    let result = preprocess(input, &dummy_path()).unwrap();
    assert_eq!(result, "a\nb\nc\rd\ne");
    assert!(!result.contains("\r\n"), "no CRLF should remain");
    assert!(result.contains('\r'), "bare \\r should be preserved");
}

#[test]
fn file_with_only_crlf() {
    let input = b"\r\n";
    let result = preprocess(input, &dummy_path()).unwrap();
    assert_eq!(result, "\n");
}
