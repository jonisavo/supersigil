// Unit tests for Config types and deserialization
// Task 3.1: TDD — tests written before implementation
// Requirements: 11.1, 11.3, 12.1, 12.2, 12.6, 12.7, 13.1-13.4, 14.1-14.3,
//               15.1-15.2, 16.1-16.3, 17.1-17.4, 18.1-18.2, 19.1-19.5, 24.1

use std::collections::HashMap;

use serde::Deserialize;
use supersigil_core::{
    Config, DocumentsConfig, EcosystemConfig, HooksConfig, Severity, TestResultsConfig,
    VerifyConfig,
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
fn tests_defaults_to_empty() {
    let toml_str = r#"paths = ["specs/**/*.mdx"]"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    // In single-project mode without `tests`, it should be None
    assert_eq!(config.tests, None);
}

// ---------------------------------------------------------------------------
// Severity enum deserialization (Req 15.1, 15.2)
// ---------------------------------------------------------------------------

#[test]
fn severity_deserialize_off() {
    #[derive(Deserialize)]
    struct W {
        s: Severity,
    }
    let w: W = toml::from_str(r#"s = "off""#).unwrap();
    assert_eq!(w.s, Severity::Off);
}

#[test]
fn severity_deserialize_warning() {
    #[derive(Deserialize)]
    struct W {
        s: Severity,
    }
    let w: W = toml::from_str(r#"s = "warning""#).unwrap();
    assert_eq!(w.s, Severity::Warning);
}

#[test]
fn severity_deserialize_error() {
    #[derive(Deserialize)]
    struct W {
        s: Severity,
    }
    let w: W = toml::from_str(r#"s = "error""#).unwrap();
    assert_eq!(w.s, Severity::Error);
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

#[test]
fn single_project_without_tests() {
    let toml_str = r#"paths = ["specs/**/*.mdx"]"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.paths, Some(vec!["specs/**/*.mdx".to_string()]));
    assert_eq!(config.tests, None);
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
        projects: None,
        id_pattern: None,
        documents: DocumentsConfig::default(),
        components: HashMap::new(),
        verify: VerifyConfig::default(),
        ecosystem: EcosystemConfig::default(),
        hooks: HooksConfig::default(),
        test_results: TestResultsConfig::default(),
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
use supersigil_core::{ConfigError, load_config};

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
uncovered_criterion = "warning"
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
uncovered_criterion = "warning"
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
