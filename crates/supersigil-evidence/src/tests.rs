//! Tests for the supersigil-evidence crate.
//!
//! These tests validate semantic behavior of public types, trait contracts,
//! and collection integration for the shared evidence layer.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::{
    EcosystemPlugin, EvidenceConflict, EvidenceId, EvidenceKind, PluginDiagnostic,
    PluginDiscoveryResult, PluginError, PluginProvenance, ProjectScope, SourceLocation,
    TestIdentity, TestKind, VerifiableRef, VerificationEvidenceRecord, VerificationTargets,
};

// ===========================================================================
// VerifiableRef
// ===========================================================================

#[test]
fn verifiable_ref_ordering_in_btreeset() {
    let mut set = BTreeSet::new();
    set.insert(VerifiableRef {
        doc_id: "req/b".into(),
        target_id: "crit-2".into(),
    });
    set.insert(VerifiableRef {
        doc_id: "req/a".into(),
        target_id: "crit-1".into(),
    });
    set.insert(VerifiableRef {
        doc_id: "req/a".into(),
        target_id: "crit-1".into(),
    });
    // Duplicates are deduplicated; ordering is deterministic.
    assert_eq!(set.len(), 2);
    let refs: Vec<_> = set.iter().collect();
    assert!(refs[0].doc_id <= refs[1].doc_id);
}

#[test]
fn verifiable_ref_parse_valid() {
    let cr = VerifiableRef::parse("req/auth#crit-1").unwrap();
    assert_eq!(cr.doc_id, "req/auth");
    assert_eq!(cr.target_id, "crit-1");
}

#[test]
fn verifiable_ref_parse_missing_fragment() {
    assert!(VerifiableRef::parse("req/auth").is_none());
}

#[test]
fn verifiable_ref_parse_empty_fragment() {
    assert!(VerifiableRef::parse("req/auth#").is_none());
}

#[test]
fn verifiable_ref_parse_empty_document() {
    assert!(VerifiableRef::parse("#crit-1").is_none());
}

#[test]
fn verifiable_ref_parse_multi_hash_rejected() {
    assert!(
        VerifiableRef::parse("doc#a#b").is_none(),
        "refs with multiple '#' characters should be rejected"
    );
}

// ===========================================================================
// TestIdentity
// ===========================================================================

#[test]
fn test_identity_equality_same() {
    let a = TestIdentity {
        file: PathBuf::from("tests/auth.rs"),
        name: "test_login".into(),
        kind: TestKind::Unit,
    };
    let b = TestIdentity {
        file: PathBuf::from("tests/auth.rs"),
        name: "test_login".into(),
        kind: TestKind::Unit,
    };
    assert_eq!(a, b);
}

#[test]
fn test_identity_inequality_different_name() {
    let a = TestIdentity {
        file: PathBuf::from("tests/auth.rs"),
        name: "test_login".into(),
        kind: TestKind::Unit,
    };
    let b = TestIdentity {
        file: PathBuf::from("tests/auth.rs"),
        name: "test_logout".into(),
        kind: TestKind::Unit,
    };
    assert_ne!(a, b);
}

#[test]
fn test_identity_inequality_different_kind() {
    let a = TestIdentity {
        file: PathBuf::from("tests/auth.rs"),
        name: "test_login".into(),
        kind: TestKind::Unit,
    };
    let b = TestIdentity {
        file: PathBuf::from("tests/auth.rs"),
        name: "test_login".into(),
        kind: TestKind::Async,
    };
    assert_ne!(a, b);
}

// ===========================================================================
// TestKind
// ===========================================================================

#[test]
fn test_kind_as_str() {
    assert_eq!(TestKind::Unit.as_str(), "unit");
    assert_eq!(TestKind::Async.as_str(), "async");
    assert_eq!(TestKind::Property.as_str(), "property");
    assert_eq!(TestKind::Snapshot.as_str(), "snapshot");
    assert_eq!(TestKind::Unknown.as_str(), "unknown");
}

// ===========================================================================
// EvidenceKind
// ===========================================================================

#[test]
fn evidence_kind_as_str() {
    assert_eq!(EvidenceKind::Tag.as_str(), "tag");
    assert_eq!(EvidenceKind::FileGlob.as_str(), "file-glob");
    assert_eq!(EvidenceKind::RustAttribute.as_str(), "rust-attribute");
    assert_eq!(EvidenceKind::Example.as_str(), "example");
}

