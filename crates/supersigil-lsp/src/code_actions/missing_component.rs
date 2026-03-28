//! Code action provider for missing required components.

use lsp_types::{CodeAction, Diagnostic, Position, Range, TextEdit};
use supersigil_verify::RuleName;

use crate::code_actions::{ActionRequestContext, CodeActionProvider};
use crate::diagnostics::{ActionContext, DiagnosticData, DiagnosticSource};
use crate::supersigil_fence_regions;

// ---------------------------------------------------------------------------
// MissingComponentProvider
// ---------------------------------------------------------------------------

/// Offers to insert a skeleton of a missing required component at the end of
/// the last `supersigil-xml` fence in the document.
#[derive(Debug)]
pub struct MissingComponentProvider;

impl CodeActionProvider for MissingComponentProvider {
    fn handles(&self, data: &DiagnosticData) -> bool {
        matches!(
            data.source,
            DiagnosticSource::Verify(RuleName::MissingRequiredComponent)
        )
    }

    fn actions(
        &self,
        _diagnostic: &Diagnostic,
        data: &DiagnosticData,
        ctx: &ActionRequestContext,
    ) -> Vec<CodeAction> {
        let ActionContext::MissingComponent { component, .. } = &data.context else {
            return vec![];
        };
        let component_name: &str = component;

        let Some(insert_pos) = find_last_fence_close(ctx.file_content) else {
            return vec![];
        };

        let attrs_str = build_required_attrs_str(component_name, ctx);

        let skeleton = format!(
            "<{component_name}{attrs_str}>\n\
             <!-- TODO: Add {component_name} content -->\n\
             </{component_name}>\n"
        );

        let edit = ctx.single_file_edit(vec![TextEdit {
            range: Range::new(insert_pos, insert_pos),
            new_text: skeleton,
        }]);

        vec![CodeAction {
            title: format!("Add <{component_name}> skeleton"),
            edit: Some(edit),
            ..Default::default()
        }]
    }
}

/// Build a string of required attributes for the component opening tag.
///
/// For example, if `Criterion` requires `id`, this returns ` id=""`.
/// Returns an empty string if the component is unknown or has no required attrs.
fn build_required_attrs_str(component_name: &str, ctx: &ActionRequestContext) -> String {
    let Some(def) = ctx.component_defs.get(component_name) else {
        return String::new();
    };

    let mut required: Vec<&str> = def
        .attributes
        .iter()
        .filter(|(_, attr)| attr.required)
        .map(|(name, _)| name.as_str())
        .collect();
    required.sort_unstable();

    if required.is_empty() {
        return String::new();
    }

    let mut s = String::new();
    for attr in required {
        use std::fmt::Write;
        let _ = write!(s, " {attr}=\"\"");
    }
    s
}

