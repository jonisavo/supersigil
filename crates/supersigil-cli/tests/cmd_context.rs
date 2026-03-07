mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn context_shows_document_info() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(
        tmp.path(),
        "specs/req.mdx",
        "auth/req/login",
        Some("requirements"),
        Some("approved"),
        r#"# User Login

<AcceptanceCriteria>
  <Criterion id="valid-creds">
    WHEN valid email and password, THEN return session token.
  </Criterion>
</AcceptanceCriteria>
"#,
    );

    cargo_bin_cmd!("supersigil")
        .args(["context", "auth/req/login"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("auth/req/login"))
        .stdout(predicate::str::contains("valid-creds"));
}

#[test]
fn context_json_format() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(
        tmp.path(),
        "specs/req.mdx",
        "test/doc",
        Some("requirements"),
        None,
        "# Test\n",
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["context", "test/doc", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert!(json.get("document").is_some());
}

#[test]
fn context_unknown_id_exits_one() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(tmp.path(), "specs/req.mdx", "test/doc", None, None, "");

    cargo_bin_cmd!("supersigil")
        .args(["context", "nonexistent/doc"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}
