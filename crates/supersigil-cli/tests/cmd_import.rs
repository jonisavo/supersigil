//! Integration tests for the `import` command.

mod common;

use std::fs;
use std::path::Path;

use assert_cmd::assert::OutputAssertExt;
use common::supersigil_cmd;
use predicates::prelude::*;
use supersigil_rust::verifies;
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

#[verifies("kiro-import/req#req-4-2")]
#[test]
fn import_dry_run_succeeds_with_tempdir_fixture() {
    let project = TempDir::new().unwrap();
    let specs_dir = project.path().join(".kiro/specs");
    write_feature_requirements(&specs_dir, "auth-login", PARSEABLE_REQUIREMENTS);

    supersigil_cmd()
        .args([
            "import",
            "--from",
            "kiro",
            "--dry-run",
            "--output-dir",
            project.path().join("out").to_str().unwrap(),
        ])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("features_processed"));
}

#[test]
fn import_write_mode_creates_files() {
    let project = TempDir::new().unwrap();
    let specs_dir = project.path().join(".kiro/specs");
    write_feature_requirements(&specs_dir, "auth-login", PARSEABLE_REQUIREMENTS);
    let out_dir = project.path().join("out");

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

    // Verify at least one md file was created
    let has_md = walkdir::WalkDir::new(&out_dir)
        .into_iter()
        .filter_map(Result::ok)
        .any(|e| e.path().extension().is_some_and(|ext| ext == "md"));
    assert!(has_md, "expected md files in output dir");
}

