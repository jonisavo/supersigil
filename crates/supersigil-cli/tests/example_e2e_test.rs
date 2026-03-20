//! E2E test: verify executable examples work against a fixture project.

use std::path::Path;
use supersigil_rust::verifies;

#[verifies(
    "executable-examples/req#req-1-4",
    "executable-examples/req#req-2-1",
    "executable-examples/req#req-3-1"
)]
#[test]
fn fixture_project_examples_pass() {
    let fixture_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/example-project");

    // Check fixture exists
    assert!(
        fixture_root.join("supersigil.toml").exists(),
        "fixture project should exist at {}",
        fixture_root.display()
    );

    // Load config and build graph
    let config_path = fixture_root.join("supersigil.toml");
    let (config, graph) = supersigil_cli::load_graph(&config_path).unwrap();

    // Collect examples
    let specs = supersigil_verify::collect_examples(&graph, &config.examples);
    assert!(!specs.is_empty(), "should find at least one example");

    let example_ids: Vec<&str> = specs.iter().map(|spec| spec.example_id.as_str()).collect();
    assert_eq!(
        example_ids,
        vec!["echo-test", "rust-test"],
        "fixture project should expose both sh and cargo-test examples",
    );

    assert!(
        specs
            .iter()
            .any(|spec| spec.runner == "cargo-test" && spec.lang == "rust"),
        "fixture project should include a cargo-test example, got: {specs:#?}",
    );

    // Execute examples
    let results = supersigil_verify::execute_examples(&specs, &fixture_root, &config.examples);

    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|result| matches!(result.outcome, supersigil_verify::ExampleOutcome::Pass)),
        "fixture examples should all pass, got: {results:#?}",
    );
}

#[test]
fn fixture_project_discovery_finds_examples() {
    let fixture_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/example-project");

    let config_path = fixture_root.join("supersigil.toml");
    let (config, graph) = supersigil_cli::load_graph(&config_path).unwrap();

    let specs = supersigil_verify::collect_examples(&graph, &config.examples);

    // Verify discovery
    assert_eq!(specs.len(), 2);
    assert!(specs.iter().all(|spec| spec.doc_id == "demo/req"));
    assert_eq!(specs[0].example_id, "echo-test");
    assert_eq!(specs[1].example_id, "rust-test");
}
