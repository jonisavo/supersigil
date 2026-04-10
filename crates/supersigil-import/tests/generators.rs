//! Proptest generators for import-related types.

#![allow(
    dead_code,
    reason = "generator functions are used by other test files via mod"
)]
#![allow(missing_docs, reason = "test-only generators do not need doc comments")]
#![allow(
    clippy::missing_panics_doc,
    reason = "test helper generators, panics are intentional on invalid regex"
)]

use std::fmt::Write;

use proptest::prelude::*;
use supersigil_import::parse::RawRef;
use supersigil_import::parse::design::{DesignBlock, DesignSection, ParsedDesign};
use supersigil_import::parse::requirements::{
    ParsedCriterion, ParsedRequirement, ParsedRequirements,
};
use supersigil_import::parse::tasks::{
    ParsedSubTask, ParsedTask, ParsedTasks, TaskRefs, TaskStatus,
};

// ---------------------------------------------------------------------------
// Primitive generators
// ---------------------------------------------------------------------------

pub fn arb_feature_name() -> BoxedStrategy<String> {
    proptest::string::string_regex(r"[a-z][a-z0-9]{0,9}(-[a-z][a-z0-9]{0,9}){0,3}")
        .unwrap()
        .boxed()
}

pub fn arb_id_prefix() -> BoxedStrategy<Option<String>> {
    prop_oneof![
        Just(None),
        Just(Some("my-project".into())),
        Just(Some("my-project/".into())),
        arb_feature_name().prop_map(Some),
    ]
    .boxed()
}

pub fn arb_requirement_number() -> BoxedStrategy<String> {
    (1..100u32).prop_map(|n| n.to_string()).boxed()
}

pub fn arb_criterion_index() -> BoxedStrategy<String> {
    prop_oneof![
        (1..20u32).prop_map(|n| n.to_string()),
        (1..20u32, prop::sample::select(vec!['a', 'b', 'c'])).prop_map(|(n, c)| format!("{n}{c}")),
    ]
    .boxed()
}

pub fn arb_raw_ref() -> BoxedStrategy<RawRef> {
    (arb_requirement_number(), arb_criterion_index())
        .prop_map(|(requirement_number, criterion_index)| RawRef {
            requirement_number,
            criterion_index,
        })
        .boxed()
}

pub fn arb_raw_ref_list() -> BoxedStrategy<Vec<RawRef>> {
    proptest::collection::vec(arb_raw_ref(), 1..5).boxed()
}

pub fn arb_task_status() -> BoxedStrategy<TaskStatus> {
    prop_oneof![
        Just(TaskStatus::Done),
        Just(TaskStatus::Ready),
        Just(TaskStatus::InProgress),
        Just(TaskStatus::Draft),
    ]
    .boxed()
}

pub fn arb_prose_block() -> BoxedStrategy<String> {
    proptest::string::string_regex("[A-Za-z][A-Za-z ]{9,79}")
        .unwrap()
        .boxed()
}

pub fn arb_code_block() -> BoxedStrategy<String> {
    let langs = prop::sample::select(vec!["rust", "python", "javascript", "toml", "yaml"]);
    (langs, arb_prose_block())
        .prop_map(|(lang, content)| format!("```{lang}\n{content}\n```"))
        .boxed()
}

pub fn arb_mermaid_block() -> BoxedStrategy<String> {
    Just("```mermaid\ngraph TD\n    A --> B\n```".to_string()).boxed()
}

// ---------------------------------------------------------------------------
// Parsed IR generators
// ---------------------------------------------------------------------------

pub fn arb_optional_title() -> BoxedStrategy<Option<String>> {
    prop_oneof![
        Just(None),
        arb_prose_block().prop_map(|s| Some(s.trim().to_string())),
    ]
    .boxed()
}

pub fn arb_parsed_criterion() -> BoxedStrategy<ParsedCriterion> {
    let conditions = prop::sample::select(vec![
        "a user submits a form",
        "the system receives a request",
        "the input is valid",
        "the timeout expires",
        "the connection is established",
    ]);
    let actions = prop::sample::select(vec![
        "return a success response",
        "log the event",
        "update the database",
        "notify the user",
        "reject the request",
    ]);
    (arb_criterion_index(), conditions, actions)
        .prop_map(|(index, cond, act)| ParsedCriterion {
            index,
            text: format!("WHEN {cond} THE System SHALL {act}"),
        })
        .boxed()
}

