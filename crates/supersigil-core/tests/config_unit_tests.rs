// Unit tests for Config types and deserialization
// Task 3.1: TDD — tests written before implementation
// Requirements: 11.1, 11.3, 12.1, 12.2, 12.6, 12.7, 13.1-13.4, 14.1-14.3,
//               15.1-15.2, 16.1-16.3, 17.1-17.4, 18.1-18.2, 19.1-19.5, 24.1

use serde::Deserialize;
use supersigil_core::{
    Config, EcosystemConfig, ExamplesConfig, HooksConfig, Severity, VerifyConfig,
};

// ---------------------------------------------------------------------------
// Minimal config (Req 24)
// ---------------------------------------------------------------------------

#[test]
fn minimal_config_paths_only() {
    let toml_str = r#"paths = ["specs/**/*.mdx"]"#;
    let config: Config = toml::from_str(toml_str).unwrap();

    assert_eq!(config.paths, Some(vec!["specs/**/*.mdx".to_string()]));
    assert_eq!(config.tests, None);
    assert_eq!(config.projects, None);
    assert_eq!(config.id_pattern, None);
    assert!(config.documents.types.is_empty());
    assert!(config.components.is_empty());
    assert_eq!(config.verify, VerifyConfig::default());
    assert_eq!(config.ecosystem.plugins, vec!["rust".to_string()]);
    assert_eq!(config.hooks.timeout_seconds, 30);
    assert!(config.hooks.post_verify.is_empty());
    assert!(config.hooks.post_lint.is_empty());
    assert!(config.hooks.export.is_empty());
    assert!(config.test_results.formats.is_empty());
    assert!(config.test_results.paths.is_empty());
}

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_defaults_to_rust_plugin() {
    let eco = EcosystemConfig::default();
    assert_eq!(eco.plugins, vec!["rust".to_string()]);
}

#[test]
fn hooks_defaults() {
    let hooks = HooksConfig::default();
    assert_eq!(hooks.timeout_seconds, 30);
    assert!(hooks.post_verify.is_empty());
    assert!(hooks.post_lint.is_empty());
    assert!(hooks.export.is_empty());
}

#[test]
fn severity_deserializes_known_values() {
    #[derive(Debug, Deserialize)]
    struct SeverityWrapper {
        s: Severity,
    }

    let cases = [
        ("off", Severity::Off),
        ("warning", Severity::Warning),
        ("error", Severity::Error),
    ];

    for (raw, expected) in cases {
        let parsed: SeverityWrapper = toml::from_str(&format!(r#"s = "{raw}""#)).unwrap();
        assert_eq!(parsed.s, expected, "failed to deserialize {raw}");
    }
}

#[test]
fn severity_deserialize_invalid_rejected() {
    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "field exists only for deserialization testing")]
    struct W {
        s: Severity,
    }
    toml::from_str::<W>(r#"s = "fatal""#).unwrap_err();
}

// ---------------------------------------------------------------------------
// deny_unknown_fields rejects unknown keys at top level (Req 11.3)
// ---------------------------------------------------------------------------

#[test]
fn unknown_top_level_key_rejected() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]
unknown_key = "oops"
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("unknown"),
        "error should mention unknown field: {err_msg}"
    );
}

// ---------------------------------------------------------------------------
// Single-project config (Req 12.1, 12.6, 12.7)
// ---------------------------------------------------------------------------

#[test]
fn single_project_with_paths_and_tests() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]
tests = ["tests/**/*.rs"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.paths, Some(vec!["specs/**/*.mdx".to_string()]));
    assert_eq!(config.tests, Some(vec!["tests/**/*.rs".to_string()]));
    assert_eq!(config.projects, None);
}

// ---------------------------------------------------------------------------
// Multi-project config (Req 12.2, 19.1-19.5)
// ---------------------------------------------------------------------------

