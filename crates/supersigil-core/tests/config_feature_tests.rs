// ExamplesConfig, RunnerConfig, LSP config, and ecosystem.rust deserialization tests

use std::path::Path;
use supersigil_core::{Config, ConfigError, RustValidationPolicy, load_config};

mod common;
use common::write_temp_toml;

// ===========================================================================
// Task 2: ExamplesConfig and RunnerConfig tests
// ===========================================================================

// ---------------------------------------------------------------------------
// 1. Default values: timeout=30, parallelism=available/2 (min 1), empty runners
// ---------------------------------------------------------------------------

#[test]
fn examples_config_defaults() {
    let config = supersigil_core::ExamplesConfig::default();
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
    let config = supersigil_core::ExamplesConfig::default();
    assert!(config.parallelism >= 1);
}

// ---------------------------------------------------------------------------
// 2. Custom values via TOML parsing
// ---------------------------------------------------------------------------

#[test]
fn examples_config_custom_values() {
    let toml_str = r#"
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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
paths = ["specs/**/*.md"]

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

// ---------------------------------------------------------------------------
// LSP config
// ---------------------------------------------------------------------------

#[test]
fn config_without_lsp_section_loads() {
    let toml_str = r#"paths = ["specs/**/*.md"]"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.lsp.is_none());
}

#[test]
fn config_with_lsp_diagnostics_verify() {
    let toml_str = r#"
paths = ["specs/**/*.md"]

[lsp]
diagnostics = "verify"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let lsp = config.lsp.unwrap();
    assert_eq!(lsp.diagnostics, supersigil_core::DiagnosticsTier::Verify);
}

#[test]
fn lsp_diagnostics_defaults_to_verify() {
    let toml_str = r#"
paths = ["specs/**/*.md"]

[lsp]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let lsp = config.lsp.unwrap();
    assert_eq!(lsp.diagnostics, supersigil_core::DiagnosticsTier::Verify);
}

#[test]
fn lsp_diagnostics_lint_variant() {
    let toml_str = r#"
paths = ["specs/**/*.md"]

[lsp]
diagnostics = "lint"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(
        config.lsp.unwrap().diagnostics,
        supersigil_core::DiagnosticsTier::Lint,
    );
}

#[test]
fn lsp_diagnostics_rejects_unknown_variant() {
    let toml_str = r#"
paths = ["specs/**/*.md"]

[lsp]
diagnostics = "full"
"#;
    toml::from_str::<Config>(toml_str).unwrap_err();
}
