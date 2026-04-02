//! Verification report types: findings, summaries, and result status.

use serde::Serialize;
use supersigil_core::{Severity, SourcePosition};

pub use crate::rule_name::RuleName;

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
// RuleName (defined in crate::rule_name, re-exported here)
// ---------------------------------------------------------------------------

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
    /// When `true`, this criterion is targeted by at least one `<Example
    /// verifies="...">` component, so running examples may cover it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub example_coverable: bool,
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
                        PluginProvenance::Example { doc_id, example_id } => {
                            format!("example:{doc_id}:{example_id}")
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
    pub overall_status: ResultStatus,
    pub findings: Vec<Finding>,
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
mod tests;
