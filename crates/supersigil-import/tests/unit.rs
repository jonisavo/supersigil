// Unit tests for real-world Kiro spec inputs and edge cases.
//
// 22.1: Parse existing `.kiro/specs/` directories as integration tests.
// 22.2: Edge case tests for boundary conditions and error paths.

mod common;

use common::{config_for, workspace_root, write_kiro_spec};
use std::path::PathBuf;

use supersigil_import::discover::discover_kiro_specs;
use supersigil_import::parse::design::{DesignBlock, parse_design};
use supersigil_import::parse::requirements::parse_requirements;
use supersigil_import::parse::tasks::{TaskRefs, TaskStatus, parse_tasks};
use supersigil_import::write::write_files;
use supersigil_import::{ImportError, PlannedDocument, plan_kiro_import};

/// Read a real Kiro spec file from the workspace.
fn read_spec_file(feature: &str, filename: &str) -> String {
    let path = workspace_root()
        .join(".kiro")
        .join("specs")
        .join(feature)
        .join(filename);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

// ===========================================================================
// 22.1: Real-world Kiro spec input tests
// ===========================================================================

mod real_world_requirements {
    use super::*;

    #[test]
    fn parser_and_config_extracts_title() {
        let content = read_spec_file("parser-and-config", "requirements.md");
        let parsed = parse_requirements(&content);
        // parser-and-config has `# Requirements Document` with no title suffix
        assert!(
            parsed.title.is_none(),
            "parser-and-config requirements has no title suffix"
        );
    }

    #[test]
    fn parser_and_config_extracts_introduction() {
        let content = read_spec_file("parser-and-config", "requirements.md");
        let parsed = parse_requirements(&content);
        assert!(
            !parsed.introduction.is_empty(),
            "introduction should be non-empty"
        );
        assert!(
            parsed.introduction.contains("parser"),
            "introduction should mention the parser"
        );
    }

    #[test]
    fn parser_and_config_extracts_glossary() {
        let content = read_spec_file("parser-and-config", "requirements.md");
        let parsed = parse_requirements(&content);
        assert!(parsed.glossary.is_some(), "should have a glossary section");
        let glossary = parsed.glossary.unwrap();
        assert!(
            glossary.contains("Parser"),
            "glossary should define Parser term"
        );
    }

    #[test]
    fn parser_and_config_extracts_all_requirements() {
        let content = read_spec_file("parser-and-config", "requirements.md");
        let parsed = parse_requirements(&content);
        // parser-and-config has multiple requirements; verify we got a reasonable count
        assert!(
            parsed.requirements.len() >= 5,
            "expected at least 5 requirements, got {}",
            parsed.requirements.len()
        );
    }

    #[test]
    fn parser_and_config_requirement_has_number_and_title() {
        let content = read_spec_file("parser-and-config", "requirements.md");
        let parsed = parse_requirements(&content);
        let req1 = &parsed.requirements[0];
        assert_eq!(req1.number, "1");
        assert!(
            req1.title.is_some(),
            "first requirement should have a title"
        );
    }

    #[test]
    fn parser_and_config_requirement_has_user_story() {
        let content = read_spec_file("parser-and-config", "requirements.md");
        let parsed = parse_requirements(&content);
        let req1 = &parsed.requirements[0];
        assert!(
            req1.user_story.is_some(),
            "first requirement should have a user story"
        );
        assert!(
            req1.user_story.as_ref().unwrap().contains("developer"),
            "user story should mention the developer"
        );
    }

    #[test]
    fn parser_and_config_requirement_has_criteria() {
        let content = read_spec_file("parser-and-config", "requirements.md");
        let parsed = parse_requirements(&content);
        let req1 = &parsed.requirements[0];
        assert!(
            !req1.criteria.is_empty(),
            "first requirement should have acceptance criteria"
        );
        // Criteria should have sequential indices
        assert_eq!(req1.criteria[0].index, "1");
    }

    #[test]
    fn document_graph_extracts_requirements() {
        let content = read_spec_file("document-graph", "requirements.md");
        let parsed = parse_requirements(&content);
        assert!(
            parsed.requirements.len() >= 3,
            "document-graph should have at least 3 requirements, got {}",
            parsed.requirements.len()
        );
        // Verify glossary exists
        assert!(
            parsed.glossary.is_some(),
            "document-graph should have a glossary"
        );
    }

    #[test]
    fn kiro_import_extracts_requirements_with_title() {
        let content = read_spec_file("kiro-import", "requirements.md");
        let parsed = parse_requirements(&content);
        // kiro-import has `# Requirements Document` with no title suffix
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
    fn parser_and_config_extracts_design_sections() {
        let content = read_spec_file("parser-and-config", "design.md");
        let parsed = parse_design(&content);
        assert!(!parsed.sections.is_empty(), "design should have sections");
    }

    #[test]
    fn parser_and_config_design_has_code_blocks() {
        let content = read_spec_file("parser-and-config", "design.md");
        let parsed = parse_design(&content);
        let has_code = parsed.sections.iter().any(|s| {
            s.content
                .iter()
                .any(|b| matches!(b, DesignBlock::CodeBlock { .. }))
        });
        assert!(has_code, "design should contain code blocks");
    }

    #[test]
    fn document_graph_design_has_mermaid() {
        let content = read_spec_file("document-graph", "design.md");
        let parsed = parse_design(&content);
        let has_mermaid = parsed.sections.iter().any(|s| {
            s.content
                .iter()
                .any(|b| matches!(b, DesignBlock::MermaidBlock(_)))
        });
        assert!(
            has_mermaid,
            "document-graph design should contain mermaid diagrams"
        );
    }

    #[test]
    fn document_graph_design_has_validates_lines() {
        let content = read_spec_file("document-graph", "design.md");
        let parsed = parse_design(&content);
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
    fn parser_and_config_extracts_tasks() {
        let content = read_spec_file("parser-and-config", "tasks.md");
        let parsed = parse_tasks(&content);
        assert!(!parsed.tasks.is_empty(), "should have at least one task");
    }

    #[test]
    fn parser_and_config_tasks_have_title() {
        let content = read_spec_file("parser-and-config", "tasks.md");
        let parsed = parse_tasks(&content);
        assert_eq!(
            parsed.title.as_deref(),
            Some("Parser and Config"),
            "tasks title should match"
        );
    }

    #[test]
    fn parser_and_config_tasks_have_sub_tasks() {
        let content = read_spec_file("parser-and-config", "tasks.md");
        let parsed = parse_tasks(&content);
        let has_subs = parsed.tasks.iter().any(|t| !t.sub_tasks.is_empty());
        assert!(has_subs, "some tasks should have sub-tasks");
    }

    #[test]
    fn parser_and_config_task_status_mapping() {
        let content = read_spec_file("parser-and-config", "tasks.md");
        let parsed = parse_tasks(&content);
        // First task should be done (marked [x])
        assert_eq!(parsed.tasks[0].status, TaskStatus::Done);
    }

    #[test]
    fn parser_and_config_tasks_metadata_with_dash_prefix_is_parsed() {
        let content = read_spec_file("parser-and-config", "tasks.md");
        let parsed = parse_tasks(&content);
        // In real Kiro specs, metadata lines use `  - _Requirements: ..._` format
        // (with `- ` prefix as markdown list item). The metadata regex now handles
        // this prefix, so tasks with ref-like metadata should have TaskRefs::Refs.
        // In parser-and-config, refs appear on sub-tasks (e.g., 2.1), not top-level tasks.
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
    }

    #[test]
    fn parser_and_config_tasks_na_metadata_produces_none() {
        let content = read_spec_file("parser-and-config", "tasks.md");
        let parsed = parse_tasks(&content);
        // Task 1 has `  - _Requirements: N/A (project setup)_` — the dash-prefix
        // regex now matches, and the N/A value is correctly handled as TaskRefs::None.
        let task1 = &parsed.tasks[0];
        assert!(
            matches!(&task1.requirement_refs, TaskRefs::None),
            "task 1 should have TaskRefs::None for N/A, got {:?}",
            task1.requirement_refs
        );
    }

    #[test]
    fn document_graph_extracts_tasks() {
        let content = read_spec_file("document-graph", "tasks.md");
        let parsed = parse_tasks(&content);
        assert!(
            parsed.tasks.len() >= 3,
            "document-graph should have at least 3 tasks, got {}",
            parsed.tasks.len()
        );
    }

    #[test]
    fn kiro_import_tasks_have_many_tasks() {
        let content = read_spec_file("kiro-import", "tasks.md");
        let parsed = parse_tasks(&content);
        assert!(
            parsed.tasks.len() >= 15,
            "kiro-import should have many tasks, got {}",
            parsed.tasks.len()
        );
    }
}

// ===========================================================================
// 22.2: Edge case tests
// ===========================================================================

mod edge_empty_requirements {
    use super::*;

    #[test]
    fn empty_requirements_produces_empty_introduction() {
        let parsed = parse_requirements("");
        assert!(parsed.title.is_none());
        assert!(parsed.introduction.is_empty());
        assert!(parsed.glossary.is_none());
        assert!(parsed.requirements.is_empty());
    }

    #[test]
    fn requirements_with_no_sections_produces_prose_only() {
        let content =
            "# Requirements Document: Bare\n\nJust some prose, no requirement sections.\n";
        let parsed = parse_requirements(content);
        assert_eq!(parsed.title.as_deref(), Some("Bare"));
        assert!(parsed.requirements.is_empty());
        assert!(parsed.introduction.contains("Just some prose"));
    }

    #[test]
    fn empty_requirements_pipeline_produces_mdx_with_prose_only() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "empty-reqs",
            Some(
                "# Requirements Document: Empty\n\nSome introductory prose but no requirements.\n",
            ),
            None,
            None,
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        assert_eq!(plan.documents.len(), 1);
        let doc = &plan.documents[0];
        assert!(doc.content.contains("Some introductory prose"));
        // Should still have valid front matter
        assert!(doc.content.starts_with("---\n"));
    }
}

mod edge_tasks_no_subtasks {
    use super::*;

    #[test]
    fn tasks_without_subtasks_produce_flat_structure() {
        let content = "\
# Implementation Plan: Flat

## Tasks

- [x] 1. First task
  - Description of first task

- [ ] 2. Second task
  - Description of second task

- [-] 3. Third task in progress
";
        let parsed = parse_tasks(content);
        assert_eq!(parsed.tasks.len(), 3);
        for task in &parsed.tasks {
            assert!(
                task.sub_tasks.is_empty(),
                "task {} should have no sub-tasks",
                task.number
            );
        }
        assert_eq!(parsed.tasks[0].status, TaskStatus::Done);
        assert_eq!(parsed.tasks[1].status, TaskStatus::Ready);
        assert_eq!(parsed.tasks[2].status, TaskStatus::InProgress);
    }

    #[test]
    fn flat_tasks_emit_single_level_task_components() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "flat-tasks",
            None,
            None,
            Some(
                "\
# Implementation Plan: Flat Tasks

## Tasks

- [x] 1. First task
- [ ] 2. Second task
",
            ),
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let tasks_doc = plan
            .documents
            .iter()
            .find(|d| d.document_id.contains("tasks/"))
            .expect("should have a tasks document");
        // Should have Task components but no nesting
        assert!(tasks_doc.content.contains("<Task id=\"task-1\""));
        assert!(tasks_doc.content.contains("<Task id=\"task-2\""));
        // task-2 should depend on task-1
        assert!(tasks_doc.content.contains("depends=\"task-1\""));
    }
}

mod edge_design_no_validates {
    use super::*;

    #[test]
    fn design_without_validates_has_no_validates_blocks() {
        let content = "\
# Design Document: No Validates

## Overview

Just prose, no validates lines.

## Architecture

More prose here.
";
        let parsed = parse_design(content);
        let has_validates = parsed.sections.iter().any(|s| {
            s.content
                .iter()
                .any(|b| matches!(b, DesignBlock::ValidatesLine { .. }))
        });
        assert!(
            !has_validates,
            "design without validates lines should have no ValidatesLine blocks"
        );
    }

    #[test]
    fn design_without_validates_emits_no_validates_components() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "no-validates",
            None,
            Some("# Design: No Validates\n\n## Overview\n\nJust prose.\n"),
            None,
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let design_doc = plan
            .documents
            .iter()
            .find(|d| d.document_id.contains("design/"))
            .expect("should have a design document");
        assert!(
            !design_doc.content.contains("<Validates"),
            "should not contain <Validates> component"
        );
    }

    #[test]
    fn validates_lines_emit_refs_for_each_line_without_cross_merging() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");

        let req_md = "\
# Requirements Document: Validate Scope

### Requirement 1: First

#### Acceptance Criteria

1. THE System SHALL satisfy the first behavior.

### Requirement 2: Second

#### Acceptance Criteria

1. THE System SHALL satisfy the second behavior.
";

        let design_md = "\
# Design Document: Validate Scope

## Checks

**Validates: Requirements 1.1**

Some design rationale between validations.

**Validates: Requirements 2.1**
";

        write_kiro_spec(
            &specs_dir,
            "validates-scope",
            Some(req_md),
            Some(design_md),
            None,
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let design_doc = plan
            .documents
            .iter()
            .find(|d| d.document_id.ends_with("design/validates-scope"))
            .expect("should include design document");

        assert!(
            design_doc
                .content
                .contains("<Validates refs=\"req/validates-scope#req-1-1\" />"),
            "first validates line should map to requirement 1.1 only"
        );
        assert!(
            design_doc
                .content
                .contains("<Validates refs=\"req/validates-scope#req-2-1\" />"),
            "second validates line should map to requirement 2.1 only"
        );
        assert!(
            !design_doc
                .content
                .contains("req/validates-scope#req-1-1, req/validates-scope#req-2-1"),
            "refs from separate validates lines should not be merged together"
        );
    }
}

