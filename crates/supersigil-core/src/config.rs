//! Configuration types and loader for `supersigil.toml`.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ConfigError;

// ---------------------------------------------------------------------------
// Severity
// ---------------------------------------------------------------------------

/// Severity level for verification rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Off,
    Warning,
    Error,
}

// ---------------------------------------------------------------------------
// AttributeDef
// ---------------------------------------------------------------------------

/// Definition of a single component attribute in config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributeDef {
    pub required: bool,
    #[serde(default)]
    pub list: bool,
}

// ---------------------------------------------------------------------------
// ComponentDef
// ---------------------------------------------------------------------------

/// Definition of a component in config, with attribute schemas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentDef {
    #[serde(default)]
    pub attributes: HashMap<String, AttributeDef>,
    #[serde(default)]
    pub referenceable: bool,
    #[serde(default)]
    pub verifiable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
}

// ---------------------------------------------------------------------------
// DocumentTypeDef
// ---------------------------------------------------------------------------

/// Definition of a document type with valid statuses and required components.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentTypeDef {
    #[serde(default)]
    pub status: Vec<String>,
    #[serde(default)]
    pub required_components: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// DocumentsConfig
// ---------------------------------------------------------------------------

/// Document type definitions section of config.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentsConfig {
    #[serde(default)]
    pub types: HashMap<String, DocumentTypeDef>,
}

// ---------------------------------------------------------------------------
// VerifyConfig
// ---------------------------------------------------------------------------

/// Verification rule severity overrides.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strictness: Option<Severity>,
    #[serde(default)]
    pub rules: HashMap<String, Severity>,
}

// ---------------------------------------------------------------------------
// EcosystemConfig
// ---------------------------------------------------------------------------

fn default_plugins() -> Vec<String> {
    vec!["rust".to_string()]
}

// ---------------------------------------------------------------------------
// RustValidationPolicy
// ---------------------------------------------------------------------------

/// Controls which Cargo manifests are validated by the Rust ecosystem plugin.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RustValidationPolicy {
    /// Skip all Cargo-based validation.
    Off,
    /// Validate only dev-dependency manifests (default).
    #[default]
    Dev,
    /// Validate every reachable Cargo.toml.
    All,
}

// ---------------------------------------------------------------------------
// RustProjectScope
// ---------------------------------------------------------------------------

/// Maps a manifest directory prefix to a named project for multi-project Rust
/// workspaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustProjectScope {
    /// Cargo.toml directories matching this prefix are assigned to `project`.
    pub manifest_dir_prefix: String,
    /// The supersigil project name this scope maps to.
    pub project: String,
}

// ---------------------------------------------------------------------------
// RustEcosystemConfig
// ---------------------------------------------------------------------------

fn default_validation_policy() -> RustValidationPolicy {
    RustValidationPolicy::Dev
}

/// Per-plugin configuration for the Rust ecosystem plugin.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustEcosystemConfig {
    /// Which manifests to validate.
    #[serde(default = "default_validation_policy")]
    pub validation: RustValidationPolicy,
    /// Optional project-scope mappings for multi-project workspaces.
    #[serde(default)]
    pub project_scope: Vec<RustProjectScope>,
}

// ---------------------------------------------------------------------------
// JsEcosystemConfig
// ---------------------------------------------------------------------------

fn default_js_test_patterns() -> Vec<String> {
    vec![
        "**/*.test.{ts,tsx,js,jsx}".to_string(),
        "**/*.spec.{ts,tsx,js,jsx}".to_string(),
    ]
}

/// Per-plugin configuration for the JS/TS ecosystem plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsEcosystemConfig {
    /// Glob patterns for discovering JS/TS test files.
    #[serde(default = "default_js_test_patterns")]
    pub test_patterns: Vec<String>,
}

impl Default for JsEcosystemConfig {
    fn default() -> Self {
        Self {
            test_patterns: default_js_test_patterns(),
        }
    }
}