pub fn arb_parsed_requirement() -> BoxedStrategy<ParsedRequirement> {
    let title_strat = arb_optional_title();
    let story_strat = prop_oneof![
        Just(None),
        arb_prose_block().prop_map(|s| Some(format!(
            "As a developer, I want {}",
            s.trim().to_lowercase()
        ))),
        arb_prose_block()
            .prop_map(|s| Some(format!("As a user, I want {}", s.trim().to_lowercase()))),
        arb_prose_block()
            .prop_map(|s| Some(format!("As an admin, I want {}", s.trim().to_lowercase()))),
    ];
    (
        arb_requirement_number(),
        title_strat,
        story_strat,
        proptest::collection::vec(arb_parsed_criterion(), 1..6),
        proptest::collection::vec(arb_prose_block(), 0..2),
    )
        .prop_map(
            |(number, title, user_story, criteria, extra_prose)| ParsedRequirement {
                number,
                title,
                user_story,
                criteria,
                extra_prose,
            },
        )
        .boxed()
}

pub fn arb_parsed_requirements() -> BoxedStrategy<ParsedRequirements> {
    let title_strat = arb_optional_title();
    let glossary_strat = prop_oneof![
        2 => Just(None),
        1 => arb_prose_block().prop_map(|s| Some(s.trim().to_string())),
    ];
    (
        title_strat,
        arb_prose_block(),
        glossary_strat,
        proptest::collection::vec(arb_parsed_requirement(), 1..5),
    )
        .prop_map(
            |(title, introduction, glossary, requirements)| ParsedRequirements {
                title,
                introduction,
                glossary,
                requirements,
            },
        )
        .boxed()
}

fn arb_task_refs() -> BoxedStrategy<TaskRefs> {
    prop_oneof![
        3 => Just(TaskRefs::None),
        1 => arb_raw_ref_list().prop_map(TaskRefs::Refs),
        1 => arb_prose_block()
            .prop_map(|s| TaskRefs::Comment(s.trim().to_string())),
    ]
    .boxed()
}

fn arb_sub_task(parent_number: String) -> BoxedStrategy<ParsedSubTask> {
    (
        (1..20u32).prop_map(|n| n.to_string()),
        arb_prose_block().prop_map(|s| s.trim().to_string()),
        arb_task_status(),
        proptest::bool::ANY,
        proptest::collection::vec(arb_prose_block(), 0..2),
        arb_task_refs(),
    )
        .prop_map(
            move |(number, title, status, is_optional, description, requirement_refs)| {
                ParsedSubTask {
                    parent_number: parent_number.clone(),
                    number,
                    title,
                    status,
                    is_optional,
                    description,
                    requirement_refs,
                }
            },
        )
        .boxed()
}

pub fn arb_parsed_task() -> BoxedStrategy<ParsedTask> {
    (1..20u32)
        .prop_map(|n| n.to_string())
        .prop_flat_map(|number| {
            let num_clone = number.clone();
            (
                Just(number),
                arb_prose_block().prop_map(|s| s.trim().to_string()),
                arb_task_status(),
                proptest::bool::ANY,
                proptest::collection::vec(arb_prose_block(), 0..3),
                arb_task_refs(),
                proptest::collection::vec(arb_sub_task(num_clone), 0..4),
            )
        })
        .prop_map(
            |(number, title, status, is_optional, description, requirement_refs, sub_tasks)| {
                ParsedTask {
                    number,
                    title,
                    status,
                    is_optional,
                    description,
                    requirement_refs,
                    sub_tasks,
                }
            },
        )
        .boxed()
}

pub fn arb_parsed_tasks() -> BoxedStrategy<ParsedTasks> {
    let title_strat = arb_optional_title();
    (
        title_strat,
        proptest::collection::vec(arb_prose_block(), 0..3),
        proptest::collection::vec(arb_parsed_task(), 1..6),
    )
        .prop_map(|(title, preamble, tasks)| ParsedTasks {
            title,
            preamble,
            tasks,
            postamble: Vec::new(),
        })
        .boxed()
}

// ---------------------------------------------------------------------------
// Design IR generators
// ---------------------------------------------------------------------------

fn arb_design_block() -> BoxedStrategy<DesignBlock> {
    prop_oneof![
        3 => arb_prose_block().prop_map(DesignBlock::Prose),
        1 => arb_code_block().prop_map(|s| {
            // Parse the code block string back into structured form
            let lines: Vec<&str> = s.lines().collect();
            let lang = lines[0].strip_prefix("```").unwrap_or("");
            let lang = if lang.is_empty() { None } else { Some(lang.to_string()) };
            let content = lines[1..lines.len()-1].join("\n");
            DesignBlock::CodeBlock { language: lang, content }
        }),
        1 => Just(DesignBlock::CodeBlock { language: Some("mermaid".to_string()), content: "graph TD\n    A --> B".to_string() }),
    ]
    .boxed()
}

fn arb_design_section() -> BoxedStrategy<DesignSection> {
    (
        arb_prose_block().prop_map(|s| s.trim().to_string()),
        prop::sample::select(vec![2u8, 3, 4]),
        proptest::collection::vec(arb_design_block(), 1..4),
    )
        .prop_map(|(heading, level, content)| DesignSection {
            heading,
            level,
            content,
        })
        .boxed()
}

