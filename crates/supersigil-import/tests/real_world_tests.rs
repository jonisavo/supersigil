// Real-world Kiro spec input tests.
//
// 22.1: Parse existing `.kiro/specs/` directories as integration tests.

mod common;

use common::workspace_root;

use supersigil_import::parse::design::{DesignBlock, parse_design};
use supersigil_import::parse::requirements::parse_requirements;
use supersigil_import::parse::tasks::{TaskRefs, TaskStatus, parse_tasks};

/// Read a real Kiro spec file from the workspace.
fn read_spec_file(feature: &str, filename: &str) -> String {
    let path = workspace_root()
        .join("tests/fixtures/.kiro/specs")
        .join(feature)
        .join(filename);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

mod real_world_requirements {
    use super::*;

    #[test]
    fn parser_and_config_requirements() {
        let content = read_spec_file("parser-and-config", "requirements.md");
        let parsed = parse_requirements(&content);

        // No title suffix
        assert!(
            parsed.title.is_none(),
            "parser-and-config requirements has no title suffix"
        );
        // Has introduction prose
        assert!(
            !parsed.introduction.is_empty(),
            "should have introduction text"
        );
        // May or may not have a glossary — just check it's parseable
        // Has requirement sections
        assert!(
            parsed.requirements.len() >= 3,
            "should have at least 3 requirements, got {}",
            parsed.requirements.len()
        );

        let req = &parsed.requirements[0];
        // First requirement has number and title
        assert!(!req.number.is_empty());
        assert!(req.title.is_some());
        // Has a user story
        assert!(
            req.user_story.is_some(),
            "first requirement should have a user story"
        );
        // Has acceptance criteria
        assert!(
            !req.criteria.is_empty(),
            "first requirement should have criteria"
        );
    }

    #[test]
    fn document_graph_requirements() {
        let content = read_spec_file("document-graph", "requirements.md");
        let parsed = parse_requirements(&content);
        assert!(
            parsed.requirements.len() >= 3,
            "document-graph should have at least 3 requirements, got {}",
            parsed.requirements.len()
        );
        assert!(
            parsed.glossary.is_some(),
            "document-graph should have a glossary"
        );
    }

    #[test]
    fn kiro_import_requirements() {
        let content = read_spec_file("kiro-import", "requirements.md");
        let parsed = parse_requirements(&content);
        assert!(parsed.title.is_none());
        assert!(
            parsed.requirements.len() >= 15,
            "kiro-import should have many requirements, got {}",
            parsed.requirements.len()
        );
    }
}

mod real_world_design {
    use super::*;

    #[test]
    fn parser_and_config_design() {
        let content = read_spec_file("parser-and-config", "design.md");
        let parsed = parse_design(&content);
        assert!(!parsed.sections.is_empty(), "design should have sections");
        let has_code = parsed.sections.iter().any(|s| {
            s.content
                .iter()
                .any(|b| matches!(b, DesignBlock::CodeBlock { .. }))
        });
        assert!(has_code, "design should contain code blocks");
    }

    #[test]
    fn document_graph_design() {
        let content = read_spec_file("document-graph", "design.md");
        let parsed = parse_design(&content);
        let has_mermaid = parsed.sections.iter().any(|s| {
            s.content.iter().any(
                |b| matches!(b, DesignBlock::CodeBlock { language: Some(l), .. } if l == "mermaid"),
            )
        });
        assert!(
            has_mermaid,
            "document-graph design should contain mermaid diagrams"
        );
        let has_validates = parsed.sections.iter().any(|s| {
            s.content
                .iter()
                .any(|b| matches!(b, DesignBlock::ValidatesLine { .. }))
        });
        assert!(
            has_validates,
            "document-graph design should contain Validates lines"
        );
    }

    #[test]
    fn kiro_import_design_extracts_title() {
        let content = read_spec_file("kiro-import", "design.md");
        let parsed = parse_design(&content);
        assert_eq!(parsed.title.as_deref(), Some("Kiro Import"));
    }
}

mod real_world_tasks {
    use super::*;

    #[test]
    fn parser_and_config_tasks() {
        let content = read_spec_file("parser-and-config", "tasks.md");
        let parsed = parse_tasks(&content);

        assert!(!parsed.tasks.is_empty(), "should have at least one task");
        assert_eq!(
            parsed.title.as_deref(),
            Some("Parser and Config"),
            "tasks title should match"
        );
        let has_subs = parsed.tasks.iter().any(|t| !t.sub_tasks.is_empty());
        assert!(has_subs, "some tasks should have sub-tasks");
        // First task should be done (marked [x])
        assert_eq!(parsed.tasks[0].status, TaskStatus::Done);
        // In real Kiro specs, metadata lines use `  - _Requirements: ..._` format.
        // Tasks with ref-like metadata should have TaskRefs::Refs.
        let has_refs = parsed.tasks.iter().any(|t| {
            matches!(&t.requirement_refs, TaskRefs::Refs(_))
                || t.sub_tasks
                    .iter()
                    .any(|s| matches!(&s.requirement_refs, TaskRefs::Refs(_)))
        });
        assert!(
            has_refs,
            "at least one task or sub-task should have parsed requirement refs from dash-prefixed metadata"
        );
        // Task 1 has `  - _Requirements: N/A (project setup)_`
        let task1 = &parsed.tasks[0];
        assert!(
            matches!(&task1.requirement_refs, TaskRefs::None),
            "task 1 should have TaskRefs::None for N/A, got {:?}",
            task1.requirement_refs
        );
    }

    #[test]
    fn document_graph_tasks() {
        let content = read_spec_file("document-graph", "tasks.md");
        let parsed = parse_tasks(&content);
        assert!(
            parsed.tasks.len() >= 3,
            "document-graph should have at least 3 tasks, got {}",
            parsed.tasks.len()
        );
    }

    #[test]
    fn kiro_import_tasks() {
        let content = read_spec_file("kiro-import", "tasks.md");
        let parsed = parse_tasks(&content);
        assert!(
            parsed.tasks.len() >= 15,
            "kiro-import should have many tasks, got {}",
            parsed.tasks.len()
        );
    }
}