/// Ecosystem plugin declarations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcosystemConfig {
    #[serde(default = "default_plugins")]
    pub plugins: Vec<String>,
    /// Per-plugin configuration for the Rust ecosystem plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust: Option<RustEcosystemConfig>,
    /// Per-plugin configuration for the JS/TS ecosystem plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub js: Option<JsEcosystemConfig>,
}

impl Default for EcosystemConfig {
    fn default() -> Self {
        Self {
            plugins: default_plugins(),
            rust: None,
            js: None,
        }
    }
}

// ---------------------------------------------------------------------------
// SkillsConfig
// ---------------------------------------------------------------------------

/// Agent skills configuration.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillsConfig {
    /// Custom path for installed skills (default: `.agents/skills/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

// ---------------------------------------------------------------------------
// TestResultsConfig
// ---------------------------------------------------------------------------

/// Test results file configuration.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestResultsConfig {
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// ProjectConfig
// ---------------------------------------------------------------------------

/// Per-project configuration in multi-project mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub paths: Vec<String>,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub isolated: bool,
}

// ---------------------------------------------------------------------------
// LSP config
// ---------------------------------------------------------------------------

/// Diagnostics depth the LSP server runs on save.
///
/// `Lint` runs parse errors and structural rules only. `Verify` adds
/// evidence discovery, tag scanning, and coverage checks.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticsTier {
    Lint,
    #[default]
    Verify,
}

/// LSP server configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LspConfig {
    /// Diagnostics tier to run on save. Default: `Verify`.
    #[serde(default)]
    pub diagnostics: DiagnosticsTier,
}

// ---------------------------------------------------------------------------
// RepositoryProvider (config-level)
// ---------------------------------------------------------------------------

/// Well-known Git hosting providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryProvider {
    GitHub,
    GitLab,
    Bitbucket,
    Gitea,
}

impl RepositoryProvider {
    /// Canonical hostname for this provider, if one exists.
    ///
    /// Returns `None` for Gitea since it has no single canonical host.
    #[must_use]
    pub fn default_host(self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some("github.com"),
            Self::GitLab => Some("gitlab.com"),
            Self::Bitbucket => Some("bitbucket.org"),
            Self::Gitea => None,
        }
    }
}

// ---------------------------------------------------------------------------
// DocumentationConfig
// ---------------------------------------------------------------------------

/// Documentation configuration, currently supporting repository metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentationConfig {
    /// Optional repository metadata for source linking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryConfig>,
}

// ---------------------------------------------------------------------------
// RepositoryConfig
// ---------------------------------------------------------------------------

/// Repository metadata for documentation source links.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryConfig {
    /// Git hosting provider.
    pub provider: RepositoryProvider,
    /// Repository path, e.g. `"owner/repo"`.
    pub repo: String,
    /// Optional custom hostname (for self-hosted instances).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Optional main branch override (defaults vary by provider).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_branch: Option<String>,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Top-level configuration deserialized from `supersigil.toml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Single-project mode: glob patterns for spec files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    /// Single-project mode: glob patterns for test files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<Vec<String>>,
    /// Multi-project mode: named project entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects: Option<HashMap<String, ProjectConfig>>,
    /// Optional regex pattern for ID validation (stored as string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_pattern: Option<String>,
    /// Document type definitions.
    #[serde(default)]
    pub documents: DocumentsConfig,
    /// User-defined component definitions (overrides only).
    #[serde(default)]
    pub components: HashMap<String, ComponentDef>,
    /// Verification rule configuration.
    #[serde(default)]
    pub verify: VerifyConfig,
    /// Ecosystem plugin declarations.
    #[serde(default)]
    pub ecosystem: EcosystemConfig,
    /// Test results configuration.
    #[serde(default)]
    pub test_results: TestResultsConfig,
    /// Agent skills configuration.
    #[serde(default)]
    pub skills: SkillsConfig,
    /// Documentation configuration (repository metadata, etc.).
    #[serde(default)]
    pub documentation: DocumentationConfig,
    /// LSP server configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lsp: Option<LspConfig>,
}

// ---------------------------------------------------------------------------
// Known built-in plugin identifiers
// ---------------------------------------------------------------------------

/// The set of known built-in ecosystem plugin identifiers.
pub const KNOWN_PLUGINS: &[&str] = &["rust", "js"];

