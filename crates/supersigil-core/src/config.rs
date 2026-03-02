//! Configuration types and loader for `supersigil.toml`.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ConfigError;

// ---------------------------------------------------------------------------
// Severity
// ---------------------------------------------------------------------------

/// Severity level for verification rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_component: Option<String>,
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

/// Ecosystem plugin declarations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcosystemConfig {
    #[serde(default = "default_plugins")]
    pub plugins: Vec<String>,
}

impl Default for EcosystemConfig {
    fn default() -> Self {
        Self {
            plugins: default_plugins(),
        }
    }
}

// ---------------------------------------------------------------------------
// HooksConfig
// ---------------------------------------------------------------------------

/// Default hook timeout, chosen to accommodate slow test runners while
/// failing promptly on hung processes.
const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 30;

fn default_timeout() -> u64 {
    DEFAULT_HOOK_TIMEOUT_SECS
}

/// Hook configuration for external process hooks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HooksConfig {
    #[serde(default)]
    pub post_verify: Vec<String>,
    #[serde(default)]
    pub post_lint: Vec<String>,
    #[serde(default)]
    pub export: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            post_verify: Vec::new(),
            post_lint: Vec::new(),
            export: Vec::new(),
            timeout_seconds: default_timeout(),
        }
    }
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
// Config
// ---------------------------------------------------------------------------

/// Top-level configuration deserialized from `supersigil.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Hook configuration.
    #[serde(default)]
    pub hooks: HooksConfig,
    /// Test results configuration.
    #[serde(default)]
    pub test_results: TestResultsConfig,
}

// ---------------------------------------------------------------------------
// Known verification rule names
// ---------------------------------------------------------------------------

/// The set of known verification rule names that can appear in `[verify.rules]`.
pub const KNOWN_RULES: &[&str] = &[
    "uncovered_criterion",
    "unverified_validation",
    "missing_test_files",
    "dangling_ref",
    "stale_tracked_files",
    "empty_tracked_glob",
    "orphan_test_tag",
    "invalid_id_pattern",
    "isolated_document",
    "status_inconsistency",
    "missing_required_component",
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
