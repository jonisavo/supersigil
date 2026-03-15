mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use supersigil_rust::verifies;
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

fn write_requirement_with_shared_file_glob_evidence(root: &Path) {
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
  <Criterion id="ac-2">
    Must keep the session alive
    <VerifiedBy strategy="file-glob" paths="tests/auth_test.rs" />
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

fn setup_shared_file_glob_fixture(root: &Path) {
    write_config(
        root,
        r#"paths = ["specs/**/*.mdx"]
tests = ["tests/**/*.rs"]

[ecosystem]
plugins = []
"#,
    );
    write_requirement_with_shared_file_glob_evidence(root);
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests/auth_test.rs"),
        "# shared authored evidence\n",
    )
    .unwrap();
}

fn setup_clean_example_fixture(root: &Path) {
    common::setup_project(root);
    common::write_mdx(
        root,
        "specs/examples.mdx",
        "examples/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="examples-1">cargo-test examples run during verify</Criterion>
</AcceptanceCriteria>

<Example
  id="cargo-pass"
  lang="rust"
  runner="cargo-test"
  verifies="examples/req#examples-1"
>

```rust
#[test]
fn cargo_pass() {
    println!("cargo-test-pass");
}
```

<Expected status="0" contains="cargo-test-pass" />
</Example>"#,
    );
}

fn setup_failing_example_fixture(root: &Path) {
    common::setup_project(root);
    common::write_mdx(
        root,
        "specs/examples.mdx",
        "examples/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="examples-1">cargo-test examples run during verify</Criterion>
</AcceptanceCriteria>

<Example
  id="cargo-pass"
  lang="rust"
  runner="cargo-test"
  verifies="examples/req#examples-1"
>

```rust
#[test]
fn cargo_pass() {
    println!("cargo-test-pass");
}
```

<Expected status="0" contains="cargo-test-pass" />
</Example>

<Example
  id="cargo-fail"
  lang="rust"
  runner="cargo-test"
  verifies="examples/req#examples-1"
>

```rust
#[test]
fn cargo_fail() {
    assert_eq!(1, 2);
}
```

<Expected status="0" />
</Example>"#,
    );
}

fn setup_non_blocking_failing_example_fixture(root: &Path) {
    common::setup_project(root);
    common::write_mdx(
        root,
        "specs/examples.mdx",
        "examples/req",
        Some("requirements"),
        Some("draft"),
        r#"<AcceptanceCriteria>
  <Criterion id="examples-1">draft examples can fail without blocking verify</Criterion>
</AcceptanceCriteria>

<Example
  id="body-mismatch"
  lang="sh"
  runner="sh"
  verifies="examples/req#examples-1"
>

```sh
printf 'line1\nline2\n'
```

<Expected status="0" format="regex">

```regex
expected-output
```

</Expected>
</Example>"#,
    );
}

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

#[verifies("executable-examples/req#req-4-2")]
#[test]
fn verify_terminal_reports_example_pass_counts_on_clean_run() {
    let tmp = TempDir::new().unwrap();
    setup_clean_example_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .arg("verify")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Executing 1 example"),
        "terminal verify output should announce example execution, got: {stdout}",
    );
    assert!(
        stdout.contains("cargo-pass"),
        "terminal verify output should name the example being executed, got: {stdout}",
    );
    assert!(
        stdout.contains("cargo-pass (cargo-test) passed"),
        "terminal verify output should show the example completion status, got: {stdout}",
    );
    assert!(
        stdout.contains("Examples"),
        "terminal verify output should include an Examples section when examples run, got: {stdout}",
    );
    assert!(
        stdout.contains("1 passed"),
        "terminal verify output should report passing example counts, got: {stdout}",
    );
    assert!(
        stdout.contains("Clean"),
        "clean verify output should still include the clean summary, got: {stdout}",
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "terminal verify output should not emit an extra clean summary to stderr, got: {stderr}",
    );
}

