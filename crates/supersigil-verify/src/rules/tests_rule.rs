use std::path::{Path, PathBuf};

use supersigil_core::{DocumentGraph, SpecDocument, split_list_attribute};

use crate::report::{Finding, RuleName};
use crate::rules::{find_components, has_component};
use crate::scan::scan_for_tag;

// ---------------------------------------------------------------------------
// check_unverified
// ---------------------------------------------------------------------------

/// For each document that has a `Validates` component but no `VerifiedBy`
/// component, emit `UnverifiedValidation`.
pub fn check_unverified(graph: &DocumentGraph) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (doc_id, doc) in graph.documents() {
        let has_validates = has_component(&doc.components, "Validates");
        let has_verified_by = has_component(&doc.components, "VerifiedBy");

        if has_validates && !has_verified_by {
            findings.push(Finding {
                rule: RuleName::UnverifiedValidation,
                doc_id: Some(doc_id.to_owned()),
                message: format!("document `{doc_id}` has Validates but no VerifiedBy"),
                effective_severity: RuleName::UnverifiedValidation.default_severity(),
                raw_severity: RuleName::UnverifiedValidation.default_severity(),
                position: None,
            });
        }
    }

    findings
}

// ---------------------------------------------------------------------------
// check_file_globs
// ---------------------------------------------------------------------------

/// For each `VerifiedBy` with `strategy="file-glob"`, expand `paths` as globs
/// relative to `project_root`. Emit `MissingTestFiles` when a glob matches
/// zero files.
pub fn check_file_globs(docs: &[&SpecDocument], project_root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();

    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        for vb in find_components(&doc.components, "VerifiedBy") {
            let strategy = vb.attributes.get("strategy").map(String::as_str);
            if strategy != Some("file-glob") {
                continue;
            }

            let Some(paths_raw) = vb.attributes.get("paths") else {
                findings.push(Finding {
                    rule: RuleName::MissingTestFiles,
                    doc_id: Some(doc_id.clone()),
                    message: format!("VerifiedBy file-glob in `{doc_id}` has no paths attribute"),
                    effective_severity: RuleName::MissingTestFiles.default_severity(),
                    raw_severity: RuleName::MissingTestFiles.default_severity(),
                    position: Some(vb.position),
                });
                continue;
            };

            let Ok(paths) = split_list_attribute(paths_raw) else {
                continue;
            };

            for path in &paths {
                let pattern = project_root.join(path).to_string_lossy().to_string();
                let matches = glob::glob(&pattern)
                    .map(|entries| entries.filter_map(Result::ok).count())
                    .unwrap_or(0);

                if matches == 0 {
                    findings.push(Finding {
                        rule: RuleName::MissingTestFiles,
                        doc_id: Some(doc_id.clone()),
                        message: format!(
                            "VerifiedBy file-glob in `{doc_id}` matched zero files (path: {path})"
                        ),
                        effective_severity: RuleName::MissingTestFiles.default_severity(),
                        raw_severity: RuleName::MissingTestFiles.default_severity(),
                        position: Some(vb.position),
                    });
                }
            }
        }
    }

    findings
}

// ---------------------------------------------------------------------------
// check_tags
// ---------------------------------------------------------------------------

