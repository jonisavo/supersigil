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
