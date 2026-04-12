//! Code action provider for missing required attributes.

use lsp_types::{CodeAction, Diagnostic, Position, Range, TextEdit};
use supersigil_core::{CRITERION, TASK, VERIFIED_BY};

use crate::code_actions::sequential_id::parse_sequential_id;
use crate::code_actions::{ActionRequestContext, CodeActionProvider};
use crate::diagnostics::{ActionContext, DiagnosticData, DiagnosticSource, ParseDiagnosticKind};
use crate::position::utf16_col;

// ---------------------------------------------------------------------------
// MissingAttributeProvider
// ---------------------------------------------------------------------------

/// Offers to insert a missing required attribute with a context-aware default
/// value at the component's opening tag.
#[derive(Debug)]
pub struct MissingAttributeProvider;

impl CodeActionProvider for MissingAttributeProvider {
    fn handles(&self, data: &DiagnosticData) -> bool {
        matches!(
            data.source,
            DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute)
        )
    }

    fn actions(
        &self,
        diagnostic: &Diagnostic,
        data: &DiagnosticData,
        ctx: &ActionRequestContext,
    ) -> Vec<CodeAction> {
        let ActionContext::MissingAttribute {
            component,
            attribute,
        } = &data.context
        else {
            return vec![];
        };

        let Some(insert_pos) = find_insertion_point(ctx.file_content, &diagnostic.range) else {
            return vec![];
        };

        let default_value = default_for_attribute(attribute, component, diagnostic, ctx);
        let new_text = format!(" {attribute}=\"{default_value}\"");

        let edit = ctx.single_file_edit(vec![TextEdit {
            range: Range::new(insert_pos, insert_pos),
            new_text,
        }]);

        vec![CodeAction {
            title: format!("Add missing '{attribute}' attribute to <{component}>"),
            edit: Some(edit),
            ..Default::default()
        }]
    }
}

