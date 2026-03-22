//! Convert supersigil errors and findings to LSP diagnostics.

use std::path::PathBuf;

use lsp_types::{Diagnostic, DiagnosticSeverity, Url};
use supersigil_core::{GraphError, ParseError};
use supersigil_verify::{Finding, ReportSeverity};

use crate::DIAGNOSTIC_SOURCE;
use crate::path_to_url;
use crate::position::{raw_to_lsp, source_to_lsp_from_file, source_to_lsp_utf16, zero_range};

// ---------------------------------------------------------------------------
// Parse error → Diagnostic
// ---------------------------------------------------------------------------

/// Convert a [`ParseError`] to an `(Url, Diagnostic)` pair.
///
/// When `buffer` is provided, UTF-16 positions are computed from the live
/// buffer content (important for unsaved edits). Otherwise the file is
/// read from disk as a fallback.
///
/// Returns `None` if the error does not carry enough location information to
/// produce a valid file URL (e.g., the path cannot be turned into a `file://`
/// URI).
///
/// All parse errors map to [`DiagnosticSeverity::ERROR`].
#[must_use]
pub fn parse_error_to_diagnostic(
    err: &ParseError,
    buffer: Option<&str>,
) -> Option<(Url, Diagnostic)> {
    let (path, pos, message) = match err {
        ParseError::MdxSyntaxError {
            path,
            line,
            column,
            message,
        } => {
            let sp = supersigil_core::SourcePosition {
                byte_offset: 0,
                line: *line,
                column: *column,
            };
            (path, sp_to_lsp(&sp, path, buffer), message.clone())
        }
        ParseError::ExpressionAttribute {
            path,
            position,
            component,
            attribute,
        } => (
            path,
            sp_to_lsp(position, path, buffer),
            format!(
                "expression attribute `{attribute}` on `<{component}>` \
                 (only string literals supported)"
            ),
        ),
        ParseError::MissingRequiredAttribute {
            path,
            position,
            component,
            attribute,
        } => (
            path,
            sp_to_lsp(position, path, buffer),
            format!("missing required attribute `{attribute}` on `<{component}>`"),
        ),
        ParseError::UnclosedFrontMatter { path } => (
            path,
            raw_to_lsp(0, 0),
            "unclosed front matter (missing closing `---`)".to_owned(),
        ),
        ParseError::MissingId { path } => (
            path,
            raw_to_lsp(0, 0),
            "missing required `id` field in supersigil front matter".to_owned(),
        ),
        ParseError::InvalidYaml { path, message } => {
            (path, raw_to_lsp(0, 0), format!("invalid YAML: {message}"))
        }
        ParseError::IoError { path, source } => {
            (path, raw_to_lsp(0, 0), format!("I/O error: {source}"))
        }
    };

    let url = path_to_url(path)?;
    let diagnostic = Diagnostic {
        range: zero_range(pos),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some(DIAGNOSTIC_SOURCE.to_string()),
        message,
        ..Diagnostic::default()
    };
    Some((url, diagnostic))
}

/// Convert a [`SourcePosition`] using buffer content if available,
/// otherwise fall back to reading from disk.
fn sp_to_lsp(
    sp: &supersigil_core::SourcePosition,
    path: &std::path::Path,
    buffer: Option<&str>,
) -> lsp_types::Position {
    match buffer {
        Some(content) => source_to_lsp_utf16(sp, content),
        None => source_to_lsp_from_file(sp, path),
    }
}

// ---------------------------------------------------------------------------
// Graph error → Diagnostics
// ---------------------------------------------------------------------------

