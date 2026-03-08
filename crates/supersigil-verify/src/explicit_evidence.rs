//! Explicit evidence normalization: converts authored `<VerifiedBy>` components
//! into the shared `VerificationEvidenceRecord` format from `supersigil-evidence`.
//!
//! This module bridges the existing verify pipeline (which checks for tag/glob
//! *existence*) with the new evidence model (which produces normalized records
//! for merging in the `ArtifactGraph`).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use supersigil_core::{DocumentGraph, split_list_attribute};
use supersigil_evidence::{
    EvidenceId, EvidenceKind, PluginProvenance, SourceLocation, TestIdentity, TestKind,
    VerifiableRef, VerificationEvidenceRecord,
};

use crate::scan::{TagMatch, scan_all_tags};

/// Extract normalized evidence records from all explicit `<VerifiedBy>` components
/// in the document graph.
///
/// Evidence is collected from contextual `<VerifiedBy>` components nested inside
/// `<Criterion>`. Evidence targets the parent criterion directly.
///
/// For each `VerifiedBy` component:
/// - **`strategy="tag"`**: looks up pre-scanned tag results for matching comment
///   tags and produces one record per discovered test match.
/// - **`strategy="file-glob"`**: expands path globs relative to `project_root`
///   and produces one record per matched file, using coarse file-level identity.
///
/// `test_files` should be pre-resolved via `resolve_test_files` to avoid
/// redundant glob expansion when the caller already has the file list.
///
/// Returns an empty `Vec` when no `VerifiedBy` components exist or no matches
/// are found.
#[must_use]
pub fn extract_explicit_evidence(
    graph: &DocumentGraph,
    test_files: &[PathBuf],
    project_root: &Path,
) -> Vec<VerificationEvidenceRecord> {
    let mut records = Vec::new();
    let mut next_id: usize = 0;

    // Single-pass scan for all tags across all test files (E5/E9).
    let all_matches = scan_all_tags(test_files);
    let mut tag_index: HashMap<&str, Vec<&TagMatch>> = HashMap::new();
    for m in &all_matches {
        tag_index.entry(m.tag.as_str()).or_default().push(m);
    }

    for (_id, doc) in graph.documents() {
        let doc_id = &doc.frontmatter.id;

        // 1. Contextual VerifiedBy (nested inside Criterion) — preferred path
        collect_criterion_evidence(
            doc_id,
            &doc.components,
            &tag_index,
            project_root,
            &mut records,
            &mut next_id,
        );
    }

    records
}

/// Process a single `<VerifiedBy>` component into evidence records.
fn process_verified_by(
    comp: &supersigil_core::ExtractedComponent,
    doc_id: &str,
    targets: &BTreeSet<VerifiableRef>,
    tag_index: &HashMap<&str, Vec<&TagMatch>>,
    project_root: &Path,
    records: &mut Vec<VerificationEvidenceRecord>,
    next_id: &mut usize,
) {
    let strategy = comp.attributes.get("strategy").map(String::as_str);

    match strategy {
        Some("tag") => {
            let Some(tag) = comp.attributes.get("tag") else {
                return;
            };

            let matches = tag_index.get(tag.as_str()).cloned().unwrap_or_default();

            for m in matches {
                records.push(VerificationEvidenceRecord {
                    id: EvidenceId(*next_id),
                    targets: targets.clone(),
                    test: TestIdentity {
                        file: m.file.clone(),
                        name: tag.clone(),
                        kind: TestKind::Unknown,
                    },
                    source_location: SourceLocation {
                        file: m.file.clone(),
                        line: m.line,
                        column: 1,
                    },
                    evidence_kind: EvidenceKind::Tag,
                    provenance: vec![PluginProvenance::VerifiedByTag {
                        doc_id: doc_id.to_owned(),
                        tag: tag.clone(),
                    }],
                    metadata: std::collections::BTreeMap::new(),
                });
                *next_id += 1;
            }
        }
        Some("file-glob") => {
            let Some(paths_attr) = comp.attributes.get("paths") else {
                return;
            };

            let Ok(path_list) = split_list_attribute(paths_attr) else {
                return;
            };

            let matched_files: Vec<_> = path_list
                .iter()
                .flat_map(|p| crate::expand_glob(p, project_root))
                .collect();

            for file in matched_files {
                records.push(VerificationEvidenceRecord {
                    id: EvidenceId(*next_id),
                    targets: targets.clone(),
                    test: TestIdentity {
                        file: file.clone(),
                        name: "<file-glob>".into(),
                        kind: TestKind::Unknown,
                    },
                    source_location: SourceLocation {
                        file,
                        line: 1,
                        column: 1,
                    },
                    evidence_kind: EvidenceKind::FileGlob,
                    provenance: vec![PluginProvenance::VerifiedByFileGlob {
                        doc_id: doc_id.to_owned(),
                        paths: vec![paths_attr.clone()],
                    }],
                    metadata: std::collections::BTreeMap::new(),
                });
                *next_id += 1;
            }
        }
        _ => {}
    }
}

