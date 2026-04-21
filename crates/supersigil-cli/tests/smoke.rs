//! End-to-end smoke test: import kiro specs, then run ls/verify/context/plan.

mod common;

use std::fs;
use std::path::Path;

use assert_cmd::assert::OutputAssertExt;
use common::supersigil_cmd;
use tempfile::TempDir;

const PARSEABLE_REQUIREMENTS: &str = r"# Requirements Document: Login

### Requirement 1: Authenticate User
**User Story:** As a user, I want to sign in so I can access my account.
#### Acceptance Criteria
1. Given valid credentials, sign-in succeeds.
";

fn write_feature_requirements(specs_dir: &Path, feature: &str, body: &str) {
    let feature_dir = specs_dir.join(feature);
    fs::create_dir_all(&feature_dir).unwrap();
    fs::write(feature_dir.join("requirements.md"), body).unwrap();
}

#[test]
fn dogfooding_pipeline() {
    let project = TempDir::new().unwrap();
    let specs_dir = project.path().join(".kiro/specs");
    write_feature_requirements(&specs_dir, "auth-login", PARSEABLE_REQUIREMENTS);
    let out_dir = project.path().join("specs");

    // Step 1: Import kiro specs
    supersigil_cmd()
        .args([
            "import",
            "--from",
            "kiro",
            "--output-dir",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(project.path())
        .assert()
        .success();

    // Step 2: Create a supersigil.toml pointing at the imported specs
    fs::write(
        project.path().join("supersigil.toml"),
        "paths = [\"specs/**/*.md\"]\n",
    )
    .unwrap();

    // Step 3: Verify should pass (imported docs have draft status, so findings are non-blocking)
    supersigil_cmd()
        .args(["verify"])
        .current_dir(project.path())
        .assert()
        .success();

    // Step 4: ls should list documents
    let ls_output = supersigil_cmd()
        .args(["ls", "--format", "json"])
        .current_dir(project.path())
        .output()
        .unwrap();

    assert!(ls_output.status.success());
    let ls_json: serde_json::Value = serde_json::from_slice(&ls_output.stdout).expect("ls JSON");
    let docs = ls_json["documents"].as_array().expect("documents array");
    assert!(!docs.is_empty(), "expected imported docs to show in ls");

    // Step 5: context on first document should succeed
    let first_id = docs[0]["id"].as_str().unwrap();
    supersigil_cmd()
        .args(["context", first_id])
        .current_dir(project.path())
        .assert()
        .success();

    // Step 6: plan (all) should succeed
    supersigil_cmd()
        .args(["plan", "--format", "json"])
        .current_dir(project.path())
        .assert()
        .success();
}