/// Convert a [`GraphError`] to zero or more `(Url, Diagnostic)` pairs.
///
/// Some graph errors reference multiple documents (e.g. `DuplicateId`), so
/// this function returns a `Vec`.
///
/// All graph errors map to [`DiagnosticSeverity::ERROR`].
pub(crate) fn graph_error_to_diagnostic(err: &GraphError) -> Vec<(Url, Diagnostic)> {
    match err {
        GraphError::DuplicateId { id, paths } => paths
            .iter()
            .filter_map(|path| {
                let url = path_to_url(path)?;
                let diag = Diagnostic {
                    range: zero_range(raw_to_lsp(0, 0)),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some(DIAGNOSTIC_SOURCE.to_string()),
                    message: format!("duplicate document ID `{id}`"),
                    ..Diagnostic::default()
                };
                Some((url, diag))
            })
            .collect(),

        GraphError::DuplicateComponentId {
            doc_id,
            component_id,
            ..
        } => {
            tracing::warn!(
                doc_id = %doc_id,
                component_id = %component_id,
                "DuplicateComponentId has no path; cannot produce LSP diagnostic"
            );
            vec![]
        }

        GraphError::BrokenRef {
            ref_str,
            reason,
            position,
            ..
        } => {
            tracing::warn!(
                ref_str = %ref_str,
                reason = %reason,
                ?position,
                "BrokenRef has no path; cannot produce LSP diagnostic without doc lookup"
            );
            vec![]
        }

        GraphError::TaskDependencyCycle { doc_id, cycle } => {
            tracing::warn!(
                doc_id = %doc_id,
                "TaskDependencyCycle has no path; cannot produce LSP diagnostic"
            );
            let _ = cycle;
            vec![]
        }

        GraphError::DocumentDependencyCycle { cycle } => {
            tracing::warn!(
                cycle = ?cycle,
                "DocumentDependencyCycle has no path; cannot produce LSP diagnostic"
            );
            vec![]
        }

        GraphError::InvalidComponentDef(err) => {
            tracing::warn!(
                error = %err,
                "InvalidComponentDef has no path; cannot produce LSP diagnostic"
            );
            vec![]
        }
    }
}

/// Convert a [`GraphError`] to diagnostics, using a `doc_id → path` lookup
/// to resolve file URLs for errors that carry only a document ID.
pub fn graph_error_to_diagnostic_with_lookup(
    err: &GraphError,
    doc_path: impl Fn(&str) -> Option<PathBuf>,
) -> Vec<(Url, Diagnostic)> {
    match err {
        GraphError::BrokenRef {
            doc_id,
            ref_str,
            reason,
            position,
        } => {
            let Some(path) = doc_path(doc_id) else {
                return vec![];
            };
            let Some(url) = path_to_url(&path) else {
                return vec![];
            };
            let pos = source_to_lsp_from_file(position, &path);
            let diag = Diagnostic {
                range: zero_range(pos),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some(DIAGNOSTIC_SOURCE.to_string()),
                message: format!("broken ref `{ref_str}`: {reason}"),
                ..Diagnostic::default()
            };
            vec![(url, diag)]
        }

        GraphError::DuplicateComponentId {
            doc_id,
            component_id,
            positions,
        } => {
            let Some(path) = doc_path(doc_id) else {
                return vec![];
            };
            let Some(url) = path_to_url(&path) else {
                return vec![];
            };
            positions
                .iter()
                .map(|sp| {
                    let diag = Diagnostic {
                        range: zero_range(source_to_lsp_from_file(sp, &path)),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some(DIAGNOSTIC_SOURCE.to_string()),
                        message: format!("duplicate component ID `{component_id}`"),
                        ..Diagnostic::default()
                    };
                    (url.clone(), diag)
                })
                .collect()
        }

        GraphError::TaskDependencyCycle { doc_id, cycle } => {
            let Some(path) = doc_path(doc_id) else {
                return vec![];
            };
            let Some(url) = path_to_url(&path) else {
                return vec![];
            };
            let diag = Diagnostic {
                range: zero_range(raw_to_lsp(0, 0)),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some(DIAGNOSTIC_SOURCE.to_string()),
                message: format!("dependency cycle in tasks: {}", cycle.join(" → ")),
                ..Diagnostic::default()
            };
            vec![(url, diag)]
        }

        GraphError::DocumentDependencyCycle { cycle } => {
            let message = format!("document dependency cycle: {}", cycle.join(" → "));
            cycle
                .iter()
                .filter_map(|doc_id| {
                    let path = doc_path(doc_id)?;
                    let url = path_to_url(&path)?;
                    let diag = Diagnostic {
                        range: zero_range(raw_to_lsp(0, 0)),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some(DIAGNOSTIC_SOURCE.to_string()),
                        message: message.clone(),
                        ..Diagnostic::default()
                    };
                    Some((url, diag))
                })
                .collect()
        }

        other => graph_error_to_diagnostic(other),
    }
}

// ---------------------------------------------------------------------------
// Severity mapping
// ---------------------------------------------------------------------------

