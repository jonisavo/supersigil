use supersigil_core::DocumentGraph;

use crate::glob_resolver::GlobResolver;
use crate::report::{Finding, RuleName};

/// For each `TrackedFiles` component, expand globs. Emit `EmptyTrackedGlob`
/// for zero-match globs.
#[cfg(test)]
pub fn check_empty_globs(graph: &DocumentGraph, project_root: &std::path::Path) -> Vec<Finding> {
    let mut glob_resolver = GlobResolver::new(project_root);
    check_empty_globs_with_resolver(graph, None, &mut glob_resolver)
}

pub(crate) fn check_empty_globs_with_resolver(
    graph: &DocumentGraph,
    doc_ids: Option<&[String]>,
    glob_resolver: &mut GlobResolver,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let scoped_ids = doc_ids.map(|ids| {
        ids.iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>()
    });

    for (doc_id, globs) in graph.all_tracked_files() {
        if scoped_ids.as_ref().is_some_and(|ids| !ids.contains(doc_id)) {
            continue;
        }
        for glob_pattern in globs {
            if glob_resolver.expand(glob_pattern).is_empty() {
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
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
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
    fn repeated_tracked_globs_resolve_once_per_run() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);

        fn counting_loader(pattern: &str, _base_dir: &std::path::Path) -> Vec<PathBuf> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            vec![PathBuf::from(pattern)]
        }

        CALLS.store(0, Ordering::SeqCst);
        let dir = TempDir::new().unwrap();
        let docs = vec![
            make_doc("design/auth", vec![make_tracked_files("src/**/*.rs", 5)]),
            make_doc("design/api", vec![make_tracked_files("src/**/*.rs", 5)]),
        ];
        let graph = build_test_graph(docs);
        let mut glob_resolver =
            crate::glob_resolver::GlobResolver::with_loader_for_tests(dir.path(), counting_loader);

        let findings = check_empty_globs_with_resolver(&graph, None, &mut glob_resolver);

        assert!(findings.is_empty());
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            1,
            "repeated tracked globs should be expanded once per verify run",
        );
    }
}
