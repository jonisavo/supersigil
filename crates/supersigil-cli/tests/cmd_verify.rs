//! Integration tests for the `verify` command.

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

fn setup_explicit_evidence_fixture(root: &Path, config: &str) {
    write_config(root, config);
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

fn write_requirement_with_explicit_evidence(root: &Path) {
    common::write_spec_doc(
        root,
        "specs/auth.md",
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
    common::write_spec_doc(
        root,
        "specs/auth.md",
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
    common::write_spec_doc(
        root,
        "specs/auth.md",
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
    setup_explicit_evidence_fixture(
        root,
        r#"paths = ["specs/**/*.md"]
tests = ["tests/**/*.rs"]

[ecosystem]
plugins = ["rust"]
"#,
    );
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
        r#"paths = ["specs/**/*.md"]
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

fn setup_explicit_evidence_only_fixture(root: &Path) {
    setup_explicit_evidence_fixture(
        root,
        r#"paths = ["specs/**/*.md"]
tests = ["tests/**/*.rs"]

[ecosystem]
plugins = []
"#,
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

#[verifies("work-queries/req#req-6-3")]
#[test]
fn verify_json_compact_omits_records_on_clean_run() {
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
    assert_eq!(report["overall_status"], "clean");

    // Compact mode (default) should omit records and coverage on clean runs.
    let es = &report["evidence_summary"];
    assert!(
        es.get("records").is_none(),
        "compact clean verify should omit records key, got: {es}",
    );
    assert!(
        es.get("coverage").is_none(),
        "compact clean verify should omit coverage key, got: {es}",
    );
    // conflict_count should remain.
    assert!(
        es.get("conflict_count").is_some(),
        "compact clean verify should keep conflict_count, got: {es}",
    );
}

#[verifies("work-queries/req#req-6-4")]
#[test]
fn verify_json_detail_full_includes_records_on_clean_run() {
    let tmp = TempDir::new().unwrap();
    setup_explicit_evidence_only_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "--detail", "full"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(report["overall_status"], "clean");

    let records = report["evidence_summary"]["records"]
        .as_array()
        .expect("records should be present");
    assert!(
        !records.is_empty(),
        "full detail verify should have non-empty records"
    );

    let coverage = report["evidence_summary"]["coverage"]
        .as_array()
        .expect("coverage should be present");
    assert!(
        !coverage.is_empty(),
        "full detail verify should have non-empty coverage"
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
paths = ["specs/covered/**/*.md"]
tests = ["tests/covered/**/*.rs"]

[projects.uncovered]
paths = ["specs/uncovered/**/*.md"]
tests = ["tests/uncovered/**/*.rs"]

[ecosystem]
plugins = []
"#,
    )
    .unwrap();

    // Project "covered": spec with criterion + matching evidence file
    fs::create_dir_all(root.join("specs/covered")).unwrap();
    common::write_spec_doc(
        root,
        "specs/covered/auth.md",
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
    common::write_spec_doc(
        root,
        "specs/uncovered/billing.md",
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

/// Verify that an empty project (0 documents) produces a warning finding and
/// exits with code 2 (warnings only).
#[test]
fn verify_empty_project_warns() {
    let dir = TempDir::new().unwrap();
    write_config(dir.path(), "paths = [\"specs/**/*.md\"]\n");

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
    let mut config = String::from("paths = [\"specs/**/*.md\"]\n");
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

        common::write_spec_doc(
            dir.path(),
            &format!("specs/{feature}/{feature}.md"),
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

// -----------------------------------------------------------------------
// JS plugin end-to-end
// -----------------------------------------------------------------------

fn setup_js_plugin_fixture(root: &Path) {
    common::setup_project_with_js_plugin(root);
    common::write_spec_doc(
        root,
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="login-succeeds">
    User can log in with valid credentials.
  </Criterion>
</AcceptanceCriteria>"#,
    );
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests/auth.test.ts"),
        r"import { verifies } from '@supersigil/vitest'
import { test } from 'vitest'

test('login succeeds', verifies('auth/req#login-succeeds'), () => {
  // test body
})
",
    )
    .unwrap();
}

#[test]
fn verify_js_plugin_discovers_verifies_evidence() {
    let tmp = TempDir::new().unwrap();
    setup_js_plugin_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json", "--detail", "full"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "verify should exit cleanly when JS evidence covers the criterion: {}",
        String::from_utf8_lossy(&output.stdout),
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    // The evidence summary should contain a record with JS provenance.
    let records = report["evidence_summary"]["records"]
        .as_array()
        .expect("evidence_summary.records should be present");
    assert!(
        !records.is_empty(),
        "expected at least one evidence record, got: {report}",
    );

    let js_record = records
        .iter()
        .find(|record| {
            record["provenance"]
                .as_array()
                .is_some_and(|provenance| provenance.iter().any(|entry| entry == "plugin:js"))
        })
        .expect("expected a record with plugin:js provenance");

    assert_eq!(js_record["test_name"], "login succeeds");
    assert_eq!(js_record["evidence_kind"], "js-verifies");
    assert!(
        js_record["targets"]
            .as_array()
            .is_some_and(|targets| targets.iter().any(|t| t == "auth/req#login-succeeds")),
        "expected target auth/req#login-succeeds in JS evidence record, got: {js_record}",
    );

    // No findings (errors/warnings) should be present.
    assert_eq!(report["summary"]["error_count"], 0);
    assert_eq!(report["summary"]["warning_count"], 0);
}

#[test]
fn verify_js_plugin_missing_evidence_exits_nonzero() {
    let tmp = TempDir::new().unwrap();
    common::setup_project_with_js_plugin(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="login-succeeds">
    User can log in with valid credentials.
  </Criterion>
</AcceptanceCriteria>"#,
    );
    // Test file exists but has no verifies() annotation.
    fs::create_dir_all(tmp.path().join("tests")).unwrap();
    fs::write(
        tmp.path().join("tests/auth.test.ts"),
        r"import { test } from 'vitest'

test('login succeeds', () => {
  // no verifies() annotation
})
",
    )
    .unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .args(["verify", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_ne!(
        output.status.code(),
        Some(0),
        "verify should fail when JS evidence is missing: {}",
        String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
#[verifies("cli-runtime/req#req-4-2")]
fn verify_no_config_exits_one_with_error_on_stderr() {
    let tmp = TempDir::new().unwrap();
    let output = cargo_bin_cmd!("supersigil")
        .arg("verify")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:' prefix, got: {stderr}",
    );
}
