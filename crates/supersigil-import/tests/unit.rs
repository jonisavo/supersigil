//! Unit tests for real-world Kiro spec inputs and edge cases.

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
        .join("tests/fixtures/.kiro/specs")
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
            .find(|d| d.document_id.ends_with("/tasks"))
            .expect("should have a tasks document");
        assert!(tasks_doc.content.contains("<Task id=\"task-1\""));
        assert!(tasks_doc.content.contains("<Task id=\"task-2\""));
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
            .find(|d| d.document_id.ends_with("/design"))
            .expect("should have a design document");
        assert!(
            !design_doc.content.contains("<References"),
            "should not contain <References> component"
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
            .find(|d| d.document_id.ends_with("validates-scope/design"))
            .expect("should include design document");

        assert!(
            design_doc
                .content
                .contains("<References refs=\"validates-scope/req#req-1-1\" />"),
            "first validates line should map to requirement 1.1 only"
        );
        assert!(
            design_doc
                .content
                .contains("<References refs=\"validates-scope/req#req-2-1\" />"),
            "second validates line should map to requirement 2.1 only"
        );
        assert!(
            !design_doc
                .content
                .contains("validates-scope/req#req-1-1, validates-scope/req#req-2-1"),
            "refs from separate validates lines should not be merged together"
        );
    }
}

/// Table-driven tests for task metadata parsing (N/A, refs, bold, dash-prefixed).
mod edge_task_metadata {
    use super::*;

    /// Helper: parse a minimal tasks.md and return the first task's `requirement_refs`.
    fn parse_task_refs(metadata_line: &str) -> TaskRefs {
        let content = format!(
            "\
# Tasks: Test

## Tasks

- [x] 1. Test task
  {metadata_line}
"
        );
        let parsed = parse_tasks(&content);
        parsed.tasks[0].requirement_refs.clone()
    }

    #[test]
    fn na_produces_none() {
        assert!(matches!(
            parse_task_refs("_Requirements: N/A_"),
            TaskRefs::None
        ));
    }

    #[test]
    fn na_with_parenthetical_produces_none() {
        assert!(matches!(
            parse_task_refs("_Requirements: N/A (project setup)_"),
            TaskRefs::None
        ));
    }

    #[test]
    fn dash_prefixed_na_produces_none() {
        assert!(matches!(
            parse_task_refs("- _Requirements: N/A (project setup)_"),
            TaskRefs::None
        ));
    }

    #[test]
    fn non_ref_annotation_produces_comment() {
        let refs = parse_task_refs("_Requirements: (test infrastructure)_");
        assert!(
            matches!(&refs, TaskRefs::Comment(c) if c.contains("test infrastructure")),
            "non-ref annotation should produce TaskRefs::Comment, got {refs:?}",
        );
    }

    #[test]
    fn italic_refs_produce_refs() {
        let refs = parse_task_refs("_Requirements: 1.1, 1.2_");
        match &refs {
            TaskRefs::Refs(r) => {
                assert_eq!(r.len(), 2);
                assert_eq!(r[0].requirement_number, "1");
                assert_eq!(r[0].criterion_index, "1");
                assert_eq!(r[1].requirement_number, "1");
                assert_eq!(r[1].criterion_index, "2");
            }
            other => panic!("expected TaskRefs::Refs, got {other:?}"),
        }
    }

    #[test]
    fn dash_prefixed_italic_refs_produce_refs() {
        let refs = parse_task_refs("- _Requirements: 1.1, 1.2_");
        match &refs {
            TaskRefs::Refs(r) => {
                assert_eq!(r.len(), 2);
                assert_eq!(r[0].requirement_number, "1");
            }
            other => panic!("expected TaskRefs::Refs, got {other:?}"),
        }
    }

    #[test]
    fn bold_validates_produce_refs() {
        let refs = parse_task_refs("**Validates: Requirements 2.1, 2.2**");
        match &refs {
            TaskRefs::Refs(r) => {
                assert_eq!(r.len(), 2);
                assert_eq!(r[0].requirement_number, "2");
            }
            other => panic!("expected TaskRefs::Refs, got {other:?}"),
        }
    }

