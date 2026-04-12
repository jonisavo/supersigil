//! Integration tests that apply `WorkspaceEdit`s to source files, re-parse, and
//! verify that the originating diagnostic is resolved (req-6-2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lsp_types::{
    DocumentChangeOperation, DocumentChanges, OneOf, Position, Range, TextEdit, Url, WorkspaceEdit,
};
use supersigil_core::{ComponentDefs, Config, ParseResult, SpecDocument, build_graph, load_config};
use supersigil_lsp::code_actions::{
    ActionRequestContext, BrokenRefProvider, CodeActionProvider, DuplicateIdProvider,
    IncompleteDecisionProvider, MissingAttributeProvider,
};
use supersigil_lsp::diagnostics::{
    DiagnosticData, finding_to_diagnostic, graph_error_to_diagnostic_with_lookup,
    parse_error_to_diagnostic,
};
use supersigil_parser::parse_content;
use supersigil_rust_macros::verifies;
use supersigil_verify::{ArtifactGraph, VerifyOptions, verify};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// WorkspaceEdit application helpers
// ---------------------------------------------------------------------------

/// Apply a `WorkspaceEdit` to an in-memory file map.
///
/// Handles both `changes` (simple edits) and `document_changes` (create file +
/// text document edits). Text edits are sorted in reverse position order so
/// earlier edits don't invalidate the byte offsets of later ones.
fn apply_workspace_edit(files: &mut HashMap<Url, String>, edit: &WorkspaceEdit) {
    if let Some(changes) = &edit.changes {
        for (uri, edits) in changes {
            let content = files.get_mut(uri).expect("file should exist in map");
            apply_text_edits(content, edits);
        }
    }

    if let Some(doc_changes) = &edit.document_changes {
        match doc_changes {
            DocumentChanges::Edits(edits) => {
                for tde in edits {
                    let content = files
                        .get_mut(&tde.text_document.uri)
                        .expect("file should exist in map");
                    let text_edits: Vec<TextEdit> = tde
                        .edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(te) => te.clone(),
                            OneOf::Right(ate) => ate.text_edit.clone(),
                        })
                        .collect();
                    apply_text_edits(content, &text_edits);
                }
            }
            DocumentChanges::Operations(ops) => {
                for op in ops {
                    match op {
                        DocumentChangeOperation::Op(lsp_types::ResourceOp::Create(cf)) => {
                            files.entry(cf.uri.clone()).or_default();
                        }
                        DocumentChangeOperation::Edit(tde) => {
                            let content = files
                                .get_mut(&tde.text_document.uri)
                                .expect("file should exist in map");
                            let text_edits: Vec<TextEdit> = tde
                                .edits
                                .iter()
                                .map(|e| match e {
                                    OneOf::Left(te) => te.clone(),
                                    OneOf::Right(ate) => ate.text_edit.clone(),
                                })
                                .collect();
                            apply_text_edits(content, &text_edits);
                        }
                        DocumentChangeOperation::Op(_) => {}
                    }
                }
            }
        }
    }
}

/// Apply a set of `TextEdit`s to a string, sorting by position descending so
/// each edit doesn't invalidate subsequent offsets.
fn apply_text_edits(content: &mut String, edits: &[TextEdit]) {
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by(|a, b| {
        b.range
            .start
            .line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });

    for edit in sorted {
        let start = lsp_pos_to_byte_offset(content, edit.range.start);
        let end = lsp_pos_to_byte_offset(content, edit.range.end);
        content.replace_range(start..end, &edit.new_text);
    }
}

/// Convert an LSP `Position` (0-based line, 0-based UTF-16 character offset) to
/// a byte offset within a string. Assumes ASCII content so UTF-16 code units
/// equal byte offsets within a line.
fn lsp_pos_to_byte_offset(content: &str, pos: Position) -> usize {
    let mut offset = 0;
    for (i, line) in content.lines().enumerate() {
        if i == pos.line as usize {
            return offset + pos.character as usize;
        }
        offset += line.len() + 1; // +1 for '\n'
    }
    // Position is past end of content — return content length.
    content.len()
}

// ---------------------------------------------------------------------------
// Test project setup helpers
// ---------------------------------------------------------------------------

/// Write a minimal `supersigil.toml` and return the parsed `Config`.
fn write_config(dir: &Path) -> Config {
    let toml_content = "paths = [\"specs/**/*.md\"]\n";
    std::fs::create_dir_all(dir.join("specs")).unwrap();
    let config_path = dir.join("supersigil.toml");
    std::fs::write(&config_path, toml_content).unwrap();
    load_config(&config_path).expect("config should parse")
}

