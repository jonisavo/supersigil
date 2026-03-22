//! Integration tests for diagnostics conversion (req-1-1 through req-1-5).

use std::path::PathBuf;

use lsp_types::{DiagnosticSeverity, Url};
use supersigil_core::{ParseError, SourcePosition};
use supersigil_lsp::diagnostics::{
    finding_to_diagnostic, parse_error_to_diagnostic, severity_to_lsp,
};
use supersigil_verify::{Finding, ReportSeverity, RuleName};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Build a `file://` URL for a given absolute path string.
fn file_url(path: &str) -> Url {
    Url::from_file_path(path).expect("valid file path")
}

/// Build a dummy absolute path that can be turned into a `file://` URL.
fn abs_path(name: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/test/{name}"))
}

// ---------------------------------------------------------------------------
// parse_error_to_diagnostic
// ---------------------------------------------------------------------------

#[test]
fn xml_syntax_error_maps_to_error_with_position() {
    let path = abs_path("spec.md");
    let err = ParseError::XmlSyntaxError {
        path: path.clone(),
        line: 5,
        column: 3,
        message: "unexpected token".to_owned(),
    };

    let (url, diag) = parse_error_to_diagnostic(&err, None).expect("should produce diagnostic");

    assert_eq!(url, file_url("/tmp/test/spec.md"));
    assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
    // LSP positions are 0-based.
    assert_eq!(diag.range.start.line, 4);
    assert_eq!(diag.range.start.character, 2);
    assert!(diag.message.contains("unexpected token"));
}

#[test]
fn missing_id_maps_to_error_at_origin() {
    let path = abs_path("spec.md");
    let err = ParseError::MissingId { path: path.clone() };

    let (url, diag) = parse_error_to_diagnostic(&err, None).expect("should produce diagnostic");

    assert_eq!(url, file_url("/tmp/test/spec.md"));
    assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diag.range.start.line, 0);
    assert_eq!(diag.range.start.character, 0);
    assert!(diag.message.contains("id"));
}

#[test]
fn unclosed_front_matter_maps_to_origin() {
    let path = abs_path("spec.md");
    let err = ParseError::UnclosedFrontMatter { path: path.clone() };

    let (_, diag) = parse_error_to_diagnostic(&err, None).expect("should produce diagnostic");

    assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diag.range.start.line, 0);
    assert_eq!(diag.range.start.character, 0);
}

#[test]
fn invalid_yaml_maps_to_origin() {
    let path = abs_path("spec.md");
    let err = ParseError::InvalidYaml {
        path: path.clone(),
        message: "unexpected scalar".to_owned(),
    };

    let (_, diag) = parse_error_to_diagnostic(&err, None).expect("should produce diagnostic");

    assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diag.range.start.line, 0);
    assert!(diag.message.contains("unexpected scalar"));
}

#[test]
fn missing_required_attribute_maps_position() {
    let path = abs_path("spec.md");
    let err = ParseError::MissingRequiredAttribute {
        path: path.clone(),
        component: "Criterion".to_owned(),
        attribute: "id".to_owned(),
        position: SourcePosition {
            byte_offset: 0,
            line: 2,
            column: 1,
        },
    };

    let (_, diag) = parse_error_to_diagnostic(&err, None).expect("should produce diagnostic");

    assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diag.range.start.line, 1);
    assert_eq!(diag.range.start.character, 0);
}

#[test]
fn parse_error_uses_buffer_content_for_utf16_conversion() {
    // Buffer content has a 2-byte UTF-8 char (é) before the error column.
    // "aéb<X" — 'a'(1 byte) + 'é'(2 bytes) + 'b'(1 byte) + '<'(1 byte) + 'X'(1 byte)
    // The parser reports byte column 5 (1-based) for '<X'.
    // UTF-16: 'a'(1) + 'é'(1) + 'b'(1) + '<'(1) = character 4 (0-based: 3).
    // Without buffer, source_to_lsp_from_file would read a non-existent file
    // and fall back to byte-based: 5-1 = 4 (wrong).
    let path = abs_path("buffer_test.md");
    let buffer = "a\u{00E9}b<X";
    let err = ParseError::XmlSyntaxError {
        path,
        line: 1,
        column: 5,
        message: "unexpected".into(),
    };

    let (_, diag) = parse_error_to_diagnostic(&err, Some(buffer)).expect("should produce");

    assert_eq!(diag.range.start.line, 0);
    // With buffer: byte col 4 → UTF-16 col 3 (é is 1 UTF-16 unit, not 2 bytes)
    assert_eq!(diag.range.start.character, 3);
}

// ---------------------------------------------------------------------------
// severity_to_lsp
// ---------------------------------------------------------------------------

#[test]
fn severity_error_maps_to_lsp_error() {
    assert_eq!(
        severity_to_lsp(ReportSeverity::Error),
        Some(DiagnosticSeverity::ERROR)
    );
}

