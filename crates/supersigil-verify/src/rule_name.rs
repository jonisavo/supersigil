//! The `RuleName` enum identifying each built-in verification rule.

use serde::{Deserialize, Serialize};

use super::report::ReportSeverity;

/// Identifies a specific verification rule.
///
/// The built-in rules correspond 1:1 with `KNOWN_RULES` in supersigil-core.
/// `HookOutput` and `HookFailure` are synthetic rules emitted by hook
/// execution rather than config-driven checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    InvalidExpectedPlacement,
    InvalidCodeBlockCardinality,
    InvalidEnvFormat,
    HookOutput,
    HookFailure,
    PluginDiscoveryFailure,
    PluginDiscoveryWarning,
    ExampleFailed,
    SequentialIdOrder,
    SequentialIdGap,
    InvalidRationalePlacement,
    InvalidAlternativePlacement,
    DuplicateRationale,
    InvalidAlternativeStatus,
    IncompleteDecision,
    OrphanDecision,
    MissingDecisionCoverage,
    EmptyProject,
    MultipleExpectedChildren,
    InlineExampleWithoutLang,
    CodeRefConflict,
}

impl RuleName {
    /// The built-in rules (excludes hook-related synthetic rules).
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
        Self::InvalidExpectedPlacement,
        Self::InvalidCodeBlockCardinality,
        Self::InvalidEnvFormat,
        Self::ExampleFailed,
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
        Self::MultipleExpectedChildren,
        Self::InlineExampleWithoutLang,
        Self::CodeRefConflict,
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
            Self::StaleTrackedFiles => "stale_tracked_files",
            Self::EmptyTrackedGlob => "empty_tracked_glob",
            Self::OrphanTestTag => "orphan_test_tag",
            Self::InvalidIdPattern => "invalid_id_pattern",
            Self::IsolatedDocument => "isolated_document",
            Self::StatusInconsistency => "status_inconsistency",
            Self::MissingRequiredComponent => "missing_required_component",
            Self::InvalidVerifiedByPlacement => "invalid_verified_by_placement",
            Self::InvalidExpectedPlacement => "invalid_expected_placement",
            Self::InvalidCodeBlockCardinality => "invalid_code_block_cardinality",
            Self::InvalidEnvFormat => "invalid_env_format",
            Self::HookOutput => "hook_output",
            Self::HookFailure => "hook_failure",
            Self::PluginDiscoveryFailure => "plugin_discovery_failure",
            Self::PluginDiscoveryWarning => "plugin_discovery_warning",
            Self::ExampleFailed => "example_failed",
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
            Self::MultipleExpectedChildren => "multiple_expected_children",
            Self::InlineExampleWithoutLang => "inline_example_without_lang",
            Self::CodeRefConflict => "code_ref_conflict",
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
            | Self::InvalidVerifiedByPlacement
            | Self::InvalidExpectedPlacement
            | Self::InvalidCodeBlockCardinality
            | Self::InvalidEnvFormat
            | Self::ExampleFailed
            | Self::MultipleExpectedChildren
            | Self::InlineExampleWithoutLang => ReportSeverity::Error,

            Self::IsolatedDocument | Self::MissingDecisionCoverage => ReportSeverity::Off,

            Self::EmptyProject
            | Self::ZeroTagMatches
            | Self::StaleTrackedFiles
            | Self::EmptyTrackedGlob
            | Self::OrphanTestTag
            | Self::InvalidIdPattern
            | Self::StatusInconsistency
            | Self::MissingRequiredComponent
            | Self::HookOutput
            | Self::PluginDiscoveryFailure
            | Self::PluginDiscoveryWarning
            | Self::SequentialIdOrder
            | Self::SequentialIdGap
            | Self::InvalidRationalePlacement
            | Self::InvalidAlternativePlacement
            | Self::DuplicateRationale
            | Self::InvalidAlternativeStatus
            | Self::IncompleteDecision
            | Self::OrphanDecision
            | Self::CodeRefConflict => ReportSeverity::Warning,
        }
    }
}
