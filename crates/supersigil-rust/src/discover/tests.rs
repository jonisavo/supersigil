use super::*;
use supersigil_evidence::EvidenceKind;

/// Discover `#[verifies(...)]` evidence in a single Rust source file.
///
/// Parses the file with `syn`, walks all item-level functions, and extracts
/// evidence records for each function annotated with `#[verifies(...)]`.
///
/// # Errors
///
/// Returns `PluginError` if the file cannot be read or parsed.
fn discover_file(path: &Path) -> Result<Vec<VerificationEvidenceRecord>, PluginError> {
    Ok(discover_file_summary(path)?.records)
}

/// Return the path to a fixture file under `tests/fixtures/discover/`.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/discover")
        .join(name)
}

// -----------------------------------------------------------------------
// Unit test discovery (req-6-1, req-6-2, req-6-3)
// -----------------------------------------------------------------------

#[test]
fn discovers_unit_test_with_verifies() {
    let records = discover_file(&fixture("unit_test.rs")).unwrap();

    assert_eq!(records.len(), 1, "expected exactly one evidence record");
    let record = &records[0];

    // Test identity (req-6-3)
    assert_eq!(record.test.name, "test_login_succeeds");
    assert_eq!(record.test.kind, TestKind::Unit);
    assert_eq!(record.test.file, fixture("unit_test.rs"));

    // Criterion targets (req-6-3)
    let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
        doc_id: "req/auth".to_string(),
        target_id: "crit-1".to_string(),
    }]);
    assert_eq!(record.targets, expected_targets);

    // Evidence kind (req-6-1)
    assert_eq!(record.kind(), Some(EvidenceKind::RustAttribute));

    // Source location (req-6-3): the `#[verifies(...)]` attribute is on line 3
    assert_eq!(record.source_location.file, fixture("unit_test.rs"));
    assert_eq!(record.source_location.line, 3);
    assert!(
        record.source_location.column > 0,
        "expected column to be > 0, got {}",
        record.source_location.column
    );
}

// -----------------------------------------------------------------------
// Async test discovery (req-6-2, req-6-3)
// -----------------------------------------------------------------------

#[test]
fn discovers_async_test_with_tokio() {
    let records = discover_file(&fixture("async_test.rs")).unwrap();

    assert_eq!(records.len(), 1, "expected exactly one evidence record");
    let record = &records[0];

    assert_eq!(record.test.name, "test_api_call");
    assert_eq!(record.test.kind, TestKind::Async);
    assert_eq!(record.test.file, fixture("async_test.rs"));

    let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
        doc_id: "req/api".to_string(),
        target_id: "crit-1".to_string(),
    }]);
    assert_eq!(record.targets, expected_targets);
    assert_eq!(record.kind(), Some(EvidenceKind::RustAttribute));
    assert_eq!(record.source_location.line, 3);
}

// -----------------------------------------------------------------------
// Non-test functions must NOT produce evidence
// -----------------------------------------------------------------------

#[test]
fn async_helper_with_verifies_produces_no_evidence() {
    let records = discover_file(&fixture("async_helper.rs")).unwrap();
    assert!(
        records.is_empty(),
        "async helper without #[test] should produce no evidence, got {} records",
        records.len(),
    );
}

// -----------------------------------------------------------------------
// Proptest discovery (req-6-2, req-6-3, req-6-4)
// -----------------------------------------------------------------------

#[test]
fn discovers_proptest_with_verifies() {
    let records = discover_file(&fixture("proptest_test.rs")).unwrap();

    assert_eq!(records.len(), 1, "expected exactly one evidence record");
    let record = &records[0];

    assert_eq!(record.test.name, "test_roundtrip");
    assert_eq!(record.test.kind, TestKind::Property);
    assert_eq!(record.test.file, fixture("proptest_test.rs"));

    let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
        doc_id: "req/validation".to_string(),
        target_id: "crit-1".to_string(),
    }]);
    assert_eq!(record.targets, expected_targets);
    assert_eq!(record.kind(), Some(EvidenceKind::RustAttribute));
    assert_eq!(record.source_location.line, 3);

    assert_eq!(
        record.metadata.get("framework").map(String::as_str),
        Some("proptest"),
    );
}