    #[test]
    fn dash_prefixed_bold_validates_produce_refs() {
        let refs = parse_task_refs("- **Validates: Requirements 3.1, 3.2**");
        match &refs {
            TaskRefs::Refs(r) => {
                assert_eq!(r.len(), 2);
                assert_eq!(r[0].requirement_number, "3");
            }
            other => panic!("expected TaskRefs::Refs, got {other:?}"),
        }
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
            .find(|d| d.document_id.ends_with("/tasks"))
            .expect("should have tasks document");
        assert!(
            tasks_doc.content.contains("implements="),
            "task should have implements attribute from dash-prefixed metadata"
        );
    }
}

mod edge_optional_task_marker {
    use super::*;

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
            .find(|d| d.document_id.ends_with("/tasks"))
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

mod edge_discovery {
    use super::*;
    use supersigil_rust::verifies;

    #[verifies("kiro-import/req#req-1-3")]
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

    #[verifies("kiro-import/req#req-1-2")]
    #[test]
    fn dir_with_no_recognized_files_emits_skipped_diagnostic() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        let empty_feature = specs_dir.join("empty-feature");
        std::fs::create_dir_all(&empty_feature).unwrap();
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
    fn write_files_best_effort_partial_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let good_path = tmp.path().join("good.md");
        let conflict_path = tmp.path().join("conflict.md");
        std::fs::write(&conflict_path, "existing").unwrap();

        let docs = vec![
            PlannedDocument {
                output_path: good_path.clone(),
                document_id: "good/req".to_string(),
                content: "good content".to_string(),
            },
            PlannedDocument {
                output_path: conflict_path.clone(),
                document_id: "conflict/req".to_string(),
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
            .find(|d| d.document_id.ends_with("/tasks"))
            .expect("should have tasks document");

        assert!(
            tasks_doc.content.contains("id=\"task-1\""),
            "first occurrence should keep task-1"
        );
        assert!(
            tasks_doc.content.contains("id=\"task-1-2\"")
                || tasks_doc.content.contains("id=\"task-1-3\""),
            "duplicate should be disambiguated with suffix"
        );
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
            .find(|d| d.document_id.ends_with("/req"))
            .expect("should have requirements document");

        assert!(
            req_doc.content.contains("id=\"req-1-1\""),
            "first occurrence should keep req-1-1"
        );
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
            .find(|d| d.document_id.ends_with("/tasks"))
            .expect("should have tasks document");

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
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        let feature_dir = specs_dir.join("bad-feature");
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

mod output_filenames {
    use super::*;
    use supersigil_rust::verifies;

    #[verifies("kiro-import/req#req-2-2")]
    #[test]
    fn plan_uses_feature_prefixed_output_filenames() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");

        let req_md = "\
# Requirements Document: Naming

### Requirement 1: Paths

#### Acceptance Criteria

1. THE System SHALL write files with unique basenames.
";

        let design_md = "\
# Design Document: Naming

## Overview

The importer should produce unique filenames.
";

        let tasks_md = "\
# Implementation Plan: Naming

## Tasks

- [x] 1. Update filenames
";

        write_kiro_spec(
            &specs_dir,
            "unique-names",
            Some(req_md),
            Some(design_md),
            Some(tasks_md),
        );

        let config = config_for(&specs_dir, &tmp.path().join("out"));
        let plan = plan_kiro_import(&config).unwrap();

        let actual_paths: std::collections::BTreeSet<_> = plan
            .documents
            .iter()
            .map(|doc| {
                doc.output_path
                    .strip_prefix(&config.output_dir)
                    .unwrap()
                    .to_path_buf()
            })
            .collect();

        let expected_paths = std::collections::BTreeSet::from([
            PathBuf::from("unique-names").join("unique-names.req.md"),
            PathBuf::from("unique-names").join("unique-names.design.md"),
            PathBuf::from("unique-names").join("unique-names.tasks.md"),
        ]);

        assert_eq!(actual_paths, expected_paths);
    }
}

mod feature_title_precedence {
    use super::*;
    use supersigil_rust::verifies;

    /// Helper: run `plan_kiro_import` on a single feature and return the title
    /// used in the first emitted document's front matter.
    fn extract_title(
        specs_dir: &std::path::Path,
        output_dir: &std::path::Path,
        feature: &str,
    ) -> String {
        let config = config_for(specs_dir, output_dir);
        let plan = plan_kiro_import(&config).unwrap();
        assert!(
            !plan.documents.is_empty(),
            "expected at least one document for feature '{feature}'"
        );
        // Parse the title from the YAML front matter of the first document.
        let content = &plan.documents[0].content;
        let rest = content.strip_prefix("---\n").expect("front matter start");
        let end = rest.find("\n---\n").expect("front matter end");
        let fm = &rest[..end];
        fm.lines()
            .find_map(|line| {
                line.strip_prefix("title: ")
                    .map(|v| v.trim_matches('"').to_string())
            })
            .expect("title in front matter")
    }

    #[verifies("kiro-import/req#req-2-4")]
    #[test]
    fn requirements_title_takes_highest_precedence() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "prec-req",
            Some(
                "# Requirements Document: Req Title\n\n### Requirement 1: A\n\n#### Acceptance Criteria\n\n1. THE System SHALL exist.\n",
            ),
            Some("# Design Document: Design Title\n\n## Overview\n\nSome design.\n"),
            Some("# Implementation Plan: Tasks Title\n\n## Tasks\n\n- [x] 1. Do it\n"),
        );

        let title = extract_title(&specs_dir, &tmp.path().join("out"), "prec-req");
        assert_eq!(title, "Req Title");
    }

