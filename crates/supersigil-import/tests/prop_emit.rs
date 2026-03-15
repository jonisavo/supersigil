mod generators;

use generators::{
    arb_code_block, arb_mermaid_block, arb_parsed_design, arb_parsed_requirements, arb_parsed_tasks,
};
use proptest::prelude::*;
use std::collections::HashMap;
use supersigil_import::emit::design::emit_design_mdx;
use supersigil_import::emit::requirements::emit_requirements_mdx;
use supersigil_import::emit::tasks::emit_tasks_mdx;
use supersigil_import::ids::{deduplicate_ids, make_criterion_id, make_task_id};
use supersigil_rust::verifies;

// Feature: kiro-import, Property 12: Prose and code block round-trip fidelity
//
// For any Kiro spec file containing prose paragraphs, fenced code blocks, and
// mermaid blocks, the MDX output contains each element verbatim as a substring.
//
// Validates: Requirements 19.1, 19.2, 19.3, 4.2, 4.5, 5.2, 5.3, 7.3, 12.4, 12.5
proptest! {
    #[test]
    fn prop_12_prose_round_trip(
        parsed in arb_parsed_requirements(),
    ) {
        let doc_id = "req/test-feature";
        let title = parsed.title.as_deref().unwrap_or("Test Feature");
        let (mdx, _) = emit_requirements_mdx(&parsed, doc_id, title);

        // Introduction prose must appear verbatim
        if !parsed.introduction.trim().is_empty() {
            prop_assert!(
                mdx.contains(&parsed.introduction),
                "MDX missing introduction prose: {:?}",
                parsed.introduction
            );
        }

        // Glossary prose must appear verbatim when present
        if let Some(glossary) = parsed.glossary.as_ref().filter(|g| !g.trim().is_empty()) {
            prop_assert!(
                mdx.contains(glossary.as_str()),
                "MDX missing glossary prose: {:?}",
                glossary
            );
        }

        // Each criterion text must appear verbatim
        for req in &parsed.requirements {
            for criterion in &req.criteria {
                prop_assert!(
                    mdx.contains(&criterion.text),
                    "MDX missing criterion text: {:?}",
                    criterion.text
                );
            }

            // User story must appear verbatim
            if let Some(ref story) = req.user_story {
                prop_assert!(
                    mdx.contains(story),
                    "MDX missing user story: {:?}",
                    story
                );
            }
        }
    }

    #[test]
    fn prop_12_code_block_round_trip(
        code_block in arb_code_block(),
    ) {
        // Embed a code block in the introduction and verify it survives emission
        let parsed = supersigil_import::parse::requirements::ParsedRequirements {
            title: Some("Code Test".to_string()),
            introduction: format!("Some intro.\n\n{code_block}\n\nMore text."),
            glossary: None,
            requirements: vec![],
        };
        let (mdx, _) = emit_requirements_mdx(&parsed, "req/test", "Code Test");
        prop_assert!(
            mdx.contains(&code_block),
            "MDX missing code block: {:?}",
            code_block
        );
    }

    #[test]
    fn prop_12_mermaid_block_round_trip(
        mermaid in arb_mermaid_block(),
    ) {
        let parsed = supersigil_import::parse::requirements::ParsedRequirements {
            title: Some("Mermaid Test".to_string()),
            introduction: format!("Intro.\n\n{mermaid}\n\nEnd."),
            glossary: None,
            requirements: vec![],
        };
        let (mdx, _) = emit_requirements_mdx(&parsed, "req/test", "Mermaid Test");
        prop_assert!(
            mdx.contains(&mermaid),
            "MDX missing mermaid block: {:?}",
            mermaid
        );
    }
}

// Feature: kiro-import, Property 13: Front matter round-trip
//
// For all generated MDX documents, parsing the front matter YAML produces
// correct `id`, `type`, `status` (always `draft`), and `title`.
// Front matter delimited by `---` lines.
//
// Validates: Requirements 4.1, 7.1, 12.1, 21.1, 21.2, 21.3
proptest! {
    #[verifies("kiro-import/req#req-2-3")]
    #[test]
    fn prop_13_front_matter_round_trip(
        parsed in arb_parsed_requirements(),
    ) {
        let doc_id = "req/test-feature";
        let title = parsed.title.as_deref().unwrap_or("Test Feature");
        let (mdx, _) = emit_requirements_mdx(&parsed, doc_id, title);

        // Front matter must start with ---
        prop_assert!(
            mdx.starts_with("---\n"),
            "MDX must start with front matter delimiter"
        );

        // Find the closing --- delimiter
        let rest = &mdx[4..]; // skip opening "---\n"
        let close_idx = rest.find("\n---\n").expect("closing front matter delimiter");
        let front_matter = &rest[..close_idx];

        // Verify supersigil.id
        prop_assert!(
            front_matter.contains(&format!("id: {doc_id}")),
            "Front matter missing id: {doc_id}"
        );

        // Verify supersigil.type
        prop_assert!(
            front_matter.contains("type: requirement"),
            "Front matter missing type: requirement"
        );

        // Verify supersigil.status
        prop_assert!(
            front_matter.contains("status: draft"),
            "Front matter missing status: draft"
        );

        // Verify title field
        let expected_title = title;
        prop_assert!(
            front_matter.contains(&format!("title: \"{expected_title}\"")),
            "Front matter missing title: \"{}\"",
            expected_title
        );
    }
}

