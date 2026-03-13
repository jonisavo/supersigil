use assert_cmd::cargo::cargo_bin_cmd;
use std::collections::BTreeSet;
use std::path::PathBuf;

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
