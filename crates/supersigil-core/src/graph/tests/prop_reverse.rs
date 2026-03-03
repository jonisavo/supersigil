//! Property tests for reverse mappings (pipeline stage 8).
//!
//! Properties 16 and 23 from the design document.

use proptest::prelude::*;

use crate::graph::tests::generators::{
    arb_component_id, arb_id, make_acceptance_criteria, make_criterion, make_doc_with_path,
    make_refs_component, single_project_config,
};
use crate::graph::{ILLUSTRATES, IMPLEMENTS, VALIDATES, build_graph};

// ---------------------------------------------------------------------------
// Property 16: Reverse mapping completeness
// ---------------------------------------------------------------------------

proptest! {
    /// For any document containing a `Validates` component with a resolved
    /// ref targeting a criterion, the `validates` reverse mapping should
    /// contain the source document ID in the set for that target.
    ///
    /// Validates: Requirements 8.1
    #[test]
    fn prop_validates_reverse_mapping(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        crit_id in arb_component_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![make_acceptance_criteria(vec![make_criterion(&crit_id, 2)], 1)],
        );

        let ref_str = format!("{target_doc_id}#{crit_id}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(VALIDATES, &ref_str, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed");

        // The validates reverse mapping for (target_doc_id, Some(crit_id))
        // should contain source_doc_id.
        let validators = graph.validates(&target_doc_id, Some(&crit_id));
        prop_assert!(
            validators.contains(&source_doc_id),
            "validates reverse should contain source doc: validators={validators:?}"
        );
    }

    /// For any document containing an `Implements` component with a resolved
    /// ref, the `implements` reverse mapping should contain the source
    /// document ID for the target document. Fragment portions are discarded
    /// — the mapping is document-level only.
    ///
    /// Validates: Requirements 8.2
    #[test]
    fn prop_implements_reverse_mapping_discards_fragments(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![],
        );

        // Implements with a doc-only ref.
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(IMPLEMENTS, &target_doc_id, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed");

        let implementors = graph.implements(&target_doc_id);
        prop_assert!(
            implementors.contains(&source_doc_id),
            "implements reverse should contain source doc: implementors={implementors:?}"
        );
    }

    /// For any document containing an `Illustrates` component with a resolved
    /// ref, the `illustrates` reverse mapping should contain the source
    /// document ID for the target.
    ///
    /// Validates: Requirements 8.3
    #[test]
    fn prop_illustrates_reverse_mapping(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![],
        );

        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(ILLUSTRATES, &target_doc_id, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed");

        let illustrators = graph.illustrates(&target_doc_id, None);
        prop_assert!(
            illustrators.contains(&source_doc_id),
            "illustrates reverse should contain source doc: illustrators={illustrators:?}"
        );
    }

    /// Duplicate refs within the same attribute contribute only once to the
    /// reverse mapping.
    ///
    /// Validates: Requirements 8.6
    #[test]
    fn prop_duplicate_refs_deduplicated(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![],
        );

        // Implements with the same ref listed twice.
        let dup_refs = format!("{target_doc_id}, {target_doc_id}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(IMPLEMENTS, &dup_refs, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed");

        let implementors = graph.implements(&target_doc_id);
        // The source doc should appear exactly once (BTreeSet deduplicates).
        prop_assert!(implementors.contains(&source_doc_id));
        prop_assert_eq!(implementors.len(), 1, "duplicate refs should contribute only once");
    }

    /// Validates with a doc-only ref (no fragment) is stored with
    /// `fragment: None` in the reverse mapping.
    ///
    /// Validates: Requirements 8.1 (fragmentless Validates)
    #[test]
    fn prop_validates_doc_level_ref(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![],
        );

        // Validates with doc-only ref (no fragment). Note: Validates has
        // target_component = "Criterion", but doc-only refs skip fragment
        // checking, so this resolves successfully.
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(VALIDATES, &target_doc_id, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed with doc-only Validates ref");

        let validators = graph.validates(&target_doc_id, None);
        prop_assert!(
            validators.contains(&source_doc_id),
            "validates reverse (fragment=None) should contain source doc"
        );
    }

    /// Illustrates with a fragment ref stores the fragment in the reverse mapping.
    ///
    /// Validates: Requirements 8.3
    #[test]
    fn prop_illustrates_with_fragment(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        crit_id in arb_component_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![make_acceptance_criteria(vec![make_criterion(&crit_id, 2)], 1)],
        );

        let ref_str = format!("{target_doc_id}#{crit_id}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(ILLUSTRATES, &ref_str, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed");

        let illustrators = graph.illustrates(&target_doc_id, Some(&crit_id));
        prop_assert!(
            illustrators.contains(&source_doc_id),
            "illustrates reverse (with fragment) should contain source doc"
        );
    }
}