#[test]
fn multi_project_config() {
    let toml_str = r#"
[projects.frontend]
paths = ["frontend/specs/**/*.mdx"]
tests = ["frontend/tests/**/*.rs"]
isolated = true

[projects.backend]
paths = ["backend/specs/**/*.mdx"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.paths, None);
    assert_eq!(config.tests, None);

    let projects = config.projects.unwrap();
    assert_eq!(projects.len(), 2);

    let fe = &projects["frontend"];
    assert_eq!(fe.paths, vec!["frontend/specs/**/*.mdx".to_string()]);
    assert_eq!(fe.tests, vec!["frontend/tests/**/*.rs".to_string()]);
    assert!(fe.isolated);

    let be = &projects["backend"];
    assert_eq!(be.paths, vec!["backend/specs/**/*.mdx".to_string()]);
    assert!(be.tests.is_empty()); // defaults to empty
    assert!(!be.isolated); // defaults to false
}

// ---------------------------------------------------------------------------
// ProjectConfig missing paths → serde error (Req 19.2)
// ---------------------------------------------------------------------------

#[test]
fn project_config_missing_paths_error() {
    let toml_str = r#"
[projects.broken]
tests = ["tests/**/*.rs"]
"#;
    toml::from_str::<Config>(toml_str).unwrap_err();
}

// ---------------------------------------------------------------------------
// Document type definitions (Req 13.1-13.4)
// ---------------------------------------------------------------------------

#[test]
fn document_type_with_status_and_required_components() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[documents.types.requirements]
status = ["draft", "approved", "deprecated"]
required_components = ["AcceptanceCriteria"]

[documents.types.design]
status = ["draft", "final"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let types = &config.documents.types;
    assert_eq!(types.len(), 2);

    let req_type = &types["requirements"];
    assert_eq!(
        req_type.status,
        vec!["draft", "approved", "deprecated"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        req_type.required_components,
        vec!["AcceptanceCriteria".to_string()]
    );

    let design_type = &types["design"];
    assert_eq!(
        design_type.status,
        vec!["draft", "final"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
    assert!(design_type.required_components.is_empty());
}

#[test]
fn no_document_types_defaults_to_empty() {
    let toml_str = r#"paths = ["specs/**/*.mdx"]"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.documents.types.is_empty());
}

// ---------------------------------------------------------------------------
// Component definitions (Req 14.1-14.3)
// ---------------------------------------------------------------------------

#[test]
fn component_def_with_attributes() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[components.Validates.attributes.refs]
required = true
list = true

[components.Validates.attributes.note]
required = false
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let validates = &config.components["Validates"];

    let refs_attr = &validates.attributes["refs"];
    assert!(refs_attr.required);
    assert!(refs_attr.list);

    let note_attr = &validates.attributes["note"];
    assert!(!note_attr.required);
    assert!(!note_attr.list); // default
}

#[test]
fn component_def_referenceable_and_target() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[components.Criterion]
referenceable = true

[components.Criterion.attributes.id]
required = true

[components.Validates]
target_component = "Criterion"

[components.Validates.attributes.refs]
required = true
list = true
"#;
    let config: Config = toml::from_str(toml_str).unwrap();

    let criterion = &config.components["Criterion"];
    assert!(criterion.referenceable);
    assert_eq!(criterion.target_component, None);

    let validates = &config.components["Validates"];
    assert!(!validates.referenceable);
    assert_eq!(validates.target_component, Some("Criterion".to_string()));
}

// ---------------------------------------------------------------------------
// Verification config (Req 15.1, 15.2)
// ---------------------------------------------------------------------------

#[test]
fn verify_config_with_strictness_and_rules() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[verify]
strictness = "warning"

[verify.rules]
missing_ref = "error"
unused_criterion = "off"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.verify.strictness, Some(Severity::Warning));
    assert_eq!(config.verify.rules["missing_ref"], Severity::Error);
    assert_eq!(config.verify.rules["unused_criterion"], Severity::Off);
}

#[test]
fn verify_config_defaults_when_absent() {
    let toml_str = r#"paths = ["specs/**/*.mdx"]"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.verify.strictness, None);
    assert!(config.verify.rules.is_empty());
}