// ===========================================================================
// PluginProvenance
// ===========================================================================

#[test]
fn plugin_provenance_verified_by_tag() {
    let prov = PluginProvenance::VerifiedByTag {
        doc_id: "prop/auth".into(),
        tag: "prop:auth".into(),
    };
    match &prov {
        PluginProvenance::VerifiedByTag { doc_id, tag } => {
            assert_eq!(doc_id, "prop/auth");
            assert_eq!(tag, "prop:auth");
        }
        _ => panic!("expected VerifiedByTag variant"),
    }
}

#[test]
fn plugin_provenance_verified_by_file_glob() {
    let prov = PluginProvenance::VerifiedByFileGlob {
        doc_id: "prop/auth".into(),
        paths: vec!["tests/auth/**".into()],
    };
    match &prov {
        PluginProvenance::VerifiedByFileGlob { doc_id, paths } => {
            assert_eq!(doc_id, "prop/auth");
            assert_eq!(paths, &["tests/auth/**"]);
        }
        _ => panic!("expected VerifiedByFileGlob variant"),
    }
}

#[test]
fn plugin_provenance_rust_attribute() {
    let prov = PluginProvenance::RustAttribute {
        attribute_span: SourceLocation {
            file: PathBuf::from("tests/auth.rs"),
            line: 5,
            column: 1,
        },
    };
    match &prov {
        PluginProvenance::RustAttribute { attribute_span } => {
            assert_eq!(attribute_span.file, PathBuf::from("tests/auth.rs"));
            assert_eq!(attribute_span.line, 5);
            assert_eq!(attribute_span.column, 1);
        }
        _ => panic!("expected RustAttribute variant"),
    }
}

#[test]
fn plugin_provenance_inequality_across_variants() {
    let tag = PluginProvenance::VerifiedByTag {
        doc_id: "prop/auth".into(),
        tag: "prop:auth".into(),
    };
    let glob = PluginProvenance::VerifiedByFileGlob {
        doc_id: "prop/auth".into(),
        paths: vec!["tests/**".into()],
    };
    assert_ne!(tag, glob);
}

// ===========================================================================
// VerificationTargets
// ===========================================================================

#[test]
fn verification_targets_reject_empty_sets() {
    assert!(VerificationTargets::new(BTreeSet::new()).is_none());
}

#[test]
fn verification_targets_accept_non_empty_sets() {
    let targets = VerificationTargets::new(BTreeSet::from([VerifiableRef {
        doc_id: "req/auth".into(),
        target_id: "crit-1".into(),
    }]))
    .expect("non-empty target set should be accepted");

    assert_eq!(targets.len(), 1);
    assert!(targets.iter().any(|target| target.target_id == "crit-1"));
}

// ===========================================================================
// VerificationEvidenceRecord
// ===========================================================================

#[test]
fn verification_evidence_record_construction() {
    let mut targets = BTreeSet::new();
    targets.insert(VerifiableRef {
        doc_id: "req/auth".into(),
        target_id: "crit-1".into(),
    });

    let record = VerificationEvidenceRecord {
        id: EvidenceId::new(0),
        targets: VerificationTargets::new(targets.clone()).expect("record target set"),
        test: TestIdentity {
            file: PathBuf::from("tests/auth.rs"),
            name: "test_login".into(),
            kind: TestKind::Unit,
        },
        source_location: SourceLocation {
            file: PathBuf::from("tests/auth.rs"),
            line: 10,
            column: 1,
        },
        provenance: vec![PluginProvenance::VerifiedByTag {
            doc_id: "prop/auth".into(),
            tag: "prop:auth".into(),
        }],
        metadata: BTreeMap::new(),
    };

    assert_eq!(record.id, EvidenceId::new(0));
    assert_eq!(record.targets, targets);
    assert_eq!(record.test.name, "test_login");
    assert_eq!(record.kind(), Some(EvidenceKind::Tag));
    assert_eq!(record.provenance.len(), 1);
    assert!(record.metadata.is_empty());
}