// Feature: kiro-import, Property 14: AcceptanceCriteria structure
//
// For any parsed requirements with N requirement sections, emitted MDX
// contains exactly N `<AcceptanceCriteria>` blocks, each with correct
// `<Criterion>` components.
//
// Validates: Requirements 4.3, 4.4
proptest! {
    #[verifies("kiro-import/req#req-3-1")]
    #[test]
    fn prop_14_acceptance_criteria_structure(
        parsed in arb_parsed_requirements(),
    ) {
        let doc_id = "req/test-feature";
        let title = parsed.title.as_deref().unwrap_or("Test Feature");
        let (mdx, _) = emit_requirements_mdx(&parsed, doc_id, title);

        // Count requirements that have criteria (only those get AcceptanceCriteria blocks)
        let reqs_with_criteria = parsed.requirements.iter()
            .filter(|r| !r.criteria.is_empty())
            .count();

        // Count <AcceptanceCriteria> blocks in output
        let ac_count = mdx.matches("<AcceptanceCriteria>").count();
        prop_assert_eq!(
            ac_count, reqs_with_criteria,
            "Expected {} <AcceptanceCriteria> blocks, found {}",
            reqs_with_criteria, ac_count
        );

        // Each criterion should have a <Criterion> with the correct id
        for req in &parsed.requirements {
            for criterion in &req.criteria {
                let crit_id = make_criterion_id(&req.number, &criterion.index);
                let expected_tag = format!("<Criterion id=\"{crit_id}\">");
                prop_assert!(
                    mdx.contains(&expected_tag),
                    "MDX missing Criterion tag: {}",
                    expected_tag
                );
            }
        }

        // Count closing tags match
        let close_ac_count = mdx.matches("</AcceptanceCriteria>").count();
        prop_assert_eq!(
            ac_count, close_ac_count,
            "Mismatched AcceptanceCriteria open/close tags"
        );

        let criterion_open = mdx.matches("<Criterion id=").count();
        let criterion_close = mdx.matches("</Criterion>").count();
        prop_assert_eq!(
            criterion_open, criterion_close,
            "Mismatched Criterion open/close tags"
        );

        // Total criteria count should match
        let total_criteria: usize = parsed.requirements.iter()
            .map(|r| r.criteria.len())
            .sum();
        prop_assert_eq!(
            criterion_open, total_criteria,
            "Expected {} Criterion components, found {}",
            total_criteria, criterion_open
        );
    }
}

// Feature: kiro-import, Property 15: Design Implements emission
//
// When both requirements and design exist, design MDX contains
// `<Implements refs="{req_doc_id}" />`. When only design exists, MDX contains
// ambiguity marker and no `<Implements>`.
//
// Validates: Requirements 7.2
proptest! {
    #[test]
    fn prop_15_design_implements_with_requirements(
        parsed in arb_parsed_design(),
    ) {
        let doc_id = "design/test-feature";
        let req_doc_id = "req/test-feature";
        let title = parsed.title.as_deref().unwrap_or("Test Feature");
        let resolved = HashMap::new();
        let markers: Vec<String> = vec![];

        let (mdx, _) = emit_design_mdx(
            &parsed,
            doc_id,
            Some(req_doc_id),
            &resolved,
            title,
            &markers,
        );

        // Must contain <Implements> with the req doc id
        let expected = format!("<Implements refs=\"{req_doc_id}\" />");
        prop_assert!(
            mdx.contains(&expected),
            "Design MDX missing Implements component: {expected}"
        );

        // Must NOT contain an ambiguity marker about missing requirements
        prop_assert!(
            !mdx.contains("<!-- TODO(supersigil-import): No requirements document"),
            "Design MDX should not have missing-requirements marker when req_doc_id is present"
        );
    }

    #[test]
    fn prop_15_design_implements_without_requirements(
        parsed in arb_parsed_design(),
    ) {
        let doc_id = "design/test-feature";
        let title = parsed.title.as_deref().unwrap_or("Test Feature");
        let resolved = HashMap::new();
        let markers: Vec<String> = vec![];

        let (mdx, ambiguity_count) = emit_design_mdx(
            &parsed,
            doc_id,
            None,
            &resolved,
            title,
            &markers,
        );

        // Must NOT contain <Implements> component (the tag, not just the word in a comment)
        prop_assert!(
            !mdx.contains("<Implements refs="),
            "Design MDX should not have Implements component when no requirements exist"
        );

        // Must contain an ambiguity marker about missing requirements
        prop_assert!(
            mdx.contains("<!-- TODO(supersigil-import):"),
            "Design MDX should have ambiguity marker when no requirements exist"
        );

        // Ambiguity count must be at least 1 (for the missing requirements marker)
        prop_assert!(
            ambiguity_count >= 1,
            "Ambiguity count should be >= 1 when no requirements exist, got {ambiguity_count}"
        );
    }
}

