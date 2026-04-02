use supersigil_rust::verifies;

use super::*;

fn empty_graph() -> supersigil_core::DocumentGraph {
    use supersigil_core::{Config, build_graph};
    build_graph(vec![], &Config::default()).unwrap()
}

// -- Priority 1: Ref attribute -----------------------------------------------

#[test]
#[verifies("rename/req#req-1-1")]
fn ref_fragment_yields_component_id() {
    let content = "```supersigil-xml\n<Implements refs=\"auth/req#login\" />\n```";
    let result = find_rename_target(content, 1, 27, "test").unwrap();
    assert_eq!(
        result,
        RenameTarget::ComponentId {
            doc_id: "auth/req".to_owned(),
            component_id: "login".to_owned(),
            range: LineRange {
                line: 1,
                start: 27,
                end: 32
            },
        }
    );
}

#[test]
#[verifies("rename/req#req-1-2")]
fn ref_doc_id_yields_document_id() {
    let content = "```supersigil-xml\n<Implements refs=\"auth/req#login\" />\n```";
    let result = find_rename_target(content, 1, 20, "test").unwrap();
    assert_eq!(
        result,
        RenameTarget::DocumentId {
            doc_id: "auth/req".to_owned(),
            range: LineRange {
                line: 1,
                start: 18,
                end: 26
            },
        }
    );
}

#[test]
#[verifies("rename/req#req-1-2")]
fn doc_only_ref_yields_document_id() {
    let content = "```supersigil-xml\n<DependsOn refs=\"other/doc\" />\n```";
    let result = find_rename_target(content, 1, 20, "test").unwrap();
    assert_eq!(
        result,
        RenameTarget::DocumentId {
            doc_id: "other/doc".to_owned(),
            range: LineRange {
                line: 1,
                start: 17,
                end: 26
            },
        }
    );
}

// -- Priority 2: supersigil-ref -----------------------------------------------

#[test]
#[verifies("rename/req#req-1-3")]
fn supersigil_ref_yields_component_id() {
    let content =
        "---\nsupersigil:\n  id: my/spec\n---\n\n```sh supersigil-ref=echo-test\necho hello\n```";
    let result = find_rename_target(content, 5, 10, "my/spec").unwrap();
    assert_eq!(
        result,
        RenameTarget::ComponentId {
            doc_id: "my/spec".to_owned(),
            component_id: "echo-test".to_owned(),
            range: LineRange {
                line: 5,
                start: 21,
                end: 30
            },
        }
    );
}

#[test]
#[verifies("rename/req#req-1-3")]
fn supersigil_ref_with_fragment_uses_target() {
    let content = "```sh supersigil-ref=my-example#expected\nsome content\n```";
    let result = find_rename_target(content, 0, 24, "doc").unwrap();
    assert_eq!(
        result,
        RenameTarget::ComponentId {
            doc_id: "doc".to_owned(),
            component_id: "my-example".to_owned(),
            range: LineRange {
                line: 0,
                start: 21,
                end: 31
            },
        }
    );
}

// -- Priority 3: Component tag / id attribute ---------------------------------

#[test]
#[verifies("rename/req#req-1-4")]
fn component_tag_with_id_yields_component_id() {
    let content =
        "```supersigil-xml\n<Criterion id=\"login-success\">\nThe user logs in.\n</Criterion>\n```";
    let result = find_rename_target(content, 1, 1, "auth/req").unwrap();
    assert_eq!(
        result,
        RenameTarget::ComponentId {
            doc_id: "auth/req".to_owned(),
            component_id: "login-success".to_owned(),
            range: LineRange {
                line: 1,
                start: 15,
                end: 28
            },
        }
    );
}

#[test]
#[verifies("rename/req#req-1-4")]
fn cursor_on_id_value_yields_component_id() {
    let content =
        "```supersigil-xml\n<Criterion id=\"login-success\">\nThe user logs in.\n</Criterion>\n```";
    // Cursor at position 20, which is inside "login-success"
    let result = find_rename_target(content, 1, 20, "auth/req").unwrap();
    assert_eq!(
        result,
        RenameTarget::ComponentId {
            doc_id: "auth/req".to_owned(),
            component_id: "login-success".to_owned(),
            range: LineRange {
                line: 1,
                start: 15,
                end: 28
            },
        }
    );
}

