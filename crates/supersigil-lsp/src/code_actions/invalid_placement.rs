//! Code action provider for misplaced components (Rationale, Alternative).

use lsp_types::{CodeAction, Diagnostic, Position, Range, TextEdit};
use supersigil_verify::RuleName;

use crate::code_actions::{ActionRequestContext, CodeActionProvider};
use crate::diagnostics::{DiagnosticData, DiagnosticSource};

// ---------------------------------------------------------------------------
// InvalidPlacementProvider
// ---------------------------------------------------------------------------

/// Offers to wrap a misplaced component in its correct parent element.
///
/// - `Rationale` / `Alternative` → wrap in `<Decision id="">`
#[derive(Debug)]
pub struct InvalidPlacementProvider;

impl CodeActionProvider for InvalidPlacementProvider {
    fn handles(&self, data: &DiagnosticData) -> bool {
        matches!(
            data.source,
            DiagnosticSource::Verify(
                RuleName::InvalidRationalePlacement | RuleName::InvalidAlternativePlacement
            )
        )
    }

    fn actions(
        &self,
        diagnostic: &Diagnostic,
        data: &DiagnosticData,
        ctx: &ActionRequestContext,
    ) -> Vec<CodeAction> {
        let DiagnosticSource::Verify(rule) = &data.source else {
            return vec![];
        };

        let (child_name, parent_tag_open, parent_tag_close) = match rule {
            RuleName::InvalidRationalePlacement => {
                ("Rationale", "<Decision id=\"\">", "</Decision>")
            }
            RuleName::InvalidAlternativePlacement => {
                ("Alternative", "<Decision id=\"\">", "</Decision>")
            }
            _ => return vec![],
        };

        let Some((component_start_line, component_end_line, indent)) =
            find_component_extent(ctx.file_content, &diagnostic.range, child_name)
        else {
            return vec![];
        };

        let parent_name = match rule {
            RuleName::InvalidRationalePlacement | RuleName::InvalidAlternativePlacement => {
                "Decision"
            }
            _ => return vec![],
        };

        // Insert the opening parent tag before the component and the closing
        // parent tag after it.  We use two TextEdits in a single change set.
        #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
        let open_pos = Position::new(component_start_line as u32, 0);
        #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
        let close_pos = Position::new((component_end_line + 1) as u32, 0);

        let open_text = format!("{indent}{parent_tag_open}\n");
        let close_text = format!("{indent}{parent_tag_close}\n");

        let edit = ctx.single_file_edit(vec![
            TextEdit {
                range: Range::new(open_pos, open_pos),
                new_text: open_text,
            },
            TextEdit {
                range: Range::new(close_pos, close_pos),
                new_text: close_text,
            },
        ]);

        vec![CodeAction {
            title: format!("Wrap in <{parent_name}>"),
            edit: Some(edit),
            ..Default::default()
        }]
    }
}

