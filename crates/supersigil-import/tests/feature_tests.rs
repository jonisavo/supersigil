// Feature tests for specific features.

mod common;

use common::{config_for, write_kiro_spec};

use supersigil_import::plan_kiro_import;

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
        let (output, ambiguity, _) = emit_tasks_md(&parsed, "test/tasks", None, "", "Test");

        // Must NOT contain JSX/MDX comment syntax
        assert!(
            !output.contains("{/*"),
            "output must not contain JSX comment syntax `{{/*`, got:\n{output}"
        );
        assert!(
            !output.contains("*/}"),
            "output must not contain JSX comment syntax `*/}}`, got:\n{output}"
        );
        // Comment should appear as an HTML comment marker outside fences
        assert!(
            output.contains("<!-- TODO(supersigil-import): Kiro metadata for task"),
            "output must contain marker comment, got:\n{output}"
        );
        assert!(
            output.contains("test infrastructure"),
            "marker must contain the original comment text, got:\n{output}"
        );
        // Comment should count as an ambiguity
        assert!(ambiguity >= 1, "comment should increase ambiguity count");
    }

    #[test]
    fn comment_marker_escapes_double_dash() {
        // XML comments must not contain `--`; the emitter should replace it with `- -`.
        let parsed = make_parsed_tasks_with_comment("note -- important");
        let (output, _, _) = emit_tasks_md(&parsed, "test/tasks", None, "", "Test");

        assert!(
            !output.contains("{/*"),
            "output must not contain JSX comment syntax"
        );
        // The raw `--` in the comment body must be escaped to `- -`
        assert!(
            output.contains("note - - important"),
            "output must escape `--` to `- -` inside XML comment, got:\n{output}"
        );
    }
}
