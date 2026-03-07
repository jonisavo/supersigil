mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

/// task-7-1: Criterion nested inside `AcceptanceCriteria` must be counted.
#[test]
fn status_counts_nested_criteria() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(
        tmp.path(),
        "specs/auth.mdx",
        "auth/req",
        Some("requirement"),
        Some("draft"),
        "<AcceptanceCriteria>\n  <Criterion id=\"ac-1\">\n    Must log in\n  </Criterion>\n  <Criterion id=\"ac-2\">\n    Must log out\n  </Criterion>\n</AcceptanceCriteria>",
    );

    cargo_bin_cmd!("supersigil")
        .args(["status", "--format", "json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"criteria_total\": 2"));
}
