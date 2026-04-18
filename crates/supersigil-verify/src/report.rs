//! Verification report types: findings, summaries, and result status.

use serde::{Deserialize, Serialize};
use supersigil_core::{Severity, SourcePosition};

// ---------------------------------------------------------------------------
// ReportSeverity
// ---------------------------------------------------------------------------

/// Severity level used in verification findings.
///
/// Extends the core `Severity` with an `Info` level for purely informational
/// findings that do not represent violations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportSeverity {
    /// Rule is disabled.
    Off,
    /// Informational finding.
    Info,
    /// Non-blocking warning.
    Warning,
    /// Blocking error.
    Error,
}

impl From<Severity> for ReportSeverity {
    fn from(s: Severity) -> Self {
        match s {
            Severity::Off => Self::Off,
            Severity::Warning => Self::Warning,
            Severity::Error => Self::Error,
        }
    }
}

// ---------------------------------------------------------------------------
// RuleName
// ---------------------------------------------------------------------------

/// Identifies a specific verification rule.
///
/// The built-in rules correspond 1:1 with `KNOWN_RULES` in supersigil-core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleName {
    /// A criterion has no verification evidence.
    MissingVerificationEvidence,
    /// A document's test file globs matched no files.
    MissingTestFiles,
    /// A tag yielded zero matches in scanned files.
    ZeroTagMatches,
    /// A tracked-files glob pattern matches no files.
    EmptyTrackedGlob,
    /// A tag in test code does not correspond to any document.
    OrphanTestTag,
    /// A document ID does not match the configured pattern.
    InvalidIdPattern,
    /// A document has no edges to other documents.
    IsolatedDocument,
    /// A document's status is inconsistent with its dependencies.
    StatusInconsistency,
    /// A graph construction error (broken ref, duplicate ID, cycle).
    BrokenRef,
    /// A `VerifiedBy` component is placed in a disallowed position.
    InvalidVerifiedByPlacement,
    /// A plugin failed during evidence discovery.
    PluginDiscoveryFailure,
    /// A plugin emitted a non-fatal warning during discovery.
    PluginDiscoveryWarning,
    /// Sequential IDs within a document are out of order.
    SequentialIdOrder,
    /// Sequential IDs have gaps in their numbering.
    SequentialIdGap,
    /// A `Rationale` component is placed in a disallowed position.
    InvalidRationalePlacement,
    /// An `Alternative` component is placed in a disallowed position.
    InvalidAlternativePlacement,
    /// A decision has more than one `Rationale`.
    DuplicateRationale,
    /// An `Alternative` has an invalid status value.
    InvalidAlternativeStatus,
    /// A decision is missing required sub-components.
    IncompleteDecision,
    /// A decision is not referenced by any document.
    OrphanDecision,
    /// A decision's criteria lack evidence coverage.
    MissingDecisionCoverage,
    /// No documents were found in the project.
    EmptyProject,
}

impl RuleName {
    /// The built-in verification rules.
    pub const ALL: &[Self] = &[
        Self::MissingVerificationEvidence,
        Self::MissingTestFiles,
        Self::ZeroTagMatches,
        Self::EmptyTrackedGlob,
        Self::OrphanTestTag,
        Self::InvalidIdPattern,
        Self::IsolatedDocument,
        Self::StatusInconsistency,
        Self::BrokenRef,
        Self::InvalidVerifiedByPlacement,
        Self::PluginDiscoveryFailure,
        Self::PluginDiscoveryWarning,
        Self::SequentialIdOrder,
        Self::SequentialIdGap,
        Self::InvalidRationalePlacement,
        Self::InvalidAlternativePlacement,
        Self::DuplicateRationale,
        Self::InvalidAlternativeStatus,
        Self::IncompleteDecision,
        Self::OrphanDecision,
        Self::MissingDecisionCoverage,
        Self::EmptyProject,
    ];
}

// Compile-time check: every RuleName variant must have a KNOWN_RULES entry.
const _: () = assert!(RuleName::ALL.len() == supersigil_core::KNOWN_RULES.len());

