// Regression tests for bug fixes.

mod common;

use common::{config_for, write_kiro_spec};

use supersigil_import::discover::discover_kiro_specs;
use supersigil_import::plan_kiro_import;

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