/// Map a [`ReportSeverity`] to an LSP [`DiagnosticSeverity`].
///
/// Returns `None` for `Off` findings, which should be excluded from
/// published diagnostics (req-1-4).
#[must_use]
pub fn severity_to_lsp(severity: ReportSeverity) -> Option<DiagnosticSeverity> {
    match severity {
        ReportSeverity::Error => Some(DiagnosticSeverity::ERROR),
        ReportSeverity::Warning => Some(DiagnosticSeverity::WARNING),
        ReportSeverity::Info => Some(DiagnosticSeverity::HINT),
        ReportSeverity::Off => None,
    }
}

// ---------------------------------------------------------------------------
// Finding → Diagnostic
// ---------------------------------------------------------------------------

/// Convert a verification [`Finding`] to an `(Url, Diagnostic)` pair.
///
/// The `doc_path` parameter resolves a document ID to its file path.  It is
/// called only when the finding does not carry an explicit path in
/// `FindingDetails`.
///
/// Returns `None` when:
/// - The finding's effective severity is `Off` (req-1-4).
/// - No file URL can be determined (neither `FindingDetails.path` nor
///   `doc_path(doc_id)` yields a valid path).
pub fn finding_to_diagnostic(
    finding: &Finding,
    doc_path: impl Fn(&str) -> Option<PathBuf>,
) -> Option<(Url, Diagnostic)> {
    let mut lsp_severity = severity_to_lsp(finding.effective_severity)?;

    // Downgrade example-coverable findings to HINT since the LSP does not
    // execute examples (req-1-6).
    if finding
        .details
        .as_ref()
        .is_some_and(|d| d.example_coverable)
    {
        lsp_severity = DiagnosticSeverity::HINT;
    }

    let (url, pos) = if let Some(details) = &finding.details
        && let Some(path_str) = &details.path
    {
        let path: PathBuf = path_str.into();
        let url = path_to_url(&path)?;
        let line = details.line.unwrap_or(0);
        let col = details.column.unwrap_or(0);
        let sp = supersigil_core::SourcePosition {
            byte_offset: 0,
            line,
            column: col,
        };
        (url, source_to_lsp_from_file(&sp, &path))
    } else if let Some(sp) = &finding.position {
        let doc_id = finding.doc_id.as_deref().unwrap_or("");
        let path = doc_path(doc_id)?;
        let url = path_to_url(&path)?;
        (url, source_to_lsp_from_file(sp, &path))
    } else {
        let doc_id = finding.doc_id.as_deref().unwrap_or("");
        let path = doc_path(doc_id)?;
        let url = path_to_url(&path)?;
        (url, raw_to_lsp(0, 0))
    };

    let diagnostic = Diagnostic {
        range: zero_range(pos),
        severity: Some(lsp_severity),
        source: Some(DIAGNOSTIC_SOURCE.to_string()),
        message: finding.message.clone(),
        ..Diagnostic::default()
    };
    Some((url, diagnostic))
}

// ---------------------------------------------------------------------------
// Publish helpers
// ---------------------------------------------------------------------------

