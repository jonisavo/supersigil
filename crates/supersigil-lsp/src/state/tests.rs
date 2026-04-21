use super::indexing::{collect_globs, discover_config, empty_graph};
use super::*;
use crate::commands as lsp_commands;

use std::sync::{Arc as StdArc, Mutex};

use async_lsp::router::Router;
use futures::executor::block_on;
use lsp_types::{
    PartialResultParams, TextDocumentIdentifier, TextDocumentItem, WorkDoneProgressParams,
};

use supersigil_rust_macros::verifies;

#[test]
fn empty_graph_builds_successfully() {
    let graph = empty_graph();
    assert_eq!(graph.doc_order().len(), 0);
}

#[test]
fn collect_globs_single_project() {
    let config = Config {
        paths: Some(vec!["specs/**/*.md".into()]),
        ..Config::default()
    };
    let globs = collect_globs(&config);
    assert_eq!(globs, vec!["specs/**/*.md"]);
}

#[test]
fn collect_globs_empty_config() {
    let config = Config::default();
    let globs = collect_globs(&config);
    assert!(globs.is_empty());
}

#[test]
#[verifies("lsp-server/req#req-5-2", "lsp-server/req#req-7-2")]
fn discover_config_returns_none_in_temp_dir() {
    let dir = tempfile::tempdir().unwrap();
    assert!(discover_config(dir.path()).is_none());
}

#[test]
#[verifies("lsp-server/req#req-1-3")]
fn plugin_evidence_incorporated_into_verify() {
    use std::collections::HashMap;
    use supersigil_core::{EcosystemConfig, ExtractedComponent, Frontmatter, SpecDocument};

    let dir = tempfile::tempdir().unwrap();
    let test_dir = dir.path().join("tests");
    std::fs::create_dir_all(&test_dir).unwrap();

    // A Rust test file that uses #[verifies("my-feature/req#crit-1")]
    std::fs::write(
        test_dir.join("feature_test.rs"),
        concat!(
            "#[test]\n",
            "#[verifies(\"my-feature/req#crit-1\")]\n",
            "fn test_feature() { assert!(true); }\n",
        ),
    )
    .unwrap();

    // A requirement doc with one criterion and no explicit VerifiedBy
    let doc = SpecDocument {
        path: dir.path().join("specs/my-feature.req.md"),
        frontmatter: Frontmatter {
            id: "my-feature/req".into(),
            doc_type: Some("requirements".into()),
            status: Some("implemented".into()),
        },
        extra: HashMap::new(),
        components: vec![ExtractedComponent {
            name: "Criterion".into(),
            attributes: HashMap::from([("id".into(), "crit-1".into())]),
            children: vec![],
            body_text: Some("The feature shall work".into()),
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: supersigil_core::SourcePosition {
                byte_offset: 0,
                line: 5,
                column: 1,
            },
            end_position: supersigil_core::SourcePosition {
                byte_offset: 0,
                line: 5,
                column: 1,
            },
        }],
    };

    let mut config = Config {
        paths: Some(vec!["specs/**/*.md".into()]),
        tests: Some(vec!["tests/**/*.rs".into()]),
        ecosystem: EcosystemConfig {
            plugins: vec!["rust".into()],
            rust: None,
        },
        ..Config::default()
    };
    config.ecosystem.rust = Some(supersigil_core::RustEcosystemConfig {
        validation: supersigil_core::RustValidationPolicy::Dev,
        project_scope: vec![],
    });

    let graph = build_graph(vec![doc], &config).unwrap();
    let project_root = dir.path().to_path_buf();

    // Run the same pipeline as run_verify_and_publish
    let inputs = VerifyInputs::resolve(&config, &project_root);
    let (artifact_graph, _plugin_findings) =
        supersigil_verify::plugins::build_evidence(&config, &graph, &project_root, None, &inputs);

    // Plugin should find evidence from the #[verifies] attribute
    assert!(
        !artifact_graph.evidence.is_empty(),
        "Rust plugin should discover evidence from #[verifies] attribute"
    );

    // The criterion should be covered via plugin evidence
    assert!(
        artifact_graph.has_evidence("my-feature/req", "crit-1"),
        "crit-1 should be covered by Rust plugin evidence"
    );

    // Verify should produce no error-level findings for this criterion
    let options = VerifyOptions::default();
    let report = verify(&graph, &config, &project_root, &options, &artifact_graph)
        .expect("verify should succeed");

    let errors: Vec<_> = report
        .findings
        .iter()
        .filter(|f| {
            f.effective_severity == supersigil_verify::ReportSeverity::Error
                && f.doc_id.as_deref() == Some("my-feature/req")
        })
        .collect();
    assert!(
        errors.is_empty(),
        "no error-level findings expected when plugin evidence covers the criterion, got: {errors:?}"
    );
}

fn test_client_socket() -> ClientSocket {
    let captured = StdArc::new(Mutex::new(None));
    let captured_for_builder = StdArc::clone(&captured);

    let (_main_loop, _socket) = async_lsp::MainLoop::new_server(move |client| {
        *captured_for_builder.lock().unwrap() = Some(client.clone());
        Router::<(), ResponseError>::new(())
    });

    captured
        .lock()
        .unwrap()
        .clone()
        .expect("builder should capture client socket")
}