impl RuleName {
    /// Returns the config key string used in `[verify.rules]`.
    #[must_use]
    pub fn config_key(self) -> &'static str {
        match self {
            Self::MissingVerificationEvidence => "missing_verification_evidence",
            Self::MissingTestFiles => "missing_test_files",
            Self::ZeroTagMatches => "zero_tag_matches",
            Self::EmptyTrackedGlob => "empty_tracked_glob",
            Self::OrphanTestTag => "orphan_test_tag",
            Self::InvalidIdPattern => "invalid_id_pattern",
            Self::IsolatedDocument => "isolated_document",
            Self::StatusInconsistency => "status_inconsistency",
            Self::BrokenRef => "broken_ref",
            Self::InvalidVerifiedByPlacement => "invalid_verified_by_placement",
            Self::PluginDiscoveryFailure => "plugin_discovery_failure",
            Self::PluginDiscoveryWarning => "plugin_discovery_warning",
            Self::SequentialIdOrder => "sequential_id_order",
            Self::SequentialIdGap => "sequential_id_gap",
            Self::InvalidRationalePlacement => "invalid_rationale_placement",
            Self::InvalidAlternativePlacement => "invalid_alternative_placement",
            Self::DuplicateRationale => "duplicate_rationale",
            Self::InvalidAlternativeStatus => "invalid_alternative_status",
            Self::IncompleteDecision => "incomplete_decision",
            Self::OrphanDecision => "orphan_decision",
            Self::MissingDecisionCoverage => "missing_decision_coverage",
            Self::EmptyProject => "empty_project",
        }
    }

    /// Returns the default severity for this rule when no config override
    /// is present.
    #[must_use]
    pub fn default_severity(self) -> ReportSeverity {
        match self {
            Self::MissingVerificationEvidence
            | Self::MissingTestFiles
            | Self::InvalidVerifiedByPlacement
            | Self::BrokenRef => ReportSeverity::Error,

            Self::IsolatedDocument | Self::MissingDecisionCoverage => ReportSeverity::Off,

            Self::EmptyProject
            | Self::ZeroTagMatches
            | Self::EmptyTrackedGlob
            | Self::OrphanTestTag
            | Self::InvalidIdPattern
            | Self::StatusInconsistency
            | Self::PluginDiscoveryFailure
            | Self::PluginDiscoveryWarning
            | Self::SequentialIdOrder
            | Self::SequentialIdGap
            | Self::InvalidRationalePlacement
            | Self::InvalidAlternativePlacement
            | Self::DuplicateRationale
            | Self::InvalidAlternativeStatus
            | Self::IncompleteDecision
            | Self::OrphanDecision => ReportSeverity::Warning,
        }
    }
}

// ---------------------------------------------------------------------------
// Finding
// ---------------------------------------------------------------------------

/// A single verification finding produced by a rule.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Which rule produced this finding.
    pub rule: RuleName,
    /// Document ID this finding belongs to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_id: Option<String>,
    /// Human-readable description of the finding.
    pub message: String,
    /// Severity after config overrides and status-based adjustments.
    pub effective_severity: ReportSeverity,
    /// Severity before any overrides.
    pub raw_severity: ReportSeverity,
    /// Source position in the spec file, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<SourcePosition>,
    /// Structured metadata for programmatic consumers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Box<FindingDetails>>,
    /// A "did you mean?" suggestion for broken references.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// Structured metadata for programmatic remediation and source attribution.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FindingDetails {
    /// Name of the plugin that produced the finding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    /// Verification target reference (e.g. `"doc#criterion"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<String>,
    /// File path related to the finding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Line number in the related file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// Column number in the related file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    /// Machine-readable error code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Suggested fix for the finding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl Finding {
    /// Create a finding with default severity for the given rule.
    #[must_use]
    pub fn new(
        rule: RuleName,
        doc_id: Option<String>,
        message: String,
        position: Option<SourcePosition>,
    ) -> Self {
        let severity = rule.default_severity();
        Self {
            rule,
            doc_id,
            message,
            effective_severity: severity,
            raw_severity: severity,
            position,
            details: None,
            suggestion: None,
        }
    }

    /// Attach structured details to a finding without changing its headline text.
    #[must_use]
    pub fn with_details(mut self, details: FindingDetails) -> Self {
        self.details = Some(Box::new(details));
        self
    }

    /// Attach a "did you mean?" suggestion for broken references.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: String) -> Self {
        self.suggestion = Some(suggestion);
        self
    }
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