#[test]
fn verification_evidence_record_multiple_criteria() {
    let mut targets = BTreeSet::new();
    targets.insert(VerifiableRef {
        doc_id: "req/auth".into(),
        target_id: "crit-1".into(),
    });
    targets.insert(VerifiableRef {
        doc_id: "req/auth".into(),
        target_id: "crit-2".into(),
    });

    let record = VerificationEvidenceRecord {
        id: EvidenceId::new(1),
        targets: VerificationTargets::new(targets.clone()).expect("record target set"),
        test: TestIdentity {
            file: PathBuf::from("tests/auth.rs"),
            name: "test_full_flow".into(),
            kind: TestKind::Unit,
        },
        source_location: SourceLocation {
            file: PathBuf::from("tests/auth.rs"),
            line: 20,
            column: 1,
        },
        provenance: vec![PluginProvenance::RustAttribute {
            attribute_span: SourceLocation {
                file: PathBuf::from("tests/auth.rs"),
                line: 19,
                column: 1,
            },
        }],
        metadata: BTreeMap::new(),
    };

    assert_eq!(record.targets.len(), 2);
}

#[test]
fn verification_evidence_record_with_metadata() {
    let mut metadata = BTreeMap::new();
    metadata.insert("snapshot_id".into(), "auth_login_1".into());

    let mut targets = BTreeSet::new();
    targets.insert(VerifiableRef {
        doc_id: "req/snapshots".into(),
        target_id: "crit-1".into(),
    });

    let record = VerificationEvidenceRecord {
        id: EvidenceId::new(2),
        targets: VerificationTargets::new(targets).expect("record target set"),
        test: TestIdentity {
            file: PathBuf::from("tests/snapshots.rs"),
            name: "test_snapshot".into(),
            kind: TestKind::Snapshot,
        },
        source_location: SourceLocation {
            file: PathBuf::from("tests/snapshots.rs"),
            line: 5,
            column: 1,
        },
        provenance: vec![],
        metadata,
    };

    assert_eq!(record.metadata.get("snapshot_id").unwrap(), "auth_login_1");
    assert_eq!(record.test.kind, TestKind::Snapshot);
}

// ===========================================================================
// EvidenceConflict
// ===========================================================================

#[test]
fn evidence_conflict_construction() {
    let mut left = BTreeSet::new();
    left.insert(VerifiableRef {
        doc_id: "req/auth".into(),
        target_id: "crit-1".into(),
    });

    let mut right = BTreeSet::new();
    right.insert(VerifiableRef {
        doc_id: "req/auth".into(),
        target_id: "crit-2".into(),
    });

    let conflict = EvidenceConflict {
        test: TestIdentity {
            file: PathBuf::from("tests/auth.rs"),
            name: "test_login".into(),
            kind: TestKind::Unit,
        },
        left: left.clone(),
        right: right.clone(),
        sources: vec![
            PluginProvenance::VerifiedByTag {
                doc_id: "prop/auth".into(),
                tag: "prop:auth".into(),
            },
            PluginProvenance::RustAttribute {
                attribute_span: SourceLocation {
                    file: PathBuf::from("tests/auth.rs"),
                    line: 9,
                    column: 1,
                },
            },
        ],
    };

    assert_eq!(conflict.test.name, "test_login");
    assert_eq!(conflict.left, left);
    assert_eq!(conflict.right, right);
    assert_eq!(conflict.sources.len(), 2);
}

// ===========================================================================
// PluginError
// ===========================================================================

#[test]
fn plugin_error_parse_failure() {
    let err = PluginError::ParseFailure {
        plugin: "rust".into(),
        file: PathBuf::from("src/lib.rs"),
        message: "unexpected token".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("rust"));
    assert!(msg.contains("src/lib.rs"));
    assert!(msg.contains("unexpected token"));
}

#[test]
fn plugin_error_discovery() {
    let err = PluginError::Discovery {
        plugin: "rust".into(),
        message: "no test files found".into(),
        details: None,
    };
    let msg = err.to_string();
    assert!(msg.contains("rust"));
    assert!(msg.contains("no test files found"));
}

#[test]
fn plugin_error_io() {
    let err = PluginError::Io {
        plugin: "rust".into(),
        path: PathBuf::from("/nonexistent"),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
    };
    let msg = err.to_string();
    assert!(msg.contains("rust"));
    assert!(msg.contains("/nonexistent"));
}

#[test]
fn plugin_diagnostic_warning_for_path_records_message_and_path() {
    let diagnostic = PluginDiagnostic::warning_for_path(
        PathBuf::from("src/lib.rs"),
        "skipping due to parse failure",
    );

    assert_eq!(diagnostic.message, "skipping due to parse failure");
    assert_eq!(diagnostic.path, Some(PathBuf::from("src/lib.rs")));
}

