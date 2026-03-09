//! Artifact graph: merges explicit and plugin-derived verification evidence
//! into a unified graph with deduplication, conflict detection, and secondary
//! indexes.

use std::collections::{BTreeSet, HashMap};

use supersigil_core::DocumentGraph;
use supersigil_evidence::{
    EvidenceConflict, EvidenceId, PluginProvenance, TestIdentity, VerifiableRef,
    VerificationEvidenceRecord,
};

// ---------------------------------------------------------------------------
// ArtifactGraph
// ---------------------------------------------------------------------------

/// Unified view of all verification evidence, both from authored `<VerifiedBy>`
/// components (explicit) and from ecosystem plugins (implicit).
///
/// Built by [`build_artifact_graph`], which merges, deduplicates, and detects
/// conflicts across both sources.
#[derive(Debug)]
pub struct ArtifactGraph<'g> {
    /// Reference to the underlying document graph.
    pub documents: &'g DocumentGraph,
    /// All effective evidence records after merge/dedup.
    pub evidence: Vec<VerificationEvidenceRecord>,
    /// Secondary index: verifiable ref → evidence IDs that target it.
    pub evidence_by_target: HashMap<VerifiableRef, Vec<EvidenceId>>,
    /// Secondary index: test identity → evidence IDs for that test.
    pub evidence_by_test: HashMap<TestIdentity, Vec<EvidenceId>>,
    /// Conflicts detected during merge (same test, different criterion sets).
    pub conflicts: Vec<EvidenceConflict>,
}

impl<'g> ArtifactGraph<'g> {
    /// Create an empty artifact graph with no evidence, no indexes, and no
    /// conflicts.  Useful as a default when no plugins or explicit evidence
    /// sources are available.
    #[must_use]
    pub fn empty(documents: &'g DocumentGraph) -> ArtifactGraph<'g> {
        ArtifactGraph {
            documents,
            evidence: Vec::new(),
            evidence_by_target: HashMap::new(),
            evidence_by_test: HashMap::new(),
            conflicts: Vec::new(),
        }
    }

    /// Check whether at least one evidence record targets the given verifiable target.
    ///
    /// This is a convenience wrapper around looking up the
    /// `evidence_by_target` secondary index.
    #[must_use]
    pub fn has_evidence(&self, doc_id: &str, target_id: &str) -> bool {
        self.evidence_by_target.contains_key(&VerifiableRef {
            doc_id: doc_id.to_owned(),
            target_id: target_id.to_owned(),
        })
    }

    /// Returns evidence records that have no resolved targets.
    ///
    /// These typically represent `#[verifies("...")]` attributes where the
    /// target ref could not be resolved to a known criterion (e.g. a bare
    /// fragment like `"login-succeeds"` instead of the full
    /// `"doc-id#criterion-id"` form).
    #[must_use]
    pub fn unresolved_evidence(&self) -> Vec<&VerificationEvidenceRecord> {
        self.evidence
            .iter()
            .filter(|rec| rec.targets.is_empty())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Build an artifact graph by merging explicit and plugin-derived evidence.
///
/// Algorithm:
/// 1. Collect normalized explicit evidence from authored `<VerifiedBy>` sources.
/// 2. Run enabled plugins and collect normalized implicit evidence.
/// 3. Group all evidence by `TestIdentity`.
/// 4. Within each group, compare `targets`:
///    - same set: merge into one effective record, append provenance
///    - different set: emit `EvidenceConflict`, keep records separate
/// 5. Build secondary indexes by target and by test.
#[must_use]
#[allow(
    clippy::missing_panics_doc,
    reason = "unwrap is guarded by len() == 1 check"
)]
pub fn build_artifact_graph(
    documents: &DocumentGraph,
    explicit_evidence: Vec<VerificationEvidenceRecord>,
    plugin_evidence: Vec<VerificationEvidenceRecord>,
) -> ArtifactGraph<'_> {
    // 1. Combine all evidence into one pool.
    let mut all_evidence: Vec<VerificationEvidenceRecord> =
        Vec::with_capacity(explicit_evidence.len() + plugin_evidence.len());
    all_evidence.extend(explicit_evidence);
    all_evidence.extend(plugin_evidence);

    // 2. Group by TestIdentity.
    let mut groups: HashMap<TestIdentity, Vec<VerificationEvidenceRecord>> = HashMap::new();
    for record in all_evidence {
        groups.entry(record.test.clone()).or_default().push(record);
    }

