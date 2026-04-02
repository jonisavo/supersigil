// Edge case tests for boundary conditions and error paths.
//
// 22.2: Edge case tests for boundary conditions and error paths.

mod common;

use common::{config_for, write_kiro_spec};
use std::path::PathBuf;

use supersigil_import::discover::discover_kiro_specs;
use supersigil_import::parse::design::{DesignBlock, parse_design};
use supersigil_import::parse::requirements::parse_requirements;
use supersigil_import::parse::tasks::{TaskRefs, TaskStatus, parse_tasks};
use supersigil_import::{ImportError, PlannedDocument, plan_kiro_import};

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
    use supersigil_import::write::write_files;

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
