//! Integration tests for autocomplete (req-3-1 through req-3-4).

use std::collections::HashMap;
use std::path::PathBuf;

use lsp_types::CompletionItemKind;
use supersigil_core::{
    ComponentDefs, Config, ExtractedComponent, Frontmatter, SourcePosition, SpecDocument,
    build_graph,
};
use supersigil_lsp::completion::{
    CompletionContext, StatusContext, complete_component_names, complete_document_ids,
    complete_fragment_ids, complete_status, complete_strategy, detect_context,
};
use supersigil_rust_macros::verifies;

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
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: Vec::new(),
        position: pos(line),
        end_position: pos(line),
    }
}

fn make_doc(id: &str, doc_type: Option<&str>, components: Vec<ExtractedComponent>) -> SpecDocument {
    SpecDocument {
        path: PathBuf::from(format!("/specs/{id}.md")),
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
        paths: Some(vec!["specs/**/*.md".to_owned()]),
        ..Config::default()
    }
}

/// Wrap a single line of content inside a `supersigil-xml` fence.
/// Returns `(content, line)` where `line` is the 0-based line number of the
/// wrapped content (always 1).
fn fenced(line_content: &str) -> (String, u32) {
    let content = format!("```supersigil-xml\n{line_content}\n```");
    (content, 1)
}

/// Wrap multiple lines inside a `supersigil-xml` fence.
/// Returns the full content string. Line numbers inside the fence start at 1.
fn fenced_multi(lines: &str) -> String {
    format!("```supersigil-xml\n{lines}\n```")
}

// ---------------------------------------------------------------------------
// detect_context — ref-accepting attribute, before '#' (req-3-1)
// ---------------------------------------------------------------------------

