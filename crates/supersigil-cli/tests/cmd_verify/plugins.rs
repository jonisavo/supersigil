use super::*;

#[verifies("ecosystem-plugins/req#req-3-3", "cli-runtime/req#req-4-3")]
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

#[verifies(
    "ecosystem-plugins/req#req-2-3",
    "ecosystem-plugins/req#req-2-4",
    "ecosystem-plugins/req#req-3-1",
    "ecosystem-plugins/req#req-3-2"
)]
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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "json verify output should not emit human-readable stderr, got: {stderr}",
    );
}

#[verifies("cli-runtime/req#req-3-4", "cli-runtime/req#req-1-4")]
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
        stderr.is_empty(),
        "json verify output should not emit human-readable stderr, got: {stderr}",
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
fn verify_json_shared_file_glob_evidence_does_not_surface_conflicts() {
    let tmp = TempDir::new().unwrap();
    setup_shared_file_glob_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "shared file-glob evidence should not downgrade verify to warnings: {}",
        String::from_utf8_lossy(&output.stdout),
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(report["summary"]["warning_count"], 0);
    assert!(
        !report["findings"]
            .as_array()
            .expect("findings should be an array")
            .iter()
            .any(|finding| finding["rule"] == "plugin_discovery_failure"),
        "shared file-glob evidence should not surface conflict warnings, got: {report}",
    );
}

#[verifies("ecosystem-plugins/req#req-1-2")]
#[test]
fn verify_with_plugins_disabled_keeps_explicit_evidence_and_stays_clean() {
    let tmp = TempDir::new().unwrap();
    setup_explicit_evidence_only_fixture(tmp.path());

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
        stderr.is_empty(),
        "json verify output should not emit human-readable stderr, got: {stderr}",
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

#[verifies("ecosystem-plugins/req#req-1-3")]
#[test]
fn verify_unknown_plugin_config_fails_before_plugin_assembly() {
    let tmp = TempDir::new().unwrap();
    write_config(
        tmp.path(),
        r#"paths = ["specs/**/*.md"]

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