fn test_state(root: &Path) -> SupersigilLsp {
    SupersigilLsp {
        client: test_client_socket(),
        config: Some(Config {
            paths: Some(vec!["specs/**/*.md".into()]),
            ..Config::default()
        }),
        project_root: Some(root.to_path_buf()),

        open_files: HashMap::new(),
        file_parses: HashMap::new(),
        partial_file_parses: HashMap::new(),
        graph: Arc::new(empty_graph()),
        component_defs: Arc::new(ComponentDefs::defaults()),
        file_diagnostics: HashMap::new(),
        graph_diagnostics: HashMap::new(),
        evidence_by_target: None,
        evidence_records: None,
        explorer_revision: 0,
        last_explorer_snapshot: None,
        last_explorer_detail_fingerprints: None,
        providers: Vec::new(),
    }
}

/// Helper: build a minimal `SupersigilLsp` for code-action unit tests.
fn test_server() -> SupersigilLsp {
    SupersigilLsp {
        client: ClientSocket::new_closed(),
        config: Some(Config::default()),
        project_root: Some(PathBuf::from("/tmp/project")),

        open_files: HashMap::new(),
        file_parses: HashMap::new(),
        partial_file_parses: HashMap::new(),
        graph: Arc::new(empty_graph()),
        component_defs: Arc::new(ComponentDefs::defaults()),
        file_diagnostics: HashMap::new(),
        graph_diagnostics: HashMap::new(),
        evidence_by_target: None,
        evidence_records: None,
        explorer_revision: 0,
        last_explorer_snapshot: None,
        last_explorer_detail_fingerprints: None,
        providers: Vec::new(),
    }
}

fn indexed_doc(state: &mut SupersigilLsp, abs_path: &Path, rel_path: &Path) {
    let parse = supersigil_parser::parse_file(abs_path, &state.component_defs)
        .expect("fixture should parse");
    let ParseResult::Document(doc) = parse else {
        panic!("fixture should be a supersigil document");
    };
    state.file_parses.insert(rel_path.to_path_buf(), doc);
}

fn set_graph_from_indexed_doc(state: &mut SupersigilLsp, rel_path: &Path) {
    let doc = state
        .file_parses
        .get(rel_path)
        .expect("indexed doc")
        .clone();
    state.graph = Arc::new(
        supersigil_verify::test_helpers::build_test_graph_with_config(
            vec![doc],
            &Config::default(),
        ),
    );
}

fn make_explorer_evidence_record(
    id: usize,
    doc_id: &str,
    target_id: &str,
    test_name: &str,
) -> VerificationEvidenceRecord {
    make_explorer_evidence_record_with_provenance(
        id,
        doc_id,
        target_id,
        test_name,
        vec![supersigil_evidence::PluginProvenance::RustAttribute {
            attribute_span: supersigil_evidence::SourceLocation {
                file: PathBuf::from("tests/auth_test.rs"),
                line: 3,
                column: 1,
            },
        }],
    )
}

fn make_explorer_evidence_record_with_provenance(
    id: usize,
    doc_id: &str,
    target_id: &str,
    test_name: &str,
    provenance: Vec<supersigil_evidence::PluginProvenance>,
) -> VerificationEvidenceRecord {
    VerificationEvidenceRecord {
        id: EvidenceId::new(id),
        targets: supersigil_evidence::VerificationTargets::single(
            supersigil_evidence::VerifiableRef {
                doc_id: doc_id.into(),
                target_id: target_id.into(),
            },
        ),
        test: supersigil_evidence::TestIdentity {
            file: PathBuf::from("tests/auth_test.rs"),
            name: test_name.into(),
            kind: supersigil_evidence::TestKind::Unit,
        },
        source_location: supersigil_evidence::SourceLocation {
            file: PathBuf::from("tests/auth_test.rs"),
            line: 3,
            column: 1,
        },
        provenance,
        metadata: std::collections::BTreeMap::new(),
    }
}

fn document_symbol_params(uri: Url) -> DocumentSymbolParams {
    DocumentSymbolParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn nested_symbols(response: Option<DocumentSymbolResponse>) -> Vec<lsp_types::DocumentSymbol> {
    let Some(DocumentSymbolResponse::Nested(symbols)) = response else {
        panic!("expected nested document symbols");
    };
    symbols
}

fn requirement_doc(doc_id: &str, criterion_id: &str, text: &str) -> String {
    format!(
        "\
---
supersigil:
  id: {doc_id}
  type: requirements
  status: draft
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"{criterion_id}\">{text}</Criterion>
</AcceptanceCriteria>
```
"
    )
}

fn child_symbol_names(response: Option<DocumentSymbolResponse>) -> Vec<String> {
    let symbols = nested_symbols(response);
    symbols[0]
        .children
        .as_ref()
        .expect("nested child symbols")
        .iter()
        .map(|symbol| symbol.name.clone())
        .collect()
}

fn graph_diagnostic_kind(
    diagnostic: &Diagnostic,
) -> Option<crate::diagnostics::GraphDiagnosticKind> {
    let data = serde_json::from_value::<DiagnosticData>(diagnostic.data.as_ref()?.clone()).ok()?;
    let crate::diagnostics::DiagnosticSource::Graph(kind) = data.source else {
        return None;
    };
    Some(kind)
}

#[test]
fn document_symbol_falls_back_to_disk_for_indexed_closed_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: draft
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">ok</Criterion>
</AcceptanceCriteria>
```
",
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);

    let uri = Url::from_file_path(&abs_path).unwrap();
    let response = block_on(state.document_symbol(document_symbol_params(uri))).unwrap();
    let symbols = nested_symbols(response);

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "AcceptanceCriteria");
    let children = symbols[0].children.as_ref().expect("nested criterion");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "auth-1");
}

