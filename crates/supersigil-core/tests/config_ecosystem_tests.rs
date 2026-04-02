// Ecosystem plugin activation & Rust policy config tests (TDD)
// Tests for: RustValidationPolicy, RustProjectScope, RustEcosystemConfig,
//            unknown plugin validation, and project_scope resolution.

use std::path::Path;
use supersigil_core::{ConfigError, RustEcosystemConfig, RustValidationPolicy, load_config};

mod common;
use common::write_temp_toml;

// ---------------------------------------------------------------------------
// 1. Default ecosystem config: plugins = ["rust"], validation defaults to "dev"
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_default_rust_validation_is_dev() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.md"]
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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
// 4-6. [ecosystem.rust] validation policies parsed correctly
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_validation_policies() {
    let cases = [
        ("off", RustValidationPolicy::Off),
        ("dev", RustValidationPolicy::Dev),
        ("all", RustValidationPolicy::All),
    ];

    for (value, expected) in cases {
        let path = write_temp_toml(&format!(
            r#"
paths = ["specs/**/*.md"]

[ecosystem.rust]
validation = "{value}"
"#,
        ));
        let config = load_config(Path::new(&path)).unwrap();
        let rust_config = config
            .ecosystem
            .rust
            .unwrap_or_else(|| panic!("ecosystem.rust should be present for validation={value}"));
        assert_eq!(
            rust_config.validation, expected,
            "validation=\"{value}\" should parse to {expected:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. [ecosystem.rust] validation = "invalid" → config error
// ---------------------------------------------------------------------------

#[test]
fn ecosystem_rust_validation_invalid_rejected() {
    let path = write_temp_toml(
        r#"
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
