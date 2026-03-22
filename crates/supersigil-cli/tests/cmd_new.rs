mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use supersigil_rust::verifies;
use tempfile::TempDir;

fn setup_multi_project(dir: &std::path::Path) {
    fs::write(
        dir.join("supersigil.toml"),
        r#"
[projects.workspace]
paths = ["specs/**/*.md"]

[projects.cli]
paths = ["crates/my-cli/specs/**/*.md"]
"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("specs")).unwrap();
    fs::create_dir_all(dir.join("crates/my-cli/specs")).unwrap();
}

/// task-7-2: Generated requirements template must pass lint.
#[verifies("authoring-commands/req#req-2-3", "authoring-commands/req#req-3-1")]
#[test]
fn new_requirements_passes_lint() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    // Generate a requirements scaffold
    cargo_bin_cmd!("supersigil")
        .args(["new", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // The generated file must pass lint
    cargo_bin_cmd!("supersigil")
        .args(["lint"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// task-7-2: Generated tasks template must pass lint.
#[verifies("authoring-commands/req#req-3-1")]
#[test]
fn new_tasks_passes_lint() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "tasks", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    cargo_bin_cmd!("supersigil")
        .args(["lint"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// task-7-3: Generated design template must not break graph loading.
#[test]
fn new_design_does_not_break_graph() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "design", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // ls requires a working graph — must not fail with broken ref
    cargo_bin_cmd!("supersigil")
        .args(["ls"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// Design template with existing req fills in Implements ref.
#[verifies("authoring-commands/req#req-3-3")]
#[test]
fn new_design_with_existing_req_fills_implements() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    // Create a requirements doc first
    cargo_bin_cmd!("supersigil")
        .args(["new", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Now create a design doc — should detect the req file
    cargo_bin_cmd!("supersigil")
        .args(["new", "design", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // The design file should have a filled-in Implements ref
    let design_content =
        std::fs::read_to_string(tmp.path().join("specs/auth/auth.design.md")).unwrap();
    assert!(
        design_content.contains(r#"<Implements refs="auth/req" />"#),
        "design should have filled Implements ref, got:\n{design_content}"
    );

    // Graph must load successfully (Implements ref is valid)
    cargo_bin_cmd!("supersigil")
        .args(["ls"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// In multi-project mode, --project places file under the project's spec directory.
#[test]
fn new_with_project_places_file_in_project_dir() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "cli", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // File should be under the cli project's spec directory
    let expected = tmp.path().join("crates/my-cli/specs/auth/auth.req.md");
    assert!(
        expected.is_file(),
        "expected file at {}",
        expected.display()
    );

    // Must pass lint
    cargo_bin_cmd!("supersigil")
        .args(["lint"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// --project with workspace project uses root specs/.
#[test]
fn new_with_workspace_project_uses_root_specs() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "workspace", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let expected = tmp.path().join("specs/auth/auth.req.md");
    assert!(
        expected.is_file(),
        "expected file at {}",
        expected.display()
    );
}

/// --project with unknown project name errors.
#[test]
fn new_with_unknown_project_errors() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "nonexistent", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("nonexistent").from_utf8());
}

/// --project in single-project mode errors.
#[verifies("authoring-commands/req#req-2-7")]
#[test]
fn new_with_project_in_single_project_mode_errors() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "foo", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .failure();
}

/// Omitting --project in multi-project mode errors.
#[test]
fn new_without_project_in_multi_project_mode_errors() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("--project").from_utf8());
}

/// Design template with --project detects sibling req in correct directory.
#[test]
fn new_design_with_project_detects_sibling_req() {
    let tmp = TempDir::new().unwrap();
    setup_multi_project(tmp.path());

    // Create req in the cli project
    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "cli", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Create design in the cli project — should find sibling req
    cargo_bin_cmd!("supersigil")
        .args(["new", "--project", "cli", "design", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let design_content =
        std::fs::read_to_string(tmp.path().join("crates/my-cli/specs/auth/auth.design.md"))
            .unwrap();
    assert!(
        design_content.contains(r#"<Implements refs="auth/req" />"#),
        "design should have filled Implements ref, got:\n{design_content}"
    );
}

/// Custom document type configured in supersigil.toml is accepted by `new`.
#[verifies("authoring-commands/req#req-2-2")]
#[test]
fn new_accepts_custom_doc_type_from_config() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("supersigil.toml"),
        r#"
paths = ["specs/**/*.md"]

[documents.types.narrative]
status = ["draft", "approved"]
"#,
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("specs")).unwrap();

    // `new narrative` should succeed without warnings because "narrative"
    // is a configured custom type.
    cargo_bin_cmd!("supersigil")
        .args(["new", "narrative", "onboarding"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("unknown document type").not());

    // The generated file should exist with the correct type in frontmatter.
    let content =
        fs::read_to_string(tmp.path().join("specs/onboarding/onboarding.narrative.md")).unwrap();
    assert!(
        content.contains("type: narrative"),
        "frontmatter should contain custom type, got:\n{content}"
    );
}

/// Unknown document type causes `new` to fail fast with an error listing known types.
#[verifies("authoring-commands/req#req-2-2")]
#[test]
fn new_rejects_unknown_doc_type() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "bogustype", "my-feature"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "unknown document type 'bogustype'",
        ))
        .stderr(predicates::str::contains("requirements"));

    // No file should have been created.
    assert!(
        !tmp.path().join("specs/my-feature").exists(),
        "no file should be created for unknown document type"
    );
}

/// After successful creation, `new` prints path to stdout and lint hint to stderr.
#[verifies("authoring-commands/req#req-2-4")]
#[test]
fn new_prints_path_to_stdout_and_lint_hint_to_stderr() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["new", "requirements", "billing"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("specs/billing/billing.req.md"),
        "stdout should contain the generated file path, got: {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("lint"),
        "stderr should contain a lint hint, got: {stderr}"
    );
}

/// `supersigil new adr` creates a file with type: adr and status: draft.
#[verifies("decision-components/req#req-4-2")]
#[test]
fn new_adr_produces_correct_frontmatter() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "adr", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let content = fs::read_to_string(tmp.path().join("specs/auth/auth.adr.md")).unwrap();
    assert!(
        content.contains("type: adr"),
        "frontmatter should contain type: adr, got:\n{content}"
    );
    assert!(
        content.contains("status: draft"),
        "frontmatter should contain status: draft, got:\n{content}"
    );
    assert!(
        content.contains("id: auth/adr"),
        "frontmatter should contain id: auth/adr, got:\n{content}"
    );
}

/// `supersigil new adr` produces a lint-clean document.
#[verifies("decision-components/req#req-4-3")]
#[test]
fn new_adr_passes_lint() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "adr", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    cargo_bin_cmd!("supersigil")
        .args(["lint"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// When a requirements doc exists, the ADR scaffold includes a References link.
#[test]
fn new_adr_with_existing_req_includes_references() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    // Create a requirements doc first
    cargo_bin_cmd!("supersigil")
        .args(["new", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Now create an ADR doc — should detect the req file
    cargo_bin_cmd!("supersigil")
        .args(["new", "adr", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let content = fs::read_to_string(tmp.path().join("specs/auth/auth.adr.md")).unwrap();
    assert!(
        content.contains(r#"<References refs="auth/req" />"#),
        "adr should include References link to req, got:\n{content}"
    );

    // Graph must load successfully
    cargo_bin_cmd!("supersigil")
        .args(["ls"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

/// When no requirements doc exists, the ADR scaffold has a commented-out References placeholder.
#[test]
fn new_adr_without_req_has_commented_references() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "adr", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let content = fs::read_to_string(tmp.path().join("specs/auth/auth.adr.md")).unwrap();
    // Should not have an active References component
    assert!(
        !content.contains(r#"<References refs="auth/req" />"#),
        "adr without req should not have active References, got:\n{content}"
    );
}

/// `adr` is recognized as a built-in document type (not rejected as unknown).
#[test]
fn new_adr_is_recognized_as_builtin_type() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["new", "adr", "my-feature"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("unknown document type").not());
}

/// Scaffolds include type-appropriate placeholder sections.
#[verifies("authoring-commands/req#req-3-4")]
#[test]
fn new_scaffolds_include_type_appropriate_placeholders() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    // Generate a requirements doc and check for AcceptanceCriteria placeholder.
    cargo_bin_cmd!("supersigil")
        .args(["new", "requirements", "auth"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let req_content = fs::read_to_string(tmp.path().join("specs/auth/auth.req.md")).unwrap();
    assert!(
        req_content.contains("AcceptanceCriteria"),
        "requirements scaffold should contain AcceptanceCriteria, got:\n{req_content}"
    );

    // Generate a design doc and check for Architecture placeholder.
    cargo_bin_cmd!("supersigil")
        .args(["new", "design", "billing"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let design_content =
        fs::read_to_string(tmp.path().join("specs/billing/billing.design.md")).unwrap();
    assert!(
        design_content.contains("Architecture"),
        "design scaffold should contain Architecture section, got:\n{design_content}"
    );
}