#[test]
fn document_symbol_uses_partial_parse_for_invalid_open_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/partial.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        "\
---
supersigil:
  id: partial/req
  type: requirements
  status: draft
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"disk-ok\">ok</Criterion>
</AcceptanceCriteria>
```
",
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);

    let uri = Url::from_file_path(&abs_path).unwrap();
    let invalid_buffer = "\
---
supersigil:
  id: partial/req
  type: requirements
  status: draft
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion>broken</Criterion>
  <Criterion id=\"ok-1\">ok</Criterion>
</AcceptanceCriteria>
```
";

    let _ = state.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "markdown".into(),
            version: 1,
            text: invalid_buffer.into(),
        },
    });

    let response = block_on(state.document_symbol(document_symbol_params(uri))).unwrap();
    let symbols = nested_symbols(response);

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "AcceptanceCriteria");
    let children = symbols[0].children.as_ref().expect("nested criteria");
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name, "Criterion");
    assert_eq!(children[1].name, "ok-1");
}

#[test]
fn document_symbol_uses_partial_parse_after_watch_reload_for_invalid_open_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/partial.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        requirement_doc("partial/req", "disk-1", "disk before reload"),
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);

    let uri = Url::from_file_path(&abs_path).unwrap();
    let invalid_buffer = "\
---
supersigil:
  id: partial/req
  type: requirements
  status: draft
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion>broken</Criterion>
  <Criterion id=\"buffer-1\">buffer</Criterion>
</AcceptanceCriteria>
```
";

    let _ = state.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "markdown".into(),
            version: 1,
            text: invalid_buffer.into(),
        },
    });

    std::fs::write(
        &abs_path,
        requirement_doc("partial/req", "disk-2", "disk after reload"),
    )
    .unwrap();

    let _ = state.did_change_watched_files(lsp_types::DidChangeWatchedFilesParams {
        changes: vec![lsp_types::FileEvent {
            uri: uri.clone(),
            typ: lsp_types::FileChangeType::CHANGED,
        }],
    });

    assert_eq!(
        child_symbol_names(
            block_on(state.document_symbol(document_symbol_params(uri)))
                .expect("document symbols after watch reload")
        ),
        vec!["Criterion".to_owned(), "buffer-1".to_owned()],
    );
}

#[test]
fn document_components_use_partial_parse_after_watch_reload_for_invalid_open_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/partial.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        requirement_doc("partial/req", "disk-1", "disk before reload"),
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);

    let uri = Url::from_file_path(&abs_path).unwrap();
    let invalid_buffer = "\
---
supersigil:
  id: partial/req
  type: requirements
  status: draft
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion>broken</Criterion>
  <Criterion id=\"buffer-1\">buffer</Criterion>
</AcceptanceCriteria>
```
";

    let _ = state.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "markdown".into(),
            version: 1,
            text: invalid_buffer.into(),
        },
    });

    std::fs::write(
        &abs_path,
        requirement_doc("partial/req", "disk-2", "disk after reload"),
    )
    .unwrap();

    let _ = state.did_change_watched_files(lsp_types::DidChangeWatchedFilesParams {
        changes: vec![lsp_types::FileEvent {
            uri: uri.clone(),
            typ: lsp_types::FileChangeType::CHANGED,
        }],
    });

    let result = block_on(state.handle_document_components(
        crate::document_components::DocumentComponentsParams {
            uri: uri.to_string(),
        },
    ))
    .unwrap();

    assert!(result.stale);
    assert_eq!(result.document_id, "partial/req");
    let children = &result.fences[0].components[0].children;
    assert_eq!(children[0].id, None);
    assert_eq!(children[1].id.as_deref(), Some("buffer-1"));
}

#[test]
fn did_close_restores_disk_parse_after_unsaved_buffer_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(&abs_path, requirement_doc("auth/req", "disk-1", "disk")).unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    let uri = Url::from_file_path(&abs_path).unwrap();

    let _ = state.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "markdown".into(),
            version: 1,
            text: requirement_doc("auth/req", "buffer-1", "buffer"),
        },
    });

    assert_eq!(
        child_symbol_names(
            block_on(state.document_symbol(document_symbol_params(uri.clone())))
                .expect("document symbols for open buffer")
        ),
        vec!["buffer-1".to_owned()],
    );

    let _ = state.did_close(DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
    });

    assert_eq!(
        child_symbol_names(
            block_on(state.document_symbol(document_symbol_params(uri.clone())))
                .expect("document symbols after close")
        ),
        vec!["disk-1".to_owned()],
    );
    assert!(!state.open_files.contains_key(&uri));
}

