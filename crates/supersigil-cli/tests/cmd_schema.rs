mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn schema_json_succeeds_without_parsing_specs() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("supersigil.toml"),
        r#"
paths = ["specs/**/*.mdx"]

[documents.types.requirement]
status = ["draft", "approved"]
"#,
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("specs")).unwrap();
    std::fs::write(tmp.path().join("specs/broken.mdx"), "---\nnot yaml").unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
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
    assert!(json.get("components").is_some());
    assert!(json.get("document_types").is_some());
    assert_eq!(json["document_types"]["requirement"]["status"][0], "draft");
}

#[test]
fn schema_yaml_format_outputs_valid_yaml() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "yaml"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let yaml: yaml_serde::Value =
        yaml_serde::from_str(&String::from_utf8_lossy(&output.stdout)).expect("valid YAML");
    assert!(yaml.get("components").is_some());
    assert!(yaml.get("document_types").is_some());
}

#[test]
fn schema_contains_builtin_components() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"Criterion\""))
        .stdout(predicate::str::contains("\"Task\""))
        .stdout(predicate::str::contains("\"VerifiedBy\""));
}

#[test]
fn schema_includes_builtin_document_types_for_minimal_config() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
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

    assert_eq!(
        json["document_types"]["requirements"]["status"],
        serde_json::json!(["draft", "review", "approved", "implemented"])
    );
    assert_eq!(
        json["document_types"]["design"]["status"],
        serde_json::json!(["draft", "review", "approved"])
    );
    assert_eq!(
        json["document_types"]["tasks"]["status"],
        serde_json::json!(["draft", "ready", "in-progress", "done"])
    );
}

#[test]
fn schema_merges_user_component_overrides() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("supersigil.toml"),
        r#"
paths = ["specs/**/*.mdx"]

[components.Task]
referenceable = false
target_component = "Criterion"

[components.Task.attributes.id]
required = true

[components.Task.attributes.owner]
required = false

[components.Task.attributes.depends]
required = false
list = true
"#,
    )
    .unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
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
    let task = &json["components"]["Task"];
    assert_eq!(task["target_component"], "Criterion");
    assert!(task.get("referenceable").is_none());
    assert!(task["attributes"].get("owner").is_some());
}

#[test]
fn schema_includes_configured_document_types() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("supersigil.toml"),
        r#"
paths = ["specs/**/*.mdx"]

[documents.types.requirement]
status = ["draft", "review", "approved"]
required_components = ["AcceptanceCriteria"]

[documents.types.design]
status = ["draft"]
"#,
    )
    .unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
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
    assert_eq!(
        json["document_types"]["requirement"]["required_components"][0],
        "AcceptanceCriteria"
    );
    assert_eq!(json["document_types"]["design"]["status"][0], "draft");
}

#[test]
fn schema_omits_default_empty_fields() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
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

    let acceptance_criteria = &json["components"]["AcceptanceCriteria"];
    assert!(acceptance_criteria.get("attributes").is_none());
    assert!(acceptance_criteria.get("referenceable").is_none());
    assert!(acceptance_criteria.get("target_component").is_none());

    let criterion_id = &json["components"]["Criterion"]["attributes"]["id"];
    assert_eq!(criterion_id["required"], true);
    assert!(criterion_id.get("list").is_none());
}

#[test]
fn schema_builtin_components_have_descriptions() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");

    for name in [
        "AcceptanceCriteria",
        "Criterion",
        "References",
        "VerifiedBy",
        "Implements",
        "DependsOn",
        "Task",
        "TrackedFiles",
    ] {
        assert!(
            json["components"][name].get("description").is_some(),
            "{name} should have description in schema output"
        );
    }
}

#[test]
fn schema_builtin_components_have_examples() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");

    for name in [
        "AcceptanceCriteria",
        "Criterion",
        "References",
        "VerifiedBy",
        "Implements",
        "DependsOn",
        "Task",
        "TrackedFiles",
    ] {
        let examples = &json["components"][name]["examples"];
        assert!(
            examples.is_array() && !examples.as_array().unwrap().is_empty(),
            "{name} should have examples in schema output"
        );
    }
}

#[test]
fn schema_builtin_document_types_have_descriptions() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");

    for name in ["requirements", "design", "tasks"] {
        assert!(
            json["document_types"][name].get("description").is_some(),
            "{name} should have description in schema output"
        );
    }
}

#[test]
fn schema_user_override_without_description_omits_it() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("supersigil.toml"),
        r#"
paths = ["specs/**/*.mdx"]

[components.Custom]

[components.Custom.attributes.x]
required = true
"#,
    )
    .unwrap();

    let output = cargo_bin_cmd!("supersigil")
        .args(["schema", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");

    let custom = &json["components"]["Custom"];
    assert!(custom.get("description").is_none());
    assert!(custom.get("examples").is_none());
}

#[test]
fn schema_missing_config_exits_one() {
    let tmp = TempDir::new().unwrap();

    cargo_bin_cmd!("supersigil")
        .args(["schema"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("config file not found"));
}
