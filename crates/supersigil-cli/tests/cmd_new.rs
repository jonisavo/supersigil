mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

/// task-7-2: Generated requirements template must pass lint.
#[test]
fn new_requirements_passes_lint() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    // Generate a requirements scaffold
    cargo_bin_cmd!("supersigil")
        .args(["new", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // The generated file must pass lint
    cargo_bin_cmd!("supersigil")
        .args(["lint"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// task-7-2: Generated tasks template must pass lint.
#[test]
fn new_tasks_passes_lint() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "tasks", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    cargo_bin_cmd!("supersigil")
        .args(["lint"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// task-7-3: Generated design template must not break graph loading.
#[test]
fn new_design_does_not_break_graph() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "design", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // ls requires a working graph — must not fail with broken ref
    cargo_bin_cmd!("supersigil")
        .args(["ls"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// Design template with existing req fills in Implements ref.
#[test]
fn new_design_with_existing_req_fills_implements() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    // Create a requirements doc first
    cargo_bin_cmd!("supersigil")
        .args(["new", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Now create a design doc — should detect the req file
    cargo_bin_cmd!("supersigil")
        .args(["new", "design", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // The design file should have a filled-in Implements ref
    let design_content =
        std::fs::read_to_string(tmp.path().join("specs/auth/auth.design.mdx")).unwrap();
    assert!(
        design_content.contains(r#"<Implements refs="auth/req" />"#),
        "design should have filled Implements ref, got:\n{design_content}"
    );

    // Graph must load successfully (Implements ref is valid)
    cargo_bin_cmd!("supersigil")
        .args(["ls"])
        .current_dir(tmp.path())
        .assert()
        .success();
}
