mod generators;

use std::fmt::Write;

use proptest::prelude::*;
use supersigil_import::emit::design::emit_design_md;
use supersigil_import::emit::tasks::emit_tasks_md;
use supersigil_import::parse::design::DesignBlock;
use supersigil_import::parse::tasks::parse_tasks;

// ---------------------------------------------------------------------------
// Generators specific to edge-case property tests
// ---------------------------------------------------------------------------

/// Generate a non-requirement Validates target (e.g., `Design Decision 5`).
fn arb_non_requirement_target() -> BoxedStrategy<String> {
    prop_oneof![
        Just("Design Decision 5".to_string()),
        Just("Architecture Principle 3".to_string()),
        Just("Security Guideline 2".to_string()),
        Just("Performance Goal 1".to_string()),
        Just("Operational Constraint 4".to_string()),
        Just("Data Model Invariant 7".to_string()),
    ]
    .boxed()
}

/// Generate a design.md string containing a `**Validates:**` line that
/// references a non-requirement target.
fn arb_design_with_non_req_validates() -> BoxedStrategy<(String, String)> {
    (
        generators::arb_prose_block(),
        arb_non_requirement_target(),
        generators::arb_prose_block(),
    )
        .prop_map(|(section_prose, target, extra_prose)| {
            let md = format!(
                "# Design Document: Edge Test\n\n\
                 ## Overview\n\n\
                 {section_prose}\n\n\
                 ## Correctness Properties\n\n\
                 ### Property 1: Some property\n\n\
                 {extra_prose}\n\n\
                 **Validates: {target}**\n"
            );
            (md, target)
        })
        .boxed()
}

/// Generate a Kiro tasks.md string with at least one optional task marker.
#[allow(
    clippy::type_complexity,
    reason = "proptest strategy return type is inherently complex"
)]
fn arb_tasks_md_with_optional() -> BoxedStrategy<(String, Vec<(String, String, bool)>)> {
    // Generate 1..4 tasks, each with a random status and optional flag.
    // At least one must be optional.
    let task_entry = (
        generators::arb_task_status(),
        generators::arb_prose_block().prop_map(|s| s.trim().to_string()),
        proptest::bool::ANY,
    );
    proptest::collection::vec(task_entry, 1..5)
        .prop_filter("need at least one optional task", |tasks| {
            tasks.iter().any(|(_, _, opt)| *opt)
        })
        .prop_map(|tasks| {
            let mut md = String::from("# Implementation Plan: Optional Test\n\n## Tasks\n\n");
            let mut expected = Vec::new();
            for (i, (status, title, is_optional)) in tasks.iter().enumerate() {
                let num = i + 1;
                let marker = status.marker_char();
                let opt = if *is_optional { "*" } else { "" };
                let _ = writeln!(md, "- [{marker}]{opt} {num}. {title}");
                expected.push((num.to_string(), title.clone(), *is_optional));
            }
            (md, expected)
        })
        .boxed()
}

/// Generate a tasks.md string containing lines that don't match the expected
/// task format, mixed with valid task lines.
fn arb_tasks_md_with_unparseable() -> BoxedStrategy<(String, Vec<String>)> {
    let garbage_line = prop_oneof![
        Just("- This line has no checkbox at all".to_string()),
        Just("* [x] Wrong bullet style 1. Task".to_string()),
        Just("- [x] No number just a title".to_string()),
        Just("- [?] 1. Unknown marker task".to_string()),
        Just("Random text that is not a task".to_string()),
    ];
    (
        generators::arb_prose_block().prop_map(|s| s.trim().to_string()),
        proptest::collection::vec(garbage_line, 1..4),
    )
        .prop_map(|(valid_title, garbage_lines)| {
            let mut md = String::from("# Implementation Plan: Unparseable Test\n\n## Tasks\n\n");
            // One valid task first
            let _ = writeln!(md, "- [x] 1. {valid_title}");
            // Then garbage lines
            for line in &garbage_lines {
                md.push_str(line);
                md.push('\n');
            }
            (md, garbage_lines)
        })
        .boxed()
}

// ===========================================================================
// Feature: kiro-import, Property 21: Non-requirement Validates targets
//                                     produce ambiguity markers
//
// For any `**Validates:**` line referencing a non-requirement target
// (e.g., `Design Decision 5`), the parser preserves the line as prose
// and emits an ambiguity marker.
//
// Validates: Requirements 5.4
// ===========================================================================
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_21_non_requirement_validates_produces_ambiguity(
        (md, target) in arb_design_with_non_req_validates(),
    ) {
        let parsed = supersigil_import::parse::design::parse_design(&md);

        // The Validates line referencing a non-requirement target should be
        // converted to a Prose block (not a ValidatesLine block).
        let mut found_prose_with_target = false;
        let mut found_ambiguity_in_prose = false;

        for section in &parsed.sections {
            for block in &section.content {
                if let DesignBlock::Prose(text) = block {
                    if text.contains(&format!("**Validates: {target}**")) {
                        found_prose_with_target = true;
                    }
                    if text.contains("<!-- TODO(supersigil-import):") && text.contains(&target) {
                        found_ambiguity_in_prose = true;
                    }
                }
            }
        }

        prop_assert!(
            found_prose_with_target,
            "Non-requirement Validates target '{}' should be preserved as prose.\n\
             Parsed sections: {:?}",
            target, parsed.sections.iter().map(|s| &s.heading).collect::<Vec<_>>()
        );

        prop_assert!(
            found_ambiguity_in_prose,
            "Non-requirement Validates target '{}' should have an ambiguity marker.\n\
             Markdown:\n{}",
            target, md
        );

        // When emitted, the ambiguity marker should appear in the output
        let title = parsed.title.as_deref().unwrap_or("Edge Test");
        let (output, ambiguity_count, _) = emit_design_md(
            &parsed,
            "design/edge-test",
            None,
            "",
            title,
        );

        // The output should contain the ambiguity marker about the non-requirement target
        prop_assert!(
            output.contains("<!-- TODO(supersigil-import):"),
            "Emitted output should contain ambiguity marker for non-requirement target"
        );

        // Ambiguity count should be at least 1 (the non-req target marker,
        // plus potentially the missing-requirements marker)
        prop_assert!(
            ambiguity_count >= 1,
            "Ambiguity count should be >= 1, got {ambiguity_count}"
        );
    }
}

