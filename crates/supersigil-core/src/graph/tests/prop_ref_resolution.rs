//! Property tests for ref resolution (pipeline stage 3).

use std::collections::HashMap;

use proptest::prelude::*;

use crate::graph::tests::generators::{
    arb_component_id, arb_id, make_acceptance_criteria, make_criterion, make_doc_with_path,
    make_refs_component, single_project_config, two_project_config,
};
use crate::graph::{GraphError, IMPLEMENTS, TASK, VALIDATES, build_graph};
use crate::{ExtractedComponent, ProjectConfig, SourcePosition};

// ---------------------------------------------------------------------------
// Property 5: Valid refs resolve successfully
// ---------------------------------------------------------------------------

proptest! {
    /// For any ref string pointing to an existing document (doc-only) or an
    /// existing document + fragment (doc#fragment), ref resolution should
    /// succeed and produce a `ResolvedRef` with the correct target doc ID
    /// and fragment. When the component def has `target_component`, the
    /// resolved component's name must match.
    ///
    /// Validates: Requirements 3.2, 3.3, 3.4
    #[test]
    fn prop_valid_refs_resolve_successfully(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        crit_id in arb_component_id(),
    ) {
        // Ensure source and target are distinct documents.
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        // Target document has a Criterion component.
        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![make_criterion(&crit_id, 1)],
        );

        // Source document A: Validates with doc#fragment ref (target_component = Criterion).
        let fragment_ref = format!("{target_doc_id}#{crit_id}");
        let source_doc_a = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(VALIDATES, &fragment_ref, 1)],
        );

        let graph = build_graph(vec![target_doc.clone(), source_doc_a], &config)
            .expect("build_graph should succeed with valid refs");

        // The Validates component is at index [0] in source_doc_a.
        let resolved = graph
            .resolved_refs(&source_doc_id, &[0])
            .expect("resolved_refs should return refs for the Validates component");

        prop_assert_eq!(resolved.len(), 1);
        prop_assert_eq!(&resolved[0].target_doc_id, &target_doc_id);
        prop_assert_eq!(resolved[0].fragment.as_deref(), Some(crit_id.as_str()));
        prop_assert_eq!(&resolved[0].raw, &fragment_ref);
    }

    /// Doc-only refs (no fragment) resolve to the target document.
    ///
    /// Validates: Requirements 3.2
    #[test]
    fn prop_valid_doc_only_refs_resolve(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![],
        );

        // Implements has no target_component, so doc-only ref is fine.
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(IMPLEMENTS, &target_doc_id, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed with valid doc-only ref");

        let resolved = graph
            .resolved_refs(&source_doc_id, &[0])
            .expect("resolved_refs should return refs for the Implements component");

        prop_assert_eq!(resolved.len(), 1);
        prop_assert_eq!(&resolved[0].target_doc_id, &target_doc_id);
        prop_assert!(resolved[0].fragment.is_none());
    }

    /// Validates component with target_component = "Criterion" resolves when
    /// the fragment points to a Criterion.
    ///
    /// Validates: Requirements 3.4
    #[test]
    fn prop_target_component_type_matching(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        crit_id in arb_component_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        // Target doc has a Criterion nested inside AcceptanceCriteria.
        let nested_crit = make_criterion(&crit_id, 5);
        let ac = make_acceptance_criteria(vec![nested_crit], 4);
        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![ac],
        );

        let fragment_ref = format!("{target_doc_id}#{crit_id}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(VALIDATES, &fragment_ref, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed: Validates targets Criterion correctly");

        let resolved = graph
            .resolved_refs(&source_doc_id, &[0])
            .expect("resolved_refs should return refs");

        prop_assert_eq!(resolved.len(), 1);
        prop_assert_eq!(&resolved[0].target_doc_id, &target_doc_id);
        prop_assert_eq!(resolved[0].fragment.as_deref(), Some(crit_id.as_str()));
    }
}

