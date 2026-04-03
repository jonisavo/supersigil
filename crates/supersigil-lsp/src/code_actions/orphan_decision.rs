//! Code action provider for orphan Decision components (no References child).

use lsp_types::{CodeAction, Diagnostic, Range, TextEdit};
use supersigil_verify::RuleName;

use crate::code_actions::{ActionRequestContext, CodeActionProvider, find_closing_decision_tag};
use crate::diagnostics::{DiagnosticData, DiagnosticSource};

// ---------------------------------------------------------------------------
// OrphanDecisionProvider
// ---------------------------------------------------------------------------

/// Offers to insert a `<References refs="" />` component inside a Decision that
/// has no References linking it to a requirement.
#[derive(Debug)]
pub struct OrphanDecisionProvider;

impl CodeActionProvider for OrphanDecisionProvider {
    fn handles(&self, data: &DiagnosticData) -> bool {
        matches!(
            data.source,
            DiagnosticSource::Verify(RuleName::OrphanDecision)
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

        let refs_value = infer_refs_target(ctx);
        let new_text = format!("{indent}<References refs=\"{refs_value}\" />\n");
        let edit = ctx.single_file_edit(vec![TextEdit {
            range: insert_range,
            new_text,
        }]);

        vec![CodeAction {
            title: "Add <References> to Decision".to_string(),
            edit: Some(edit),
            ..Default::default()
        }]
    }
}

/// Try to infer a reasonable `refs` value for a new `<References>` component.
///
/// Looks at the current file's parsed document for an `Implements` or
/// top-level `References` component with a `refs` attribute. Prefers
/// `Implements` (design docs) but falls back to `References` (ADR docs).
fn infer_refs_target(ctx: &ActionRequestContext) -> String {
    let Some(rel) = ctx.file_relative_key() else {
        return String::new();
    };

    let Some(doc) = ctx.file_parses.get(&rel) else {
        return String::new();
    };

    // Prefer Implements (design docs), fall back to top-level References (ADRs).
    let mut fallback_refs: Option<&str> = None;
    for component in &doc.components {
        if component.name == "Implements"
            && let Some(refs) = component.attributes.get("refs")
        {
            return refs.clone();
        }
        if component.name == "References"
            && let Some(refs) = component.attributes.get("refs")
            && fallback_refs.is_none()
        {
            fallback_refs = Some(refs);
        }
    }

    fallback_refs.map(String::from).unwrap_or_default()
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

    use super::OrphanDecisionProvider;

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
            source: DiagnosticSource::Verify(RuleName::OrphanDecision),
            doc_id: Some("adr/design".into()),
            context: ActionContext::None,
        }
    }

    // -- handles() ----------------------------------------------------------

    #[test]
    fn handles_orphan_decision() {
        let provider = OrphanDecisionProvider;
        let data = make_data();
        assert!(provider.handles(&data));
    }

    #[test]
    fn rejects_other_verify_rules() {
        let provider = OrphanDecisionProvider;
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

        let provider = OrphanDecisionProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    // -- actions() ----------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-4-6")]
    #[test]
    fn insert_references_basic() {
        let provider = OrphanDecisionProvider;
        let content = "\
---
id: adr/design
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
            "Decision 'use-postgres' in 'adr/design' has no References to any requirement",
        );
        let data = make_data();

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <References> to Decision
          edit: file:///tmp/project/spec.md
            @5:0 insert `<References refs="" />\n`
        "#);
    }

    #[test]
    fn insert_references_indented() {
        let provider = OrphanDecisionProvider;
        let content = "\
---
id: adr/design
---
```supersigil-xml
<Decision id=\"use-postgres\" status=\"proposed\">
  <Rationale>
  Good performance.
  </Rationale>
  </Decision>
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Decision 'use-postgres' in 'adr/design' has no References to any requirement",
        );
        let data = make_data();

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <References> to Decision
          edit: file:///tmp/project/spec.md
            @8:0 insert `  <References refs="" />\n`
        "#);
    }

    #[test]
    fn no_action_when_closing_tag_not_found() {
        let provider = OrphanDecisionProvider;
        let content = "\
---
id: adr/design
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
            "Decision 'use-postgres' in 'adr/design' has no References to any requirement",
        );
        let data = make_data();

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn insert_references_with_existing_children() {
        let provider = OrphanDecisionProvider;
        let content = "\
---
id: adr/design
---
```supersigil-xml
<Decision id=\"use-postgres\" status=\"proposed\">
  <Rationale>
  We need a robust database.
  </Rationale>
  <Alternative id=\"alt-mysql\" status=\"rejected\">
  MySQL was considered.
  </Alternative>
</Decision>
```
";
        let diag = make_diagnostic(
            4,
            0,
            10,
            "Decision 'use-postgres' in 'adr/design' has no References to any requirement",
        );
        let data = make_data();

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <References> to Decision
          edit: file:///tmp/project/spec.md
            @11:0 insert `<References refs="" />\n`
        "#);
    }

    #[test]
    fn prefill_refs_from_implements_component() {
        use std::collections::HashMap;
        use std::path::{Path, PathBuf};

        use lsp_types::Url;
        use supersigil_core::{
            ComponentDefs, Config, ExtractedComponent, Frontmatter, SourcePosition, SpecDocument,
            build_graph,
        };

        use crate::code_actions::ActionRequestContext;

        let provider = OrphanDecisionProvider;
        let content = "\
---
id: auth/adr
---
```supersigil-xml
<Implements refs=\"auth/req\" />
<Decision id=\"use-postgres\" status=\"proposed\">
</Decision>
```
";
        let diag = make_diagnostic(
            5,
            0,
            10,
            "Decision 'use-postgres' in 'auth/adr' has no References to any requirement",
        );
        let data = make_data();

        let graph = build_graph(Vec::new(), &Config::default()).unwrap();
        let config = Config::default();
        let component_defs = ComponentDefs::defaults();
        let uri = Url::parse("file:///tmp/project/specs/auth/auth.adr.md").unwrap();

        // Build a file_parses map with a document that has an Implements component.
        let mut file_parses = HashMap::new();
        let doc = SpecDocument {
            path: PathBuf::from("specs/auth/auth.adr.md"),
            frontmatter: Frontmatter {
                id: "auth/adr".into(),
                doc_type: Some("adr".into()),
                status: Some("draft".into()),
            },
            extra: HashMap::new(),
            components: vec![ExtractedComponent {
                name: "Implements".into(),
                attributes: HashMap::from([("refs".into(), "auth/req".into())]),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: SourcePosition {
                    byte_offset: 0,
                    line: 4,
                    column: 1,
                },
                end_position: SourcePosition {
                    byte_offset: 0,
                    line: 4,
                    column: 1,
                },
            }],
            warnings: vec![],
        };
        file_parses.insert(PathBuf::from("specs/auth/auth.adr.md"), doc);

        let ctx = ActionRequestContext {
            graph: &graph,
            config: &config,
            component_defs: &component_defs,
            file_parses: &file_parses,
            project_root: Path::new("/tmp/project"),
            file_uri: &uri,
            file_content: content,
        };

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <References> to Decision
          edit: file:///tmp/project/specs/auth/auth.adr.md
            @6:0 insert `<References refs="auth/req" />\n`
        "#);
    }

    #[test]
    fn prefill_refs_from_top_level_references_component() {
        use std::collections::HashMap;
        use std::path::{Path, PathBuf};

        use lsp_types::Url;
        use supersigil_core::{
            ComponentDefs, Config, ExtractedComponent, Frontmatter, SourcePosition, SpecDocument,
            build_graph,
        };

        use crate::code_actions::ActionRequestContext;

        let provider = OrphanDecisionProvider;
        let content = "\
---
id: auth/adr
---
```supersigil-xml
<References refs=\"auth/req\" />
<Decision id=\"use-postgres\" status=\"proposed\">
</Decision>
```
";
        let diag = make_diagnostic(
            5,
            0,
            10,
            "Decision 'use-postgres' in 'auth/adr' has no References to any requirement",
        );
        let data = make_data();

        let graph = build_graph(Vec::new(), &Config::default()).unwrap();
        let config = Config::default();
        let component_defs = ComponentDefs::defaults();
        let uri = Url::parse("file:///tmp/project/specs/auth/auth.adr.md").unwrap();

        // Build a file_parses map with a document that has a top-level References
        // component (the standard ADR pattern, not Implements).
        let mut file_parses = HashMap::new();
        let doc = SpecDocument {
            path: PathBuf::from("specs/auth/auth.adr.md"),
            frontmatter: Frontmatter {
                id: "auth/adr".into(),
                doc_type: Some("adr".into()),
                status: Some("draft".into()),
            },
            extra: HashMap::new(),
            components: vec![ExtractedComponent {
                name: "References".into(),
                attributes: HashMap::from([("refs".into(), "auth/req".into())]),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: SourcePosition {
                    byte_offset: 0,
                    line: 4,
                    column: 1,
                },
                end_position: SourcePosition {
                    byte_offset: 0,
                    line: 4,
                    column: 1,
                },
            }],
            warnings: vec![],
        };
        file_parses.insert(PathBuf::from("specs/auth/auth.adr.md"), doc);

        let ctx = ActionRequestContext {
            graph: &graph,
            config: &config,
            component_defs: &component_defs,
            file_parses: &file_parses,
            project_root: Path::new("/tmp/project"),
            file_uri: &uri,
            file_content: content,
        };

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Add <References> to Decision
          edit: file:///tmp/project/specs/auth/auth.adr.md
            @6:0 insert `<References refs="auth/req" />\n`
        "#);
    }
}