#[test]
fn did_change_watched_files_removes_deleted_open_buffer_before_merge() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let deleted_rel_path = PathBuf::from("specs/deleted.req.md");
    let deleted_abs_path = root.join(&deleted_rel_path);
    let replacement_rel_path = PathBuf::from("specs/renamed.req.md");
    let replacement_abs_path = root.join(&replacement_rel_path);
    std::fs::create_dir_all(deleted_abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &deleted_abs_path,
        requirement_doc("renamed/req", "deleted-1", "deleted"),
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &deleted_abs_path, &deleted_rel_path);
    let deleted_uri = Url::from_file_path(&deleted_abs_path).unwrap();
    let replacement_uri = Url::from_file_path(&replacement_abs_path).unwrap();

    let _ = state.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: deleted_uri.clone(),
            language_id: "markdown".into(),
            version: 1,
            text: requirement_doc("renamed/req", "stale-1", "stale buffer"),
        },
    });
    state
        .file_diagnostics
        .insert(deleted_uri.clone(), vec![lsp_types::Diagnostic::default()]);

    std::fs::remove_file(&deleted_abs_path).unwrap();
    std::fs::write(
        &replacement_abs_path,
        requirement_doc("renamed/req", "replacement-1", "replacement"),
    )
    .unwrap();

    let _ = state.did_change_watched_files(lsp_types::DidChangeWatchedFilesParams {
        changes: vec![
            lsp_types::FileEvent {
                uri: deleted_uri.clone(),
                typ: lsp_types::FileChangeType::DELETED,
            },
            lsp_types::FileEvent {
                uri: replacement_uri.clone(),
                typ: lsp_types::FileChangeType::CREATED,
            },
        ],
    });

    assert_eq!(
        child_symbol_names(
            block_on(state.document_symbol(document_symbol_params(replacement_uri.clone())))
                .expect("replacement document symbols")
        ),
        vec!["replacement-1".to_owned()],
    );
    assert!(!state.file_parses.contains_key(&deleted_rel_path));
    assert_eq!(
        state.build_id_to_path().get("renamed/req"),
        Some(&replacement_abs_path)
    );

    let duplicate_diagnostics: Vec<_> = state
        .graph_diagnostics
        .values()
        .flat_map(|diagnostics| diagnostics.iter())
        .filter(|diagnostic| {
            matches!(
                graph_diagnostic_kind(diagnostic),
                Some(crate::diagnostics::GraphDiagnosticKind::DuplicateDocumentId)
            )
        })
        .collect();
    assert!(
        duplicate_diagnostics.is_empty(),
        "rename replacement should not leave duplicate-ID diagnostics behind: {duplicate_diagnostics:?}"
    );
    assert!(!state.open_files.contains_key(&deleted_uri));
    assert!(!state.file_diagnostics.contains_key(&deleted_uri));
}

#[test]
#[verifies("spec-rendering/req#req-1-1")]
fn document_components_normalize_open_crlf_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/crlf.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();

    let crlf_buffer = "\
---\r
supersigil:\r
  id: crlf/req\r
  type: requirements\r
  status: approved\r
---\r
\r
```supersigil-xml\r
<AcceptanceCriteria>\r
  <Criterion id=\"c1\">ok</Criterion>\r
</AcceptanceCriteria>\r
```\r
";
    std::fs::write(&abs_path, crlf_buffer).unwrap();

    let mut state = test_state(root);
    let uri = Url::from_file_path(&abs_path).unwrap();

    let _ = state.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "markdown".into(),
            version: 1,
            text: crlf_buffer.into(),
        },
    });

    let stored = state
        .open_files
        .get(&uri)
        .expect("open file should be tracked");
    assert!(
        !stored.contains('\r'),
        "open buffer should be normalized before reparsing",
    );

    let result = block_on(state.handle_document_components(
        crate::document_components::DocumentComponentsParams {
            uri: uri.to_string(),
        },
    ))
    .unwrap();

    assert_eq!(result.document_id, "crlf/req");
    assert_eq!(result.fences.len(), 1);
    assert_eq!(result.fences[0].components.len(), 1);
    let root_component = &result.fences[0].components[0];
    assert_eq!(root_component.kind, "AcceptanceCriteria");
    assert_eq!(root_component.children.len(), 1);
    assert_eq!(root_component.children[0].kind, "Criterion");
}

#[test]
fn current_explorer_document_fingerprints_mark_detail_only_content_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();

    let initial = "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