/// Aggregate counts for a verification run.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// Number of documents in scope.
    pub total_documents: usize,
    /// Number of error-level findings.
    pub error_count: usize,
    /// Number of warning-level findings.
    pub warning_count: usize,
    /// Number of info-level findings.
    pub info_count: usize,
}

impl Summary {
    /// Build a summary by counting findings by severity.
    #[must_use]
    pub fn from_findings(total_documents: usize, findings: &[Finding]) -> Self {
        let (mut error_count, mut warning_count, mut info_count) = (0, 0, 0);
        for f in findings {
            match f.effective_severity {
                ReportSeverity::Error => error_count += 1,
                ReportSeverity::Warning => warning_count += 1,
                ReportSeverity::Info => info_count += 1,
                ReportSeverity::Off => {}
            }
        }
        Self {
            total_documents,
            error_count,
            warning_count,
            info_count,
        }
    }
}

// ---------------------------------------------------------------------------
// ResultStatus
// ---------------------------------------------------------------------------

/// Overall outcome of a verification run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    /// No findings above info level.
    Clean,
    /// At least one error-level finding.
    HasErrors,
    /// Warnings present but no errors.
    WarningsOnly,
}

// ---------------------------------------------------------------------------
// EvidenceSummary (req-9-1, req-9-2, req-9-3)
// ---------------------------------------------------------------------------

/// Evidence summary for report enrichment (req-9-1, req-9-2, req-9-3).
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceSummary {
    /// All effective evidence records after merge.
    pub records: Vec<EvidenceReportEntry>,
    /// Evidence counts per verification target.
    pub coverage: Vec<TargetCoverage>,
    /// Any conflicts detected during merge.
    pub conflict_count: usize,
}

/// A single evidence entry for report output.
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceReportEntry {
    /// Name of the test function.
    pub test_name: String,
    /// Path to the test file.
    pub test_file: String,
    /// Classification of the test (e.g. "unit", "async").
    pub test_kind: String,
    /// How the evidence was discovered (e.g. "rust-attribute").
    pub evidence_kind: String,
    /// Verification target references covered by this test.
    pub targets: Vec<String>,
    /// Provenance chain for the evidence.
    pub provenance: Vec<String>,
    /// Source file where the evidence was discovered.
    pub source_file: String,
    /// Source line (1-based) where the evidence was discovered.
    pub source_line: usize,
    /// Source column (1-based) where the evidence was discovered.
    pub source_column: usize,
}

/// Coverage status for a single verification target.
///
/// Note: entries are only created for criteria that have at least one
/// evidence record, so `test_count` is always >= 1. Consumers should
/// check `test_count > 0` if they need a boolean "covered" flag.
#[derive(Debug, Clone, Serialize)]
pub struct TargetCoverage {
    /// The verification target reference.
    pub target: String,
    /// Number of tests covering this target.
    pub test_count: usize,
}