    #[verifies("kiro-import/req#req-2-4")]
    #[test]
    fn design_title_used_when_requirements_has_no_title() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "prec-design",
            Some(
                "# Requirements Document\n\nSome intro.\n\n### Requirement 1: A\n\n#### Acceptance Criteria\n\n1. THE System SHALL exist.\n",
            ),
            Some("# Design Document: Design Title\n\n## Overview\n\nSome design.\n"),
            Some("# Implementation Plan: Tasks Title\n\n## Tasks\n\n- [x] 1. Do it\n"),
        );

        let title = extract_title(&specs_dir, &tmp.path().join("out"), "prec-design");
        assert_eq!(title, "Design Title");
    }

    #[verifies("kiro-import/req#req-2-4")]
    #[test]
    fn tasks_title_used_when_requirements_and_design_have_no_title() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        write_kiro_spec(
            &specs_dir,
            "prec-tasks",
            None,
            None,
            Some("# Implementation Plan: Tasks Title\n\n## Tasks\n\n- [x] 1. Do it\n"),
        );

        let title = extract_title(&specs_dir, &tmp.path().join("out"), "prec-tasks");
        assert_eq!(title, "Tasks Title");
    }

    #[verifies("kiro-import/req#req-2-4")]
    #[test]
    fn directory_name_used_when_no_parsed_titles() {
        let tmp = tempfile::tempdir().unwrap();
        let specs_dir = tmp.path().join("specs");
        // requirements.md with no title suffix, no design, no tasks
        write_kiro_spec(
            &specs_dir,
            "fallback-dir-name",
            Some(
                "# Requirements Document\n\nSome intro.\n\n### Requirement 1: A\n\n#### Acceptance Criteria\n\n1. THE System SHALL exist.\n",
            ),
            None,
            None,
        );

        let title = extract_title(&specs_dir, &tmp.path().join("out"), "fallback-dir-name");
        assert_eq!(title, "fallback-dir-name");
    }
}

mod fix_task_comment_syntax {
    use supersigil_import::emit::tasks::emit_tasks_md;
    use supersigil_import::parse::tasks::{ParsedTask, ParsedTasks, TaskRefs, TaskStatus};