pub fn arb_parsed_design() -> BoxedStrategy<ParsedDesign> {
    let title_strat = arb_optional_title();
    (
        title_strat,
        proptest::collection::vec(arb_design_section(), 1..5),
    )
        .prop_map(|(title, sections)| ParsedDesign { title, sections })
        .boxed()
}

// ---------------------------------------------------------------------------
// Kiro markdown rendering generators (for round-trip testing)
// ---------------------------------------------------------------------------

fn render_task_refs(refs: &TaskRefs) -> Option<String> {
    match refs {
        TaskRefs::None => None,
        TaskRefs::Comment(c) => Some(format!("_Requirements: {c}_")),
        TaskRefs::Refs(raw_refs) => {
            let formatted: Vec<String> = raw_refs
                .iter()
                .map(std::string::ToString::to_string)
                .collect();
            Some(format!("_Requirements: {}_", formatted.join(", ")))
        }
    }
}

pub fn arb_kiro_requirements_md() -> BoxedStrategy<(ParsedRequirements, String)> {
    arb_parsed_requirements()
        .prop_map(|parsed| {
            let mut md = String::new();

            if let Some(ref title) = parsed.title {
                let _ = writeln!(md, "# Requirements Document: {title}");
            } else {
                let _ = writeln!(md, "# Requirements Document");
            }
            md.push('\n');

            md.push_str(&parsed.introduction);
            md.push('\n');

            if let Some(ref glossary) = parsed.glossary {
                md.push_str("\n## Glossary\n\n");
                md.push_str(glossary);
                md.push('\n');
            }

            for req in &parsed.requirements {
                md.push('\n');
                if let Some(ref title) = req.title {
                    let _ = writeln!(md, "### Requirement {}: {title}", req.number);
                } else {
                    let _ = writeln!(md, "### Requirement {}", req.number);
                }
                md.push('\n');

                if let Some(ref story) = req.user_story {
                    let _ = writeln!(md, "**User Story:** {story}");
                    md.push('\n');
                }

                if !req.criteria.is_empty() {
                    md.push_str("#### Acceptance Criteria\n\n");
                    for criterion in &req.criteria {
                        let _ = writeln!(md, "{}. {}", criterion.index, criterion.text);
                    }
                }

                for prose in &req.extra_prose {
                    md.push('\n');
                    md.push_str(prose);
                    md.push('\n');
                }
            }

            (parsed, md)
        })
        .boxed()
}

pub fn arb_kiro_tasks_md() -> BoxedStrategy<(ParsedTasks, String)> {
    arb_parsed_tasks()
        .prop_map(|parsed| {
            let mut md = String::new();

            if let Some(ref title) = parsed.title {
                let _ = writeln!(md, "# Implementation Plan: {title}");
            } else {
                let _ = writeln!(md, "# Implementation Plan");
            }
            md.push('\n');

            // Track whether we emit a ## Overview heading so we can include it
            // in the expected preamble (the parser now preserves headings).
            let mut expected_preamble = Vec::new();
            if !parsed.preamble.is_empty() {
                md.push_str("## Overview\n\n");
                expected_preamble.push("## Overview".to_string());
                for block in &parsed.preamble {
                    md.push_str(block);
                    md.push('\n');
                    expected_preamble.push(block.clone());
                }
                md.push('\n');
            }

            md.push_str("## Tasks\n\n");
            for task in &parsed.tasks {
                let marker = task.status.marker_char();
                let opt = if task.is_optional { "*" } else { "" };
                let _ = writeln!(md, "- [{marker}]{opt} {}. {}", task.number, task.title);

                for desc in &task.description {
                    for line in desc.lines() {
                        let _ = writeln!(md, "  {line}");
                    }
                }

                if let Some(ref_line) = render_task_refs(&task.requirement_refs) {
                    let _ = writeln!(md, "  {ref_line}");
                }

                for sub in &task.sub_tasks {
                    let sub_marker = sub.status.marker_char();
                    let sub_opt = if sub.is_optional { "*" } else { "" };
                    let _ = writeln!(
                        md,
                        "  - [{sub_marker}]{sub_opt} {}.{} {}",
                        sub.parent_number, sub.number, sub.title
                    );

                    for desc in &sub.description {
                        for line in desc.lines() {
                            let _ = writeln!(md, "    {line}");
                        }
                    }

                    if let Some(ref_line) = render_task_refs(&sub.requirement_refs) {
                        let _ = writeln!(md, "    {ref_line}");
                    }
                }
            }

            let mut expected = parsed;
            expected.preamble = expected_preamble;
            (expected, md)
        })
        .boxed()
}
