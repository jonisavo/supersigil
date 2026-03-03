//! Property tests for document indexing (pipeline stage 1).

use std::path::PathBuf;

use proptest::prelude::*;

use crate::graph::tests::generators::{
    arb_config, arb_document_set, arb_id, arb_spec_document_with_id,
};
use crate::graph::{GraphError, build_graph};

// ---------------------------------------------------------------------------
// Property 1: Document index round-trip
// ---------------------------------------------------------------------------

proptest! {
    /// For any collection of SpecDocuments with unique IDs, building the graph
    /// and looking up each document by ID returns the original document.
    ///
    /// Validates: Requirements 1.1, 1.4
    #[test]
    fn prop_document_index_round_trip(
        docs in arb_document_set(5),
        config in arb_config(),
    ) {
        let originals: Vec<_> = docs
            .iter()
            .map(|d| (d.frontmatter.id.clone(), d.clone()))
            .collect();

        let graph = build_graph(docs, &config)
            .expect("build_graph should succeed with unique IDs");

        for (id, original) in &originals {
            let looked_up = graph
                .document(id)
                .unwrap_or_else(|| panic!("document `{id}` should be in the graph"));
            prop_assert_eq!(looked_up, original);
        }

        // Also verify the total count matches.
        let count = graph.documents().count();
        prop_assert_eq!(count, originals.len());
    }
}

// ---------------------------------------------------------------------------
// Property 2: Duplicate document ID detection
// ---------------------------------------------------------------------------

proptest! {
    /// When two or more documents share the same `frontmatter.id`,
    /// `build_graph` returns a `DuplicateId` error carrying the conflicting
    /// ID and all file paths that share it.
    ///
    /// Validates: Requirements 1.2
    #[test]
    fn prop_duplicate_document_id_detection(
        shared_id in arb_id(),
        doc_a in arb_spec_document_with_id("placeholder".to_owned(), Vec::new()),
        doc_b in arb_spec_document_with_id("placeholder".to_owned(), Vec::new()),
        config in arb_config(),
    ) {
        // Overwrite IDs and give each document a distinct path.
        let mut doc_a = doc_a;
        let mut doc_b = doc_b;
        doc_a.frontmatter.id = shared_id.clone();
        doc_b.frontmatter.id = shared_id.clone();
        doc_a.path = PathBuf::from(format!("specs/a/{shared_id}.mdx"));
        doc_b.path = PathBuf::from(format!("specs/b/{shared_id}.mdx"));

        let result = build_graph(vec![doc_a.clone(), doc_b.clone()], &config);
        let errors = result.expect_err("build_graph should fail on duplicate IDs");

        // There should be exactly one DuplicateId error for our shared ID.
        let dup_errors: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::DuplicateId { id, paths } if id == &shared_id => Some(paths),
                _ => None,
            })
            .collect();

        prop_assert_eq!(dup_errors.len(), 1, "expected exactly one DuplicateId error");

        let paths = dup_errors[0];
        prop_assert!(
            paths.contains(&doc_a.path),
            "DuplicateId should contain doc_a path"
        );
        prop_assert!(
            paths.contains(&doc_b.path),
            "DuplicateId should contain doc_b path"
        );
    }
}