/// Write a spec file and return its absolute path.
fn write_spec(dir: &Path, rel_path: &str, content: &str) -> PathBuf {
    let path = dir.join(rel_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, content).unwrap();
    path
}

/// Parse a spec file from content, returning the `SpecDocument` (panics if
/// the file is not a supersigil document or has fatal parse errors).
fn parse_spec(
    path: &Path,
    content: &str,
) -> Result<SpecDocument, Vec<supersigil_core::ParseError>> {
    let defs = ComponentDefs::defaults();
    match parse_content(path, content, &defs) {
        Ok(ParseResult::Document(doc)) => Ok(doc),
        Ok(ParseResult::NotSupersigil(_)) => panic!("expected a supersigil document"),
        Err(errors) => Err(errors),
    }
}

// ---------------------------------------------------------------------------
// Test: MissingAttributeProvider — adds missing `id` attribute
// ---------------------------------------------------------------------------

#[verifies("lsp-code-actions/req#req-6-2")]
#[test]
fn missing_attribute_fix_resolves_diagnostic() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let config = write_config(dir);

    // Create a spec file with a Task missing `id`.
    let original_content = "\
---
supersigil:
  id: test-doc
---
```supersigil-xml
<Task status=\"draft\" />
```
";
    let spec_path = write_spec(dir, "specs/test-doc/test-doc.spec.md", original_content);

    // Step 1: Parse and verify the error exists.
    let parse_result = parse_content(&spec_path, original_content, &ComponentDefs::defaults());
    let errors = parse_result.expect_err("should have parse errors");
    let missing_id_error = errors
        .iter()
        .find(|e| matches!(e, supersigil_core::ParseError::MissingRequiredAttribute { attribute, .. } if attribute == "id"))
        .expect("should have a MissingRequiredAttribute error for 'id'");

    // Step 2: Convert to diagnostic.
    let (uri, diagnostic) =
        parse_error_to_diagnostic(missing_id_error, Some(original_content)).unwrap();
    let data: DiagnosticData = serde_json::from_value(diagnostic.data.clone().unwrap()).unwrap();

    // Step 3: Get the code action.
    let provider = MissingAttributeProvider;
    assert!(provider.handles(&data));

    let empty_graph = build_graph(Vec::new(), &Config::default()).unwrap();
    let component_defs = ComponentDefs::defaults();
    let file_parses = HashMap::new();
    let ctx = ActionRequestContext {
        graph: &empty_graph,
        config: &config,
        component_defs: &component_defs,
        file_parses: &file_parses,
        partial_file_parses: &file_parses,
        project_root: dir,
        file_uri: &uri,
        file_content: original_content,
    };

    let actions = provider.actions(&diagnostic, &data, &ctx);
    assert!(
        !actions.is_empty(),
        "should produce at least one code action"
    );
    let action = &actions[0];
    let workspace_edit = action
        .edit
        .as_ref()
        .expect("action should have a workspace edit");

    // Step 4: Apply the edit.
    let mut files: HashMap<Url, String> = HashMap::new();
    files.insert(uri.clone(), original_content.to_string());
    apply_workspace_edit(&mut files, workspace_edit);
    let fixed_content = &files[&uri];

    // Step 5: Re-parse and verify the error is gone.
    // The fix adds `id=""` which is empty — that's still valid from the
    // "missing attribute" perspective (the parser only checks presence, not
    // non-emptiness).
    let re_parse = parse_content(&spec_path, fixed_content, &ComponentDefs::defaults());
    let has_missing_id = match &re_parse {
        Err(errors) => errors.iter().any(|e| {
            matches!(e, supersigil_core::ParseError::MissingRequiredAttribute { attribute, .. } if attribute == "id")
        }),
        Ok(_) => false,
    };
    assert!(
        !has_missing_id,
        "after applying the fix, the MissingRequiredAttribute error for 'id' should be gone.\nFixed content:\n{fixed_content}"
    );
}

// ---------------------------------------------------------------------------
// Test: DuplicateIdProvider — renames a duplicate component ID
// ---------------------------------------------------------------------------

#[test]
fn duplicate_component_id_fix_resolves_diagnostic() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let config = write_config(dir);

    // Create a spec file with two components sharing the same ID.
    let original_content = "\
---
supersigil:
  id: test-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\" />