mod edge_task_metadata_sentinels {
    use super::*;

    #[test]
    fn na_produces_task_refs_none() {
        let content = "\
# Tasks: Sentinels

## Tasks

- [x] 1. Setup task
  _Requirements: N/A_
";
        let parsed = parse_tasks(content);
        assert!(matches!(&parsed.tasks[0].requirement_refs, TaskRefs::None));
    }

    #[test]
    fn na_with_parenthetical_produces_task_refs_none() {
        let content = "\
# Tasks: Sentinels

## Tasks

- [x] 1. Setup task
  _Requirements: N/A (project setup)_
";
        let parsed = parse_tasks(content);
        assert!(matches!(&parsed.tasks[0].requirement_refs, TaskRefs::None));
    }

    #[test]
    fn non_ref_annotation_produces_task_refs_comment() {
        let content = "\
# Tasks: Sentinels

## Tasks

- [x] 1. Infra task
  _Requirements: (test infrastructure)_
";
        let parsed = parse_tasks(content);
        assert!(
            matches!(&parsed.tasks[0].requirement_refs, TaskRefs::Comment(c) if c.contains("test infrastructure")),
            "non-ref annotation should produce TaskRefs::Comment, got {:?}",
            parsed.tasks[0].requirement_refs
        );
    }

    #[test]
    fn valid_refs_produce_task_refs_refs() {
        let content = "\
# Tasks: Refs

## Tasks

- [x] 1. Real task
  _Requirements: 1.1, 1.2_
";
        let parsed = parse_tasks(content);
        match &parsed.tasks[0].requirement_refs {
            TaskRefs::Refs(refs) => {
                assert_eq!(refs.len(), 2);
                assert_eq!(refs[0].requirement_number, "1");
                assert_eq!(refs[0].criterion_index, "1");
                assert_eq!(refs[1].requirement_number, "1");
                assert_eq!(refs[1].criterion_index, "2");
            }
            other => panic!("expected TaskRefs::Refs, got {other:?}"),
        }
    }

