mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use supersigil_rust::verifies;
use tempfile::TempDir;

fn setup_multi_project(dir: &std::path::Path) {
    fs::write(
        dir.join("supersigil.toml"),
        r#"
[projects.workspace]
paths = ["specs/**/*.mdx"]

[projects.cli]
paths = ["crates/my-cli/specs/**/*.mdx"]
"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("specs")).unwrap();
    fs::create_dir_all(dir.join("crates/my-cli/specs")).unwrap();
}

/// task-7-2: Generated requirements template must pass lint.
#[verifies("authoring-commands/req#req-2-3", "authoring-commands/req#req-3-1")]
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
#[verifies("authoring-commands/req#req-3-1")]
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
#[verifies("authoring-commands/req#req-3-3")]
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

/// In multi-project mode, --project places file under the project's spec directory.
#[test]
fn new_with_project_places_file_in_project_dir() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "cli", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // File should be under the cli project's spec directory
    let expected = tmp.path().join("crates/my-cli/specs/auth/auth.req.mdx");
    assert!(
        expected.is_file(),
        "expected file at {}",
        expected.display()
    );

    // Must pass lint
    cargo_bin_cmd!("supersigil")
        .args(["lint"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// --project with workspace project uses root specs/.
#[test]
fn new_with_workspace_project_uses_root_specs() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "workspace", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let expected = tmp.path().join("specs/auth/auth.req.mdx");
    assert!(
        expected.is_file(),
        "expected file at {}",
        expected.display()
    );
}

/// --project with unknown project name errors.
#[test]
fn new_with_unknown_project_errors() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "nonexistent", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("nonexistent").from_utf8());
}

/// --project in single-project mode errors.
#[verifies("authoring-commands/req#req-2-7")]
#[test]
fn new_with_project_in_single_project_mode_errors() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "foo", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .failure();
}

/// Omitting --project in multi-project mode errors.
#[test]
fn new_without_project_in_multi_project_mode_errors() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("--project").from_utf8());
}

/// Design template with --project detects sibling req in correct directory.
#[test]
fn new_design_with_project_detects_sibling_req() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    // Create req in the cli project
    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "cli", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Create design in the cli project — should find sibling req
    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "cli", "design", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let design_content =
        std::fs::read_to_string(tmp.path().join("crates/my-cli/specs/auth/auth.design.mdx"))
            .unwrap();
    assert!(
        design_content.contains(r#"<Implements refs="auth/req" />"#),
        "design should have filled Implements ref, got:\n{design_content}"
    );
}
