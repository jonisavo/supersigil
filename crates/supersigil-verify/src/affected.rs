use std::path::{Path, PathBuf};

use serde::Serialize;
use supersigil_core::DocumentGraph;

use crate::git;

#[derive(Debug, Clone, Serialize)]
pub struct AffectedDocument {
    pub id: String,
    pub path: PathBuf,
    pub matched_globs: Vec<String>,
    pub changed_files: Vec<PathBuf>,
}

/// Return documents whose `TrackedFiles` globs match any changed files since
/// a git ref.
///
/// # Errors
///
/// Propagates [`git::GitError`] when the git diff cannot be computed.
///
/// # Panics
///
/// Panics if a document ID from the tracked-files index is missing from the
/// graph (should never happen in a well-formed graph).
pub fn affected(
    graph: &DocumentGraph,
    project_root: &Path,
    since_ref: &str,
    committed_only: bool,
    use_merge_base: bool,
) -> Result<Vec<AffectedDocument>, git::GitError> {
    let changed = git::changed_files(project_root, since_ref, committed_only, use_merge_base)?;
    let mut results = Vec::new();

    for (doc_id, globs) in graph.all_tracked_files() {
        let mut matched_globs = Vec::new();
        let mut matched_files = Vec::new();

        for glob_pattern in globs {
            let pattern_str = project_root
                .join(glob_pattern)
                .to_string_lossy()
                .to_string();
            let Ok(pattern) = glob::Pattern::new(&pattern_str) else {
                continue;
            };

            for changed_file in &changed {
                let full = project_root.join(changed_file);
                if pattern.matches_path(&full) {
                    if !matched_globs.contains(glob_pattern) {
                        matched_globs.push(glob_pattern.clone());
                    }
                    if !matched_files.contains(changed_file) {
                        matched_files.push(changed_file.clone());
                    }
                }
            }
        }

        if !matched_globs.is_empty() {
            let doc = graph.document(doc_id).expect("doc exists in graph");
            let path = doc
                .path
                .strip_prefix(project_root)
                .unwrap_or(&doc.path)
                .to_path_buf();
            results.push(AffectedDocument {
                id: doc_id.to_owned(),
                path,
                matched_globs,
                changed_files: matched_files,
            });
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn affected_returns_documents_with_matching_tracked_files() {
        let (dir, initial) = init_repo();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "code").unwrap();
        git_commit(&dir);

        let docs = vec![make_doc(
            "design/auth",
            vec![make_tracked_files("src/**/*.rs", 5)],
        )];
        let graph = build_test_graph(docs);
        let result = affected(&graph, dir.path(), &initial, false, false).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "design/auth");
        assert!(!result[0].changed_files.is_empty());
        assert!(!result[0].matched_globs.is_empty());
    }

    #[test]
    fn affected_excludes_documents_with_no_matching_changes() {
        let (dir, initial) = init_repo();
        // No changes to src/ files
        let docs = vec![make_doc(
            "design/auth",
            vec![make_tracked_files("src/**/*.rs", 5)],
        )];
        let graph = build_test_graph(docs);
        let result = affected(&graph, dir.path(), &initial, false, false).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn affected_tracks_multiple_globs_per_document() {
        let (dir, initial) = init_repo();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("tests/test.rs"), "fn test() {}").unwrap();
        git_commit(&dir);

        let docs = vec![make_doc(
            "design/auth",
            vec![
                make_tracked_files("src/**/*.rs", 5),
                make_tracked_files("tests/**/*.rs", 10),
            ],
        )];
        let graph = build_test_graph(docs);
        let result = affected(&graph, dir.path(), &initial, false, false).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].matched_globs.len(), 2);
        assert_eq!(result[0].changed_files.len(), 2);
    }

    #[test]
    fn affected_paths_are_relative_to_project_root() {
        let (dir, initial) = init_repo();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "code").unwrap();
        git_commit(&dir);

        // Simulate the real loader which sets absolute paths on SpecDocument
        let mut doc = make_doc("design/auth", vec![make_tracked_files("src/**/*.rs", 5)]);
        doc.path = dir.path().join("specs/design/auth.mdx");

        let docs = vec![doc];
        let graph = build_test_graph(docs);
        let result = affected(&graph, dir.path(), &initial, false, false).unwrap();
        assert_eq!(result.len(), 1);
        // The path should be relative, not absolute
        assert!(
            result[0].path.is_relative(),
            "AffectedDocument.path should be relative to project_root, got: {:?}",
            result[0].path,
        );
    }

    #[test]
    fn affected_returns_multiple_documents() {
        let (dir, initial) = init_repo();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "code").unwrap();
        git_commit(&dir);

        let docs = vec![
            make_doc("design/auth", vec![make_tracked_files("src/**/*.rs", 5)]),
            make_doc("design/api", vec![make_tracked_files("src/**/*.rs", 5)]),
        ];
        let graph = build_test_graph(docs);
        let result = affected(&graph, dir.path(), &initial, false, false).unwrap();
        assert_eq!(result.len(), 2);
    }
}