```
";
    std::fs::write(&abs_path, initial).unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);

    let initial_snapshot = state.build_explorer_snapshot_for_revision("1");
    let initial_fingerprints = state.current_explorer_document_fingerprints(&initial_snapshot);

    let shifted = "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---


```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
```
";
    std::fs::write(&abs_path, shifted).unwrap();
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);

    let shifted_snapshot = state.build_explorer_snapshot_for_revision("2");
    let shifted_fingerprints = state.current_explorer_document_fingerprints(&shifted_snapshot);

    assert!(
        crate::explorer_runtime::diff_explorer_snapshots(
            Some(&initial_snapshot),
            &shifted_snapshot
        )
        .changed_document_ids
        .is_empty()
    );
    assert_eq!(
        crate::explorer_runtime::diff_explorer_documents(
            Some(&initial_fingerprints),
            &shifted_fingerprints,
        ),
        ["auth/req".to_owned()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
}

#[test]
fn current_explorer_document_fingerprints_mark_evidence_detail_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
```
",
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);
    state.evidence_by_target = Some(Arc::new(HashMap::from([(
        "auth/req".to_owned(),
        HashMap::from([("auth-1".to_owned(), vec![EvidenceId::new(0)])]),
    )])));
    state.evidence_records = Some(Arc::new(vec![make_explorer_evidence_record(
        0,
        "auth/req",
        "auth-1",
        "login_succeeds",
    )]));

    let initial_snapshot = state.build_explorer_snapshot_for_revision("1");
    let initial_fingerprints = state.current_explorer_document_fingerprints(&initial_snapshot);

    state.evidence_records = Some(Arc::new(vec![make_explorer_evidence_record(
        0,
        "auth/req",
        "auth-1",
        "session_refresh_succeeds",
    )]));

    let updated_snapshot = state.build_explorer_snapshot_for_revision("2");
    let updated_fingerprints = state.current_explorer_document_fingerprints(&updated_snapshot);

    assert!(
        crate::explorer_runtime::diff_explorer_snapshots(
            Some(&initial_snapshot),
            &updated_snapshot
        )
        .changed_document_ids
        .is_empty()
    );
    assert_eq!(
        crate::explorer_runtime::diff_explorer_documents(
            Some(&initial_fingerprints),
            &updated_fingerprints,
        ),
        ["auth/req".to_owned()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
}

#[test]
fn current_explorer_document_fingerprints_ignore_equivalent_evidence_id_reassignment() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
```
",
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);

    state.evidence_by_target = Some(Arc::new(HashMap::from([(
        "auth/req".to_owned(),
        HashMap::from([(
            "auth-1".to_owned(),
            vec![EvidenceId::new(10), EvidenceId::new(3)],
        )]),
    )])));
    state.evidence_records = Some(Arc::new(vec![
        make_explorer_evidence_record(10, "auth/req", "auth-1", "login_succeeds"),
        make_explorer_evidence_record(3, "auth/req", "auth-1", "session_refresh_succeeds"),
    ]));

    let initial_snapshot = state.build_explorer_snapshot_for_revision("1");
    let initial_fingerprints = state.current_explorer_document_fingerprints(&initial_snapshot);

    state.evidence_by_target = Some(Arc::new(HashMap::from([(
        "auth/req".to_owned(),
        HashMap::from([(
            "auth-1".to_owned(),
            vec![EvidenceId::new(5), EvidenceId::new(9)],
        )]),
    )])));
    state.evidence_records = Some(Arc::new(vec![
        make_explorer_evidence_record(9, "auth/req", "auth-1", "session_refresh_succeeds"),
        make_explorer_evidence_record(5, "auth/req", "auth-1", "login_succeeds"),
    ]));

    let updated_snapshot = state.build_explorer_snapshot_for_revision("2");
    let updated_fingerprints = state.current_explorer_document_fingerprints(&updated_snapshot);

    assert!(
        crate::explorer_runtime::diff_explorer_snapshots(
            Some(&initial_snapshot),
            &updated_snapshot
        )
        .changed_document_ids
        .is_empty()
    );
    assert!(
        crate::explorer_runtime::diff_explorer_documents(
            Some(&initial_fingerprints),
            &updated_fingerprints,
        )
        .is_empty()
    );
}

#[test]
fn current_explorer_document_fingerprints_ignore_equivalent_provenance_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
```
",
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);
    state.evidence_by_target = Some(Arc::new(HashMap::from([(
        "auth/req".to_owned(),
        HashMap::from([("auth-1".to_owned(), vec![EvidenceId::new(0)])]),
    )])));

    let tag_provenance = supersigil_evidence::PluginProvenance::VerifiedByTag {
        doc_id: "auth/design".into(),
        tag: "auth:login".into(),
    };
    let rust_provenance = supersigil_evidence::PluginProvenance::RustAttribute {
        attribute_span: supersigil_evidence::SourceLocation {
            file: PathBuf::from("tests/auth_test.rs"),
            line: 7,
            column: 1,
        },
    };

    state.evidence_records = Some(Arc::new(vec![
        make_explorer_evidence_record_with_provenance(
            0,
            "auth/req",
            "auth-1",
            "login_succeeds",
            vec![tag_provenance.clone(), rust_provenance.clone()],
        ),
    ]));

    let initial_snapshot = state.build_explorer_snapshot_for_revision("1");
    let initial_fingerprints = state.current_explorer_document_fingerprints(&initial_snapshot);

    state.evidence_records = Some(Arc::new(vec![
        make_explorer_evidence_record_with_provenance(
            0,
            "auth/req",
            "auth-1",
            "login_succeeds",
            vec![rust_provenance, tag_provenance],
        ),
    ]));

    let updated_snapshot = state.build_explorer_snapshot_for_revision("2");
    let updated_fingerprints = state.current_explorer_document_fingerprints(&updated_snapshot);

    assert!(
        crate::explorer_runtime::diff_explorer_snapshots(
            Some(&initial_snapshot),
            &updated_snapshot
        )
        .changed_document_ids
        .is_empty()
    );
    assert!(
        crate::explorer_runtime::diff_explorer_documents(
            Some(&initial_fingerprints),
            &updated_fingerprints,
        )
        .is_empty()
    );
}