/// Determine a context-aware default value for the given attribute.
fn default_for_attribute(
    attribute: &str,
    component: &str,
    diagnostic: &Diagnostic,
    ctx: &ActionRequestContext,
) -> String {
    match (attribute, component) {
        ("status", TASK) => "draft".to_owned(),
        ("strategy", VERIFIED_BY) => "tag".to_owned(),
        ("tag", VERIFIED_BY) => default_verified_by_tag(diagnostic, ctx).unwrap_or_default(),
        ("id", _) => next_sequential_id(component, ctx).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Derive a default tag for a `<VerifiedBy>` by combining the document ID
/// with the enclosing Criterion's ID (e.g., `my-feature/req#req-1-1`).
fn default_verified_by_tag(diagnostic: &Diagnostic, ctx: &ActionRequestContext) -> Option<String> {
    let doc = ctx.current_doc()?;
    let doc_id = &doc.frontmatter.id;

    let criterion_tag = format!("<{CRITERION}");
    let diag_line = diagnostic.range.start.line as usize;
    let lines: Vec<&str> = ctx.file_content.lines().take(diag_line + 1).collect();

    for line_text in lines.into_iter().rev() {
        let trimmed = line_text.trim();
        let Some(rest) = trimmed.strip_prefix(criterion_tag.as_str()) else {
            continue;
        };
        let Some(id_start) = rest.find("id=\"") else {
            continue;
        };
        let after_quote = &rest[id_start + 4..];
        let Some(id_end) = after_quote.find('"') else {
            continue;
        };
        let criterion_id = &after_quote[..id_end];
        if criterion_id.is_empty() {
            continue;
        }
        return Some(format!("{doc_id}#{criterion_id}"));
    }

    None
}

/// Find the next sequential ID for a component type by scanning existing
/// siblings in the parsed document. Walks the component tree recursively
/// to find siblings at any nesting level.
fn next_sequential_id(component: &str, ctx: &ActionRequestContext) -> Option<String> {
    let doc = ctx.current_doc()?;

    let mut best_prefix: Option<String> = None;
    let mut max_num: usize = 0;

    collect_ids_recursive(&doc.components, component, &mut best_prefix, &mut max_num);

    best_prefix.map(|prefix| format!("{prefix}-{}", max_num + 1))
}

/// Recursively walk the component tree collecting sequential IDs from
/// components matching the given name.
fn collect_ids_recursive(
    components: &[supersigil_core::ExtractedComponent],
    target_name: &str,
    best_prefix: &mut Option<String>,
    max_num: &mut usize,
) {
    for comp in components {
        if comp.name == target_name
            && let Some(id) = comp.attributes.get("id")
            && let Some((prefix, n)) = parse_sequential_id(id)
            && (best_prefix.is_none() || best_prefix.as_deref() == Some(&prefix))
        {
            *best_prefix = Some(prefix);
            *max_num = (*max_num).max(n);
        }
        collect_ids_recursive(&comp.children, target_name, best_prefix, max_num);
    }
}

/// Find the position just before `>` or `/>` that closes the opening tag on
/// the line indicated by the diagnostic range.
///
/// Scans forward from the diagnostic's start position through the file content.
/// Returns a zero-width insertion point right before the tag-close sequence,
/// including any trailing whitespace that precedes the close.
fn find_insertion_point(content: &str, diag_range: &Range) -> Option<Position> {
    let start_line = diag_range.start.line as usize;

    // Iterate lines starting from the diagnostic line.
    for (offset, line_text) in content.lines().enumerate().skip(start_line) {
        #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
        let line_idx = offset as u32;

        // Scan for `/>` or `>` in this line.
        let bytes = line_text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let close_pos = if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                // Self-closing `/>`.
                Some(i)
            } else if bytes[i] == b'>' {
                // Normal close `>`.
                Some(i)
            } else {
                None
            };

            if let Some(pos) = close_pos {
                // Walk backwards over any whitespace before the close sequence.
                let insert_byte = {
                    let mut p = pos;
                    while p > 0 && bytes[p - 1] == b' ' {
                        p -= 1;
                    }
                    p
                };
                let col = utf16_col(line_text, insert_byte);
                return Some(Position::new(line_idx, col));
            }
            i += 1;
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use lsp_types::{Diagnostic, Position, Range};
    use supersigil_rust_macros::verifies;

    use crate::code_actions::CodeActionProvider;
    use crate::code_actions::test_helpers::{TestContext, format_actions};
    use crate::diagnostics::{
        ActionContext, DiagnosticData, DiagnosticSource, ParseDiagnosticKind,
    };

    use super::{MissingAttributeProvider, find_insertion_point};

    // -- Test helpers -------------------------------------------------------

    fn make_diagnostic(line: u32, start_col: u32, end_col: u32) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, start_col), Position::new(line, end_col)),
            message: "missing required attribute".into(),
            ..Default::default()
        }
    }

    fn make_data(component: &str, attribute: &str) -> DiagnosticData {
        DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute),
            doc_id: None,
            context: ActionContext::MissingAttribute {
                component: component.to_string(),
                attribute: attribute.to_string(),
            },
        }
    }

    // -- handles() ----------------------------------------------------------

    #[test]
    fn handles_missing_required_attribute() {
        let provider = MissingAttributeProvider;
        let data = make_data("Task", "id");
        assert!(provider.handles(&data));
    }

    #[test]
    fn rejects_other_parse_errors() {
        let provider = MissingAttributeProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    // -- actions() ----------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-4-2")]
    #[test]
    fn insert_attribute_self_closing_tag() {
        let provider = MissingAttributeProvider;
        let content = "# Title\n```supersigil-xml\n<Task status=\"draft\" />\n```\n";
        let diag = make_diagnostic(2, 0, 5);
        let data = make_data("Task", "id");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add missing 'id' attribute to <Task>
          edit: file:///tmp/project/spec.md
            @2:20 insert ` id=""`
        "#);
    }

    #[test]
    fn insert_attribute_normal_close_tag() {
        let provider = MissingAttributeProvider;
        let content = "```supersigil-xml\n<Task status=\"draft\">\nbody\n</Task>\n```\n";
        let diag = make_diagnostic(1, 0, 5);
        let data = make_data("Task", "id");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add missing 'id' attribute to <Task>
          edit: file:///tmp/project/spec.md
            @1:20 insert ` id=""`
        "#);
    }

    #[test]
    fn insert_attribute_different_attr_name() {
        let provider = MissingAttributeProvider;
        let content = "```supersigil-xml\n<Criterion id=\"c1\">\nbody\n</Criterion>\n```\n";
        let diag = make_diagnostic(1, 0, 10);
        let data = make_data("Criterion", "status");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add missing 'status' attribute to <Criterion>
          edit: file:///tmp/project/spec.md
            @1:18 insert ` status=""`
        "#);
    }

    #[test]
    fn no_action_when_context_is_none() {
        let provider = MissingAttributeProvider;
        let content = "<Task />\n";
        let diag = make_diagnostic(0, 0, 5);
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute),
            doc_id: None,
            context: ActionContext::None,
        };

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn insert_attribute_tag_with_no_attributes() {
        let provider = MissingAttributeProvider;
        let content = "```supersigil-xml\n<Task />\n```\n";
        let diag = make_diagnostic(1, 0, 5);
        let data = make_data("Task", "id");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add missing 'id' attribute to <Task>
          edit: file:///tmp/project/spec.md
            @1:5 insert ` id=""`
        "#);
    }

    // -- find_insertion_point() ---------------------------------------------

    #[test]
    fn insertion_before_self_closing() {
        let content = "<Task status=\"draft\" />";
        let range = Range::new(Position::new(0, 0), Position::new(0, 5));
        let pos = find_insertion_point(content, &range).unwrap();
        assert_eq!(pos, Position::new(0, 20));
    }

    #[test]
    fn insertion_before_closing_angle() {
        let content = "<Task status=\"draft\">";
        let range = Range::new(Position::new(0, 0), Position::new(0, 5));
        let pos = find_insertion_point(content, &range).unwrap();
        assert_eq!(pos, Position::new(0, 20));
    }

    #[test]
    fn insertion_minimal_tag() {
        let content = "<Task />";
        let range = Range::new(Position::new(0, 0), Position::new(0, 5));
        let pos = find_insertion_point(content, &range).unwrap();
        assert_eq!(pos, Position::new(0, 5));
    }

    // -- context-aware defaults (req-4-8) -----------------------------------

    #[verifies("lsp-code-actions/req#req-4-8")]
    #[test]
    fn status_on_task_defaults_to_draft() {
        let provider = MissingAttributeProvider;
        let content = "```supersigil-xml\n<Task id=\"t1\" />\n```\n";
        let diag = make_diagnostic(1, 0, 5);
        let data = make_data("Task", "status");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add missing 'status' attribute to <Task>
          edit: file:///tmp/project/spec.md
            @1:13 insert ` status="draft"`
        "#);
    }

    #[verifies("lsp-code-actions/req#req-4-8")]
    #[test]
    fn strategy_on_verified_by_defaults_to_tag() {
        let provider = MissingAttributeProvider;
        let content = "```supersigil-xml\n<VerifiedBy tag=\"foo\" />\n```\n";
        let diag = make_diagnostic(1, 0, 11);
        let data = make_data("VerifiedBy", "strategy");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add missing 'strategy' attribute to <VerifiedBy>
          edit: file:///tmp/project/spec.md
            @1:21 insert ` strategy="tag"`
        "#);
    }

    #[verifies("lsp-code-actions/req#req-4-8")]
    #[test]
    fn id_on_criterion_generates_sequential_id() {
        use std::path::PathBuf;
        use supersigil_core::test_helpers::make_criterion;
        use supersigil_core::{Frontmatter, SpecDocument};

        let provider = MissingAttributeProvider;
        let content = "```supersigil-xml\n<AcceptanceCriteria>\n  <Criterion id=\"req-1\">\n    First criterion.\n  </Criterion>\n  <Criterion>\n    Needs an id.\n  </Criterion>\n</AcceptanceCriteria>\n```\n";
        let diag = make_diagnostic(5, 2, 12);
        let data = make_data("Criterion", "id");

        let mut tc = TestContext::new();
        // Use partial_file_parses to exercise the production path where
        // the document has a MissingRequiredAttribute parse error.
        tc.partial_file_parses.insert(
            PathBuf::from("spec.md"),
            SpecDocument {
                path: PathBuf::from("spec.md"),
                frontmatter: Frontmatter {
                    id: "test/req".into(),
                    doc_type: Some("requirements".into()),
                    status: None,
                },
                extra: std::collections::HashMap::new(),
                components: vec![make_criterion("req-1", 2)],
            },
        );
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert_eq!(actions.len(), 1);
        let edit_text = actions[0]
            .edit
            .as_ref()
            .unwrap()
            .changes
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap()[0]
            .new_text
            .as_str();
        assert!(
            edit_text.contains("\"req-2\""),
            "expected req-2, got: {edit_text}"
        );
    }

    #[verifies("lsp-code-actions/req#req-4-8")]
    #[test]
    fn tag_on_verified_by_derives_from_criterion() {
        use std::path::PathBuf;
        use supersigil_core::{Frontmatter, SpecDocument};

        let provider = MissingAttributeProvider;
        let content = "```supersigil-xml\n<Criterion id=\"req-1\">\n  Body text.\n  <VerifiedBy strategy=\"tag\" />\n</Criterion>\n```\n";
        let diag = make_diagnostic(3, 2, 12);
        let data = make_data("VerifiedBy", "tag");

        let mut tc = TestContext::new();
        // Use partial_file_parses to exercise the production path.
        tc.partial_file_parses.insert(
            PathBuf::from("spec.md"),
            SpecDocument {
                path: PathBuf::from("spec.md"),
                frontmatter: Frontmatter {
                    id: "auth/req".into(),
                    doc_type: Some("requirements".into()),
                    status: None,
                },
                extra: std::collections::HashMap::new(),
                components: vec![],
            },
        );
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert_eq!(actions.len(), 1);
        let edit_text = actions[0]
            .edit
            .as_ref()
            .unwrap()
            .changes
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap()[0]
            .new_text
            .as_str();
        assert!(
            edit_text.contains("\"auth/req#req-1\""),
            "expected auth/req#req-1, got: {edit_text}"
        );
    }
}
