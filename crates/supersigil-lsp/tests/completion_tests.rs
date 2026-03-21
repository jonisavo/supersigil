//! Integration tests for autocomplete (req-3-1 through req-3-4).

use std::collections::HashMap;
use std::path::PathBuf;

use lsp_types::CompletionItemKind;
use supersigil_core::{
    ComponentDefs, Config, ExtractedComponent, Frontmatter, SourcePosition, SpecDocument,
    build_graph,
};
use supersigil_lsp::completion::{
    CompletionContext, complete_attribute_values, complete_component_names, complete_document_ids,
    complete_fragment_ids, detect_context,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn pos(line: usize) -> SourcePosition {
    SourcePosition {
        byte_offset: line * 40,
        line,
        column: 1,
    }
}

fn make_criterion(id: &str, body: &str, line: usize) -> ExtractedComponent {
    ExtractedComponent {
        name: "Criterion".to_owned(),
        attributes: HashMap::from([("id".to_owned(), id.to_owned())]),
        children: Vec::new(),
        body_text: Some(body.to_owned()),
        code_blocks: Vec::new(),
        position: pos(line),
    }
}

fn make_doc(id: &str, doc_type: Option<&str>, components: Vec<ExtractedComponent>) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(format!("/specs/{id}.mdx")),
        frontmatter: Frontmatter {
            id: id.to_owned(),
            doc_type: doc_type.map(str::to_owned),
            status: None,
        },
        extra: HashMap::new(),
        components,
    }
}

fn default_config() -> Config {
    Config {
        paths: Some(vec!["specs/**/*.mdx".to_owned()]),
        ..Config::default()
    }
}

// ---------------------------------------------------------------------------
// detect_context — ref-accepting attribute, before '#' (req-3-1)
// ---------------------------------------------------------------------------

#[test]
fn detect_refs_attribute_doc_id_prefix() {
    // `refs="auth-` — cursor after `auth-`
    let line = r#"<References refs="auth-"#;
    //            0         1         2
    //            012345678901234567890123
    // `refs="` starts at 13; value starts at 19. Cursor at 24 = after `auth-`.
    let ctx = detect_context(line, 0, 24);
    assert_eq!(
        ctx,
        CompletionContext::RefDocId {
            prefix: "auth-".to_owned()
        }
    );
}

#[test]
fn detect_implements_attribute_doc_id_prefix() {
    let line = r#"<Task id="t1" implements="design/"#;
    // `implements="` starts at 14; value starts at 26.
    // Cursor at 33 = after `design/`.
    let ctx = detect_context(line, 0, 33);
    assert_eq!(
        ctx,
        CompletionContext::RefDocId {
            prefix: "design/".to_owned()
        }
    );
}

#[test]
fn detect_depends_attribute_empty_prefix() {
    let line = r#"<Task id="t1" depends=""#;
    // `depends="` starts at 14; value starts at 23.
    // Cursor at 23 = just after opening quote, empty prefix.
    let ctx = detect_context(line, 0, 23);
    assert_eq!(
        ctx,
        CompletionContext::RefDocId {
            prefix: String::new()
        }
    );
}

// ---------------------------------------------------------------------------
// detect_context — ref-accepting attribute, after '#' (req-3-2)
// ---------------------------------------------------------------------------

#[test]
fn detect_refs_attribute_fragment_prefix() {
    // `refs="auth/req#req-` — cursor after `req-`
    let line = r#"<References refs="auth/req#req-"#;
    //                                  0         1         2         3
    //                                  0123456789012345678901234567890
    // `refs="` starts at 13; value starts at 19.
    // `auth/req#req-` is 14 chars; cursor at 19 + 14 = 33.
    let ctx = detect_context(line, 0, 33);
    assert_eq!(
        ctx,
        CompletionContext::RefFragment {
            doc_id: "auth/req".to_owned(),
            prefix: "req-".to_owned()
        }
    );
}

#[test]
fn detect_refs_attribute_fragment_empty_prefix() {
    let line = r#"<References refs="auth/req#"#;
    // cursor at 27 = just after `#`
    let ctx = detect_context(line, 0, 27);
    assert_eq!(
        ctx,
        CompletionContext::RefFragment {
            doc_id: "auth/req".to_owned(),
            prefix: String::new()
        }
    );
}

#[test]
fn detect_second_ref_in_comma_list() {
    // `refs="a/req, auth/req#req-` — cursor after `req-` in second token
    let line = r#"<References refs="a/req, auth/req#req-"#;
    // After last comma + space: `auth/req#req-`
    // Cursor at end = 38
    let ctx = detect_context(line, 0, 38);
    assert_eq!(
        ctx,
        CompletionContext::RefFragment {
            doc_id: "auth/req".to_owned(),
            prefix: "req-".to_owned()
        }
    );
}

