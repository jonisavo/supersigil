//! Property-based tests for ID generation and deduplication.

mod generators;

use proptest::prelude::*;
use supersigil_import::ids::{deduplicate_ids, make_criterion_id, make_document_id, make_task_id};

// Feature: kiro-import, Property 1: Document ID construction
// Validates: Requirements 16.1, 16.2, 16.3
proptest! {
    #[test]
    fn prop_document_id_construction(
        prefix in generators::arb_id_prefix(),
        feature_name in generators::arb_feature_name(),
        type_hint in prop::sample::select(vec!["req", "design", "tasks"]),
    ) {
        let id = make_document_id(prefix.as_deref(), &feature_name, type_hint);

        if let Some(p) = &prefix {
            let stripped = p.trim_end_matches('/');
            let expected = format!("{stripped}/{feature_name}/{type_hint}");
            prop_assert_eq!(&id, &expected,
                "With prefix {:?}, expected {:?} but got {:?}", p, expected, id);
        } else {
            let expected = format!("{feature_name}/{type_hint}");
            prop_assert_eq!(&id, &expected,
                "Without prefix, expected {:?} but got {:?}", expected, id);
        }

        // ID must never contain double slashes
        prop_assert!(!id.contains("//"), "ID contains double slashes: {id}");
        // ID must never start with a slash
        prop_assert!(!id.starts_with('/'), "ID starts with slash: {id}");
        // ID must never end with a slash
        prop_assert!(!id.ends_with('/'), "ID ends with slash: {id}");
    }
}

// Feature: kiro-import, Property 2: Criterion ID uniqueness after deduplication
// Validates: Requirements 3.1, 3.2, 3.3
proptest! {
    #[test]
    fn prop_criterion_id_uniqueness_after_dedup(
        ids in proptest::collection::vec(
            (generators::arb_requirement_number(), generators::arb_criterion_index())
                .prop_map(|(r, c)| make_criterion_id(&r, &c)),
            2..10,
        ),
    ) {
        let (deduped, markers) = deduplicate_ids(&ids);

        // All deduplicated IDs must be unique
        let mut seen = std::collections::HashSet::new();
        for id in &deduped {
            prop_assert!(seen.insert(id.clone()),
                "Duplicate ID after deduplication: {id}");
        }

        // Output length must equal input length
        prop_assert_eq!(deduped.len(), ids.len());

        // Count collisions in the original list
        let mut orig_counts = std::collections::HashMap::new();
        for id in &ids {
            *orig_counts.entry(id.clone()).or_insert(0usize) += 1;
        }
        let collision_count: usize = orig_counts.values().filter(|&&c| c > 1).map(|c| c - 1).sum();

        // Number of ambiguity markers must equal number of collisions
        prop_assert_eq!(markers.len(), collision_count,
            "Expected {} markers for collisions, got {}", collision_count, markers.len());
    }
}

// Feature: kiro-import, Property 3: Task ID uniqueness after deduplication
// Validates: Requirements 9.1, 9.2, 9.3, 9.4
proptest! {
    #[test]
    fn prop_task_id_uniqueness_after_dedup(
        ids in proptest::collection::vec(
            prop_oneof![
                generators::arb_requirement_number()
                    .prop_map(|n| make_task_id(&n, None)),
                (generators::arb_requirement_number(), generators::arb_requirement_number())
                    .prop_map(|(n, m)| make_task_id(&n, Some(&m))),
            ],
            2..10,
        ),
    ) {
        let (deduped, markers) = deduplicate_ids(&ids);

        // All deduplicated IDs must be unique
        let mut seen = std::collections::HashSet::new();
        for id in &deduped {
            prop_assert!(seen.insert(id.clone()),
                "Duplicate task ID after deduplication: {id}");
        }

        // Output length must equal input length
        prop_assert_eq!(deduped.len(), ids.len());

        // Markers count matches collision count
        let mut orig_counts = std::collections::HashMap::new();
        for id in &ids {
            *orig_counts.entry(id.clone()).or_insert(0usize) += 1;
        }
        let collision_count: usize = orig_counts.values().filter(|&&c| c > 1).map(|c| c - 1).sum();
        prop_assert_eq!(markers.len(), collision_count);
    }
}