/// Find the insertion position just before the closing ` ``` ` of the last
/// `supersigil-xml` fence block in the document.
///
/// Returns a position at the beginning of the closing fence line (column 0).
fn find_last_fence_close(content: &str) -> Option<Position> {
    let regions = supersigil_fence_regions(content);
    #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
    regions
        .last()
        .map(|r| Position::new(r.close_line as u32, 0))
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

    use super::{MissingComponentProvider, find_last_fence_close};

    // -- Test helpers -------------------------------------------------------

    fn make_diagnostic(line: u32, start_col: u32, end_col: u32, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, start_col), Position::new(line, end_col)),
            message: message.into(),
            ..Default::default()
        }
    }

    fn make_data(component: &str) -> DiagnosticData {
        DiagnosticData {
            source: DiagnosticSource::Verify(RuleName::MissingRequiredComponent),
            doc_id: Some("auth/req".into()),
            context: ActionContext::MissingComponent {
                component: component.to_string(),
                parent_id: "auth/req".to_string(),
            },
        }
    }

    // -- handles() ----------------------------------------------------------

    #[test]
    fn handles_missing_required_component() {
        let provider = MissingComponentProvider;
        let data = make_data("AcceptanceCriteria");
        assert!(provider.handles(&data));
    }

    #[test]
    fn rejects_other_verify_rules() {
        let provider = MissingComponentProvider;
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

        let provider = MissingComponentProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    // -- find_last_fence_close() --------------------------------------------

    #[test]
    fn finds_single_fence_close() {
        let content = "---\nid: doc\n---\n```supersigil-xml\n<Task id=\"t1\" />\n```\n";
        let pos = find_last_fence_close(content).unwrap();
        assert_eq!(pos, Position::new(5, 0));
    }

    #[test]
    fn finds_last_of_multiple_fences() {
        let content = "\
---
id: doc
---
```supersigil-xml
<Task id=\"t1\" />
```

Some text.

```supersigil-xml
<Criterion id=\"c1\" />
```
";
        let pos = find_last_fence_close(content).unwrap();
        assert_eq!(pos, Position::new(11, 0));
    }

    #[test]
    fn returns_none_when_no_fence() {
        let content = "# Just a heading\nNo fences here.\n";
        assert!(find_last_fence_close(content).is_none());
    }

    // -- actions() ----------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-4-5")]
    #[test]
    fn insert_skeleton_basic() {
        let provider = MissingComponentProvider;
        let content = "\
---
id: auth/req
type: requirements
---
```supersigil-xml
<Task id=\"t1\" status=\"draft\" />
```
";
        let diag = make_diagnostic(
            0,
            0,
            0,
            "document `auth/req` (type `requirements`) is missing required component `AcceptanceCriteria`",
        );
        let data = make_data("AcceptanceCriteria");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <AcceptanceCriteria> skeleton
          edit: file:///tmp/project/spec.md
            @6:0 insert `<AcceptanceCriteria>\n<!-- TODO: Add AcceptanceCriteria content -->\n</AcceptanceCriteria>\n`
        "#);
    }

    #[test]
    fn insert_skeleton_multiple_fences() {
        let provider = MissingComponentProvider;
        let content = "\
---
id: auth/req
type: requirements
---
```supersigil-xml
<Task id=\"t1\" status=\"draft\" />
```

Some prose here.

```supersigil-xml
<Criterion id=\"c1\" />
```
";
        let diag = make_diagnostic(
            0,
            0,
            0,
            "document `auth/req` (type `requirements`) is missing required component `AcceptanceCriteria`",
        );
        let data = make_data("AcceptanceCriteria");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <AcceptanceCriteria> skeleton
          edit: file:///tmp/project/spec.md
            @12:0 insert `<AcceptanceCriteria>\n<!-- TODO: Add AcceptanceCriteria content -->\n</AcceptanceCriteria>\n`
        "#);
    }

    #[test]
    fn insert_skeleton_criterion() {
        let provider = MissingComponentProvider;
        let content = "\
---
id: auth/req
type: requirements
---
```supersigil-xml
<Task id=\"t1\" status=\"draft\" />
```
";
        let diag = make_diagnostic(
            0,
            0,
            0,
            "document `auth/req` (type `requirements`) is missing required component `Criterion`",
        );
        let data = make_data("Criterion");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <Criterion> skeleton
          edit: file:///tmp/project/spec.md
            @6:0 insert `<Criterion id="">\n<!-- TODO: Add Criterion content -->\n</Criterion>\n`
        "#);
    }

    #[test]
    fn no_action_when_no_fence() {
        let provider = MissingComponentProvider;
        let content = "\
---
id: auth/req
type: requirements
---
# Some content
";
        let diag = make_diagnostic(
            0,
            0,
            0,
            "document `auth/req` (type `requirements`) is missing required component `AcceptanceCriteria`",
        );
        let data = make_data("AcceptanceCriteria");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn no_action_when_context_is_none() {
        let provider = MissingComponentProvider;
        let content = "\
---
id: auth/req
---
```supersigil-xml
<Task id=\"t1\" />
```
";
        let diag = make_diagnostic(0, 0, 0, "some unrecognized message format");
        let data = DiagnosticData {
            source: DiagnosticSource::Verify(RuleName::MissingRequiredComponent),
            doc_id: Some("auth/req".into()),
            context: ActionContext::None,
        };

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }
}