// -----------------------------------------------------------------------
// Snapshot test discovery (req-6-2, req-6-3, req-6-4)
// -----------------------------------------------------------------------

#[test]
fn discovers_snapshot_test_with_insta() {
    let records = discover_file(&fixture("snapshot_test.rs")).unwrap();

    assert_eq!(records.len(), 1, "expected exactly one evidence record");
    let record = &records[0];

    assert_eq!(record.test.name, "test_render_output");
    assert_eq!(record.test.kind, TestKind::Snapshot);
    assert_eq!(record.test.file, fixture("snapshot_test.rs"));

    let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
        doc_id: "req/output".to_string(),
        target_id: "crit-1".to_string(),
    }]);
    assert_eq!(record.targets, expected_targets);
    assert_eq!(record.kind(), Some(EvidenceKind::RustAttribute));
    assert_eq!(record.source_location.line, 3);

    assert_eq!(
        record.metadata.get("framework").map(String::as_str),
        Some("insta"),
    );
    assert_eq!(
        record.metadata.get("snapshot_name").map(String::as_str),
        Some("render_output"),
    );
}

// -----------------------------------------------------------------------
// Multiple criterion refs (req-6-3)
// -----------------------------------------------------------------------

#[test]
fn discovers_multiple_targets() {
    let records = discover_file(&fixture("multiple_attrs.rs")).unwrap();

    assert_eq!(records.len(), 1, "expected exactly one evidence record");
    let record = &records[0];

    assert_eq!(record.test.name, "test_full_auth_flow");
    assert_eq!(record.test.kind, TestKind::Unit);

    let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([
        VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string(),
        },
        VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-2".to_string(),
        },
        VerifiableRef {
            doc_id: "req/security".to_string(),
            target_id: "crit-3".to_string(),
        },
    ]);
    assert_eq!(record.targets, expected_targets);
    assert_eq!(record.kind(), Some(EvidenceKind::RustAttribute));
}

// -----------------------------------------------------------------------
// Stacked #[verifies] attributes (multiple attributes on one function)
// -----------------------------------------------------------------------

#[test]
fn discovers_stacked_verifies_attributes() {
    let records = discover_file(&fixture("stacked_verifies.rs")).unwrap();

    assert_eq!(records.len(), 1, "expected exactly one evidence record");
    let record = &records[0];

    assert_eq!(record.test.name, "test_with_stacked_verifies");

    let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([
        VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string(),
        },
        VerifiableRef {
            doc_id: "req/security".to_string(),
            target_id: "crit-2".to_string(),
        },
    ]);
    assert_eq!(record.targets, expected_targets);
}

// -----------------------------------------------------------------------
// Invalid refs are rejected
// -----------------------------------------------------------------------

#[test]
fn document_level_ref_is_rejected() {
    let result = discover_file(&fixture("doc_level_ref.fixture"));

    let err = result.expect_err("fragmentless ref should be rejected");
    assert!(
        matches!(err, PluginError::Discovery { .. }),
        "expected PluginError::Discovery, got {err:?}",
    );
    assert!(
        err.to_string().contains("full criterion ref"),
        "error should require a full criterion ref: {err}",
    );
}

#[test]
fn empty_fragment_ref_is_rejected() {
    let result = discover_file(&fixture("empty_fragment_ref.fixture"));

    let err = result.expect_err("empty fragment ref should be rejected");
    assert!(
        matches!(err, PluginError::Discovery { .. }),
        "expected PluginError::Discovery, got {err:?}",
    );
    assert!(
        err.to_string().contains("full criterion ref"),
        "error should require a full criterion ref: {err}",
    );
}

// -----------------------------------------------------------------------
// Path-qualified attribute: #[supersigil_rust::verifies(...)]
// -----------------------------------------------------------------------

#[test]
fn discovers_path_qualified_verifies_attribute() {
    let records = discover_file(&fixture("path_qualified_attr.rs")).unwrap();

    assert_eq!(records.len(), 1, "expected exactly one evidence record");
    let record = &records[0];

    assert_eq!(record.test.name, "test_path_qualified");
    assert_eq!(record.test.kind, TestKind::Unit);
    assert_eq!(record.test.file, fixture("path_qualified_attr.rs"));

    let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
        doc_id: "req/auth".to_string(),
        target_id: "crit-1".to_string(),
    }]);
    assert_eq!(record.targets, expected_targets);
    assert_eq!(record.kind(), Some(EvidenceKind::RustAttribute));
    assert_eq!(record.source_location.line, 1);
}

