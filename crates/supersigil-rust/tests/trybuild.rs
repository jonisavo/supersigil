//! `trybuild` tests for the `#[verifies(...)]` proc macro.
//!
//! These tests verify that the proc macro compiles correctly when applied
//! to valid Rust test functions, and produces useful errors for invalid usage.

#[test]
fn verifies_macro() {
    // Explicitly disable graph validation by setting an empty project root.
    // SAFETY: nextest runs each test in its own process.
    unsafe { std::env::set_var("SUPERSIGIL_PROJECT_ROOT", "") };
    let t = trybuild::TestCases::new();
    t.pass("tests/fixtures/pass/*.rs");
    t.compile_fail("tests/fixtures/fail/empty_fragment_ref.rs");
    t.compile_fail("tests/fixtures/fail/fragmentless_ref.rs");
    t.compile_fail("tests/fixtures/fail/malformed_empty_args.rs");
    t.compile_fail("tests/fixtures/fail/malformed_not_string.rs");
    t.compile_fail("tests/fixtures/fail/unsupported_item_struct.rs");
    // Note: trybuild runs tests on drop, so env var must remain set until then.
}

#[test]
fn verifies_graph_validation() {
    // Set env to enable graph validation pointing to test project.
    // Use an absolute path since trybuild may run cargo in a different cwd.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let validation_project = manifest_dir.join("tests/fixtures/validation-project");
    // SAFETY: nextest runs each test in its own process.
    unsafe { std::env::set_var("SUPERSIGIL_PROJECT_ROOT", &validation_project) };
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/fixtures/fail/unresolved_criterion_ref.rs");
    // Note: trybuild runs tests on drop, so env var must remain set until then.
}

#[test]
fn verifies_missing_config_at_project_root() {
    // Point SUPERSIGIL_PROJECT_ROOT to a directory that exists but has no
    // supersigil.toml. The macro should emit a compile-time error.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let no_config = manifest_dir.join("tests/fixtures/no-config-project");
    // SAFETY: nextest runs each test in its own process.
    unsafe { std::env::set_var("SUPERSIGIL_PROJECT_ROOT", &no_config) };
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/fixtures/fail/missing_config_at_project_root.rs");
}

#[test]
fn verifies_malformed_spec_parse_error() {
    // Point SUPERSIGIL_PROJECT_ROOT to a project with a malformed spec file.
    // The macro should emit a compile-time diagnostic about the parse failure.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let malformed_project = manifest_dir.join("tests/fixtures/malformed-spec-project");
    // SAFETY: nextest runs each test in its own process.
    unsafe { std::env::set_var("SUPERSIGIL_PROJECT_ROOT", &malformed_project) };
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/fixtures/fail/malformed_spec_parse_error.rs");
}