    #[test]
    fn bold_metadata_also_recognized() {
        let content = "\
# Tasks: Bold

## Tasks

- [x] 1. Bold task
  **Validates: Requirements 2.1, 2.2**
";
        let parsed = parse_tasks(content);
        match &parsed.tasks[0].requirement_refs {
            TaskRefs::Refs(refs) => {
                assert_eq!(refs.len(), 2);
                assert_eq!(refs[0].requirement_number, "2");
            }
            other => panic!("expected TaskRefs::Refs, got {other:?}"),
        }
    }
}

mod edge_optional_task_marker {
    use super::*;

    #[test]
    fn optional_marker_detected_on_top_level_task() {
        let content = "\
# Tasks: Optional

## Tasks

- [ ]* 1. Optional task
  - This is optional
";
        let parsed = parse_tasks(content);
        assert_eq!(parsed.tasks.len(), 1);
        assert!(
            parsed.tasks[0].is_optional,
            "task with * marker should be optional"
        );
    }

    #[test]
    fn optional_marker_detected_on_sub_task() {
        let content = "\
# Tasks: Optional Sub

## Tasks

- [x] 1. Parent task

  - [x]* 1.1 Optional sub-task
";
        let parsed = parse_tasks(content);
        assert_eq!(parsed.tasks[0].sub_tasks.len(), 1);
        assert!(
            parsed.tasks[0].sub_tasks[0].is_optional,
            "sub-task with * marker should be optional"
        );
    }

