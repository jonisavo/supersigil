//! Verification report types: findings, summaries, and result status.

use serde::Serialize;
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
    Off,
    Info,
    Warning,
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
/// The 13 built-in rules correspond 1:1 with `KNOWN_RULES` in supersigil-core.
/// `HookOutput` and `HookFailure` are synthetic rules emitted by hook
/// execution rather than config-driven checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleName {
    MissingVerificationEvidence,
    MissingTestFiles,
    ZeroTagMatches,
    StaleTrackedFiles,
    EmptyTrackedGlob,
    OrphanTestTag,
    InvalidIdPattern,
    IsolatedDocument,
    StatusInconsistency,
    MissingRequiredComponent,
    InvalidVerifiedByPlacement,
    HookOutput,
    HookFailure,
    PluginDiscoveryFailure,
    PluginDiscoveryWarning,
}

impl RuleName {
    /// The 13 built-in rules (excludes hook-related synthetic rules).
    pub const ALL: &[Self] = &[
        Self::MissingVerificationEvidence,
        Self::MissingTestFiles,
        Self::ZeroTagMatches,
        Self::StaleTrackedFiles,
        Self::EmptyTrackedGlob,
        Self::OrphanTestTag,
        Self::InvalidIdPattern,
        Self::IsolatedDocument,
        Self::StatusInconsistency,
        Self::MissingRequiredComponent,
        Self::InvalidVerifiedByPlacement,
        Self::PluginDiscoveryFailure,
        Self::PluginDiscoveryWarning,
    ];
    /// Returns the config key string used in `[verify.rules]`.
    #[must_use]
    pub fn config_key(self) -> &'static str {
        match self {
            Self::MissingVerificationEvidence => "missing_verification_evidence",
            Self::MissingTestFiles => "missing_test_files",
            Self::ZeroTagMatches => "zero_tag_matches",
            Self::StaleTrackedFiles => "stale_tracked_files",
            Self::EmptyTrackedGlob => "empty_tracked_glob",
            Self::OrphanTestTag => "orphan_test_tag",
            Self::InvalidIdPattern => "invalid_id_pattern",
            Self::IsolatedDocument => "isolated_document",
            Self::StatusInconsistency => "status_inconsistency",
            Self::MissingRequiredComponent => "missing_required_component",
            Self::InvalidVerifiedByPlacement => "invalid_verified_by_placement",
            Self::HookOutput => "hook_output",
            Self::HookFailure => "hook_failure",
            Self::PluginDiscoveryFailure => "plugin_discovery_failure",
            Self::PluginDiscoveryWarning => "plugin_discovery_warning",
        }
    }

    /// Returns the default severity for this rule when no config override
    /// is present.
    #[must_use]
    pub fn default_severity(self) -> ReportSeverity {
        match self {
            Self::MissingVerificationEvidence
            | Self::MissingTestFiles
            | Self::HookFailure
            | Self::InvalidVerifiedByPlacement => ReportSeverity::Error,

            Self::IsolatedDocument => ReportSeverity::Off,

            Self::ZeroTagMatches
            | Self::StaleTrackedFiles
            | Self::EmptyTrackedGlob
            | Self::OrphanTestTag
            | Self::InvalidIdPattern
            | Self::StatusInconsistency
            | Self::MissingRequiredComponent
            | Self::HookOutput
            | Self::PluginDiscoveryFailure
            | Self::PluginDiscoveryWarning => ReportSeverity::Warning,
        }
    }
}

// ---------------------------------------------------------------------------
// Finding
// ---------------------------------------------------------------------------

/// A single verification finding produced by a rule.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: RuleName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_id: Option<String>,
    pub message: String,
    pub effective_severity: ReportSeverity,
    pub raw_severity: ReportSeverity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<SourcePosition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Box<FindingDetails>>,
}

/// Structured metadata for programmatic remediation and source attribution.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FindingDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
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
        }
    }

    /// Attach structured details to a finding without changing its headline text.
    #[must_use]
    pub fn with_details(mut self, details: FindingDetails) -> Self {
        self.details = Some(Box::new(details));
        self
    }
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

/// Aggregate counts for a verification run.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub total_documents: usize,
    pub error_count: usize,
    pub warning_count: usize,
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
    Clean,
    HasErrors,
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
    pub test_name: String,
    pub test_file: String,
    pub test_kind: String,
    pub evidence_kind: String,
    pub targets: Vec<String>,
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
    pub target: String,
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
                let evidence_kind = rec.evidence_kind.as_str().to_string();
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
    pub findings: Vec<Finding>,
    pub summary: Summary,
    /// Optional evidence summary from `ArtifactGraph` enrichment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_summary: Option<EvidenceSummary>,
}

impl VerificationReport {
    /// Derives the overall result status from the summary counts.
    #[must_use]
    pub fn result_status(&self) -> ResultStatus {
        if self.summary.error_count > 0 {
            ResultStatus::HasErrors
        } else if self.summary.warning_count > 0 {
            ResultStatus::WarningsOnly
        } else {
            ResultStatus::Clean
        }
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
        assert_eq!(RuleName::ALL.len(), 13);
    }

    // -----------------------------------------------------------------------
    // default_severity for all 13 variants
    // -----------------------------------------------------------------------

