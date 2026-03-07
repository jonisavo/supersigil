mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

/// task-7-2: Generated requirement template must pass lint.
#[test]
fn new_requirement_passes_lint() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    // Generate a requirement scaffold
    cargo_bin_cmd!("supersigil")
        .args(["new", "requirement", "auth"])
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

/// task-7-3 + task-7-4: Generated property template must not break graph
/// and must pass lint (`VerifiedBy` requires strategy).
#[test]
fn new_property_passes_lint_and_graph() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "property", "perf"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Must pass lint (no missing required attributes)
    cargo_bin_cmd!("supersigil")
        .args(["lint"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Must not break graph (no empty refs)
    cargo_bin_cmd!("supersigil")
        .args(["ls"])
        .current_dir(tmp.path())
        .assert()
        .success();
}