#[verifies("executable-examples/req#req-4-3")]
#[test]
fn verify_terminal_reports_failed_examples_after_summary() {
    let tmp = TempDir::new().unwrap();
    setup_failing_example_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .arg("verify")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Executing 2 examples"),
        "terminal verify output should announce example execution, got: {stdout}",
    );
    assert!(
        stdout.contains("cargo-pass") && stdout.contains("cargo-fail"),
        "terminal verify output should name each example as it executes, got: {stdout}",
    );
    assert!(
        stdout.contains("cargo-pass (cargo-test) passed"),
        "terminal verify output should show passing example completion status, got: {stdout}",
    );
    assert!(
        stdout.contains("cargo-fail (cargo-test) failed"),
        "terminal verify output should show failing example completion status, got: {stdout}",
    );
    assert!(
        stdout.contains("Examples"),
        "terminal verify output should include an Examples section when examples run, got: {stdout}",
    );
    assert!(
        stdout.contains("1 passed"),
        "terminal verify output should report passing example counts, got: {stdout}",
    );
    assert!(
        stdout.contains("1 failed"),
        "terminal verify output should report failing example counts, got: {stdout}",
    );
    assert!(
        stdout.contains("cargo-fail"),
        "terminal verify output should list failed examples by id, got: {stdout}",
    );
    assert!(
        stdout.contains("cargo-test"),
        "terminal verify output should mention the failed example runner, got: {stdout}",
    );
}

#[verifies("executable-examples/req#req-4-4")]
#[test]
fn verify_skips_examples_when_structural_errors_exist() {
    let tmp = TempDir::new().unwrap();
    // Set up a project with examples AND a structural error:
    // a <VerifiedBy> at document root (outside a Criterion) triggers
    // InvalidVerifiedByPlacement, which is an Error-severity structural finding.
    common::setup_project(tmp.path());
    common::write_mdx(
        tmp.path(),
        "specs/mixed.mdx",
        "mixed/req",
        Some("requirements"),
        Some("approved"),
        r#"<VerifiedBy strategy="file-glob" paths="specs/mixed.mdx" />

<AcceptanceCriteria>
  <Criterion id="crit-1">
    Has evidence
    <VerifiedBy strategy="file-glob" paths="specs/mixed.mdx" />
  </Criterion>
</AcceptanceCriteria>

<Example
  id="should-not-run"
  lang="sh"
  runner="sh"
  verifies="mixed/req#crit-1"
>

```sh
echo "this should be skipped"
```

<Expected status="0" contains="skipped" />
</Example>"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // Should fail due to structural errors
    assert_ne!(output.status.code(), Some(0));

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let findings = report["findings"]
        .as_array()
        .expect("findings should be an array");

    // Should have the structural error
    assert!(
        findings
            .iter()
            .any(|f| f["rule"] == "invalid_verified_by_placement"),
        "expected a structural error finding, got: {report}",
    );

    // Should contain a finding noting that examples were skipped
    assert!(
        findings.iter().any(|f| {
            f["message"]
                .as_str()
                .is_some_and(|m| m.contains("example execution skipped"))
        }),
        "expected an info finding noting examples were skipped, got: {report}",
    );
}

#[verifies("executable-examples/req#req-4-5")]
#[test]
fn verify_skip_examples_flag_prevents_example_execution() {
    let tmp = TempDir::new().unwrap();
    setup_clean_example_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "--skip-examples"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let findings = report["findings"]
        .as_array()
        .expect("findings should be an array");

    // Should contain a finding noting that examples were skipped via flag
    assert!(
        findings.iter().any(|f| {
            f["message"]
                .as_str()
                .is_some_and(|m| m.contains("--skip-examples"))
        }),
        "expected an info finding noting examples were skipped via --skip-examples, got: {report}",
    );
}

#[verifies("executable-examples/req#req-4-5")]
#[test]
fn verify_update_snapshots_flag_is_accepted() {
    let tmp = TempDir::new().unwrap();
    setup_clean_example_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "--update-snapshots"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(
        report["summary"]["error_count"], 0,
        "verify with --update-snapshots should succeed on a clean fixture, got: {report}",
    );
}

#[verifies("executable-examples/req#req-4-8")]
#[test]
fn verify_terminal_non_blocking_failed_examples_stay_readable() {
    let tmp = TempDir::new().unwrap();
    setup_non_blocking_failing_example_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .arg("verify")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("body-mismatch"),
        "terminal verify output should list the failing example id, got: {stdout}",
    );
    assert!(
        stdout.contains("expected:"),
        "terminal verify output should show a readable expected block for failed examples, got: {stdout}",
    );
    assert!(
        stdout.contains("actual:"),
        "terminal verify output should show a readable actual block for failed examples, got: {stdout}",
    );
    assert!(
        stdout.contains("line1") && stdout.contains("line2"),
        "terminal verify output should render multiline actual output instead of escaping it, got: {stdout}",
    );
    assert!(
        stdout.contains("No blocking findings"),
        "terminal verify output should explain that draft-only failures are non-blocking, got: {stdout}",
    );
    assert!(
        !stdout.contains("Clean — no findings"),
        "terminal verify output should not claim there were no findings when examples failed, got: {stdout}",
    );
}
