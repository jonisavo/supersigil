use std::path::{Path, PathBuf};

use supersigil_core::{SpecDocument, split_list_attribute};

use crate::report::{Finding, RuleName};
use crate::rules::find_criterion_nested_verified_by;
use crate::scan::scan_for_tag;

// ---------------------------------------------------------------------------
// check_file_globs
// ---------------------------------------------------------------------------

/// For each `VerifiedBy` with `strategy="file-glob"`, expand `paths` as globs
/// relative to `project_root`. Emit `MissingTestFiles` when a glob matches
/// zero files.
///
/// Only criterion-nested `<VerifiedBy>` components are checked. Document-level
/// placement is caught by `check_verified_by_placement` in `structural.rs`.
pub fn check_file_globs(docs: &[&SpecDocument], project_root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();

    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        let criterion_nested = find_criterion_nested_verified_by(&doc.components);

        for vb in &criterion_nested {
            let strategy = vb.attributes.get("strategy").map(String::as_str);
            if strategy != Some("file-glob") {
                continue;
            }

            let Some(paths_raw) = vb.attributes.get("paths") else {
                findings.push(Finding::new(
                    RuleName::MissingTestFiles,
                    Some(doc_id.clone()),
                    format!("VerifiedBy file-glob in `{doc_id}` has no paths attribute"),
                    Some(vb.position),
                ));
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
                    findings.push(Finding::new(
                        RuleName::MissingTestFiles,
                        Some(doc_id.clone()),
                        format!(
                            "VerifiedBy file-glob in `{doc_id}` matched zero files (path: {path})"
                        ),
                        Some(vb.position),
                    ));
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
///
/// Only criterion-nested `<VerifiedBy>` components are checked. Document-level
/// placement is caught by `check_verified_by_placement` in `structural.rs`.
pub fn check_tags(docs: &[&SpecDocument], test_files: &[PathBuf]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        let criterion_nested = find_criterion_nested_verified_by(&doc.components);

        for vb in &criterion_nested {
            let strategy = vb.attributes.get("strategy").map(String::as_str);
            if strategy != Some("tag") {
                continue;
            }

            let Some(tag) = vb.attributes.get("tag") else {
                findings.push(Finding::new(
                    RuleName::ZeroTagMatches,
                    Some(doc_id.clone()),
                    format!("VerifiedBy tag in `{doc_id}` has no tag attribute"),
                    Some(vb.position),
                ));
                continue;
            };

            let matches = scan_for_tag(tag, test_files);
            if matches.is_empty() {
                findings.push(Finding::new(
                    RuleName::ZeroTagMatches,
                    Some(doc_id.clone()),
                    format!("VerifiedBy tag `{tag}` in `{doc_id}` matched zero test files"),
                    Some(vb.position),
                ));
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
    // check_file_globs — criterion-nested VerifiedBy
    // -----------------------------------------------------------------------

    #[test]
    fn file_glob_matching_zero_files_emits_finding() {
        let dir = TempDir::new().unwrap();
        let docs = [make_doc(
            "prop/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_glob("tests/nonexistent/**/*.rs", 8),
                    7,
                )],
                6,
            )],
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
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_glob("tests/auth_test.rs", 8),
                    7,
                )],
                6,
            )],
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
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_tag("prop:auth", 8),
                    7,
                )],
                6,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_file_globs(&doc_refs, dir.path());
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // check_tags — criterion-nested VerifiedBy
    // -----------------------------------------------------------------------

    #[test]
    fn tag_with_zero_matches_emits_finding() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("tests/test.rs"), "// no tags here\n").unwrap();
        let docs = [make_doc(
            "prop/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_tag("prop:auth-login", 8),
                    7,
                )],
                6,
            )],
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
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_tag("prop:auth-login", 8),
                    7,
                )],
                6,
            )],
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
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_glob("tests/active/**/*.rs, tests/old/**/*.rs", 8),
                    7,
                )],
                6,
            )],
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
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_glob("tests/**/*.rs", 8),
                    7,
                )],
                6,
            )],
        )];
        let test_files = vec![dir.path().join("tests/test.rs")];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_tags(&doc_refs, &test_files);
        assert!(findings.is_empty());
    }
}
