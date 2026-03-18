mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use supersigil_rust::verifies;
use tempfile::TempDir;

#[test]
fn lint_clean_project_exits_zero() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_mdx(
        tmp.path(),
        "specs/doc.mdx",
        "test/doc",
        None,
        None,
        "# Test\n",
    );

    cargo_bin_cmd!("supersigil")
        .arg("lint")
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn lint_invalid_file_exits_one() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    std::fs::write(
        tmp.path().join("specs/bad.mdx"),
        "---\nsupersigil:\n  id: bad\n",
    )
    .unwrap();

    cargo_bin_cmd!("supersigil")
        .arg("lint")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("error"));
}

#[test]
fn lint_empty_project_exits_zero_with_warning() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .arg("lint")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("no documents found"));
}

#[verifies("cli-runtime/req#req-4-2")]
#[test]
fn lint_no_config_exits_one_with_stderr() {
    let tmp = TempDir::new().unwrap();

    cargo_bin_cmd!("supersigil")
        .arg("lint")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("config"));
}

#[test]
fn lint_stdout_stderr_discipline() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    std::fs::write(
        tmp.path().join("specs/bad.mdx"),
        "---\nsupersigil:\n  id: bad\n",
    )
    .unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .arg("lint")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error"), "lint errors should be on stdout");
}

#[test]
fn lint_summary_counts_files_not_errors() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    // Two Criterion components missing required `id` → 2 errors, 1 file
    common::write_mdx(
        tmp.path(),
        "specs/bad.mdx",
        "bad/doc",
        None,
        None,
        "<Criterion>text one</Criterion>\n\n<Criterion>text two</Criterion>\n",
    );

    cargo_bin_cmd!("supersigil")
        .arg("lint")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("1 files checked, 2 error(s)"));
}
