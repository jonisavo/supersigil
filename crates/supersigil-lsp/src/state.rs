//! Server state, initialization, and helper functions.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_lsp::{ClientSocket, LanguageServer, ResponseError};
use futures::future::BoxFuture;
use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DidChangeConfigurationParams, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentSymbolParams, DocumentSymbolResponse, ExecuteCommandParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, InitializeParams, InitializeResult, Location,
    MessageType, NumberOrString, PositionEncodingKind, ProgressParams, ProgressParamsValue,
    PublishDiagnosticsParams, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, Url, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressEnd,
};

use supersigil_core::{
    ComponentDefs, Config, DocumentGraph, ParseResult, SpecDocument, build_graph, expand_globs,
    find_config, load_config,
};
use supersigil_evidence::{EvidenceId, VerificationEvidenceRecord};
use supersigil_verify::{VerifyInputs, VerifyOptions, verify};

use crate::code_actions::{ActionRequestContext, CodeActionProvider};
use crate::commands;
use crate::completion;
use crate::definition;
use crate::diagnostics::{
    DiagnosticData, finding_to_diagnostic, graph_error_to_diagnostic_with_lookup, group_by_url,
    parse_error_to_diagnostic,
};
use crate::document_symbols;
use crate::hover;
use crate::position;
use crate::references;
use crate::rename;

// ---------------------------------------------------------------------------
// SupersigilLsp
// ---------------------------------------------------------------------------

/// Evidence index: `doc_id` → `target_id` → evidence IDs.
type EvidenceIndex = HashMap<String, HashMap<String, Vec<EvidenceId>>>;

/// The main language server state.
pub struct SupersigilLsp {
    client: ClientSocket,
    config: Option<Config>,
    project_root: Option<PathBuf>,
    open_files: HashMap<lsp_types::Url, Arc<String>>,
    file_parses: HashMap<PathBuf, SpecDocument>,
    partial_file_parses: HashMap<PathBuf, SpecDocument>,
    graph: Arc<DocumentGraph>,
    component_defs: Arc<ComponentDefs>,
    file_diagnostics: HashMap<Url, Vec<Diagnostic>>,
    graph_diagnostics: HashMap<Url, Vec<Diagnostic>>,
    /// Cached evidence-by-target index from the last verify run.
    /// Keyed by `doc_id` → `target_id` → evidence IDs.
    evidence_by_target: Option<Arc<EvidenceIndex>>,
    /// Cached evidence records from the last verify run.
    evidence_records: Option<Arc<Vec<VerificationEvidenceRecord>>>,
    providers: Vec<Box<dyn CodeActionProvider>>,
}

impl std::fmt::Debug for SupersigilLsp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SupersigilLsp")
            .field("project_root", &self.project_root)
            .field("open_files", &self.open_files.len())
            .field("file_parses", &self.file_parses.len())
            .field("partial_file_parses", &self.partial_file_parses.len())
            .finish_non_exhaustive()
    }
}

impl SupersigilLsp {
    /// Create a new LSP router wired to all supersigil handlers.
    #[must_use]
    pub fn new_router(client: ClientSocket) -> async_lsp::router::Router<Self, ResponseError> {
        let mut router = async_lsp::router::Router::from_language_server(Self {
            client,
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
            providers: vec![
                Box::new(crate::code_actions::MissingAttributeProvider),
                Box::new(crate::code_actions::DuplicateIdProvider),
                Box::new(crate::code_actions::IncompleteDecisionProvider),
                Box::new(crate::code_actions::OrphanDecisionProvider),
                Box::new(crate::code_actions::InvalidPlacementProvider),
                Box::new(crate::code_actions::SequentialIdProvider),
                Box::new(crate::code_actions::BrokenRefProvider),
            ],
        });
        router.request::<crate::document_list::DocumentListRequest, _>(Self::handle_document_list);
        router.request::<crate::graph_data::GraphDataRequest, _>(Self::handle_graph_data);
        router.request::<crate::document_components::DocumentComponentsRequest, _>(
            Self::handle_document_components,
        );
        router
    }

    fn effective_component_defs(config: Option<&Config>) -> ComponentDefs {
        config
            .and_then(|c| {
                ComponentDefs::merge(ComponentDefs::defaults(), c.components.clone()).ok()
            })
            .unwrap_or_else(ComponentDefs::defaults)
    }

    fn publish_merged_diagnostics(&self, uri: &Url) {
        let file = self.file_diagnostics.get(uri).cloned().unwrap_or_default();
        let graph = self.graph_diagnostics.get(uri).cloned().unwrap_or_default();
        let mut merged = file;
        merged.extend(graph);
        let _ = self
            .client
            .notify::<lsp_types::notification::PublishDiagnostics>(PublishDiagnosticsParams {
                uri: uri.clone(),
                diagnostics: merged,
                version: None,
            });
    }

    /// Check whether a URI is strictly under `project_root` (no nested
    /// supersigil roots in between). Returns `false` if the file is inside
    /// a subdirectory that has its own `supersigil.toml`.
    fn uri_is_owned(&self, uri: &lsp_types::Url) -> bool {
        let Some(root) = &self.project_root else {
            return false;
        };
        let Ok(abs) = uri.to_file_path() else {
            return false;
        };
        let Ok(rel) = abs.strip_prefix(root) else {
            return false;
        };
        // Walk up from the file's parent toward root, checking for an
        // intermediate supersigil.toml that would indicate a nested project.
        let mut check = rel;
        while let Some(parent) = check.parent() {
            if parent.as_os_str().is_empty() {
                break;
            }
            if root.join(parent).join("supersigil.toml").is_file() {
                return false; // Nested project owns this file
            }
            check = parent;
        }
        true
    }

