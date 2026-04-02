use std::sync::{Arc as StdArc, Mutex};

use async_lsp::router::Router;
use futures::executor::block_on;
use lsp_types::{
    PartialResultParams, TextDocumentIdentifier, TextDocumentItem, WorkDoneProgressParams,
};

use super::*;
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
fn discover_config_returns_none_in_temp_dir() {
    let dir = tempfile::tempdir().unwrap();
    assert!(discover_config(dir.path()).is_none());
}

#[test]
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
        warnings: vec![],
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
    let (artifact_graph, plugin_findings) =
        supersigil_verify::build_evidence(&config, &graph, &project_root, None, &inputs);

    // Plugin should find evidence from the #[verifies] attribute
    assert!(
        !artifact_graph.evidence.is_empty(),
        "Rust plugin should discover evidence from #[verifies] attribute"
    );
    assert!(plugin_findings.is_empty(), "expected no plugin failures");

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
        diagnostics_tier: DiagnosticsTier::default(),
        open_files: HashMap::new(),
        file_parses: HashMap::new(),
        partial_file_parses: HashMap::new(),
        graph: Arc::new(empty_graph()),
        component_defs: Arc::new(ComponentDefs::defaults()),
        file_diagnostics: HashMap::new(),
        graph_diagnostics: HashMap::new(),
        evidence_by_target: None,
        providers: Vec::new(),
    }
}

/// Helper: build a minimal `SupersigilLsp` for code-action unit tests.
fn test_server() -> SupersigilLsp {
    SupersigilLsp {
        client: ClientSocket::new_closed(),
        config: Some(Config::default()),
        project_root: Some(PathBuf::from("/tmp/project")),
        diagnostics_tier: DiagnosticsTier::default(),
        open_files: HashMap::new(),
        file_parses: HashMap::new(),
        partial_file_parses: HashMap::new(),
        graph: Arc::new(empty_graph()),
        component_defs: Arc::new(ComponentDefs::defaults()),
        file_diagnostics: HashMap::new(),
        graph_diagnostics: HashMap::new(),
        evidence_by_target: None,
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

#[verifies("lsp-code-actions/req#req-2-1")]
#[test]
fn initialize_advertises_code_action_provider() {
    let mut server = SupersigilLsp {
        client: ClientSocket::new_closed(),
        config: None,
        project_root: None,
        diagnostics_tier: DiagnosticsTier::default(),
        open_files: HashMap::new(),
        file_parses: HashMap::new(),
        partial_file_parses: HashMap::new(),
        graph: Arc::new(empty_graph()),
        component_defs: Arc::new(ComponentDefs::defaults()),
        file_diagnostics: HashMap::new(),
        graph_diagnostics: HashMap::new(),
        evidence_by_target: None,
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
