mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use supersigil_rust::verifies;
use tempfile::TempDir;

/// task-7-1: Criterion nested inside `AcceptanceCriteria` must be counted.
#[test]
fn status_counts_nested_criteria() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("draft"),
        "<AcceptanceCriteria>\n  <Criterion id=\"ac-1\">\n    Must log in\n  </Criterion>\n  <Criterion id=\"ac-2\">\n    Must log out\n  </Criterion>\n</AcceptanceCriteria>",
    );

    cargo_bin_cmd!("supersigil")
        .args(["status", "--format", "json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"targets_total\": 2"));
}

/// Per-document status JSON includes per-criterion `verified_by` labels when
/// `<VerifiedBy>` components are nested inside `<Criterion>`.
#[test]
fn status_per_document_shows_verified_by_per_criterion() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("draft"),
        r#"<AcceptanceCriteria>
  <Criterion id="ac-1">
    Must log in
    <VerifiedBy strategy="tag" tag="auth:login" />
  </Criterion>
  <Criterion id="ac-2">
    Must log out
    <VerifiedBy strategy="file-glob" paths="tests/logout_test.rs" />
  </Criterion>
</AcceptanceCriteria>"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["status", "auth/req", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    let criteria = json["criteria"]
        .as_array()
        .expect("criteria should be array");
    assert_eq!(criteria.len(), 2);

    // ac-1 has tag-based VerifiedBy
    let ac1 = &criteria[0];
    assert_eq!(ac1["id"], "ac-1");
    let ac1_vb = ac1["verified_by"]
        .as_array()
        .expect("verified_by should be array");
    assert_eq!(ac1_vb.len(), 1);
    assert_eq!(ac1_vb[0], "tag:auth:login");

    // ac-2 has file-glob-based VerifiedBy
    let ac2 = &criteria[1];
    assert_eq!(ac2["id"], "ac-2");
    let ac2_vb = ac2["verified_by"]
        .as_array()
        .expect("verified_by should be array");
    assert_eq!(ac2_vb.len(), 1);
    assert_eq!(ac2_vb[0], "file-glob:tests/logout_test.rs");
}

/// Per-document status JSON omits `verified_by` key for criteria without
/// `VerifiedBy` components (via `skip_serializing_if`).
#[test]
fn status_per_document_omits_empty_verified_by() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("draft"),
        "<AcceptanceCriteria>\n  <Criterion id=\"ac-1\">\n    Must log in\n  </Criterion>\n</AcceptanceCriteria>",
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["status", "auth/req", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    let criteria = json["criteria"]
        .as_array()
        .expect("criteria should be array");
    assert_eq!(criteria.len(), 1);

    // No VerifiedBy → key should be absent (skip_serializing_if)
    assert!(
        criteria[0].get("verified_by").is_none(),
        "verified_by should be omitted when empty, got: {:?}",
        criteria[0],
    );
}

/// Terminal output shows per-criterion "verified by:" lines.
#[test]
fn status_terminal_shows_verified_by_per_criterion() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("draft"),
        r#"<AcceptanceCriteria>
  <Criterion id="ac-1">
    Must log in
    <VerifiedBy strategy="tag" tag="auth:login" />
  </Criterion>
</AcceptanceCriteria>"#,
    );

    cargo_bin_cmd!("supersigil")
        .args(["status", "auth/req"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("verified by: tag:auth:login"));
}

/// When the Rust plugin is enabled but finds Rust files with zero test items,
/// the plugin failure warning must appear on stderr while stdout stays clean.
#[test]
fn status_plugin_failure_warning_on_stderr() {
    let tmp = TempDir::new().unwrap();
    common::setup_project_with_rust_plugin(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("draft"),
        "<AcceptanceCriteria>\n  <Criterion id=\"ac-1\">\n    Must log in\n  </Criterion>\n</AcceptanceCriteria>",
    );

    // Rust source with no test items triggers the plugin failure.
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src/lib.rs"), "pub fn hello() {}\n").unwrap();

    cargo_bin_cmd!("supersigil")
        .args(["status"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("plugin"))
        .stderr(predicate::str::contains("zero supported Rust test items"));
}

/// With --format json, stdout must be valid JSON even when plugin warnings
/// are emitted on stderr.
#[test]
fn status_json_stdout_clean_despite_plugin_warning() {
    let tmp = TempDir::new().unwrap();
    common::setup_project_with_rust_plugin(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("draft"),
        "<AcceptanceCriteria>\n  <Criterion id=\"ac-1\">\n    Must log in\n  </Criterion>\n</AcceptanceCriteria>",
    );

    // Rust source with no test items.
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src/lib.rs"), "pub fn helper() {}\n").unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .args(["status", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    // stderr has the plugin warning.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("zero supported Rust test items"),
        "stderr should contain plugin warning, got: {stderr}",
    );

    // stdout is valid JSON.
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert!(json.get("targets_total").is_some());
}

