//! Code action provider for incomplete Decision components.

use lsp_types::{CodeAction, Diagnostic, Range, TextEdit};
use supersigil_verify::RuleName;

use crate::code_actions::{ActionRequestContext, CodeActionProvider, find_closing_decision_tag};
use crate::diagnostics::{DiagnosticData, DiagnosticSource};

// ---------------------------------------------------------------------------
// IncompleteDecisionProvider
// ---------------------------------------------------------------------------

/// Offers to insert a stub `<Rationale>` component inside a Decision that is
/// missing one.
#[derive(Debug)]
pub struct IncompleteDecisionProvider;

impl CodeActionProvider for IncompleteDecisionProvider {
    fn handles(&self, data: &DiagnosticData) -> bool {
        matches!(
            data.source,
            DiagnosticSource::Verify(RuleName::IncompleteDecision)
        )
    }

    fn actions(
        &self,
        diagnostic: &Diagnostic,
        _data: &DiagnosticData,
        ctx: &ActionRequestContext,
    ) -> Vec<CodeAction> {
        let Some((insert_pos, indent)) =
            find_closing_decision_tag(ctx.file_content, &diagnostic.range)
        else {
            return vec![];
        };

        let insert_range = Range::new(insert_pos, insert_pos);

        let rationale_stub = format!(
            "{indent}<Rationale>\n\
             {indent}TODO: Add rationale for this decision.\n\
             {indent}</Rationale>\n"
        );
        let rationale_edit = ctx.single_file_edit(vec![TextEdit {
            range: insert_range,
            new_text: rationale_stub,
        }]);

        let alternative_stub = format!(
            "{indent}<Alternative id=\"\" status=\"\">\n\
             {indent}TODO: Describe alternative.\n\
             {indent}</Alternative>\n"
        );
        let alternative_edit = ctx.single_file_edit(vec![TextEdit {
            range: insert_range,
            new_text: alternative_stub,
        }]);

        vec![
            CodeAction {
                title: "Add <Rationale> stub to Decision".to_string(),
                edit: Some(rationale_edit),
                ..Default::default()
            },
            CodeAction {
                title: "Add <Alternative> stub to Decision".to_string(),
                edit: Some(alternative_edit),
                ..Default::default()
            },
        ]
    }
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

    use super::IncompleteDecisionProvider;

    // -- Test helpers -------------------------------------------------------

    fn make_diagnostic(line: u32, start_col: u32, end_col: u32, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, start_col), Position::new(line, end_col)),
            message: message.into(),
            ..Default::default()
        }
    }

    fn make_data() -> DiagnosticData {
        DiagnosticData {
            source: DiagnosticSource::Verify(RuleName::IncompleteDecision),
            doc_id: Some("adr/logging".into()),
            context: ActionContext::None,
        }
    }

    // -- handles() ----------------------------------------------------------

    #[test]
    fn handles_incomplete_decision() {
        let provider = IncompleteDecisionProvider;
        let data = make_data();
        assert!(provider.handles(&data));
    }

    #[test]
    fn rejects_other_verify_rules() {
        let provider = IncompleteDecisionProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Verify(RuleName::OrphanDecision),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    #[test]
    fn rejects_parse_diagnostic() {
        use crate::diagnostics::ParseDiagnosticKind;

        let provider = IncompleteDecisionProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    // -- actions() ----------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-4-4")]
    #[test]
    fn insert_rationale_stub_basic() {
        let provider = IncompleteDecisionProvider;
        let content = "\
---
id: adr/logging
---
```supersigil-xml
<Decision id=\"use-postgres\" status=\"proposed\">
</Decision>
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Decision in `adr/logging` has no Rationale child; \
             every Decision should include a Rationale",
        );
        let data = make_data();

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <Rationale> stub to Decision
          edit: file:///tmp/project/spec.md
            @5:0 insert `<Rationale>\nTODO: Add rationale for this decision.\n</Rationale>\n`
        [none] Add <Alternative> stub to Decision
          edit: file:///tmp/project/spec.md
            @5:0 insert `<Alternative id="" status="">\nTODO: Describe alternative.\n</Alternative>\n`
        "#);
    }

    #[test]
    fn insert_rationale_stub_indented() {
        let provider = IncompleteDecisionProvider;
        let content = "\
---
id: adr/logging
---
```supersigil-xml
<Decision id=\"use-postgres\" status=\"proposed\">
  <Alternative id=\"alt-1\" status=\"rejected\" />
  </Decision>
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Decision in `adr/logging` has no Rationale child; \
             every Decision should include a Rationale",
        );
        let data = make_data();

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <Rationale> stub to Decision
          edit: file:///tmp/project/spec.md
            @6:0 insert `  <Rationale>\n  TODO: Add rationale for this decision.\n  </Rationale>\n`
        [none] Add <Alternative> stub to Decision
          edit: file:///tmp/project/spec.md
            @6:0 insert `  <Alternative id="" status="">\n  TODO: Describe alternative.\n  </Alternative>\n`
        "#);
    }

    #[test]
    fn no_action_when_closing_tag_not_found() {
        let provider = IncompleteDecisionProvider;
        // Content without </Decision> closing tag.
        let content = "\
---
id: adr/logging
---
```supersigil-xml
<Decision id=\"use-postgres\" status=\"proposed\">
Some body text
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Decision in `adr/logging` has no Rationale child; \
             every Decision should include a Rationale",
        );
        let data = make_data();

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn insert_rationale_stub_with_existing_children() {
        let provider = IncompleteDecisionProvider;
        let content = "\
---
id: adr/logging
---
```supersigil-xml
<Decision id=\"use-postgres\" status=\"proposed\">
  <Alternative id=\"alt-mysql\" status=\"rejected\">
  MySQL was considered but rejected.
  </Alternative>
  <Alternative id=\"alt-sqlite\" status=\"rejected\">
  SQLite was considered but rejected.
  </Alternative>
</Decision>
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Decision in `adr/logging` has no Rationale child; \
             every Decision should include a Rationale",
        );
        let data = make_data();

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <Rationale> stub to Decision
          edit: file:///tmp/project/spec.md
            @11:0 insert `<Rationale>\nTODO: Add rationale for this decision.\n</Rationale>\n`
        [none] Add <Alternative> stub to Decision
          edit: file:///tmp/project/spec.md
            @11:0 insert `<Alternative id="" status="">\nTODO: Describe alternative.\n</Alternative>\n`
        "#);
    }
}