// -----------------------------------------------------------------------
// No evidence for unannotated functions (req-6-1)
// -----------------------------------------------------------------------

#[test]
fn no_evidence_for_tests_without_verifies() {
    let records = discover_file(&fixture("no_verifies.rs")).unwrap();

    assert!(
        records.is_empty(),
        "expected no evidence records for unannotated tests, got {}",
        records.len()
    );
}

// -----------------------------------------------------------------------
// Provenance tracking (req-6-1, req-6-3)
// -----------------------------------------------------------------------

#[test]
fn unit_test_provenance_is_rust_attribute() {
    let records = discover_file(&fixture("unit_test.rs")).unwrap();

    assert_eq!(records.len(), 1);
    let record = &records[0];

    assert!(
        !record.provenance.is_empty(),
        "expected at least one provenance entry"
    );
    assert!(
        record
            .provenance
            .iter()
            .any(|p| matches!(p, PluginProvenance::RustAttribute { .. })),
        "expected at least one RustAttribute provenance entry, got {:?}",
        record.provenance
    );
}

// -----------------------------------------------------------------------
// Mod block recursion (functions inside `mod tests { ... }`)
// -----------------------------------------------------------------------

#[test]
fn discovers_tests_inside_mod_block() {
    let records = discover_file(&fixture("mod_block_test.rs")).unwrap();

    assert_eq!(
        records.len(),
        2,
        "expected 2 evidence records from mod block, got {}",
        records.len()
    );

    let names: Vec<&str> = records.iter().map(|r| r.test.name.as_str()).collect();
    assert!(
        names.contains(&"test_inside_mod"),
        "should discover test inside mod block, got: {names:?}",
    );
    assert!(
        names.contains(&"test_async_inside_mod"),
        "should discover async test inside mod block, got: {names:?}",
    );

    let unit = records
        .iter()
        .find(|r| r.test.name == "test_inside_mod")
        .unwrap();
    assert_eq!(unit.test.kind, TestKind::Unit);

    let async_test = records
        .iter()
        .find(|r| r.test.name == "test_async_inside_mod")
        .unwrap();
    assert_eq!(async_test.test.kind, TestKind::Async);

    assert!(unit.targets.iter().any(|c| c.target_id == "crit-1"));
    assert!(async_test.targets.iter().any(|c| c.target_id == "crit-2"));
}

// -----------------------------------------------------------------------
// RustPlugin trait implementation (req-10-3)
// -----------------------------------------------------------------------

fn empty_graph() -> DocumentGraph {
    let config = supersigil_core::Config {
        paths: Some(vec![]),
        ..supersigil_core::Config::default()
    };
    supersigil_core::build_graph(vec![], &config).unwrap()
}

#[test]
fn rust_plugin_plans_discovery_inputs_infers_rust_defaults_when_tests_absent() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("tests/login_test.rs"), "#[test] fn ok() {}").unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn helper() {}").unwrap();

    let plugin = RustPlugin;
    let scope = ProjectScope {
        project: None,
        project_root: dir.path().to_path_buf(),
    };

    let files = plugin.plan_discovery_inputs(&[], &scope);

    assert!(
        files
            .iter()
            .any(|path| path.ends_with("tests/login_test.rs")),
        "expected inferred Rust discovery to include tests/**/*.rs, got {files:?}",
    );
    assert!(
        files.iter().any(|path| path.ends_with("src/lib.rs")),
        "expected inferred Rust discovery to include src/**/*.rs, got {files:?}",
    );
}

#[test]
fn rust_plugin_plans_discovery_inputs_include_src_when_test_globs_configured() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("tests/integration_test.rs"),
        "#[test] fn ok() {}",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "#[cfg(test)] mod tests { #[test] fn unit() {} }",
    )
    .unwrap();

    let plugin = RustPlugin;
    let scope = ProjectScope {
        project: None,
        project_root: dir.path().to_path_buf(),
    };
    let test_files = vec![dir.path().join("tests/integration_test.rs")];

    let files = plugin.plan_discovery_inputs(&test_files, &scope);

    assert!(
        files
            .iter()
            .any(|path| path.ends_with("tests/integration_test.rs")),
        "should include explicit test files, got {files:?}",
    );
    assert!(
        files.iter().any(|path| path.ends_with("src/lib.rs")),
        "should also include src/**/*.rs for Rust plugin unit test discovery, got {files:?}",
    );
}