// ---------------------------------------------------------------------------
// Hooks config (Req 17.1-17.4)
// ---------------------------------------------------------------------------

#[test]
fn hooks_with_timeout() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[hooks]
post_verify = ["cargo test"]
post_lint = ["cargo clippy"]
export = ["cargo doc"]
timeout_seconds = 60
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.hooks.post_verify, vec!["cargo test".to_string()]);
    assert_eq!(config.hooks.post_lint, vec!["cargo clippy".to_string()]);
    assert_eq!(config.hooks.export, vec!["cargo doc".to_string()]);
    assert_eq!(config.hooks.timeout_seconds, 60);
}

#[test]
fn hooks_without_timeout_defaults_to_30() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[hooks]
post_verify = ["cargo test"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.hooks.timeout_seconds, 30);
    assert_eq!(config.hooks.post_verify, vec!["cargo test".to_string()]);
    assert!(config.hooks.post_lint.is_empty());
    assert!(config.hooks.export.is_empty());
}

#[test]
fn no_hooks_section_uses_defaults() {
    let toml_str = r#"paths = ["specs/**/*.mdx"]"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.hooks, HooksConfig::default());
}

// ---------------------------------------------------------------------------
// Test results config (Req 18.1, 18.2)
// ---------------------------------------------------------------------------

#[test]
fn test_results_config() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[test_results]
formats = ["junit", "tap"]
paths = ["target/test-results/**/*.xml"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(
        config.test_results.formats,
        vec!["junit".to_string(), "tap".to_string()]
    );
    assert_eq!(
        config.test_results.paths,
        vec!["target/test-results/**/*.xml".to_string()]
    );
}

#[test]
fn no_test_results_defaults_to_empty() {
    let toml_str = r#"paths = ["specs/**/*.mdx"]"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.test_results.formats.is_empty());
    assert!(config.test_results.paths.is_empty());
}

// ---------------------------------------------------------------------------
// Ecosystem config (Req 16.1-16.3)
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_with_explicit_plugins() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[ecosystem]
plugins = ["rust", "python"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(
        config.ecosystem.plugins,
        vec!["rust".to_string(), "python".to_string()]
    );
}

#[test]
fn ecosystem_explicit_empty_plugins() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[ecosystem]
plugins = []
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.ecosystem.plugins.is_empty());
}

#[test]
fn ecosystem_absent_defaults_to_rust() {
    let toml_str = r#"paths = ["specs/**/*.mdx"]"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.ecosystem.plugins, vec!["rust".to_string()]);
}

// ---------------------------------------------------------------------------
// ID pattern (Req 20)
// ---------------------------------------------------------------------------

#[test]
fn id_pattern_stored_as_string() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]
id_pattern = "^[a-z][a-z0-9-/]+$"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.id_pattern, Some("^[a-z][a-z0-9-/]+$".to_string()));
}

// ---------------------------------------------------------------------------
// Round-trip serialization sanity check
// ---------------------------------------------------------------------------

#[test]
fn config_toml_round_trip_basic() {
    let original = Config {
        paths: Some(vec!["specs/**/*.mdx".to_string()]),
        tests: Some(vec!["tests/**/*.rs".to_string()]),
        ..Config::default()
    };
    let toml_str = toml::to_string(&original).unwrap();
    let deserialized: Config = toml::from_str(&toml_str).unwrap();
    assert_eq!(original, deserialized);
}

// ---------------------------------------------------------------------------
// deny_unknown_fields at nested levels
// ---------------------------------------------------------------------------

#[test]
fn unknown_key_in_hooks_rejected() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[hooks]
post_verify = ["cargo test"]
unknown_hook_field = true
"#;
    toml::from_str::<Config>(toml_str).unwrap_err();
}

#[test]
fn unknown_key_in_ecosystem_rejected() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[ecosystem]
plugins = ["rust"]
extra = "nope"
"#;
    toml::from_str::<Config>(toml_str).unwrap_err();
}