// ---------------------------------------------------------------------------
// Known verification rule names
// ---------------------------------------------------------------------------

/// The set of known verification rule names that can appear in `[verify.rules]`.
pub const KNOWN_RULES: &[&str] = &[
    "missing_verification_evidence",
    "missing_test_files",
    "zero_tag_matches",
    "stale_tracked_files",
    "empty_tracked_glob",
    "orphan_test_tag",
    "invalid_id_pattern",
    "isolated_document",
    "status_inconsistency",
    "missing_required_component",
    "invalid_verified_by_placement",
    "plugin_discovery_failure",
    "plugin_discovery_warning",
    "sequential_id_order",
    "sequential_id_gap",
    "invalid_rationale_placement",
    "invalid_alternative_placement",
    "duplicate_rationale",
    "invalid_alternative_status",
    "incomplete_decision",
    "orphan_decision",
    "missing_decision_coverage",
    "empty_project",
];

// ---------------------------------------------------------------------------
// load_config
// ---------------------------------------------------------------------------

/// Load and validate `supersigil.toml` from the given path.
///
/// 1. Reads the file and deserializes TOML (with `deny_unknown_fields`).
/// 2. Runs post-deserialization validation:
///    - Mutual exclusivity of `paths`/`tests` vs `projects`
///    - Unknown verification rule names
///    - `id_pattern` regex compilation
/// 3. Collects all post-deserialization errors before returning.
///
/// # Errors
///
/// Returns `Vec<ConfigError>` containing all detected errors.
#[allow(
    clippy::missing_panics_doc,
    reason = "regex literals are compile-time known-valid"
)]
pub fn load_config(path: impl AsRef<Path>) -> Result<Config, Vec<ConfigError>> {
    let path = path.as_ref();
    let content = std::fs::read_to_string(path).map_err(|e| {
        vec![ConfigError::IoError {
            path: path.to_path_buf(),
            source: e,
        }]
    })?;

    let config: Config = toml::from_str(&content).map_err(|e| {
        vec![ConfigError::TomlSyntax {
            message: e.to_string(),
        }]
    })?;

    // Post-deserialization validation: collect all errors
    let mut errors = Vec::new();

    // Mutual exclusivity check
    let has_paths = config.paths.is_some();
    let has_tests = config.tests.is_some();
    let has_projects = config.projects.is_some();

    if has_paths && has_projects {
        errors.push(ConfigError::MutualExclusivity {
            keys: vec!["paths".into(), "projects".into()],
        });
    }
    if has_tests && has_projects {
        errors.push(ConfigError::MutualExclusivity {
            keys: vec!["tests".into(), "projects".into()],
        });
    }
    if !has_paths && !has_projects {
        errors.push(ConfigError::MissingRequired {
            message: "one of `paths` or `projects` is required".into(),
        });
    }

    // Unknown verification rule names
    for rule_name in config.verify.rules.keys() {
        if !KNOWN_RULES.contains(&rule_name.as_str()) {
            errors.push(ConfigError::UnknownRule {
                rule: rule_name.clone(),
            });
        }
    }

    // Unknown ecosystem plugin names
    for plugin in &config.ecosystem.plugins {
        if !KNOWN_PLUGINS.contains(&plugin.as_str()) {
            errors.push(ConfigError::UnknownPlugin {
                plugin: plugin.clone(),
            });
        }
    }

    // id_pattern regex validation
    if let Some(pattern) = &config.id_pattern
        && let Err(e) = regex::Regex::new(pattern)
    {
        errors.push(ConfigError::InvalidIdPattern {
            pattern: pattern.clone(),
            message: e.to_string(),
        });
    }

    if errors.is_empty() {
        Ok(config)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_path_deserializes() {
        let toml = r#"
paths = ["specs/**/*.md"]

[skills]
path = "custom/skills"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.skills.path.as_deref(), Some("custom/skills"));
    }

    #[test]
    fn absent_skills_section_deserializes_to_none() {
        let toml = r#"paths = ["specs/**/*.md"]"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.skills.path.is_none());
    }
}
