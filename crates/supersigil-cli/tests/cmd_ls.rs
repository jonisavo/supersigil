mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn ls_lists_all_documents() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirement", "draft");
    common::write_spec(tmp.path(), "b", "doc/b", "property", "verified");

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
    common::write_spec(tmp.path(), "a", "doc/a", "requirement", "draft");
    common::write_spec(tmp.path(), "b", "doc/b", "property", "verified");

    cargo_bin_cmd!("supersigil")
        .args(["ls", "--type", "requirement"])
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
    common::write_spec(tmp.path(), "a", "doc/a", "requirement", "draft");
    common::write_spec(tmp.path(), "b", "doc/b", "property", "verified");

    cargo_bin_cmd!("supersigil")
        .args(["ls", "--status", "draft"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("doc/a"))
        .stdout(predicate::str::contains("doc/b").not());
}

#[test]
fn ls_json_format() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirement", "draft");

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
    common::write_spec(tmp.path(), "a", "doc/a", "requirement", "draft");

    cargo_bin_cmd!("supersigil")
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("doc/a"));
}