    fn make_parsed_tasks_with_comment(comment: &str) -> ParsedTasks {
        ParsedTasks {
            title: None,
            preamble: vec![],
            tasks: vec![ParsedTask {
                number: "1".to_string(),
                title: "Do something".to_string(),
                status: TaskStatus::Ready,
                is_optional: false,
                description: vec![],
                requirement_refs: TaskRefs::Comment(comment.to_string()),
                sub_tasks: vec![],
            }],
            postamble: vec![],
        }
    }

    #[test]
    fn comment_emitted_as_marker_outside_fence() {
        let parsed = make_parsed_tasks_with_comment("test infrastructure");
        let (output, breakdown, _) = emit_tasks_md(&parsed, "test/tasks", None, "", "Test");

        // Must NOT contain JSX/MDX comment syntax
        assert!(
            !output.contains("{/*"),
            "output must not contain JSX comment syntax `{{/*`, got:\n{output}"
        );
        assert!(
            !output.contains("*/}"),
            "output must not contain JSX comment syntax `*/}}`, got:\n{output}"
        );
        // Comment should appear as a blockquote marker outside fences
        assert!(
            output.contains("> **TODO(supersigil-import):** Kiro metadata for task"),
            "output must contain marker comment, got:\n{output}"
        );
        assert!(
            output.contains("test infrastructure"),
            "marker must contain the original comment text, got:\n{output}"
        );
        // Comment should count as an ambiguity
        assert!(
            breakdown.total() >= 1,
            "comment should increase ambiguity count"
        );
    }

    #[test]
    fn comment_marker_preserves_double_dash() {
        // Blockquote format does not require escaping `--` (unlike HTML comments).
        let parsed = make_parsed_tasks_with_comment("note -- important");
        let (output, _, _) = emit_tasks_md(&parsed, "test/tasks", None, "", "Test");

        assert!(
            output.contains("note -- important"),
            "output must preserve original `--` in blockquote marker, got:\n{output}"
        );
    }
}

mod ambiguity_breakdown_tracking {
    use supersigil_import::emit::design::emit_design_md;
    use supersigil_import::emit::tasks::emit_tasks_md;
    use supersigil_import::parse::design::parse_design;
    use supersigil_import::parse::tasks::{ParsedTask, ParsedTasks, TaskRefs, TaskStatus};

    #[test]
    fn design_without_requirements_reports_missing_context() {
        let design_md = "# Design: Test\n\n## Overview\n\nSome content.\n";
        let parsed = parse_design(design_md);
        let (_, breakdown, _) = emit_design_md(&parsed, "test/design", None, "", "Test");
        assert_eq!(breakdown.missing_context, 1);
        assert_eq!(breakdown.total(), 1);
    }

    #[test]
    fn optional_task_reports_unsupported_feature() {
        let parsed = ParsedTasks {
            title: None,
            preamble: vec![],
            postamble: vec![],
            tasks: vec![ParsedTask {
                number: "1".to_string(),
                title: "Optional task".to_string(),
                status: TaskStatus::Ready,
                is_optional: true,
                description: vec![],
                requirement_refs: TaskRefs::None,
                sub_tasks: vec![],
            }],
        };
        let (_, breakdown, _) = emit_tasks_md(&parsed, "test/tasks", None, "", "Test");
        assert_eq!(breakdown.unsupported_feature, 1);
    }

    #[test]
    fn task_metadata_comment_reports_unsupported_feature() {
        let parsed = ParsedTasks {
            title: None,
            preamble: vec![],
            postamble: vec![],
            tasks: vec![ParsedTask {
                number: "1".to_string(),
                title: "Do something".to_string(),
                status: TaskStatus::Ready,
                is_optional: false,
                description: vec![],
                requirement_refs: TaskRefs::Comment("test note".to_string()),
                sub_tasks: vec![],
            }],
        };
        let (_, breakdown, _) = emit_tasks_md(&parsed, "test/tasks", None, "", "Test");
        assert_eq!(breakdown.unsupported_feature, 1);
    }
}
