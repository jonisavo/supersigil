//! Property tests for task implements resolution (pipeline stage 4).
//!
//! Property 20: Task implements resolution
//! Validates: Requirements 11.1, 11.2

use proptest::prelude::*;

use crate::graph::tests::generators::{
    arb_component_id, arb_id, make_acceptance_criteria, make_criterion, make_doc_with_path,
    make_task, single_project_config, two_project_config,
};
use crate::graph::{GraphError, build_graph};

// ---------------------------------------------------------------------------
// Property 20: Task implements resolution
// ---------------------------------------------------------------------------

proptest! {
    /// Valid `implements` refs with `#fragment` targeting a `Criterion` resolve
    /// successfully and are available via `task_implements`.
    ///
    /// Validates: Requirements 11.1
    #[test]
    fn prop_task_implements_valid_refs_resolve(
        req_doc_id in arb_id(),
        tasks_doc_id in arb_id(),
        crit_id in arb_component_id(),
        task_id in arb_component_id(),
    ) {
        prop_assume!(req_doc_id != tasks_doc_id);
        prop_assume!(crit_id != task_id);

        let config = single_project_config();

        // Requirement doc with a Criterion.
        let req_doc = make_doc_with_path(
            &req_doc_id,
            &format!("specs/{req_doc_id}.mdx"),
            vec![make_acceptance_criteria(vec![make_criterion(&crit_id, 2)], 1)],
        );

        // Tasks doc with a Task that implements the criterion.
        let impl_ref = format!("{req_doc_id}#{crit_id}");
        let tasks_doc = make_doc_with_path(
            &tasks_doc_id,
            &format!("specs/{tasks_doc_id}.mdx"),
            vec![make_task(&task_id, None, Some(&impl_ref), None, 1)],
        );

        let graph = build_graph(vec![req_doc, tasks_doc], &config)
            .expect("build_graph should succeed with valid task implements ref");

        let implements = graph
            .task_implements(&tasks_doc_id, &task_id)
            .expect("task_implements should return resolved refs");

        prop_assert_eq!(implements.len(), 1);
        prop_assert_eq!(&implements[0].0, &req_doc_id);
        prop_assert_eq!(&implements[0].1, &crit_id);
    }

    /// Multiple valid `implements` refs on a single Task all resolve.
    ///
    /// Validates: Requirements 11.1
    #[test]
    fn prop_task_implements_multiple_refs(
        req_doc_id in arb_id(),
        tasks_doc_id in arb_id(),
        crit_a in arb_component_id(),
        crit_b in arb_component_id(),
        task_id in arb_component_id(),
    ) {
        prop_assume!(req_doc_id != tasks_doc_id);
        prop_assume!(crit_a != crit_b);
        prop_assume!(task_id != crit_a && task_id != crit_b);

        let config = single_project_config();

        let req_doc = make_doc_with_path(
            &req_doc_id,
            &format!("specs/{req_doc_id}.mdx"),
            vec![make_acceptance_criteria(
                vec![make_criterion(&crit_a, 2), make_criterion(&crit_b, 3)],
                1,
            )],
        );

        let impl_refs = format!("{req_doc_id}#{crit_a}, {req_doc_id}#{crit_b}");
        let tasks_doc = make_doc_with_path(
            &tasks_doc_id,
            &format!("specs/{tasks_doc_id}.mdx"),
            vec![make_task(&task_id, None, Some(&impl_refs), None, 1)],
        );

        let graph = build_graph(vec![req_doc, tasks_doc], &config)
            .expect("build_graph should succeed with multiple valid implements refs");

        let implements = graph
            .task_implements(&tasks_doc_id, &task_id)
            .expect("task_implements should return all resolved refs");

        prop_assert_eq!(implements.len(), 2);

        let targets: Vec<(&str, &str)> = implements.iter().map(|(d, c)| (d.as_str(), c.as_str())).collect();
        prop_assert!(targets.contains(&(req_doc_id.as_str(), crit_a.as_str())));
        prop_assert!(targets.contains(&(req_doc_id.as_str(), crit_b.as_str())));
    }

    /// An `implements` ref without a `#fragment` produces a `BrokenRef` error.
    ///
    /// Validates: Requirements 11.1
    #[test]
    fn prop_task_implements_missing_fragment_is_broken(
        req_doc_id in arb_id(),
        tasks_doc_id in arb_id(),
        task_id in arb_component_id(),
    ) {
        prop_assume!(req_doc_id != tasks_doc_id);

        let config = single_project_config();

        let req_doc = make_doc_with_path(
            &req_doc_id,
            &format!("specs/{req_doc_id}.mdx"),
            vec![],
        );

        // Task implements ref has no fragment — just a doc ID.
        let tasks_doc = make_doc_with_path(
            &tasks_doc_id,
            &format!("specs/{tasks_doc_id}.mdx"),
            vec![make_task(&task_id, None, Some(&req_doc_id), None, 1)],
        );

        let result = build_graph(vec![req_doc, tasks_doc], &config);
        let errors = result.expect_err("build_graph should fail: implements ref without fragment");

        let broken: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::BrokenRef {
                    doc_id,
                    ref_str,
                    reason,
                    ..
                } if doc_id == &tasks_doc_id && ref_str == &req_doc_id => Some(reason.clone()),
                _ => None,
            })
            .collect();

        prop_assert!(!broken.is_empty(), "expected BrokenRef for implements ref without fragment");
        prop_assert!(
            broken[0].contains("fragment"),
            "error reason should mention missing fragment, got: {}",
            broken[0]
        );
    }

    /// An `implements` ref pointing to a nonexistent criterion produces a `BrokenRef` error.
    ///
    /// Validates: Requirements 11.2
    #[test]
    fn prop_task_implements_nonexistent_criterion_is_broken(
        req_doc_id in arb_id(),
        tasks_doc_id in arb_id(),
        task_id in arb_component_id(),
        bad_crit_id in arb_component_id(),
    ) {
        prop_assume!(req_doc_id != tasks_doc_id);

        let config = single_project_config();

        // Requirement doc exists but has no criteria.
        let req_doc = make_doc_with_path(
            &req_doc_id,
            &format!("specs/{req_doc_id}.mdx"),
            vec![],
        );

        let impl_ref = format!("{req_doc_id}#{bad_crit_id}");
        let tasks_doc = make_doc_with_path(
            &tasks_doc_id,
            &format!("specs/{tasks_doc_id}.mdx"),
            vec![make_task(&task_id, None, Some(&impl_ref), None, 1)],
        );

        let result = build_graph(vec![req_doc, tasks_doc], &config);
        let errors = result.expect_err("build_graph should fail: implements ref to nonexistent criterion");

        let broken: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::BrokenRef {
                    doc_id,
                    ref_str,
                    ..
                } if doc_id == &tasks_doc_id && ref_str == &impl_ref => Some(()),
                _ => None,
            })
            .collect();

        prop_assert!(!broken.is_empty(), "expected BrokenRef for nonexistent criterion");
    }

    /// An `implements` ref pointing to a nonexistent document produces a `BrokenRef` error.
    ///
    /// Validates: Requirements 11.2
    #[test]
    fn prop_task_implements_nonexistent_doc_is_broken(
        tasks_doc_id in arb_id(),
        nonexistent_id in arb_id(),
        task_id in arb_component_id(),
        crit_id in arb_component_id(),
    ) {
        prop_assume!(tasks_doc_id != nonexistent_id);

        let config = single_project_config();

        let impl_ref = format!("{nonexistent_id}#{crit_id}");
        let tasks_doc = make_doc_with_path(
            &tasks_doc_id,
            &format!("specs/{tasks_doc_id}.mdx"),
            vec![make_task(&task_id, None, Some(&impl_ref), None, 1)],
        );

        let result = build_graph(vec![tasks_doc], &config);
        let errors = result.expect_err("build_graph should fail: implements ref to nonexistent doc");

        let broken: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::BrokenRef {
                    doc_id,
                    ref_str,
                    ..
                } if doc_id == &tasks_doc_id && ref_str == &impl_ref => Some(()),
                _ => None,
            })
            .collect();

        prop_assert!(!broken.is_empty(), "expected BrokenRef for nonexistent doc in implements ref");
    }

    /// A Task with no `implements` attribute produces no entries in `task_implements`.
    ///
    /// Validates: Requirements 11.1 (only tasks with implements are resolved)
    #[test]
    fn prop_task_without_implements_has_no_entries(
        tasks_doc_id in arb_id(),
        task_id in arb_component_id(),
    ) {
        let config = single_project_config();

        let tasks_doc = make_doc_with_path(
            &tasks_doc_id,
            &format!("specs/{tasks_doc_id}.mdx"),
            vec![make_task(&task_id, None, None, None, 1)],
        );

        let graph = build_graph(vec![tasks_doc], &config)
            .expect("build_graph should succeed with no implements refs");

        prop_assert!(
            graph.task_implements(&tasks_doc_id, &task_id).is_none(),
            "task without implements should have no entries"
        );
    }

    /// A Task `implements` ref from an isolated project to a criterion in
    /// another project produces a `BrokenRef` error, consistent with the
    /// cross-project isolation applied to normal `refs` resolution (Req 3/4).
    ///
    /// Validates: Requirements 11.1 ("same resolution logic as Requirement 3")
    #[test]
    fn prop_task_implements_respects_project_isolation(
        req_doc_id in arb_id(),
        tasks_doc_id in arb_id(),
        crit_id in arb_component_id(),
        task_id in arb_component_id(),
    ) {
        prop_assume!(req_doc_id != tasks_doc_id);
        prop_assume!(crit_id != task_id);

        let config = two_project_config(true, false);

        // Requirement doc in project-b with a Criterion.
        let req_doc = make_doc_with_path(
            &req_doc_id,
            &format!("project-b/specs/{req_doc_id}.mdx"),
            vec![make_acceptance_criteria(vec![make_criterion(&crit_id, 2)], 1)],
        );

        // Tasks doc in isolated project-a with a Task that implements the
        // criterion in project-b — this should be rejected.
        let impl_ref = format!("{req_doc_id}#{crit_id}");
        let tasks_doc = make_doc_with_path(
            &tasks_doc_id,
            &format!("project-a/specs/{tasks_doc_id}.mdx"),
            vec![make_task(&task_id, None, Some(&impl_ref), None, 1)],
        );

        let result = build_graph(vec![req_doc, tasks_doc], &config);
        let errors = result.expect_err(
            "build_graph should fail: task implements ref crosses isolated project boundary",
        );

        let broken: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::BrokenRef {
                    doc_id,
                    ref_str,
                    reason,
                    ..
                } if doc_id == &tasks_doc_id && ref_str == &impl_ref => Some(reason.clone()),
                _ => None,
            })
            .collect();

        prop_assert!(
            !broken.is_empty(),
            "expected BrokenRef for cross-project task implements ref from isolated project"
        );
        prop_assert!(
            broken[0].contains("cross-project"),
            "error reason should mention cross-project violation, got: {}",
            broken[0]
        );
    }
}
