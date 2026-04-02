use super::*;

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

    // 1. --project covered -> should be clean (evidence exists)
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

    // 2. --project uncovered -> should have errors (no evidence)
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

    // 3. No filter -> should have errors from uncovered project
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