/// Set up a project where one Rust file parses successfully and another is
/// intentionally broken, so the plugin emits a recoverable diagnostic.
fn setup_partial_rust_warning_fixture(root: &std::path::Path) {
    common::setup_project_with_rust_plugin(root);
    common::write_spec_doc(
        root,
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("draft"),
        "<AcceptanceCriteria>\n  <Criterion id=\"ac-1\">\n    Must log in\n  </Criterion>\n</AcceptanceCriteria>",
    );

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

/// Recoverable Rust discovery issues should still surface as warnings through
/// the CLI reporting path when another file yields usable evidence.
#[test]
fn status_partial_rust_plugin_warning_on_stderr() {
    let tmp = TempDir::new().unwrap();
    setup_partial_rust_warning_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["status"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("plugin 'rust'"),
        "stderr should mention the rust plugin, got: {stderr}",
    );
    assert!(
        stderr.contains("skipping due to parse failure"),
        "stderr should contain the structured parse warning, got: {stderr}",
    );
    assert_eq!(
        stderr.matches("skipping due to parse failure").count(),
        1,
        "partial-warning path should emit the parse warning once, got: {stderr}",
    );
}

/// Recoverable Rust discovery warnings must not pollute JSON stdout.
#[test]
fn status_json_stdout_clean_despite_partial_rust_plugin_warning() {
    let tmp = TempDir::new().unwrap();
    setup_partial_rust_warning_fixture(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["status", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("skipping due to parse failure"),
        "stderr should contain the structured parse warning, got: {stderr}",
    );
    assert_eq!(
        stderr.matches("skipping due to parse failure").count(),
        1,
        "partial-warning path should emit the parse warning once, got: {stderr}",
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert!(json.get("targets_total").is_some());
}

/// When the ID argument doesn't match any document exactly but matches
/// multiple documents by prefix, `status` should aggregate them into a
/// project-style summary scoped to the matching documents.
#[verifies("verification-engine/req#req-7-3")]
#[test]
fn status_prefix_aggregates_matching_documents() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/feat/req.md",
        "feat/req",
        Some("requirements"),
        Some("approved"),
        "<AcceptanceCriteria>\n  <Criterion id=\"ac-1\">\n    First\n  </Criterion>\n</AcceptanceCriteria>",
    );
    common::write_spec_doc(
        tmp.path(),
        "specs/feat/design.md",
        "feat/design",
        Some("design"),
        Some("draft"),
        "<AcceptanceCriteria>\n  <Criterion id=\"dc-1\">\n    Second\n  </Criterion>\n</AcceptanceCriteria>",
    );
    // Unrelated document that should NOT appear in the prefix output.
    common::write_spec_doc(
        tmp.path(),
        "specs/other.md",
        "other/req",
        Some("requirements"),
        Some("draft"),
        "",
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["status", "feat", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    // Only the two feat/* documents should be included.
    assert_eq!(json["total_documents"], 2);
    assert_eq!(json["targets_total"], 2);
}

/// Prefix status in terminal mode should display which prefix was used.
#[verifies("verification-engine/req#req-7-3")]
#[test]
fn status_prefix_terminal_shows_prefix_header() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/feat/req.md",
        "feat/req",
        Some("requirements"),
        Some("approved"),
        "",
    );

    cargo_bin_cmd!("supersigil")
        .args(["status", "feat"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("feat"));
}

/// When the ID argument matches nothing — neither exact nor prefix — the
/// command should fail with a useful error message.
#[verifies("verification-engine/req#req-7-3")]
#[test]
fn status_no_match_fails_with_error() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec_doc(
        tmp.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("draft"),
        "",
    );

    cargo_bin_cmd!("supersigil")
        .args(["status", "nonexistent"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("no documents match"));
}