    /// Convert a URI to a relative path suitable as a `file_parses` key.
    ///
    /// Returns a path relative to `project_root` (matching how initial
    /// indexing stores keys) so that reparsed documents use the same key
    /// as the original parse.
    fn uri_to_relative_key(&self, uri: &lsp_types::Url) -> Option<PathBuf> {
        let abs = uri.to_file_path().ok()?;
        if let Some(root) = &self.project_root {
            Some(abs.strip_prefix(root).map(Path::to_path_buf).unwrap_or(abs))
        } else {
            Some(abs)
        }
    }

    fn build_id_to_path(&self) -> HashMap<String, PathBuf> {
        let project_root = self.project_root.clone().unwrap_or_default();
        self.file_parses
            .iter()
            .map(|(rel_path, doc)| {
                let abs = if rel_path.is_absolute() {
                    rel_path.clone()
                } else {
                    project_root.join(rel_path)
                };
                (doc.frontmatter.id.clone(), abs)
            })
            .collect()
    }

    /// Check whether a file URI belongs to this supersigil project.
    ///
    /// A file belongs to the project if it was part of the initial indexing
    /// (already in `file_parses`) OR if its path, relative to the project
    /// root, matches one of the configured glob patterns. Files from other
    /// supersigil roots (e.g. worktrees) are rejected.
    fn is_project_file(&self, uri: &Url) -> bool {
        let Some(rel_key) = self.uri_to_relative_key(uri) else {
            return false;
        };
        // Already known from initial indexing.
        if self.file_parses.contains_key(&rel_key) {
            return true;
        }
        // Check against configured globs.
        let Some(config) = &self.config else {
            return false;
        };
        let Some(root) = &self.project_root else {
            return false;
        };
        let abs = root.join(&rel_key);
        let configured_files = discover_files(config, root);
        configured_files.contains(&abs)
    }

    fn reparse_and_publish(&mut self, uri: &Url, content: &str) {
        let Some(rel_key) = self.uri_to_relative_key(uri) else {
            return;
        };
        // Skip files that don't belong to this project (e.g. files from
        // a worktree opened inside the main repo's VS Code workspace).
        if !self.file_parses.contains_key(&rel_key) && !self.is_project_file(uri) {
            return;
        }
        // Parse with the absolute path so SpecDocument.path is absolute
        // (consistent with initial indexing via parse_file).
        let abs_path = match &self.project_root {
            Some(root) => root.join(&rel_key),
            None => rel_key.clone(),
        };
        match supersigil_parser::parse_content_recovering(&abs_path, content, &self.component_defs)
        {
            Ok(recovered) => match recovered.result {
                ParseResult::Document(doc) => {
                    let diags: Vec<Diagnostic> = recovered
                        .fatal_errors
                        .iter()
                        .filter_map(|e| parse_error_to_diagnostic(e, Some(content)))
                        .map(|(_, d)| d)
                        .collect();

                    if recovered.fatal_errors.is_empty() {
                        self.partial_file_parses.remove(&rel_key);
                        self.file_parses.insert(rel_key, doc);
                    } else {
                        self.file_parses.remove(&rel_key);
                        self.partial_file_parses.insert(rel_key, doc);
                    }
                    self.file_diagnostics.insert(uri.clone(), diags);
                }
                ParseResult::NotSupersigil(_) => {
                    self.file_parses.remove(&rel_key);
                    self.partial_file_parses.remove(&rel_key);
                    self.file_diagnostics.insert(uri.clone(), vec![]);
                }
            },
            Err(errs) => {
                self.file_parses.remove(&rel_key);
                self.partial_file_parses.remove(&rel_key);
                let diags: Vec<Diagnostic> = errs
                    .iter()
                    .filter_map(|e| parse_error_to_diagnostic(e, Some(content)))
                    .map(|(_, d)| d)
                    .collect();
                self.file_diagnostics.insert(uri.clone(), diags);
            }
        }
        self.publish_merged_diagnostics(uri);
    }

    fn notify_documents_changed(&self) {
        let _ = self
            .client
            .notify::<crate::document_list::DocumentsChanged>(());
    }

    fn republish_all_diagnostics(&self) {
        let mut uris: Vec<Url> = self.open_files.keys().cloned().collect();
        for uri in self.graph_diagnostics.keys() {
            if !self.open_files.contains_key(uri) {
                uris.push(uri.clone());
            }
        }
        for uri in &uris {
            self.publish_merged_diagnostics(uri);
        }
    }

    fn run_verify_and_publish(&mut self) {
        let Some(config) = &self.config else {
            return;
        };

        let project_root = self.project_root.clone().unwrap_or_default();
        let id_to_path = self.build_id_to_path();
        let options = VerifyOptions::default();

        // Build evidence from both explicit VerifiedBy components and
        // ecosystem plugins (e.g. Rust #[verifies] macros), matching the
        // CLI's full evidence pipeline.
        let inputs = VerifyInputs::resolve(config, &project_root);
        let (artifact_graph, _plugin_findings) = supersigil_verify::plugins::build_evidence(
            config,
            &self.graph,
            &project_root,
            None,
            &inputs,
        );
        self.evidence_by_target = Some(Arc::new(artifact_graph.evidence_by_target.clone()));
        self.evidence_records = Some(Arc::new(artifact_graph.evidence.clone()));

        match verify(
            &self.graph,
            config,
            &project_root,
            &options,
            &artifact_graph,
        ) {
            Ok(report) => {
                let pairs: Vec<(Url, Diagnostic)> = report
                    .findings
                    .iter()
                    .filter_map(|finding| {
                        finding_to_diagnostic(finding, |doc_id| id_to_path.get(doc_id).cloned())
                    })
                    .collect();
                let grouped = group_by_url(pairs);
                for (uri, diags) in grouped {
                    self.graph_diagnostics.insert(uri, diags);
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "verify pipeline failed");
            }
        }
    }

    fn handle_document_list(
        &mut self,
        _params: serde_json::Value,
    ) -> BoxFuture<'static, Result<crate::document_list::DocumentListResult, ResponseError>> {
        let graph = Arc::clone(&self.graph);
        let project_root = self.project_root.clone().unwrap_or_default();