<Task id=\"task-1\" status=\"active\" />
```
";
    let spec_path = write_spec(dir, "specs/test-doc/test-doc.spec.md", original_content);

    // Step 1: Parse (should succeed) and build graph (should fail with DuplicateComponentId).
    let doc = parse_spec(&spec_path, original_content).expect("parse should succeed");
    let graph_errors = build_graph(vec![doc], &config).expect_err("should have graph errors");

    let dup_error = graph_errors
        .iter()
        .find(|e| matches!(e, supersigil_core::GraphError::DuplicateComponentId { .. }))
        .expect("should have a DuplicateComponentId error");

    // Step 2: Convert to diagnostics. DuplicateComponentId produces one diagnostic
    // per position.
    let id_to_path: HashMap<String, PathBuf> =
        HashMap::from([("test-doc".to_string(), spec_path.clone())]);
    let diag_pairs =
        graph_error_to_diagnostic_with_lookup(dup_error, |doc_id| id_to_path.get(doc_id).cloned());
    assert!(
        !diag_pairs.is_empty(),
        "should produce diagnostics for duplicate IDs"
    );

    // Pick the first diagnostic (for the first occurrence).
    let (uri, diagnostic) = &diag_pairs[0];
    let data: DiagnosticData = serde_json::from_value(diagnostic.data.clone().unwrap()).unwrap();

    // Step 3: Get the code action.
    let provider = DuplicateIdProvider;
    assert!(provider.handles(&data));

    let empty_graph = build_graph(Vec::new(), &Config::default()).unwrap();
    let component_defs = ComponentDefs::defaults();
    let file_parses = HashMap::new();
    let ctx = ActionRequestContext {
        graph: &empty_graph,
        config: &config,
        component_defs: &component_defs,
        file_parses: &file_parses,
        partial_file_parses: &file_parses,
        project_root: dir,
        file_uri: uri,
        file_content: original_content,
    };

    let actions = provider.actions(diagnostic, &data, &ctx);
    assert!(
        !actions.is_empty(),
        "should produce at least one code action"
    );
    let action = &actions[0];
    let workspace_edit = action
        .edit
        .as_ref()
        .expect("action should have a workspace edit");

    // Step 4: Apply the edit.
    let mut files: HashMap<Url, String> = HashMap::new();
    files.insert(uri.clone(), original_content.to_string());
    apply_workspace_edit(&mut files, workspace_edit);
    let fixed_content = &files[uri];

    // Write the fixed content to disk so the graph builder can read it.
    std::fs::write(&spec_path, fixed_content).unwrap();

    // Step 5: Re-parse and re-build graph — the duplicate should be gone.
    let re_doc = parse_spec(&spec_path, fixed_content).expect("re-parse should succeed");
    let graph_result = build_graph(vec![re_doc], &config);
    let has_duplicate = match &graph_result {
        Err(errors) => errors
            .iter()
            .any(|e| matches!(e, supersigil_core::GraphError::DuplicateComponentId { .. })),
        Ok(_) => false,
    };
    assert!(
        !has_duplicate,
        "after applying the fix, the DuplicateComponentId error should be gone.\nFixed content:\n{fixed_content}"
    );
}

// ---------------------------------------------------------------------------
// Test: BrokenRefProvider (remove ref) — removes a broken ref
// ---------------------------------------------------------------------------

#[test]
fn broken_ref_remove_fix_resolves_diagnostic() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let config = write_config(dir);

    // Create two spec files. Doc A has a Criterion that refs both a valid doc
    // (doc-b#crit-real) and a non-existent one (doc-c#crit-x). Doc B exists
    // with crit-real.
    let content_a = "\
---
supersigil:
  id: doc-a
---
```supersigil-xml
<Criterion id=\"crit-1\" refs=\"doc-b#crit-real, doc-c#crit-x\" />
```
";
    let content_b = "\
---
supersigil:
  id: doc-b
---
```supersigil-xml
<Criterion id=\"crit-real\" />
```
";
    let spec_path_a = write_spec(dir, "specs/doc-a/doc-a.spec.md", content_a);
    let spec_path_b = write_spec(dir, "specs/doc-b/doc-b.spec.md", content_b);

    // Step 1: Parse docs and build graph.
    let doc_a = parse_spec(&spec_path_a, content_a).expect("parse should succeed");
    let doc_b = parse_spec(&spec_path_b, content_b).expect("parse should succeed");
    let graph_errors =
        build_graph(vec![doc_a, doc_b], &config).expect_err("should have graph errors");

    let broken_ref_error = graph_errors
        .iter()
        .find(|e| matches!(e, supersigil_core::GraphError::BrokenRef { ref_str, .. } if ref_str.contains("doc-c")))
        .expect("should have a BrokenRef error for doc-c");

    // Step 2: Convert to diagnostic.
    let id_to_path: HashMap<String, PathBuf> = HashMap::from([
        ("doc-a".to_string(), spec_path_a.clone()),
        ("doc-b".to_string(), spec_path_b.clone()),
    ]);
    let diag_pairs = graph_error_to_diagnostic_with_lookup(broken_ref_error, |doc_id| {
        id_to_path.get(doc_id).cloned()
    });
    assert!(!diag_pairs.is_empty(), "should produce diagnostics");

    let (uri, diagnostic) = &diag_pairs[0];
    let data: DiagnosticData = serde_json::from_value(diagnostic.data.clone().unwrap()).unwrap();

    // Step 3: Get the "remove broken ref" code action.
    let provider = BrokenRefProvider;
    assert!(provider.handles(&data));

    let empty_graph = build_graph(Vec::new(), &Config::default()).unwrap();
    let component_defs = ComponentDefs::defaults();
    let file_parses = HashMap::new();
    let ctx = ActionRequestContext {
        graph: &empty_graph,
        config: &config,
        component_defs: &component_defs,
        file_parses: &file_parses,
        partial_file_parses: &file_parses,
        project_root: dir,
        file_uri: uri,
        file_content: content_a,
    };

    let actions = provider.actions(diagnostic, &data, &ctx);
    // The first action should be "Remove broken ref".
    let remove_action = actions
        .iter()
        .find(|a| a.title.contains("Remove"))
        .expect("should have a 'Remove broken ref' action");
    let workspace_edit = remove_action
        .edit
        .as_ref()
        .expect("action should have a workspace edit");

    // Step 4: Apply the edit.
    let mut files: HashMap<Url, String> = HashMap::new();
    files.insert(uri.clone(), content_a.to_string());
    apply_workspace_edit(&mut files, workspace_edit);
    let fixed_content = &files[uri];

    // Write fixed content to disk.
    std::fs::write(&spec_path_a, fixed_content).unwrap();

    // Step 5: Re-parse and re-build graph — the broken ref should be gone.
    let re_doc_a = parse_spec(&spec_path_a, fixed_content).expect("re-parse should succeed");
    let re_doc_b = parse_spec(&spec_path_b, content_b).expect("re-parse should succeed");
    let graph_result = build_graph(vec![re_doc_a, re_doc_b], &config);
    let has_broken_ref = match &graph_result {
        Err(errors) => errors
            .iter()
            .any(|e| matches!(e, supersigil_core::GraphError::BrokenRef { .. })),
        Ok(_) => false,
    };
    assert!(
        !has_broken_ref,
        "after applying the fix, the BrokenRef error should be gone.\nFixed content:\n{fixed_content}"
    );
}

// ---------------------------------------------------------------------------
// Test: IncompleteDecisionProvider — adds Rationale stub
// ---------------------------------------------------------------------------

#[verifies("lsp-code-actions/req#req-6-2")]
#[test]
fn incomplete_decision_fix_resolves_diagnostic() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let config = write_config(dir);

    // Create a spec file with a Decision missing Rationale.
    let original_content = "\
---
supersigil:
  id: test-adr
---
```supersigil-xml
<Decision id=\"use-postgres\" status=\"proposed\">
</Decision>
```
";
    let spec_path = write_spec(dir, "specs/test-adr/test-adr.spec.md", original_content);

    // Step 1: Parse, build graph, and run verify to get findings.
    let doc = parse_spec(&spec_path, original_content).expect("parse should succeed");
    let graph = build_graph(vec![doc], &config).expect("graph build should succeed");
    let artifact_graph = ArtifactGraph::empty(&graph);
    let options = VerifyOptions::default();

    let report =
        verify(&graph, &config, dir, &options, &artifact_graph).expect("verify should succeed");

    // Find the IncompleteDecision finding.
    let incomplete_finding = report
        .findings
        .iter()
        .find(|f| f.rule == supersigil_verify::RuleName::IncompleteDecision)
        .expect("should have an IncompleteDecision finding");

    // Step 2: Convert to diagnostic.
    let id_to_path: HashMap<String, PathBuf> =
        HashMap::from([("test-adr".to_string(), spec_path.clone())]);
    let (uri, diagnostic) =
        finding_to_diagnostic(incomplete_finding, |doc_id| id_to_path.get(doc_id).cloned())
            .expect("should produce a diagnostic");
    let data: DiagnosticData = serde_json::from_value(diagnostic.data.clone().unwrap()).unwrap();

    // Step 3: Get the code action.
    let provider = IncompleteDecisionProvider;
    assert!(provider.handles(&data));

    let component_defs = ComponentDefs::defaults();
    let file_parses = HashMap::new();
    let ctx = ActionRequestContext {
        graph: &graph,
        config: &config,
        component_defs: &component_defs,
        file_parses: &file_parses,
        partial_file_parses: &file_parses,
        project_root: dir,
        file_uri: &uri,
        file_content: original_content,
    };

    let actions = provider.actions(&diagnostic, &data, &ctx);
    // The first action should be "Add <Rationale> stub".
    let rationale_action = actions
        .iter()
        .find(|a| a.title.contains("Rationale"))
        .expect("should have a 'Add <Rationale> stub' action");
    let workspace_edit = rationale_action
        .edit
        .as_ref()
        .expect("action should have a workspace edit");

    // Step 4: Apply the edit.
    let mut files: HashMap<Url, String> = HashMap::new();
    files.insert(uri.clone(), original_content.to_string());
    apply_workspace_edit(&mut files, workspace_edit);
    let fixed_content = &files[&uri];

    // Write fixed content to disk.
    std::fs::write(&spec_path, fixed_content).unwrap();

    // Step 5: Re-parse, re-build graph, and re-verify.
    let re_doc = parse_spec(&spec_path, fixed_content).expect("re-parse should succeed");
    let re_graph = build_graph(vec![re_doc], &config).expect("graph should build");
    let re_artifact_graph = ArtifactGraph::empty(&re_graph);

    let re_report = verify(&re_graph, &config, dir, &options, &re_artifact_graph)
        .expect("re-verify should succeed");

    let has_incomplete_decision = re_report
        .findings
        .iter()
        .any(|f| f.rule == supersigil_verify::RuleName::IncompleteDecision);
    assert!(
        !has_incomplete_decision,
        "after applying the fix, the IncompleteDecision finding should be gone.\nFixed content:\n{fixed_content}"
    );
}

// ---------------------------------------------------------------------------
// Test: apply_workspace_edit helper correctness
// ---------------------------------------------------------------------------

#[test]
fn apply_workspace_edit_insert_at_position() {
    let uri = Url::parse("file:///tmp/test.md").unwrap();
    let mut files = HashMap::new();
    files.insert(uri.clone(), "line0\nline1\nline2\n".to_string());

    let edit = WorkspaceEdit {
        changes: Some(HashMap::from([(
            uri.clone(),
            vec![TextEdit {
                range: Range::new(Position::new(1, 5), Position::new(1, 5)),
                new_text: " inserted".to_string(),
            }],
        )])),
        ..Default::default()
    };

    apply_workspace_edit(&mut files, &edit);
    assert_eq!(files[&uri], "line0\nline1 inserted\nline2\n");
}

#[test]
fn apply_workspace_edit_replace_range() {
    let uri = Url::parse("file:///tmp/test.md").unwrap();
    let mut files = HashMap::new();
    files.insert(uri.clone(), "hello world\n".to_string());

    let edit = WorkspaceEdit {
        changes: Some(HashMap::from([(
            uri.clone(),
            vec![TextEdit {
                range: Range::new(Position::new(0, 6), Position::new(0, 11)),
                new_text: "rust".to_string(),
            }],
        )])),
        ..Default::default()
    };

    apply_workspace_edit(&mut files, &edit);
    assert_eq!(files[&uri], "hello rust\n");
}

#[test]
fn apply_workspace_edit_multiple_edits_descending_order() {
    let uri = Url::parse("file:///tmp/test.md").unwrap();
    let mut files = HashMap::new();
    files.insert(uri.clone(), "aaa bbb ccc\n".to_string());

    // Two replacements that should both apply cleanly.
    let edit = WorkspaceEdit {
        changes: Some(HashMap::from([(
            uri.clone(),
            vec![
                TextEdit {
                    range: Range::new(Position::new(0, 0), Position::new(0, 3)),
                    new_text: "AAA".to_string(),
                },
                TextEdit {
                    range: Range::new(Position::new(0, 8), Position::new(0, 11)),
                    new_text: "CCC".to_string(),
                },
            ],
        )])),
        ..Default::default()
    };

    apply_workspace_edit(&mut files, &edit);
    assert_eq!(files[&uri], "AAA bbb CCC\n");
}