/// Find the extent of a component starting near the diagnostic position.
///
/// Searches from the diagnostic line for the opening `<{child_name}` tag, then
/// finds the matching `</{child_name}>` closing tag.  Returns the start line,
/// end line (inclusive), and indentation string of the opening tag.
fn find_component_extent(
    content: &str,
    diag_range: &Range,
    child_name: &str,
) -> Option<(usize, usize, String)> {
    let start_line = diag_range.start.line as usize;

    // Find the opening tag on or after the diagnostic line.
    let open_prefix = format!("<{child_name}");
    let mut open_line = None;
    let mut open_line_text = "";
    for (idx, line_text) in content.lines().enumerate().skip(start_line) {
        let trimmed = line_text.trim_start();
        if trimmed.starts_with(&open_prefix) {
            open_line = Some(idx);
            open_line_text = line_text;
            break;
        }
    }
    let open_line = open_line?;

    // Determine indentation from the opening tag line.
    let trimmed = open_line_text.trim_start();
    let indent_len = open_line_text.len() - trimmed.len();
    let indent = open_line_text[..indent_len].to_string();

    // Check for self-closing tag on the opening line.
    if trimmed.contains("/>") {
        return Some((open_line, open_line, indent));
    }

    // Find the matching closing tag.
    let close_tag = format!("</{child_name}>");
    for (idx, line_text) in content.lines().enumerate().skip(open_line + 1) {
        let trimmed = line_text.trim_start();
        if trimmed.starts_with(&close_tag) {
            return Some((open_line, idx, indent));
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
    use supersigil_verify::RuleName;

    use crate::code_actions::CodeActionProvider;
    use crate::code_actions::test_helpers::{TestContext, format_actions};
    use crate::diagnostics::{ActionContext, DiagnosticData, DiagnosticSource};

    use super::InvalidPlacementProvider;

    // -- Test helpers -------------------------------------------------------

    fn make_diagnostic(line: u32, start_col: u32, end_col: u32, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, start_col), Position::new(line, end_col)),
            message: message.into(),
            ..Default::default()
        }
    }

    fn make_data(rule: RuleName) -> DiagnosticData {
        DiagnosticData {
            source: DiagnosticSource::Verify(rule),
            doc_id: Some("adr/design".into()),
            context: ActionContext::None,
        }
    }

    // -- handles() ----------------------------------------------------------

    #[test]
    fn handles_invalid_rationale_placement() {
        let provider = InvalidPlacementProvider;
        let data = make_data(RuleName::InvalidRationalePlacement);
        assert!(provider.handles(&data));
    }

    #[test]
    fn handles_invalid_alternative_placement() {
        let provider = InvalidPlacementProvider;
        let data = make_data(RuleName::InvalidAlternativePlacement);
        assert!(provider.handles(&data));
    }

    #[test]
    fn rejects_other_verify_rules() {
        let provider = InvalidPlacementProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Verify(RuleName::IncompleteDecision),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    #[test]
    fn rejects_parse_diagnostic() {
        use crate::diagnostics::ParseDiagnosticKind;

        let provider = InvalidPlacementProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    // -- actions() ----------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-4-7")]
    #[test]
    fn wrap_rationale_in_decision() {
        let provider = InvalidPlacementProvider;
        let content = "\
---
id: adr/design
---
```supersigil-xml
<Rationale>
This is a standalone rationale.
</Rationale>
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Rationale in `adr/design` is placed at document root; \
             it must be a direct child of Decision",
        );
        let data = make_data(RuleName::InvalidRationalePlacement);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Wrap in <Decision>
          edit: file:///tmp/project/spec.md
            @4:0 insert `<Decision id="">\n`
            @7:0 insert `</Decision>\n`
        "#);
    }

    #[test]
    fn wrap_alternative_in_decision() {
        let provider = InvalidPlacementProvider;
        let content = "\
---
id: adr/design
---
```supersigil-xml
<Alternative id=\"alt-1\" status=\"rejected\">
MySQL was considered.
</Alternative>
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Alternative in `adr/design` is placed at document root; \
             it must be a direct child of Decision",
        );
        let data = make_data(RuleName::InvalidAlternativePlacement);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Wrap in <Decision>
          edit: file:///tmp/project/spec.md
            @4:0 insert `<Decision id="">\n`
            @7:0 insert `</Decision>\n`
        "#);
    }

    #[test]
    fn wrap_indented_rationale() {
        let provider = InvalidPlacementProvider;
        let content = "\
---
id: adr/design
---
```supersigil-xml
  <Rationale>
  Some rationale text.
  </Rationale>
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Rationale in `adr/design` is placed at document root; \
             it must be a direct child of Decision",
        );
        let data = make_data(RuleName::InvalidRationalePlacement);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Wrap in <Decision>
          edit: file:///tmp/project/spec.md
            @4:0 insert `  <Decision id="">\n`
            @7:0 insert `  </Decision>\n`
        "#);
    }

    #[test]
    fn no_action_when_component_not_found() {
        let provider = InvalidPlacementProvider;
        let content = "\
---
id: adr/design
---
```supersigil-xml
Some random text
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Rationale in `adr/design` is placed at document root; \
             it must be a direct child of Decision",
        );
        let data = make_data(RuleName::InvalidRationalePlacement);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn no_action_when_closing_tag_missing() {
        let provider = InvalidPlacementProvider;
        let content = "\
---
id: adr/design
---
```supersigil-xml
<Rationale>
This rationale is never closed.
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Rationale in `adr/design` is placed at document root; \
             it must be a direct child of Decision",
        );
        let data = make_data(RuleName::InvalidRationalePlacement);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }
}