fn make_code_action_params(diagnostics: Vec<Diagnostic>) -> CodeActionParams {
    CodeActionParams {
        text_document: lsp_types::TextDocumentIdentifier {
            uri: Url::parse("file:///tmp/project/spec.md").unwrap(),
        },
        range: lsp_types::Range::default(),
        context: lsp_types::CodeActionContext {
            diagnostics,
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    }
}

#[test]
fn code_action_returns_empty_with_no_providers() {
    let mut server = test_server();
    let params = make_code_action_params(vec![]);
    let result = futures::executor::block_on(server.code_action(params)).unwrap();
    assert_eq!(result.unwrap().len(), 0);
}

#[verifies("lsp-code-actions/req#req-2-3")]
#[test]
fn code_action_skips_diagnostic_without_data() {
    let mut server = test_server();

    // A diagnostic with no `data` field.
    let diag = Diagnostic {
        range: lsp_types::Range::default(),
        severity: Some(lsp_types::DiagnosticSeverity::WARNING),
        message: "some warning".into(),
        data: None,
        ..Diagnostic::default()
    };
    let params = make_code_action_params(vec![diag]);
    let result = futures::executor::block_on(server.code_action(params)).unwrap();
    assert_eq!(result.unwrap().len(), 0);
}

#[test]
fn code_action_skips_diagnostic_with_invalid_data() {
    let mut server = test_server();

    // A diagnostic with data that is not a valid DiagnosticData.
    let diag = Diagnostic {
        range: lsp_types::Range::default(),
        severity: Some(lsp_types::DiagnosticSeverity::WARNING),
        message: "some warning".into(),
        data: Some(serde_json::json!({ "unrelated": true })),
        ..Diagnostic::default()
    };
    let params = make_code_action_params(vec![diag]);
    let result = futures::executor::block_on(server.code_action(params)).unwrap();
    assert_eq!(result.unwrap().len(), 0);
}

#[verifies("lsp-code-actions/req#req-2-2", "lsp-code-actions/req#req-2-4")]
#[test]
fn code_action_collects_actions_from_provider() {
    use crate::code_actions::{ActionRequestContext, CodeActionProvider};
    use crate::diagnostics::{ActionContext, DiagnosticSource, ParseDiagnosticKind};

    struct TestProvider;
    impl CodeActionProvider for TestProvider {
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
        ) -> Vec<lsp_types::CodeAction> {
            vec![lsp_types::CodeAction {
                title: "Add missing attribute".into(),
                ..lsp_types::CodeAction::default()
            }]
        }
    }

    let mut server = test_server();
    server.providers.push(Box::new(TestProvider));

    let data = DiagnosticData {
        source: DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute),
        doc_id: None,
        context: ActionContext::MissingAttribute {
            component: "Task".into(),
            attribute: "id".into(),
        },
    };
    let diag = Diagnostic {
        range: lsp_types::Range::default(),
        severity: Some(lsp_types::DiagnosticSeverity::WARNING),
        message: "missing attribute".into(),
        data: Some(serde_json::to_value(&data).unwrap()),
        ..Diagnostic::default()
    };
    let params = make_code_action_params(vec![diag.clone()]);
    let result = futures::executor::block_on(server.code_action(params))
        .unwrap()
        .unwrap();

    assert_eq!(result.len(), 1);
    let CodeActionOrCommand::CodeAction(action) = &result[0] else {
        panic!("expected CodeAction, got Command");
    };
    assert_eq!(action.title, "Add missing attribute");
    assert_eq!(action.kind, Some(lsp_types::CodeActionKind::QUICKFIX));
    assert_eq!(action.diagnostics.as_ref().unwrap().len(), 1);
    assert_eq!(
        action.diagnostics.as_ref().unwrap()[0].message,
        diag.message
    );
}

#[test]
fn code_action_skips_provider_that_does_not_handle() {
    use crate::code_actions::{ActionRequestContext, CodeActionProvider};
    use crate::diagnostics::{ActionContext, DiagnosticSource, ParseDiagnosticKind};

    struct NeverHandles;
    impl CodeActionProvider for NeverHandles {
        fn handles(&self, _data: &DiagnosticData) -> bool {
            false
        }

        fn actions(
            &self,
            _diagnostic: &Diagnostic,
            _data: &DiagnosticData,
            _ctx: &ActionRequestContext,
        ) -> Vec<lsp_types::CodeAction> {
            panic!("should not be called");
        }
    }

    let mut server = test_server();
    server.providers.push(Box::new(NeverHandles));

    let data = DiagnosticData {
        source: DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError),
        doc_id: None,
        context: ActionContext::None,
    };
    let diag = Diagnostic {
        range: lsp_types::Range::default(),
        severity: Some(lsp_types::DiagnosticSeverity::WARNING),
        message: "xml error".into(),
        data: Some(serde_json::to_value(&data).unwrap()),
        ..Diagnostic::default()
    };
    let params = make_code_action_params(vec![diag]);
    let result = futures::executor::block_on(server.code_action(params)).unwrap();
    assert_eq!(result.unwrap().len(), 0);
}

#[verifies("lsp-code-actions/req#req-2-1", "lsp-server/req#req-5-1")]
#[test]
fn initialize_advertises_code_action_provider() {
    let mut server = SupersigilLsp {
        client: ClientSocket::new_closed(),
        config: None,
        project_root: None,

        open_files: HashMap::new(),
        file_parses: HashMap::new(),
        partial_file_parses: HashMap::new(),
        graph: Arc::new(empty_graph()),
        component_defs: Arc::new(ComponentDefs::defaults()),
        file_diagnostics: HashMap::new(),
        graph_diagnostics: HashMap::new(),
        evidence_by_target: None,
        evidence_records: None,
        explorer_revision: 0,
        last_explorer_snapshot: None,
        last_explorer_detail_fingerprints: None,
        providers: Vec::new(),
    };

    // Create a temp dir with supersigil.toml so capabilities are full.
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("supersigil.toml"), "").unwrap();

    #[allow(
        deprecated,
        reason = "root_uri is the standard field for this lsp-types version"
    )]
    let params = InitializeParams {
        root_uri: Some(Url::from_directory_path(tmp.path()).unwrap()),
        ..Default::default()
    };

    let result = futures::executor::block_on(server.initialize(params)).unwrap();

    match result.capabilities.code_action_provider {
        Some(lsp_types::CodeActionProviderCapability::Options(opts)) => {
            let kinds = opts.code_action_kinds.unwrap();
            assert!(kinds.contains(&lsp_types::CodeActionKind::QUICKFIX));
        }
        other => panic!("expected CodeActionOptions with quickfix kind, got {other:?}"),
    }
}

