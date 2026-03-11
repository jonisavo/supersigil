mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_config(root: &Path, content: &str) {
    fs::write(root.join("supersigil.toml"), content).unwrap();
    fs::create_dir_all(root.join("specs")).unwrap();
}

fn write_requirement_with_explicit_evidence(root: &Path) {
    common::write_mdx(
        root,
        "specs/auth.mdx",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="ac-1">
    Must log in
    <VerifiedBy strategy="file-glob" paths="tests/auth_test.rs" />
  </Criterion>
</AcceptanceCriteria>"#,
    );
}

fn write_requirement_for_plugin_evidence(root: &Path) {
    common::write_mdx(
        root,
        "specs/auth.mdx",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="ac-1">
    Must log in
  </Criterion>
</AcceptanceCriteria>"#,
    );
}

fn setup_plugin_failure_fixture(root: &Path) {
    common::setup_project_with_rust_plugin_and_tests(root, "tests/**/*.rs", "");
    write_requirement_with_explicit_evidence(root);
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("tests/auth_test.rs"),
        "# explicit authored evidence\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
}

fn setup_partial_plugin_warning_fixture(root: &Path, extra_config: &str) {
    common::setup_project_with_rust_plugin_and_tests(root, "tests/**/*.rs", extra_config);
    write_requirement_for_plugin_evidence(root);
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("tests/auth_test.rs"),
        "#[test]\n#[verifies(\"auth/req#ac-1\")]\nfn login_succeeds() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/bad.rs"),
        "#[verifies(\"auth/req#ac-1\")] fn { broken\n",
    )
    .unwrap();
}

fn setup_missing_evidence_fixture(root: &Path) {
    common::setup_project_with_rust_plugin_and_tests(root, "tests/**/*.rs", "");
    write_requirement_for_plugin_evidence(root);
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests/auth_test.rs"),
        "#[test]\nfn login_succeeds() {}\n",
    )
    .unwrap();
}

#[test]
fn verify_terminal_surfaces_plugin_failure_as_report_finding() {
    let tmp = TempDir::new().unwrap();
    setup_plugin_failure_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .arg("verify")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[plugin_discovery_failure]"),
        "verify output should tag the plugin failure rule, got: {stdout}",
    );
    assert!(
        stdout.contains("zero supported Rust test items"),
        "verify output should include the plugin failure message, got: {stdout}",
    );
}

#[test]
fn verify_json_surfaces_partial_plugin_warning_and_preserves_evidence() {
    let tmp = TempDir::new().unwrap();
    setup_partial_plugin_warning_fixture(tmp.path(), "");

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let findings = report["findings"]
        .as_array()
        .expect("findings should be an array");
    assert!(
        findings.iter().any(|finding| {
            finding["rule"] == "plugin_discovery_warning"
                && finding["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("skipping due to parse failure"))
        }),
        "expected a surfaced plugin warning in verify JSON, got: {report}",
    );

    let warning_count = report["summary"]["warning_count"]
        .as_u64()
        .expect("warning count should be numeric");
    assert_eq!(warning_count, 1, "unexpected summary: {report}");

    let records = report["evidence_summary"]["records"]
        .as_array()
        .expect("evidence_summary.records should be present");
    assert!(
        records.iter().any(|record| {
            record["provenance"]
                .as_array()
                .is_some_and(|provenance| provenance.iter().any(|entry| entry == "plugin:rust"))
        }),
        "expected plugin-derived evidence in report summary, got: {report}",
    );
}

#[test]
fn verify_missing_evidence_prints_concrete_remediation_hints() {
    let tmp = TempDir::new().unwrap();
    setup_missing_evidence_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("hint:"),
        "machine-readable verify stdout must stay JSON-only, got: {stdout}",
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("supersigil refs"),
        "missing-evidence runs should point users at criterion-ref discovery, got: {stderr}",
    );
    assert!(
        stderr.contains("#[verifies(\"doc#criterion\")]"),
        "Rust-native evidence guidance should mention verifies syntax, got: {stderr}",
    );
    assert!(
        stderr.contains("<VerifiedBy strategy=\"file-glob\""),
        "authored evidence guidance should mention VerifiedBy, got: {stderr}",
    );
    assert!(
        stderr.contains("paths=\"path/to/test-file.rs\""),
        "authored evidence guidance should use a schematic string-literal path example, got: {stderr}",
    );
    assert!(
        !stderr.contains("paths={["),
        "authored evidence guidance should not use expression syntax, got: {stderr}",
    );
    assert!(
        !stderr.contains("Run `supersigil plan` to see outstanding work."),
        "missing-evidence remediation should avoid the generic plan hint, got: {stderr}",
    );
}

#[test]
fn verify_json_plugin_failure_includes_structured_details() {
    let tmp = TempDir::new().unwrap();
    setup_plugin_failure_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let finding = report["findings"]
        .as_array()
        .expect("findings should be an array")
        .iter()
        .find(|finding| finding["rule"] == "plugin_discovery_failure")
        .expect("expected plugin_discovery_failure finding");

    assert_eq!(finding["details"]["plugin"], "rust");
    assert_eq!(finding["details"]["code"], "zero_supported_test_items");
    assert!(
        finding["details"]["suggestion"]
            .as_str()
            .is_some_and(|value| value.contains("#[verifies(\"doc#criterion\")]")),
        "expected a structured remediation suggestion, got: {finding}",
    );
}

#[test]
fn verify_with_plugins_disabled_keeps_explicit_evidence_and_stays_clean() {
    let tmp = TempDir::new().unwrap();
    write_config(
        tmp.path(),
        r#"paths = ["specs/**/*.mdx"]
tests = ["tests/**/*.rs"]

[ecosystem]
plugins = []
"#,
    );
    write_requirement_with_explicit_evidence(tmp.path());
    fs::create_dir_all(tmp.path().join("tests")).unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("tests/auth_test.rs"),
        "# explicit authored evidence\n",
    )
    .unwrap();
    fs::write(tmp.path().join("src/lib.rs"), "pub fn helper() {}\n").unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(report["summary"]["error_count"], 0);
    assert_eq!(report["summary"]["warning_count"], 0);
    assert!(
        report["findings"]
            .as_array()
            .expect("findings should be an array")
            .is_empty(),
        "verify should stay clean when only explicit evidence is enabled, got: {report}",
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("verified, no findings"),
        "clean verify run should still report its clean summary, got: {stderr}",
    );
    assert!(
        !stderr.contains("plugin"),
        "plugin diagnostics should stay absent when plugins are disabled, got: {stderr}",
    );
}

#[test]
fn verify_rule_override_can_suppress_plugin_discovery_warning() {
    let tmp = TempDir::new().unwrap();
    setup_partial_plugin_warning_fixture(
        tmp.path(),
        r#"
[verify.rules]
plugin_discovery_warning = "off"
"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(report["summary"]["warning_count"], 0);
    assert!(
        report["findings"]
            .as_array()
            .expect("findings should be an array")
            .is_empty(),
        "plugin warning override should suppress the surfaced finding, got: {report}",
    );
}

#[test]
fn verify_unknown_plugin_config_fails_before_plugin_assembly() {
    let tmp = TempDir::new().unwrap();
    write_config(
        tmp.path(),
        r#"paths = ["specs/**/*.mdx"]

[ecosystem]
plugins = ["python"]
"#,
    );

    cargo_bin_cmd!("supersigil")
        .arg("verify")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown ecosystem plugin"))
        .stderr(predicate::str::contains("python"));
}
