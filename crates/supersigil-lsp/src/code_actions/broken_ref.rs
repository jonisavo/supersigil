//! Code action provider for broken ref diagnostics.

use lsp_types::{
    CodeAction, Command, CreateFile, CreateFileOptions, Diagnostic, DocumentChangeOperation, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, ResourceOp, TextDocumentEdit,
    TextEdit, Url, WorkspaceEdit,
};
use supersigil_core::Config;
use supersigil_core::glob_prefix;
use supersigil_core::scaffold::{generate_template, is_known_doc_type, type_full_name};

use crate::code_actions::{ActionRequestContext, CodeActionProvider};
use crate::commands;
use crate::diagnostics::{ActionContext, DiagnosticData, DiagnosticSource, GraphDiagnosticKind};
use crate::position::byte_range_to_lsp;

// ---------------------------------------------------------------------------
// BrokenRefProvider
// ---------------------------------------------------------------------------

/// Offers to (a) remove a broken ref from a comma-separated attribute list,
/// and (b) create a missing document when the target path is unambiguous.
#[derive(Debug)]
pub struct BrokenRefProvider;

impl CodeActionProvider for BrokenRefProvider {
    fn handles(&self, data: &DiagnosticData) -> bool {
        matches!(
            data.source,
            DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef)
        )
    }

    fn actions(
        &self,
        diagnostic: &Diagnostic,
        data: &DiagnosticData,
        ctx: &ActionRequestContext,
    ) -> Vec<CodeAction> {
        let ActionContext::BrokenRef { target_ref } = &data.context else {
            return vec![];
        };

        let mut actions = Vec::new();

        // Action 1: Remove broken ref
        if let Some(action) = remove_broken_ref_action(diagnostic, target_ref, ctx) {
            actions.push(action);
        }

        // Action 2: Create document (only when "not found" and no fragment)
        if diagnostic.message.contains("not found") && !target_ref.contains('#') {
            if let Some(action) = create_document_action(target_ref, ctx) {
                // Unambiguous path — direct WorkspaceEdit.
                actions.push(action);
            } else if let Some(action) = create_document_command_action(target_ref, ctx.config) {
                // Ambiguous project — Command for interactive project selection.
                actions.push(action);
            }
        }

        actions
    }
}

// ---------------------------------------------------------------------------
// Action 1: Remove broken ref
// ---------------------------------------------------------------------------

/// Build a code action that removes the broken ref from the attribute value.
fn remove_broken_ref_action(
    diagnostic: &Diagnostic,
    target_ref: &str,
    ctx: &ActionRequestContext,
) -> Option<CodeAction> {
    let (attr_value_range, new_value) =
        find_and_remove_ref(ctx.file_content, &diagnostic.range, target_ref)?;

    let edit = ctx.single_file_edit(vec![TextEdit {
        range: attr_value_range,
        new_text: new_value,
    }]);

    Some(CodeAction {
        title: format!("Remove broken ref '{target_ref}'"),
        edit: Some(edit),
        ..Default::default()
    })
}

/// Static attribute name → needle prefix pairs for ref attributes.
const REF_ATTR_PREFIXES: &[(&str, &str)] = &[
    ("refs", "refs=\""),
    ("implements", "implements=\""),
    ("depends", "depends=\""),
];

