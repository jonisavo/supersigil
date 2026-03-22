mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use supersigil_rust::verifies;
use tempfile::TempDir;

#[test]
fn ls_lists_all_documents() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirements", "draft");
    common::write_spec(tmp.path(), "b", "doc/b", "design", "verified");

    cargo_bin_cmd!("supersigil")
        .args(["ls"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("doc/a"))
        .stdout(predicate::str::contains("doc/b"));
}

#[test]
fn ls_filter_by_type() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirements", "draft");
    common::write_spec(tmp.path(), "b", "doc/b", "design", "verified");

    cargo_bin_cmd!("supersigil")
        .args(["ls", "--type", "requirements"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("doc/a"))
        .stdout(predicate::str::contains("doc/b").not());
}

#[test]
fn ls_filter_by_status() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirements", "draft");
    common::write_spec(tmp.path(), "b", "doc/b", "design", "verified");

    cargo_bin_cmd!("supersigil")
        .args(["ls", "--status", "draft"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("doc/a"))
        .stdout(predicate::str::contains("doc/b").not());
}

#[verifies("inventory-queries/req#req-1-4")]
#[test]
fn ls_json_format() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirements", "draft");

    let output = cargo_bin_cmd!("supersigil")
        .args(["ls", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert!(json.is_array());
}

#[verifies("inventory-queries/req#req-1-3")]
#[test]
fn ls_empty_result_exits_zero() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["ls"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn list_alias_works() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirements", "draft");

    cargo_bin_cmd!("supersigil")
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("doc/a"));
}

#[verifies("workspace-projects/req#req-2-1")]
#[test]
fn ls_filter_by_project_in_multi_project_mode() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("supersigil.toml"),
        r#"[projects.workspace]
paths = ["specs/**/*.md"]
"#,
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("specs")).unwrap();
    common::write_spec_doc(
        tmp.path(),
        "specs/workspace-doc.md",
        "workspace/doc",
        Some("requirements"),
        Some("draft"),
        "",
    );

    cargo_bin_cmd!("supersigil")
        .args(["ls", "--project", "workspace"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("workspace/doc"));
}

/// `graph --format dot` writes DOT syntax to stdout; summary goes to stderr.
#[verifies("inventory-queries/req#req-3-2")]
#[test]
fn graph_dot_format_writes_dot_syntax_to_stdout() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirements", "draft");

    let output = cargo_bin_cmd!("supersigil")
        .args(["graph", "--format", "dot"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("digraph specs {"),
        "stdout should contain DOT digraph declaration, got: {stdout}"
    );
    assert!(
        stdout.contains("rankdir=TB;"),
        "stdout should contain DOT layout directive, got: {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Graph:"),
        "stderr should contain the summary line, got: {stderr}"
    );

    // DOT syntax keywords should NOT appear on stderr.
    assert!(
        !stderr.contains("digraph"),
        "stderr should not contain DOT syntax"
    );
}

/// `graph` writes syntax to stdout only; summary and hint go to stderr.
#[verifies("inventory-queries/req#req-3-3")]
#[test]
fn graph_writes_syntax_to_stdout_and_summary_to_stderr() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirements", "draft");

    let output = cargo_bin_cmd!("supersigil")
        .args(["graph"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Default format is mermaid — stdout should contain mermaid graph syntax.
    assert!(
        stdout.contains("graph TD"),
        "stdout should contain mermaid syntax, got: {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Summary (node/edge counts) and hint go to stderr.
    assert!(
        stderr.contains("Graph:"),
        "stderr should contain the summary line, got: {stderr}"
    );
    assert!(
        stderr.contains("hint:"),
        "stderr should contain the pipe-to-file hint, got: {stderr}"
    );

    // Ensure summary text is NOT on stdout.
    assert!(
        !stdout.contains("Graph:"),
        "stdout should not contain summary text"
    );
}