#[test]
#[verifies("lsp-server/req#req-3-1")]
fn detect_refs_attribute_doc_id_prefix() {
    // `refs="auth-` — cursor after `auth-`
    let (content, line) = fenced(r#"<References refs="auth-"#);
    let ctx = detect_context(&content, line, 24);
    assert_eq!(
        ctx,
        CompletionContext::RefDocId {
            prefix: "auth-".to_owned()
        }
    );
}

#[test]
#[verifies("lsp-server/req#req-3-1")]
fn detect_implements_attribute_doc_id_prefix() {
    let (content, line) = fenced(r#"<Task id="t1" implements="design/"#);
    let ctx = detect_context(&content, line, 33);
    assert_eq!(
        ctx,
        CompletionContext::RefDocId {
            prefix: "design/".to_owned()
        }
    );
}

#[test]
#[verifies("lsp-server/req#req-3-1")]
fn detect_depends_attribute_empty_prefix() {
    let (content, line) = fenced(r#"<Task id="t1" depends=""#);
    let ctx = detect_context(&content, line, 23);
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
#[verifies("lsp-server/req#req-3-2")]
fn detect_refs_attribute_fragment_prefix() {
    let (content, line) = fenced(r#"<References refs="auth/req#req-"#);
    let ctx = detect_context(&content, line, 33);
    assert_eq!(
        ctx,
        CompletionContext::RefFragment {
            doc_id: "auth/req".to_owned(),
            prefix: "req-".to_owned()
        }
    );
}

#[test]
#[verifies("lsp-server/req#req-3-2")]
fn detect_refs_attribute_fragment_empty_prefix() {
    let (content, line) = fenced(r#"<References refs="auth/req#"#);
    let ctx = detect_context(&content, line, 27);
    assert_eq!(
        ctx,
        CompletionContext::RefFragment {
            doc_id: "auth/req".to_owned(),
            prefix: String::new()
        }
    );
}

#[test]
#[verifies("lsp-server/req#req-3-2")]
fn detect_second_ref_in_comma_list() {
    let (content, line) = fenced(r#"<References refs="a/req, auth/req#req-"#);
    let ctx = detect_context(&content, line, 38);
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
#[verifies("lsp-server/req#req-3-3")]
fn detect_component_name_prefix() {
    let (content, line) = fenced("<Cri");
    let ctx = detect_context(&content, line, 4);
    assert_eq!(
        ctx,
        CompletionContext::ComponentName {
            prefix: "Cri".to_owned()
        }
    );
}

#[test]
#[verifies("lsp-server/req#req-3-3")]
fn detect_component_name_empty_prefix() {
    let (content, line) = fenced("<");
    let ctx = detect_context(&content, line, 1);
    assert_eq!(
        ctx,
        CompletionContext::ComponentName {
            prefix: String::new()
        }
    );
}

#[test]
fn detect_closing_tag_returns_none() {
    let (content, line) = fenced("</Criterion>");
    let ctx = detect_context(&content, line, 3);
    // After `</` — this is a closing tag, not a completion context.
    assert_eq!(ctx, CompletionContext::None);
}

#[test]
fn detect_component_name_with_whitespace_returns_none() {
    // Cursor is inside the attributes of an already-opened tag.
    let (content, line) = fenced(r#"<Task id=""#);
    let ctx = detect_context(&content, line, 10);
    // After `<Task ` there's whitespace, so component name detection should not fire.
    assert_eq!(ctx, CompletionContext::None);
}

// ---------------------------------------------------------------------------
// detect_context — strategy / status attributes (req-3-4)
// ---------------------------------------------------------------------------

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn detect_strategy_attribute_on_verified_by() {
    let (content, line) = fenced(r#"<VerifiedBy strategy="ta"#);
    let ctx = detect_context(&content, line, 24);
    assert_eq!(
        ctx,
        CompletionContext::AttributeStrategy {
            prefix: "ta".to_owned(),
            component: Some("VerifiedBy".to_owned()),
        }
    );
}

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn detect_status_attribute_on_task() {
    let (content, line) = fenced(r#"<Task id="t1" status="dra"#);
    let ctx = detect_context(&content, line, 25);
    assert_eq!(
        ctx,
        CompletionContext::AttributeStatus {
            prefix: "dra".to_owned(),
            context: StatusContext::Task,
        }
    );
}

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn detect_status_attribute_on_alternative() {
    let content = fenced_multi("<Decision id=\"d1\">\n  <Alternative id=\"a1\" status=\"rej");
    // Line 1 = <Decision ...>, Line 2 = <Alternative ...>
    let ctx = detect_context(&content, 2, 51);
    assert_eq!(
        ctx,
        CompletionContext::AttributeStatus {
            prefix: "rej".to_owned(),
            context: StatusContext::Alternative,
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
// detect_context — plain Markdown should NOT trigger completions
// ---------------------------------------------------------------------------

#[test]
#[verifies("lsp-server/req#req-7-3")]
fn detect_angle_bracket_in_plain_markdown_returns_none() {
    // A `<` in regular prose (not in a fence) should not trigger completions.
    let content = "# Title\n\nSome text with <Criterion in it";
    let ctx = detect_context(content, 2, 16);
    assert_eq!(ctx, CompletionContext::None);
}

#[test]
#[verifies("lsp-server/req#req-7-3")]
fn detect_refs_outside_fence_returns_none() {
    // ref-like pattern in plain Markdown should not trigger completions.
    let content = "plain text refs=\"auth-";
    let ctx = detect_context(content, 0, 21);
    assert_eq!(ctx, CompletionContext::None);
}

// ---------------------------------------------------------------------------
// complete_document_ids (req-3-1)
// ---------------------------------------------------------------------------

#[test]
#[verifies("lsp-server/req#req-3-1")]
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
#[verifies("lsp-server/req#req-3-1")]
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
#[verifies("lsp-server/req#req-3-1")]
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
#[verifies("lsp-server/req#req-3-2")]
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
#[verifies("lsp-server/req#req-3-2")]
fn complete_fragment_ids_wrong_doc_returns_empty() {
    let doc = make_doc("auth/req", None, vec![make_criterion("req-1-1", "body", 5)]);
    let graph = build_graph(vec![doc], &default_config()).expect("graph must build");

    let items = complete_fragment_ids("billing/req", "req-", &graph);
    assert!(items.is_empty());
}

#[test]
#[verifies("lsp-server/req#req-3-2")]
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
    assert!(
        detail.ends_with('\u{2026}'),
        "detail should end with ellipsis"
    );
}

// ---------------------------------------------------------------------------
// complete_component_names (req-3-3)
// ---------------------------------------------------------------------------

#[test]
#[verifies("lsp-server/req#req-3-3")]
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
#[verifies("lsp-server/req#req-3-3")]
fn complete_component_names_empty_prefix_returns_all() {
    let defs = ComponentDefs::defaults();
    let items = complete_component_names("", &defs);
    assert_eq!(items.len(), defs.len());
}

#[test]
#[verifies("lsp-server/req#req-3-3")]
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
#[verifies("lsp-server/req#req-3-3")]
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
#[verifies("lsp-server/req#req-3-3")]
fn complete_component_names_no_match_returns_empty() {
    let defs = ComponentDefs::defaults();
    let items = complete_component_names("Zzz", &defs);
    assert!(items.is_empty());
}

// ---------------------------------------------------------------------------
// Context-sensitive attribute completions (req-3-4)
// ---------------------------------------------------------------------------

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn complete_strategy_on_verified_by() {
    let items = complete_strategy("", Some("VerifiedBy"));
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"tag"));
    assert!(labels.contains(&"file-glob"));

    for item in &items {
        assert_eq!(item.kind, Some(CompletionItemKind::ENUM_MEMBER));
    }
}

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn complete_strategy_with_prefix() {
    let items = complete_strategy("ta", Some("VerifiedBy"));
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"tag"), "should match 'tag'");
    assert!(
        !labels.contains(&"file-glob"),
        "should not match 'file-glob'"
    );
}

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn complete_strategy_on_other_component_returns_empty() {
    let items = complete_strategy("", Some("Task"));
    assert!(items.is_empty());
}

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn complete_task_status_values() {
    let items = complete_status("", &StatusContext::Task, None, None);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"draft"));
    assert!(labels.contains(&"ready"));
    assert!(labels.contains(&"in-progress"));
    assert!(labels.contains(&"done"));

    // Should NOT contain document-level or alternative statuses
    assert!(!labels.contains(&"approved"));
    assert!(!labels.contains(&"rejected"));

    for item in &items {
        assert_eq!(item.kind, Some(CompletionItemKind::ENUM_MEMBER));
    }
}

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn complete_task_status_with_prefix() {
    let items = complete_status("dra", &StatusContext::Task, None, None);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"draft"), "should match 'draft'");
    assert!(!labels.contains(&"done"), "should not match 'done'");
}

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn complete_alternative_status_values() {
    let items = complete_status("", &StatusContext::Alternative, None, None);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"rejected"));
    assert!(labels.contains(&"deferred"));
    assert!(labels.contains(&"superseded"));

    // Should NOT contain task or document-level statuses
    assert!(!labels.contains(&"draft"));
    assert!(!labels.contains(&"done"));
}

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn complete_frontmatter_status_from_config() {
    let mut config = Config::default();
    config.documents.types.insert(
        "requirements".into(),
        supersigil_core::DocumentTypeDef {
            status: vec![
                "draft".into(),
                "review".into(),
                "approved".into(),
                "implemented".into(),
            ],
            required_components: vec![],
            description: None,
        },
    );

    let items = complete_status(
        "",
        &StatusContext::Frontmatter,
        Some(&config),
        Some("requirements"),
    );
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"draft"));
    assert!(labels.contains(&"review"));
    assert!(labels.contains(&"approved"));
    assert!(labels.contains(&"implemented"));

    // Should NOT contain task or alternative statuses
    assert!(!labels.contains(&"done"));
    assert!(!labels.contains(&"rejected"));
}

#[test]
#[verifies("lsp-server/req#req-3-4")]
fn complete_frontmatter_status_unknown_doc_type() {
    let items = complete_status(
        "",
        &StatusContext::Frontmatter,
        Some(&Config::default()),
        Some("unknown"),
    );
    assert!(items.is_empty());
}