// ---------------------------------------------------------------------------
// Property 23: Reverse mapping queryability
// ---------------------------------------------------------------------------

proptest! {
    /// Querying `validates` by `(doc_id, Some(fragment))` returns the correct
    /// set of validating documents. Querying for an unreferenced target
    /// returns an empty set.
    ///
    /// Validates: Requirements 8.4, 8.5
    #[test]
    fn prop_validates_queryable_by_fragment(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        crit_id in arb_component_id(),
        unreferenced_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);
        prop_assume!(unreferenced_id != target_doc_id);
        prop_assume!(unreferenced_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![make_acceptance_criteria(vec![make_criterion(&crit_id, 2)], 1)],
        );

        let ref_str = format!("{target_doc_id}#{crit_id}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(VALIDATES, &ref_str, 1)],
        );

        let unreferenced_doc = make_doc_with_path(
            &unreferenced_id,
            &format!("specs/{unreferenced_id}.mdx"),
            vec![],
        );

        let graph = build_graph(vec![target_doc, source_doc, unreferenced_doc], &config)
            .expect("build_graph should succeed");

        // Query for the referenced target — should find the source.
        let validators = graph.validates(&target_doc_id, Some(&crit_id));
        prop_assert!(validators.contains(&source_doc_id));

        // Query for unreferenced target — should return empty set.
        let empty = graph.validates(&unreferenced_id, None);
        prop_assert!(empty.is_empty(), "unreferenced target should return empty set");
    }

    /// Querying `implements` by target doc ID returns the correct set.
    /// Querying for an unreferenced target returns an empty set.
    ///
    /// Validates: Requirements 8.4, 8.5
    #[test]
    fn prop_implements_queryable(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        unreferenced_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);
        prop_assume!(unreferenced_id != target_doc_id);
        prop_assume!(unreferenced_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![],
        );

        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(IMPLEMENTS, &target_doc_id, 1)],
        );

        let unreferenced_doc = make_doc_with_path(
            &unreferenced_id,
            &format!("specs/{unreferenced_id}.mdx"),
            vec![],
        );

        let graph = build_graph(vec![target_doc, source_doc, unreferenced_doc], &config)
            .expect("build_graph should succeed");

        let implementors = graph.implements(&target_doc_id);
        prop_assert!(implementors.contains(&source_doc_id));

        let empty = graph.implements(&unreferenced_id);
        prop_assert!(empty.is_empty(), "unreferenced target should return empty set");
    }

    /// Querying `illustrates` by `(doc_id, fragment)` and `(doc_id, None)`
    /// returns the correct sets. Unreferenced targets return empty.
    ///
    /// Validates: Requirements 8.4, 8.5
    #[test]
    fn prop_illustrates_queryable(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        crit_id in arb_component_id(),
        unreferenced_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);
        prop_assume!(unreferenced_id != target_doc_id);
        prop_assume!(unreferenced_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![make_acceptance_criteria(vec![make_criterion(&crit_id, 2)], 1)],
        );

        let ref_str = format!("{target_doc_id}#{crit_id}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(ILLUSTRATES, &ref_str, 1)],
        );

        let unreferenced_doc = make_doc_with_path(
            &unreferenced_id,
            &format!("specs/{unreferenced_id}.mdx"),
            vec![],
        );

        let graph = build_graph(vec![target_doc, source_doc, unreferenced_doc], &config)
            .expect("build_graph should succeed");

        // Query with fragment.
        let illustrators = graph.illustrates(&target_doc_id, Some(&crit_id));
        prop_assert!(illustrators.contains(&source_doc_id));

        // Unreferenced target.
        let empty = graph.illustrates(&unreferenced_id, None);
        prop_assert!(empty.is_empty(), "unreferenced target should return empty set");
    }
}
