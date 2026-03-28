//! Convert supersigil errors and findings to LSP diagnostics.

use std::path::PathBuf;

use lsp_types::{Diagnostic, DiagnosticSeverity, Url};
use serde::{Deserialize, Serialize};
use supersigil_core::{GraphError, ParseError, ParseWarning};
use supersigil_verify::{Finding, ReportSeverity, RuleName};

use crate::DIAGNOSTIC_SOURCE;
use crate::path_to_url;
use crate::position::{raw_to_lsp, source_to_lsp_from_file, source_to_lsp_utf16, zero_range};

// ---------------------------------------------------------------------------
// DiagnosticData types
// ---------------------------------------------------------------------------

/// Structured metadata attached to the `data` field of an LSP [`Diagnostic`].
///
/// This allows code action providers to inspect the diagnostic and determine
/// which quick-fix actions are applicable without re-parsing the message text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticData {
    pub source: DiagnosticSource,
    pub doc_id: Option<String>,
    pub context: ActionContext,
}

/// Identifies which subsystem produced the diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiagnosticSource {
    Parse(ParseDiagnosticKind),
    Graph(GraphDiagnosticKind),
    Verify(RuleName),
}

/// Categorises parse-stage diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParseDiagnosticKind {
    MissingRequiredAttribute,
    UnknownComponent,
    XmlSyntaxError,
    UnclosedFrontmatter,
    DuplicateCodeRef,
    Other,
}

/// Categorises graph-stage diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphDiagnosticKind {
    DuplicateDocumentId,
    DuplicateComponentId,
    BrokenRef,
    DependencyCycle,
    InvalidComponent,
}

/// Fix-specific metadata carried by a diagnostic for code action providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionContext {
    None,
    BrokenRef {
        target_ref: String,
    },
    MissingAttribute {
        component: String,
        attribute: String,
    },
    DuplicateId {
        id: String,
        other_path: String,
    },
    IncompleteDecision {
        decision_id: String,
    },
    MissingComponent {
        component: String,
        parent_id: String,
    },
    OrphanDecision {
        decision_id: String,
    },
    InvalidPlacement {
        component: String,
        expected_parent: String,
    },
    SequentialIdGap {
        component_type: String,
    },
}

/// Serialize a [`DiagnosticData`] into a [`serde_json::Value`] suitable for
/// the `data` field of an LSP [`Diagnostic`].
fn diagnostic_data_to_value(data: &DiagnosticData) -> serde_json::Value {
    serde_json::to_value(data).expect("DiagnosticData serialization should not fail")
}

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
/// All parse errors map to [`DiagnosticSeverity::ERROR`] by default. Use
/// [`parse_warning_to_diagnostic`] for code-ref warnings that should map
/// to [`DiagnosticSeverity::WARNING`].
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "match arms for each ParseError variant"
)]
pub fn parse_error_to_diagnostic(
    err: &ParseError,
    buffer: Option<&str>,
) -> Option<(Url, Diagnostic)> {
    let (path, pos, message, kind, context) = match err {
        ParseError::XmlSyntaxError {
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
            (
                path,
                sp_to_lsp(&sp, path, buffer),
                message.clone(),
                ParseDiagnosticKind::XmlSyntaxError,
                ActionContext::None,
            )
        }
        ParseError::MissingRequiredAttribute {
            path,
            position,
            component,
            attribute,
        } => (
            path,
            sp_to_lsp(position, path, buffer),
            format!("missing required attribute `{attribute}` on `<{component}>`"),
            ParseDiagnosticKind::MissingRequiredAttribute,
            ActionContext::MissingAttribute {
                component: component.clone(),
                attribute: attribute.clone(),
            },
        ),
        ParseError::UnclosedFrontMatter { path } => (
            path,
            raw_to_lsp(0, 0),
            "unclosed front matter (missing closing `---`)".to_owned(),
            ParseDiagnosticKind::UnclosedFrontmatter,
            ActionContext::None,
        ),
        ParseError::MissingId { path } => (
            path,
            raw_to_lsp(0, 0),
            "missing required `id` field in supersigil front matter".to_owned(),
            ParseDiagnosticKind::Other,
            ActionContext::None,
        ),
        ParseError::InvalidYaml { path, message } => (
            path,
            raw_to_lsp(0, 0),
            format!("invalid YAML: {message}"),
            ParseDiagnosticKind::Other,
            ActionContext::None,
        ),
        ParseError::IoError { path, source } => (
            path,
            raw_to_lsp(0, 0),
            format!("I/O error: {source}"),
            ParseDiagnosticKind::Other,
            ActionContext::None,
        ),
        ParseError::OrphanCodeRef {
            path,
            target,
            content_offset,
        } => {
            let (line, column) = if let Some(buf) = buffer {
                let offset = (*content_offset).min(buf.len());
                let before = &buf[..offset];
                let line = before.chars().filter(|&c| c == '\n').count() + 1;
                let last_nl = before.rfind('\n').map_or(0, |p| p + 1);
                let column = offset - last_nl + 1;
                (line, column)
            } else {
                (1, 1)
            };
            let sp = supersigil_core::SourcePosition {
                byte_offset: *content_offset,
                line,
                column,
            };
            (
                path,
                sp_to_lsp(&sp, path, buffer),
                format!("orphan supersigil-ref `{target}` (no matching component)"),
                ParseDiagnosticKind::Other,
                ActionContext::None,
            )
        }
        ParseError::DuplicateCodeRef { path, target } => (
            path,
            raw_to_lsp(0, 0),
            format!("duplicate supersigil-ref fences targeting `{target}`"),
            ParseDiagnosticKind::DuplicateCodeRef,
            ActionContext::None,
        ),
        ParseError::DualSourceConflict {
            path,
            target,
            content_offset,
        } => {
            let (line, column) = if let Some(buf) = buffer {
                let offset = (*content_offset).min(buf.len());
                let before = &buf[..offset];
                let line = before.chars().filter(|&c| c == '\n').count() + 1;
                let last_nl = before.rfind('\n').map_or(0, |p| p + 1);
                let column = offset - last_nl + 1;
                (line, column)
            } else {
                (1, 1)
            };
            let sp = supersigil_core::SourcePosition {
                byte_offset: *content_offset,
                line,
                column,
            };
            (
                path,
                sp_to_lsp(&sp, path, buffer),
                format!(
                    "dual-source conflict for `{target}` (both inline text and supersigil-ref fence)"
                ),
                ParseDiagnosticKind::Other,
                ActionContext::None,
            )
        }
    };

    let url = path_to_url(path)?;
    let data = DiagnosticData {
        source: DiagnosticSource::Parse(kind),
        doc_id: None,
        context,
    };
    let diagnostic = Diagnostic {
        range: zero_range(pos),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some(DIAGNOSTIC_SOURCE.to_string()),
        message,
        data: Some(diagnostic_data_to_value(&data)),
        ..Diagnostic::default()
    };
    Some((url, diagnostic))
}