// ---------------------------------------------------------------------------
// detect_context — component name after '<' (req-3-3)
// ---------------------------------------------------------------------------

#[test]
fn detect_component_name_prefix() {
    let line = "<Cri";
    let ctx = detect_context(line, 0, 4);
    assert_eq!(
        ctx,
        CompletionContext::ComponentName {
            prefix: "Cri".to_owned()
        }
    );
}

#[test]
fn detect_component_name_empty_prefix() {
    let line = "<";
    let ctx = detect_context(line, 0, 1);
    assert_eq!(
        ctx,
        CompletionContext::ComponentName {
            prefix: String::new()
        }
    );
}

#[test]
fn detect_closing_tag_returns_none() {
    let line = "</Criterion>";
    let ctx = detect_context(line, 0, 3);
    // After `</` — this is a closing tag, not a completion context.
    assert_eq!(ctx, CompletionContext::None);
}

#[test]
fn detect_component_name_with_whitespace_returns_none() {
    // Cursor is inside the attributes of an already-opened tag.
    let line = r#"<Task id=""#;
    let ctx = detect_context(line, 0, 10);
    // After `<Task ` there's whitespace, so component name detection should not fire.
    assert_eq!(ctx, CompletionContext::None);
}

// ---------------------------------------------------------------------------
// detect_context — strategy / status attributes (req-3-4)
// ---------------------------------------------------------------------------

#[test]
fn detect_strategy_attribute_prefix() {
    let line = r#"<VerifiedBy strategy="ta"#;
    // `strategy="` starts at 12; value starts at 22. Cursor at 24 = after `ta`.
    let ctx = detect_context(line, 0, 24);
    assert_eq!(
        ctx,
        CompletionContext::AttributeStrategy {
            prefix: "ta".to_owned()
        }
    );
}

#[test]
fn detect_status_attribute_prefix() {
    let line = r#"<Task id="t1" status="dra"#;
    // `status="` starts at 14; value starts at 22. Cursor at 25 = after `dra`.
    let ctx = detect_context(line, 0, 25);
    assert_eq!(
        ctx,
        CompletionContext::AttributeStatus {
            prefix: "dra".to_owned()
        }
    );
}

#[test]
fn detect_plain_text_returns_none() {
    let line = "This is just some plain text.";
    let ctx = detect_context(line, 0, 10);
    assert_eq!(ctx, CompletionContext::None);
}

#[test]
fn detect_out_of_bounds_line_returns_none() {
    let content = "only one line";
    let ctx = detect_context(content, 5, 0);
    assert_eq!(ctx, CompletionContext::None);
}

// ---------------------------------------------------------------------------
// complete_document_ids (req-3-1)
// ---------------------------------------------------------------------------

#[test]
fn complete_doc_ids_returns_matching_items() {
    let docs = vec![
        make_doc("auth/req", Some("requirements"), vec![]),
        make_doc("auth/design", Some("design"), vec![]),
        make_doc("billing/req", Some("requirements"), vec![]),
    ];
    let graph = build_graph(docs, &default_config()).expect("graph must build");

    let items = complete_document_ids("auth/", &graph);

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"auth/req"), "should include auth/req");
    assert!(
        labels.contains(&"auth/design"),
        "should include auth/design"
    );
    assert!(
        !labels.contains(&"billing/req"),
        "should not include billing/req"
    );

    for item in &items {
        assert_eq!(item.kind, Some(CompletionItemKind::REFERENCE));
    }
}

#[test]
fn complete_doc_ids_empty_prefix_returns_all() {
    let docs = vec![
        make_doc("a/req", None, vec![]),
        make_doc("b/req", None, vec![]),
    ];
    let graph = build_graph(docs, &default_config()).expect("graph must build");
    let items = complete_document_ids("", &graph);
    assert_eq!(items.len(), 2);
}

#[test]
fn complete_doc_ids_no_match_returns_empty() {
    let docs = vec![make_doc("auth/req", None, vec![])];
    let graph = build_graph(docs, &default_config()).expect("graph must build");
    let items = complete_document_ids("zzz", &graph);
    assert!(items.is_empty());
}

// ---------------------------------------------------------------------------
// complete_fragment_ids (req-3-2)
// ---------------------------------------------------------------------------

#[test]
fn complete_fragment_ids_returns_matching_items() {
    let doc = make_doc(
        "auth/req",
        None,
        vec![
            make_criterion("req-1-1", "User can log in", 5),
            make_criterion("req-1-2", "User sees error on bad password", 8),
            make_criterion("billing-1", "User is charged", 11),
        ],
    );
    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");

    let items = complete_fragment_ids("auth/req", "req-", &graph);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"req-1-1"));
    assert!(labels.contains(&"req-1-2"));
    assert!(!labels.contains(&"billing-1"));

    for item in &items {
        assert_eq!(item.kind, Some(CompletionItemKind::REFERENCE));
    }
}