/// Search the file content near the diagnostic position for a ref attribute
/// (`refs`, `implements`, or `depends`) containing `target_ref`.
///
/// Returns the range of the attribute value (inside the quotes) and the new
/// value with the broken ref removed.
fn find_and_remove_ref(
    content: &str,
    diag_range: &Range,
    target_ref: &str,
) -> Option<(Range, String)> {
    let diag_line = diag_range.start.line as usize;
    let search_start = diag_line.saturating_sub(3);
    let search_end = diag_line + 4;

    for (line_idx, line_text) in content
        .lines()
        .enumerate()
        .skip(search_start)
        .take(search_end - search_start)
    {
        // Look for refs="...", implements="...", or depends="..."
        for &(_attr_name, needle) in REF_ATTR_PREFIXES {
            let Some(attr_start) = line_text.find(needle) else {
                continue;
            };

            let value_start_byte = attr_start + needle.len();
            let rest = &line_text[value_start_byte..];
            let Some(closing_quote) = rest.find('"') else {
                continue;
            };

            let attr_value = &rest[..closing_quote];

            // Check that this attribute actually contains the target ref.
            let refs: Vec<&str> = attr_value.split(',').map(str::trim).collect();
            if !refs.contains(&target_ref) {
                continue;
            }

            // Remove the target ref from the list.
            let new_refs: Vec<&str> = refs.into_iter().filter(|r| *r != target_ref).collect();
            let new_value = new_refs.join(", ");

            #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
            let range = byte_range_to_lsp(
                line_text,
                line_idx as u32,
                value_start_byte,
                value_start_byte + closing_quote,
            );

            return Some((range, new_value));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Action 2: Create document
// ---------------------------------------------------------------------------

/// Build a code action that creates a new document file for the broken ref.
fn create_document_action(target_ref: &str, ctx: &ActionRequestContext) -> Option<CodeAction> {
    let (feature, type_short) = parse_doc_ref(target_ref)?;
    let full_type = type_full_name(type_short);

    if !is_known_doc_type(full_type, ctx.config) {
        return None;
    }

    let spec_dir = resolve_spec_dir(ctx)?;
    let file_rel = format!("{spec_dir}{feature}/{feature}.{type_short}.md");
    let file_path = ctx.project_root.join(&file_rel);
    let file_uri = Url::from_file_path(&file_path).ok()?;

    let content = generate_template(full_type, target_ref, feature, false);

    let create_op = DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
        uri: file_uri.clone(),
        options: Some(CreateFileOptions {
            overwrite: Some(false),
            ignore_if_exists: Some(false),
        }),
        annotation_id: None,
    }));

    let insert_op = DocumentChangeOperation::Edit(TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier {
            uri: file_uri,
            version: None,
        },
        edits: vec![OneOf::Left(TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            new_text: content,
        })],
    });

    let edit = WorkspaceEdit {
        document_changes: Some(lsp_types::DocumentChanges::Operations(vec![
            create_op, insert_op,
        ])),
        ..Default::default()
    };

    Some(CodeAction {
        title: format!("Create document '{target_ref}'"),
        edit: Some(edit),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Action 2b: Create document (command — interactive project selection)
// ---------------------------------------------------------------------------

/// Build a `CodeAction` with a `Command` for interactive project selection.
///
/// This is used when the project is ambiguous in multi-project mode. The
/// command handler (`supersigil.createDocument`) will prompt the user to pick
/// a project via `window/showMessageRequest`.
///
/// Returns `None` if the ref cannot be parsed into a `feature/type` pair.
fn create_document_command_action(target_ref: &str, config: &Config) -> Option<CodeAction> {
    let (feature, type_short) = parse_doc_ref(target_ref)?;
    let full_type = type_full_name(type_short);

    if !is_known_doc_type(full_type, config) {
        return None;
    }

    Some(CodeAction {
        title: format!("Create document '{target_ref}'"),
        command: Some(Command {
            title: "Create document".to_string(),
            command: commands::CREATE_DOCUMENT_COMMAND.to_string(),
            arguments: Some(vec![serde_json::json!({
                "ref": target_ref,
                "feature": feature,
                "type": full_type,
            })]),
        }),
        ..Default::default()
    })
}

/// Parse a document ref like `"auth/design"` into `("auth", "design")`.
///
/// Returns `None` if the ref does not have exactly one `/` separator.
fn parse_doc_ref(target_ref: &str) -> Option<(&str, &str)> {
    let (feature, type_short) = target_ref.split_once('/')?;
    if feature.is_empty() || type_short.is_empty() || type_short.contains('/') {
        return None;
    }
    Some((feature, type_short))
}

/// Determine the spec directory for new documents.
///
/// - Single-project mode (no `projects` in config): `"specs/"`
/// - Multi-project mode: find which project owns the current file by matching
///   its path against project glob patterns, and derive the spec dir via
///   `glob_prefix`.
/// - If the project is ambiguous (multiple matches or none), return `None`.
fn resolve_spec_dir(ctx: &ActionRequestContext) -> Option<String> {
    let projects = match &ctx.config.projects {
        Some(p) if !p.is_empty() => p,
        _ => return Some("specs/".to_string()),
    };

    // Get the file path relative to project root.
    let file_path = ctx.file_uri.to_file_path().ok()?;
    let rel_path = file_path
        .strip_prefix(ctx.project_root)
        .ok()?
        .to_string_lossy();

    // Find matching projects by checking if the file path matches any project pattern.
    let mut matching_dirs: Vec<String> = Vec::new();

    for project_config in projects.values() {
        for pattern in &project_config.paths {
            let prefix = glob_prefix(pattern);
            if rel_path.starts_with(&prefix) {
                matching_dirs.push(prefix);
                break; // One match per project is enough.
            }
        }
    }

    // Ambiguous or no match — skip the create action.
    (matching_dirs.len() == 1).then(|| matching_dirs.into_iter().next().unwrap())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use lsp_types::{Diagnostic, Position, Range, Url};
    use supersigil_core::{ComponentDefs, Config, ProjectConfig, build_graph};

    use supersigil_rust_macros::verifies;

    use crate::code_actions::test_helpers::{TestContext, format_actions};
    use crate::code_actions::{ActionRequestContext, CodeActionProvider};
    use crate::diagnostics::{
        ActionContext, DiagnosticData, DiagnosticSource, GraphDiagnosticKind,
    };

    use super::{BrokenRefProvider, find_and_remove_ref, parse_doc_ref};

    // -- Test helpers -------------------------------------------------------

    fn make_broken_ref_diagnostic(line: u32, col: u32, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, col), Position::new(line, col)),
            message: message.into(),
            ..Default::default()
        }
    }

    fn make_broken_ref_data(target_ref: &str) -> DiagnosticData {
        DiagnosticData {
            source: DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef),
            doc_id: Some("doc/req".into()),
            context: ActionContext::BrokenRef {
                target_ref: target_ref.to_string(),
            },
        }
    }

    // -- handles() ----------------------------------------------------------

    #[test]
    fn handles_broken_ref() {
        let provider = BrokenRefProvider;
        let data = make_broken_ref_data("auth/design");
        assert!(provider.handles(&data));
    }

    #[test]
    fn rejects_non_broken_ref() {
        let provider = BrokenRefProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateDocumentId),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    // -- Remove broken ref action -------------------------------------------

    #[verifies("lsp-code-actions/req#req-4-1", "lsp-code-actions/req#req-6-1")]
    #[test]
    fn remove_ref_from_multi_item_list() {
        let provider = BrokenRefProvider;
        let content = "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/req, auth/design\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 30, "broken ref `auth/design`: not found");
        let data = make_broken_ref_data("auth/design");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Remove broken ref 'auth/design'
          edit: file:///tmp/project/spec.md
            @4:18-4:39 replace `auth/req`
        [none] Create document 'auth/design'
          document_changes:
            create file:///tmp/project/specs/auth/auth.design.md
            edit file:///tmp/project/specs/auth/auth.design.md
              @0:0 insert `---\nsupersigil:\n  id: auth/design\n  type: design\n  status: draft\ntitle: ""\n---\n\n<!-- ```supersigil-xml\n<Implements refs="" />\n``` -->\n\n<!-- ```supersigil-xml\n<DependsOn refs="" />\n``` -->\n\n<!-- ```supersigil-xml\n<TrackedFiles paths="" />\n``` -->\n\n## Overview\n\n<!-- High-level summary of the design approach. -->\n\n## Architecture\n\n<!-- System structure, data flow, crate/module boundaries. Mermaid diagrams encouraged. -->\n\n## Key Types\n\n<!-- Core data structures and their relationships. Rust type sketches encouraged. -->\n\n## Error Handling\n\n<!-- Error types, failure modes, recovery strategies. -->\n\n## Testing Strategy\n\n<!-- How correctness will be verified: property tests, unit tests, integration tests. -->\n\n## Alternatives Considered\n\n<!-- Approaches that were evaluated and rejected, with rationale. -->\n`
        "#);
    }

    #[test]
    fn remove_ref_single_item() {
        let provider = BrokenRefProvider;
        let content =
            "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/design\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `auth/design`: not found");
        let data = make_broken_ref_data("auth/design");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Remove broken ref 'auth/design'
          edit: file:///tmp/project/spec.md
            @4:18-4:29 replace ``
        [none] Create document 'auth/design'
          document_changes:
            create file:///tmp/project/specs/auth/auth.design.md
            edit file:///tmp/project/specs/auth/auth.design.md
              @0:0 insert `---\nsupersigil:\n  id: auth/design\n  type: design\n  status: draft\ntitle: ""\n---\n\n<!-- ```supersigil-xml\n<Implements refs="" />\n``` -->\n\n<!-- ```supersigil-xml\n<DependsOn refs="" />\n``` -->\n\n<!-- ```supersigil-xml\n<TrackedFiles paths="" />\n``` -->\n\n## Overview\n\n<!-- High-level summary of the design approach. -->\n\n## Architecture\n\n<!-- System structure, data flow, crate/module boundaries. Mermaid diagrams encouraged. -->\n\n## Key Types\n\n<!-- Core data structures and their relationships. Rust type sketches encouraged. -->\n\n## Error Handling\n\n<!-- Error types, failure modes, recovery strategies. -->\n\n## Testing Strategy\n\n<!-- How correctness will be verified: property tests, unit tests, integration tests. -->\n\n## Alternatives Considered\n\n<!-- Approaches that were evaluated and rejected, with rationale. -->\n`
        "#);
    }

    #[test]
    fn remove_ref_first_of_three() {
        let provider = BrokenRefProvider;
        let content = "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"broken/ref, auth/req, auth/design\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `broken/ref`: not found");
        let data = make_broken_ref_data("broken/ref");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Remove broken ref 'broken/ref'
          edit: file:///tmp/project/spec.md
            @4:18-4:51 replace `auth/req, auth/design`
        "#);
    }

    #[test]
    fn remove_ref_middle_of_three() {
        let provider = BrokenRefProvider;
        let content = "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/req, broken/ref, auth/design\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 28, "broken ref `broken/ref`: not found");
        let data = make_broken_ref_data("broken/ref");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Remove broken ref 'broken/ref'
          edit: file:///tmp/project/spec.md
            @4:18-4:51 replace `auth/req, auth/design`
        "#);
    }

    #[test]
    fn remove_ref_with_depends_attribute() {
        let provider = BrokenRefProvider;
        let content = "---\nid: doc/tasks\n---\n```supersigil-xml\n<Task id=\"t-1\" status=\"draft\" depends=\"auth/design, billing/tasks\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 40, "broken ref `auth/design`: not found");
        let data = make_broken_ref_data("auth/design");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Remove broken ref 'auth/design'
          edit: file:///tmp/project/spec.md
            @4:39-4:65 replace `billing/tasks`
        [none] Create document 'auth/design'
          document_changes:
            create file:///tmp/project/specs/auth/auth.design.md
            edit file:///tmp/project/specs/auth/auth.design.md
              @0:0 insert `---\nsupersigil:\n  id: auth/design\n  type: design\n  status: draft\ntitle: ""\n---\n\n<!-- ```supersigil-xml\n<Implements refs="" />\n``` -->\n\n<!-- ```supersigil-xml\n<DependsOn refs="" />\n``` -->\n\n<!-- ```supersigil-xml\n<TrackedFiles paths="" />\n``` -->\n\n## Overview\n\n<!-- High-level summary of the design approach. -->\n\n## Architecture\n\n<!-- System structure, data flow, crate/module boundaries. Mermaid diagrams encouraged. -->\n\n## Key Types\n\n<!-- Core data structures and their relationships. Rust type sketches encouraged. -->\n\n## Error Handling\n\n<!-- Error types, failure modes, recovery strategies. -->\n\n## Testing Strategy\n\n<!-- How correctness will be verified: property tests, unit tests, integration tests. -->\n\n## Alternatives Considered\n\n<!-- Approaches that were evaluated and rejected, with rationale. -->\n`
        "#);
    }

    #[test]
    fn no_create_action_for_fragment_ref() {
        let provider = BrokenRefProvider;
        let content = "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/design#section\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `auth/design#section`: not found");
        let data = make_broken_ref_data("auth/design#section");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        // Should only have remove action, no create action.
        assert_eq!(actions.len(), 1);
        assert!(actions[0].title.starts_with("Remove broken ref"));
    }

    #[test]
    fn no_create_action_for_non_not_found_reason() {
        let provider = BrokenRefProvider;
        let content =
            "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/design\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `auth/design`: ambiguous target");
        let data = make_broken_ref_data("auth/design");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        // Should only have remove action, no create action.
        assert_eq!(actions.len(), 1);
        assert!(actions[0].title.starts_with("Remove broken ref"));
    }

    #[verifies("lsp-code-actions/req#req-5-1", "lsp-code-actions/req#req-5-4")]
    #[test]
    fn create_document_uses_req_expansion() {
        let provider = BrokenRefProvider;
        let content =
            "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/req\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `auth/req`: not found");
        let data = make_broken_ref_data("auth/req");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Remove broken ref 'auth/req'
          edit: file:///tmp/project/spec.md
            @4:18-4:26 replace ``
        [none] Create document 'auth/req'
          document_changes:
            create file:///tmp/project/specs/auth/auth.req.md
            edit file:///tmp/project/specs/auth/auth.req.md
              @0:0 insert `---\nsupersigil:\n  id: auth/req\n  type: requirements\n  status: draft\ntitle: ""\n---\n\n## Introduction\n\n<!-- What problem does this feature solve? What is in scope and out of scope? -->\n\n## Definitions\n\n<!-- Domain terms used in the requirements below. Use bold for the term name. -->\n\n- **Term**: Definition.\n\n## Requirement 1: Title\n\nAs a [role], I want [capability], so that [benefit].\n\n```supersigil-xml\n<AcceptanceCriteria>\n  <Criterion id="req-1-1">\n    WHEN [precondition], THE [component] SHALL [behavior].\n  </Criterion>\n</AcceptanceCriteria>\n```\n`
        "#);
    }

    #[test]
    fn create_document_multi_project_unambiguous() {
        let provider = BrokenRefProvider;
        let content =
            "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/design\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `auth/design`: not found");
        let data = make_broken_ref_data("auth/design");

        let graph = build_graph(Vec::new(), &Config::default()).unwrap();
        let mut projects = HashMap::new();
        projects.insert(
            "backend".to_string(),
            ProjectConfig {
                paths: vec!["backend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );
        projects.insert(
            "frontend".to_string(),
            ProjectConfig {
                paths: vec!["frontend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );
        let config = Config {
            projects: Some(projects),
            ..Config::default()
        };
        let component_defs = ComponentDefs::defaults();
        let file_parses = HashMap::new();
        // File is under backend/specs/
        let uri = Url::parse("file:///tmp/project/backend/specs/doc/doc.req.md").unwrap();
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
        [none] Remove broken ref 'auth/design'
          edit: file:///tmp/project/backend/specs/doc/doc.req.md
            @4:18-4:29 replace ``
        [none] Create document 'auth/design'
          document_changes:
            create file:///tmp/project/backend/specs/auth/auth.design.md
            edit file:///tmp/project/backend/specs/auth/auth.design.md
              @0:0 insert `---\nsupersigil:\n  id: auth/design\n  type: design\n  status: draft\ntitle: ""\n---\n\n<!-- ```supersigil-xml\n<Implements refs="" />\n``` -->\n\n<!-- ```supersigil-xml\n<DependsOn refs="" />\n``` -->\n\n<!-- ```supersigil-xml\n<TrackedFiles paths="" />\n``` -->\n\n## Overview\n\n<!-- High-level summary of the design approach. -->\n\n## Architecture\n\n<!-- System structure, data flow, crate/module boundaries. Mermaid diagrams encouraged. -->\n\n## Key Types\n\n<!-- Core data structures and their relationships. Rust type sketches encouraged. -->\n\n## Error Handling\n\n<!-- Error types, failure modes, recovery strategies. -->\n\n## Testing Strategy\n\n<!-- How correctness will be verified: property tests, unit tests, integration tests. -->\n\n## Alternatives Considered\n\n<!-- Approaches that were evaluated and rejected, with rationale. -->\n`
        "#);
    }

    #[verifies("lsp-code-actions/req#req-5-2")]
    #[test]
    fn create_document_command_when_multi_project_ambiguous() {
        let provider = BrokenRefProvider;
        let content =
            "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/design\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `auth/design`: not found");
        let data = make_broken_ref_data("auth/design");

        let graph = build_graph(Vec::new(), &Config::default()).unwrap();
        let mut projects = HashMap::new();
        projects.insert(
            "backend".to_string(),
            ProjectConfig {
                paths: vec!["backend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );
        projects.insert(
            "frontend".to_string(),
            ProjectConfig {
                paths: vec!["frontend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );
        let config = Config {
            projects: Some(projects),
            ..Config::default()
        };
        let component_defs = ComponentDefs::defaults();
        let file_parses = HashMap::new();
        // File is NOT under any project's spec dir — ambiguous.
        let uri = Url::parse("file:///tmp/project/shared/doc.req.md").unwrap();
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
        [none] Remove broken ref 'auth/design'
          edit: file:///tmp/project/shared/doc.req.md
            @4:18-4:29 replace ``
        [none] Create document 'auth/design'
          command: supersigil.createDocument
            arg: {"feature":"auth","ref":"auth/design","type":"design"}
        "#);
    }

    #[test]
    fn no_action_when_context_is_none() {
        let provider = BrokenRefProvider;
        let content = "---\nid: doc/req\n---\n";
        let diag = make_broken_ref_diagnostic(0, 0, "broken ref");
        let data = DiagnosticData {
            source: DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef),
            doc_id: None,
            context: ActionContext::None,
        };

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn no_create_action_for_unparseable_ref() {
        let provider = BrokenRefProvider;
        let content =
            "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"badref\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `badref`: not found");
        let data = make_broken_ref_data("badref");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        // Only remove, no create (can't parse feature/type from "badref").
        assert_eq!(actions.len(), 1);
        assert!(actions[0].title.starts_with("Remove broken ref"));
    }

    #[test]
    fn create_document_command_uses_req_expansion() {
        let provider = BrokenRefProvider;
        let content =
            "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/req\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `auth/req`: not found");
        let data = make_broken_ref_data("auth/req");

        let graph = build_graph(Vec::new(), &Config::default()).unwrap();
        let mut projects = HashMap::new();
        projects.insert(
            "backend".to_string(),
            ProjectConfig {
                paths: vec!["backend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );
        projects.insert(
            "frontend".to_string(),
            ProjectConfig {
                paths: vec!["frontend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );
        let config = Config {
            projects: Some(projects),
            ..Config::default()
        };
        let component_defs = ComponentDefs::defaults();
        let file_parses = HashMap::new();
        let uri = Url::parse("file:///tmp/project/shared/doc.req.md").unwrap();
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
        [none] Remove broken ref 'auth/req'
          edit: file:///tmp/project/shared/doc.req.md
            @4:18-4:26 replace ``
        [none] Create document 'auth/req'
          command: supersigil.createDocument
            arg: {"feature":"auth","ref":"auth/req","type":"requirements"}
        "#);
    }

    // -- Unit tests for helpers ---------------------------------------------

    #[test]
    fn parse_doc_ref_valid() {
        assert_eq!(parse_doc_ref("auth/design"), Some(("auth", "design")));
        assert_eq!(parse_doc_ref("billing/req"), Some(("billing", "req")));
    }

    #[test]
    fn parse_doc_ref_invalid() {
        assert_eq!(parse_doc_ref("nofeature"), None);
        assert_eq!(parse_doc_ref("a/b/c"), None);
        assert_eq!(parse_doc_ref("/empty"), None);
        assert_eq!(parse_doc_ref("empty/"), None);
    }

    #[test]
    fn find_and_remove_ref_single() {
        let content = "<Implements refs=\"auth/design\" />";
        let range = Range::new(Position::new(0, 18), Position::new(0, 18));
        let (edit_range, new_value) = find_and_remove_ref(content, &range, "auth/design").unwrap();
        assert_eq!(new_value, "");
        assert_eq!(edit_range.start, Position::new(0, 18));
        assert_eq!(edit_range.end, Position::new(0, 29));
    }

    #[test]
    fn find_and_remove_ref_first_of_two() {
        let content = "<Implements refs=\"auth/design, auth/req\" />";
        let range = Range::new(Position::new(0, 18), Position::new(0, 18));
        let (_, new_value) = find_and_remove_ref(content, &range, "auth/design").unwrap();
        assert_eq!(new_value, "auth/req");
    }

    #[test]
    fn find_and_remove_ref_last_of_two() {
        let content = "<Implements refs=\"auth/req, auth/design\" />";
        let range = Range::new(Position::new(0, 28), Position::new(0, 28));
        let (_, new_value) = find_and_remove_ref(content, &range, "auth/design").unwrap();
        assert_eq!(new_value, "auth/req");
    }

    #[test]
    fn find_and_remove_ref_with_implements_attr() {
        let content = "<Task id=\"t-1\" implements=\"auth/req, broken/ref\" />";
        let range = Range::new(Position::new(0, 36), Position::new(0, 36));
        let (_, new_value) = find_and_remove_ref(content, &range, "broken/ref").unwrap();
        assert_eq!(new_value, "auth/req");
    }

    // -- is_known_doc_type() --------------------------------------------------

    #[test]
    fn known_builtin_types() {
        use super::is_known_doc_type;
        let config = Config::default();
        assert!(is_known_doc_type("requirements", &config));
        assert!(is_known_doc_type("design", &config));
        assert!(is_known_doc_type("tasks", &config));
        assert!(is_known_doc_type("adr", &config));
    }

    #[test]
    fn unknown_type_rejected() {
        use super::is_known_doc_type;
        let config = Config::default();
        assert!(!is_known_doc_type("ref", &config));
        assert!(!is_known_doc_type("unknown", &config));
    }

    #[test]
    fn custom_type_from_config_accepted() {
        use super::is_known_doc_type;
        use supersigil_core::{DocumentTypeDef, DocumentsConfig};

        let mut types = HashMap::new();
        types.insert(
            "runbook".to_string(),
            DocumentTypeDef {
                status: vec![],
                required_components: vec![],
                description: None,
            },
        );
        let config = Config {
            documents: DocumentsConfig { types },
            ..Config::default()
        };
        assert!(is_known_doc_type("runbook", &config));
    }

    #[test]
    fn no_create_action_for_unknown_type() {
        let provider = BrokenRefProvider;
        let content =
            "---\nid: doc/req\n---\n```supersigil-xml\n<Implements refs=\"auth/unknown\" />\n```\n";
        let diag = make_broken_ref_diagnostic(4, 18, "broken ref `auth/unknown`: not found");
        let data = make_broken_ref_data("auth/unknown");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        // Should only have remove action, no create action for unknown type.
        assert_eq!(actions.len(), 1);
        assert!(actions[0].title.starts_with("Remove broken ref"));
    }
}