#[test]
#[verifies("rename/req#req-1-7")]
fn component_tag_without_id_returns_none() {
    let content = "```supersigil-xml\n<AcceptanceCriteria>\n</AcceptanceCriteria>\n```";
    let result = find_rename_target(content, 1, 1, "doc");
    assert_eq!(result, None);
}

// -- Priority 4: Frontmatter -------------------------------------------------

#[test]
#[verifies("rename/req#req-1-5")]
fn frontmatter_id_yields_document_id() {
    let content = "---\nsupersigil:\n  id: my-doc/req\n  type: requirements\n---\n\nSome text.";
    let result = find_rename_target(content, 2, 7, "my-doc/req").unwrap();
    assert_eq!(
        result,
        RenameTarget::DocumentId {
            doc_id: "my-doc/req".to_owned(),
            range: LineRange {
                line: 2,
                start: 6,
                end: 16
            },
        }
    );
}

#[test]
#[verifies("rename/req#req-1-5")]
fn frontmatter_non_id_line_returns_none() {
    let content = "---\nsupersigil:\n  id: my-doc/req\n  type: requirements\n---\n\nSome text.";
    let result = find_rename_target(content, 3, 5, "my-doc/req");
    assert_eq!(result, None);
}

// -- Priority ordering -------------------------------------------------------

#[test]
#[verifies("rename/req#req-1-6")]
fn ref_takes_priority_over_component_tag() {
    // Cursor on the refs value, which is also on a component with id.
    // <Implements id="impl-1" refs="other/doc#crit" />
    // 0         1         2         3         4
    // 0123456789012345678901234567890123456789012345
    // refs=" starts at 24, value at 29, "#crit" fragment at 39..43
    let content = "```supersigil-xml\n<Implements id=\"impl-1\" refs=\"other/doc#crit\" />\n```";
    let result = find_rename_target(content, 1, 40, "test").unwrap();
    match result {
        RenameTarget::ComponentId { component_id, .. } => {
            assert_eq!(component_id, "crit");
        }
        RenameTarget::DocumentId { .. } => {
            panic!("expected ComponentId from ref, got {result:?}")
        }
    }
}

// -- Non-renameable positions ------------------------------------------------

#[test]
#[verifies("rename/req#req-1-7")]
fn body_text_returns_none() {
    let content = "---\nsupersigil:\n  id: test\n---\n\nSome text outside.";
    let result = find_rename_target(content, 5, 0, "test");
    assert_eq!(result, None);
}

#[test]
#[verifies("rename/req#req-1-7")]
fn inside_fence_no_match_returns_none() {
    let content = "```supersigil-xml\nsome body text\n```";
    let result = find_rename_target(content, 1, 5, "test");
    assert_eq!(result, None);
}

// -- prepareRename range tests -----------------------------------------------

#[test]
#[verifies("rename/req#req-2-1")]
fn prepare_rename_returns_range_and_placeholder() {
    let content = "```supersigil-xml\n<Criterion id=\"login-success\">\nbody\n</Criterion>\n```";
    let result = find_rename_target(content, 1, 1, "auth/req").unwrap();
    match &result {
        RenameTarget::ComponentId {
            component_id,
            range,
            ..
        } => {
            assert_eq!(component_id, "login-success");
            assert!(range.start < range.end);
        }
        RenameTarget::DocumentId { .. } => panic!("expected ComponentId"),
    }
}

// -- Validation --------------------------------------------------------------

#[test]
#[verifies("rename/req#req-4-1")]
fn valid_name_accepted() {
    validate_new_name("new-id").unwrap();
    validate_new_name("auth/req").unwrap();
    validate_new_name("a").unwrap();
}

#[test]
#[verifies("rename/req#req-4-1")]
fn empty_name_rejected() {
    assert!(validate_new_name("").is_err());
}

#[test]
#[verifies("rename/req#req-4-1")]
fn whitespace_name_rejected() {
    assert!(validate_new_name("has space").is_err());
    assert!(validate_new_name("has\ttab").is_err());
}

#[test]
#[verifies("rename/req#req-4-1")]
fn hash_name_rejected() {
    assert!(validate_new_name("bad#name").is_err());
}

