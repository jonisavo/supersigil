mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn plan_all_shows_outstanding_criteria() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(
        tmp.path(),
        "specs/req.mdx",
        "auth/req/login",
        Some("requirements"),
        Some("approved"),
        r#"# Login

<AcceptanceCriteria>
  <Criterion id="valid-creds">
    WHEN valid creds THEN return token.
  </Criterion>
</AcceptanceCriteria>
"#,
    );

    cargo_bin_cmd!("supersigil")
        .args(["plan"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("valid-creds"));
}

#[test]
fn plan_exact_id() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(
        tmp.path(),
        "specs/req.mdx",
        "auth/req/login",
        Some("requirements"),
        None,
        "# Login\n\n<AcceptanceCriteria>\n  <Criterion id=\"c1\">\n    Test criterion.\n  </Criterion>\n</AcceptanceCriteria>\n",
    );

    cargo_bin_cmd!("supersigil")
        .args(["plan", "auth/req/login"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("c1"));
}

#[test]
fn plan_prefix_match() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(
        tmp.path(),
        "specs/a.mdx",
        "auth/req/login",
        Some("requirements"),
        None,
        "# A\n\n<AcceptanceCriteria>\n  <Criterion id=\"c1\">\n    Test.\n  </Criterion>\n</AcceptanceCriteria>\n",
    );
    common::write_mdx(
        tmp.path(),
        "specs/b.mdx",
        "billing/req/pay",
        Some("requirements"),
        None,
        "# B\n\n<AcceptanceCriteria>\n  <Criterion id=\"c2\">\n    Test.\n  </Criterion>\n</AcceptanceCriteria>\n",
    );

    cargo_bin_cmd!("supersigil")
        .args(["plan", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("c1"))
        .stdout(predicate::str::contains("c2").not());
}

#[test]
fn plan_no_match_exits_one() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(tmp.path(), "specs/req.mdx", "test/doc", None, None, "");

    cargo_bin_cmd!("supersigil")
        .args(["plan", "nonexistent"])
        .current_dir(tmp.path())
        .assert()
        .failure();
}

#[test]
fn plan_json_format() {
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
        .args(["plan", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert!(json.get("outstanding_criteria").is_some());
}