#[test]
fn rust_plugin_plans_discovery_inputs_traverse_workspace_members() {
    let dir = tempfile::TempDir::new().unwrap();

    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/my-crate\"]\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("crates/my-crate/src")).unwrap();
    std::fs::write(
        dir.path().join("crates/my-crate/src/lib.rs"),
        "pub fn hello() {}",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("crates/my-crate/tests")).unwrap();
    std::fs::write(
        dir.path().join("crates/my-crate/tests/integration.rs"),
        "#[test] fn ok() {}",
    )
    .unwrap();

    let plugin = RustPlugin;
    let scope = ProjectScope {
        project: None,
        project_root: dir.path().to_path_buf(),
    };

    let files = plugin.plan_discovery_inputs(&[], &scope);

    assert!(
        files
            .iter()
            .any(|path| path.ends_with("crates/my-crate/src/lib.rs")),
        "should include workspace member src files, got {files:?}",
    );
    assert!(
        files
            .iter()
            .any(|path| path.ends_with("crates/my-crate/tests/integration.rs")),
        "should include workspace member test files, got {files:?}",
    );
}

#[test]
fn rust_plugin_plans_discovery_inputs_skip_fixture_directories() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("tests/fixtures/fail")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();

    std::fs::write(
        dir.path().join("tests/fixtures/fail/bad_case.rs"),
        "#[verifies(\"req/auth\")]\n#[test]\nfn bad_case() {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests/real_test.rs"),
        "#[test]\nfn real_test() {}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn helper() {}\n").unwrap();

    let plugin = RustPlugin;
    let scope = ProjectScope {
        project: None,
        project_root: dir.path().to_path_buf(),
    };

    let files = plugin.plan_discovery_inputs(&[], &scope);

    assert!(
        files
            .iter()
            .any(|path| path.ends_with("tests/real_test.rs")),
        "real test files should still be inferred, got {files:?}",
    );
    assert!(
        files.iter().any(|path| path.ends_with("src/lib.rs")),
        "src files should still be inferred, got {files:?}",
    );
    assert!(
        files
            .iter()
            .all(|path| !path.ends_with("tests/fixtures/fail/bad_case.rs")),
        "fixture files should be excluded from inferred Rust discovery, got {files:?}",
    );
}

#[test]
fn rust_plugin_discovers_across_multiple_files() {
    let scope = ProjectScope {
        project: None,
        project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    };
    let graph = empty_graph();

    let files = vec![
        fixture("unit_test.rs"),
        fixture("async_test.rs"),
        fixture("no_verifies.rs"),
    ];

    let plugin = RustPlugin;
    assert_eq!(plugin.name(), PLUGIN_NAME);

    let result = plugin.discover(&files, &scope, &graph).unwrap();
    assert_eq!(
        result.evidence.len(),
        2,
        "expected 2 evidence records from 3 files",
    );
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics from clean files, got {:?}",
        result.diagnostics,
    );
}

#[test]
fn rust_plugin_continues_past_per_file_errors() {
    let scope = ProjectScope {
        project: None,
        project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    };
    let graph = empty_graph();

    let files = vec![
        fixture("unit_test.rs"),
        // This file does not exist — should be skipped, not abort discovery
        fixture("this_file_does_not_exist.rs"),
        fixture("async_test.rs"),
    ];

    let plugin = RustPlugin;
    let result = plugin.discover(&files, &scope, &graph).unwrap();
    assert_eq!(
        result.evidence.len(),
        2,
        "should discover 2 records despite 1 missing file, got {}",
        result.evidence.len(),
    );
    assert_eq!(
        result.diagnostics.len(),
        1,
        "missing file should surface exactly 1 structured diagnostic, got {:?}",
        result.diagnostics,
    );
    assert!(
        result.diagnostics[0].message.contains("skipping"),
        "diagnostic should explain the skipped file, got {:?}",
        result.diagnostics,
    );
}

