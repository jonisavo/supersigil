//! Code action provider trait and request context.

mod broken_ref;
mod duplicate_id;
mod incomplete_decision;
mod invalid_placement;
mod missing_attribute;
mod missing_component;
mod orphan_decision;
mod sequential_id;

pub use broken_ref::BrokenRefProvider;
pub use duplicate_id::DuplicateIdProvider;
pub use incomplete_decision::IncompleteDecisionProvider;
pub use invalid_placement::InvalidPlacementProvider;
pub use missing_attribute::MissingAttributeProvider;
pub use missing_component::MissingComponentProvider;
pub use orphan_decision::OrphanDecisionProvider;
pub use sequential_id::SequentialIdProvider;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lsp_types::{CodeAction, Diagnostic, Url};
use supersigil_core::{ComponentDefs, Config, DocumentGraph, SpecDocument};

use crate::diagnostics::DiagnosticData;

// ---------------------------------------------------------------------------
// Shared utilities
// ---------------------------------------------------------------------------

/// Search forward from the diagnostic line for a `</Decision>` closing tag.
///
/// Returns the insertion position (beginning of the closing-tag line) and the
/// indentation string to use for inserted content.
pub(crate) fn find_closing_decision_tag(
    content: &str,
    diag_range: &lsp_types::Range,
) -> Option<(lsp_types::Position, String)> {
    let start_line = diag_range.start.line as usize;

    for (line_idx, line_text) in content.lines().enumerate().skip(start_line) {
        let trimmed = line_text.trim_start();
        if trimmed.starts_with("</Decision>") {
            let indent_len = line_text.len() - trimmed.len();
            let indent: String = line_text[..indent_len].to_string();

            // Insert just before this line — position is at the start of this line.
            #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
            let pos = lsp_types::Position::new(line_idx as u32, 0);
            return Some((pos, indent));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// ActionRequestContext
// ---------------------------------------------------------------------------

/// All context a [`CodeActionProvider`] needs to produce code actions.
///
/// Providers are stateless; every piece of information they require is passed
/// through this struct, which borrows server state for the duration of a single
/// `textDocument/codeAction` request.
#[derive(Debug)]
pub struct ActionRequestContext<'a> {
    /// The resolved document graph.
    pub graph: &'a DocumentGraph,
    /// The loaded supersigil configuration.
    pub config: &'a Config,
    /// Component definitions (built-in and custom).
    pub component_defs: &'a ComponentDefs,
    /// Per-file parse results keyed by project-relative path.
    pub file_parses: &'a HashMap<PathBuf, SpecDocument>,
    /// Absolute path to the project root directory.
    pub project_root: &'a Path,
    /// URI of the file the code action request targets.
    pub file_uri: &'a Url,
    /// Full text content of the target file.
    pub file_content: &'a str,
}

impl ActionRequestContext<'_> {
    /// Convert the file URI to a relative path key matching `file_parses` keys.
    #[must_use]
    pub fn file_relative_key(&self) -> Option<PathBuf> {
        let abs = self.file_uri.to_file_path().ok()?;
        Some(
            abs.strip_prefix(self.project_root)
                .map(Path::to_path_buf)
                .unwrap_or(abs),
        )
    }

    /// Build a `WorkspaceEdit` with text edits on the current file.
    #[must_use]
    pub fn single_file_edit(&self, edits: Vec<lsp_types::TextEdit>) -> lsp_types::WorkspaceEdit {
        lsp_types::WorkspaceEdit {
            changes: Some(HashMap::from([(self.file_uri.clone(), edits)])),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// CodeActionProvider
// ---------------------------------------------------------------------------

/// A stateless provider that can inspect a diagnostic and optionally produce
/// one or more [`CodeAction`]s.
///
/// Implementations must be `Send + Sync` because the LSP server is async.
pub trait CodeActionProvider: Send + Sync {
    /// Returns `true` if this provider can handle the given diagnostic data.
    fn handles(&self, data: &DiagnosticData) -> bool;

    /// Produce zero or more code actions for the given diagnostic.
    ///
    /// Only called when [`handles`](Self::handles) returned `true`.
    fn actions(
        &self,
        diagnostic: &Diagnostic,
        data: &DiagnosticData,
        ctx: &ActionRequestContext,
    ) -> Vec<CodeAction>;
}

// ---------------------------------------------------------------------------
// Test helpers (shared across provider tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_helpers {
    use std::collections::HashMap;
    use std::fmt::Write;
    use std::path::{Path, PathBuf};

    use lsp_types::{
        CodeAction, CodeActionKind, DocumentChangeOperation, DocumentChanges, OneOf, ResourceOp,
        Url,
    };
    use supersigil_core::{ComponentDefs, Config, DocumentGraph, SpecDocument, build_graph};

    use super::ActionRequestContext;

    /// Encapsulates common test setup for code action provider tests.
    ///
    /// Replaces the 5-line boilerplate (`graph`, `config`, `file_parses`, `uri`,
    /// `make_ctx(...)`) with a 2-line pattern:
    ///
    /// ```ignore
    /// let tc = TestContext::new();
    /// let ctx = tc.make_ctx(content);
    /// ```
    pub struct TestContext {
        pub graph: DocumentGraph,
        pub config: Config,
        pub component_defs: ComponentDefs,
        pub file_parses: HashMap<PathBuf, SpecDocument>,
        pub uri: Url,
    }

    impl TestContext {
        pub fn new() -> Self {
            Self {
                graph: build_graph(Vec::new(), &Config::default()).unwrap(),
                config: Config::default(),
                component_defs: ComponentDefs::defaults(),
                file_parses: HashMap::new(),
                uri: Url::parse("file:///tmp/project/spec.md").unwrap(),
            }
        }

        pub fn make_ctx<'a>(&'a self, content: &'a str) -> ActionRequestContext<'a> {
            ActionRequestContext {
                graph: &self.graph,
                config: &self.config,
                component_defs: &self.component_defs,
                file_parses: &self.file_parses,
                project_root: Path::new("/tmp/project"),
                file_uri: &self.uri,
                file_content: content,
            }
        }
    }

    /// Format a list of code actions into a human-readable snapshot string.
    ///
    /// Distinguishes zero-width ranges (insert) from non-zero ranges (replace).
    /// Multi-line `new_text` is shown with escaped newlines for readability.
    /// Handles both `workspace_edit.changes` and `workspace_edit.document_changes`.
    pub fn format_actions(actions: &[CodeAction]) -> String {
        let mut out = String::new();
        for action in actions {
            let kind = action.kind.as_ref().map_or("none", CodeActionKind::as_str);
            let _ = writeln!(out, "[{kind}] {}", action.title);
            if let Some(edit) = &action.edit {
                if let Some(changes) = &edit.changes {
                    for (uri, edits) in changes {
                        let _ = writeln!(out, "  edit: {uri}");
                        for te in edits {
                            format_text_edit(&mut out, te, "    ");
                        }
                    }
                }
                if let Some(doc_changes) = &edit.document_changes {
                    let _ = writeln!(out, "  document_changes:");
                    match doc_changes {
                        DocumentChanges::Edits(edits) => {
                            for tde in edits {
                                let _ = writeln!(out, "    edit {}", tde.text_document.uri);
                                for edit in &tde.edits {
                                    match edit {
                                        OneOf::Left(te) => {
                                            format_text_edit(&mut out, te, "      ");
                                        }
                                        OneOf::Right(ate) => {
                                            format_text_edit(&mut out, &ate.text_edit, "      ");
                                        }
                                    }
                                }
                            }
                        }
                        DocumentChanges::Operations(ops) => {
                            for op in ops {
                                match op {
                                    DocumentChangeOperation::Op(res_op) => match res_op {
                                        ResourceOp::Create(cf) => {
                                            let _ = writeln!(out, "    create {}", cf.uri);
                                        }
                                        ResourceOp::Rename(rf) => {
                                            let _ = writeln!(
                                                out,
                                                "    rename {} -> {}",
                                                rf.old_uri, rf.new_uri
                                            );
                                        }
                                        ResourceOp::Delete(df) => {
                                            let _ = writeln!(out, "    delete {}", df.uri);
                                        }
                                    },
                                    DocumentChangeOperation::Edit(tde) => {
                                        let _ = writeln!(out, "    edit {}", tde.text_document.uri);
                                        for edit in &tde.edits {
                                            match edit {
                                                OneOf::Left(te) => {
                                                    format_text_edit(&mut out, te, "      ");
                                                }
                                                OneOf::Right(ate) => {
                                                    format_text_edit(
                                                        &mut out,
                                                        &ate.text_edit,
                                                        "      ",
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(cmd) = &action.command {
                let _ = writeln!(out, "  command: {}", cmd.command);
                if let Some(args) = &cmd.arguments {
                    for arg in args {
                        let _ = writeln!(out, "    arg: {arg}");
                    }
                }
            }
        }
        out
    }

    fn format_text_edit(out: &mut String, te: &lsp_types::TextEdit, indent: &str) {
        let r = &te.range;
        let text = te.new_text.replace('\n', "\\n");
        if r.start == r.end {
            let _ = writeln!(
                out,
                "{indent}@{}:{} insert `{text}`",
                r.start.line, r.start.character
            );
        } else {
            let _ = writeln!(
                out,
                "{indent}@{}:{}-{}:{} replace `{text}`",
                r.start.line, r.start.character, r.end.line, r.end.character
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use supersigil_core::build_graph;
    use supersigil_rust_macros::verifies;

    use crate::diagnostics::{ActionContext, DiagnosticSource, ParseDiagnosticKind};

    /// A trivial provider used only to verify the trait is implementable.
    struct StubProvider;

    impl CodeActionProvider for StubProvider {
        fn handles(&self, data: &DiagnosticData) -> bool {
            matches!(
                data.source,
                DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute)
            )
        }

        fn actions(
            &self,
            _diagnostic: &Diagnostic,
            _data: &DiagnosticData,
            _ctx: &ActionRequestContext,
        ) -> Vec<CodeAction> {
            vec![]
        }
    }

    #[test]
    fn stub_provider_handles_matching_data() {
        let provider = StubProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute),
            doc_id: None,
            context: ActionContext::MissingAttribute {
                component: "Task".to_string(),
                attribute: "id".to_string(),
            },
        };
        assert!(provider.handles(&data));
    }

    #[test]
    fn stub_provider_rejects_non_matching_data() {
        let provider = StubProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    #[verifies("lsp-code-actions/req#req-3-2")]
    #[test]
    fn action_request_context_borrows_correctly() {
        let graph = build_graph(Vec::new(), &Config::default()).unwrap();
        let config = Config::default();
        let component_defs = ComponentDefs::defaults();
        let file_parses = HashMap::new();
        let project_root = PathBuf::from("/tmp/project");
        let file_uri = Url::parse("file:///tmp/project/spec.md").unwrap();
        let file_content = "hello";

        let ctx = ActionRequestContext {
            graph: &graph,
            config: &config,
            component_defs: &component_defs,
            file_parses: &file_parses,
            project_root: &project_root,
            file_uri: &file_uri,
            file_content,
        };

        // Verify all fields are accessible.
        assert_eq!(ctx.project_root, Path::new("/tmp/project"));
        assert_eq!(ctx.file_content, "hello");
        assert!(ctx.file_parses.is_empty());
        let _ = ctx.graph;
        let _ = ctx.config;
        let _ = ctx.component_defs;
        assert_eq!(ctx.file_uri.scheme(), "file");
    }

    #[verifies("lsp-code-actions/req#req-3-1")]
    #[test]
    fn provider_is_object_safe() {
        // Verify we can store a provider as a trait object.
        let provider: Box<dyn CodeActionProvider> = Box::new(StubProvider);
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(provider.handles(&data));
    }
}