/// Build the deduplicated task ID list matching the emitter's order.
///
/// Returns a flat list of deduped IDs: [top1, sub1a, sub1b, top2, sub2a, ...].
fn build_deduped_task_ids(parsed: &supersigil_import::parse::tasks::ParsedTasks) -> Vec<String> {
    let mut raw_ids = Vec::new();
    for task in &parsed.tasks {
        raw_ids.push(make_task_id(&task.number, None));
        for sub in &task.sub_tasks {
            raw_ids.push(make_task_id(&task.number, Some(&sub.number)));
        }
    }
    let (deduped, _) = deduplicate_ids(&raw_ids);
    deduped
}

/// Find the next `<Task ...>` opening tag in `mdx` starting from `offset`.
/// Returns `(tag_start, tag_str)` where `tag_str` is the full opening tag.
fn find_task_tag(mdx: &str, offset: usize) -> Option<(usize, String)> {
    let rest = &mdx[offset..];
    let rel_start = rest.find("<Task ")?;
    let abs_start = offset + rel_start;
    let tag_end = mdx[abs_start..].find('>')?;
    let tag_str = mdx[abs_start..=abs_start + tag_end].to_string();
    Some((abs_start, tag_str))
}

// Feature: kiro-import, Property 16: Task dependency chain
//
// For any sequence of top-level tasks, each after the first has
// `depends="{previous_task_id}"`. Same for sub-tasks within a parent.
// First in any sibling group has no `depends`.
//
// Validates: Requirements 10.1, 10.2, 10.3
proptest! {
    #[test]
    fn prop_16_task_dependency_chain(
        parsed in arb_parsed_tasks(),
    ) {
        let doc_id = "tasks/test-feature";
        let title = parsed.title.as_deref().unwrap_or("Test Feature");
        let resolved: HashMap<String, Vec<String>> = HashMap::new();
        let markers: Vec<String> = vec![];

        let (mdx, _) = emit_tasks_mdx(&parsed, doc_id, &resolved, title, &markers);

        let deduped = build_deduped_task_ids(&parsed);

        // Walk through the MDX sequentially, verifying dependency chain order.
        // Uses deduped IDs to account for collision disambiguation.
        let mut scan_pos = 0;
        let mut id_cursor = 0;
        let mut prev_top_id: Option<&str> = None;

        for task in &parsed.tasks {
            let task_id = &deduped[id_cursor];
            id_cursor += 1;

            // Find the next top-level <Task> tag from current position
            let (tag_start, tag_str) = find_task_tag(&mdx, scan_pos)
                .expect("should find Task tag");
            prop_assert!(
                tag_str.contains(&format!("id=\"{task_id}\"")),
                "Expected task {task_id}, found tag: {tag_str}"
            );

            if let Some(prev) = prev_top_id {
                prop_assert!(
                    tag_str.contains(&format!("depends=\"{prev}\"")),
                    "Task {task_id} should depend on {prev}, tag: {tag_str}"
                );
            } else {
                prop_assert!(
                    !tag_str.contains("depends="),
                    "First top-level task {task_id} should not have depends, tag: {tag_str}"
                );
            }

            // Move past this tag
            scan_pos = tag_start + tag_str.len();

            // Check sub-task dependency chain within this parent
            let mut prev_sub_id: Option<&str> = None;
            for _sub in &task.sub_tasks {
                let sub_id = &deduped[id_cursor];
                id_cursor += 1;

                let (sub_start, sub_tag) = find_task_tag(&mdx, scan_pos)
                    .expect("should find sub-Task tag");
                prop_assert!(
                    sub_tag.contains(&format!("id=\"{sub_id}\"")),
                    "Expected sub-task {sub_id}, found tag: {sub_tag}"
                );

                if let Some(prev_s) = prev_sub_id {
                    prop_assert!(
                        sub_tag.contains(&format!("depends=\"{prev_s}\"")),
                        "Sub-task {sub_id} should depend on {prev_s}, tag: {sub_tag}"
                    );
                } else {
                    prop_assert!(
                        !sub_tag.contains("depends="),
                        "First sub-task {sub_id} should not have depends, tag: {sub_tag}"
                    );
                }

                scan_pos = sub_start + sub_tag.len();
                prev_sub_id = Some(sub_id);
            }

            prev_top_id = Some(task_id);
        }
    }
}