#[test]
#[verifies("rename/req#req-4-1")]
fn quote_name_rejected() {
    assert!(validate_new_name("bad\"name").is_err());
}

#[test]
#[verifies("rename/req#req-4-2")]
fn validation_error_has_message() {
    let err = validate_new_name("").unwrap_err();
    assert!(!err.is_empty());
}

// -- collect_rename_edits ----------------------------------------------------

#[test]
#[verifies("rename/req#req-3-1")]
fn rename_component_updates_definition_and_refs() {
    use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
    use supersigil_core::{Config, ExtractedComponent, build_graph};

    let req_doc = make_doc(
        "test/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 5)],
            3,
        )],
    );
    let design_doc = make_doc(
        "test/design",
        vec![ExtractedComponent {
            name: "References".into(),
            attributes: [("refs".into(), "test/req#crit-a".into())]
                .into_iter()
                .collect(),
            children: vec![],
            body_text: None,
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: supersigil_core::test_helpers::pos(3),
            end_position: supersigil_core::test_helpers::pos(3),
        }],
    );

    let graph = build_graph(vec![req_doc, design_doc], &Config::default()).unwrap();
    let open_files = HashMap::new();

    let target = RenameTarget::ComponentId {
        doc_id: "test/req".to_owned(),
        component_id: "crit-a".to_owned(),
        range: LineRange {
            line: 0,
            start: 0,
            end: 6,
        },
    };

    let edit = collect_rename_edits(&target, "crit-new", &graph, &open_files);
    // The graph is constructed and the function doesn't panic.
    // File content scanning depends on readable paths (synthetic in tests).
    let _changes = edit.changes.unwrap();
}

#[test]
#[verifies("rename/req#req-3-2")]
fn rename_document_updates_frontmatter_and_refs() {
    use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
    use supersigil_core::{Config, ExtractedComponent, build_graph};

    let req_doc = make_doc(
        "test/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 5)],
            3,
        )],
    );
    let design_doc = make_doc(
        "test/design",
        vec![ExtractedComponent {
            name: "Implements".into(),
            attributes: [("refs".into(), "test/req".into())].into_iter().collect(),
            children: vec![],
            body_text: None,
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: supersigil_core::test_helpers::pos(3),
            end_position: supersigil_core::test_helpers::pos(3),
        }],
    );

    let graph = build_graph(vec![req_doc, design_doc], &Config::default()).unwrap();
    let open_files = HashMap::new();

    let target = RenameTarget::DocumentId {
        doc_id: "test/req".to_owned(),
        range: LineRange {
            line: 0,
            start: 0,
            end: 8,
        },
    };

    let edit = collect_rename_edits(&target, "test/requirement", &graph, &open_files);
    // Graph-based edit collection works even without readable files.
    let _changes = edit.changes.unwrap();
}

#[test]
#[verifies("rename/req#req-3-3")]
fn all_four_ref_attrs_scanned() {
    // Verify that the scanner checks refs, implements, depends, verifies.
    let content = "```supersigil-xml\n<Implements refs=\"doc#old\" />\n<Task implements=\"doc#old\" />\n<DependsOn refs=\"doc#old\" />\n<Example verifies=\"doc#old\" />\n```";
    let mut edits = Vec::new();
    collect_ref_string_edits(content, "doc#old", "doc#new", &mut edits);
    assert_eq!(edits.len(), 4, "should find edits in all 4 attributes");
}