    #[test]
    fn non_optional_task_is_not_marked() {
        let content = "\
# Tasks: Normal

## Tasks

- [x] 1. Normal task
";
        let parsed = parse_tasks(content);
        assert!(
            !parsed.tasks[0].is_optional,
            "task without * marker should not be optional"
        );
    }

    #[test]
    fn optional_task_produces_ambiguity_marker_in_output() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "opt-task",
            None,
            None,
            Some(
                "\
# Implementation Plan: Opt

## Tasks

- [ ]* 1. Optional task
  - This is optional
",
            ),
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let tasks_doc = plan
            .documents
            .iter()
            .find(|d| d.document_id.contains("tasks/"))
            .unwrap();
        assert!(
            tasks_doc.content.contains("TODO(supersigil-import)"),
            "optional task should produce an ambiguity marker"
        );
        assert!(
            tasks_doc.content.contains("optional"),
            "ambiguity marker should mention 'optional'"
        );
    }
}

mod edge_non_requirement_validates {
    use super::*;

    #[test]
    fn non_requirement_validates_produces_prose_with_marker() {
        let content = "\
## Section

**Validates: Design Decision 5**
";
        let parsed = parse_design(content);
        let blocks = &parsed.sections[0].content;
        let prose = blocks.iter().find(|b| matches!(b, DesignBlock::Prose(_)));
        assert!(prose.is_some(), "should produce a Prose block");
        if let Some(DesignBlock::Prose(text)) = prose {
            assert!(
                text.contains("TODO(supersigil-import)"),
                "should contain ambiguity marker"
            );
            assert!(
                text.contains("non-requirement target"),
                "marker should describe the issue"
            );
        }
    }
}