#[test]
fn unknown_key_in_project_rejected() {
    let toml_str = r#"
[projects.myproj]
paths = ["specs/**/*.mdx"]
unknown = true
"#;
    toml::from_str::<Config>(toml_str).unwrap_err();
}

#[test]
fn unknown_key_in_component_def_rejected() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[components.Foo]
unknown_field = "bad"
"#;
    toml::from_str::<Config>(toml_str).unwrap_err();
}

#[test]
fn unknown_key_in_verify_rejected() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[verify]
strictness = "warning"
unknown = "bad"
"#;
    toml::from_str::<Config>(toml_str).unwrap_err();
}

// ===========================================================================
// Task 5.1: Unit tests for `load_config` validation (TDD)
// Requirements: 12.3, 12.4, 12.5, 15.3, 15.4, 20.1, 20.2, 20.3, 11.2
// ===========================================================================

use std::path::Path;
use supersigil_core::{ConfigError, RustEcosystemConfig, RustValidationPolicy, load_config};

mod common;
use common::write_temp_toml;

// ---------------------------------------------------------------------------
// Mutual exclusivity (Req 12.3, 12.4, 12.5)
// ---------------------------------------------------------------------------

#[test]
fn load_config_paths_and_projects_mutual_exclusivity() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[projects.frontend]
paths = ["frontend/**/*.mdx"]
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(e, ConfigError::MutualExclusivity { keys } if keys.contains(&"paths".to_string()) && keys.contains(&"projects".to_string()))),
        "expected MutualExclusivity error for paths+projects, got: {errs:?}"
    );
}

#[test]
fn load_config_tests_and_projects_mutual_exclusivity() {
    let path = write_temp_toml(
        r#"
tests = ["tests/**/*.rs"]

[projects.frontend]
paths = ["frontend/**/*.mdx"]
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(e, ConfigError::MutualExclusivity { keys } if keys.contains(&"tests".to_string()) && keys.contains(&"projects".to_string()))),
        "expected MutualExclusivity error for tests+projects, got: {errs:?}"
    );
}

#[test]
fn load_config_neither_paths_nor_projects_error() {
    let path = write_temp_toml(
        r#"
[verify]
strictness = "warning"
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::MissingRequired { .. })),
        "expected MissingRequired error when neither paths nor projects, got: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// Unknown verification rule names (Req 15.3)
// ---------------------------------------------------------------------------

#[test]
fn load_config_unknown_rule_name_error() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[verify.rules]
nonexistent_rule = "warning"
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::UnknownRule { rule } if rule == "nonexistent_rule")),
        "expected UnknownRule error, got: {errs:?}"
    );
}

#[test]
fn load_config_known_rule_names_accepted() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[verify.rules]
missing_verification_evidence = "warning"
zero_tag_matches = "error"
stale_tracked_files = "off"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert_eq!(config.verify.rules.len(), 3);
}

// ---------------------------------------------------------------------------
// Invalid severity values (Req 15.4)
// ---------------------------------------------------------------------------

#[test]
fn load_config_invalid_severity_toml_error() {
    // Invalid severity is caught by serde during deserialization, producing a TomlSyntax error
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[verify.rules]
zero_tag_matches = "fatal"
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::TomlSyntax { .. })),
        "expected TomlSyntax error for invalid severity, got: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// id_pattern regex validation (Req 20.1, 20.2, 20.3)
// ---------------------------------------------------------------------------

#[test]
fn load_config_valid_id_pattern_accepted() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]
id_pattern = "^[a-z][a-z0-9-/]+$"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert_eq!(config.id_pattern, Some("^[a-z][a-z0-9-/]+$".to_string()));
}

#[test]
fn load_config_invalid_id_pattern_error() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]
id_pattern = "[invalid(regex"
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(e, ConfigError::InvalidIdPattern { pattern, .. } if pattern == "[invalid(regex")),
        "expected InvalidIdPattern error, got: {errs:?}"
    );
}

#[test]
fn load_config_no_id_pattern_no_validation() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert_eq!(config.id_pattern, None);
}

