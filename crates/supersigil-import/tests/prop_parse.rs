//! Property-based tests for spec parsing.

mod generators;

use std::fs;

use proptest::prelude::*;
use supersigil_import::Diagnostic;
use supersigil_import::discover::discover_kiro_specs;
use supersigil_import::parse::requirements::parse_requirements;
use supersigil_import::parse::tasks::{TaskRefs, parse_tasks};

/// Generate a mixed directory layout: a list of `(dir_name, has_requirements, has_design, has_tasks)`.
/// At least one dir is valid (has at least one file) and at least one is empty.
fn arb_spec_dir_layout() -> BoxedStrategy<Vec<(String, bool, bool, bool)>> {
    // Generate 2..6 feature dirs with random file presence
    let dir_entry = (
        generators::arb_feature_name(),
        proptest::bool::ANY,
        proptest::bool::ANY,
        proptest::bool::ANY,
    );
    proptest::collection::vec(dir_entry, 2..6)
        .prop_filter("need at least one valid and one empty dir", |dirs| {
            let has_valid = dirs.iter().any(|(_, r, d, t)| *r || *d || *t);
            let has_empty = dirs.iter().any(|(_, r, d, t)| !*r && !*d && !*t);
            has_valid && has_empty
        })
        .boxed()
}

// Feature: kiro-import, Property 11: Discovery includes valid dirs and skips empty ones
// Validates: Requirements 1.1, 1.2, 1.3, 1.5
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn prop_discovery_finds_valid_dirs_skips_empty(
        layout in arb_spec_dir_layout(),
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join(".kiro").join("specs");
        fs::create_dir_all(&specs_dir).unwrap();

        // Deduplicate feature names to avoid filesystem collisions
        let mut seen_names = std::collections::HashSet::new();
        let mut unique_layout = Vec::new();
        for (name, has_req, has_design, has_tasks) in &layout {
            if seen_names.insert(name.clone()) {
                unique_layout.push((name.clone(), *has_req, *has_design, *has_tasks));
            }
        }

        // Create the directory structure
        for (name, has_req, has_design, has_tasks) in &unique_layout {
            let dir = specs_dir.join(name);
            fs::create_dir_all(&dir).unwrap();
            if *has_req {
                fs::write(dir.join("requirements.md"), "# Requirements").unwrap();
            }
            if *has_design {
                fs::write(dir.join("design.md"), "# Design").unwrap();
            }
            if *has_tasks {
                fs::write(dir.join("tasks.md"), "# Tasks").unwrap();
            }
        }

        let (discovered, diagnostics) = discover_kiro_specs(&specs_dir).unwrap();

        // Compute expected valid and empty dirs
        let expected_valid: Vec<&String> = unique_layout
            .iter()
            .filter(|(_, r, d, t)| *r || *d || *t)
            .map(|(name, _, _, _)| name)
            .collect();

        let expected_empty: Vec<&String> = unique_layout
            .iter()
            .filter(|(_, r, d, t)| !*r && !*d && !*t)
            .map(|(name, _, _, _)| name)
            .collect();

        // Discovered dirs should match valid dirs exactly
        let mut discovered_names: Vec<String> = discovered
            .iter()
            .map(|d| d.feature_name.clone())
            .collect();
        discovered_names.sort();

        let mut expected_names: Vec<String> = expected_valid.iter().map(|s| (*s).clone()).collect();
        expected_names.sort();

        prop_assert_eq!(
            &discovered_names, &expected_names,
            "Discovered features don't match expected valid dirs"
        );

        // Each discovered dir should have correct file presence flags
        for dir in &discovered {
            let entry = unique_layout
                .iter()
                .find(|(name, _, _, _)| name == &dir.feature_name)
                .unwrap();
            prop_assert_eq!(dir.has_requirements, entry.1,
                "has_requirements mismatch for {}", dir.feature_name);
            prop_assert_eq!(dir.has_design, entry.2,
                "has_design mismatch for {}", dir.feature_name);
            prop_assert_eq!(dir.has_tasks, entry.3,
                "has_tasks mismatch for {}", dir.feature_name);
        }

        // Feature name should be derived from directory name (Req 1.5)
        for dir in &discovered {
            let dir_name = dir.path.file_name().unwrap().to_str().unwrap();
            prop_assert_eq!(&dir.feature_name, dir_name,
                "Feature name should match directory name");
        }

        // SkippedDir diagnostics should match empty dirs
        let skipped_names: Vec<String> = diagnostics
            .iter()
            .filter_map(|d| match d {
                Diagnostic::SkippedDir { path, .. } => {
                    path.file_name().map(|n| n.to_str().unwrap().to_string())
                }
                Diagnostic::Warning { .. } => None,
            })
            .collect();

        let mut sorted_skipped = skipped_names.clone();
        sorted_skipped.sort();
        let mut sorted_expected_empty: Vec<String> =
            expected_empty.iter().map(|s| (*s).clone()).collect();
        sorted_expected_empty.sort();

        prop_assert_eq!(
            &sorted_skipped, &sorted_expected_empty,
            "SkippedDir diagnostics don't match expected empty dirs"
        );

        // Discovered dirs should be sorted alphabetically by feature name
        let names_in_order: Vec<&str> = discovered
            .iter()
            .map(|d| d.feature_name.as_str())
            .collect();
        let mut sorted = names_in_order.clone();
        sorted.sort_unstable();
        prop_assert_eq!(
            &names_in_order, &sorted,
            "Discovered dirs should be sorted alphabetically"
        );
    }
}