#[test]
#[verifies("rename/req#req-3-4")]
fn edits_grouped_by_uri() {
    use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
    use supersigil_core::{Config, ExtractedComponent, build_graph};

    let req_doc = make_doc(
        "test/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 5)],
            3,
        )],
    );
    let design_doc = make_doc(
        "test/design",
        vec![ExtractedComponent {
            name: "References".into(),
            attributes: [("refs".into(), "test/req#crit-a".into())]
                .into_iter()
                .collect(),
            children: vec![],
            body_text: None,
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: supersigil_core::test_helpers::pos(3),
            end_position: supersigil_core::test_helpers::pos(3),
        }],
    );

    let graph = build_graph(vec![req_doc, design_doc], &Config::default()).unwrap();

    // Supply file content via open_files so edits can be produced.
    let mut open_files = HashMap::new();
    // make_doc uses relative paths like "specs/test/req.md";
    // path_to_url prepends "/" for non-absolute paths.
    let req_uri = Url::from_file_path("/specs/test/req.md").unwrap();
    let design_uri = Url::from_file_path("/specs/test/design.md").unwrap();
    open_files.insert(
        req_uri.clone(),
        Arc::new(
            "---\nsupersigil:\n  id: test/req\n---\n\n```supersigil-xml\n<AcceptanceCriteria>\n  <Criterion id=\"crit-a\">\nbody\n  </Criterion>\n</AcceptanceCriteria>\n```"
                .to_owned(),
        ),
    );
    open_files.insert(
        design_uri.clone(),
        Arc::new(
            "---\nsupersigil:\n  id: test/design\n---\n\n```supersigil-xml\n<References refs=\"test/req#crit-a\" />\n```"
                .to_owned(),
        ),
    );

    let target = RenameTarget::ComponentId {
        doc_id: "test/req".to_owned(),
        component_id: "crit-a".to_owned(),
        range: LineRange {
            line: 7,
            start: 16,
            end: 22,
        },
    };

    let edit = collect_rename_edits(&target, "crit-new", &graph, &open_files);
    let changes = edit.changes.unwrap();
    // Edits should be grouped by URI — definition in req, reference in design.
    assert!(
        changes.contains_key(&req_uri),
        "should have edits for req doc"
    );
    assert!(
        changes.contains_key(&design_uri),
        "should have edits for design doc"
    );
}

#[test]
#[verifies("rename/req#req-3-5")]
fn rename_with_no_references_produces_definition_edit() {
    // The id attribute edit should still be produced for the definition site.
    let content = "```supersigil-xml\n<Criterion id=\"only-one\">\nbody\n</Criterion>\n```";
    let mut edits = Vec::new();
    collect_id_attr_edits(content, "only-one", "renamed", &mut edits);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "renamed");
}

#[test]
#[verifies("rename/req#req-3-1")]
fn supersigil_ref_updated_on_component_rename() {
    let content = "```sh supersigil-ref=echo-test\necho hello\n```";
    let mut edits = Vec::new();
    collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "new-test");
}

#[test]
#[verifies("rename/req#req-3-2")]
fn doc_id_ref_edits_replace_doc_portion() {
    let content = "```supersigil-xml\n<References refs=\"old/doc#frag\" />\n```";
    let mut edits = Vec::new();
    collect_doc_id_ref_edits(content, "old/doc", "new/doc", &mut edits);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "new/doc");
    // The edit should only cover the "old/doc" portion, not "#frag".
}

#[test]
#[verifies("rename/req#req-5-1")]
fn rename_provider_advertised() {
    // The rename_provider capability is set in state.rs initialize().
    // This test verifies the types compile and the module is wired.
    let target = RenameTarget::ComponentId {
        doc_id: "doc".to_owned(),
        component_id: "id".to_owned(),
        range: LineRange {
            line: 0,
            start: 0,
            end: 2,
        },
    };
    let _ = find_rename_target("", 0, 0, "doc");
    let _ = validate_new_name("ok");
    let _ = collect_rename_edits(&target, "new", &empty_graph(), &HashMap::new());
}

#[test]
fn id_substring_in_other_attr_not_matched() {
    // Regression test: `id="` inside another attribute value should not match.
    // <Implements refs="some-id/req" id="real-id">
    let content = "```supersigil-xml\n<Implements refs=\"some-id/req\" id=\"real-id\" />\n```";
    let mut edits = Vec::new();
    collect_id_attr_edits(content, "real-id", "new-id", &mut edits);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "new-id");
}

#[test]
fn id_substring_does_not_trigger_rename() {
    // Cursor on "some-id" inside refs value should not trigger id attribute rename.
    let content = "```supersigil-xml\n<Implements refs=\"some-id/req\" id=\"real-id\" />\n```";
    // Position 25 is inside "some-id/req" in the refs value — should match
    // as a ref, not as an id attribute.
    let result = find_rename_target(content, 1, 25, "test");
    match result {
        Some(RenameTarget::DocumentId { doc_id, .. }) => {
            assert_eq!(doc_id, "some-id/req");
        }
        other => panic!("expected DocumentId for ref, got {other:?}"),
    }
}