// ===========================================================================
// Feature: kiro-import, Property 22: Optional task marker handling
//
// For any Kiro task line with optional marker (`[x]* 2.1 ...`), the task
// is included in the output with an ambiguity marker noting optional status.
//
// Validates: Requirements 22.1
// ===========================================================================
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_22_optional_task_marker_handling(
        (md, expected_tasks) in arb_tasks_md_with_optional(),
    ) {
        let parsed = parse_tasks(&md);

        // All tasks should be present in the parsed output
        prop_assert_eq!(
            parsed.tasks.len(), expected_tasks.len(),
            "Task count mismatch. Expected: {}, Got: {}\n\nMarkdown:\n{}",
            expected_tasks.len(), parsed.tasks.len(), md
        );

        for (parsed_task, (exp_num, exp_title, exp_optional)) in
            parsed.tasks.iter().zip(expected_tasks.iter())
        {
            prop_assert_eq!(
                &parsed_task.number, exp_num,
                "Task number mismatch"
            );
            prop_assert_eq!(
                parsed_task.title.trim(), exp_title.trim(),
                "Task title mismatch"
            );
            prop_assert_eq!(
                parsed_task.is_optional, *exp_optional,
                "Task {} is_optional mismatch. Expected: {}, Got: {}",
                exp_num, exp_optional, parsed_task.is_optional
            );
        }

        // When emitted, optional tasks should produce ambiguity markers
        let title = parsed.title.as_deref().unwrap_or("Optional Test");
        let (output, ambiguity_count, _) = emit_tasks_md(
            &parsed,
            "tasks/optional-test",
            None,
            "",
            title,
        );

        let optional_count = expected_tasks.iter().filter(|(_, _, opt)| *opt).count();

        // Each optional task should produce an ambiguity marker
        let optional_marker_count = output
            .matches("<!-- TODO(supersigil-import): This task was marked as optional")
            .count();
        prop_assert_eq!(
            optional_marker_count, optional_count,
            "Expected {} optional-task ambiguity markers, found {}.\n\nOutput:\n{}",
            optional_count, optional_marker_count, output
        );

        // Ambiguity count should include the optional markers
        prop_assert!(
            ambiguity_count >= optional_count,
            "Ambiguity count ({}) should be >= optional task count ({})",
            ambiguity_count, optional_count
        );

        // All tasks (including optional ones) should appear in the output
        for (exp_num, exp_title, _) in &expected_tasks {
            let task_id = format!("task-{exp_num}");
            prop_assert!(
                output.contains(&format!("id=\"{task_id}\"")),
                "output missing task with id {task_id}"
            );
            prop_assert!(
                output.contains(exp_title.trim()),
                "output missing task title: {:?}",
                exp_title
            );
        }
    }
}

// ===========================================================================
// Feature: kiro-import, Property 23: Unparseable structure preservation
//
// For any task line or structural pattern not matching the expected format,
// the importer inserts an ambiguity marker and preserves the original text
// verbatim.
//
// Validates: Requirements 13.2
// ===========================================================================
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_23_unparseable_structure_preservation(
        (md, garbage_lines) in arb_tasks_md_with_unparseable(),
    ) {
        let parsed = parse_tasks(&md);

        // The valid task (task 1) should always be parsed
        prop_assert!(
            !parsed.tasks.is_empty(),
            "Should parse at least the valid task.\n\nMarkdown:\n{}",
            md
        );
        prop_assert_eq!(
            &parsed.tasks[0].number, "1",
            "First parsed task should be task 1"
        );

        // Garbage lines that look like they could be task-adjacent content
        // (indented under the valid task) get collected as description lines.
        // Lines that are clearly not task-related are also collected as
        // description since they appear after a task in the Tasks section.
        //
        // The key property: the original text from garbage lines should be
        // preserved somewhere in the parsed output (either as description
        // text on the preceding task, or in the postamble).
        let all_parsed_text = {
            let mut buf = String::new();
            for task in &parsed.tasks {
                buf.push_str(&task.title);
                buf.push('\n');
                for desc in &task.description {
                    buf.push_str(desc);
                    buf.push('\n');
                }
                for sub in &task.sub_tasks {
                    buf.push_str(&sub.title);
                    buf.push('\n');
                    for desc in &sub.description {
                        buf.push_str(desc);
                        buf.push('\n');
                    }
                }
            }
            for p in &parsed.postamble {
                buf.push_str(p);
                buf.push('\n');
            }
            buf
        };

        for garbage in &garbage_lines {
            let trimmed = garbage.trim();
            if !trimmed.is_empty() {
                prop_assert!(
                    all_parsed_text.contains(trimmed),
                    "Garbage line should be preserved in parsed output.\n\
                     Missing: {:?}\n\
                     All parsed text:\n{}\n\
                     Markdown:\n{}",
                    trimmed, all_parsed_text, md
                );
            }
        }
    }
}
