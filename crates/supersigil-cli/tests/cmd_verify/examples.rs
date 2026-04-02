use super::*;

#[verifies("executable-examples/req#req-4-2")]
#[test]
fn verify_terminal_reports_example_progress_on_clean_run() {
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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "terminal verify output should not emit an extra clean summary to stderr, got: {stderr}",
    );
}

#[verifies("executable-examples/req#req-4-3")]
#[test]
fn verify_terminal_reports_example_progress_for_failures() {
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
}

#[verifies("executable-examples/req#req-4-4")]
#[test]
fn verify_skips_examples_when_structural_errors_exist() {
    let tmp = TempDir::new().unwrap();
    // Set up a project with examples AND a structural error:
    // a <VerifiedBy> at document root (outside a Criterion) triggers
    // InvalidVerifiedByPlacement, which is an Error-severity structural finding.
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/mixed.md",
        "mixed/req",
        Some("requirements"),
        Some("approved"),
        r#"```supersigil-xml
<VerifiedBy strategy="file-glob" paths="specs/mixed.md" />

<AcceptanceCriteria>
  <Criterion id="crit-1">
    Has evidence
    <VerifiedBy strategy="file-glob" paths="specs/mixed.md" />
  </Criterion>
</AcceptanceCriteria>

<Example
  id="should-not-run"
  lang="sh"
  runner="sh"
  verifies="mixed/req#crit-1"
>
  <Expected status="0" contains="skipped" />
</Example>
```

```sh supersigil-ref=should-not-run
echo "this should be skipped"
```"#,
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
fn verify_update_snapshots_accepts_flag() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/snap.md",
        "snap/req",
        Some("requirements"),
        Some("approved"),
        r#"```supersigil-xml
<AcceptanceCriteria>
  <Criterion id="snap-1">snapshot test</Criterion>
</AcceptanceCriteria>

<Example id="snap-ex" lang="sh" runner="sh" verifies="snap/req#snap-1">
  echo "new output"
  <Expected status="0" format="snapshot">old output</Expected>
</Example>
```"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--update-snapshots", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty(),
        "verify --update-snapshots should produce output, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    // After --update-snapshots, the spec file should have "old output" replaced
    // with the actual output ("new output") from the example execution.
    let updated = fs::read_to_string(tmp.path().join("specs/snap.md")).unwrap();
    assert!(
        updated.contains("new output"),
        "snapshot rewrite should replace inline Expected content with actual output, got:\n{updated}",
    );
    assert!(
        !updated.contains("old output"),
        "snapshot rewrite should remove the old Expected content, got:\n{updated}",
    );
}

#[verifies("executable-examples/req#req-4-8")]
#[test]
fn verify_terminal_non_blocking_failed_examples_render_multiline_details() {
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
}