    // 3. Within each group, compare targets and merge or conflict.
    let mut merged_evidence: Vec<VerificationEvidenceRecord> = Vec::new();
    let mut conflicts: Vec<EvidenceConflict> = Vec::new();

    for (_test_identity, records) in groups {
        // Sub-group by targets set: records with the same criterion set
        // can be merged together.
        let mut by_criteria: HashMap<BTreeSet<VerifiableRef>, Vec<VerificationEvidenceRecord>> =
            HashMap::new();
        for record in records {
            by_criteria
                .entry(record.targets.clone())
                .or_default()
                .push(record);
        }

        if by_criteria.len() == 1 {
            // All records have the same criterion set — merge into one.
            let (_criteria_set, sub_records) = by_criteria.into_iter().next().unwrap();
            merged_evidence.push(merge_compatible_records(sub_records));
        } else {
            // Multiple distinct criterion sets — emit conflicts and keep all
            // records separate (merging within each compatible sub-group).
            let sub_groups: Vec<(BTreeSet<VerifiableRef>, Vec<VerificationEvidenceRecord>)> =
                by_criteria.into_iter().collect();

            // Collect all provenances across all conflicting records for the
            // conflict's `sources` field.
            let all_provenances: Vec<PluginProvenance> = sub_groups
                .iter()
                .flat_map(|(_crit, recs)| recs.iter().flat_map(|r| r.provenance.iter().cloned()))
                .collect();

            // Emit conflicts: pick the first criterion set as "left", then for
            // each different set emit one conflict with that set as "right".
            let left = &sub_groups[0].0;
            for other in &sub_groups[1..] {
                conflicts.push(EvidenceConflict {
                    test: sub_groups[0].1[0].test.clone(),
                    left: left.clone(),
                    right: other.0.clone(),
                    sources: all_provenances.clone(),
                });
            }

            // Keep all records separate but merge within each compatible sub-group.
            for (_criteria_set, sub_records) in sub_groups {
                merged_evidence.push(merge_compatible_records(sub_records));
            }
        }
    }

    // 4. Re-assign sequential EvidenceIds starting from 0.
    for (i, record) in merged_evidence.iter_mut().enumerate() {
        record.id = EvidenceId(i);
    }

    // 5. Build secondary indexes.
    let mut evidence_by_target: HashMap<VerifiableRef, Vec<EvidenceId>> = HashMap::new();
    let mut evidence_by_test: HashMap<TestIdentity, Vec<EvidenceId>> = HashMap::new();

    for record in &merged_evidence {
        for crit in &record.targets {
            evidence_by_target
                .entry(crit.clone())
                .or_default()
                .push(record.id);
        }
        evidence_by_test
            .entry(record.test.clone())
            .or_default()
            .push(record.id);
    }

    ArtifactGraph {
        documents,
        evidence: merged_evidence,
        evidence_by_target,
        evidence_by_test,
        conflicts,
    }
}