// ---------------------------------------------------------------------------
// Property 6: Invalid refs produce broken_ref errors
// ---------------------------------------------------------------------------

proptest! {
    /// A ref pointing to a nonexistent document ID produces a BrokenRef error.
    ///
    /// Validates: Requirements 3.5
    #[test]
    fn prop_broken_ref_nonexistent_doc(
        source_doc_id in arb_id(),
        nonexistent_id in arb_id(),
    ) {
        prop_assume!(source_doc_id != nonexistent_id);

        let config = single_project_config();

        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(IMPLEMENTS, &nonexistent_id, 1)],
        );

        let result = build_graph(vec![source_doc], &config);
        let errors = result.expect_err("build_graph should fail with broken ref");

        let broken: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::BrokenRef {
                    doc_id,
                    ref_str,
                    ..
                } if doc_id == &source_doc_id && ref_str == &nonexistent_id => Some(()),
                _ => None,
            })
            .collect();

        prop_assert!(!broken.is_empty(), "expected BrokenRef for nonexistent doc");
    }

    /// A ref with a fragment pointing to a nonexistent component produces a BrokenRef error.
    ///
    /// Validates: Requirements 3.6
    #[test]
    fn prop_broken_ref_nonexistent_fragment(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        bad_fragment in arb_component_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        // Target doc exists but has no referenceable components.
        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![],
        );

        let ref_str = format!("{target_doc_id}#{bad_fragment}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(VALIDATES, &ref_str, 1)],
        );

        let result = build_graph(vec![target_doc, source_doc], &config);
        let errors = result.expect_err("build_graph should fail with broken fragment ref");

        let broken: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::BrokenRef {
                    doc_id,
                    ref_str: rs,
                    ..
                } if doc_id == &source_doc_id && rs == &ref_str => Some(()),
                _ => None,
            })
            .collect();

        prop_assert!(!broken.is_empty(), "expected BrokenRef for nonexistent fragment");
    }

    /// A Validates ref with a fragment that resolves to a non-Criterion component
    /// (wrong target_component type) produces a BrokenRef error.
    ///
    /// Validates: Requirements 3.7
    #[test]
    fn prop_broken_ref_wrong_target_component(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
        task_id in arb_component_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = single_project_config();

        // Target doc has a Task (referenceable), not a Criterion.
        let task = ExtractedComponent {
            name: TASK.to_owned(),
            attributes: HashMap::from([("id".to_owned(), task_id.clone())]),
            children: Vec::new(),
            body_text: Some("a task".to_owned()),
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
        };
        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("specs/{target_doc_id}.mdx"),
            vec![task],
        );

        // Validates has target_component = "Criterion", but fragment points to a Task.
        let ref_str = format!("{target_doc_id}#{task_id}");
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("specs/{source_doc_id}.mdx"),
            vec![make_refs_component(VALIDATES, &ref_str, 1)],
        );

        let result = build_graph(vec![target_doc, source_doc], &config);
        let errors = result.expect_err("build_graph should fail with wrong target_component");

        let broken: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::BrokenRef {
                    doc_id,
                    ref_str: rs,
                    reason,
                    ..
                } if doc_id == &source_doc_id && rs == &ref_str => Some(reason.clone()),
                _ => None,
            })
            .collect();

        prop_assert!(!broken.is_empty(), "expected BrokenRef for wrong target_component type");
    }
}

// ---------------------------------------------------------------------------
// Property 7: Non-isolated cross-project refs resolve globally
// ---------------------------------------------------------------------------