mod edge_discovery {
    use super::*;

    #[test]
    fn nonexistent_specs_dir_returns_error() {
        let result = discover_kiro_specs(&PathBuf::from("/nonexistent/path/specs"));
        assert!(result.is_err());
        match result.unwrap_err() {
            ImportError::SpecsDirNotFound { path } => {
                assert_eq!(path, PathBuf::from("/nonexistent/path/specs"));
            }
            other => panic!("expected SpecsDirNotFound, got {other:?}"),
        }
    }

    #[test]
    fn empty_dir_with_no_features_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        std::fs::create_dir_all(&specs_dir).unwrap();

        let (dirs, diagnostics) = discover_kiro_specs(&specs_dir).unwrap();
        assert!(dirs.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn dir_with_no_recognized_files_emits_skipped_diagnostic() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        let empty_feature = specs_dir.join("empty-feature");
        std::fs::create_dir_all(&empty_feature).unwrap();
        // Create a random file that isn't requirements/design/tasks
        std::fs::write(empty_feature.join("notes.txt"), "some notes").unwrap();

        let (dirs, diagnostics) = discover_kiro_specs(&specs_dir).unwrap();
        assert!(dirs.is_empty(), "should not discover empty feature");
        assert_eq!(
            diagnostics.len(),
            1,
            "should emit one SkippedDir diagnostic"
        );
    }