/// For each `VerifiedBy` with `strategy="tag"`, use the scanner to find
/// matches. Emit `ZeroTagMatches` for zero matches.
pub fn check_tags(docs: &[&SpecDocument], test_files: &[PathBuf]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        for vb in find_components(&doc.components, "VerifiedBy") {
            let strategy = vb.attributes.get("strategy").map(String::as_str);
            if strategy != Some("tag") {
                continue;
            }

            let Some(tag) = vb.attributes.get("tag") else {
                findings.push(Finding {
                    rule: RuleName::ZeroTagMatches,
                    doc_id: Some(doc_id.clone()),
                    message: format!("VerifiedBy tag in `{doc_id}` has no tag attribute"),
                    effective_severity: RuleName::ZeroTagMatches.default_severity(),
                    raw_severity: RuleName::ZeroTagMatches.default_severity(),
                    position: Some(vb.position),
                });
                continue;
            };

            let matches = scan_for_tag(tag, test_files);
            if matches.is_empty() {
                findings.push(Finding {
                    rule: RuleName::ZeroTagMatches,
                    doc_id: Some(doc_id.clone()),
                    message: format!(
                        "VerifiedBy tag `{tag}` in `{doc_id}` matched zero test files"
                    ),
                    effective_severity: RuleName::ZeroTagMatches.default_severity(),
                    raw_severity: RuleName::ZeroTagMatches.default_severity(),
                    position: Some(vb.position),
                });
            }
        }
    }

    findings
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // check_unverified
    // -----------------------------------------------------------------------

    #[test]
    fn validates_without_verified_by_emits_finding() {
        let docs = vec![
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
            make_doc("prop/auth", vec![make_validates("req/auth#req-1", 5)]),
        ];
        let graph = build_test_graph(docs);
        let findings = check_unverified(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::UnverifiedValidation);
        assert_eq!(findings[0].doc_id.as_deref(), Some("prop/auth"));
    }

    #[test]
    fn validates_with_verified_by_is_clean() {
        let docs = vec![
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("req-1", 10)],
                    9,
                )],
            ),
            make_doc(
                "prop/auth",
                vec![
                    make_validates("req/auth#req-1", 5),
                    make_verified_by_tag("prop:auth", 6),
                ],
            ),
        ];
        let graph = build_test_graph(docs);
        let findings = check_unverified(&graph);
        assert!(findings.is_empty());
    }

    #[test]
    fn doc_without_validates_is_not_flagged() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let findings = check_unverified(&graph);
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // check_file_globs
    // -----------------------------------------------------------------------

    #[test]
    fn file_glob_matching_zero_files_emits_finding() {
        let dir = TempDir::new().unwrap();
        let docs = [make_doc(
            "prop/auth",
            vec![
                make_validates("req/auth#req-1", 5),
                make_verified_by_glob("tests/nonexistent/**/*.rs", 6),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_file_globs(&doc_refs, dir.path());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::MissingTestFiles);
    }

    #[test]
    fn file_glob_matching_existing_files_is_clean() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("tests/auth_test.rs"), "test").unwrap();
        let docs = [make_doc(
            "prop/auth",
            vec![
                make_validates("req/auth#req-1", 5),
                make_verified_by_glob("tests/auth_test.rs", 6),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_file_globs(&doc_refs, dir.path());
        assert!(findings.is_empty());
    }

    #[test]
    fn file_glob_ignores_tag_strategy() {
        let dir = TempDir::new().unwrap();
        let docs = [make_doc(
            "prop/auth",
            vec![
                make_validates("req/auth#req-1", 5),
                make_verified_by_tag("prop:auth", 6),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_file_globs(&doc_refs, dir.path());
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // check_tags
    // -----------------------------------------------------------------------

    #[test]
    fn tag_with_zero_matches_emits_finding() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("tests/test.rs"), "// no tags here\n").unwrap();
        let docs = [make_doc(
            "prop/auth",
            vec![
                make_validates("req/auth#req-1", 5),
                make_verified_by_tag("prop:auth-login", 6),
            ],
        )];
        let test_files = vec![dir.path().join("tests/test.rs")];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_tags(&doc_refs, &test_files);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::ZeroTagMatches);
    }

    #[test]
    fn tag_with_matches_is_clean() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(
            dir.path().join("tests/test.rs"),
            "// supersigil: prop:auth-login\n",
        )
        .unwrap();
        let docs = [make_doc(
            "prop/auth",
            vec![
                make_validates("req/auth#req-1", 5),
                make_verified_by_tag("prop:auth-login", 6),
            ],
        )];
        let test_files = vec![dir.path().join("tests/test.rs")];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_tags(&doc_refs, &test_files);
        assert!(findings.is_empty());
    }

    #[test]
    fn file_glob_one_stale_one_healthy_emits_finding() {
        // If a VerifiedBy has multiple paths and one is stale (matches zero),
        // that stale glob should be reported even if the other matches.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests/active")).unwrap();
        std::fs::write(dir.path().join("tests/active/test.rs"), "test").unwrap();
        // tests/old/ does NOT exist — stale glob
        let docs = [make_doc(
            "prop/auth",
            vec![
                make_validates("req/auth#req-1", 5),
                make_verified_by_glob("tests/active/**/*.rs, tests/old/**/*.rs", 6),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_file_globs(&doc_refs, dir.path());
        assert!(
            !findings.is_empty(),
            "stale glob 'tests/old/**/*.rs' should produce a finding even when 'tests/active/**/*.rs' matches"
        );
        assert_eq!(findings[0].rule, RuleName::MissingTestFiles);
    }

    #[test]
    fn tag_check_ignores_file_glob_strategy() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("tests/test.rs"), "// no tags\n").unwrap();
        let docs = [make_doc(
            "prop/auth",
            vec![
                make_validates("req/auth#req-1", 5),
                make_verified_by_glob("tests/**/*.rs", 6),
            ],
        )];
        let test_files = vec![dir.path().join("tests/test.rs")];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_tags(&doc_refs, &test_files);
        assert!(findings.is_empty());
    }
}