proptest! {
    /// In a multi-project config where no project has `isolated = true`,
    /// a ref from a document in one project to a document in another project
    /// resolves successfully against the global document index.
    ///
    /// Validates: Requirements 4.1, 4.4
    #[test]
    fn prop_non_isolated_cross_project_refs(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = two_project_config(false, false);

        // Target in project-a, source in project-b.
        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("project-a/specs/{target_doc_id}.mdx"),
            vec![],
        );
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("project-b/specs/{source_doc_id}.mdx"),
            vec![make_refs_component(IMPLEMENTS, &target_doc_id, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed: non-isolated cross-project ref");

        let resolved = graph
            .resolved_refs(&source_doc_id, &[0])
            .expect("cross-project ref should resolve");

        prop_assert_eq!(resolved.len(), 1);
        prop_assert_eq!(&resolved[0].target_doc_id, &target_doc_id);
    }
}

// ---------------------------------------------------------------------------
// Property 8: Isolated project refs are restricted
// ---------------------------------------------------------------------------

proptest! {
    /// For a project configured with `isolated = true`, a ref from a document
    /// in that project to a document in a different project produces a
    /// BrokenRef error, even if the target document exists in the global index.
    ///
    /// Validates: Requirements 4.2, 4.3
    #[test]
    fn prop_isolated_project_ref_restriction(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let config = two_project_config(true, false);

        // Source in isolated project-a, target in project-b.
        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("project-b/specs/{target_doc_id}.mdx"),
            vec![],
        );
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("project-a/specs/{source_doc_id}.mdx"),
            vec![make_refs_component(IMPLEMENTS, &target_doc_id, 1)],
        );

        let result = build_graph(vec![target_doc, source_doc], &config);
        let errors = result.expect_err("build_graph should fail: isolated project cross-ref");

        let broken: Vec<_> = errors
            .iter()
            .filter_map(|e| match e {
                GraphError::BrokenRef {
                    doc_id,
                    ref_str,
                    ..
                } if doc_id == &source_doc_id && ref_str == &target_doc_id => Some(()),
                _ => None,
            })
            .collect();

        prop_assert!(!broken.is_empty(), "expected BrokenRef for isolated cross-project ref");
    }

    /// A ref within the same isolated project resolves successfully.
    ///
    /// Validates: Requirements 4.2 (same-project refs are allowed)
    #[test]
    fn prop_isolated_project_same_project_ref_ok(
        target_doc_id in arb_id(),
        source_doc_id in arb_id(),
    ) {
        prop_assume!(target_doc_id != source_doc_id);

        let mut projects = HashMap::new();
        projects.insert(
            "project-a".to_owned(),
            ProjectConfig {
                paths: vec!["project-a/specs/**/*.mdx".to_owned()],
                tests: Vec::new(),
                isolated: true,
            },
        );

        let config = crate::Config {
            paths: None,
            tests: None,
            projects: Some(projects),
            id_pattern: None,
            documents: crate::DocumentsConfig {
                types: HashMap::new(),
            },
            components: HashMap::new(),
            verify: crate::VerifyConfig {
                strictness: None,
                rules: HashMap::new(),
            },
            ecosystem: crate::EcosystemConfig {
                plugins: vec!["rust".to_owned()],
            },
            hooks: crate::HooksConfig::default(),
            test_results: crate::TestResultsConfig {
                formats: Vec::new(),
                paths: Vec::new(),
            },
        };

        // Both docs in the same isolated project.
        let target_doc = make_doc_with_path(
            &target_doc_id,
            &format!("project-a/specs/{target_doc_id}.mdx"),
            vec![],
        );
        let source_doc = make_doc_with_path(
            &source_doc_id,
            &format!("project-a/specs/{source_doc_id}.mdx"),
            vec![make_refs_component(IMPLEMENTS, &target_doc_id, 1)],
        );

        let graph = build_graph(vec![target_doc, source_doc], &config)
            .expect("build_graph should succeed: same isolated project ref");

        let resolved = graph
            .resolved_refs(&source_doc_id, &[0])
            .expect("same-project ref should resolve");

        prop_assert_eq!(resolved.len(), 1);
        prop_assert_eq!(&resolved[0].target_doc_id, &target_doc_id);
    }
}