// ---------------------------------------------------------------------------
// TOML syntax error (Req 11.2)
// ---------------------------------------------------------------------------

#[test]
fn load_config_toml_syntax_error() {
    let path = write_temp_toml("this is not valid toml {{{{");
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::TomlSyntax { .. })),
        "expected TomlSyntax error, got: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// Multi-project missing paths → serde error (Req 19.2)
// ---------------------------------------------------------------------------

#[test]
fn load_config_multi_project_missing_paths_serde_error() {
    let path = write_temp_toml(
        r#"
[projects.broken]
tests = ["tests/**/*.rs"]
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    // serde will produce a TomlSyntax error because `paths` is required on ProjectConfig
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::TomlSyntax { .. })),
        "expected TomlSyntax error for missing paths in project, got: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// Valid config passes load_config successfully
// ---------------------------------------------------------------------------

#[test]
fn load_config_valid_single_project() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]
tests = ["tests/**/*.rs"]
id_pattern = "^[a-z]+"

[verify.rules]
zero_tag_matches = "error"
missing_verification_evidence = "warning"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert_eq!(config.paths, Some(vec!["specs/**/*.mdx".to_string()]));
    assert_eq!(config.tests, Some(vec!["tests/**/*.rs".to_string()]));
    assert_eq!(config.verify.rules.len(), 2);
}

#[test]
fn load_config_valid_multi_project() {
    let path = write_temp_toml(
        r#"
[projects.frontend]
paths = ["frontend/**/*.mdx"]
tests = ["frontend/tests/**/*.rs"]

[projects.backend]
paths = ["backend/**/*.mdx"]
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert!(config.projects.is_some());
    assert_eq!(config.projects.as_ref().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// description and examples fields on ComponentDef and DocumentTypeDef
// ---------------------------------------------------------------------------

#[test]
fn component_def_with_description_and_examples() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[components.Criterion]
referenceable = true
description = "A verifiable acceptance criterion"
examples = [
    '<Criterion id="login-ok">User sees dashboard</Criterion>',
]

[components.Criterion.attributes.id]
required = true
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let criterion = &config.components["Criterion"];
    assert_eq!(
        criterion.description,
        Some("A verifiable acceptance criterion".to_string())
    );
    assert_eq!(criterion.examples.len(), 1);
    assert!(criterion.examples[0].contains("login-ok"));
}

#[test]
fn component_def_description_and_examples_default_to_empty() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[components.Foo]
referenceable = false

[components.Foo.attributes.x]
required = true
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let foo = &config.components["Foo"];
    assert_eq!(foo.description, None);
    assert!(foo.examples.is_empty());
}

#[test]
fn document_type_def_with_description() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[documents.types.requirements]
description = "Captures what the system must do"
status = ["draft", "approved"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let req = &config.documents.types["requirements"];
    assert_eq!(
        req.description,
        Some("Captures what the system must do".to_string())
    );
}

#[test]
fn document_type_def_description_defaults_to_none() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[documents.types.design]
status = ["draft"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.documents.types["design"].description, None);
}

// ---------------------------------------------------------------------------
// Multiple errors collected
// ---------------------------------------------------------------------------

#[test]
fn load_config_collects_multiple_errors() {
    // Both paths+projects AND unknown rule → should get at least 2 errors
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]
id_pattern = "[bad(regex"

[projects.frontend]
paths = ["frontend/**/*.mdx"]

[verify.rules]
nonexistent_rule = "warning"
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    // Should have MutualExclusivity + UnknownRule + InvalidIdPattern
    assert!(
        errs.len() >= 3,
        "expected at least 3 errors, got {}: {errs:?}",
        errs.len()
    );
}

// ===========================================================================
// Task 1-3: Ecosystem plugin activation & Rust policy config tests (TDD)
// Tests for: RustValidationPolicy, RustProjectScope, RustEcosystemConfig,
//            unknown plugin validation, and project_scope resolution.
// ===========================================================================