    #[test]
    fn discovers_feature_with_only_design() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        let feature_dir = specs_dir.join("design-only");
        std::fs::create_dir_all(&feature_dir).unwrap();
        std::fs::write(feature_dir.join("design.md"), "# Design\n").unwrap();

        let (dirs, _) = discover_kiro_specs(&specs_dir).unwrap();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].feature_name, "design-only");
        assert!(!dirs[0].has_requirements);
        assert!(dirs[0].has_design);
        assert!(!dirs[0].has_tasks);
    }
}

mod edge_file_writing {
    use super::*;

    #[test]
    fn write_files_creates_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let docs = vec![PlannedDocument {
            output_path: tmp.path().join("deep").join("nested").join("req.mdx"),
            document_id: "req/test".to_string(),
            content: "---\ntest: true\n---\n".to_string(),
        }];

        let written = write_files(&docs, false).unwrap();
        assert_eq!(written.len(), 1);
        assert!(
            tmp.path()
                .join("deep")
                .join("nested")
                .join("req.mdx")
                .exists()
        );
    }

    #[test]
    fn write_files_without_force_fails_on_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("existing.mdx");
        std::fs::write(&output_path, "existing content").unwrap();

        let docs = vec![PlannedDocument {
            output_path: output_path.clone(),
            document_id: "req/test".to_string(),
            content: "new content".to_string(),
        }];

        let result = write_files(&docs, false);
        assert!(result.is_err());
        match result.unwrap_err() {
            ImportError::FileExists { path } => {
                assert_eq!(path, output_path);
            }
            other => panic!("expected FileExists, got {other:?}"),
        }
        // Original content should be preserved
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert_eq!(content, "existing content");
    }

    #[test]
    fn write_files_with_force_overwrites_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("existing.mdx");
        std::fs::write(&output_path, "old content").unwrap();

        let docs = vec![PlannedDocument {
            output_path: output_path.clone(),
            document_id: "req/test".to_string(),
            content: "new content".to_string(),
        }];

        let written = write_files(&docs, true).unwrap();
        assert_eq!(written.len(), 1);
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert_eq!(content, "new content");
    }

    #[test]
    fn write_files_best_effort_partial_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let good_path = tmp.path().join("good.mdx");
        let conflict_path = tmp.path().join("conflict.mdx");
        std::fs::write(&conflict_path, "existing").unwrap();

        let docs = vec![
            PlannedDocument {
                output_path: good_path.clone(),
                document_id: "req/good".to_string(),
                content: "good content".to_string(),
            },
            PlannedDocument {
                output_path: conflict_path.clone(),
                document_id: "req/conflict".to_string(),
                content: "conflict content".to_string(),
            },
        ];

        let result = write_files(&docs, false);
        assert!(result.is_err(), "should fail on second file");
        // First file should have been written (best-effort)
        assert!(good_path.exists(), "first file should be written");
        let content = std::fs::read_to_string(&good_path).unwrap();
        assert_eq!(content, "good content");
        // Second file should still have original content
        let conflict_content = std::fs::read_to_string(&conflict_path).unwrap();
        assert_eq!(conflict_content, "existing");
    }
}

// ===========================================================================
// Tests for review fixes
// ===========================================================================

mod fix_dash_prefix_metadata {
    use super::*;

    #[test]
    fn dash_prefixed_italic_metadata_parsed() {
        let content = "\
# Tasks: Dash

## Tasks

- [x] 1. Task with dash metadata
  - _Requirements: 1.1, 1.2_
";
        let parsed = parse_tasks(content);
        match &parsed.tasks[0].requirement_refs {
            TaskRefs::Refs(refs) => {
                assert_eq!(refs.len(), 2);
                assert_eq!(refs[0].requirement_number, "1");
                assert_eq!(refs[0].criterion_index, "1");
            }
            other => panic!("expected TaskRefs::Refs, got {other:?}"),
        }
    }

