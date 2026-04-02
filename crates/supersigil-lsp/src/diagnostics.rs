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
                let message = if other_path.is_empty() {
                    format!("duplicate document ID `{id}`")
                } else {
                    format!("duplicate document ID `{id}` (also in {other_path})")
                };
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
                    message,
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
mod tests;
