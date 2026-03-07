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
/// The 11 built-in rules correspond 1:1 with `KNOWN_RULES` in supersigil-core.
/// `HookOutput` and `HookFailure` are synthetic rules emitted by hook
/// execution rather than config-driven checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleName {
    UncoveredCriterion,
    UnverifiedValidation,
    MissingTestFiles,
    ZeroTagMatches,
    StaleTrackedFiles,
    EmptyTrackedGlob,
    OrphanTestTag,
    InvalidIdPattern,
    IsolatedDocument,
    StatusInconsistency,
    MissingRequiredComponent,
    HookOutput,
    HookFailure,
}

impl RuleName {
    /// The 11 built-in rules (excludes hook-related synthetic rules).
    pub const ALL: &[Self] = &[
        Self::UncoveredCriterion,
        Self::UnverifiedValidation,
        Self::MissingTestFiles,
        Self::ZeroTagMatches,
        Self::StaleTrackedFiles,
        Self::EmptyTrackedGlob,
        Self::OrphanTestTag,
        Self::InvalidIdPattern,
        Self::IsolatedDocument,
        Self::StatusInconsistency,
        Self::MissingRequiredComponent,
    ];
    /// Returns the config key string used in `[verify.rules]`.
    #[must_use]
    pub fn config_key(self) -> &'static str {
        match self {
            Self::UncoveredCriterion => "uncovered_criterion",
            Self::UnverifiedValidation => "unverified_validation",
            Self::MissingTestFiles => "missing_test_files",
            Self::ZeroTagMatches => "zero_tag_matches",
            Self::StaleTrackedFiles => "stale_tracked_files",
            Self::EmptyTrackedGlob => "empty_tracked_glob",
            Self::OrphanTestTag => "orphan_test_tag",
            Self::InvalidIdPattern => "invalid_id_pattern",
            Self::IsolatedDocument => "isolated_document",
            Self::StatusInconsistency => "status_inconsistency",
            Self::MissingRequiredComponent => "missing_required_component",
            Self::HookOutput => "hook_output",
            Self::HookFailure => "hook_failure",
        }
    }

    /// Returns the default severity for this rule when no config override
    /// is present.
    #[must_use]
    pub fn default_severity(self) -> ReportSeverity {
        match self {
            Self::UncoveredCriterion
            | Self::MissingTestFiles
            | Self::UnverifiedValidation
            | Self::HookFailure => ReportSeverity::Error,

            Self::IsolatedDocument => ReportSeverity::Off,

            Self::ZeroTagMatches
            | Self::StaleTrackedFiles
            | Self::EmptyTrackedGlob
            | Self::OrphanTestTag
            | Self::InvalidIdPattern
            | Self::StatusInconsistency
            | Self::MissingRequiredComponent
            | Self::HookOutput => ReportSeverity::Warning,
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
        }
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
// VerificationReport
// ---------------------------------------------------------------------------

/// Complete output of a verification run.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationReport {
    pub findings: Vec<Finding>,
    pub summary: Summary,
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

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Format a verification report for terminal output.
///
/// Groups findings by `doc_id` (using "global" for `None`), prefixes each
/// finding with a severity symbol (`✖`/`⚠`/`ℹ`), and appends a summary
/// line. When `use_color` is true, severity symbols are wrapped in ANSI
/// colour codes. When `use_color` is false, ASCII fallback symbols are used
/// instead of Unicode.
#[must_use]
pub fn format_terminal(report: &VerificationReport, use_color: bool) -> String {
    if report.result_status() == ResultStatus::Clean {
        return if use_color {
            "\x1b[32m✔ Clean — no findings\x1b[0m\n".to_string()
        } else {
            "[ok] Clean -- no findings\n".to_string()
        };
    }

    // Group findings by doc_id
    let mut groups: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for f in &report.findings {
        let key = f.doc_id.as_deref().unwrap_or("global");
        groups.entry(key).or_default().push(f);
    }

    let mut out = String::new();

    for (doc, findings) in &groups {
        let _ = writeln!(out, "{doc}");
        for f in findings {
            let (symbol, ansi_start, ansi_end) = if use_color {
                match f.effective_severity {
                    ReportSeverity::Error => ("✖", "\x1b[31m", "\x1b[0m"),
                    ReportSeverity::Warning => ("⚠", "\x1b[33m", "\x1b[0m"),
                    ReportSeverity::Info => ("ℹ", "\x1b[34m", "\x1b[0m"),
                    ReportSeverity::Off => continue,
                }
            } else {
                match f.effective_severity {
                    ReportSeverity::Error => ("[err]", "", ""),
                    ReportSeverity::Warning => ("[warn]", "", ""),
                    ReportSeverity::Info => ("[info]", "", ""),
                    ReportSeverity::Off => continue,
                }
            };
            let _ = writeln!(
                out,
                "  {ansi_start}{symbol}{ansi_end} [{}] {}",
                f.rule.config_key(),
                f.message,
            );
        }
    }

    let s = &report.summary;
    let _ = writeln!(
        out,
        "\n{} error(s), {} warning(s), {} info(s) across {} documents",
        s.error_count, s.warning_count, s.info_count, s.total_documents,
    );

    out
}

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
        assert_eq!(RuleName::ALL.len(), 11);
    }

    // -----------------------------------------------------------------------
    // default_severity for all 13 variants
    // -----------------------------------------------------------------------

    #[test]
    fn default_severity_all_variants() {
        let expected = [
            (RuleName::UncoveredCriterion, ReportSeverity::Error),
            (RuleName::MissingTestFiles, ReportSeverity::Error),
            (RuleName::UnverifiedValidation, ReportSeverity::Error),
            (RuleName::HookFailure, ReportSeverity::Error),
            (RuleName::IsolatedDocument, ReportSeverity::Off),
            (RuleName::ZeroTagMatches, ReportSeverity::Warning),
            (RuleName::StaleTrackedFiles, ReportSeverity::Warning),
            (RuleName::EmptyTrackedGlob, ReportSeverity::Warning),
            (RuleName::OrphanTestTag, ReportSeverity::Warning),
            (RuleName::InvalidIdPattern, ReportSeverity::Warning),
            (RuleName::StatusInconsistency, ReportSeverity::Warning),
            (RuleName::MissingRequiredComponent, ReportSeverity::Warning),
            (RuleName::HookOutput, ReportSeverity::Warning),
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
            }],
            summary: Summary {
                total_documents: 1,
                error_count: 0,
                warning_count: 1,
                info_count: 0,
            },
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
            };
            assert_eq!(
                report.result_status(),
                expected,
                "for counts ({errors}, {warnings}, {infos})",
            );
        }
    }

    // -----------------------------------------------------------------------
    // format_terminal
    // -----------------------------------------------------------------------

    #[test]
    fn terminal_format_groups_by_document() {
        let report = VerificationReport {
            findings: vec![
                Finding {
                    rule: RuleName::UncoveredCriterion,
                    doc_id: Some("req/auth".to_string()),
                    message: "criterion AC-1 not covered".to_string(),
                    effective_severity: ReportSeverity::Error,
                    raw_severity: ReportSeverity::Error,
                    position: None,
                },
                Finding {
                    rule: RuleName::OrphanTestTag,
                    doc_id: None,
                    message: "tag 'foo:bar' has no matching document".to_string(),
                    effective_severity: ReportSeverity::Warning,
                    raw_severity: ReportSeverity::Warning,
                    position: None,
                },
            ],
            summary: Summary {
                total_documents: 2,
                error_count: 1,
                warning_count: 1,
                info_count: 0,
            },
        };

        // With color: Unicode symbols + ANSI
        let out = format_terminal(&report, true);
        assert!(out.contains("req/auth"), "should contain doc_id header");
        assert!(out.contains("global"), "should contain global header");
        assert!(out.contains("✖"), "should contain error symbol");
        assert!(out.contains("⚠"), "should contain warning symbol");
        assert!(
            out.contains("[uncovered_criterion]"),
            "should contain rule name",
        );
        assert!(
            out.contains("1 error(s), 1 warning(s), 0 info(s) across 2 documents"),
            "should contain summary line, got: {out}",
        );

        // Without color: ASCII symbols, no Unicode
        let out_plain = format_terminal(&report, false);
        assert!(
            out_plain.contains("[err]"),
            "no-color should use ASCII error, got: {out_plain}",
        );
        assert!(
            out_plain.contains("[warn]"),
            "no-color should use ASCII warning, got: {out_plain}",
        );
        assert!(
            !out_plain.contains('✖') && !out_plain.contains('⚠'),
            "no-color should not contain Unicode symbols, got: {out_plain}",
        );
    }

    #[test]
    fn terminal_format_clean_report() {
        let report = VerificationReport {
            findings: vec![],
            summary: Summary {
                total_documents: 3,
                error_count: 0,
                warning_count: 0,
                info_count: 0,
            },
        };

        let out_color = format_terminal(&report, true);
        assert!(
            out_color.contains("✔ Clean"),
            "colored clean report should show Unicode, got: {out_color}",
        );

        let out_plain = format_terminal(&report, false);
        assert!(
            out_plain.contains("[ok] Clean"),
            "plain clean report should show ASCII, got: {out_plain}",
        );
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
            }],
            summary: Summary {
                total_documents: 1,
                error_count: 1,
                warning_count: 0,
                info_count: 0,
            },
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
                    rule: RuleName::UncoveredCriterion,
                    doc_id: Some("req/auth".to_string()),
                    message: "criterion AC-1 not covered".to_string(),
                    effective_severity: ReportSeverity::Error,
                    raw_severity: ReportSeverity::Error,
                    position: None,
                },
                Finding {
                    rule: RuleName::ZeroTagMatches,
                    doc_id: Some("prop/auth".to_string()),
                    message: "tag 'prop:auth' has zero matches".to_string(),
                    effective_severity: ReportSeverity::Warning,
                    raw_severity: ReportSeverity::Warning,
                    position: None,
                },
            ],
            summary: Summary {
                total_documents: 2,
                error_count: 1,
                warning_count: 1,
                info_count: 0,
            },
        };

        let out = format_markdown(&report);
        assert!(out.contains("# Verification Report"), "should have header",);
        assert!(
            out.contains("| Severity | Document | Rule | Message |"),
            "should have table header, got: {out}",
        );
        assert!(out.contains("error"), "should contain error severity");
        assert!(
            out.contains("uncovered_criterion"),
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
        };

        let out = format_markdown(&report);
        assert!(
            out.contains("✔ Clean"),
            "clean report should show clean status, got: {out}",
        );
    }
}
