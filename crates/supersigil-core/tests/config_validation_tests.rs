// Unit tests for `load_config` validation (TDD)
// Requirements: 12.3, 12.4, 12.5, 15.3, 15.4, 20.1, 20.2, 20.3, 11.2

use std::path::Path;
use supersigil_core::{Config, ConfigError, load_config};

mod common;
use common::write_temp_toml;

// ---------------------------------------------------------------------------
// Mutual exclusivity (Req 12.3, 12.4, 12.5)
// ---------------------------------------------------------------------------

#[test]
fn load_config_paths_and_projects_mutual_exclusivity() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.md"]

[projects.frontend]
paths = ["frontend/**/*.md"]
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
paths = ["frontend/**/*.md"]
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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]
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
paths = ["specs/**/*.md"]
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
paths = ["specs/**/*.md"]
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
paths = ["specs/**/*.md"]
tests = ["tests/**/*.rs"]
id_pattern = "^[a-z]+"

[verify.rules]
zero_tag_matches = "error"
missing_verification_evidence = "warning"
"#,
    );
    let config = load_config(Path::new(&path)).unwrap();
    assert_eq!(config.paths, Some(vec!["specs/**/*.md".to_string()]));
    assert_eq!(config.tests, Some(vec!["tests/**/*.rs".to_string()]));
    assert_eq!(config.verify.rules.len(), 2);
}

#[test]
fn load_config_valid_multi_project() {
    let path = write_temp_toml(
        r#"
[projects.frontend]
paths = ["frontend/**/*.md"]
tests = ["frontend/tests/**/*.rs"]

[projects.backend]
paths = ["backend/**/*.md"]
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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]
id_pattern = "[bad(regex"

[projects.frontend]
paths = ["frontend/**/*.md"]

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