        let documents = crate::document_list::build_document_entries(&graph, &project_root);

        Box::pin(async move { Ok(crate::document_list::DocumentListResult { documents }) })
    }

    fn handle_graph_data(
        &mut self,
        _params: serde_json::Value,
    ) -> BoxFuture<'static, Result<supersigil_verify::graph_json::GraphJson, ResponseError>> {
        let graph = Arc::clone(&self.graph);
        let project_root = self.project_root.clone().unwrap_or_default();

        let result = supersigil_verify::graph_json::build_graph_json(&graph, &project_root);

        Box::pin(async move { Ok(result) })
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "async_lsp Router requires params by value"
    )]
    fn handle_document_components(
        &mut self,
        params: crate::document_components::DocumentComponentsParams,
    ) -> BoxFuture<
        'static,
        Result<crate::document_components::DocumentComponentsResult, ResponseError>,
    > {
        use crate::document_components::{
            BuildComponentsInput, DocumentComponentsResult, build_document_components,
        };

        let empty = || -> BoxFuture<'static, Result<DocumentComponentsResult, ResponseError>> {
            let result = DocumentComponentsResult {
                document_id: String::new(),
                stale: false,
                project: None,
                fences: Vec::new(),
                edges: Vec::new(),
            };
            Box::pin(async move { Ok(result) })
        };

        let Ok(uri) = Url::parse(&params.uri) else {
            return empty();
        };

        // Resolve to relative key for file_parses lookup.
        let Some(rel_key) = self.uri_to_relative_key(&uri) else {
            return empty();
        };

        // Look up the SpecDocument. Prefer file_parses (current), fall back to
        // partial_file_parses (stale).
        let (doc, stale) = if let Some(doc) = self.file_parses.get(&rel_key) {
            (doc.clone(), false)
        } else if let Some(doc) = self.partial_file_parses.get(&rel_key) {
            (doc.clone(), true)
        } else {
            return empty();
        };

        // Get document content for fence extraction.
        // Prefer in-memory content for open files; fall back to disk for unopened ones.
        let content = self.open_files.get(&uri).cloned().unwrap_or_else(|| {
            let path = uri.to_file_path().ok();
            let text = path.and_then(|p| std::fs::read_to_string(p).ok());
            Arc::new(text.unwrap_or_default())
        });

        let graph = Arc::clone(&self.graph);
        let evidence = self.evidence_by_target.clone();
        let records = self.evidence_records.clone();
        let root = self.project_root.clone().unwrap_or_default();

        Box::pin(async move {
            let result = build_document_components(&BuildComponentsInput {
                doc: &doc,
                stale,
                content: &content,
                graph: &graph,
                evidence_by_target: evidence.as_deref(),
                evidence_records: records.as_deref().map(Vec::as_slice),
                project_root: &root,
            });
            Ok(result)
        })
    }

    /// Handle the `supersigil.createDocument` command.
    ///
    /// Prompts the user to pick a project via `window/showMessageRequest`,
    /// then scaffolds a new document file in the chosen project's spec
    /// directory and applies the edit via `workspace/applyEdit`.
    #[allow(
        clippy::too_many_lines,
        reason = "async scaffolding dominates line count"
    )]
    fn execute_create_document(
        &self,
        arguments: &[serde_json::Value],
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, ResponseError>> {
        // Parse command arguments.
        let Some(args) = arguments.first() else {
            return Box::pin(async { Ok(None) });
        };
        let Some(target_ref) = args.get("ref").and_then(|v| v.as_str()) else {
            return Box::pin(async { Ok(None) });
        };
        let Some(feature) = args.get("feature").and_then(|v| v.as_str()) else {
            return Box::pin(async { Ok(None) });
        };
        let Some(full_type) = args.get("type").and_then(|v| v.as_str()) else {
            return Box::pin(async { Ok(None) });
        };

        // Collect project names.
        let project_names: Vec<String> = match &self.config {
            Some(config) => match &config.projects {
                Some(projects) if !projects.is_empty() => {
                    let mut names: Vec<String> = projects.keys().cloned().collect();
                    names.sort();
                    names
                }
                _ => return Box::pin(async { Ok(None) }),
            },
            None => return Box::pin(async { Ok(None) }),
        };

        // Clone data needed for the async block.
        let mut client = self.client.clone();
        let config = self.config.clone().unwrap_or_default();
        let project_root = self.project_root.clone().unwrap_or_default();
        let target_ref = target_ref.to_string();
        let feature = feature.to_string();
        let full_type = full_type.to_string();

        Box::pin(async move {
            use async_lsp::LanguageClient;
            use lsp_types::{
                ApplyWorkspaceEditParams, CreateFile, CreateFileOptions, DocumentChangeOperation,
                DocumentChanges, MessageActionItem, OptionalVersionedTextDocumentIdentifier,
                Position, Range, ResourceOp, ShowMessageRequestParams, TextDocumentEdit, TextEdit,
                WorkspaceEdit,
            };
            use supersigil_core::glob_prefix;
            use supersigil_core::scaffold::generate_template;

            // Ask the user which project to use.
            let params = ShowMessageRequestParams {
                typ: MessageType::INFO,
                message: format!("Which project should '{target_ref}' be created in?"),
                actions: Some(
                    project_names
                        .iter()
                        .map(|name| MessageActionItem {
                            title: name.clone(),
                            properties: HashMap::new(),
                        })
                        .collect(),
                ),
            };

            let response = client.show_message_request(params).await;
            let chosen = match response {
                Ok(Some(item)) => item.title,
                // User dismissed the dialog or request failed — take no action.
                _ => return Ok(None),
            };

            // Resolve the spec directory from the chosen project.
            let Some(projects) = &config.projects else {
                return Ok(None);
            };
            let Some(project_config) = projects.get(&chosen) else {
                return Ok(None);
            };
            let Some(first_pattern) = project_config.paths.first() else {
                return Ok(None);
            };
            let spec_dir = glob_prefix(first_pattern);

            // Derive the short type for file naming.
            let type_short = supersigil_core::scaffold::type_short_name(&full_type);

            let file_rel = format!("{spec_dir}{feature}/{feature}.{type_short}.md");
            let file_path = project_root.join(&file_rel);
            let Some(file_uri) = Url::from_file_path(&file_path).ok() else {
                return Ok(None);
            };

            let content = generate_template(&full_type, &target_ref, &feature, false);

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
                edits: vec![lsp_types::OneOf::Left(TextEdit {
                    range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                    new_text: content,
                })],
            });

            let workspace_edit = WorkspaceEdit {
                document_changes: Some(DocumentChanges::Operations(vec![create_op, insert_op])),
                ..Default::default()
            };

            let _ = client
                .apply_edit(ApplyWorkspaceEditParams {
                    label: Some(format!("Create {target_ref}")),
                    edit: workspace_edit,
                })
                .await;

            Ok(None)
        })
    }
}