impl EvidenceSummary {
    /// Build an evidence summary from an `ArtifactGraph`.
    #[must_use]
    pub fn from_artifact_graph(ag: &crate::artifact_graph::ArtifactGraph<'_>) -> Self {
        use std::collections::BTreeMap;
        use supersigil_evidence::PluginProvenance;

        let mut crit_tests: BTreeMap<String, usize> = BTreeMap::new();
        let records: Vec<EvidenceReportEntry> = ag
            .evidence
            .iter()
            .map(|rec| {
                let test_kind = rec.test.kind.as_str().to_string();
                let evidence_kind = rec
                    .kind()
                    .map_or_else(|| "unknown".to_string(), |k| k.as_str().to_string());
                let targets: Vec<String> = rec
                    .targets
                    .iter()
                    .map(|c| {
                        let key = c.to_string();
                        *crit_tests.entry(key.clone()).or_default() += 1;
                        key
                    })
                    .collect();
                let provenance: Vec<String> = rec
                    .provenance
                    .iter()
                    .map(|p| match p {
                        PluginProvenance::RustAttribute { .. } => "plugin:rust".to_string(),
                        PluginProvenance::VerifiedByTag { doc_id, tag } => {
                            format!("authored:tag({doc_id}:{tag})")
                        }
                        PluginProvenance::VerifiedByFileGlob { doc_id, .. } => {
                            format!("authored:glob({doc_id})")
                        }
                        PluginProvenance::JsVerifies { .. } => "plugin:js".to_string(),
                    })
                    .collect();
                EvidenceReportEntry {
                    test_name: rec.test.name.clone(),
                    test_file: rec.test.file.to_string_lossy().to_string(),
                    test_kind,
                    evidence_kind,
                    targets,
                    provenance,
                    source_file: rec.source_location.file.to_string_lossy().to_string(),
                    source_line: rec.source_location.line,
                    source_column: rec.source_location.column,
                }
            })
            .collect();

        let coverage: Vec<TargetCoverage> = crit_tests
            .into_iter()
            .map(|(target, test_count)| TargetCoverage { target, test_count })
            .collect();

        Self {
            records,
            coverage,
            conflict_count: ag.conflicts.len(),
        }
    }
}

// ---------------------------------------------------------------------------
// VerificationReport
// ---------------------------------------------------------------------------

/// Complete output of a verification run.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationReport {
    /// Overall pass/fail status.
    pub overall_status: ResultStatus,
    /// All findings from the verification run.
    pub findings: Vec<Finding>,
    /// Aggregate counts by severity.
    pub summary: Summary,
    /// Optional evidence summary from `ArtifactGraph` enrichment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_summary: Option<EvidenceSummary>,
}

impl VerificationReport {
    /// Creates a report, computing `overall_status` from the summary counts.
    #[must_use]
    pub fn new(
        findings: Vec<Finding>,
        summary: Summary,
        evidence_summary: Option<EvidenceSummary>,
    ) -> Self {
        let overall_status = if summary.error_count > 0 {
            ResultStatus::HasErrors
        } else if summary.warning_count > 0 {
            ResultStatus::WarningsOnly
        } else {
            ResultStatus::Clean
        };
        Self {
            overall_status,
            findings,
            summary,
            evidence_summary,
        }
    }

    /// Returns the overall result status.
    #[must_use]
    pub fn result_status(&self) -> ResultStatus {
        self.overall_status
    }
}

// ---------------------------------------------------------------------------
// Formatters
// ---------------------------------------------------------------------------

use std::fmt::Write as _;

/// Format a verification report as pretty-printed JSON.
///
/// # Panics
///
/// Panics if the report fails to serialize (should never happen for
/// well-formed `VerificationReport` values).
#[must_use]
pub fn format_json(report: &VerificationReport) -> String {
    serde_json::to_string_pretty(report).expect("report serializes")
}