// ---------------------------------------------------------------------------
// 1. Default ecosystem config: plugins = ["rust"], validation defaults to "dev"
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_default_rust_validation_is_dev() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert_eq!(config.ecosystem.plugins, vec!["rust".to_string()]);
    // When no [ecosystem.rust] section is present, accessing the rust config
    // (via default) should yield validation = "dev"
    let rust_config = config.ecosystem.rust.unwrap_or_default();
    assert_eq!(rust_config.validation, RustValidationPolicy::Dev);
    assert!(rust_config.project_scope.is_empty());
}

// ---------------------------------------------------------------------------
// 2. Explicit plugins = [] → no plugins enabled
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_explicit_empty_plugins_via_load_config() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[ecosystem]
plugins = []
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert!(config.ecosystem.plugins.is_empty());
}

// ---------------------------------------------------------------------------
// 3. Unknown plugin plugins = ["python"] → config error
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_unknown_plugin_rejected() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[ecosystem]
plugins = ["python"]
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::UnknownPlugin { plugin } if plugin == "python")),
        "expected UnknownPlugin error for 'python', got: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// 4. [ecosystem.rust] validation = "off" → parsed correctly
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_validation_off() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[ecosystem.rust]
validation = "off"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let rust_config = config
        .ecosystem
        .rust
        .expect("ecosystem.rust should be present");
    assert_eq!(rust_config.validation, RustValidationPolicy::Off);
}

// ---------------------------------------------------------------------------
// 5. [ecosystem.rust] validation = "dev" → parsed correctly
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_validation_dev() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[ecosystem.rust]
validation = "dev"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let rust_config = config
        .ecosystem
        .rust
        .expect("ecosystem.rust should be present");
    assert_eq!(rust_config.validation, RustValidationPolicy::Dev);
}

// ---------------------------------------------------------------------------
// 6. [ecosystem.rust] validation = "all" → parsed correctly
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_validation_all() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[ecosystem.rust]
validation = "all"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let rust_config = config
        .ecosystem
        .rust
        .expect("ecosystem.rust should be present");
    assert_eq!(rust_config.validation, RustValidationPolicy::All);
}

// ---------------------------------------------------------------------------
// 7. [ecosystem.rust] validation = "invalid" → config error
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_validation_invalid_rejected() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[ecosystem.rust]
validation = "invalid"
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::TomlSyntax { .. })),
        "expected TomlSyntax error for invalid validation policy, got: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// 8. [[ecosystem.rust.project_scope]] with manifest_dir_prefix and project
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_single_project_scope() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[ecosystem.rust]
validation = "dev"

[[ecosystem.rust.project_scope]]
manifest_dir_prefix = "services/api"
project = "backend"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let rust_config = config
        .ecosystem
        .rust
        .expect("ecosystem.rust should be present");
    assert_eq!(rust_config.project_scope.len(), 1);
    assert_eq!(
        rust_config.project_scope[0].manifest_dir_prefix,
        "services/api"
    );
    assert_eq!(rust_config.project_scope[0].project, "backend");
}

// ---------------------------------------------------------------------------
// 9. Multiple project scopes → parsed correctly
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_multiple_project_scopes() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[ecosystem.rust]
validation = "all"

[[ecosystem.rust.project_scope]]
manifest_dir_prefix = "services/api"
project = "backend"

[[ecosystem.rust.project_scope]]
manifest_dir_prefix = "crates/ui"
project = "frontend"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    let rust_config = config
        .ecosystem
        .rust
        .expect("ecosystem.rust should be present");
    assert_eq!(rust_config.project_scope.len(), 2);
    assert_eq!(
        rust_config.project_scope[0].manifest_dir_prefix,
        "services/api"
    );
    assert_eq!(rust_config.project_scope[0].project, "backend");
    assert_eq!(
        rust_config.project_scope[1].manifest_dir_prefix,
        "crates/ui"
    );
    assert_eq!(rust_config.project_scope[1].project, "frontend");
}

// ---------------------------------------------------------------------------
// 10. Missing `project` field in project_scope → config error
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_project_scope_missing_project_rejected() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[ecosystem.rust]
validation = "dev"

