//! Integration tests for the `render` command.

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use supersigil_rust::verifies;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// (a) Render output for a multi-document project matches expected JSON structure
// ---------------------------------------------------------------------------

#[test]
#[verifies("spec-rendering/req#req-1-5")]
fn render_multi_document_project_produces_json_array() {
    let dir = TempDir::new().unwrap();
    common::setup_project(dir.path());

    common::write_spec_doc(
        dir.path(),
        "specs/auth/req.md",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="c1">Users SHALL authenticate with OAuth2.</Criterion>
  <Criterion id="c2">Sessions SHALL expire after 30 minutes.</Criterion>
</AcceptanceCriteria>"#,
    );

    common::write_spec_doc(
        dir.path(),
        "specs/auth/design.md",
        "auth/design",
        Some("design"),
        Some("approved"),
        r#"<Implements refs="auth/req" />
<Task id="t1" status="open">Implement OAuth2 login.</Task>"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["render", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "render should succeed");

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("should produce valid JSON");
    let json = parsed["documents"]
        .as_array()
        .expect("should have documents array");

    assert_eq!(json.len(), 2, "should have two document results");

    // Each result should have the expected top-level fields.
    for doc_result in json {
        assert!(
            doc_result.get("document_id").is_some(),
            "each result should have document_id",
        );
        assert!(
            doc_result.get("stale").is_some(),
            "each result should have stale field",
        );
        assert!(
            doc_result.get("fences").is_some(),
            "each result should have fences array",
        );
        assert!(
            doc_result.get("edges").is_some(),
            "each result should have edges array",
        );
    }

    // Find the requirements doc and verify its structure.
    let req_doc = json
        .iter()
        .find(|d| d["document_id"] == "auth/req")
        .expect("should have auth/req");
    let fences = req_doc["fences"].as_array().unwrap();
    assert!(!fences.is_empty(), "auth/req should have fences");

    // The first fence should have components with nested children.
    let first_fence = &fences[0];
    assert!(
        first_fence.get("byte_range").is_some(),
        "fence should have byte_range",
    );
    let components = first_fence["components"].as_array().unwrap();
    assert!(!components.is_empty(), "fence should have components");

    // Check the design doc has edges.
    let design_doc = json
        .iter()
        .find(|d| d["document_id"] == "auth/design")
        .expect("should have auth/design");
    let edges = design_doc["edges"].as_array().unwrap();
    assert!(
        edges.iter().any(|e| e["kind"] == "implements"),
        "auth/design should have an implements edge",
    );
}

// ---------------------------------------------------------------------------
// (b) Render output includes verification status when verify data is available
// ---------------------------------------------------------------------------

#[test]
#[verifies("spec-rendering/req#req-1-5")]
fn render_includes_verification_status() {
    let dir = TempDir::new().unwrap();
    common::setup_project_with_rust_plugin_and_tests(dir.path(), "tests/**/*.rs", "");

    common::write_spec_doc(
        dir.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"<AcceptanceCriteria>
  <Criterion id="c1">
    Users SHALL authenticate.
    <VerifiedBy strategy="file-glob" paths="tests/auth_test.rs" />
  </Criterion>
  <Criterion id="c2">Sessions SHALL expire.</Criterion>
</AcceptanceCriteria>"#,
    );

    // Create a test file that matches the file-glob evidence.
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("tests/auth_test.rs"),
        "#[test] fn test_auth() { assert!(true); }\n",
    )
    .unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .args(["render", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "render should succeed");

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("should produce valid JSON");
    let json = parsed["documents"]
        .as_array()
        .expect("should have documents array");

    assert_eq!(json.len(), 1);
    let doc = &json[0];
    assert_eq!(doc["document_id"], "auth/req");

    // Walk component tree to find criteria with verification status.
    let fences = doc["fences"].as_array().unwrap();
    assert!(!fences.is_empty());

    let mut found_verified = false;
    let mut found_unverified = false;
    visit_components(fences, &mut |comp| {
        if comp["kind"] == "Criterion"
            && let Some(v) = comp.get("verification")
        {
            if v["state"] == "verified" {
                found_verified = true;
            } else if v["state"] == "unverified" {
                found_unverified = true;
            }
        }
    });

    assert!(
        found_verified,
        "should have a verified criterion (c1 with file-glob evidence)",
    );
    assert!(
        found_unverified,
        "should have an unverified criterion (c2 with no evidence)",
    );
}

// ---------------------------------------------------------------------------
// (c) Render with --format json flag produces valid JSON to stdout
// ---------------------------------------------------------------------------

#[test]
#[verifies("spec-rendering/req#req-1-5")]
fn render_format_json_produces_valid_json_stdout() {
    let dir = TempDir::new().unwrap();
    common::setup_project(dir.path());

    common::write_spec_doc(
        dir.path(),
        "specs/simple.md",
        "simple/req",
        Some("requirements"),
        Some("draft"),
        r#"<Criterion id="c1">Hello world.</Criterion>"#,
    );

    let output = cargo_bin_cmd!("supersigil")
        .args(["render", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "render should succeed");

    // stdout should be valid JSON.
    let stdout = String::from_utf8(output.stdout).expect("stdout should be valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert!(parsed.is_object(), "top-level JSON should be an object");
    assert!(
        parsed["documents"].is_array(),
        "should have documents array"
    );

    // stderr should have a summary message (similar to graph command).
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("Render:") || stderr.contains("document"),
        "stderr should have a summary, got: {stderr}",
    );
}

// ---------------------------------------------------------------------------
// Helper: recursively visit components in the JSON fence array.
// ---------------------------------------------------------------------------

fn visit_components(fences: &[serde_json::Value], visitor: &mut dyn FnMut(&serde_json::Value)) {
    for fence in fences {
        if let Some(components) = fence.get("components").and_then(|c| c.as_array()) {
            visit_component_tree(components, visitor);
        }
    }
}

fn visit_component_tree(
    components: &[serde_json::Value],
    visitor: &mut dyn FnMut(&serde_json::Value),
) {
    for comp in components {
        visitor(comp);
        if let Some(children) = comp.get("children").and_then(|c| c.as_array()) {
            visit_component_tree(children, visitor);
        }
    }
}