/// Group a flat list of `(Url, Diagnostic)` pairs into a map keyed by URL.
#[must_use]
pub fn group_by_url(
    pairs: Vec<(Url, Diagnostic)>,
) -> std::collections::HashMap<Url, Vec<Diagnostic>> {
    let mut map: std::collections::HashMap<Url, Vec<Diagnostic>> = std::collections::HashMap::new();
    for (url, diag) in pairs {
        map.entry(url).or_default().push(diag);
    }
    map
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use supersigil_core::SourcePosition;

    use super::*;

    fn dummy_source_pos(line: usize, column: usize) -> SourcePosition {
        SourcePosition {
            byte_offset: 0,
            line,
            column,
        }
    }

    // -----------------------------------------------------------------------
    // graph_error_to_diagnostic
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_id_produces_one_diagnostic_per_path() {
        let path = std::path::PathBuf::from("/tmp/spec-a.mdx");
        let err = GraphError::DuplicateId {
            id: "REQ-001".into(),
            paths: vec![path.clone()],
        };

        let pairs = graph_error_to_diagnostic(&err);
        assert_eq!(pairs.len(), 1);
        let (url, diag) = &pairs[0];
        assert_eq!(url.to_file_path().unwrap(), path);
        assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
        assert!(diag.message.contains("REQ-001"));
    }

    #[test]
    fn duplicate_id_with_multiple_paths_produces_multiple_diagnostics() {
        let err = GraphError::DuplicateId {
            id: "REQ-002".into(),
            paths: vec![
                std::path::PathBuf::from("/tmp/a.mdx"),
                std::path::PathBuf::from("/tmp/b.mdx"),
            ],
        };

        let pairs = graph_error_to_diagnostic(&err);
        assert_eq!(pairs.len(), 2);
        assert!(
            pairs
                .iter()
                .all(|(_, d)| d.severity == Some(DiagnosticSeverity::ERROR))
        );
    }

    #[test]
    fn broken_ref_without_lookup_returns_empty() {
        let err = GraphError::BrokenRef {
            doc_id: "REQ-001".into(),
            ref_str: "REQ-999".into(),
            reason: "not found".into(),
            position: dummy_source_pos(5, 3),
        };

        let pairs = graph_error_to_diagnostic(&err);
        assert!(
            pairs.is_empty(),
            "BrokenRef needs a doc-id lookup; without it returns empty"
        );
    }

    #[test]
    fn task_dependency_cycle_without_lookup_returns_empty() {
        let err = GraphError::TaskDependencyCycle {
            doc_id: "TASK-001".into(),
            cycle: vec!["TASK-001".into(), "TASK-002".into()],
        };

        let pairs = graph_error_to_diagnostic(&err);
        assert!(pairs.is_empty());
    }

    // -----------------------------------------------------------------------
    // graph_error_to_diagnostic_with_lookup
    // -----------------------------------------------------------------------

    #[test]
    fn broken_ref_with_lookup_produces_diagnostic() {
        let path = std::path::PathBuf::from("/tmp/req-001.mdx");
        let err = GraphError::BrokenRef {
            doc_id: "REQ-001".into(),
            ref_str: "REQ-999".into(),
            reason: "not found".into(),
            position: dummy_source_pos(10, 5),
        };

        let pairs = graph_error_to_diagnostic_with_lookup(&err, |id| {
            (id == "REQ-001").then(|| path.clone())
        });

        assert_eq!(pairs.len(), 1);
        let (url, diag) = &pairs[0];
        assert_eq!(url.to_file_path().unwrap(), path);
        assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
        assert!(diag.message.contains("REQ-999"));
        assert!(diag.message.contains("not found"));
        // line 10, col 5 → 0-based: (9, 4)
        assert_eq!(diag.range.start.line, 9);
        assert_eq!(diag.range.start.character, 4);
    }

    #[test]
    fn broken_ref_with_unknown_doc_id_returns_empty() {
        let err = GraphError::BrokenRef {
            doc_id: "REQ-UNKNOWN".into(),
            ref_str: "REQ-999".into(),
            reason: "not found".into(),
            position: dummy_source_pos(1, 1),
        };

        let pairs = graph_error_to_diagnostic_with_lookup(&err, |_| None);
        assert!(pairs.is_empty());
    }

    #[test]
    fn duplicate_component_id_with_lookup_produces_one_diagnostic_per_position() {
        let path = std::path::PathBuf::from("/tmp/req-comp.mdx");
        let err = GraphError::DuplicateComponentId {
            doc_id: "REQ-001".into(),
            component_id: "crit-1".into(),
            positions: vec![dummy_source_pos(3, 1), dummy_source_pos(7, 1)],
        };

        let pairs = graph_error_to_diagnostic_with_lookup(&err, |id| {
            (id == "REQ-001").then(|| path.clone())
        });

        assert_eq!(pairs.len(), 2);
        assert!(
            pairs
                .iter()
                .all(|(_, d)| d.severity == Some(DiagnosticSeverity::ERROR))
        );
        assert!(pairs.iter().all(|(_, d)| d.message.contains("crit-1")));
    }

    #[test]
    fn task_dependency_cycle_with_lookup_produces_diagnostic() {
        let path = std::path::PathBuf::from("/tmp/tasks.mdx");
        let err = GraphError::TaskDependencyCycle {
            doc_id: "TASKS".into(),
            cycle: vec!["TASK-A".into(), "TASK-B".into()],
        };

        let pairs =
            graph_error_to_diagnostic_with_lookup(&err, |id| (id == "TASKS").then(|| path.clone()));

        assert_eq!(pairs.len(), 1);
        let (_, diag) = &pairs[0];
        assert!(diag.message.contains("TASK-A"));
        assert!(diag.message.contains("TASK-B"));
    }
}
