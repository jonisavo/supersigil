//! Code action provider for missing required attributes.

use lsp_types::{CodeAction, Diagnostic, Position, Range, TextEdit};

use crate::code_actions::{ActionRequestContext, CodeActionProvider};
use crate::diagnostics::{ActionContext, DiagnosticData, DiagnosticSource, ParseDiagnosticKind};
use crate::position::utf16_col;

// ---------------------------------------------------------------------------
// MissingAttributeProvider
// ---------------------------------------------------------------------------

/// Offers to insert a missing required attribute with a placeholder value at the
/// component's opening tag.
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

        let new_text = format!(" {attribute}=\"\"");

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
}