/// Format a verification report as Markdown.
///
/// Includes a heading, a findings table (or a clean-status note), and a
/// summary section.
#[must_use]
pub fn format_markdown(report: &VerificationReport) -> String {
    let mut out = String::from("# Verification Report\n\n");

    if report.result_status() == ResultStatus::Clean {
        out.push_str("Status: ✔ Clean\n");
        return out;
    }

    // Findings table
    out.push_str("| Severity | Document | Rule | Message |\n");
    out.push_str("|----------|----------|------|---------|\n");

    for f in &report.findings {
        let severity = match f.effective_severity {
            ReportSeverity::Error => "error",
            ReportSeverity::Warning => "warning",
            ReportSeverity::Info => "info",
            ReportSeverity::Off => continue,
        };
        let doc = f.doc_id.as_deref().unwrap_or("—");
        let _ = writeln!(
            out,
            "| {severity} | {doc} | {} | {} |",
            f.rule.config_key(),
            f.message,
        );
    }

    // Summary
    let s = &report.summary;
    let _ = write!(
        out,
        "\n## Summary\n\n- **Documents:** {}\n- **Errors:** {}\n- **Warnings:** {}\n- **Info:** {}\n",
        s.total_documents, s.error_count, s.warning_count, s.info_count,
    );

    // Evidence (optional)
    if let Some(ref ev) = report.evidence_summary {
        out.push_str("\n## Evidence\n\n");
        out.push_str("| Test | File | Kind | Evidence | Targets | Provenance |\n");
        out.push_str("|------|------|------|----------|---------|------------|\n");
        for entry in &ev.records {
            let targets = entry.targets.join(", ");
            let prov = entry.provenance.join(", ");
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} |",
                entry.test_name,
                entry.test_file,
                entry.test_kind,
                entry.evidence_kind,
                targets,
                prov,
            );
        }
    }

    out
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use supersigil_core::KNOWN_RULES;

    use super::*;

    // -----------------------------------------------------------------------
    // config_key <-> KNOWN_RULES round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn config_keys_match_known_rules() {
        let built_in_keys: HashSet<&str> = RuleName::ALL.iter().map(|r| r.config_key()).collect();
        let known: HashSet<&str> = KNOWN_RULES.iter().copied().collect();
        assert_eq!(built_in_keys, known);
    }

    // -----------------------------------------------------------------------
    // default_severity for built-in rule variants
    // -----------------------------------------------------------------------

    #[test]
    fn default_severity_all_variants() {
        let expected = [
            (RuleName::MissingVerificationEvidence, ReportSeverity::Error),
            (RuleName::MissingTestFiles, ReportSeverity::Error),
            (RuleName::BrokenRef, ReportSeverity::Error),
            (RuleName::InvalidVerifiedByPlacement, ReportSeverity::Error),
            (RuleName::IsolatedDocument, ReportSeverity::Off),
            (RuleName::ZeroTagMatches, ReportSeverity::Warning),
            (RuleName::EmptyTrackedGlob, ReportSeverity::Warning),
            (RuleName::OrphanTestTag, ReportSeverity::Warning),
            (RuleName::InvalidIdPattern, ReportSeverity::Warning),
            (RuleName::StatusInconsistency, ReportSeverity::Warning),
            (RuleName::PluginDiscoveryFailure, ReportSeverity::Warning),
            (RuleName::PluginDiscoveryWarning, ReportSeverity::Warning),
            (RuleName::SequentialIdOrder, ReportSeverity::Warning),
            (RuleName::SequentialIdGap, ReportSeverity::Warning),
            (RuleName::InvalidRationalePlacement, ReportSeverity::Warning),
            (
                RuleName::InvalidAlternativePlacement,
                ReportSeverity::Warning,
            ),
            (RuleName::DuplicateRationale, ReportSeverity::Warning),
            (RuleName::InvalidAlternativeStatus, ReportSeverity::Warning),
            (RuleName::IncompleteDecision, ReportSeverity::Warning),
            (RuleName::OrphanDecision, ReportSeverity::Warning),
            (RuleName::MissingDecisionCoverage, ReportSeverity::Off),
            (RuleName::EmptyProject, ReportSeverity::Warning),
        ];
        for (rule, severity) in expected {
            assert_eq!(rule.default_severity(), severity, "for {rule:?}");
        }
    }

    // -----------------------------------------------------------------------
    // ReportSeverity::from(Severity)
    // -----------------------------------------------------------------------

    #[test]
    fn report_severity_from_core() {
        for (input, expected) in [
            (Severity::Off, ReportSeverity::Off),
            (Severity::Warning, ReportSeverity::Warning),
            (Severity::Error, ReportSeverity::Error),
        ] {
            assert_eq!(ReportSeverity::from(input), expected, "for {input:?}");
        }
    }

    // -----------------------------------------------------------------------
    // VerificationReport JSON serialization
    // -----------------------------------------------------------------------

    #[test]
    fn verification_report_includes_position_when_present() {
        let report = VerificationReport::new(
            vec![Finding {
                rule: RuleName::InvalidIdPattern,
                doc_id: None,
                message: "bad pattern".to_string(),
                effective_severity: ReportSeverity::Warning,
                raw_severity: ReportSeverity::Warning,
                position: Some(SourcePosition {
                    byte_offset: 42,
                    line: 3,
                    column: 1,
                }),
                details: None,
                suggestion: None,
            }],
            Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 1,
                info_count: 0,
            },
            None,
        );

        let json = serde_json::to_string(&report).expect("serialization should succeed");
        assert!(json.contains("\"position\""), "position should be present");
        assert!(json.contains("\"byte_offset\""), "missing byte_offset");
        assert!(json.contains("\"line\""), "missing line");
        assert!(json.contains("\"column\""), "missing column");
        // doc_id is None, so it should be skipped
        assert!(
            !json.contains("\"doc_id\""),
            "doc_id should be skipped when None",
        );
    }

    // -----------------------------------------------------------------------
    // result_status()
    // -----------------------------------------------------------------------

    #[test]
    fn result_status_derives_from_counts() {
        // (error_count, warning_count, info_count, expected)
        let cases = [
            (0, 0, 0, ResultStatus::Clean),
            (0, 0, 5, ResultStatus::Clean),
            (2, 1, 0, ResultStatus::HasErrors),
            (0, 4, 1, ResultStatus::WarningsOnly),
        ];
        for (errors, warnings, infos, expected) in cases {
            let report = VerificationReport::new(
                vec![],
                Summary {
                    total_documents: 3,
                    error_count: errors,
                    warning_count: warnings,
                    info_count: infos,
                },
                None,
            );
            assert_eq!(
                report.result_status(),
                expected,
                "for counts ({errors}, {warnings}, {infos})",
            );
        }
    }

    // -----------------------------------------------------------------------
    // format_json
    // -----------------------------------------------------------------------

    #[test]
    fn json_format_roundtrips() {
        let report = VerificationReport::new(
            vec![Finding {
                rule: RuleName::MissingTestFiles,
                doc_id: Some("prop/auth".to_string()),
                message: "no test files found".to_string(),
                effective_severity: ReportSeverity::Error,
                raw_severity: ReportSeverity::Error,
                position: None,
                details: None,
                suggestion: None,
            }],
            Summary {
                total_documents: 1,
                error_count: 1,
                warning_count: 0,
                info_count: 0,
            },
            None,
        );

        let json = format_json(&report);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("should parse as valid JSON");
        assert_eq!(parsed["summary"]["error_count"], 1);
        assert_eq!(parsed["findings"][0]["rule"], "missing_test_files");
        assert_eq!(parsed["findings"][0]["doc_id"], "prop/auth");
    }

    // -----------------------------------------------------------------------
    // format_markdown
    // -----------------------------------------------------------------------

    #[test]
    fn markdown_format_has_table() {
        let report = VerificationReport::new(
            vec![
                Finding {
                    rule: RuleName::MissingVerificationEvidence,
                    doc_id: Some("req/auth".to_string()),
                    message: "criterion AC-1 not covered".to_string(),
                    effective_severity: ReportSeverity::Error,
                    raw_severity: ReportSeverity::Error,
                    position: None,
                    details: None,
                    suggestion: None,
                },
                Finding {
                    rule: RuleName::ZeroTagMatches,
                    doc_id: Some("prop/auth".to_string()),
                    message: "tag 'prop:auth' has zero matches".to_string(),
                    effective_severity: ReportSeverity::Warning,
                    raw_severity: ReportSeverity::Warning,
                    position: None,
                    details: None,
                    suggestion: None,
                },
            ],
            Summary {
                total_documents: 2,
                error_count: 1,
                warning_count: 1,
                info_count: 0,
            },
            None,
        );

        let out = format_markdown(&report);
        assert!(out.contains("# Verification Report"), "should have header",);
        assert!(
            out.contains("| Severity | Document | Rule | Message |"),
            "should have table header, got: {out}",
        );
        assert!(out.contains("error"), "should contain error severity");
        assert!(
            out.contains("missing_verification_evidence"),
            "should contain rule name",
        );
        assert!(out.contains("req/auth"), "should contain doc_id");
        assert!(out.contains("## Summary"), "should have summary section");
    }

    #[test]
    fn markdown_format_clean_report() {
        let report = VerificationReport::new(
            vec![],
            Summary {
                total_documents: 3,
                error_count: 0,
                warning_count: 0,
                info_count: 0,
            },
            None,
        );

        let out = format_markdown(&report);
        assert!(
            out.contains("✔ Clean"),
            "clean report should show clean status, got: {out}",
        );
    }

    use crate::test_helpers::sample_evidence_summary;

    // -----------------------------------------------------------------------
    // evidence_summary_serializes_in_json (req-9-1)
    // -----------------------------------------------------------------------

    #[test]
    fn evidence_summary_serializes_in_json() {
        let report = VerificationReport::new(
            vec![],
            Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 0,
                info_count: 0,
            },
            Some(sample_evidence_summary()),
        );

        let json = format_json(&report);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("should parse as valid JSON");

        // The evidence_summary key should be present
        assert!(
            parsed.get("evidence_summary").is_some(),
            "JSON should contain evidence_summary key, got: {json}",
        );

        // Drill into the first record
        let records = &parsed["evidence_summary"]["records"];
        assert!(records.is_array(), "records should be an array");
        assert_eq!(records.as_array().unwrap().len(), 2);
        assert_eq!(records[0]["test_name"], "test_login_flow");
        assert_eq!(records[0]["evidence_kind"], "rust-attribute");
        assert_eq!(records[0]["provenance"][0], "plugin:rust");
    }

    // -----------------------------------------------------------------------
    // evidence_summary_absent_when_none
    // -----------------------------------------------------------------------

    #[test]
    fn evidence_summary_absent_when_none() {
        let report = VerificationReport::new(
            vec![],
            Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 0,
                info_count: 0,
            },
            None,
        );

        let json = format_json(&report);
        assert!(
            !json.contains("evidence_summary"),
            "JSON should NOT contain evidence_summary when None, got: {json}",
        );
    }

    // -----------------------------------------------------------------------
    // markdown_includes_evidence_section (req-9-2)
    // -----------------------------------------------------------------------

    #[test]
    fn markdown_includes_evidence_section() {
        let report = VerificationReport::new(
            vec![Finding {
                rule: RuleName::MissingVerificationEvidence,
                doc_id: Some("req/auth".to_string()),
                message: "criterion req-1 not covered".to_string(),
                effective_severity: ReportSeverity::Error,
                raw_severity: ReportSeverity::Error,
                position: None,
                details: None,
                suggestion: None,
            }],
            Summary {
                total_documents: 1,
                error_count: 1,
                warning_count: 0,
                info_count: 0,
            },
            Some(sample_evidence_summary()),
        );

        let out = format_markdown(&report);
        assert!(
            out.contains("## Evidence"),
            "markdown should include Evidence section when evidence_summary is present, got: {out}",
        );
        assert!(
            out.contains("test_login_flow"),
            "markdown Evidence section should list test names, got: {out}",
        );
        assert!(
            out.contains("rust-attribute"),
            "markdown Evidence section should show evidence kind, got: {out}",
        );
        assert!(
            out.contains("plugin:rust"),
            "markdown Evidence section should show provenance, got: {out}",
        );
    }

    // -----------------------------------------------------------------------
    // markdown_no_evidence_section_when_absent
    // -----------------------------------------------------------------------

    #[test]
    fn markdown_no_evidence_section_when_absent() {
        let report = VerificationReport::new(
            vec![Finding {
                rule: RuleName::MissingVerificationEvidence,
                doc_id: Some("req/auth".to_string()),
                message: "criterion req-1 not covered".to_string(),
                effective_severity: ReportSeverity::Error,
                raw_severity: ReportSeverity::Error,
                position: None,
                details: None,
                suggestion: None,
            }],
            Summary {
                total_documents: 1,
                error_count: 1,
                warning_count: 0,
                info_count: 0,
            },
            None,
        );

        let out = format_markdown(&report);
        assert!(
            !out.contains("## Evidence"),
            "markdown should NOT include Evidence section when evidence_summary is None, got: {out}",
        );
    }

    // -----------------------------------------------------------------------
    // multiple_tests_per_criterion_listed_separately (req-9-3)
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_tests_per_criterion_listed_separately() {
        let evidence = sample_evidence_summary();
        // Confirm our sample has multiple tests targeting the same criterion
        assert_eq!(evidence.coverage.len(), 1);
        assert_eq!(evidence.coverage[0].test_count, 2);

        let report = VerificationReport::new(
            vec![],
            Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 0,
                info_count: 0,
            },
            Some(evidence),
        );

        let json = format_json(&report);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("should parse as valid JSON");

        // Each test should appear as a separate record in the JSON
        let records = &parsed["evidence_summary"]["records"];
        let names: Vec<&str> = records
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["test_name"].as_str().unwrap())
            .collect();
        assert!(
            names.contains(&"test_login_flow"),
            "should list test_login_flow separately",
        );
        assert!(
            names.contains(&"test_session_timeout"),
            "should list test_session_timeout separately",
        );

        // Coverage should show the aggregate
        let coverage = &parsed["evidence_summary"]["coverage"];
        assert_eq!(coverage[0]["target"], "req-1");
        assert_eq!(coverage[0]["test_count"], 2);
    }

    #[test]
    fn finding_details_serialize_only_when_present() {
        let with_details = VerificationReport::new(
            vec![
                Finding::new(
                    RuleName::PluginDiscoveryFailure,
                    None,
                    "plugin failed".to_string(),
                    None,
                )
                .with_details(FindingDetails {
                    plugin: Some("rust".to_string()),
                    target_ref: Some("auth/req/login#happy-path-login".to_string()),
                    code: Some("invalid_verifies_attribute".to_string()),
                    suggestion: Some("Use #[verifies(\"doc#criterion\")]".to_string()),
                    ..FindingDetails::default()
                }),
            ],
            Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 1,
                info_count: 0,
            },
            None,
        );

        let with_json = format_json(&with_details);
        let with_parsed: serde_json::Value =
            serde_json::from_str(&with_json).expect("details JSON should parse");
        assert!(
            with_parsed["findings"][0].get("details").is_some(),
            "expected details in {with_json}",
        );
        assert_eq!(with_parsed["findings"][0]["details"]["plugin"], "rust");
        assert_eq!(
            with_parsed["findings"][0]["details"]["target_ref"],
            "auth/req/login#happy-path-login"
        );

        let without_details = VerificationReport::new(
            vec![Finding::new(
                RuleName::PluginDiscoveryFailure,
                None,
                "plugin failed".to_string(),
                None,
            )],
            Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 1,
                info_count: 0,
            },
            None,
        );

        let without_json = format_json(&without_details);
        assert!(
            !without_json.contains("\"details\""),
            "details should be skipped when absent: {without_json}",
        );
    }

    // -----------------------------------------------------------------------
    // Finding::with_suggestion
    // -----------------------------------------------------------------------

    #[test]
    fn with_suggestion_sets_field() {
        let finding = Finding::new(
            RuleName::BrokenRef,
            Some("tasks/auth".into()),
            "broken ref".into(),
            None,
        )
        .with_suggestion("auth/req".into());

        assert_eq!(finding.suggestion.as_deref(), Some("auth/req"));
    }

    #[test]
    fn suggestion_serializes_in_json_when_present() {
        let report = VerificationReport::new(
            vec![
                Finding::new(
                    RuleName::BrokenRef,
                    Some("tasks/auth".into()),
                    "broken ref `auth/reqs`".into(),
                    None,
                )
                .with_suggestion("auth/req".into()),
            ],
            Summary {
                total_documents: 1,
                error_count: 1,
                warning_count: 0,
                info_count: 0,
            },
            None,
        );

        let json = format_json(&report);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("should parse as valid JSON");

        assert_eq!(
            parsed["findings"][0]["suggestion"], "auth/req",
            "JSON should contain suggestion field, got: {json}",
        );
    }

    #[test]
    fn suggestion_absent_in_json_when_none() {
        let report = VerificationReport::new(
            vec![Finding::new(
                RuleName::BrokenRef,
                Some("tasks/auth".into()),
                "some finding".into(),
                None,
            )],
            Summary {
                total_documents: 1,
                error_count: 1,
                warning_count: 0,
                info_count: 0,
            },
            None,
        );

        let json = format_json(&report);
        assert!(
            !json.contains("\"suggestion\""),
            "suggestion should be absent when None, got: {json}",
        );
    }
}