// Feature: kiro-import, Property 9: Requirements parsing completeness
// Validates: Requirements 2.1, 2.2, 2.3, 2.4, 2.6
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_requirements_parsing_completeness(
        (expected, md) in generators::arb_kiro_requirements_md(),
    ) {
        let parsed = parse_requirements(&md);

        // Req 2.6: Document title extraction
        // The generator emits `# Requirements Document: Title` or `# Requirements Document`
        prop_assert_eq!(
            &parsed.title, &expected.title,
            "Document title mismatch.\nExpected: {:?}\nGot: {:?}\n\nMarkdown:\n{}",
            expected.title, parsed.title, md
        );

        // Req 2.1: Introduction text extracted
        let parsed_intro = parsed.introduction.trim();
        let expected_intro = expected.introduction.trim();
        prop_assert!(
            parsed_intro.contains(expected_intro),
            "Introduction not found in parsed output.\nExpected to contain: {:?}\nGot: {:?}",
            expected_intro, parsed_intro
        );

        // Req 2.1: Glossary extracted when present
        match (&expected.glossary, &parsed.glossary) {
            (Some(expected_gloss), Some(parsed_gloss)) => {
                prop_assert!(
                    parsed_gloss.trim().contains(expected_gloss.trim()),
                    "Glossary content mismatch.\nExpected to contain: {:?}\nGot: {:?}",
                    expected_gloss.trim(), parsed_gloss.trim()
                );
            }
            (None, None) => {} // Both absent — correct
            (Some(g), None) => {
                prop_assert!(false, "Expected glossary {:?} but parsed None", g);
            }
            (None, Some(g)) => {
                prop_assert!(false, "Expected no glossary but parsed {:?}", g);
            }
        }

        // Req 2.4: Correct number of requirement sections extracted
        prop_assert_eq!(
            parsed.requirements.len(),
            expected.requirements.len(),
            "Requirement section count mismatch.\nExpected: {}\nGot: {}\n\nMarkdown:\n{}",
            expected.requirements.len(), parsed.requirements.len(), md
        );

        // Per-requirement checks
        for (i, (exp_req, parsed_req)) in expected
            .requirements
            .iter()
            .zip(parsed.requirements.iter())
            .enumerate()
        {
            // Req 2.4: Requirement number extracted
            prop_assert_eq!(
                &parsed_req.number, &exp_req.number,
                "Requirement {} number mismatch", i
            );

            // Req 2.4: Requirement title extracted
            prop_assert_eq!(
                &parsed_req.title, &exp_req.title,
                "Requirement {} title mismatch", i
            );

            // Req 2.2: User story extracted
            match (&exp_req.user_story, &parsed_req.user_story) {
                (Some(exp_story), Some(parsed_story)) => {
                    prop_assert_eq!(
                        parsed_story.trim(), exp_story.trim(),
                        "Requirement {} user story mismatch", i
                    );
                }
                (None, None) => {}
                (exp, got) => {
                    prop_assert!(
                        false,
                        "Requirement {} user story mismatch: expected {:?}, got {:?}",
                        i, exp, got
                    );
                }
            }

            // Req 2.3: All criteria extracted with correct indices and text
            prop_assert_eq!(
                parsed_req.criteria.len(),
                exp_req.criteria.len(),
                "Requirement {} criteria count mismatch.\nExpected: {}\nGot: {}\n\nMarkdown:\n{}",
                i, exp_req.criteria.len(), parsed_req.criteria.len(), md
            );

            for (j, (exp_crit, parsed_crit)) in exp_req
                .criteria
                .iter()
                .zip(parsed_req.criteria.iter())
                .enumerate()
            {
                prop_assert_eq!(
                    &parsed_crit.index, &exp_crit.index,
                    "Requirement {} criterion {} index mismatch", i, j
                );
                prop_assert_eq!(
                    parsed_crit.text.trim(), exp_crit.text.trim(),
                    "Requirement {} criterion {} text mismatch", i, j
                );
            }
        }
    }
}