/// Convert a [`ParseWarning`] to a diagnostic with [`DiagnosticSeverity::WARNING`].
#[must_use]
pub fn parse_warning_to_diagnostic(
    warn: &ParseWarning,
    buffer: Option<&str>,
) -> Option<(Url, Diagnostic)> {
    let (path, pos, message, kind) = match warn {
        ParseWarning::OrphanCodeRef {
            path,
            target,
            content_offset,
        } => {
            let (line, column) = if let Some(buf) = buffer {
                let offset = (*content_offset).min(buf.len());
                let before = &buf[..offset];
                let line = before.chars().filter(|&c| c == '\n').count() + 1;
                let last_nl = before.rfind('\n').map_or(0, |p| p + 1);
                let column = offset - last_nl + 1;
                (line, column)
            } else {
                (1, 1)
            };
            let sp = supersigil_core::SourcePosition {
                byte_offset: *content_offset,
                line,
                column,
            };
            (
                path,
                sp_to_lsp(&sp, path, buffer),
                format!("orphan supersigil-ref `{target}` (no matching component)"),
                ParseDiagnosticKind::Other,
            )
        }
        ParseWarning::DuplicateCodeRef { path, target } => (
            path,
            raw_to_lsp(0, 0),
            format!("duplicate supersigil-ref fences targeting `{target}`"),
            ParseDiagnosticKind::DuplicateCodeRef,
        ),
        ParseWarning::DualSourceConflict {
            path,
            target,
            content_offset,
        } => {
            let (line, column) = if let Some(buf) = buffer {
                let offset = (*content_offset).min(buf.len());
                let before = &buf[..offset];
                let line = before.chars().filter(|&c| c == '\n').count() + 1;
                let last_nl = before.rfind('\n').map_or(0, |p| p + 1);
                let column = offset - last_nl + 1;
                (line, column)
            } else {
                (1, 1)
            };
            let sp = supersigil_core::SourcePosition {
                byte_offset: *content_offset,
                line,
                column,
            };
            (
                path,
                sp_to_lsp(&sp, path, buffer),
                format!(
                    "dual-source conflict for `{target}` (both inline text and supersigil-ref fence)"
                ),
                ParseDiagnosticKind::Other,
            )
        }
    };

    let url = path_to_url(path)?;
    let data = DiagnosticData {
        source: DiagnosticSource::Parse(kind),
        doc_id: None,
        context: ActionContext::None,
    };
    let diagnostic = Diagnostic {
        range: zero_range(pos),
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some(DIAGNOSTIC_SOURCE.to_string()),
        message,
        data: Some(diagnostic_data_to_value(&data)),
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
            .enumerate()
            .filter_map(|(i, path)| {
                let url = path_to_url(path)?;
                // For each path, the "other" is any path that isn't this one.
                let other_path = paths
                    .iter()
                    .enumerate()
                    .find(|(j, _)| *j != i)
                    .map_or_else(String::new, |(_, p)| p.display().to_string());
                let data = DiagnosticData {
                    source: DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateDocumentId),
                    doc_id: Some(id.clone()),
                    context: ActionContext::DuplicateId {
                        id: id.clone(),
                        other_path,
                    },
                };
                let diag = Diagnostic {
                    range: zero_range(raw_to_lsp(0, 0)),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some(DIAGNOSTIC_SOURCE.to_string()),
                    message: format!("duplicate document ID `{id}`"),
                    data: Some(diagnostic_data_to_value(&data)),
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
#[allow(
    clippy::too_many_lines,
    reason = "match arms with DiagnosticData for each GraphError variant"
)]
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
            let data = DiagnosticData {
                source: DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef),
                doc_id: Some(doc_id.clone()),
                context: ActionContext::BrokenRef {
                    target_ref: ref_str.clone(),
                },
            };
            let diag = Diagnostic {
                range: zero_range(pos),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some(DIAGNOSTIC_SOURCE.to_string()),
                message: format!("broken ref `{ref_str}`: {reason}"),
                data: Some(diagnostic_data_to_value(&data)),
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
                    let data = DiagnosticData {
                        source: DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateComponentId),
                        doc_id: Some(doc_id.clone()),
                        context: ActionContext::DuplicateId {
                            id: component_id.clone(),
                            other_path: String::new(),
                        },
                    };
                    let diag = Diagnostic {
                        range: zero_range(source_to_lsp_from_file(sp, &path)),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some(DIAGNOSTIC_SOURCE.to_string()),
                        message: format!("duplicate component ID `{component_id}`"),
                        data: Some(diagnostic_data_to_value(&data)),
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
            let data = DiagnosticData {
                source: DiagnosticSource::Graph(GraphDiagnosticKind::DependencyCycle),
                doc_id: Some(doc_id.clone()),
                context: ActionContext::None,
            };
            let diag = Diagnostic {
                range: zero_range(raw_to_lsp(0, 0)),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some(DIAGNOSTIC_SOURCE.to_string()),
                message: format!("dependency cycle in tasks: {}", cycle.join(" → ")),
                data: Some(diagnostic_data_to_value(&data)),
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
                    let data = DiagnosticData {
                        source: DiagnosticSource::Graph(GraphDiagnosticKind::DependencyCycle),
                        doc_id: Some(doc_id.clone()),
                        context: ActionContext::None,
                    };
                    let diag = Diagnostic {
                        range: zero_range(raw_to_lsp(0, 0)),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some(DIAGNOSTIC_SOURCE.to_string()),
                        message: message.clone(),
                        data: Some(diagnostic_data_to_value(&data)),
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
// Finding context enrichment
// ---------------------------------------------------------------------------

/// Extract a backtick-delimited token starting at position `pos` in `msg`.
///
/// Looks for `` `token` `` starting at or after `pos`. Returns the token
/// content and the byte position after the closing backtick.
pub(crate) fn extract_backtick_token(msg: &str, pos: usize) -> Option<(String, usize)> {
    let start = msg[pos..].find('`')? + pos + 1;
    let end = msg[start..].find('`')? + start;
    Some((msg[start..end].to_string(), end + 1))
}

/// Build an [`ActionContext`] from a [`Finding`] based on its rule and message.
///
/// This avoids code-action providers having to re-parse the diagnostic message
/// string. Falls back to `ActionContext::None` for unrecognised patterns.
fn enrich_finding_context(finding: &Finding) -> ActionContext {
    let msg = &finding.message;
    let doc_id = finding.doc_id.clone().unwrap_or_default();

    match finding.rule {
        RuleName::MissingRequiredComponent => {
            // "document `{doc_id}` (type `{doc_type}`) is missing required component `{required}`"
            if let Some(rest) = msg
                .find("missing required component `")
                .map(|p| p + "missing required component `".len())
                && let Some(end) = msg[rest..].find('`')
            {
                let component = msg[rest..rest + end].to_string();
                ActionContext::MissingComponent {
                    component,
                    parent_id: doc_id,
                }
            } else {
                ActionContext::None
            }
        }
        RuleName::OrphanDecision => {
            // "Decision `{label}` in `{doc_id}` is orphan: ..."
            if let Some((decision_id, _)) = extract_backtick_token(msg, 0) {
                ActionContext::OrphanDecision { decision_id }
            } else {
                ActionContext::None
            }
        }
        RuleName::InvalidRationalePlacement => ActionContext::InvalidPlacement {
            component: "Rationale".to_string(),
            expected_parent: "Decision".to_string(),
        },
        RuleName::InvalidAlternativePlacement => ActionContext::InvalidPlacement {
            component: "Alternative".to_string(),
            expected_parent: "Decision".to_string(),
        },
        RuleName::InvalidExpectedPlacement => ActionContext::InvalidPlacement {
            component: "Expected".to_string(),
            expected_parent: "Example".to_string(),
        },
        RuleName::SequentialIdGap | RuleName::SequentialIdOrder => {
            // Try to extract the component type from the first backtick-delimited ID.
            // E.g. "gap in sequence: `task-2` is missing" → prefix "task"
            // Or "`task-1` is declared after `task-2`" → prefix "task"
            let component_type = extract_backtick_token(msg, 0)
                .and_then(|(id, _)| {
                    let last_dash = id.rfind('-')?;
                    Some(id[..last_dash].to_string())
                })
                .unwrap_or_default();
            ActionContext::SequentialIdGap { component_type }
        }
        RuleName::IncompleteDecision => {
            // "Decision in `{doc_id}` has no Rationale child; ..."
            // The decision_id is not directly in the message, but doc_id is.
            // We use doc_id as a fallback identifier.
            ActionContext::IncompleteDecision {
                decision_id: doc_id,
            }
        }
        _ => ActionContext::None,
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
        if let Some(line) = details.line {
            let col = details.column.unwrap_or(0);
            let sp = supersigil_core::SourcePosition {
                byte_offset: 0,
                line,
                column: col,
            };
            (url, source_to_lsp_from_file(&sp, &path))
        } else if let Some(sp) = &finding.position {
            (url, source_to_lsp_from_file(sp, &path))
        } else {
            (url, raw_to_lsp(0, 0))
        }
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

    let context = enrich_finding_context(finding);
    let data = DiagnosticData {
        source: DiagnosticSource::Verify(finding.rule),
        doc_id: finding.doc_id.clone(),
        context,
    };
    let diagnostic = Diagnostic {
        range: zero_range(pos),
        severity: Some(lsp_severity),
        source: Some(DIAGNOSTIC_SOURCE.to_string()),
        message: finding.message.clone(),
        data: Some(diagnostic_data_to_value(&data)),
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
    use supersigil_rust_macros::verifies;

    use super::*;

    fn dummy_source_pos(line: usize, column: usize) -> SourcePosition {
        SourcePosition {
            byte_offset: 0,
            line,
            column,
        }
    }

    /// Helper: deserialise the `data` field of a diagnostic back into
    /// [`DiagnosticData`], panicking if it is absent or malformed.
    fn extract_data(diag: &Diagnostic) -> DiagnosticData {
        let value = diag.data.as_ref().expect("diagnostic should have data");
        serde_json::from_value(value.clone()).expect("data should deserialize to DiagnosticData")
    }

    // -----------------------------------------------------------------------
    // graph_error_to_diagnostic
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_id_produces_one_diagnostic_per_path() {
        let path = std::path::PathBuf::from("/tmp/spec-a.md");
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
                std::path::PathBuf::from("/tmp/a.md"),
                std::path::PathBuf::from("/tmp/b.md"),
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
        let path = std::path::PathBuf::from("/tmp/req-001.md");
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
        let path = std::path::PathBuf::from("/tmp/req-comp.md");
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

    // -----------------------------------------------------------------------
    // finding_to_diagnostic
    // -----------------------------------------------------------------------

    #[test]
    fn finding_with_position_and_details_path_but_no_line_uses_position() {
        // When attach_doc_paths sets details.path but not details.line/column,
        // the diagnostic should use finding.position, not default to (0, 0).
        let path = std::path::PathBuf::from("/tmp/test-doc.md");

        // Write a minimal file so source_to_lsp_from_file can read it.
        let _ = std::fs::write(
            &path,
            "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\n<Decision>\n",
        );

        let mut finding = Finding::new(
            supersigil_verify::RuleName::IncompleteDecision,
            Some("test/doc".into()),
            "Decision has no Rationale".into(),
            Some(dummy_source_pos(10, 1)),
        );
        // Simulate what attach_doc_paths does: set details.path without line/column
        finding.details = Some(Box::new(supersigil_verify::FindingDetails {
            path: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        }));

        let result = finding_to_diagnostic(&finding, |_| Some(path.clone()));

        let (_, diag) = result.expect("should produce a diagnostic");
        // Should use finding.position (line 10) → LSP line 9, not (0, 0)
        assert_eq!(
            diag.range.start.line, 9,
            "diagnostic should point to line 10 (0-based: 9), not line 0"
        );
        assert_eq!(diag.range.start.character, 0);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn finding_with_details_path_and_line_uses_details_line() {
        // When details has both path AND line, those should be used (existing behavior).
        let path = std::path::PathBuf::from("/tmp/test-doc2.md");
        let _ = std::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n");

        let mut finding = Finding::new(
            supersigil_verify::RuleName::IncompleteDecision,
            Some("test/doc".into()),
            "Decision has no Rationale".into(),
            Some(dummy_source_pos(10, 1)), // finding.position says line 10
        );
        // details says line 5 — details.line should win
        finding.details = Some(Box::new(supersigil_verify::FindingDetails {
            path: Some(path.to_string_lossy().into_owned()),
            line: Some(5),
            column: Some(3),
            ..Default::default()
        }));

        let result = finding_to_diagnostic(&finding, |_| Some(path.clone()));

        let (_, diag) = result.expect("should produce a diagnostic");
        // details.line = 5 → LSP line 4
        assert_eq!(
            diag.range.start.line, 4,
            "should use details.line (5 → 0-based 4)"
        );

        let _ = std::fs::remove_file(&path);
    }

    // -----------------------------------------------------------------------
    // OrphanCodeRef / DualSourceConflict position computation
    // -----------------------------------------------------------------------

    #[test]
    fn orphan_code_ref_diagnostic_uses_correct_line_from_byte_offset() {
        let path = std::path::PathBuf::from("/tmp/orphan-ref.md");
        // content_offset 12 → "line1\nline2\n" → line 3, column 1
        let buffer = "line1\nline2\norphan ref here\n";
        let err = ParseError::OrphanCodeRef {
            path: path.clone(),
            target: "some-target".into(),
            content_offset: 12,
        };

        let result = parse_error_to_diagnostic(&err, Some(buffer));
        let (_, diag) = result.expect("should produce a diagnostic");
        // Line 3 → 0-based LSP line 2
        assert_eq!(
            diag.range.start.line, 2,
            "orphan code ref should point to line 3 (0-based: 2), not line 0"
        );
        assert_eq!(
            diag.range.start.character, 0,
            "orphan code ref should point to column 1 (0-based: 0)"
        );
    }

    #[test]
    fn dual_source_conflict_diagnostic_uses_correct_line_from_byte_offset() {
        let path = std::path::PathBuf::from("/tmp/dual-source.md");
        // content_offset 18 → "line1\nline2\nline3\n" → line 4, column 1
        let buffer = "line1\nline2\nline3\ndual source here\n";
        let err = ParseError::DualSourceConflict {
            path: path.clone(),
            target: "some-target".into(),
            content_offset: 18,
        };

        let result = parse_error_to_diagnostic(&err, Some(buffer));
        let (_, diag) = result.expect("should produce a diagnostic");
        // Line 4 → 0-based LSP line 3
        assert_eq!(
            diag.range.start.line, 3,
            "dual source conflict should point to line 4 (0-based: 3), not line 0"
        );
    }

    #[test]
    fn task_dependency_cycle_with_lookup_produces_diagnostic() {
        let path = std::path::PathBuf::from("/tmp/tasks.md");
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

    // -----------------------------------------------------------------------
    // DiagnosticData on parse errors (req-1-2)
    // -----------------------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-1-2")]
    #[test]
    fn parse_error_missing_attribute_attaches_data() {
        let path = std::path::PathBuf::from("/tmp/parse-attr.md");
        let err = ParseError::MissingRequiredAttribute {
            path: path.clone(),
            component: "Criterion".into(),
            attribute: "id".into(),
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
        };

        let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
        let data = extract_data(&diag);

        assert!(matches!(
            data.source,
            DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute)
        ));
        assert!(data.doc_id.is_none());
        match &data.context {
            ActionContext::MissingAttribute {
                component,
                attribute,
            } => {
                assert_eq!(component, "Criterion");
                assert_eq!(attribute, "id");
            }
            other => panic!("expected MissingAttribute, got {other:?}"),
        }
    }

    #[test]
    fn parse_error_xml_syntax_attaches_data() {
        let path = std::path::PathBuf::from("/tmp/parse-xml.md");
        let err = ParseError::XmlSyntaxError {
            path: path.clone(),
            line: 3,
            column: 5,
            message: "unexpected EOF".into(),
        };

        let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
        let data = extract_data(&diag);

        assert!(matches!(
            data.source,
            DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError)
        ));
        assert!(matches!(data.context, ActionContext::None));
    }

    #[test]
    fn parse_error_unclosed_frontmatter_attaches_data() {
        let path = std::path::PathBuf::from("/tmp/parse-fm.md");
        let err = ParseError::UnclosedFrontMatter { path: path.clone() };

        let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
        let data = extract_data(&diag);

        assert!(matches!(
            data.source,
            DiagnosticSource::Parse(ParseDiagnosticKind::UnclosedFrontmatter)
        ));
    }

    #[test]
    fn parse_error_duplicate_code_ref_attaches_data() {
        let path = std::path::PathBuf::from("/tmp/parse-dup.md");
        let err = ParseError::DuplicateCodeRef {
            path: path.clone(),
            target: "comp-1".into(),
        };

        let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
        let data = extract_data(&diag);

        assert!(matches!(
            data.source,
            DiagnosticSource::Parse(ParseDiagnosticKind::DuplicateCodeRef)
        ));
    }

    #[test]
    fn parse_error_other_variants_attach_other_kind() {
        let path = std::path::PathBuf::from("/tmp/parse-id.md");
        let err = ParseError::MissingId { path: path.clone() };

        let (_, diag) = parse_error_to_diagnostic(&err, None).unwrap();
        let data = extract_data(&diag);

        assert!(matches!(
            data.source,
            DiagnosticSource::Parse(ParseDiagnosticKind::Other)
        ));
    }

    // -----------------------------------------------------------------------
    // DiagnosticData on parse warnings (req-1-2)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_warning_duplicate_code_ref_attaches_data() {
        let path = std::path::PathBuf::from("/tmp/warn-dup.md");
        let warn = ParseWarning::DuplicateCodeRef {
            path: path.clone(),
            target: "comp-1".into(),
        };

        let (_, diag) = parse_warning_to_diagnostic(&warn, None).unwrap();
        let data = extract_data(&diag);

        assert!(matches!(
            data.source,
            DiagnosticSource::Parse(ParseDiagnosticKind::DuplicateCodeRef)
        ));
        assert!(matches!(data.context, ActionContext::None));
    }

    // -----------------------------------------------------------------------
    // DiagnosticData on graph errors (req-1-3)
    // -----------------------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-1-3")]
    #[test]
    fn graph_error_duplicate_id_attaches_data() {
        let err = GraphError::DuplicateId {
            id: "REQ-001".into(),
            paths: vec![
                std::path::PathBuf::from("/tmp/a.md"),
                std::path::PathBuf::from("/tmp/b.md"),
            ],
        };

        let pairs = graph_error_to_diagnostic(&err);
        assert_eq!(pairs.len(), 2);

        let data_0 = extract_data(&pairs[0].1);
        assert!(matches!(
            data_0.source,
            DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateDocumentId)
        ));
        assert_eq!(data_0.doc_id.as_deref(), Some("REQ-001"));
        match &data_0.context {
            ActionContext::DuplicateId { id, other_path } => {
                assert_eq!(id, "REQ-001");
                assert!(other_path.contains("b.md"));
            }
            other => panic!("expected DuplicateId, got {other:?}"),
        }

        // Second diagnostic should point back at first path
        let data_1 = extract_data(&pairs[1].1);
        match &data_1.context {
            ActionContext::DuplicateId { id, other_path } => {
                assert_eq!(id, "REQ-001");
                assert!(other_path.contains("a.md"));
            }
            other => panic!("expected DuplicateId, got {other:?}"),
        }
    }

    #[test]
    fn graph_error_broken_ref_with_lookup_attaches_data() {
        let path = std::path::PathBuf::from("/tmp/broken-ref.md");
        let err = GraphError::BrokenRef {
            doc_id: "REQ-001".into(),
            ref_str: "REQ-999".into(),
            reason: "not found".into(),
            position: dummy_source_pos(5, 3),
        };

        let pairs = graph_error_to_diagnostic_with_lookup(&err, |id| {
            (id == "REQ-001").then(|| path.clone())
        });
        assert_eq!(pairs.len(), 1);

        let data = extract_data(&pairs[0].1);
        assert!(matches!(
            data.source,
            DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef)
        ));
        assert_eq!(data.doc_id.as_deref(), Some("REQ-001"));
        match &data.context {
            ActionContext::BrokenRef { target_ref } => {
                assert_eq!(target_ref, "REQ-999");
            }
            other => panic!("expected BrokenRef, got {other:?}"),
        }
    }

    #[test]
    fn graph_error_duplicate_component_id_with_lookup_attaches_data() {
        let path = std::path::PathBuf::from("/tmp/dup-comp.md");
        let err = GraphError::DuplicateComponentId {
            doc_id: "REQ-001".into(),
            component_id: "crit-1".into(),
            positions: vec![dummy_source_pos(3, 1)],
        };

        let pairs = graph_error_to_diagnostic_with_lookup(&err, |id| {
            (id == "REQ-001").then(|| path.clone())
        });
        assert_eq!(pairs.len(), 1);

        let data = extract_data(&pairs[0].1);
        assert!(matches!(
            data.source,
            DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateComponentId)
        ));
        assert_eq!(data.doc_id.as_deref(), Some("REQ-001"));
        match &data.context {
            ActionContext::DuplicateId { id, .. } => {
                assert_eq!(id, "crit-1");
            }
            other => panic!("expected DuplicateId, got {other:?}"),
        }
    }

    #[test]
    fn graph_error_task_dependency_cycle_with_lookup_attaches_data() {
        let path = std::path::PathBuf::from("/tmp/cycle.md");
        let err = GraphError::TaskDependencyCycle {
            doc_id: "TASKS".into(),
            cycle: vec!["A".into(), "B".into()],
        };

        let pairs =
            graph_error_to_diagnostic_with_lookup(&err, |id| (id == "TASKS").then(|| path.clone()));
        assert_eq!(pairs.len(), 1);

        let data = extract_data(&pairs[0].1);
        assert!(matches!(
            data.source,
            DiagnosticSource::Graph(GraphDiagnosticKind::DependencyCycle)
        ));
        assert_eq!(data.doc_id.as_deref(), Some("TASKS"));
    }

    #[test]
    fn graph_error_document_dependency_cycle_with_lookup_attaches_data() {
        let path_a = std::path::PathBuf::from("/tmp/cycle-a.md");
        let path_b = std::path::PathBuf::from("/tmp/cycle-b.md");
        let err = GraphError::DocumentDependencyCycle {
            cycle: vec!["DOC-A".into(), "DOC-B".into()],
        };

        let pairs = graph_error_to_diagnostic_with_lookup(&err, |id| match id {
            "DOC-A" => Some(path_a.clone()),
            "DOC-B" => Some(path_b.clone()),
            _ => None,
        });
        assert_eq!(pairs.len(), 2);

        for (_, diag) in &pairs {
            let data = extract_data(diag);
            assert!(matches!(
                data.source,
                DiagnosticSource::Graph(GraphDiagnosticKind::DependencyCycle)
            ));
        }
    }

    // -----------------------------------------------------------------------
    // DiagnosticData on verify findings (req-1-1)
    // -----------------------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-1-1")]
    #[test]
    fn finding_diagnostic_attaches_verify_data() {
        let path = std::path::PathBuf::from("/tmp/finding-data.md");

        let finding = Finding::new(
            RuleName::MissingRequiredComponent,
            Some("REQ-001".into()),
            "missing required component".into(),
            None,
        );

        let result = finding_to_diagnostic(&finding, |_| Some(path.clone()));
        let (_, diag) = result.expect("should produce a diagnostic");
        let data = extract_data(&diag);

        assert!(matches!(
            data.source,
            DiagnosticSource::Verify(RuleName::MissingRequiredComponent)
        ));
        assert_eq!(data.doc_id.as_deref(), Some("REQ-001"));
        assert!(matches!(data.context, ActionContext::None));
    }

    // -----------------------------------------------------------------------
    // Round-trip serialization (req-1-4)
    // -----------------------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-1-4")]
    #[test]
    fn diagnostic_data_round_trips_through_json() {
        let original = DiagnosticData {
            source: DiagnosticSource::Verify(RuleName::IncompleteDecision),
            doc_id: Some("DEC-001".into()),
            context: ActionContext::IncompleteDecision {
                decision_id: "DEC-001".into(),
            },
        };

        let json = serde_json::to_value(&original).unwrap();
        let deserialized: DiagnosticData = serde_json::from_value(json).unwrap();

        // Verify source
        assert!(matches!(
            deserialized.source,
            DiagnosticSource::Verify(RuleName::IncompleteDecision)
        ));
        assert_eq!(deserialized.doc_id.as_deref(), Some("DEC-001"));

        // Verify context
        match deserialized.context {
            ActionContext::IncompleteDecision { decision_id } => {
                assert_eq!(decision_id, "DEC-001");
            }
            other => panic!("expected IncompleteDecision, got {other:?}"),
        }
    }

    #[test]
    fn diagnostic_data_round_trips_parse_source() {
        let original = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute),
            doc_id: None,
            context: ActionContext::MissingAttribute {
                component: "Criterion".into(),
                attribute: "id".into(),
            },
        };

        let json = serde_json::to_value(&original).unwrap();
        let deserialized: DiagnosticData = serde_json::from_value(json).unwrap();

        assert!(matches!(
            deserialized.source,
            DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute)
        ));
        match deserialized.context {
            ActionContext::MissingAttribute {
                component,
                attribute,
            } => {
                assert_eq!(component, "Criterion");
                assert_eq!(attribute, "id");
            }
            other => panic!("expected MissingAttribute, got {other:?}"),
        }
    }

    #[test]
    fn diagnostic_data_round_trips_graph_broken_ref() {
        let original = DiagnosticData {
            source: DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef),
            doc_id: Some("REQ-001".into()),
            context: ActionContext::BrokenRef {
                target_ref: "REQ-999".into(),
            },
        };

        let json = serde_json::to_value(&original).unwrap();
        let deserialized: DiagnosticData = serde_json::from_value(json).unwrap();

        assert!(matches!(
            deserialized.source,
            DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef)
        ));
        match deserialized.context {
            ActionContext::BrokenRef { target_ref } => {
                assert_eq!(target_ref, "REQ-999");
            }
            other => panic!("expected BrokenRef, got {other:?}"),
        }
    }

    #[test]
    fn diagnostic_data_round_trips_all_action_context_variants() {
        let contexts = vec![
            ActionContext::None,
            ActionContext::BrokenRef {
                target_ref: "REQ-X".into(),
            },
            ActionContext::MissingAttribute {
                component: "C".into(),
                attribute: "a".into(),
            },
            ActionContext::DuplicateId {
                id: "id".into(),
                other_path: "/tmp/x.md".into(),
            },
            ActionContext::IncompleteDecision {
                decision_id: "d".into(),
            },
            ActionContext::MissingComponent {
                component: "Criterion".into(),
                parent_id: "REQ-001".into(),
            },
            ActionContext::OrphanDecision {
                decision_id: "DEC-001".into(),
            },
            ActionContext::InvalidPlacement {
                component: "VerifiedBy".into(),
                expected_parent: "Criterion".into(),
            },
            ActionContext::SequentialIdGap {
                component_type: "Criterion".into(),
            },
        ];

        for ctx in contexts {
            let data = DiagnosticData {
                source: DiagnosticSource::Verify(RuleName::EmptyProject),
                doc_id: None,
                context: ctx,
            };
            let json = serde_json::to_value(&data).unwrap();
            let _: DiagnosticData = serde_json::from_value(json)
                .expect("every ActionContext variant should round-trip");
        }
    }

    // -----------------------------------------------------------------------
    // enrich_finding_context
    // -----------------------------------------------------------------------

    #[test]
    fn enrich_missing_required_component() {
        let finding = Finding::new(
            RuleName::MissingRequiredComponent,
            Some("auth/req".into()),
            "document `auth/req` (type `requirements`) is missing required component `AcceptanceCriteria`".into(),
            None,
        );
        match enrich_finding_context(&finding) {
            ActionContext::MissingComponent {
                component,
                parent_id,
            } => {
                assert_eq!(component, "AcceptanceCriteria");
                assert_eq!(parent_id, "auth/req");
            }
            other => panic!("expected MissingComponent, got {other:?}"),
        }
    }

    #[test]
    fn enrich_missing_required_component_no_backtick() {
        let finding = Finding::new(
            RuleName::MissingRequiredComponent,
            Some("auth/req".into()),
            "some unusual message".into(),
            None,
        );
        assert!(matches!(
            enrich_finding_context(&finding),
            ActionContext::None
        ));
    }

    #[test]
    fn enrich_orphan_decision() {
        let finding = Finding::new(
            RuleName::OrphanDecision,
            Some("adr/logging".into()),
            "Decision `use-postgres` in `adr/logging` is orphan: no outward connections and not referenced by any other component".into(),
            None,
        );
        match enrich_finding_context(&finding) {
            ActionContext::OrphanDecision { decision_id } => {
                assert_eq!(decision_id, "use-postgres");
            }
            other => panic!("expected OrphanDecision, got {other:?}"),
        }
    }

    #[test]
    fn enrich_invalid_rationale_placement() {
        let finding = Finding::new(
            RuleName::InvalidRationalePlacement,
            Some("adr/design".into()),
            "Rationale in `adr/design` is placed at document root; it must be a direct child of Decision".into(),
            None,
        );
        match enrich_finding_context(&finding) {
            ActionContext::InvalidPlacement {
                component,
                expected_parent,
            } => {
                assert_eq!(component, "Rationale");
                assert_eq!(expected_parent, "Decision");
            }
            other => panic!("expected InvalidPlacement, got {other:?}"),
        }
    }

    #[test]
    fn enrich_invalid_alternative_placement() {
        let finding = Finding::new(
            RuleName::InvalidAlternativePlacement,
            Some("adr/design".into()),
            "Alternative in `adr/design` is placed at document root; it must be a direct child of Decision".into(),
            None,
        );
        match enrich_finding_context(&finding) {
            ActionContext::InvalidPlacement {
                component,
                expected_parent,
            } => {
                assert_eq!(component, "Alternative");
                assert_eq!(expected_parent, "Decision");
            }
            other => panic!("expected InvalidPlacement, got {other:?}"),
        }
    }

    #[test]
    fn enrich_invalid_expected_placement() {
        let finding = Finding::new(
            RuleName::InvalidExpectedPlacement,
            Some("auth/req".into()),
            "Expected in `auth/req` is placed at document root; it must be a direct child of Example".into(),
            None,
        );
        match enrich_finding_context(&finding) {
            ActionContext::InvalidPlacement {
                component,
                expected_parent,
            } => {
                assert_eq!(component, "Expected");
                assert_eq!(expected_parent, "Example");
            }
            other => panic!("expected InvalidPlacement, got {other:?}"),
        }
    }

    #[test]
    fn enrich_sequential_id_gap() {
        let finding = Finding::new(
            RuleName::SequentialIdGap,
            Some("my-doc".into()),
            "gap in sequence: `task-2` is missing (between `task-1` and `task-3` in document `my-doc`)".into(),
            None,
        );
        match enrich_finding_context(&finding) {
            ActionContext::SequentialIdGap { component_type } => {
                assert_eq!(component_type, "task");
            }
            other => panic!("expected SequentialIdGap, got {other:?}"),
        }
    }

    #[test]
    fn enrich_sequential_id_order() {
        let finding = Finding::new(
            RuleName::SequentialIdOrder,
            Some("my-doc".into()),
            "`task-1` is declared after `task-2` in document `my-doc`".into(),
            None,
        );
        match enrich_finding_context(&finding) {
            ActionContext::SequentialIdGap { component_type } => {
                assert_eq!(component_type, "task");
            }
            other => panic!("expected SequentialIdGap, got {other:?}"),
        }
    }

    #[test]
    fn enrich_incomplete_decision() {
        let finding = Finding::new(
            RuleName::IncompleteDecision,
            Some("adr/logging".into()),
            "Decision in `adr/logging` has no Rationale child; every Decision should include a Rationale".into(),
            None,
        );
        match enrich_finding_context(&finding) {
            ActionContext::IncompleteDecision { decision_id } => {
                assert_eq!(decision_id, "adr/logging");
            }
            other => panic!("expected IncompleteDecision, got {other:?}"),
        }
    }

    #[test]
    fn enrich_unrelated_rule_returns_none() {
        let finding = Finding::new(
            RuleName::EmptyProject,
            None,
            "project is empty".into(),
            None,
        );
        assert!(matches!(
            enrich_finding_context(&finding),
            ActionContext::None
        ));
    }
}