/// Recursively walk components to find `<VerifiedBy>` nested inside `<Criterion>`.
/// Evidence targets the parent criterion directly.
fn collect_criterion_evidence(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    tag_index: &HashMap<&str, Vec<&TagMatch>>,
    project_root: &Path,
    records: &mut Vec<VerificationEvidenceRecord>,
    next_id: &mut usize,
) {
    for component in components {
        if component.name == "Criterion"
            && let Some(criterion_id) = component.attributes.get("id")
        {
            let targets = BTreeSet::from([VerifiableRef {
                doc_id: doc_id.to_owned(),
                target_id: criterion_id.to_owned(),
            }]);

            for child in &component.children {
                if child.name == "VerifiedBy" {
                    process_verified_by(
                        child,
                        doc_id,
                        &targets,
                        tag_index,
                        project_root,
                        records,
                        next_id,
                    );
                }
            }
        }
        // Recurse into children (e.g. Criterion inside AcceptanceCriteria)
        collect_criterion_evidence(
            doc_id,
            &component.children,
            tag_index,
            project_root,
            records,
            next_id,
        );
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::test_helpers::*;

    // -----------------------------------------------------------------------
    // 1. Tag evidence via contextual `VerifiedBy`
    // -----------------------------------------------------------------------

    /// A criterion with contextual `VerifiedBy strategy="tag"`, plus a test
    /// file containing the matching tag, should produce one evidence record
    /// with the correct criterion target, evidence kind, provenance, and test kind.
    #[test]
    fn tag_evidence_produces_normalized_record() {
        let dir = TempDir::new().unwrap();
        write_test_file(
            &dir,
            "tests/auth_test.rs",
            "// supersigil: auth:crit1\nfn test_login() {}\n",
        );

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_tag("auth:crit1", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        config.tests = Some(vec!["tests/**/*.rs".into()]);

        let test_files = crate::resolve_test_files(&config, dir.path());
        let records = extract_explicit_evidence(&graph, &test_files, dir.path());

        assert_eq!(
            records.len(),
            1,
            "expected 1 evidence record, got {}",
            records.len()
        );
        let rec = &records[0];

        // Criterion targets resolved from parent Criterion
        let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
            doc_id: "req/auth".into(),
            target_id: "crit-1".into(),
        }]);
        assert_eq!(rec.targets, expected_targets);

        // Evidence kind
        assert_eq!(rec.evidence_kind, EvidenceKind::Tag);

        // Test kind is Unknown (tag scanning doesn't infer test kind)
        assert_eq!(rec.test.kind, TestKind::Unknown);

        // Provenance
        assert_eq!(rec.provenance.len(), 1);
        assert_eq!(
            rec.provenance[0],
            PluginProvenance::VerifiedByTag {
                doc_id: "req/auth".into(),
                tag: "auth:crit1".into(),
            }
        );
    }

    // -----------------------------------------------------------------------
    // 2. File-glob evidence via contextual `VerifiedBy`
    // -----------------------------------------------------------------------

    /// `VerifiedBy strategy="file-glob" paths="tests/auth_test.rs"` inside a
    /// `Criterion` should produce a coarse file-level evidence record.
    #[test]
    fn file_glob_evidence_produces_normalized_record() {
        let dir = TempDir::new().unwrap();
        write_test_file(&dir, "tests/auth_test.rs", "fn test_auth() {}\n");

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_glob("tests/auth_test.rs", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();

        let test_files = crate::resolve_test_files(&config, dir.path());
        let records = extract_explicit_evidence(&graph, &test_files, dir.path());

        assert_eq!(
            records.len(),
            1,
            "expected 1 evidence record, got {}",
            records.len()
        );
        let rec = &records[0];

        // File-glob uses sentinel test name
        assert_eq!(rec.test.name, "<file-glob>");
        assert_eq!(rec.test.kind, TestKind::Unknown);

        // Evidence kind
        assert_eq!(rec.evidence_kind, EvidenceKind::FileGlob);

        // Provenance
        assert_eq!(rec.provenance.len(), 1);
        match &rec.provenance[0] {
            PluginProvenance::VerifiedByFileGlob { doc_id, paths } => {
                assert_eq!(doc_id, "req/auth");
                assert_eq!(paths, &["tests/auth_test.rs"]);
            }
            other => panic!("expected VerifiedByFileGlob provenance, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // 3. Provenance tracking
    // -----------------------------------------------------------------------

    /// Each evidence record must preserve which document and strategy produced
    /// it via its `provenance` field.
    #[test]
    fn provenance_tracks_source_document_and_strategy() {
        let dir = TempDir::new().unwrap();
        write_test_file(&dir, "tests/login_test.rs", "// supersigil: auth:login\n");
        write_test_file(&dir, "tests/session_test.rs", "fn test_session() {}\n");

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![
                    make_criterion_with_verified_by(
                        "login",
                        make_verified_by_tag("auth:login", 11),
                        10,
                    ),
                    make_criterion_with_verified_by(
                        "session",
                        make_verified_by_glob("tests/session_test.rs", 13),
                        12,
                    ),
                ],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        config.tests = Some(vec!["tests/**/*.rs".into()]);

        let test_files = crate::resolve_test_files(&config, dir.path());
        let records = extract_explicit_evidence(&graph, &test_files, dir.path());

        assert_eq!(
            records.len(),
            2,
            "expected 2 evidence records (one tag, one glob), got {}",
            records.len(),
        );

        // Find the tag-sourced record
        let tag_rec = records
            .iter()
            .find(|r| r.evidence_kind == EvidenceKind::Tag)
            .expect("should have a Tag evidence record");
        assert_eq!(
            tag_rec.provenance[0],
            PluginProvenance::VerifiedByTag {
                doc_id: "req/auth".into(),
                tag: "auth:login".into(),
            }
        );

        // Find the glob-sourced record
        let glob_rec = records
            .iter()
            .find(|r| r.evidence_kind == EvidenceKind::FileGlob)
            .expect("should have a FileGlob evidence record");
        match &glob_rec.provenance[0] {
            PluginProvenance::VerifiedByFileGlob { doc_id, .. } => {
                assert_eq!(doc_id, "req/auth");
            }
            other => panic!("expected VerifiedByFileGlob, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // 4. Multiple tag matches → multiple evidence records
    // -----------------------------------------------------------------------

    /// One tag matching 3 separate test files should produce 3 separate evidence
    /// records, each carrying the same criterion set.
    #[test]
    fn multiple_tag_matches_produce_separate_records() {
        let dir = TempDir::new().unwrap();
        write_test_file(
            &dir,
            "tests/test_a.rs",
            "// supersigil: auth:multi\nfn a() {}\n",
        );
        write_test_file(
            &dir,
            "tests/test_b.rs",
            "// supersigil: auth:multi\nfn b() {}\n",
        );
        write_test_file(
            &dir,
            "tests/test_c.rs",
            "// supersigil: auth:multi\nfn c() {}\n",
        );

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_tag("auth:multi", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        config.tests = Some(vec!["tests/**/*.rs".into()]);

        let test_files = crate::resolve_test_files(&config, dir.path());
        let records = extract_explicit_evidence(&graph, &test_files, dir.path());

        assert_eq!(
            records.len(),
            3,
            "expected 3 evidence records (one per test file), got {}",
            records.len(),
        );

        // All should share the same criterion set
        let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
            doc_id: "req/auth".into(),
            target_id: "crit-1".into(),
        }]);
        for rec in &records {
            assert_eq!(rec.targets, expected_targets);
            assert_eq!(rec.evidence_kind, EvidenceKind::Tag);
        }

        // Each should reference a distinct file
        let files: BTreeSet<_> = records.iter().map(|r| r.test.file.clone()).collect();
        assert_eq!(
            files.len(),
            3,
            "each record should reference a distinct file"
        );
    }

    // -----------------------------------------------------------------------
    // 4b. File-glob with multiple comma-separated paths
    // -----------------------------------------------------------------------

    /// When a `VerifiedBy` has `paths="tests/a.rs, tests/b.rs"` (comma-separated),
    /// `extract_explicit_evidence` should split the paths and produce evidence
    /// for each matched file individually.
    #[test]
    fn file_glob_multi_path_produces_evidence_for_each_path() {
        let dir = TempDir::new().unwrap();
        write_test_file(&dir, "tests/a.rs", "fn test_a() {}\n");
        write_test_file(&dir, "tests/b.rs", "fn test_b() {}\n");

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_glob("tests/a.rs, tests/b.rs", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();

        let test_files = crate::resolve_test_files(&config, dir.path());
        let records = extract_explicit_evidence(&graph, &test_files, dir.path());

        assert_eq!(
            records.len(),
            2,
            "expected 2 evidence records for 2 comma-separated paths, got {}",
            records.len(),
        );

        let files: BTreeSet<_> = records.iter().map(|r| r.test.file.clone()).collect();
        assert_eq!(
            files.len(),
            2,
            "each record should reference a distinct file"
        );
    }

    // -----------------------------------------------------------------------
    // 6. Contextual VerifiedBy (nested inside Criterion)
    // -----------------------------------------------------------------------

    /// When `<VerifiedBy>` is nested inside `<Criterion>`, evidence targets
    /// the parent criterion directly — no `<Validates>` indirection needed.
    #[test]
    fn contextual_verified_by_tag_targets_parent_criterion() {
        let dir = TempDir::new().unwrap();
        write_test_file(
            &dir,
            "tests/auth_test.rs",
            "// supersigil: auth:login\nfn test_login() {}\n",
        );

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "login-success",
                    make_verified_by_tag("auth:login", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        config.tests = Some(vec!["tests/**/*.rs".into()]);

        let test_files = crate::resolve_test_files(&config, dir.path());
        let records = extract_explicit_evidence(&graph, &test_files, dir.path());

        assert_eq!(
            records.len(),
            1,
            "expected 1 evidence record, got {}",
            records.len()
        );
        let rec = &records[0];

        let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
            doc_id: "req/auth".into(),
            target_id: "login-success".into(),
        }]);
        assert_eq!(rec.targets, expected_targets);
        assert_eq!(rec.evidence_kind, EvidenceKind::Tag);
    }

    /// Contextual `VerifiedBy` with file-glob strategy targets parent criterion.
    #[test]
    fn contextual_verified_by_glob_targets_parent_criterion() {
        let dir = TempDir::new().unwrap();
        write_test_file(&dir, "tests/auth_test.rs", "fn test_auth() {}\n");

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "login-success",
                    make_verified_by_glob("tests/auth_test.rs", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();

        let test_files = crate::resolve_test_files(&config, dir.path());
        let records = extract_explicit_evidence(&graph, &test_files, dir.path());

        assert_eq!(records.len(), 1);
        let rec = &records[0];
        assert_eq!(rec.evidence_kind, EvidenceKind::FileGlob);
        assert!(rec.targets.iter().any(|c| c.target_id == "login-success"));
    }

    /// Multiple criteria with contextual `VerifiedBy` each produce evidence.
    #[test]
    fn multiple_contextual_verified_by() {
        let dir = TempDir::new().unwrap();
        write_test_file(&dir, "tests/login_test.rs", "// supersigil: auth:login\n");
        write_test_file(
            &dir,
            "tests/session_test.rs",
            "// supersigil: auth:session\n",
        );

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![
                    make_criterion_with_verified_by(
                        "login",
                        make_verified_by_tag("auth:login", 11),
                        10,
                    ),
                    make_criterion_with_verified_by(
                        "session",
                        make_verified_by_tag("auth:session", 21),
                        20,
                    ),
                ],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        config.tests = Some(vec!["tests/**/*.rs".into()]);

        let test_files = crate::resolve_test_files(&config, dir.path());
        let records = extract_explicit_evidence(&graph, &test_files, dir.path());

        assert_eq!(
            records.len(),
            2,
            "expected 2 evidence records (1 per criterion), got {}",
            records.len()
        );

        let login_rec = records
            .iter()
            .find(|r| r.targets.iter().any(|c| c.target_id == "login"))
            .expect("should have login evidence");
        assert_eq!(login_rec.evidence_kind, EvidenceKind::Tag);

        let session_rec = records
            .iter()
            .find(|r| r.targets.iter().any(|c| c.target_id == "session"))
            .expect("should have session evidence");
        assert_eq!(session_rec.evidence_kind, EvidenceKind::Tag);
    }

    // -----------------------------------------------------------------------
    // 7. No matches → empty evidence vec
    // -----------------------------------------------------------------------

    /// A tag that matches zero test files should produce zero evidence records
    /// (the existing `zero_tag_matches` rule handles the warning separately).
    #[test]
    fn no_tag_matches_produces_empty_evidence() {
        let dir = TempDir::new().unwrap();
        write_test_file(
            &dir,
            "tests/unrelated.rs",
            "// no supersigil tags here\nfn test_unrelated() {}\n",
        );

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_tag("auth:nonexistent-tag", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        config.tests = Some(vec!["tests/**/*.rs".into()]);

        let test_files = crate::resolve_test_files(&config, dir.path());
        let records = extract_explicit_evidence(&graph, &test_files, dir.path());

        assert!(
            records.is_empty(),
            "expected empty evidence vec for zero tag matches, got {} records",
            records.len(),
        );
    }
}
