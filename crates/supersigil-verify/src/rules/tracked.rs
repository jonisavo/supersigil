use std::path::Path;

use supersigil_core::DocumentGraph;

use crate::report::{Finding, RuleName};

/// For each `TrackedFiles` component, expand globs. Emit `EmptyTrackedGlob`
/// for zero-match globs.
pub fn check_empty_globs(graph: &DocumentGraph, project_root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (doc_id, globs) in graph.all_tracked_files() {
        for glob_pattern in globs {
            if supersigil_core::expand_glob(glob_pattern, project_root).is_empty() {
                findings.push(Finding::new(
                    RuleName::EmptyTrackedGlob,
                    Some(doc_id.to_owned()),
                    format!("TrackedFiles glob `{glob_pattern}` in `{doc_id}` matched zero files"),
                    None,
                ));
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
}