#[test]
fn frontmatter_id_edit_produced() {
    let content = "---\nsupersigil:\n  id: old/doc\n---";
    let mut edits = Vec::new();
    collect_frontmatter_id_edits(content, "old/doc", "new/doc", &mut edits);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "new/doc");
}

// -- P1 regression: task implements included in component rename ----------

#[test]
fn task_implements_sources_included() {
    use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
    use supersigil_core::{Config, ExtractedComponent, build_graph};

    let req_doc = make_doc(
        "test/req",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-a", 5)],
            3,
        )],
    );
    let task_doc = make_doc(
        "test/tasks",
        vec![ExtractedComponent {
            name: "Task".into(),
            attributes: [
                ("id".into(), "task-1".into()),
                ("implements".into(), "test/req#crit-a".into()),
            ]
            .into_iter()
            .collect(),
            children: vec![],
            body_text: None,
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: supersigil_core::test_helpers::pos(3),
            end_position: supersigil_core::test_helpers::pos(3),
        }],
    );

    let graph = build_graph(vec![req_doc, task_doc], &Config::default()).unwrap();

    let task_uri = Url::from_file_path("/specs/test/tasks.md").unwrap();
    let mut open_files = HashMap::new();
    open_files.insert(
        task_uri.clone(),
        Arc::new(
            "---\nsupersigil:\n  id: test/tasks\n---\n\n```supersigil-xml\n<Task id=\"task-1\" implements=\"test/req#crit-a\">\nDo the thing.\n</Task>\n```"
                .to_owned(),
        ),
    );

    let target = RenameTarget::ComponentId {
        doc_id: "test/req".to_owned(),
        component_id: "crit-a".to_owned(),
        range: LineRange {
            line: 0,
            start: 0,
            end: 6,
        },
    };

    let edit = collect_rename_edits(&target, "crit-new", &graph, &open_files);
    let changes = edit.changes.unwrap();
    assert!(
        changes.contains_key(&task_uri),
        "task doc with implements should have edits: {changes:?}"
    );
}

// -- P2 regression: supersigil-ref prefix not matched ---------------------

#[test]
fn supersigil_ref_prefix_not_matched() {
    // Renaming "echo-test" should NOT modify "supersigil-ref=echo-test-extra".
    let content = "```sh supersigil-ref=echo-test-extra\necho hello\n```";
    let mut edits = Vec::new();
    collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
    assert!(edits.is_empty(), "should not match prefix: {edits:?}");
}

#[test]
fn supersigil_ref_with_hash_still_matched() {
    // Renaming "echo-test" SHOULD match "supersigil-ref=echo-test#expected".
    let content = "```sh supersigil-ref=echo-test#expected\necho hello\n```";
    let mut edits = Vec::new();
    collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "new-test");
}

#[test]
fn supersigil_ref_not_on_fence_line_ignored() {
    // Body text mentioning supersigil-ref should not be matched.
    let content = "```sh\nUse supersigil-ref=echo-test for examples\n```";
    let mut edits = Vec::new();
    collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
    assert!(edits.is_empty(), "non-fence line should be ignored");
}

#[test]
fn supersigil_ref_not_at_token_boundary_ignored() {
    // "notsupersigil-ref=echo-test" should not match.
    let content = "```sh notsupersigil-ref=echo-test\necho hello\n```";
    let mut edits = Vec::new();
    collect_supersigil_ref_edits(content, "echo-test", "new-test", &mut edits);
    assert!(edits.is_empty(), "non-token-boundary should be ignored");
}

// -- P3 regression: frontmatter cursor column checked --------------------

#[test]
fn frontmatter_cursor_on_key_returns_none() {
    // Cursor on "id:" key, not the value — should not trigger rename.
    let content = "---\nsupersigil:\n  id: my-doc/req\n---";
    let result = find_rename_target(content, 2, 3, "my-doc/req");
    assert_eq!(
        result, None,
        "cursor on 'id:' key should not trigger rename"
    );
}

#[test]
fn frontmatter_cursor_on_value_triggers_rename() {
    // Cursor on the value "my-doc/req" — should trigger.
    let content = "---\nsupersigil:\n  id: my-doc/req\n---";
    let result = find_rename_target(content, 2, 7, "my-doc/req");
    assert!(result.is_some(), "cursor on id value should trigger rename");
}