#[test]
fn complete_fragment_ids_wrong_doc_returns_empty() {
    let doc = make_doc("auth/req", None, vec![make_criterion("req-1-1", "body", 5)]);
    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");

    let items = complete_fragment_ids("billing/req", "req-", &graph);
    assert!(items.is_empty());
}

#[test]
fn complete_fragment_ids_body_preview_in_detail() {
    let long_body = "A".repeat(80);
    let doc = make_doc(
        "auth/req",
        None,
        vec![make_criterion("req-1-1", &long_body, 5)],
    );
    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");

    let items = complete_fragment_ids("auth/req", "", &graph);
    assert_eq!(items.len(), 1);
    let detail = items[0].detail.as_deref().expect("should have detail");
    // Preview should be truncated to 60 chars + ellipsis.
    assert!(detail.len() < long_body.len(), "detail should be truncated");
    assert!(detail.ends_with('…'), "detail should end with ellipsis");
}

// ---------------------------------------------------------------------------
// complete_component_names (req-3-3)
// ---------------------------------------------------------------------------

#[test]
fn complete_component_names_prefix_match() {
    let defs = ComponentDefs::defaults();
    let items = complete_component_names("Crit", &defs);

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"Criterion"), "should include Criterion");

    for item in &items {
        assert_eq!(item.kind, Some(CompletionItemKind::CLASS));
        assert_eq!(item.detail.as_deref(), Some("Supersigil"));
        assert!(
            item.insert_text.is_some(),
            "should have insert text snippet"
        );
    }
}

#[test]
fn complete_component_names_empty_prefix_returns_all() {
    let defs = ComponentDefs::defaults();
    let items = complete_component_names("", &defs);
    assert_eq!(items.len(), defs.len());
}

#[test]
fn complete_component_names_criterion_has_snippet() {
    let defs = ComponentDefs::defaults();
    let items = complete_component_names("Criterion", &defs);

    let item = items
        .iter()
        .find(|i| i.label == "Criterion")
        .expect("Criterion");
    let snippet = item.insert_text.as_deref().expect("snippet");
    // Criterion is referenceable, so it should have a body form with closing tag.
    assert!(
        snippet.contains("id="),
        "snippet should include id attribute"
    );
    assert!(
        snippet.contains("</Criterion>"),
        "snippet should include closing tag"
    );
}

#[test]
fn complete_component_names_references_has_snippet() {
    let defs = ComponentDefs::defaults();
    let items = complete_component_names("References", &defs);

    let item = items
        .iter()
        .find(|i| i.label == "References")
        .expect("References");
    let snippet = item.insert_text.as_deref().expect("snippet");
    // References is not referenceable, self-closing with `refs` attribute.
    assert!(
        snippet.contains("refs="),
        "snippet should include refs attribute"
    );
    assert!(snippet.contains("/>"), "snippet should be self-closing");
}

#[test]
fn complete_component_names_no_match_returns_empty() {
    let defs = ComponentDefs::defaults();
    let items = complete_component_names("Zzz", &defs);
    assert!(items.is_empty());
}

// ---------------------------------------------------------------------------
// complete_attribute_values (req-3-4)
// ---------------------------------------------------------------------------

#[test]
fn complete_strategy_values() {
    let items = complete_attribute_values("strategy", "");
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"tag"));
    assert!(labels.contains(&"file-glob"));

    for item in &items {
        assert_eq!(item.kind, Some(CompletionItemKind::ENUM_MEMBER));
    }
}

#[test]
fn complete_strategy_values_with_prefix() {
    let items = complete_attribute_values("strategy", "ta");
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"tag"), "should match 'tag'");
    assert!(
        !labels.contains(&"file-glob"),
        "should not match 'file-glob'"
    );
}

#[test]
fn complete_status_values() {
    let items = complete_attribute_values("status", "");
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"draft"));
    assert!(labels.contains(&"approved"));
    assert!(labels.contains(&"in-progress"));
    assert!(labels.contains(&"done"));

    for item in &items {
        assert_eq!(item.kind, Some(CompletionItemKind::ENUM_MEMBER));
    }
}

#[test]
fn complete_status_values_with_prefix() {
    let items = complete_attribute_values("status", "dra");
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"draft"), "should match 'draft'");
    assert!(!labels.contains(&"done"), "should not match 'done'");
}

#[test]
fn complete_unknown_attribute_returns_empty() {
    let items = complete_attribute_values("unknown", "");
    assert!(items.is_empty());
}
