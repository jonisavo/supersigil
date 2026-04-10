use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use supersigil_core::DocumentGraph;

use crate::git;

/// A spec document affected by file changes since a git ref.
#[derive(Debug, Clone, Serialize)]
pub struct AffectedDocument {
    /// The document ID (e.g. `"design/auth"`).
    pub id: String,
    /// Path to the spec file, relative to the project root.
    pub path: PathBuf,
    /// Glob patterns from `TrackedFiles` that matched changed files.
    pub matched_globs: Vec<String>,
    /// Changed files that matched the tracked globs.
    pub changed_files: Vec<PathBuf>,
    /// If `Some`, this document is transitively affected via the named document.
    /// If `None`, this document is directly affected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transitive_from: Option<String>,
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
                transitive_from: None,
            });
        }
    }

    // -- One-hop transitive staleness extension ----------------------------
    // For each directly affected doc, find docs that reference it and add
    // them as transitively affected (unless already in the direct set).
    let direct_ids: BTreeSet<&str> = results.iter().map(|d| d.id.as_str()).collect();

    let mut transitive = Vec::new();
    for direct_doc in &results {
        for referencing_id in graph.references(&direct_doc.id, None) {
            if direct_ids.contains(referencing_id.as_str()) {
                continue;
            }
            // Avoid duplicate transitive entries (a doc may reference multiple
            // directly affected docs — keep only the first association).
            if transitive
                .iter()
                .any(|t: &AffectedDocument| t.id == *referencing_id)
            {
                continue;
            }
            let doc = graph
                .document(referencing_id)
                .expect("referencing doc exists in graph");
            let path = doc
                .path
                .strip_prefix(project_root)
                .unwrap_or(&doc.path)
                .to_path_buf();
            transitive.push(AffectedDocument {
                id: referencing_id.clone(),
                path,
                matched_globs: Vec::new(),
                changed_files: Vec::new(),
                transitive_from: Some(direct_doc.id.clone()),
            });
        }
    }
    results.extend(transitive);

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use supersigil_rust::verifies;

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
        doc.path = dir.path().join("specs/design/auth.md");

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

    #[verifies("decision-components/req#req-6-3")]
    #[test]
    fn affected_detects_tracked_files_nested_in_decision() {
        let (dir, initial) = init_repo();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "code").unwrap();
        git_commit(&dir);

        // TrackedFiles is nested inside a Decision component.
        let tracked = make_tracked_files("src/**/*.rs", 5);
        let decision = make_decision(vec![tracked], 3);
        let docs = vec![make_doc("design/auth", vec![decision])];
        let graph = build_test_graph(docs);

        let result = affected(&graph, dir.path(), &initial, false, false).unwrap();
        assert_eq!(
            result.len(),
            1,
            "document with nested TrackedFiles should be affected"
        );
        assert_eq!(result[0].id, "design/auth");
        assert!(!result[0].changed_files.is_empty());
        assert!(!result[0].matched_globs.is_empty());
    }

    #[test]
    fn affected_nested_tracked_files_behaves_identically_to_top_level() {
        let (dir, initial) = init_repo();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "code").unwrap();
        git_commit(&dir);

        // Top-level TrackedFiles.
        let docs_top = vec![make_doc(
            "design/top",
            vec![make_tracked_files("src/**/*.rs", 5)],
        )];
        // Identical glob nested inside a Decision.
        let tracked = make_tracked_files("src/**/*.rs", 5);
        let decision = make_decision(vec![tracked], 3);
        let docs_nested = vec![make_doc("design/nested", vec![decision])];

        let graph_top = build_test_graph(docs_top);
        let graph_nested = build_test_graph(docs_nested);

        let result_top = affected(&graph_top, dir.path(), &initial, false, false).unwrap();
        let result_nested = affected(&graph_nested, dir.path(), &initial, false, false).unwrap();

        assert_eq!(result_top.len(), 1);
        assert_eq!(
            result_nested.len(),
            1,
            "nested TrackedFiles should behave identically to top-level"
        );
        assert_eq!(result_top[0].matched_globs, result_nested[0].matched_globs);
        assert_eq!(result_top[0].changed_files, result_nested[0].changed_files);
    }

    // -- Transitive staleness tests ------------------------------------------

    #[verifies("decision-components/req#req-6-4")]
    #[test]
    fn affected_includes_transitive_via_references_reverse() {
        let (dir, initial) = init_repo();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "code").unwrap();
        git_commit(&dir);

        // design/auth is directly affected (has TrackedFiles matching src/**/*.rs).
        // arch/adr references design/auth, so it should be transitively affected.
        let docs = vec![
            make_doc("design/auth", vec![make_tracked_files("src/**/*.rs", 5)]),
            make_doc("arch/adr", vec![make_references("design/auth", 3)]),
        ];
        let graph = build_test_graph(docs);
        let result = affected(&graph, dir.path(), &initial, false, false).unwrap();

        assert_eq!(result.len(), 2, "should include direct + transitive");

        let direct = result.iter().find(|d| d.id == "design/auth").unwrap();
        assert!(
            direct.transitive_from.is_none(),
            "directly affected doc should have transitive_from = None"
        );

        let transitive = result.iter().find(|d| d.id == "arch/adr").unwrap();
        assert_eq!(
            transitive.transitive_from.as_deref(),
            Some("design/auth"),
            "transitively affected doc should reference the direct doc"
        );
        assert!(
            transitive.matched_globs.is_empty(),
            "transitive doc should have no matched globs"
        );
        assert!(
            transitive.changed_files.is_empty(),
            "transitive doc should have no changed files"
        );
    }

    #[test]
    fn affected_deduplicates_direct_over_transitive() {
        let (dir, initial) = init_repo();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "code").unwrap();
        git_commit(&dir);

        // Both docs are directly affected. design/api also references design/auth,
        // but it should appear only as directly affected (not duplicated as transitive).
        let docs = vec![
            make_doc("design/auth", vec![make_tracked_files("src/**/*.rs", 5)]),
            make_doc(
                "design/api",
                vec![
                    make_tracked_files("src/**/*.rs", 5),
                    make_references("design/auth", 10),
                ],
            ),
        ];
        let graph = build_test_graph(docs);
        let result = affected(&graph, dir.path(), &initial, false, false).unwrap();

        assert_eq!(result.len(), 2, "both should be present exactly once");
        for doc in &result {
            assert!(
                doc.transitive_from.is_none(),
                "doc {} should be direct, not transitive",
                doc.id
            );
        }
    }

    #[test]
    fn affected_no_transitive_when_no_references() {
        let (dir, initial) = init_repo();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "code").unwrap();
        git_commit(&dir);

        // design/auth is directly affected. design/api has no reference to it.
        let docs = vec![
            make_doc("design/auth", vec![make_tracked_files("src/**/*.rs", 5)]),
            make_doc("design/api", vec![]),
        ];
        let graph = build_test_graph(docs);
        let result = affected(&graph, dir.path(), &initial, false, false).unwrap();

        assert_eq!(result.len(), 1, "only directly affected doc");
        assert_eq!(result[0].id, "design/auth");
        assert!(result[0].transitive_from.is_none());
    }

    #[test]
    fn affected_existing_docs_have_transitive_from_none() {
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
        assert!(
            result[0].transitive_from.is_none(),
            "directly affected docs should have transitive_from = None"
        );
    }
}