#[test]
fn import_unsupported_source_fails() {
    // clap rejects unknown --from values before our code runs
    supersigil_cmd()
        .args(["import", "--from", "notion"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn import_missing_kiro_dir_fails() {
    let tmp = TempDir::new().unwrap();

    supersigil_cmd()
        .args(["import", "--from", "kiro"])
        .current_dir(tmp.path())
        .assert()
        .failure();
}

#[test]
fn import_source_dir_flag_overrides_default_location() {
    let project = TempDir::new().unwrap();
    let source_dir = project.path().join("custom/specs");
    write_feature_requirements(&source_dir, "billing", PARSEABLE_REQUIREMENTS);

    supersigil_cmd()
        .args([
            "import",
            "--from",
            "kiro",
            "--dry-run",
            "--source-dir",
            source_dir.to_str().unwrap(),
            "--output-dir",
            project.path().join("out").to_str().unwrap(),
        ])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run: 1 documents planned"));
}

#[test]
fn import_source_dir_env_is_respected() {
    let project = TempDir::new().unwrap();
    let source_dir = project.path().join("custom/specs");
    write_feature_requirements(&source_dir, "billing", PARSEABLE_REQUIREMENTS);

    supersigil_cmd()
        .args([
            "import",
            "--from",
            "kiro",
            "--dry-run",
            "--output-dir",
            project.path().join("out").to_str().unwrap(),
        ])
        .env("SUPERSIGIL_IMPORT_SOURCE_DIR", source_dir.to_str().unwrap())
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run: 1 documents planned"));
}

#[test]
fn import_diagnostics_use_display_format_not_debug() {
    let project = TempDir::new().unwrap();
    let specs_dir = project.path().join(".kiro/specs");
    write_feature_requirements(
        &specs_dir,
        "missing-sections",
        "# Requirements Document: Empty\n",
    );

    supersigil_cmd()
        .args(["import", "--from", "kiro", "--dry-run"])
        .current_dir(project.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("warning:"))
        .stderr(predicate::str::contains("Warning {").not());
}

/// Writing to an existing file without `--force` fails with a conflict error.
#[verifies("kiro-import/req#req-3-5")]
#[test]
fn import_write_conflict_without_force_fails() {
    let project = TempDir::new().unwrap();
    let specs_dir = project.path().join(".kiro/specs");
    write_feature_requirements(&specs_dir, "auth-login", PARSEABLE_REQUIREMENTS);
    let out_dir = project.path().join("out");

    // First import succeeds — creates the files.
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

    // Second import without --force should fail because files exist.
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
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

/// Writing to an existing file with `--force` succeeds by overwriting.
#[verifies("kiro-import/req#req-3-5")]
#[test]
fn import_write_conflict_with_force_overwrites() {
    let project = TempDir::new().unwrap();
    let specs_dir = project.path().join(".kiro/specs");
    write_feature_requirements(&specs_dir, "auth-login", PARSEABLE_REQUIREMENTS);
    let out_dir = project.path().join("out");

    // First import.
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

    // Second import with --force should succeed.
    supersigil_cmd()
        .args([
            "import",
            "--from",
            "kiro",
            "--force",
            "--output-dir",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(project.path())
        .assert()
        .success();
}

#[test]
fn import_summary_shows_ambiguity_breakdown() {
    let project = TempDir::new().unwrap();
    let specs_dir = project.path().join(".kiro/specs");

    // Create a feature with a design doc but no requirements doc
    // This triggers a missing_context ambiguity marker.
    let feature_dir = specs_dir.join("broken-refs");
    fs::create_dir_all(&feature_dir).unwrap();
    fs::write(
        feature_dir.join("design.md"),
        "# Design: Broken Refs\n\n## Overview\n\nSome content.\n",
    )
    .unwrap();

    supersigil_cmd()
        .args([
            "import",
            "--from",
            "kiro",
            "--dry-run",
            "--output-dir",
            project.path().join("out").to_str().unwrap(),
        ])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("missing_context:"));
}

/// Write mode prints a `supersigil verify` next-step hint on stderr.
#[verifies("kiro-import/req#req-4-3")]
#[test]
fn import_write_mode_prints_lint_hint() {
    let project = TempDir::new().unwrap();
    let specs_dir = project.path().join(".kiro/specs");
    write_feature_requirements(&specs_dir, "auth-login", PARSEABLE_REQUIREMENTS);

    // Create a supersigil.toml so the hint says "supersigil verify" (not "supersigil init").
    fs::write(
        project.path().join("supersigil.toml"),
        "paths = [\"specs/**/*.md\"]\n",
    )
    .unwrap();

    let output = supersigil_cmd()
        .args([
            "import",
            "--from",
            "kiro",
            "--output-dir",
            project.path().join("specs").to_str().unwrap(),
        ])
        .current_dir(project.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("verify"),
        "stderr should contain a verify hint after write mode, got: {stderr}"
    );
}

#[test]
fn import_check_finds_markers_in_imported_files() {
    let project = TempDir::new().unwrap();
    let specs_dir = project.path().join(".kiro/specs");

    // Create a feature with design but no requirements — produces a missing_context marker
    let feature_dir = specs_dir.join("check-test");
    fs::create_dir_all(&feature_dir).unwrap();
    fs::write(
        feature_dir.join("design.md"),
        "# Design: Check Test\n\n## Overview\n\nContent.\n",
    )
    .unwrap();

    let out_dir = project.path().join("specs");

    // First, import the files (this writes markers)
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

    // Then, check for markers
    supersigil_cmd()
        .args([
            "import",
            "--from",
            "kiro",
            "--check",
            "--output-dir",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(project.path())
        .assert()
        .failure() // non-zero exit when markers found
        .stdout(predicate::str::contains("import TODO"))
        .stdout(predicate::str::contains("missing_context:"));
}

#[test]
fn import_check_succeeds_when_no_markers() {
    let project = TempDir::new().unwrap();
    let out_dir = project.path().join("specs");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(
        out_dir.join("clean.md"),
        "---\nsupersigil:\n  id: test\n  type: requirements\n---\n\n# Clean doc\n",
    )
    .unwrap();

    supersigil_cmd()
        .args([
            "import",
            "--from",
            "kiro",
            "--check",
            "--output-dir",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("0 import TODOs"));
}