// Feature: kiro-import, Property 10: Tasks parsing completeness
// Validates: Requirements 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_tasks_parsing_completeness(
        (expected, md) in generators::arb_kiro_tasks_md(),
    ) {
        let parsed = parse_tasks(&md);

        // Req 8.8: Document title extraction
        prop_assert_eq!(
            &parsed.title, &expected.title,
            "Document title mismatch.\nExpected: {:?}\nGot: {:?}\n\nMarkdown:\n{}",
            expected.title, parsed.title, md
        );

        // Preamble extraction
        prop_assert_eq!(
            parsed.preamble.len(), expected.preamble.len(),
            "Preamble block count mismatch.\nExpected: {}\nGot: {}\n\nMarkdown:\n{}",
            expected.preamble.len(), parsed.preamble.len(), md
        );
        for (i, (exp_block, parsed_block)) in expected
            .preamble
            .iter()
            .zip(parsed.preamble.iter())
            .enumerate()
        {
            prop_assert!(
                parsed_block.trim().contains(exp_block.trim()),
                "Preamble block {} content mismatch.\nExpected to contain: {:?}\nGot: {:?}",
                i, exp_block.trim(), parsed_block.trim()
            );
        }

        // Req 8.1: Correct number of top-level tasks extracted
        prop_assert_eq!(
            parsed.tasks.len(), expected.tasks.len(),
            "Top-level task count mismatch.\nExpected: {}\nGot: {}\n\nMarkdown:\n{}",
            expected.tasks.len(), parsed.tasks.len(), md
        );

        for (i, (exp_task, parsed_task)) in expected
            .tasks
            .iter()
            .zip(parsed.tasks.iter())
            .enumerate()
        {
            // Req 8.3: Task number extracted
            prop_assert_eq!(
                &parsed_task.number, &exp_task.number,
                "Task {} number mismatch", i
            );

            // Req 8.3: Task title extracted
            prop_assert_eq!(
                parsed_task.title.trim(), exp_task.title.trim(),
                "Task {} title mismatch", i
            );

            // Req 8.4: Status marker mapping
            prop_assert_eq!(
                &parsed_task.status, &exp_task.status,
                "Task {} status mismatch.\nExpected: {:?}\nGot: {:?}",
                i, exp_task.status, parsed_task.status
            );

            // Req 22.1: Optional marker
            prop_assert_eq!(
                parsed_task.is_optional, exp_task.is_optional,
                "Task {} is_optional mismatch.\nExpected: {:?}\nGot: {:?}",
                i, exp_task.is_optional, parsed_task.is_optional
            );

            // Req 8.5: Description lines extracted
            let exp_desc = exp_task.description.join("\n");
            let parsed_desc = parsed_task.description.join("\n");
            if !exp_desc.trim().is_empty() {
                prop_assert!(
                    parsed_desc.trim().contains(exp_desc.trim()),
                    "Task {} description mismatch.\nExpected to contain: {:?}\nGot: {:?}",
                    i, exp_desc.trim(), parsed_desc.trim()
                );
            }

            // Req 8.6, 8.7: Requirement refs / metadata
            assert_task_refs_match(&exp_task.requirement_refs, &parsed_task.requirement_refs, i, "top-level")?;

            // Req 8.2: Sub-task count
            prop_assert_eq!(
                parsed_task.sub_tasks.len(), exp_task.sub_tasks.len(),
                "Task {} sub-task count mismatch.\nExpected: {}\nGot: {}\n\nMarkdown:\n{}",
                i, exp_task.sub_tasks.len(), parsed_task.sub_tasks.len(), md
            );

            for (j, (exp_sub, parsed_sub)) in exp_task
                .sub_tasks
                .iter()
                .zip(parsed_task.sub_tasks.iter())
                .enumerate()
            {
                // Req 8.3: Sub-task number and parent
                prop_assert_eq!(
                    &parsed_sub.parent_number, &exp_sub.parent_number,
                    "Task {}.{} parent number mismatch", i, j
                );
                prop_assert_eq!(
                    &parsed_sub.number, &exp_sub.number,
                    "Task {}.{} number mismatch", i, j
                );

                // Req 8.3: Sub-task title
                prop_assert_eq!(
                    parsed_sub.title.trim(), exp_sub.title.trim(),
                    "Task {}.{} title mismatch", i, j
                );

                // Req 8.4: Sub-task status
                prop_assert_eq!(
                    &parsed_sub.status, &exp_sub.status,
                    "Task {}.{} status mismatch.\nExpected: {:?}\nGot: {:?}",
                    i, j, exp_sub.status, parsed_sub.status
                );

                // Req 22.1: Sub-task optional marker
                prop_assert_eq!(
                    parsed_sub.is_optional, exp_sub.is_optional,
                    "Task {}.{} is_optional mismatch.\nExpected: {:?}\nGot: {:?}",
                    i, j, exp_sub.is_optional, parsed_sub.is_optional
                );

                // Req 8.5: Sub-task description
                let exp_sub_desc = exp_sub.description.join("\n");
                let parsed_sub_desc = parsed_sub.description.join("\n");
                if !exp_sub_desc.trim().is_empty() {
                    prop_assert!(
                        parsed_sub_desc.trim().contains(exp_sub_desc.trim()),
                        "Task {}.{} description mismatch.\nExpected to contain: {:?}\nGot: {:?}",
                        i, j, exp_sub_desc.trim(), parsed_sub_desc.trim()
                    );
                }

                // Req 8.6, 8.7: Sub-task requirement refs
                assert_task_refs_match(&exp_sub.requirement_refs, &parsed_sub.requirement_refs, j, &format!("sub-task of task {i}"))?;
            }
        }
    }
}