[[ecosystem.rust.project_scope]]
manifest_dir_prefix = "services/api"
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ConfigError::TomlSyntax { .. })),
        "expected TomlSyntax error for missing project field, got: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// Supplementary: RustEcosystemConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn rust_ecosystem_config_default_values() {
    let default_config = RustEcosystemConfig::default();
    assert_eq!(default_config.validation, RustValidationPolicy::Dev);
    assert!(default_config.project_scope.is_empty());
}

// ===========================================================================
// Task 2: ExamplesConfig and RunnerConfig tests
// ===========================================================================

// ---------------------------------------------------------------------------
// 1. Default values: timeout=30, parallelism=available/2 (min 1), empty runners
// ---------------------------------------------------------------------------

#[test]
fn examples_config_defaults() {
    let config = ExamplesConfig::default();
    assert_eq!(config.timeout, 30);

    let expected_parallelism = std::thread::available_parallelism()
        .map(|n| n.get() / 2)
        .unwrap_or(1)
        .max(1);
    assert_eq!(config.parallelism, expected_parallelism);
    assert!(config.runners.is_empty());
}

#[test]
fn examples_config_default_parallelism_at_least_one() {
    // Regardless of CPU count, parallelism must be >= 1.
    let config = ExamplesConfig::default();
    assert!(config.parallelism >= 1);
}

// ---------------------------------------------------------------------------
// 2. Custom values via TOML parsing
// ---------------------------------------------------------------------------

#[test]
fn examples_config_custom_values() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[examples]
timeout = 60
parallelism = 4

[examples.runners.python]
command = "python3 {file}"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.examples.timeout, 60);
    assert_eq!(config.examples.parallelism, 4);
    assert_eq!(config.examples.runners.len(), 1);
    assert_eq!(config.examples.runners["python"].command, "python3 {file}");
}

// ---------------------------------------------------------------------------
// 3. Invalid placeholder produces ConfigError
// ---------------------------------------------------------------------------

#[test]
fn examples_config_invalid_placeholder() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[examples.runners.python]
command = "python3 {invalid}"
"#,
    );
    let errs = load_config(Path::new(&path)).unwrap_err();
    assert!(
        errs.iter().any(
            |e| matches!(e, ConfigError::InvalidRunnerPlaceholder { runner, placeholder } if runner == "python" && placeholder == "{invalid}")
        ),
        "expected InvalidRunnerPlaceholder error, got: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// 4. Valid placeholders accepted
// ---------------------------------------------------------------------------

#[test]
fn examples_config_valid_placeholders() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.mdx"]

[examples.runners.python]
command = "python3 {file} --dir {dir} --lang {lang} --name {name}"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert_eq!(config.examples.runners.len(), 1);
    assert_eq!(
        config.examples.runners["python"].command,
        "python3 {file} --dir {dir} --lang {lang} --name {name}"
    );
}

// ---------------------------------------------------------------------------
// Ecosystem.rust with validation and project_scope (ecosystem-plugins/req#req-1-4)
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_deserialization_with_validation_and_project_scope() {
    let toml_str = r#"
paths = ["specs/**/*.mdx"]

[ecosystem.rust]
validation = "dev"

[[ecosystem.rust.project_scope]]
manifest_dir_prefix = "crates/api"
project = "backend"

[[ecosystem.rust.project_scope]]
manifest_dir_prefix = "crates/web"
project = "frontend"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();

    let rust = config
        .ecosystem
        .rust
        .expect("ecosystem.rust should be present");
    assert_eq!(rust.validation, RustValidationPolicy::Dev);
    assert_eq!(rust.project_scope.len(), 2);
    assert_eq!(rust.project_scope[0].manifest_dir_prefix, "crates/api");
    assert_eq!(rust.project_scope[0].project, "backend");
    assert_eq!(rust.project_scope[1].manifest_dir_prefix, "crates/web");
    assert_eq!(rust.project_scope[1].project, "frontend");
}
