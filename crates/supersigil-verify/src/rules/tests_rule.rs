use std::collections::HashSet;
use std::path::Path;

use supersigil_core::{ExtractedComponent, SpecDocument, split_list_attribute};

use crate::report::{Finding, RuleName};
use crate::rules::find_criterion_nested_verified_by;
use crate::scan::TagMatch;

// ---------------------------------------------------------------------------
// check (combined entry point — traverses each doc's component tree once)
// ---------------------------------------------------------------------------

/// Run both `file-glob` and `tag` `VerifiedBy` checks in a single pass,
/// computing `find_criterion_nested_verified_by` once per document.
pub fn check(
    docs: &[&SpecDocument],
    project_root: &Path,
    tag_matches: &[TagMatch],
) -> Vec<Finding> {
    let known_tags: HashSet<&str> = tag_matches.iter().map(|m| m.tag.as_str()).collect();
    let mut findings = Vec::new();

    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        let criterion_nested = find_criterion_nested_verified_by(&doc.components);
        check_file_globs_inner(doc_id, &criterion_nested, project_root, &mut findings);
        check_tags_inner(doc_id, &criterion_nested, &known_tags, &mut findings);
    }

    findings
}

// ---------------------------------------------------------------------------
// check_file_globs
// ---------------------------------------------------------------------------

/// For each `VerifiedBy` with `strategy="file-glob"`, expand `paths` as globs
/// relative to `project_root`. Emit `MissingTestFiles` when a glob matches
/// zero files.
///
/// Only criterion-nested `<VerifiedBy>` components are checked. Document-level
/// placement is caught by `check_verified_by_placement` in `structural.rs`.
#[cfg(test)]
fn check_file_globs(docs: &[&SpecDocument], project_root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();

    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        let criterion_nested = find_criterion_nested_verified_by(&doc.components);
        check_file_globs_inner(doc_id, &criterion_nested, project_root, &mut findings);
    }

    findings
}

fn check_file_globs_inner(
    doc_id: &str,
    criterion_nested: &[&ExtractedComponent],
    project_root: &Path,
    findings: &mut Vec<Finding>,
) {
    for vb in criterion_nested {
        let strategy = vb.attributes.get("strategy").map(String::as_str);
        if strategy != Some("file-glob") {
            continue;
        }

        let Some(paths_raw) = vb.attributes.get("paths") else {
            findings.push(Finding::new(
                RuleName::MissingTestFiles,
                Some(doc_id.to_owned()),
                format!("VerifiedBy file-glob in `{doc_id}` has no paths attribute"),
                Some(vb.position),
            ));
            continue;
        };

        let Ok(paths) = split_list_attribute(paths_raw) else {
            continue;
        };

        for path in &paths {
            if supersigil_core::expand_glob(path, project_root).is_empty() {
                findings.push(Finding::new(
                    RuleName::MissingTestFiles,
                    Some(doc_id.to_owned()),
                    format!("VerifiedBy file-glob in `{doc_id}` matched zero files (path: {path})"),
                    Some(vb.position),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// check_tags
// ---------------------------------------------------------------------------

/// For each `VerifiedBy` with `strategy="tag"`, check the pre-scanned tag
/// matches for a hit. Emit `ZeroTagMatches` for zero matches.
///
/// `tag_matches` should be pre-computed via [`crate::scan::scan_all_tags`] to
/// avoid redundant per-tag file scanning.
///
/// Only criterion-nested `<VerifiedBy>` components are checked. Document-level
/// placement is caught by `check_verified_by_placement` in `structural.rs`.
#[cfg(test)]
fn check_tags(docs: &[&SpecDocument], tag_matches: &[TagMatch]) -> Vec<Finding> {
    let known_tags: HashSet<&str> = tag_matches.iter().map(|m| m.tag.as_str()).collect();

    let mut findings = Vec::new();

    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        let criterion_nested = find_criterion_nested_verified_by(&doc.components);
        check_tags_inner(doc_id, &criterion_nested, &known_tags, &mut findings);
    }

    findings
}

fn check_tags_inner(
    doc_id: &str,
    criterion_nested: &[&ExtractedComponent],
    known_tags: &HashSet<&str>,
    findings: &mut Vec<Finding>,
) {
    for vb in criterion_nested {
        let strategy = vb.attributes.get("strategy").map(String::as_str);
        if strategy != Some("tag") {
            continue;
        }

        let Some(tag) = vb.attributes.get("tag") else {
            findings.push(Finding::new(
                RuleName::ZeroTagMatches,
                Some(doc_id.to_owned()),
                format!("VerifiedBy tag in `{doc_id}` has no tag attribute"),
                Some(vb.position),
            ));
            continue;
        };

        if !known_tags.contains(tag.as_str()) {
            findings.push(Finding::new(
                RuleName::ZeroTagMatches,
                Some(doc_id.to_owned()),
                format!("VerifiedBy tag `{tag}` in `{doc_id}` matched zero test files"),
                Some(vb.position),
            ));
        }
    }
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
        let tag_matches = crate::scan::scan_all_tags(&test_files);
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_tags(&doc_refs, &tag_matches);
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
        let tag_matches = crate::scan::scan_all_tags(&test_files);
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_tags(&doc_refs, &tag_matches);
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
        let tag_matches = crate::scan::scan_all_tags(&test_files);
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_tags(&doc_refs, &tag_matches);
        assert!(findings.is_empty());
    }
}
