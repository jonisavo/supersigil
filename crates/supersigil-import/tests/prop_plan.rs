//! Property-based tests for import planning.

mod common;
mod generators;

use common::{config_for, write_kiro_spec};
use generators::{arb_feature_name, arb_id_prefix, arb_kiro_requirements_md, arb_kiro_tasks_md};
use proptest::prelude::*;
use supersigil_import::plan_kiro_import;
use supersigil_rust::verifies;

/// Count occurrences of the ambiguity marker prefix in a string.
fn count_ambiguity_markers(content: &str) -> usize {
    content.matches("<!-- TODO(supersigil-import):").count()
}

// Feature: kiro-import, Property 18: Ambiguity marker count consistency
//
// For any import result or plan, the reported `ambiguity_count` equals the
// number of `<!-- TODO(supersigil-import):` occurrences across all generated
// spec documents.
//
// Validates: Requirements 13.3, 14.3
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[verifies("kiro-import/req#req-3-5")]
    #[test]
    fn prop_18_ambiguity_marker_count_consistency(
        (_parsed_reqs, req_md) in arb_kiro_requirements_md(),
        (_parsed_tasks, tasks_md) in arb_kiro_tasks_md(),
        feature_name in arb_feature_name(),
        id_prefix in arb_id_prefix(),
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join(".kiro").join("specs");

        write_kiro_spec(
            &specs_dir,
            &feature_name,
            Some(&req_md),
            None, // no design for simplicity — design is pass-through prose
            Some(&tasks_md),
        );

        let mut config = config_for(&specs_dir, &tmp.path().join("out"));
        config.id_prefix = id_prefix.map(|p| p.trim_end_matches('/').to_string());

        let plan = plan_kiro_import(&config).unwrap();

        // Count actual markers across all generated documents
        let actual_marker_count: usize = plan
            .documents
            .iter()
            .map(|doc| count_ambiguity_markers(&doc.content))
            .sum();

        prop_assert_eq!(
            plan.ambiguity_count,
            actual_marker_count,
            "Reported ambiguity_count ({}) != actual marker occurrences ({}) in generated output",
            plan.ambiguity_count,
            actual_marker_count,
        );
    }
}

// Feature: kiro-import, Property 19: Import plan completeness
//
// For any import plan, each planned document has non-empty `output_path` and
// correct `document_id`. Summary reports correct `criteria_converted`,
// `tasks_converted`, `validates_resolved` counts.
//
// Validates: Requirements 14.2, 14.4
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_19_import_plan_completeness(
        (parsed_reqs, req_md) in arb_kiro_requirements_md(),
        (parsed_tasks, tasks_md) in arb_kiro_tasks_md(),
        feature_name in arb_feature_name(),
        id_prefix in arb_id_prefix(),
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join(".kiro").join("specs");

        write_kiro_spec(
            &specs_dir,
            &feature_name,
            Some(&req_md),
            None,
            Some(&tasks_md),
        );

        let clean_prefix = id_prefix.as_ref().map(|p| p.trim_end_matches('/').to_string());
        let mut config = config_for(&specs_dir, &tmp.path().join("out"));
        config.id_prefix = clean_prefix.clone();

        let plan = plan_kiro_import(&config).unwrap();

        // Every planned document must have a non-empty output_path
        for doc in &plan.documents {
            prop_assert!(
                !doc.output_path.as_os_str().is_empty(),
                "PlannedDocument has empty output_path"
            );
        }

        // Every planned document must have a non-empty document_id
        for doc in &plan.documents {
            prop_assert!(
                !doc.document_id.is_empty(),
                "PlannedDocument has empty document_id"
            );
        }

        // Document IDs must follow the construction rule
        let prefix_str = clean_prefix.as_deref();
        for doc in &plan.documents {
            let id = &doc.document_id;
            // Must contain the feature name
            prop_assert!(
                id.contains(&feature_name),
                "Document ID '{}' does not contain feature name '{}'",
                id,
                feature_name,
            );
            // Must contain a type hint segment
            let has_type_hint = id.contains("/req")
                || id.contains("/design")
                || id.contains("/tasks")
                || id.ends_with("/req")
                || id.ends_with("/design")
                || id.ends_with("/tasks");
            prop_assert!(
                has_type_hint,
                "Document ID '{}' missing type hint segment",
                id,
            );
            // If prefix provided and non-empty, ID must start with it
            if let Some(prefix) = prefix_str
                && !prefix.is_empty()
            {
                prop_assert!(
                    id.starts_with(prefix),
                    "Document ID '{}' does not start with prefix '{}'",
                    id,
                    prefix,
                );
            }
        }

        // Summary: criteria_converted should equal total criteria across all requirements
        let expected_criteria: usize = parsed_reqs
            .requirements
            .iter()
            .map(|r| r.criteria.len())
            .sum();
        prop_assert_eq!(
            plan.summary.criteria_converted,
            expected_criteria,
            "criteria_converted mismatch",
        );

        // Summary: tasks_converted should equal total tasks (top-level + sub-tasks)
        let expected_tasks: usize = parsed_tasks
            .tasks
            .iter()
            .map(|t| 1 + t.sub_tasks.len())
            .sum();
        prop_assert_eq!(
            plan.summary.tasks_converted,
            expected_tasks,
            "tasks_converted mismatch",
        );

        // Summary: features_processed should be 1 (we created one feature)
        prop_assert_eq!(
            plan.summary.features_processed,
            1,
            "features_processed should be 1",
        );
    }
}