    #[test]
    fn dash_prefixed_bold_metadata_parsed() {
        let content = "\
# Tasks: Dash Bold

## Tasks

- [x] 1. Task with dash bold metadata
  - **Validates: Requirements 3.1, 3.2**
";
        let parsed = parse_tasks(content);
        match &parsed.tasks[0].requirement_refs {
            TaskRefs::Refs(refs) => {
                assert_eq!(refs.len(), 2);
                assert_eq!(refs[0].requirement_number, "3");
            }
            other => panic!("expected TaskRefs::Refs, got {other:?}"),
        }
    }

    #[test]
    fn dash_prefixed_na_still_produces_none() {
        let content = "\
# Tasks: Dash NA

## Tasks

- [x] 1. Setup
  - _Requirements: N/A (project setup)_
";
        let parsed = parse_tasks(content);
        assert!(
            matches!(&parsed.tasks[0].requirement_refs, TaskRefs::None),
            "dash-prefixed N/A should still produce TaskRefs::None, got {:?}",
            parsed.tasks[0].requirement_refs
        );
    }

    #[test]
    fn dash_prefixed_metadata_produces_implements_in_pipeline() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");

        let req_md = "\
# Requirements Document: Dash Test

### Requirement 1: Feature

#### Acceptance Criteria

1. THE System SHALL do something.
2. THE System SHALL do another thing.
";

        let tasks_md = "\
# Implementation Plan: Dash Test

## Tasks

- [x] 1. Implement feature
  - _Requirements: 1.1, 1.2_
";

        write_kiro_spec(&specs_dir, "dash-test", Some(req_md), None, Some(tasks_md));

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let tasks_doc = plan
            .documents
            .iter()
            .find(|d| d.document_id.contains("tasks/"))
            .expect("should have tasks document");
        assert!(
            tasks_doc.content.contains("implements="),
            "task should have implements attribute from dash-prefixed metadata"
        );
    }
}

mod fix_diagnostic_warning {
    use super::*;
    use supersigil_import::Diagnostic;

    #[test]
    fn empty_requirements_emits_warning_diagnostic() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "no-reqs",
            Some("# Requirements Document: Empty\n\nJust prose.\n"),
            None,
            None,
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let has_warning = plan.diagnostics.iter().any(
            |d| matches!(d, Diagnostic::Warning { message } if message.contains("no parseable")),
        );
        assert!(
            has_warning,
            "should emit a Warning diagnostic for empty requirements"
        );
    }

    #[test]
    fn non_empty_requirements_no_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "has-reqs",
            Some(
                "\
# Requirements Document: Has Reqs

### Requirement 1: Something

#### Acceptance Criteria

1. THE System SHALL exist.
",
            ),
            None,
            None,
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let has_warning = plan
            .diagnostics
            .iter()
            .any(|d| matches!(d, Diagnostic::Warning { .. }));
        assert!(
            !has_warning,
            "should not emit Warning for non-empty requirements"
        );
    }
}

mod fix_prose_ambiguity_counting {
    use super::*;

    #[test]
    fn non_requirement_validates_counted_in_ambiguity() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "non-req-val",
            None,
            Some(
                "\
# Design: Non Req

## Section

**Validates: Design Decision 5**
",
            ),
            None,
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();

        // Count actual markers in output
        let actual_markers: usize = plan
            .documents
            .iter()
            .map(|d| d.content.matches("<!-- TODO(supersigil-import):").count())
            .sum();

        assert!(
            actual_markers > 0,
            "should have ambiguity markers in output"
        );
        assert_eq!(
            plan.ambiguity_count, actual_markers,
            "reported ambiguity_count ({}) should match actual markers ({})",
            plan.ambiguity_count, actual_markers
        );
    }
}

mod fix_task_id_dedup {
    use super::*;

    #[test]
    fn duplicate_task_numbers_get_disambiguated() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "dup-tasks",
            None,
            None,
            Some(
                "\
# Implementation Plan: Dup

## Tasks

- [x] 1. First task
- [ ] 1. Duplicate task number
- [ ] 2. Normal task
",
            ),
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let tasks_doc = plan
            .documents
            .iter()
            .find(|d| d.document_id.contains("tasks/"))
            .expect("should have tasks document");