#[test]
fn rust_plugin_skips_non_rs_files() {
    let scope = ProjectScope {
        project: None,
        project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    };
    let graph = empty_graph();

    let files = vec![
        fixture("unit_test.rs"),
        PathBuf::from("some_file.txt"),
        PathBuf::from("Cargo.toml"),
    ];

    let plugin = RustPlugin;
    let result = plugin.discover(&files, &scope, &graph).unwrap();
    assert_eq!(
        result.evidence.len(),
        1,
        "should only discover from .rs files"
    );
    assert!(
        result.diagnostics.is_empty(),
        "non-Rust files should be ignored without diagnostics, got {:?}",
        result.diagnostics,
    );
}

/// Empty file list should return Ok(empty), not an error.
/// Non-Rust repos or mixed workspaces may legitimately have no .rs files
/// in the discovery scope.
#[test]
fn rust_plugin_returns_empty_for_no_rs_files() {
    let scope = ProjectScope {
        project: None,
        project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    };
    let graph = empty_graph();
    let plugin = RustPlugin;

    // Pass only non-Rust files
    let files = vec![PathBuf::from("README.md"), PathBuf::from("Cargo.toml")];
    let result = plugin.discover(&files, &scope, &graph);

    assert!(
        result.is_ok(),
        "empty Rust scope should return Ok, not Err: {result:?}",
    );
    assert!(
        result.unwrap().evidence.is_empty(),
        "should produce zero evidence for non-Rust files",
    );
}

#[test]
fn rust_plugin_errors_when_no_supported_test_items_are_found() {
    let dir = std::env::temp_dir().join("supersigil_test_no_supported_items");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("helpers.rs");
    std::fs::write(
        &path,
        "use supersigil_rust::verifies;\n#[verifies(\"req/auth#crit-1\")]\nfn helper() {}\n",
    )
    .unwrap();

    let scope = ProjectScope {
        project: None,
        project_root: dir.clone(),
    };
    let graph = empty_graph();
    let plugin = RustPlugin;

    let result = plugin.discover(std::slice::from_ref(&path), &scope, &graph);

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);

    assert!(result.is_err(), "expected discovery error");
    let err = result.unwrap_err();
    assert!(
        matches!(err, PluginError::Discovery { .. }),
        "expected PluginError::Discovery, got {err:?}",
    );
    assert!(
        err.to_string().contains("zero supported Rust test items"),
        "unexpected error message: {err}",
    );
}

#[test]
fn fragmentless_ref_is_rejected_by_plugin_discovery() {
    let dir = std::env::temp_dir().join("supersigil_test_doc_level_ref");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("auth_test.rs");
    std::fs::write(
        &path,
        "use supersigil_rust::verifies;\n#[test]\n#[verifies(\"req/auth\")]\nfn login_succeeds() {}\n",
    )
    .unwrap();

    let scope = ProjectScope {
        project: None,
        project_root: dir.clone(),
    };
    let graph = empty_graph();
    let plugin = RustPlugin;

    let result = plugin.discover(std::slice::from_ref(&path), &scope, &graph);

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);

    let err = result.expect_err("fragmentless ref should be rejected");
    assert!(
        matches!(err, PluginError::Discovery { .. }),
        "expected PluginError::Discovery, got {err:?}",
    );
    assert!(
        err.to_string().contains("full criterion ref"),
        "error should require a full criterion ref: {err}",
    );
}

// -----------------------------------------------------------------------
// Error-path tests
// -----------------------------------------------------------------------

#[test]
fn discover_nonexistent_file_returns_io_error() {
    let path = fixture("this_file_does_not_exist.rs");
    let result = discover_file(&path);

    assert!(result.is_err(), "expected Err for nonexistent file");
    assert!(
        matches!(result.unwrap_err(), PluginError::Io { .. }),
        "expected PluginError::Io variant"
    );
}

#[test]
fn discover_invalid_syntax_returns_parse_error() {
    let dir = std::env::temp_dir().join("supersigil_test_invalid_syntax");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad_syntax.rs");
    std::fs::write(&path, "#[verifies(\"req/a#c\")] fn { broken").unwrap();

    let result = discover_file(&path);

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);

    assert!(result.is_err(), "expected Err for invalid syntax");
    assert!(
        matches!(result.unwrap_err(), PluginError::ParseFailure { .. }),
        "expected PluginError::ParseFailure variant"
    );
}