// ===========================================================================
// EcosystemPlugin trait
// ===========================================================================

/// A mock plugin for testing the `EcosystemPlugin` trait contract.
struct MockPlugin;

impl EcosystemPlugin for MockPlugin {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn discover(
        &self,
        files: &[PathBuf],
        _scope: &ProjectScope,
        _documents: &supersigil_core::DocumentGraph,
    ) -> Result<PluginDiscoveryResult, PluginError> {
        // Return one evidence record per input file.
        let records = files
            .iter()
            .enumerate()
            .map(|(i, file)| VerificationEvidenceRecord {
                id: EvidenceId::new(i),
                targets: VerificationTargets::single(VerifiableRef {
                    doc_id: format!("req/mock-{i}"),
                    target_id: "crit-1".into(),
                }),
                test: TestIdentity {
                    file: file.clone(),
                    name: format!("test_{i}"),
                    kind: TestKind::Unknown,
                },
                source_location: SourceLocation {
                    file: file.clone(),
                    line: 1,
                    column: 1,
                },

                provenance: vec![],
                metadata: BTreeMap::new(),
            })
            .collect();
        Ok(PluginDiscoveryResult::from_evidence(records))
    }
}

/// A mock plugin that always fails for testing error paths.
struct FailingPlugin;

impl EcosystemPlugin for FailingPlugin {
    fn name(&self) -> &'static str {
        "failing"
    }

    fn discover(
        &self,
        _files: &[PathBuf],
        _scope: &ProjectScope,
        _documents: &supersigil_core::DocumentGraph,
    ) -> Result<PluginDiscoveryResult, PluginError> {
        Err(PluginError::Discovery {
            plugin: "failing".into(),
            message: "intentional failure".into(),
            details: None,
        })
    }
}

struct PlanningPlugin;

impl EcosystemPlugin for PlanningPlugin {
    fn name(&self) -> &'static str {
        "planning"
    }

    fn plan_discovery_inputs<'a>(
        &self,
        test_files: &'a [PathBuf],
        scope: &ProjectScope,
    ) -> Cow<'a, [PathBuf]> {
        let mut planned_files = test_files.to_vec();
        planned_files.push(scope.project_root.join("shared/support.rs"));
        Cow::Owned(planned_files)
    }

    fn discover(
        &self,
        _files: &[PathBuf],
        _scope: &ProjectScope,
        _documents: &supersigil_core::DocumentGraph,
    ) -> Result<PluginDiscoveryResult, PluginError> {
        Ok(PluginDiscoveryResult::default())
    }
}

fn sample_scope() -> ProjectScope {
    ProjectScope {
        project: Some("demo".into()),
        project_root: PathBuf::from("/workspace/demo"),
    }
}

#[test]
fn ecosystem_plugin_default_plan_discovery_inputs_returns_test_files() {
    let plugin = MockPlugin;
    let test_files = vec![
        PathBuf::from("tests/unit/auth.rs"),
        PathBuf::from("tests/integration/session.rs"),
    ];

    let planned_files = plugin.plan_discovery_inputs(&test_files, &sample_scope());

    assert_eq!(&*planned_files, test_files);
}

#[test]
fn ecosystem_plugin_trait_object() {
    let plugin: Box<dyn EcosystemPlugin> = Box::new(MockPlugin);
    assert_eq!(plugin.name(), "mock");
    let input = [PathBuf::from("tests/auth.rs")];
    let planned_files = plugin.plan_discovery_inputs(&input, &sample_scope());
    assert_eq!(&*planned_files, input);

    let failing: Box<dyn EcosystemPlugin> = Box::new(FailingPlugin);
    assert_eq!(failing.name(), "failing");
}

#[test]
fn ecosystem_plugin_trait_object_dispatches_plan_discovery_inputs_override() {
    let plugin: Box<dyn EcosystemPlugin> = Box::new(PlanningPlugin);
    let input = [PathBuf::from("tests/auth.rs")];
    let planned_files = plugin.plan_discovery_inputs(&input, &sample_scope());

    assert_eq!(
        &*planned_files,
        [
            PathBuf::from("tests/auth.rs"),
            PathBuf::from("/workspace/demo/shared/support.rs"),
        ],
    );
}

// NOTE: Full discover() tests require a DocumentGraph instance, which depends
// on supersigil-core graph construction. We verify the trait is implementable
// and the mock compiles correctly here. Integration tests with real
// DocumentGraph instances will be added in later tasks.