    #[test]
    fn default_severity_all_variants() {
        let expected = [
            (RuleName::MissingVerificationEvidence, ReportSeverity::Error),
            (RuleName::MissingTestFiles, ReportSeverity::Error),
            (RuleName::HookFailure, ReportSeverity::Error),
            (RuleName::InvalidVerifiedByPlacement, ReportSeverity::Error),
            (RuleName::IsolatedDocument, ReportSeverity::Off),
            (RuleName::ZeroTagMatches, ReportSeverity::Warning),
            (RuleName::StaleTrackedFiles, ReportSeverity::Warning),
            (RuleName::EmptyTrackedGlob, ReportSeverity::Warning),
            (RuleName::OrphanTestTag, ReportSeverity::Warning),
            (RuleName::InvalidIdPattern, ReportSeverity::Warning),
            (RuleName::StatusInconsistency, ReportSeverity::Warning),
            (RuleName::MissingRequiredComponent, ReportSeverity::Warning),
            (RuleName::HookOutput, ReportSeverity::Warning),
            (RuleName::PluginDiscoveryFailure, ReportSeverity::Warning),
            (RuleName::PluginDiscoveryWarning, ReportSeverity::Warning),
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
        let report = VerificationReport {
            findings: vec![Finding {
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
            }],
            summary: Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 1,
                info_count: 0,
            },
            evidence_summary: None,
        };

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
            let report = VerificationReport {
                findings: vec![],
                summary: Summary {
                    total_documents: 3,
                    error_count: errors,
                    warning_count: warnings,
                    info_count: infos,
                },
                evidence_summary: None,
            };
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
        let report = VerificationReport {
            findings: vec![Finding {
                rule: RuleName::MissingTestFiles,
                doc_id: Some("prop/auth".to_string()),
                message: "no test files found".to_string(),
                effective_severity: ReportSeverity::Error,
                raw_severity: ReportSeverity::Error,
                position: None,
                details: None,
            }],
            summary: Summary {
                total_documents: 1,
                error_count: 1,
                warning_count: 0,
                info_count: 0,
            },
            evidence_summary: None,
        };

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
        let report = VerificationReport {
            findings: vec![
                Finding {
                    rule: RuleName::MissingVerificationEvidence,
                    doc_id: Some("req/auth".to_string()),
                    message: "criterion AC-1 not covered".to_string(),
                    effective_severity: ReportSeverity::Error,
                    raw_severity: ReportSeverity::Error,
                    position: None,
                    details: None,
                },
                Finding {
                    rule: RuleName::ZeroTagMatches,
                    doc_id: Some("prop/auth".to_string()),
                    message: "tag 'prop:auth' has zero matches".to_string(),
                    effective_severity: ReportSeverity::Warning,
                    raw_severity: ReportSeverity::Warning,
                    position: None,
                    details: None,
                },
            ],
            summary: Summary {
                total_documents: 2,
                error_count: 1,
                warning_count: 1,
                info_count: 0,
            },
            evidence_summary: None,
        };

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
        let report = VerificationReport {
            findings: vec![],
            summary: Summary {
                total_documents: 3,
                error_count: 0,
                warning_count: 0,
                info_count: 0,
            },
            evidence_summary: None,
        };

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
        let report = VerificationReport {
            findings: vec![],
            summary: Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 0,
                info_count: 0,
            },
            evidence_summary: Some(sample_evidence_summary()),
        };

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
        let report = VerificationReport {
            findings: vec![],
            summary: Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 0,
                info_count: 0,
            },
            evidence_summary: None,
        };

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
        let report = VerificationReport {
            findings: vec![Finding {
                rule: RuleName::MissingVerificationEvidence,
                doc_id: Some("req/auth".to_string()),
                message: "criterion req-1 not covered".to_string(),
                effective_severity: ReportSeverity::Error,
                raw_severity: ReportSeverity::Error,
                position: None,
                details: None,
            }],
            summary: Summary {
                total_documents: 1,
                error_count: 1,
                warning_count: 0,
                info_count: 0,
            },
            evidence_summary: Some(sample_evidence_summary()),
        };

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
        let report = VerificationReport {
            findings: vec![Finding {
                rule: RuleName::MissingVerificationEvidence,
                doc_id: Some("req/auth".to_string()),
                message: "criterion req-1 not covered".to_string(),
                effective_severity: ReportSeverity::Error,
                raw_severity: ReportSeverity::Error,
                position: None,
                details: None,
            }],
            summary: Summary {
                total_documents: 1,
                error_count: 1,
                warning_count: 0,
                info_count: 0,
            },
            evidence_summary: None,
        };

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

        let report = VerificationReport {
            findings: vec![],
            summary: Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 0,
                info_count: 0,
            },
            evidence_summary: Some(evidence),
        };

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
        let with_details = VerificationReport {
            findings: vec![
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
            summary: Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 1,
                info_count: 0,
            },
            evidence_summary: None,
        };

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

        let without_details = VerificationReport {
            findings: vec![Finding::new(
                RuleName::PluginDiscoveryFailure,
                None,
                "plugin failed".to_string(),
                None,
            )],
            summary: Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 1,
                info_count: 0,
            },
            evidence_summary: None,
        };

        let without_json = format_json(&without_details);
        assert!(
            !without_json.contains("\"details\""),
            "details should be skipped when absent: {without_json}",
        );
    }
}