// ---------------------------------------------------------------------------
// LanguageServer impl
// ---------------------------------------------------------------------------

impl LanguageServer for SupersigilLsp {
    type Error = ResponseError;
    type NotifyResult = ControlFlow<async_lsp::Result<()>>;

    fn initialize(
        &mut self,
        params: InitializeParams,
    ) -> BoxFuture<'static, Result<InitializeResult, ResponseError>> {
        #[allow(
            deprecated,
            reason = "root_uri is the standard field for this lsp-types version"
        )]
        let root_uri = params.root_uri.as_ref().and_then(|u| u.to_file_path().ok());

        if let Some(root) = root_uri {
            self.project_root = Some(root);
        }

        // Only advertise full capabilities when supersigil.toml is
        // discoverable from the workspace root. Without config the server
        // stays dormant (text sync only) so it does not interfere with
        // other language servers in non-Supersigil workspaces.
        let has_config = self
            .project_root
            .as_ref()
            .and_then(|r| find_config(r).ok().flatten())
            .is_some();

        let text_sync = Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(lsp_types::TextDocumentSyncSaveOptions::Supported(true)),
                ..TextDocumentSyncOptions::default()
            },
        ));

        let capabilities = if has_config {
            ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: text_sync,
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["<".to_owned(), "#".to_owned(), "\"".to_owned()]),
                    ..CompletionOptions::default()
                }),
                hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
                definition_provider: Some(lsp_types::OneOf::Left(true)),
                document_symbol_provider: Some(lsp_types::OneOf::Left(true)),
                references_provider: Some(lsp_types::OneOf::Left(true)),
                rename_provider: Some(lsp_types::OneOf::Right(lsp_types::RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
                })),
                code_lens_provider: Some(lsp_types::CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                code_action_provider: Some(lsp_types::CodeActionProviderCapability::Options(
                    lsp_types::CodeActionOptions {
                        code_action_kinds: Some(vec![lsp_types::CodeActionKind::QUICKFIX]),
                        ..Default::default()
                    },
                )),
                // Note: we intentionally omit execute_command_provider from
                // capabilities. The server still handles workspace/executeCommand
                // requests, but advertising commands here causes vscode-languageclient
                // to auto-register them as VS Code commands, which fails when
                // multiple LSP instances run in a multi-root workspace.
                // The VS Code extension registers commands itself and routes to
                // the appropriate client.
                ..ServerCapabilities::default()
            }
        } else {
            ServerCapabilities {
                text_document_sync: text_sync,
                ..ServerCapabilities::default()
            }
        };

        let result = InitializeResult {
            capabilities,
            server_info: Some(ServerInfo {
                name: "supersigil-lsp".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
        };

        Box::pin(async move { Ok(result) })
    }

    fn initialized(
        &mut self,
        _params: lsp_types::InitializedParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        let Some(root) = self.project_root.clone() else {
            tracing::info!("no workspace root; skipping initial indexing");
            return ControlFlow::Continue(());
        };

        let Some((config_path, config)) = discover_config(&root) else {
            tracing::info!("no supersigil.toml found; skipping initial indexing");
            return ControlFlow::Continue(());
        };

        let project_root = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        self.project_root = Some(project_root.clone());

        self.component_defs = Arc::new(Self::effective_component_defs(Some(&config)));

        let token = NumberOrString::String("supersigil/indexing".into());
        let client = self.client.clone();

        let _ = client.notify::<lsp_types::notification::Progress>(ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(WorkDoneProgressBegin {
                title: "Supersigil: Indexing".into(),
                cancellable: Some(false),
                message: Some("Discovering files…".into()),
                percentage: Some(0),
            })),
        });

        tracing::info!(?project_root, "initial indexing started");
        let files = discover_files(&config, &project_root);
        let (parses, _errors) = parse_all_files(&files, &self.component_defs);
        tracing::info!(file_count = files.len(), "files parsed");

        let parses: HashMap<PathBuf, SpecDocument> = parses
            .into_iter()
            .map(|(p, doc)| {
                let rel = p
                    .strip_prefix(&project_root)
                    .map(Path::to_path_buf)
                    .unwrap_or(p);
                (rel, doc)
            })
            .collect();

        let docs: Vec<SpecDocument> = parses.values().cloned().collect();
        if let Ok(graph) = build_graph(docs, &config) {
            self.graph = Arc::new(graph);
        }

        self.file_parses = parses;
        self.partial_file_parses.clear();
        self.config = Some(config);

        // Run verify on initial load so diagnostics appear immediately
        // without requiring a save.
        self.run_verify_and_publish();
        self.republish_all_diagnostics();
        self.notify_documents_changed();

        let _ = client.notify::<lsp_types::notification::Progress>(ProgressParams {
            token,
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
                message: Some(format!("Indexed {} files", files.len())),
            })),
        });

        tracing::info!("initial indexing complete");
        ControlFlow::Continue(())
    }

    fn did_change_configuration(
        &mut self,
        _params: DidChangeConfigurationParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        ControlFlow::Continue(())
    }

    fn did_open(
        &mut self,
        params: DidOpenTextDocumentParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        let uri = params.text_document.uri;
        if !self.uri_is_owned(&uri) {
            return ControlFlow::Continue(());
        }
        let content = params.text_document.text;

        // Only reparse when config is loaded (server is active).
        if self.config.is_some() {
            self.reparse_and_publish(&uri, &content);
        }
        self.open_files.insert(uri, Arc::new(content));
        ControlFlow::Continue(())
    }

    fn did_change(
        &mut self,
        params: DidChangeTextDocumentParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        let uri = params.text_document.uri;
        if !self.uri_is_owned(&uri) {
            return ControlFlow::Continue(());
        }

        if let Some(change) = params.content_changes.into_iter().last() {
            let content = change.text;
            if self.config.is_some() {
                self.reparse_and_publish(&uri, &content);
            }
            self.open_files.insert(uri, Arc::new(content));
        }

        ControlFlow::Continue(())
    }

    fn did_save(
        &mut self,
        _params: DidSaveTextDocumentParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        if let Some(config) = &self.config {
            let prev_closed_uris: Vec<Url> = self
                .graph_diagnostics
                .keys()
                .filter(|u| !self.open_files.contains_key(*u))
                .cloned()
                .collect();

            let docs: Vec<SpecDocument> = self.file_parses.values().cloned().collect();

            match build_graph(docs, config) {
                Ok(graph) => {
                    self.graph = Arc::new(graph);
                    self.graph_diagnostics.clear();

                    self.run_verify_and_publish();
                }
                Err(errors) => {
                    tracing::warn!(
                        error_count = errors.len(),
                        "graph rebuild failed, retaining last-good graph"
                    );
                    self.graph_diagnostics.clear();

                    let id_to_path = self.build_id_to_path();
                    let pairs: Vec<(lsp_types::Url, Diagnostic)> = errors
                        .iter()
                        .flat_map(|e| {
                            graph_error_to_diagnostic_with_lookup(e, |doc_id| {
                                id_to_path.get(doc_id).cloned()
                            })
                        })
                        .collect();
                    let grouped = group_by_url(pairs);

                    for (uri, diags) in grouped {
                        self.graph_diagnostics.insert(uri.clone(), diags);
                    }
                }
            }

            self.republish_all_diagnostics();
            self.notify_documents_changed();

            // Clear stale diagnostics for closed files that no longer have issues.
            for uri in &prev_closed_uris {
                if !self.graph_diagnostics.contains_key(uri) {
                    self.publish_merged_diagnostics(uri);
                }
            }
        }

        ControlFlow::Continue(())
    }

    fn did_close(
        &mut self,
        params: DidCloseTextDocumentParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        let uri = &params.text_document.uri;
        self.open_files.remove(uri);
        // Clear buffer-specific diagnostics; workspace-level graph
        // diagnostics are kept so the Problems panel remains accurate.
        self.file_diagnostics.remove(uri);

        // Restore file_parses from disk so unsaved in-memory edits don't
        // linger in the graph after the buffer is discarded.
        if self.config.is_some()
            && let Some(rel_key) = self.uri_to_relative_key(uri)
        {
            let abs_path = match &self.project_root {
                Some(root) => root.join(&rel_key),
                None => rel_key.clone(),
            };
            if abs_path.exists() {
                match supersigil_parser::parse_file(&abs_path, &self.component_defs) {
                    Ok(ParseResult::Document(doc)) => {
                        self.partial_file_parses.remove(&rel_key);
                        self.file_parses.insert(rel_key, doc);
                    }
                    Ok(ParseResult::NotSupersigil(_)) | Err(_) => {
                        self.partial_file_parses.remove(&rel_key);
                        self.file_parses.remove(&rel_key);
                    }
                }
            } else {
                self.partial_file_parses.remove(&rel_key);
                self.file_parses.remove(&rel_key);
            }
        }

        // Republish remaining diagnostics (graph-level only, since
        // file_diagnostics was cleared above). Publishes an empty set if
        // there are no graph diagnostics either.
        self.publish_merged_diagnostics(uri);

        ControlFlow::Continue(())
    }

    fn did_change_watched_files(
        &mut self,
        params: lsp_types::DidChangeWatchedFilesParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        if self.config.is_none() {
            return ControlFlow::Continue(());
        }
        let Some(project_root) = &self.project_root else {
            return ControlFlow::Continue(());
        };

        // Check if supersigil.toml itself changed — reload config.
        if params.changes.iter().any(|c| {
            c.uri
                .to_file_path()
                .ok()
                .is_some_and(|p| p.ends_with("supersigil.toml"))
        }) && let Some((_, new_config)) = discover_config(project_root)
        {
            self.component_defs = Arc::new(Self::effective_component_defs(Some(&new_config)));
            self.config = Some(new_config);
        }

        let config = self.config.as_ref().unwrap();

        // Re-discover and re-parse all files, then rebuild the graph.
        let files = discover_files(config, project_root);
        let (parses, _errors) = parse_all_files(&files, &self.component_defs);
        let parses: HashMap<PathBuf, SpecDocument> = parses
            .into_iter()
            .map(|(p, doc)| {
                let rel = p
                    .strip_prefix(project_root)
                    .map(Path::to_path_buf)
                    .unwrap_or(p);
                (rel, doc)
            })
            .collect();

        // Evict open-file buffers whose backing file was deleted (e.g.
        // during a rename). Without this, the stale buffer would be
        // re-inserted below, producing a duplicate-ID error when the
        // renamed file is also discovered on disk.
        for change in &params.changes {
            if change.typ == lsp_types::FileChangeType::DELETED {
                self.open_files.remove(&change.uri);
            }
        }

        // Re-insert open file buffers (they may have unsaved changes that
        // should take precedence over the disk version).
        let mut merged = parses;
        let mut partial_parses = HashMap::new();
        for (uri, content) in &self.open_files {
            if let Some(rel_key) = self.uri_to_relative_key(uri) {
                let abs_path = project_root.join(&rel_key);
                if let Ok(recovered) = supersigil_parser::parse_content_recovering(
                    &abs_path,
                    content,
                    &self.component_defs,
                ) {
                    match recovered.result {
                        ParseResult::Document(doc) => {
                            if recovered.fatal_errors.is_empty() {
                                merged.insert(rel_key, doc);
                            } else {
                                partial_parses.insert(rel_key, doc);
                            }
                        }
                        ParseResult::NotSupersigil(_) => {}
                    }
                }
            }
        }

        let docs: Vec<SpecDocument> = merged.values().cloned().collect();
        match build_graph(docs, config) {
            Ok(graph) => {
                self.graph = Arc::new(graph);
                self.graph_diagnostics.clear();
                self.run_verify_and_publish();
            }
            Err(errors) => {
                tracing::warn!(
                    error_count = errors.len(),
                    "graph rebuild failed after file watch event"
                );
                self.graph_diagnostics.clear();
                let id_to_path = self.build_id_to_path();
                let pairs: Vec<(Url, Diagnostic)> = errors
                    .iter()
                    .flat_map(|e| {
                        graph_error_to_diagnostic_with_lookup(e, |doc_id| {
                            id_to_path.get(doc_id).cloned()
                        })
                    })
                    .collect();
                let grouped = group_by_url(pairs);
                for (uri, diags) in grouped {
                    self.graph_diagnostics.insert(uri, diags);
                }
            }
        }

        self.file_parses = merged;
        self.partial_file_parses = partial_parses;
        self.republish_all_diagnostics();
        self.notify_documents_changed();

        tracing::info!(
            changes = params.changes.len(),
            "re-indexed after file watch event"
        );

        ControlFlow::Continue(())
    }

    fn code_action(
        &mut self,
        params: CodeActionParams,
    ) -> BoxFuture<'static, Result<Option<lsp_types::CodeActionResponse>, ResponseError>> {
        let uri = params.text_document.uri;

        // Early exit: if no config or project root, nothing to offer.
        let (Some(config), Some(project_root)) = (&self.config, &self.project_root) else {
            return Box::pin(async { Ok(Some(Vec::new())) });
        };

        let content = self
            .open_files
            .get(&uri)
            .cloned()
            .unwrap_or_else(|| Arc::new(String::new()));

        let ctx = ActionRequestContext {
            graph: &self.graph,
            config,
            component_defs: &self.component_defs,
            file_parses: &self.file_parses,
            partial_file_parses: &self.partial_file_parses,
            project_root,
            file_uri: &uri,
            file_content: &content,
        };

        let mut actions: Vec<CodeActionOrCommand> = Vec::new();

        for diag in &params.context.diagnostics {
            // req-2-3: skip diagnostics without data or with non-deserializable data.
            let Some(raw_data) = &diag.data else {
                continue;
            };
            let Ok(data) = serde_json::from_value::<DiagnosticData>(raw_data.clone()) else {
                continue;
            };

            for provider in &self.providers {
                if !provider.handles(&data) {
                    continue;
                }
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    provider.actions(diag, &data, &ctx)
                })) {
                    Ok(provider_actions) => {
                        for mut action in provider_actions {
                            // req-2-4: ensure kind is quickfix and diagnostics includes the originating one.
                            action.kind = Some(lsp_types::CodeActionKind::QUICKFIX);
                            action.diagnostics = Some(vec![diag.clone()]);
                            actions.push(CodeActionOrCommand::CodeAction(action));
                        }
                    }
                    Err(_) => {
                        tracing::warn!(
                            diagnostic_message = %diag.message,
                            "code action provider panicked, skipping"
                        );
                    }
                }
            }
        }

        Box::pin(async move { Ok(Some(actions)) })
    }

    fn execute_command(
        &mut self,
        params: ExecuteCommandParams,
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, Self::Error>> {
        if params.command == commands::VERIFY_COMMAND {
            let prev_closed_uris: Vec<Url> = self
                .graph_diagnostics
                .keys()
                .filter(|u| !self.open_files.contains_key(*u))
                .cloned()
                .collect();

            self.graph_diagnostics.clear();
            self.run_verify_and_publish();
            self.republish_all_diagnostics();
            self.notify_documents_changed();

            for uri in &prev_closed_uris {
                if !self.graph_diagnostics.contains_key(uri) {
                    self.publish_merged_diagnostics(uri);
                }
            }
            return Box::pin(async { Ok(None) });
        }

        if params.command == commands::CREATE_DOCUMENT_COMMAND {
            return self.execute_create_document(&params.arguments);
        }

        if params.command == commands::DOCUMENT_LIST_COMMAND {
            let graph = Arc::clone(&self.graph);
            let project_root = self.project_root.clone().unwrap_or_default();
            let documents = crate::document_list::build_document_entries(&graph, &project_root);
            let result = crate::document_list::DocumentListResult { documents };
            return Box::pin(
                async move { Ok(Some(serde_json::to_value(result).unwrap_or_default())) },
            );
        }

        if params.command == commands::GRAPH_DATA_COMMAND {
            let future = self.handle_graph_data(serde_json::Value::Null);
            return Box::pin(async move {
                let result = future.await?;
                Ok(Some(serde_json::to_value(result).unwrap_or_default()))
            });
        }

        if params.command == commands::DOCUMENT_COMPONENTS_COMMAND {
            let uri_arg = params
                .arguments
                .first()
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            let params = crate::document_components::DocumentComponentsParams { uri: uri_arg };
            let future = self.handle_document_components(params);
            return Box::pin(async move {
                let result = future.await?;
                Ok(Some(serde_json::to_value(result).unwrap_or_default()))
            });
        }

        Box::pin(async { Ok(None) })
    }

    fn definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> BoxFuture<'static, Result<Option<GotoDefinitionResponse>, Self::Error>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        if !self.uri_is_owned(&uri) {
            return Box::pin(async { Ok(None) });
        }
        let position = params.text_document_position_params.position;
        let content = self.open_files.get(&uri).cloned();
        let graph = Arc::clone(&self.graph);

        Box::pin(async move {
            let Some(content) = content else {
                return Ok(None);
            };
            let byte_char = position::utf16_to_byte(&content, position.line, position.character);
            let Some(ref_at) = definition::find_ref_at_position(&content, position.line, byte_char)
            else {
                return Ok(None);
            };
            let location = definition::resolve_ref(&ref_at.ref_string, &graph);
            Ok(location.map(GotoDefinitionResponse::Scalar))
        })
    }

    fn document_symbol(
        &mut self,
        params: DocumentSymbolParams,
    ) -> BoxFuture<'static, Result<Option<DocumentSymbolResponse>, Self::Error>> {
        let uri = params.text_document.uri.clone();
        if !self.uri_is_owned(&uri) {
            return Box::pin(async { Ok(None) });
        }
        let symbols = self
            .open_files
            .get(&uri)
            .and_then(|content| {
                let rel = self.uri_to_relative_key(&uri)?;
                let doc = self
                    .partial_file_parses
                    .get(&rel)
                    .or_else(|| self.file_parses.get(&rel))?;
                Some(document_symbols::document_symbols(doc, content))
            })
            .or_else(|| {
                let rel = self.uri_to_relative_key(&uri)?;
                let doc = self.file_parses.get(&rel)?;
                let abs_path = match &self.project_root {
                    Some(root) => root.join(&rel),
                    None => rel.clone(),
                };
                let content = std::fs::read_to_string(abs_path).ok()?;
                Some(document_symbols::document_symbols(doc, &content))
            });
        Box::pin(async move { Ok(symbols.map(DocumentSymbolResponse::Nested)) })
    }

    fn references(
        &mut self,
        params: lsp_types::ReferenceParams,
    ) -> BoxFuture<'static, Result<Option<Vec<Location>>, Self::Error>> {
        let uri = params.text_document_position.text_document.uri.clone();
        if !self.uri_is_owned(&uri) {
            return Box::pin(async { Ok(None) });
        }
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;
        let content = self.open_files.get(&uri).cloned();
        let graph = Arc::clone(&self.graph);

        // Resolve file URI to document ID.
        let doc_id = self
            .uri_to_relative_key(&uri)
            .and_then(|rel| self.file_parses.get(&rel))
            .map(|doc| doc.frontmatter.id.clone());

        Box::pin(async move {
            let Some(content) = content else {
                return Ok(None);
            };
            let Some(doc_id) = doc_id else {
                return Ok(None);
            };
            let byte_char = position::utf16_to_byte(&content, position.line, position.character);
            let Some((target_doc, target_fragment)) =
                references::find_reference_target(&content, position.line, byte_char, &doc_id)
            else {
                return Ok(None);
            };
            let locations = references::collect_references(
                &target_doc,
                target_fragment.as_deref(),
                include_declaration,
                &graph,
            );
            if locations.is_empty() {
                Ok(None)
            } else {
                Ok(Some(locations))
            }
        })
    }

    fn prepare_rename(
        &mut self,
        params: lsp_types::TextDocumentPositionParams,
    ) -> BoxFuture<'static, Result<Option<lsp_types::PrepareRenameResponse>, Self::Error>> {
        let uri = params.text_document.uri.clone();
        if !self.uri_is_owned(&uri) {
            return Box::pin(async { Ok(None) });
        }
        let pos = params.position;
        let content = self.open_files.get(&uri).cloned();
        let doc_id = self
            .uri_to_relative_key(&uri)
            .and_then(|rel| self.file_parses.get(&rel))
            .map(|doc| doc.frontmatter.id.clone());

        Box::pin(async move {
            let Some(content) = content else {
                return Ok(None);
            };
            let Some(doc_id) = doc_id else {
                return Ok(None);
            };
            let byte_char = position::utf16_to_byte(&content, pos.line, pos.character);
            let Some(target) = rename::find_rename_target(&content, pos.line, byte_char, &doc_id)
            else {
                return Ok(None);
            };

            let (placeholder, range) = match &target {
                rename::RenameTarget::ComponentId {
                    component_id,
                    range,
                    ..
                } => (component_id.clone(), range),
                rename::RenameTarget::DocumentId { doc_id, range } => (doc_id.clone(), range),
            };

            let line_str = content.lines().nth(range.line as usize).unwrap_or("");
            let lsp_range = position::byte_range_to_lsp(
                line_str,
                range.line,
                range.start as usize,
                range.end as usize,
            );

            Ok(Some(
                lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                    range: lsp_range,
                    placeholder,
                },
            ))
        })
    }

    fn rename(
        &mut self,
        params: lsp_types::RenameParams,
    ) -> BoxFuture<'static, Result<Option<lsp_types::WorkspaceEdit>, Self::Error>> {
        let uri = params.text_document_position.text_document.uri.clone();
        if !self.uri_is_owned(&uri) {
            return Box::pin(async { Ok(None) });
        }
        let pos = params.text_document_position.position;
        let new_name = params.new_name;
        let content = self.open_files.get(&uri).cloned();
        let graph = Arc::clone(&self.graph);
        let open_files = self.open_files.clone();
        let doc_id = self
            .uri_to_relative_key(&uri)
            .and_then(|rel| self.file_parses.get(&rel))
            .map(|doc| doc.frontmatter.id.clone());

        Box::pin(async move {
            let Some(content) = content else {
                return Ok(None);
            };
            let Some(doc_id) = doc_id else {
                return Ok(None);
            };

            if let Err(msg) = rename::validate_new_name(&new_name) {
                return Err(ResponseError::new(
                    async_lsp::ErrorCode::INVALID_PARAMS,
                    msg,
                ));
            }

            let byte_char = position::utf16_to_byte(&content, pos.line, pos.character);
            let Some(target) = rename::find_rename_target(&content, pos.line, byte_char, &doc_id)
            else {
                return Ok(None);
            };

            let workspace_edit =
                rename::collect_rename_edits(&target, &new_name, &graph, &open_files);
            Ok(Some(workspace_edit))
        })
    }

    fn code_lens(
        &mut self,
        params: lsp_types::CodeLensParams,
    ) -> BoxFuture<'static, Result<Option<Vec<lsp_types::CodeLens>>, Self::Error>> {
        let uri = params.text_document.uri.clone();
        if !self.uri_is_owned(&uri) {
            return Box::pin(async { Ok(None) });
        }

        let rel_key = self.uri_to_relative_key(&uri);
        let doc = rel_key.as_ref().and_then(|rel| {
            self.file_parses
                .get(rel)
                .or_else(|| self.partial_file_parses.get(rel))
                .cloned()
        });
        let doc_id = doc.as_ref().map(|d| d.frontmatter.id.clone());

        // Read content from open buffers; fall back to disk.
        let content = self.open_files.get(&uri).cloned().or_else(|| {
            let rel = rel_key.as_ref()?;
            let abs = self
                .project_root
                .as_ref()
                .map(|r| r.join(rel))
                .unwrap_or(rel.clone());
            std::fs::read_to_string(&abs).ok().map(Arc::new)
        });

        let graph = Arc::clone(&self.graph);
        let evidence = self.evidence_by_target.clone();

        Box::pin(async move {
            let Some(doc) = doc else {
                return Ok(None);
            };
            let Some(doc_id) = doc_id else {
                return Ok(None);
            };
            let Some(content) = content else {
                return Ok(None);
            };
            let lenses = crate::code_lens::build_code_lenses(
                &doc,
                &doc_id,
                &content,
                &graph,
                evidence.as_deref(),
            );
            if lenses.is_empty() {
                Ok(Some(vec![]))
            } else {
                Ok(Some(lenses))
            }
        })
    }

    fn hover(
        &mut self,
        params: HoverParams,
    ) -> BoxFuture<'static, Result<Option<Hover>, Self::Error>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        if !self.uri_is_owned(&uri) {
            return Box::pin(async { Ok(None) });
        }
        let position = params.text_document_position_params.position;
        let content = self.open_files.get(&uri).cloned();
        let graph = Arc::clone(&self.graph);
        let defs = Arc::clone(&self.component_defs);

        Box::pin(async move {
            let Some(content) = content else {
                return Ok(None);
            };
            let byte_char = position::utf16_to_byte(&content, position.line, position.character);
            Ok(hover::hover_at_position(
                &content,
                position.line,
                byte_char,
                &defs,
                &graph,
            ))
        })
    }

    fn completion(
        &mut self,
        params: CompletionParams,
    ) -> BoxFuture<'static, Result<Option<CompletionResponse>, Self::Error>> {
        let uri = params.text_document_position.text_document.uri.clone();
        if !self.uri_is_owned(&uri) {
            return Box::pin(async { Ok(None) });
        }
        let position = params.text_document_position.position;
        let content = self.open_files.get(&uri).cloned();
        let graph = Arc::clone(&self.graph);
        let defs = Arc::clone(&self.component_defs);
        let config = self.config.clone();

        // Look up the current file's document type and ID from parsed frontmatter,
        // falling back to partial parses so completions work during editing errors.
        let rel_key = self.uri_to_relative_key(&uri);
        let parsed = rel_key.as_ref().and_then(|rel| {
            self.file_parses
                .get(rel)
                .or_else(|| self.partial_file_parses.get(rel))
        });
        let doc_type = parsed.and_then(|doc| doc.frontmatter.doc_type.clone());
        let doc_id = parsed.map(|doc| doc.frontmatter.id.clone());

        Box::pin(async move {
            let Some(content) = content else {
                return Ok(None);
            };
            let byte_char = position::utf16_to_byte(&content, position.line, position.character);
            let items = completion::complete(
                &content,
                position.line,
                byte_char,
                &graph,
                &defs,
                config.as_ref(),
                doc_type.as_deref(),
                doc_id.as_deref(),
            );
            if items.is_empty() {
                Ok(None)
            } else {
                Ok(Some(CompletionResponse::Array(items)))
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn discover_config(root: &Path) -> Option<(PathBuf, Config)> {
    let config_path = find_config(root).ok()??;
    let config = load_config(&config_path).ok()?;
    Some((config_path, config))
}

fn discover_files(config: &Config, root: &Path) -> Vec<PathBuf> {
    let globs = collect_globs(config);
    expand_globs(globs, root)
}

fn collect_globs(config: &Config) -> Vec<&str> {
    if let Some(paths) = &config.paths {
        return paths.iter().map(String::as_str).collect();
    }

    if let Some(projects) = &config.projects {
        return projects
            .values()
            .flat_map(|p| p.paths.iter().map(String::as_str))
            .collect();
    }

    Vec::new()
}

fn parse_all_files(
    files: &[PathBuf],
    defs: &ComponentDefs,
) -> (
    HashMap<PathBuf, SpecDocument>,
    Vec<supersigil_core::ParseError>,
) {
    let mut parses = HashMap::new();
    let mut errors = Vec::new();

    for path in files {
        match supersigil_parser::parse_file(path, defs) {
            Ok(ParseResult::Document(doc)) => {
                parses.insert(path.clone(), doc);
            }
            Ok(ParseResult::NotSupersigil(_)) => {}
            Err(mut errs) => errors.append(&mut errs),
        }
    }

    (parses, errors)
}

fn empty_graph() -> DocumentGraph {
    build_graph(Vec::new(), &Config::default()).unwrap_or_else(|_| {
        panic!("failed to build empty graph");
    })
}

#[cfg(test)]
mod tests {
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
                js: None,
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
        let (artifact_graph, _plugin_findings) = supersigil_verify::plugins::build_evidence(
            &config,
            &graph,
            &project_root,
            None,
            &inputs,
        );

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

    #[verifies("version-mismatch/req#req-1-1")]
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
}
