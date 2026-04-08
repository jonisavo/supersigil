mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use supersigil_rust::verifies;
use tempfile::TempDir;

#[test]
fn context_shows_document_info() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
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

#[verifies("work-queries/req#req-2-2")]
#[test]
fn context_json_format() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
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

#[verifies("work-queries/req#req-1-3")]
#[test]
fn context_unknown_id_exits_one() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(tmp.path(), "specs/req.md", "test/doc", None, None, "");

    cargo_bin_cmd!("supersigil")
        .args(["context", "nonexistent/doc"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[verifies("work-queries/req#req-6-1")]
#[test]
fn context_json_compact_omits_components() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/doc",
        Some("requirements"),
        Some("draft"),
        r#"# Test

<AcceptanceCriteria>
  <Criterion id="c1">test criterion</Criterion>
</AcceptanceCriteria>
"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["context", "test/doc", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let doc = json.get("document").expect("document key should exist");
    let components = doc
        .get("components")
        .and_then(|c| c.as_array())
        .expect("components should be an array");
    assert!(
        components.is_empty(),
        "compact context JSON should have empty components, got: {components:?}"
    );
    // Derived fields should still be present.
    assert!(
        json.get("criteria").is_some(),
        "criteria field should exist"
    );
}

#[verifies("work-queries/req#req-6-2")]
#[test]
fn context_json_detail_full_includes_components() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/doc",
        Some("requirements"),
        Some("draft"),
        r#"# Test

<AcceptanceCriteria>
  <Criterion id="c1">test criterion</Criterion>
</AcceptanceCriteria>
"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args([
            "context", "test/doc", "--format", "json", "--detail", "full",
        ])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let doc = json.get("document").expect("document key should exist");
    let components = doc
        .get("components")
        .and_then(|c| c.as_array())
        .expect("components should be an array");
    assert!(
        !components.is_empty(),
        "full context JSON should have non-empty components"
    );
}

/// Context JSON output does NOT expose a separate illustrations collection.
#[verifies("work-queries/req#req-2-3")]
#[test]
fn context_json_has_no_illustrations_key() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/req.md",
        "test/doc",
        Some("requirements"),
        Some("draft"),
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
    assert!(
        json.get("illustrations").is_none(),
        "context JSON should NOT contain an 'illustrations' key, got: {json}"
    );
}
