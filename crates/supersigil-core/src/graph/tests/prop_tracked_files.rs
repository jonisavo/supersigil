//! Property tests for `TrackedFiles` indexing (pipeline stage 9).
//!
//! Properties 21 and 27 from the design document.

use std::collections::{HashMap, HashSet};

use proptest::prelude::*;

use crate::graph::build_graph;
use crate::graph::tests::generators::{
    arb_id, make_doc_with_path, make_tracked_files_component, single_project_config,
};

// ---------------------------------------------------------------------------
// Property 21: TrackedFiles index completeness
// ---------------------------------------------------------------------------

proptest! {
    /// A document with a single `TrackedFiles` component should have all its
    /// path globs retrievable via `tracked_files(doc_id)`.
    ///
    /// Validates: Requirements 12.1, 12.2
    #[test]
    fn prop_tracked_files_single_component(
        doc_id in arb_id(),
    ) {
        let config = single_project_config();

        let doc = make_doc_with_path(
            &doc_id,
            &format!("specs/{doc_id}.md"),
            vec![make_tracked_files_component("src/**/*.rs, tests/**/*.rs", 1)],
        );

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let globs = graph.tracked_files(&doc_id);
        prop_assert!(globs.is_some(), "tracked_files should return Some for doc with TrackedFiles");
        let globs = globs.unwrap();
        prop_assert_eq!(globs.len(), 2);
        prop_assert!(globs.contains(&"src/**/*.rs".to_owned()));
        prop_assert!(globs.contains(&"tests/**/*.rs".to_owned()));
    }

    /// A document with multiple `TrackedFiles` components should have all
    /// path globs aggregated under the same document ID.
    ///
    /// Validates: Requirements 12.1, 12.2, 12.4
    #[test]
    fn prop_tracked_files_multiple_components_aggregated(
        doc_id in arb_id(),
    ) {
        let config = single_project_config();

        let doc = make_doc_with_path(
            &doc_id,
            &format!("specs/{doc_id}.md"),
            vec![
                make_tracked_files_component("src/**/*.rs", 1),
                make_tracked_files_component("tests/**/*.rs, docs/**/*.md", 2),
            ],
        );

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        let globs = graph.tracked_files(&doc_id)
            .expect("tracked_files should return Some");
        prop_assert_eq!(globs.len(), 3);
        prop_assert!(globs.contains(&"src/**/*.rs".to_owned()));
        prop_assert!(globs.contains(&"tests/**/*.rs".to_owned()));
        prop_assert!(globs.contains(&"docs/**/*.md".to_owned()));
    }

    /// A document with no `TrackedFiles` components should return `None`
    /// from `tracked_files`.
    ///
    /// Validates: Requirements 12.2
    #[test]
    fn prop_tracked_files_none_for_no_tracked_files(
        doc_id in arb_id(),
    ) {
        let config = single_project_config();

        let doc = make_doc_with_path(
            &doc_id,
            &format!("specs/{doc_id}.md"),
            vec![],
        );

        let graph = build_graph(vec![doc], &config)
            .expect("build_graph should succeed");

        prop_assert!(
            graph.tracked_files(&doc_id).is_none(),
            "tracked_files should return None for doc without TrackedFiles"
        );
    }
}

// ---------------------------------------------------------------------------
// Property 27: TrackedFiles index iteration
// ---------------------------------------------------------------------------

proptest! {
    /// Iterating `all_tracked_files()` should yield every `(doc_id, globs)`
    /// pair for documents that have `TrackedFiles` components.
    ///
    /// Validates: Requirements 12.3
    #[test]
    fn prop_all_tracked_files_yields_all_entries(
        id_a in arb_id(),
        id_b in arb_id(),
    ) {
        prop_assume!(id_a != id_b);

        let config = single_project_config();

        let doc_a = make_doc_with_path(
            &id_a,
            &format!("specs/{id_a}.md"),
            vec![make_tracked_files_component("src/**/*.rs", 1)],
        );
        let doc_b = make_doc_with_path(
            &id_b,
            &format!("specs/{id_b}.md"),
            vec![make_tracked_files_component("lib/**/*.rs, bin/**/*.rs", 1)],
        );

        let graph = build_graph(vec![doc_a, doc_b], &config)
            .expect("build_graph should succeed");

        let all: HashMap<&str, &[String]> = graph.all_tracked_files().collect();
        prop_assert_eq!(all.len(), 2, "should have entries for both docs");
        prop_assert!(all.contains_key(id_a.as_str()));
        prop_assert!(all.contains_key(id_b.as_str()));

        // Verify globs for doc_a.
        let globs_a = all[id_a.as_str()];
        prop_assert_eq!(globs_a.len(), 1);
        prop_assert!(globs_a.contains(&"src/**/*.rs".to_owned()));

        // Verify globs for doc_b.
        let globs_b = all[id_b.as_str()];
        prop_assert_eq!(globs_b.len(), 2);
        prop_assert!(globs_b.contains(&"lib/**/*.rs".to_owned()));
        prop_assert!(globs_b.contains(&"bin/**/*.rs".to_owned()));
    }

    /// Documents without `TrackedFiles` should not appear in
    /// `all_tracked_files()` iteration.
    ///
    /// Validates: Requirements 12.3
    #[test]
    fn prop_all_tracked_files_excludes_docs_without_tracked_files(
        id_with in arb_id(),
        id_without in arb_id(),
    ) {
        prop_assume!(id_with != id_without);

        let config = single_project_config();

        let doc_with = make_doc_with_path(
            &id_with,
            &format!("specs/{id_with}.md"),
            vec![make_tracked_files_component("src/**/*.rs", 1)],
        );
        let doc_without = make_doc_with_path(
            &id_without,
            &format!("specs/{id_without}.md"),
            vec![],
        );

        let graph = build_graph(vec![doc_with, doc_without], &config)
            .expect("build_graph should succeed");

        let ids: HashSet<&str> = graph.all_tracked_files().map(|(id, _)| id).collect();
        prop_assert!(ids.contains(id_with.as_str()));
        prop_assert!(
            !ids.contains(id_without.as_str()),
            "doc without TrackedFiles should not appear in all_tracked_files()"
        );
    }
}
