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

#[test]
fn verify_skip_examples_hints_about_example_pending_criteria() {
    let tmp = TempDir::new().unwrap();
    setup_clean_example_fixture(tmp.path());

    // Run with --skip-examples on terminal format so we can see hints on stderr
    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--skip-examples"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}{stderr}");

    // The fixture has an Example with verifies targeting examples-1, so when
    // examples are skipped, the hint should mention that criteria would be
    // covered by running examples.
    assert!(
        combined.contains("covered by examples") || combined.contains("example-pending"),
        "should hint about criteria that would be covered by examples:\n{combined}",
    );
}

#[verifies("executable-examples/req#req-3-4")]
#[verifies("executable-examples/req#req-4-5")]
#[test]
fn verify_update_snapshots_rewrites_mdx_file() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(
        tmp.path(),
        "specs/snap.mdx",
        "snap/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="snap-1">snapshot test</Criterion>
</AcceptanceCriteria>

<Example id="snap-ex" lang="sh" runner="sh" verifies="snap/req#snap-1">

```sh
echo "new output"
```

<Expected status="0" format="snapshot">

```
old output
```

</Expected>
</Example>"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--update-snapshots", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // Snapshot mismatch is expected (old vs new), so exit code may be non-zero.
    // The important thing is that the file was rewritten.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty(),
        "verify --update-snapshots should produce output, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    // The MDX file should have been rewritten with the actual output
    let updated = fs::read_to_string(tmp.path().join("specs/snap.mdx")).unwrap();
    assert!(
        updated.contains("new output"),
        "snapshot should be updated to actual output in MDX file:\n{updated}",
    );
    assert!(
        !updated.contains("old output"),
        "old snapshot content should be replaced:\n{updated}",
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

// ---------------------------------------------------------------------------
// Project-filtered verification findings
// ---------------------------------------------------------------------------

/// Set up a two-project workspace where project "covered" has evidence and
/// project "uncovered" does not.
fn setup_multi_project_coverage_fixture(root: &Path) {
    fs::write(
        root.join("supersigil.toml"),
        r#"
[projects.covered]
paths = ["specs/covered/**/*.mdx"]
tests = ["tests/covered/**/*.rs"]

[projects.uncovered]
paths = ["specs/uncovered/**/*.mdx"]
tests = ["tests/uncovered/**/*.rs"]

[ecosystem]
plugins = []
"#,
    )
    .unwrap();

    // Project "covered": spec with criterion + matching evidence file
    fs::create_dir_all(root.join("specs/covered")).unwrap();
    common::write_mdx(
        root,
        "specs/covered/auth.mdx",
        "covered/auth",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="cov-1">
    Must authenticate
    <VerifiedBy strategy="file-glob" paths="tests/covered/auth_test.rs" />
  </Criterion>
</AcceptanceCriteria>"#,
    );
    fs::create_dir_all(root.join("tests/covered")).unwrap();
    fs::write(
        root.join("tests/covered/auth_test.rs"),
        "// evidence for cov-1\n",
    )
    .unwrap();

    // Project "uncovered": spec with criterion and NO evidence
    fs::create_dir_all(root.join("specs/uncovered")).unwrap();
    common::write_mdx(
        root,
        "specs/uncovered/billing.mdx",
        "uncovered/billing",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="uncov-1">
    Must bill
  </Criterion>
</AcceptanceCriteria>"#,
    );
}

#[verifies("workspace-projects/req#req-3-4")]
#[test]
fn verify_project_filter_reports_only_selected_project_findings() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project_coverage_fixture(tmp.path());

    // 1. --project covered → should be clean (evidence exists)
    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "--project", "covered"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "project 'covered' should pass verification: {}",
        String::from_utf8_lossy(&output.stdout),
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(
        report["summary"]["error_count"], 0,
        "project 'covered' should have zero errors: {report}",
    );

    // 2. --project uncovered → should have errors (no evidence)
    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "--project", "uncovered"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_ne!(
        output.status.code(),
        Some(0),
        "project 'uncovered' should fail verification",
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let findings = report["findings"]
        .as_array()
        .expect("findings should be an array");

    // All findings should be for the uncovered project only
    for finding in findings {
        if let Some(doc_id) = finding["doc_id"].as_str() {
            assert!(
                doc_id.starts_with("uncovered/"),
                "finding doc_id should belong to 'uncovered' project, got: {doc_id}",
            );
        }
    }

    // 3. No filter → should have errors from uncovered project
    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_ne!(
        output.status.code(),
        Some(0),
        "unfiltered verify should fail (uncovered project has errors)",
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let findings = report["findings"]
        .as_array()
        .expect("findings should be an array");

    // Should contain findings from the uncovered project
    assert!(
        findings.iter().any(|f| f["doc_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("uncovered/"))),
        "unfiltered verify should include findings from 'uncovered' project: {report}",
    );
}

// ---------------------------------------------------------------------------
// -j / --parallelism flag
// ---------------------------------------------------------------------------

#[verifies("executable-examples/req#req-6-5")]
#[test]
fn verify_parallelism_flag_short_accepted() {
    let tmp = TempDir::new().unwrap();
    setup_clean_example_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "-j", "2"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "verify -j 2 should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[verifies("executable-examples/req#req-6-5")]
#[test]
fn verify_parallelism_flag_long_accepted() {
    let tmp = TempDir::new().unwrap();
    setup_clean_example_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "--parallelism", "1"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "verify --parallelism 1 should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[verifies("executable-examples/req#req-6-5")]
#[test]
fn verify_parallelism_flag_overrides_config() {
    let tmp = TempDir::new().unwrap();
    write_config(
        tmp.path(),
        r#"paths = ["specs/**/*.mdx"]
[examples]
parallelism = 8
"#,
    );
    common::write_mdx(
        tmp.path(),
        "specs/demo.mdx",
        "demo/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="d-1">demo</Criterion>
</AcceptanceCriteria>

<Example id="par-test" lang="sh" runner="sh" verifies="demo/req#d-1">

```sh
echo ok
```

<Expected status="0" contains="ok" />
</Example>"#,
    );

    // -j 1 forces sequential even though config says 8
    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "-j", "1"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "verify -j 1 overriding config parallelism=8 should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

// ---------------------------------------------------------------------------
// Post-verify hooks receive interim report with example findings
// ---------------------------------------------------------------------------

#[verifies("executable-examples/req#req-4-6")]
#[test]
fn verify_hooks_receive_interim_report_with_example_results() {
    let tmp = TempDir::new().unwrap();

    // Write a hook script that reads the interim report from stdin and
    // echoes the number of findings back as a hook finding.
    let hook_script = tmp.path().join("check-hook.sh");
    fs::write(
        &hook_script,
        r#"#!/bin/sh
# Read stdin (interim report JSON), count findings, emit hook finding
REPORT=$(cat)
COUNT=$(echo "$REPORT" | grep -o '"rule"' | wc -l)
echo "[[\"info\", \"hook saw $COUNT finding rules\"]]"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    #[allow(
        clippy::semicolon_outside_block,
        reason = "conflicts with semicolon_if_nothing_returned"
    )]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_script, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Config with the hook and an example
    fs::write(
        tmp.path().join("supersigil.toml"),
        format!(
            r#"paths = ["specs/**/*.mdx"]

[hooks]
post_verify = ["{hook}"]
"#,
            hook = hook_script.to_string_lossy()
        ),
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("specs")).unwrap();

    common::write_mdx(
        tmp.path(),
        "specs/hook-test.mdx",
        "hook-test/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="h-1">hook test</Criterion>
</AcceptanceCriteria>

<Example id="hook-ex" lang="sh" runner="sh" verifies="hook-test/req#h-1">

```sh
echo hello
```

<Expected status="0" contains="hello" />
</Example>"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "-j", "1"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "verify with hook should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let findings = report["findings"]
        .as_array()
        .expect("findings should be an array");

    // The hook should have produced a finding mentioning the finding count
    let hook_findings: Vec<_> = findings
        .iter()
        .filter(|f| {
            f["message"]
                .as_str()
                .is_some_and(|m| m.contains("hook saw"))
        })
        .collect();

    assert!(
        !hook_findings.is_empty(),
        "hook should produce a finding with interim report data: {report}",
    );
}

/// Verify that an empty project (0 documents) produces a warning finding and
/// exits with code 2 (warnings only).
#[test]
fn verify_empty_project_warns() {
    let dir = TempDir::new().unwrap();
    write_config(dir.path(), "paths = [\"specs/**/*.mdx\"]\n");

    // JSON output: should contain the empty_project finding as a warning
    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit code 2 (warnings only)"
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert!(
        findings.iter().any(|f| {
            f["rule"].as_str() == Some("empty_project")
                && f["effective_severity"].as_str() == Some("warning")
        }),
        "verify should include an empty_project warning, got: {findings:?}",
    );

    // Terminal output: should render the warning (not "Clean: no findings")
    let terminal_output = cargo_bin_cmd!("supersigil")
        .args(["verify"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&terminal_output.stdout);
    assert!(
        stdout.contains("no documents found"),
        "terminal output should show empty_project warning, got: {stdout}",
    );
    assert!(
        !stdout.contains("Clean: no findings"),
        "terminal should not say Clean when there are warnings, got: {stdout}",
    );
}

/// Verify that the binary does not panic (exit 101) when stdout is a broken pipe.
///
/// Agents commonly pipe through `head` or `2>&1 | head`, which closes the pipe
/// early. The binary should exit cleanly, not panic from a `BrokenPipe` error in
/// the error handler.
///
/// To trigger the bug, the JSON output must exceed the OS pipe buffer (`~64KB`),
/// so we generate many documents with evidence to produce a large report.
#[test]
fn broken_pipe_does_not_panic() {
    use std::fmt::Write;
    use std::process::{Command, Stdio};

    let dir = TempDir::new().unwrap();

    // Generate enough documents and evidence to produce >64KB of JSON output.
    let mut config = String::from("paths = [\"specs/**/*.mdx\"]\n");
    config.push_str("\n[ecosystem.rust]\n");
    fs::write(dir.path().join("supersigil.toml"), &config).unwrap();
    fs::create_dir_all(dir.path().join("specs")).unwrap();
    fs::create_dir_all(dir.path().join("tests")).unwrap();

    // Create 30 requirement documents, each with 5 criteria and file-glob evidence.
    for i in 0..30 {
        let feature = format!("feat-{i:03}");
        let feature_dir = dir.path().join("specs").join(&feature);
        fs::create_dir_all(&feature_dir).unwrap();

        let mut criteria = String::new();
        for j in 0..5 {
            write!(
                criteria,
                "  <Criterion id=\"ac-{j}\">\n    \
                 Acceptance criterion {j} for feature {i}\n    \
                 <VerifiedBy strategy=\"file-glob\" paths=\"tests/{feature}_test.rs\" />\n  \
                 </Criterion>\n"
            )
            .unwrap();
        }

        common::write_mdx(
            dir.path(),
            &format!("specs/{feature}/{feature}.mdx"),
            &format!("{feature}/req"),
            Some("requirements"),
            Some("approved"),
            &format!("<AcceptanceCriteria>\n{criteria}</AcceptanceCriteria>"),
        );

        // Create a matching test file so evidence is discovered
        fs::write(
            dir.path().join("tests").join(format!("{feature}_test.rs")),
            format!("fn test_{feature}() {{}}\n"),
        )
        .unwrap();
    }

    let bin = assert_cmd::cargo::cargo_bin("supersigil");
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "{} verify --format json 2>&1 | head -1; exit ${{PIPESTATUS[0]}}",
            bin.display()
        ))
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to run pipeline");

    let code = output.status.code().unwrap_or(-1);

    assert_ne!(
        code, 101,
        "binary panicked (exit 101) on broken pipe — should exit cleanly"
    );
}