#[test]
fn initialize_returns_server_info_with_version() {
    let mut server = SupersigilLsp {
        client: ClientSocket::new_closed(),
        config: None,
        project_root: None,

        open_files: HashMap::new(),
        file_parses: HashMap::new(),
        partial_file_parses: HashMap::new(),
        graph: Arc::new(empty_graph()),
        component_defs: Arc::new(ComponentDefs::defaults()),
        file_diagnostics: HashMap::new(),
        graph_diagnostics: HashMap::new(),
        evidence_by_target: None,
        evidence_records: None,
        explorer_revision: 0,
        last_explorer_snapshot: None,
        last_explorer_detail_fingerprints: None,
        providers: Vec::new(),
    };

    let params = InitializeParams::default();
    let result = block_on(server.initialize(params)).unwrap();

    let info = result
        .server_info
        .expect("InitializeResult should include server_info");
    assert_eq!(info.name, "supersigil-lsp");
    let version = info.version.expect("server_info should include version");
    assert!(!version.is_empty(), "version should not be empty");
}

#[verifies("lsp-code-actions/req#req-3-3")]
#[test]
fn all_providers_iterated_for_each_diagnostic() {
    use crate::code_actions::{ActionRequestContext, CodeActionProvider};
    use crate::diagnostics::{ActionContext, DiagnosticSource, ParseDiagnosticKind};

    /// Provider A: does not handle the diagnostic.
    struct ProviderA;
    impl CodeActionProvider for ProviderA {
        fn handles(&self, _data: &DiagnosticData) -> bool {
            false
        }
        fn actions(
            &self,
            _diagnostic: &Diagnostic,
            _data: &DiagnosticData,
            _ctx: &ActionRequestContext,
        ) -> Vec<lsp_types::CodeAction> {
            panic!("should not be called when handles returns false");
        }
    }

    /// Provider B: handles the diagnostic and returns one action.
    struct ProviderB;
    impl CodeActionProvider for ProviderB {
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
        ) -> Vec<lsp_types::CodeAction> {
            vec![lsp_types::CodeAction {
                title: "Fix from provider B".into(),
                ..lsp_types::CodeAction::default()
            }]
        }
    }

    let mut server = test_server();
    // Register providers in order: A first, then B.
    // The handler must iterate past A (which doesn't handle) to reach B.
    server.providers.push(Box::new(ProviderA));
    server.providers.push(Box::new(ProviderB));

    let data = DiagnosticData {
        source: DiagnosticSource::Parse(ParseDiagnosticKind::MissingRequiredAttribute),
        doc_id: None,
        context: ActionContext::MissingAttribute {
            component: "Task".into(),
            attribute: "id".into(),
        },
    };
    let diag = Diagnostic {
        range: lsp_types::Range::default(),
        severity: Some(lsp_types::DiagnosticSeverity::WARNING),
        message: "missing attribute".into(),
        data: Some(serde_json::to_value(&data).unwrap()),
        ..Diagnostic::default()
    };
    let params = make_code_action_params(vec![diag]);
    let result = futures::executor::block_on(server.code_action(params))
        .unwrap()
        .unwrap();

    // Provider B's action is returned, proving iteration reached all providers.
    assert_eq!(result.len(), 1);
    let CodeActionOrCommand::CodeAction(action) = &result[0] else {
        panic!("expected CodeAction, got Command");
    };
    assert_eq!(action.title, "Fix from provider B");
}