// Feature: kiro-import, Property 17: Task component structure
//
// For any parsed tasks, emitted MDX contains `<Task>` components with `id`,
// `status`, optional `depends` and `implements`. Sub-tasks are nested.
// Description text appears as body.
//
// Validates: Requirements 12.2, 12.3
proptest! {
    #[verifies("kiro-import/req#req-2-3")]
    #[test]
    fn prop_17_task_component_structure(
        parsed in arb_parsed_tasks(),
    ) {
        let doc_id = "tasks/test-feature";
        let title = parsed.title.as_deref().unwrap_or("Test Feature");
        let resolved: HashMap<String, Vec<String>> = HashMap::new();
        let markers: Vec<String> = vec![];

        let (mdx, _) = emit_tasks_mdx(&parsed, doc_id, &resolved, title, &markers);

        let deduped = build_deduped_task_ids(&parsed);

        // Front matter checks
        prop_assert!(mdx.starts_with("---\n"), "MDX must start with front matter");
        prop_assert!(mdx.contains("type: tasks"), "Front matter must have type: tasks");
        prop_assert!(mdx.contains(&format!("id: {doc_id}")), "Front matter must have correct id");
        prop_assert!(mdx.contains("status: draft"), "Front matter must have status: draft");

        // Preamble prose should appear in output
        for preamble_line in &parsed.preamble {
            if !preamble_line.trim().is_empty() {
                prop_assert!(
                    mdx.contains(preamble_line.trim()),
                    "MDX missing preamble prose: {:?}",
                    preamble_line
                );
            }
        }

        // Walk sequentially to verify each task component using deduped IDs
        let mut scan_pos = 0;
        let mut id_cursor = 0;
        for task in &parsed.tasks {
            let task_id = &deduped[id_cursor];
            id_cursor += 1;
            let status_str = task.status.as_str();

            // Find the next <Task> tag
            let (tag_start, tag_str) = find_task_tag(&mdx, scan_pos)
                .expect("should find Task tag");
            prop_assert!(
                tag_str.contains(&format!("id=\"{task_id}\"")),
                "Expected task {task_id}, found tag: {tag_str}"
            );
            prop_assert!(
                tag_str.contains(&format!("status=\"{status_str}\"")),
                "Task {task_id} missing status=\"{status_str}\", tag: {tag_str}"
            );

            // Title text must appear after the tag
            let after_tag = &mdx[tag_start + tag_str.len()..];
            prop_assert!(
                after_tag.contains(&task.title),
                "MDX missing task title after tag: {:?}",
                task.title
            );

            // Description text must appear
            for desc in &task.description {
                if !desc.trim().is_empty() {
                    prop_assert!(
                        after_tag.contains(desc.trim()),
                        "MDX missing task description: {:?}",
                        desc
                    );
                }
            }

            scan_pos = tag_start + tag_str.len();

            // Each sub-task must be a nested <Task>
            for sub in &task.sub_tasks {
                let sub_id = &deduped[id_cursor];
                id_cursor += 1;
                let sub_status = sub.status.as_str();

                let (sub_start, sub_tag) = find_task_tag(&mdx, scan_pos)
                    .expect("should find sub-Task tag");
                prop_assert!(
                    sub_tag.contains(&format!("id=\"{sub_id}\"")),
                    "Expected sub-task {sub_id}, found tag: {sub_tag}"
                );
                prop_assert!(
                    sub_tag.contains(&format!("status=\"{sub_status}\"")),
                    "Sub-task {sub_id} missing status=\"{sub_status}\", tag: {sub_tag}"
                );

                // Sub-task title must appear after its tag
                let after_sub = &mdx[sub_start + sub_tag.len()..];
                prop_assert!(
                    after_sub.contains(&sub.title),
                    "MDX missing sub-task title: {:?}",
                    sub.title
                );

                // Sub-task must appear after parent open tag
                prop_assert!(
                    sub_start > tag_start,
                    "Sub-task {sub_id} should appear after parent {task_id} opening tag"
                );

                scan_pos = sub_start + sub_tag.len();
            }
        }

        // Count open/close Task tags should match
        let open_count = mdx.matches("<Task ").count();
        let close_count = mdx.matches("</Task>").count();
        prop_assert_eq!(
            open_count, close_count,
            "Mismatched Task open/close tags: {} vs {}",
            open_count, close_count
        );

        // Total task count: top-level + all sub-tasks
        let expected_count: usize = parsed.tasks.len()
            + parsed.tasks.iter().map(|t| t.sub_tasks.len()).sum::<usize>();
        prop_assert_eq!(
            open_count, expected_count,
            "Expected {} Task components, found {}",
            expected_count, open_count
        );
    }
}
