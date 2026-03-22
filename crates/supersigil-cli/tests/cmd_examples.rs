mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use supersigil_rust::verifies;
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn examples_command_lists_self_referential_examples_from_workspace_specs() {
    let output = cargo_bin_cmd!("supersigil")
        .args(["examples", "--format", "json", "executable-examples/req"])
        .current_dir(workspace_root())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));

    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("examples stdout should be valid JSON");

    let example_ids: BTreeSet<&str> = entries
        .iter()
        .map(|entry| {
            entry["example_id"]
                .as_str()
                .expect("example_id should be a string")
        })
        .collect();

    let required_example_ids = BTreeSet::from([
        "fixture-cargo-test-examples-json",
        "fixture-cargo-test-verify",
        "fixture-examples-json",
        "fixture-lint",
        "fixture-verify",
        "fixture-verify-terminal",
    ]);
    assert!(
        required_example_ids.is_subset(&example_ids),
        "expected self-referential executable examples in executable-examples/req, got: {entries:#?}"
    );

    assert!(
        entries
            .iter()
            .filter(|entry| entry["runner"] == "cargo-test")
            .count()
            >= 2,
        "expected at least two cargo-test dogfooding examples, got: {entries:#?}"
    );
}

// ---------------------------------------------------------------------------
// Context scoping via TrackedFiles
// ---------------------------------------------------------------------------

#[verifies("executable-examples/req#req-5-2")]
#[test]
fn examples_scoped_to_cwd_via_tracked_files() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());

    // Two docs, each with TrackedFiles pointing to different directories
    common::write_spec_doc(
        tmp.path(),
        "specs/auth.md",
        "auth/req",
        Some("requirements"),
        Some("approved"),
        r#"```supersigil-xml
<TrackedFiles paths="src/auth/**" />
<AcceptanceCriteria>
  <Criterion id="a-1">auth</Criterion>
</AcceptanceCriteria>

<Example id="auth-ex" lang="sh" runner="sh" />
```

```sh supersigil-ref=auth-ex
echo auth
```"#,
    );

    common::write_spec_doc(
        tmp.path(),
        "specs/billing.md",
        "billing/req",
        Some("requirements"),
        Some("approved"),
        r#"```supersigil-xml
<TrackedFiles paths="src/billing/**" />
<AcceptanceCriteria>
  <Criterion id="b-1">billing</Criterion>
</AcceptanceCriteria>

<Example id="billing-ex" lang="sh" runner="sh" />
```

```sh supersigil-ref=billing-ex
echo billing
```"#,
    );

    // Create the tracked directories
    fs::create_dir_all(tmp.path().join("src/auth")).unwrap();
    fs::create_dir_all(tmp.path().join("src/billing")).unwrap();

    // From src/auth → should only see auth-ex
    let output = cargo_bin_cmd!("supersigil")
        .args(["examples", "--format", "json"])
        .current_dir(tmp.path().join("src/auth"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("valid JSON");
    let ids: Vec<&str> = entries
        .iter()
        .filter_map(|e| e["example_id"].as_str())
        .collect();
    assert!(ids.contains(&"auth-ex"), "should contain auth-ex: {ids:?}");
    assert!(
        !ids.contains(&"billing-ex"),
        "should NOT contain billing-ex: {ids:?}"
    );

    // With --all → should see both
    let output = cargo_bin_cmd!("supersigil")
        .args(["examples", "--format", "json", "--all"])
        .current_dir(tmp.path().join("src/auth"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("valid JSON");
    let ids: Vec<&str> = entries
        .iter()
        .filter_map(|e| e["example_id"].as_str())
        .collect();
    assert!(ids.contains(&"auth-ex"), "should contain auth-ex: {ids:?}");
    assert!(
        ids.contains(&"billing-ex"),
        "--all should include billing-ex: {ids:?}"
    );
}