/// Merge a group of compatible records (same `TestIdentity` and same `targets`)
/// into a single effective record. Uses the first record as the base and extends its
/// provenance with provenances from all subsequent records.
fn merge_compatible_records(
    mut records: Vec<VerificationEvidenceRecord>,
) -> VerificationEvidenceRecord {
    debug_assert!(!records.is_empty(), "merge group must not be empty");

    if records.len() == 1 {
        return records.remove(0);
    }

    let mut base = records.remove(0);
    for other in records {
        base.provenance.extend(other.provenance);
    }
    base
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use supersigil_evidence::{EvidenceKind, SourceLocation, TestKind};

    use super::*;
    use crate::test_helpers::*;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Create a `TestIdentity` for a given file and test name.
    fn test_id(file: &str, name: &str) -> TestIdentity {
        TestIdentity {
            file: PathBuf::from(file),
            name: name.into(),
            kind: TestKind::Unit,
        }
    }

    /// Create a `VerifiableRef`.
    fn crit_ref(doc_id: &str, target_id: &str) -> VerifiableRef {
        VerifiableRef {
            doc_id: doc_id.into(),
            target_id: target_id.into(),
        }
    }

    /// Create a minimal evidence record.
    fn make_evidence(
        id: usize,
        test: TestIdentity,
        criteria: BTreeSet<VerifiableRef>,
        kind: EvidenceKind,
        provenance: Vec<PluginProvenance>,
    ) -> VerificationEvidenceRecord {
        VerificationEvidenceRecord {
            id: EvidenceId(id),
            targets: criteria,
            test: test.clone(),
            source_location: SourceLocation {
                file: test.file,
                line: 1,
                column: 1,
            },
            evidence_kind: kind,
            provenance,
            metadata: BTreeMap::new(),
        }
    }

    /// Convenience: build a `BTreeSet<VerifiableRef>` from pairs.
    fn criteria(pairs: &[(&str, &str)]) -> BTreeSet<VerifiableRef> {
        pairs
            .iter()
            .map(|(doc, crit)| crit_ref(doc, crit))
            .collect()
    }

    // -----------------------------------------------------------------------
    // 1. Compatible-source deduplication (req-7-2)
    // -----------------------------------------------------------------------

    /// When a tag-based evidence source and a Rust attribute source both
    /// describe the same test (same `TestIdentity`) backing the same criterion
    /// set, they should merge into a single effective record with both
    /// provenances preserved.
    #[test]
    fn compatible_sources_are_deduplicated() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        let shared_test = test_id("tests/auth_test.rs", "test_login");
        let shared_criteria = criteria(&[("req/auth", "crit-1")]);

        let explicit = vec![make_evidence(
            0,
            shared_test.clone(),
            shared_criteria.clone(),
            EvidenceKind::Tag,
            vec![PluginProvenance::VerifiedByTag {
                doc_id: "prop/auth".into(),
                tag: "prop:auth".into(),
            }],
        )];

        let plugin = vec![make_evidence(
            1,
            shared_test,
            shared_criteria,
            EvidenceKind::RustAttribute,
            vec![PluginProvenance::RustAttribute {
                attribute_span: SourceLocation {
                    file: PathBuf::from("tests/auth_test.rs"),
                    line: 5,
                    column: 1,
                },
            }],
        )];

        let ag = build_artifact_graph(&graph, explicit, plugin);

        // After dedup, there should be exactly 1 effective record.
        assert_eq!(
            ag.evidence.len(),
            1,
            "expected 1 merged record after dedup, got {}",
            ag.evidence.len(),
        );

        // No conflicts should be emitted for compatible merges.
        assert!(
            ag.conflicts.is_empty(),
            "expected no conflicts for compatible sources, got {}",
            ag.conflicts.len(),
        );
    }

    // -----------------------------------------------------------------------
    // 2. Multiple provenances on one effective record
    // -----------------------------------------------------------------------

    /// After deduplication, the merged record's `provenance` vec should contain
    /// entries from both original sources.
    #[test]
    fn merged_record_has_both_provenances() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        let shared_test = test_id("tests/auth_test.rs", "test_login");
        let shared_criteria = criteria(&[("req/auth", "crit-1")]);

        let tag_prov = PluginProvenance::VerifiedByTag {
            doc_id: "prop/auth".into(),
            tag: "prop:auth".into(),
        };
        let attr_prov = PluginProvenance::RustAttribute {
            attribute_span: SourceLocation {
                file: PathBuf::from("tests/auth_test.rs"),
                line: 5,
                column: 1,
            },
        };

        let explicit = vec![make_evidence(
            0,
            shared_test.clone(),
            shared_criteria.clone(),
            EvidenceKind::Tag,
            vec![tag_prov.clone()],
        )];

        let plugin = vec![make_evidence(
            1,
            shared_test,
            shared_criteria,
            EvidenceKind::RustAttribute,
            vec![attr_prov.clone()],
        )];

        let ag = build_artifact_graph(&graph, explicit, plugin);

        assert_eq!(ag.evidence.len(), 1, "should have exactly 1 merged record");
        let merged = &ag.evidence[0];

        assert_eq!(
            merged.provenance.len(),
            2,
            "merged record should have 2 provenances, got {}",
            merged.provenance.len(),
        );

        assert!(
            merged.provenance.contains(&tag_prov),
            "merged provenance should contain the tag source",
        );
        assert!(
            merged.provenance.contains(&attr_prov),
            "merged provenance should contain the attribute source",
        );
    }

    // -----------------------------------------------------------------------
    // 3. Conflict detection (req-7-4)
    // -----------------------------------------------------------------------

    /// When the same test (`TestIdentity`) has evidence records with DIFFERENT
    /// `targets` sets, an `EvidenceConflict` should be emitted. The
    /// conflicting records should be kept separate (not merged).
    #[test]
    fn conflicting_criterion_sets_emit_conflict() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10), make_criterion("crit-2", 11)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        let shared_test = test_id("tests/auth_test.rs", "test_login");
        let criteria_a = criteria(&[("req/auth", "crit-1")]);
        let criteria_b = criteria(&[("req/auth", "crit-2")]);

        let explicit = vec![make_evidence(
            0,
            shared_test.clone(),
            criteria_a,
            EvidenceKind::Tag,
            vec![PluginProvenance::VerifiedByTag {
                doc_id: "prop/auth".into(),
                tag: "prop:auth".into(),
            }],
        )];

        let plugin = vec![make_evidence(
            1,
            shared_test,
            criteria_b,
            EvidenceKind::RustAttribute,
            vec![PluginProvenance::RustAttribute {
                attribute_span: SourceLocation {
                    file: PathBuf::from("tests/auth_test.rs"),
                    line: 5,
                    column: 1,
                },
            }],
        )];

        let ag = build_artifact_graph(&graph, explicit, plugin);

        // A conflict should be detected.
        assert_eq!(
            ag.conflicts.len(),
            1,
            "expected 1 conflict for mismatched criterion sets, got {}",
            ag.conflicts.len(),
        );

        // Both records should be kept separate (not merged).
        assert_eq!(
            ag.evidence.len(),
            2,
            "conflicting records should be kept separate, got {}",
            ag.evidence.len(),
        );

        // The conflict should reference both criterion sets.
        let conflict = &ag.conflicts[0];
        assert!(
            (conflict.left.contains(&crit_ref("req/auth", "crit-1"))
                && conflict.right.contains(&crit_ref("req/auth", "crit-2")))
                || (conflict.left.contains(&crit_ref("req/auth", "crit-2"))
                    && conflict.right.contains(&crit_ref("req/auth", "crit-1"))),
            "conflict should reference both criterion sets",
        );
    }

    // -----------------------------------------------------------------------
    // 4. Plugin failure isolation (req-2-5, req-8-6)
    // -----------------------------------------------------------------------

    /// Plugin failures should NOT prevent explicit evidence from being
    /// processed. The function signature takes pre-collected evidence, so this
    /// test validates that even if `plugin_evidence` is empty (simulating a
    /// failed plugin), explicit evidence still appears in the graph.
    #[test]
    fn plugin_failure_does_not_block_explicit_evidence() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        let explicit = vec![make_evidence(
            0,
            test_id("tests/auth_test.rs", "test_login"),
            criteria(&[("req/auth", "crit-1")]),
            EvidenceKind::Tag,
            vec![PluginProvenance::VerifiedByTag {
                doc_id: "prop/auth".into(),
                tag: "prop:auth".into(),
            }],
        )];

        // Simulate plugin failure: empty plugin evidence
        let plugin = vec![];

        let ag = build_artifact_graph(&graph, explicit, plugin);

        // Explicit evidence should still be present.
        assert_eq!(
            ag.evidence.len(),
            1,
            "explicit evidence should be present even with empty plugin evidence, got {}",
            ag.evidence.len(),
        );
        assert_eq!(ag.evidence[0].evidence_kind, EvidenceKind::Tag);
    }

    // -----------------------------------------------------------------------
    // 5. Secondary index correctness
    // -----------------------------------------------------------------------

    /// `evidence_by_target` should map each `VerifiableRef` to the IDs of
    /// records targeting it.
    #[test]
    fn evidence_by_target_index_is_correct() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10), make_criterion("crit-2", 11)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        let test_a = test_id("tests/a.rs", "test_a");
        let test_b = test_id("tests/b.rs", "test_b");

        let explicit = vec![
            make_evidence(
                0,
                test_a,
                criteria(&[("req/auth", "crit-1")]),
                EvidenceKind::Tag,
                vec![PluginProvenance::VerifiedByTag {
                    doc_id: "prop/a".into(),
                    tag: "prop:a".into(),
                }],
            ),
            make_evidence(
                1,
                test_b,
                criteria(&[("req/auth", "crit-1"), ("req/auth", "crit-2")]),
                EvidenceKind::Tag,
                vec![PluginProvenance::VerifiedByTag {
                    doc_id: "prop/b".into(),
                    tag: "prop:b".into(),
                }],
            ),
        ];

        let ag = build_artifact_graph(&graph, explicit, vec![]);

        // crit-1 should be targeted by both records
        let crit1 = crit_ref("req/auth", "crit-1");
        let crit1_ids = ag
            .evidence_by_target
            .get(&crit1)
            .expect("crit-1 should be in the index");
        assert_eq!(
            crit1_ids.len(),
            2,
            "crit-1 should be referenced by 2 records, got {}",
            crit1_ids.len(),
        );

        // crit-2 should be targeted by only the second record
        let crit2 = crit_ref("req/auth", "crit-2");
        let crit2_ids = ag
            .evidence_by_target
            .get(&crit2)
            .expect("crit-2 should be in the index");
        assert_eq!(
            crit2_ids.len(),
            1,
            "crit-2 should be referenced by 1 record, got {}",
            crit2_ids.len(),
        );
    }

    /// `evidence_by_test` should map each `TestIdentity` to the IDs of records
    /// for that test.
    #[test]
    fn evidence_by_test_index_is_correct() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        let test_a = test_id("tests/a.rs", "test_a");
        let test_b = test_id("tests/b.rs", "test_b");

        let explicit = vec![
            make_evidence(
                0,
                test_a.clone(),
                criteria(&[("req/auth", "crit-1")]),
                EvidenceKind::Tag,
                vec![PluginProvenance::VerifiedByTag {
                    doc_id: "prop/a".into(),
                    tag: "prop:a".into(),
                }],
            ),
            make_evidence(
                1,
                test_b.clone(),
                criteria(&[("req/auth", "crit-1")]),
                EvidenceKind::Tag,
                vec![PluginProvenance::VerifiedByTag {
                    doc_id: "prop/b".into(),
                    tag: "prop:b".into(),
                }],
            ),
        ];

        let ag = build_artifact_graph(&graph, explicit, vec![]);

        // test_a should map to 1 record
        let a_ids = ag
            .evidence_by_test
            .get(&test_a)
            .expect("test_a should be in the index");
        assert_eq!(
            a_ids.len(),
            1,
            "test_a should have 1 evidence record, got {}",
            a_ids.len(),
        );

        // test_b should map to 1 record
        let b_ids = ag
            .evidence_by_test
            .get(&test_b)
            .expect("test_b should be in the index");
        assert_eq!(
            b_ids.len(),
            1,
            "test_b should have 1 evidence record, got {}",
            b_ids.len(),
        );
    }

    // -----------------------------------------------------------------------
    // 6. No evidence produces empty graph
    // -----------------------------------------------------------------------

    /// When both explicit and plugin evidence are empty, the graph should have
    /// empty evidence, empty indexes, and empty conflicts.
    #[test]
    fn empty_inputs_produce_empty_graph() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        let ag = build_artifact_graph(&graph, vec![], vec![]);

        assert!(ag.evidence.is_empty(), "evidence should be empty");
        assert!(
            ag.evidence_by_target.is_empty(),
            "evidence_by_target should be empty",
        );
        assert!(
            ag.evidence_by_test.is_empty(),
            "evidence_by_test should be empty",
        );
        assert!(ag.conflicts.is_empty(), "conflicts should be empty");
    }

    // -----------------------------------------------------------------------
    // 7. Non-overlapping evidence passes through unchanged
    // -----------------------------------------------------------------------

    /// When explicit and plugin evidence have different `TestIdentity` values
    /// (no overlap), all records should appear in the graph without merging or
    /// conflicts.
    #[test]
    fn non_overlapping_evidence_passes_through() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);

        let test_explicit = test_id("tests/tag_test.rs", "test_tag");
        let test_plugin = test_id("tests/attr_test.rs", "test_attr");

        let shared_criteria = criteria(&[("req/auth", "crit-1")]);

        let explicit = vec![make_evidence(
            0,
            test_explicit,
            shared_criteria.clone(),
            EvidenceKind::Tag,
            vec![PluginProvenance::VerifiedByTag {
                doc_id: "prop/auth".into(),
                tag: "prop:auth".into(),
            }],
        )];

        let plugin = vec![make_evidence(
            1,
            test_plugin,
            shared_criteria,
            EvidenceKind::RustAttribute,
            vec![PluginProvenance::RustAttribute {
                attribute_span: SourceLocation {
                    file: PathBuf::from("tests/attr_test.rs"),
                    line: 5,
                    column: 1,
                },
            }],
        )];

        let ag = build_artifact_graph(&graph, explicit, plugin);

        // Both records should appear, no merging.
        assert_eq!(
            ag.evidence.len(),
            2,
            "non-overlapping evidence should produce 2 records, got {}",
            ag.evidence.len(),
        );

        // No conflicts.
        assert!(
            ag.conflicts.is_empty(),
            "non-overlapping evidence should produce no conflicts, got {}",
            ag.conflicts.len(),
        );
    }
}