#[test]
fn severity_warning_maps_to_lsp_warning() {
    assert_eq!(
        severity_to_lsp(ReportSeverity::Warning),
        Some(DiagnosticSeverity::WARNING)
    );
}

#[test]
fn severity_info_maps_to_lsp_hint() {
    assert_eq!(
        severity_to_lsp(ReportSeverity::Info),
        Some(DiagnosticSeverity::HINT)
    );
}

#[test]
fn severity_off_maps_to_none() {
    assert_eq!(severity_to_lsp(ReportSeverity::Off), None);
}

// ---------------------------------------------------------------------------
// finding_to_diagnostic
// ---------------------------------------------------------------------------

fn no_path_lookup(_: &str) -> Option<PathBuf> {
    None
}

fn path_lookup(doc_id: &str) -> Option<PathBuf> {
    match doc_id {
        "req/auth" => Some(abs_path("req/auth.md")),
        _ => None,
    }
}

#[test]
fn finding_warning_maps_to_lsp_warning() {
    let finding = Finding {
        rule: RuleName::ZeroTagMatches,
        doc_id: Some("req/auth".to_owned()),
        message: "tag has zero matches".to_owned(),
        effective_severity: ReportSeverity::Warning,
        raw_severity: ReportSeverity::Warning,
        position: None,
        details: None,
    };

    let (_, diag) =
        finding_to_diagnostic(&finding, path_lookup).expect("should produce diagnostic");

    assert_eq!(diag.severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(diag.message, "tag has zero matches");
}

#[test]
fn finding_info_maps_to_lsp_hint() {
    let finding = Finding {
        rule: RuleName::EmptyProject,
        doc_id: Some("req/auth".to_owned()),
        message: "informational note".to_owned(),
        effective_severity: ReportSeverity::Info,
        raw_severity: ReportSeverity::Info,
        position: None,
        details: None,
    };

    let (_, diag) =
        finding_to_diagnostic(&finding, path_lookup).expect("should produce diagnostic");

    assert_eq!(diag.severity, Some(DiagnosticSeverity::HINT));
}

#[test]
fn finding_off_is_filtered_out() {
    let finding = Finding {
        rule: RuleName::IsolatedDocument,
        doc_id: Some("req/auth".to_owned()),
        message: "isolated document".to_owned(),
        effective_severity: ReportSeverity::Off,
        raw_severity: ReportSeverity::Off,
        position: None,
        details: None,
    };

    assert!(
        finding_to_diagnostic(&finding, path_lookup).is_none(),
        "Off findings should be filtered out"
    );
}

#[test]
fn finding_without_doc_id_returns_none_when_no_lookup() {
    let finding = Finding {
        rule: RuleName::EmptyProject,
        doc_id: None,
        message: "empty project".to_owned(),
        effective_severity: ReportSeverity::Warning,
        raw_severity: ReportSeverity::Warning,
        position: None,
        details: None,
    };

    assert!(
        finding_to_diagnostic(&finding, no_path_lookup).is_none(),
        "should return None when no path can be determined"
    );
}

#[test]
fn finding_with_position_uses_source_position() {
    let finding = Finding {
        rule: RuleName::InvalidIdPattern,
        doc_id: Some("req/auth".to_owned()),
        message: "bad id pattern".to_owned(),
        effective_severity: ReportSeverity::Warning,
        raw_severity: ReportSeverity::Warning,
        position: Some(SourcePosition {
            byte_offset: 0,
            line: 3,
            column: 5,
        }),
        details: None,
    };

    let (url, diag) =
        finding_to_diagnostic(&finding, path_lookup).expect("should produce diagnostic");

    assert_eq!(url, file_url("/tmp/test/req/auth.md"));
    // 1-based (3, 5) → 0-based (2, 4)
    assert_eq!(diag.range.start.line, 2);
    assert_eq!(diag.range.start.character, 4);
}

#[test]
fn finding_with_details_path_uses_details_path() {
    use supersigil_verify::FindingDetails;

    let finding = Finding {
        rule: RuleName::MissingTestFiles,
        doc_id: Some("req/auth".to_owned()),
        message: "no test files".to_owned(),
        effective_severity: ReportSeverity::Error,
        raw_severity: ReportSeverity::Error,
        position: None,
        details: Some(Box::new(FindingDetails {
            path: Some("/tmp/test/req/auth.md".to_owned()),
            line: Some(7),
            column: Some(2),
            ..FindingDetails::default()
        })),
    };

    let (url, diag) =
        finding_to_diagnostic(&finding, no_path_lookup).expect("should produce diagnostic");

    assert_eq!(url, file_url("/tmp/test/req/auth.md"));
    assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
    // 1-based (7, 2) → 0-based (6, 1)
    assert_eq!(diag.range.start.line, 6);
    assert_eq!(diag.range.start.character, 1);
}
