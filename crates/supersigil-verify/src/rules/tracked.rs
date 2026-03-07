use std::path::Path;

use supersigil_core::DocumentGraph;

use crate::report::{Finding, RuleName};

/// For each `TrackedFiles` component, expand globs. Emit `EmptyTrackedGlob`
/// for zero-match globs.
pub fn check_empty_globs(graph: &DocumentGraph, project_root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (doc_id, globs) in graph.all_tracked_files() {
        for glob_pattern in globs {
            let full_pattern = project_root
                .join(glob_pattern)
                .to_string_lossy()
                .to_string();
            let match_count = glob::glob(&full_pattern)
                .map(|paths| paths.filter_map(Result::ok).count())
                .unwrap_or(0);

            if match_count == 0 {
                findings.push(Finding {
                    rule: RuleName::EmptyTrackedGlob,
                    doc_id: Some(doc_id.to_owned()),
                    message: format!(
                        "TrackedFiles glob `{glob_pattern}` in `{doc_id}` matched zero files"
                    ),
                    effective_severity: RuleName::EmptyTrackedGlob.default_severity(),
                    raw_severity: RuleName::EmptyTrackedGlob.default_severity(),
                    position: None,
                });
            }
        }
    }

    findings
}

/// When `--since` is provided, compute changed files via git, match against
/// `TrackedFiles` globs, emit `StaleTrackedFiles`.
pub fn check_staleness(
    graph: &DocumentGraph,
    project_root: &Path,
    since_ref: &str,
    committed_only: bool,
    use_merge_base: bool,
) -> Vec<Finding> {
    let Ok(changed) =
        crate::git::changed_files(project_root, since_ref, committed_only, use_merge_base)
    else {
        return Vec::new(); // Git errors are non-fatal for this rule
    };

    let mut findings = Vec::new();

    for (doc_id, globs) in graph.all_tracked_files() {
        for glob_pattern in globs {
            let pattern_str = project_root
                .join(glob_pattern)
                .to_string_lossy()
                .to_string();
            let Ok(pattern) = glob::Pattern::new(&pattern_str) else {
                continue;
            };

            let stale_count = changed
                .iter()
                .filter(|f| pattern.matches_path(&project_root.join(f)))
                .count();

            if stale_count > 0 {
                findings.push(Finding {
                    rule: RuleName::StaleTrackedFiles,
                    doc_id: Some(doc_id.to_owned()),
                    message: format!(
                        "TrackedFiles glob `{glob_pattern}` in `{doc_id}` has {stale_count} changed file(s)",
                    ),
                    effective_severity: RuleName::StaleTrackedFiles.default_severity(),
                    raw_severity: RuleName::StaleTrackedFiles.default_severity(),
                    position: None,
                });
            }
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use tempfile::TempDir;

    #[test]
    fn glob_matching_no_files_emits_finding() {
        let dir = TempDir::new().unwrap();
        let docs = vec![make_doc(
            "design/auth",
            vec![make_tracked_files("src/nonexistent/**/*.rs", 5)],
        )];
        let graph = build_test_graph(docs);
        let findings = check_empty_globs(&graph, dir.path());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::EmptyTrackedGlob);
    }

    #[test]
    fn glob_matching_files_is_clean() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        let docs = vec![make_doc(
            "design/auth",
            vec![make_tracked_files("src/**/*.rs", 5)],
        )];
        let graph = build_test_graph(docs);
        let findings = check_empty_globs(&graph, dir.path());
        assert!(findings.is_empty());
    }

    #[test]
    fn tracked_file_changed_since_ref_emits_stale_finding() {
        let (dir, initial) = init_repo();
        // Create tracked file and commit it
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "initial").unwrap();
        git_commit(&dir);

        // Modify the tracked file
        std::fs::write(dir.path().join("src/lib.rs"), "modified").unwrap();
        git_commit(&dir);

        let docs = vec![make_doc(
            "design/auth",
            vec![make_tracked_files("src/**/*.rs", 5)],
        )];
        let graph = build_test_graph(docs);
        let findings = check_staleness(&graph, dir.path(), &initial, false, false);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::StaleTrackedFiles);
    }

    #[test]
    fn no_changes_means_no_stale_findings() {
        let (dir, initial) = init_repo();
        let docs = vec![make_doc(
            "design/auth",
            vec![make_tracked_files("src/**/*.rs", 5)],
        )];
        let graph = build_test_graph(docs);
        let findings = check_staleness(&graph, dir.path(), &initial, false, false);
        assert!(findings.is_empty());
    }
}