#[test]
#[verifies("graph-explorer-runtime/req#req-1-1")]
fn execute_command_returns_explorer_snapshot_payload() {
    let content = "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
```
";
    let defs = ComponentDefs::defaults();
    let recovered = supersigil_parser::parse_content_recovering(
        std::path::Path::new("specs/auth/req.md"),
        content,
        &defs,
    )
    .expect("parse should succeed");
    let ParseResult::Document(doc) = recovered.result else {
        panic!("expected supersigil document");
    };

    let dir = tempfile::tempdir().unwrap();
    let mut state = test_state(dir.path());
    state
        .file_parses
        .insert(PathBuf::from("specs/auth/req.md"), doc);
    set_graph_from_indexed_doc(&mut state, Path::new("specs/auth/req.md"));
    state.explorer_revision = 7;

    let result = block_on(state.execute_command(ExecuteCommandParams {
        command: lsp_commands::EXPLORER_SNAPSHOT_COMMAND.to_owned(),
        arguments: Vec::new(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    }))
    .unwrap()
    .expect("command should return a payload");

    let snapshot: crate::explorer_runtime::ExplorerSnapshot =
        serde_json::from_value(result).expect("valid explorer snapshot payload");
    assert_eq!(snapshot.revision, "7");
    assert_eq!(snapshot.documents.len(), 1);
    assert_eq!(snapshot.documents[0].id, "auth/req");
}

#[test]
fn execute_command_returns_document_list_payload() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(&abs_path, requirement_doc("auth/req", "auth-1", "ok")).unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);

    let result = block_on(state.execute_command(ExecuteCommandParams {
        command: lsp_commands::DOCUMENT_LIST_COMMAND.to_owned(),
        arguments: Vec::new(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    }))
    .unwrap()
    .expect("command should return a payload");

    let document_list: crate::document_list::DocumentListResult =
        serde_json::from_value(result).expect("valid document list payload");
    assert_eq!(document_list.documents.len(), 1);
    assert_eq!(document_list.documents[0].id, "auth/req");
    assert_eq!(document_list.documents[0].doc_type, "requirements");
    assert_eq!(document_list.documents[0].status.as_deref(), Some("draft"));
    assert_eq!(document_list.documents[0].path, "specs/auth.req.md");
    assert_eq!(document_list.documents[0].project, None);
}

#[test]
fn execute_command_returns_document_components_payload() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        requirement_doc("auth/req", "auth-1", "Users SHALL authenticate."),
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);
    let uri = Url::from_file_path(&abs_path).unwrap();

    let result = block_on(state.execute_command(ExecuteCommandParams {
        command: lsp_commands::DOCUMENT_COMPONENTS_COMMAND.to_owned(),
        arguments: vec![serde_json::json!(uri.to_string())],
        work_done_progress_params: WorkDoneProgressParams::default(),
    }))
    .unwrap()
    .expect("command should return a payload");

    let components: crate::document_components::DocumentComponentsResult =
        serde_json::from_value(result).expect("valid document components payload");
    assert_eq!(components.document_id, "auth/req");
    assert!(!components.stale);
    assert_eq!(components.fences.len(), 1);
    assert_eq!(components.fences[0].components.len(), 1);
    let root_component = &components.fences[0].components[0];
    assert_eq!(root_component.kind, "AcceptanceCriteria");
    assert_eq!(root_component.children.len(), 1);
    assert_eq!(root_component.children[0].kind, "Criterion");
}

#[test]
fn execute_command_returns_none_for_unknown_command() {
    let dir = tempfile::tempdir().unwrap();
    let state = &mut test_state(dir.path());

    let result = block_on(state.execute_command(ExecuteCommandParams {
        command: "supersigil.unknown".to_owned(),
        arguments: Vec::new(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    }))
    .unwrap();

    assert!(result.is_none());
}

#[test]
fn execute_command_verify_removes_stale_graph_diagnostic_entries() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---
",
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);

    let stale_uri = Url::parse("file:///tmp/stale.md").unwrap();
    state
        .graph_diagnostics
        .insert(stale_uri.clone(), vec![lsp_types::Diagnostic::default()]);

    let result = block_on(state.execute_command(ExecuteCommandParams {
        command: lsp_commands::VERIFY_COMMAND.to_owned(),
        arguments: Vec::new(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    }))
    .unwrap();

    assert!(result.is_none());
    assert!(!state.graph_diagnostics.contains_key(&stale_uri));
}

#[test]
fn did_save_removes_stale_graph_diagnostic_entries() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---
",
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);

    let stale_uri = Url::parse("file:///tmp/stale.md").unwrap();
    state
        .graph_diagnostics
        .insert(stale_uri.clone(), vec![lsp_types::Diagnostic::default()]);

    let _ = state.did_save(DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&abs_path).unwrap(),
        },
        text: None,
    });

    assert!(!state.graph_diagnostics.contains_key(&stale_uri));
}

#[test]
fn did_save_skips_unindexed_non_project_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---
",
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);

    let stale_uri = Url::parse("file:///tmp/stale.md").unwrap();
    state
        .graph_diagnostics
        .insert(stale_uri.clone(), vec![lsp_types::Diagnostic::default()]);

    let unrelated_uri = Url::from_file_path(root.join("notes/unrelated.md")).unwrap();
    let _ = state.did_save(DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: unrelated_uri },
        text: None,
    });

    assert!(state.graph_diagnostics.contains_key(&stale_uri));
}

#[test]
#[verifies("graph-explorer-runtime/req#req-1-2")]
fn execute_command_returns_explorer_document_payload() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rel_path = PathBuf::from("specs/auth.req.md");
    let abs_path = root.join(&rel_path);
    std::fs::create_dir_all(abs_path.parent().unwrap()).unwrap();
    std::fs::write(
        &abs_path,
        "\
---
supersigil:
  id: auth/req
  type: requirements
  status: approved
---

```supersigil-xml
<AcceptanceCriteria>
  <Criterion id=\"auth-1\">Users SHALL authenticate.</Criterion>
</AcceptanceCriteria>
```
",
    )
    .unwrap();

    let mut state = test_state(root);
    indexed_doc(&mut state, &abs_path, &rel_path);
    set_graph_from_indexed_doc(&mut state, &rel_path);
    state.explorer_revision = 12;

    let result = block_on(state.execute_command(ExecuteCommandParams {
        command: lsp_commands::EXPLORER_DOCUMENT_COMMAND.to_owned(),
        arguments: vec![serde_json::json!({
            "document_id": "auth/req",
            "revision": "stale-client-rev"
        })],
        work_done_progress_params: WorkDoneProgressParams::default(),
    }))
    .unwrap()
    .expect("command should return a payload");

    let document: crate::explorer_runtime::ExplorerDocument =
        serde_json::from_value(result).expect("valid explorer document payload");
    assert_eq!(document.revision, "12");
    assert_eq!(document.document_id, "auth/req");
    assert_eq!(document.fences.len(), 1);
}