        // First task-1 should be normal
        assert!(
            tasks_doc.content.contains("id=\"task-1\""),
            "first occurrence should keep task-1"
        );
        // Second task-1 should be disambiguated
        assert!(
            tasks_doc.content.contains("id=\"task-1-2\"")
                || tasks_doc.content.contains("id=\"task-1-3\""),
            "duplicate should be disambiguated with suffix"
        );
        // Should have a dedup ambiguity marker
        assert!(
            tasks_doc.content.contains("Duplicate ID"),
            "should contain dedup ambiguity marker"
        );
    }

    #[test]
    fn duplicate_criterion_indices_get_disambiguated() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "dup-crit",
            Some(
                "\
# Requirements Document: Dup Crit

### Requirement 1: First

#### Acceptance Criteria

1. THE System SHALL do A.
1. THE System SHALL do B (duplicate index).
",
            ),
            None,
            None,
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let req_doc = plan
            .documents
            .iter()
            .find(|d| d.document_id.contains("req/"))
            .expect("should have requirements document");

        // First req-1-1 should be normal
        assert!(
            req_doc.content.contains("id=\"req-1-1\""),
            "first occurrence should keep req-1-1"
        );
        // Second req-1-1 should be disambiguated
        assert!(
            req_doc.content.contains("Duplicate ID"),
            "should contain dedup ambiguity marker"
        );
    }
}

mod fix_implements_collision {
    use super::*;

    #[test]
    fn duplicate_task_numbers_preserve_their_own_implements() {
        // P1: When two tasks have the same number, each should keep its own
        // implements refs rather than the second overwriting the first.
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");

        let req_md = "\
# Requirements Document: Impl Collision

### Requirement 1: First

#### Acceptance Criteria

1. THE System SHALL do alpha.

### Requirement 2: Second

#### Acceptance Criteria

1. THE System SHALL do beta.
";

        let tasks_md = "\
# Implementation Plan: Impl Collision

## Tasks

- [x] 1. First task
  _Requirements: 1.1_
- [ ] 1. Duplicate numbered task
  _Requirements: 2.1_
";

        write_kiro_spec(
            &specs_dir,
            "impl-collision",
            Some(req_md),
            None,
            Some(tasks_md),
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();
        let tasks_doc = plan
            .documents
            .iter()
            .find(|d| d.document_id.contains("tasks/"))
            .expect("should have tasks document");

        // The first task-1 should implement req-1-1
        // The second (deduped to task-1-2) should implement req-2-1
        // Neither should have the other's implements.
        assert!(
            tasks_doc.content.contains("req-1-1"),
            "first task should reference req-1-1, got:\n{}",
            tasks_doc.content
        );
        assert!(
            tasks_doc.content.contains("req-2-1"),
            "second task should reference req-2-1, got:\n{}",
            tasks_doc.content
        );
    }
}

mod fix_discovery_directory_as_file {
    use super::*;

    #[test]
    fn directory_named_requirements_md_is_not_treated_as_spec_file() {
        // P2: A directory named requirements.md should not be treated as a valid
        // spec file. Discovery should skip it.
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        let feature_dir = specs_dir.join("bad-feature");
        // Create a directory named requirements.md instead of a file
        std::fs::create_dir_all(feature_dir.join("requirements.md")).unwrap();

        let (dirs, diagnostics) = discover_kiro_specs(&specs_dir).unwrap();
        assert!(
            dirs.is_empty(),
            "directory named requirements.md should not be discovered as a spec"
        );
        assert_eq!(diagnostics.len(), 1, "should emit a SkippedDir diagnostic");
    }

    #[test]
    fn real_file_next_to_directory_impostor_is_discovered() {
        // A real design.md file should still be discovered even if requirements.md
        // is a directory.
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        let feature_dir = specs_dir.join("mixed-feature");
        std::fs::create_dir_all(feature_dir.join("requirements.md")).unwrap();
        std::fs::write(feature_dir.join("design.md"), "# Design\n").unwrap();

        let (dirs, _) = discover_kiro_specs(&specs_dir).unwrap();
        assert_eq!(dirs.len(), 1);
        assert!(
            !dirs[0].has_requirements,
            "directory should not count as requirements"
        );
        assert!(dirs[0].has_design, "real file should be discovered");
    }
}