/// Helper to compare `TaskRefs` variants in property tests.
fn assert_task_refs_match(
    expected: &TaskRefs,
    parsed: &TaskRefs,
    index: usize,
    context: &str,
) -> Result<(), TestCaseError> {
    match (expected, parsed) {
        (TaskRefs::None, TaskRefs::None) => {}
        (TaskRefs::Comment(exp), TaskRefs::Comment(got)) => {
            prop_assert!(
                got.contains(exp.trim()),
                "{}",
                format!(
                    "{context} task {index} TaskRefs::Comment mismatch.\nExpected to contain: {exp:?}\nGot: {got:?}"
                )
            );
        }
        (TaskRefs::Refs(exp_refs), TaskRefs::Refs(parsed_refs)) => {
            prop_assert!(
                parsed_refs.len() == exp_refs.len(),
                "{}",
                format!(
                    "{context} task {index} ref count mismatch. Expected: {} Got: {}",
                    exp_refs.len(),
                    parsed_refs.len()
                )
            );
            for (k, (exp_ref, parsed_ref)) in exp_refs.iter().zip(parsed_refs.iter()).enumerate() {
                prop_assert!(
                    parsed_ref.requirement_number == exp_ref.requirement_number,
                    "{}",
                    format!(
                        "{context} task {index} ref {k} requirement_number mismatch: {:?} vs {:?}",
                        parsed_ref.requirement_number, exp_ref.requirement_number
                    )
                );
                prop_assert!(
                    parsed_ref.criterion_index == exp_ref.criterion_index,
                    "{}",
                    format!(
                        "{context} task {index} ref {k} criterion_index mismatch: {:?} vs {:?}",
                        parsed_ref.criterion_index, exp_ref.criterion_index
                    )
                );
            }
        }
        _ => {
            return Err(TestCaseError::Fail(
                format!(
                    "{context} task {index} TaskRefs variant mismatch. Expected: {:?} Got: {:?}",
                    std::mem::discriminant(expected),
                    std::mem::discriminant(parsed)
                )
                .into(),
            ));
        }
    }
    Ok(())
}
