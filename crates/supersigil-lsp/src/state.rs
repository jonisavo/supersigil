//! Server state, initialization, and helper functions.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_lsp::{ClientSocket, LanguageServer, ResponseError};
use futures::future::BoxFuture;
use lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, Diagnostic,
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, ExecuteCommandOptions,
    ExecuteCommandParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    InitializeParams, InitializeResult, MessageType, NumberOrString, PositionEncodingKind,
    ProgressParams, ProgressParamsValue, PublishDiagnosticsParams, ServerCapabilities,
    ShowMessageParams, TextDocumentSyncCapability, TextDocumentSyncKind, Url, WorkDoneProgress,
    WorkDoneProgressBegin, WorkDoneProgressEnd,
};

use supersigil_core::{
    ComponentDefs, Config, DiagnosticsTier, DocumentGraph, ParseResult, SpecDocument, build_graph,
    expand_globs, find_config, load_config,
};
use supersigil_evidence::{EcosystemPlugin, ProjectScope};
use supersigil_parser::parse_content;
use supersigil_verify::{
    VerifyInputs, VerifyOptions, build_artifact_graph, extract_explicit_evidence, verify,
};

use crate::commands;
use crate::completion;
use crate::definition;
use crate::diagnostics::{
    finding_to_diagnostic, graph_error_to_diagnostic_with_lookup, group_by_url,
    parse_error_to_diagnostic,
};
use crate::hover;
use crate::parse_tier;
use crate::position;

// ---------------------------------------------------------------------------
// SupersigilLsp
// ---------------------------------------------------------------------------

/// The main language server state.
pub struct SupersigilLsp {
    client: ClientSocket,
    config: Option<Config>,
    project_root: Option<PathBuf>,
    diagnostics_tier: DiagnosticsTier,
    open_files: HashMap<lsp_types::Url, Arc<String>>,
    file_parses: HashMap<PathBuf, SpecDocument>,
    graph: Arc<DocumentGraph>,
    component_defs: Arc<ComponentDefs>,
    file_diagnostics: HashMap<Url, Vec<Diagnostic>>,
    graph_diagnostics: HashMap<Url, Vec<Diagnostic>>,
}

impl std::fmt::Debug for SupersigilLsp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SupersigilLsp")
            .field("project_root", &self.project_root)
            .field("diagnostics_tier", &self.diagnostics_tier)
            .field("open_files", &self.open_files.len())
            .field("file_parses", &self.file_parses.len())
            .finish_non_exhaustive()
    }
}

impl SupersigilLsp {
    #[must_use]
    pub fn new_router(client: ClientSocket) -> async_lsp::router::Router<Self, ResponseError> {
        async_lsp::router::Router::from_language_server(Self {
            client,
            config: None,
            project_root: None,
            diagnostics_tier: DiagnosticsTier::default(),
            open_files: HashMap::new(),
            file_parses: HashMap::new(),
            graph: Arc::new(empty_graph()),
            component_defs: Arc::new(ComponentDefs::defaults()),
            file_diagnostics: HashMap::new(),
            graph_diagnostics: HashMap::new(),
        })
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
        match parse_content(&abs_path, content, &self.component_defs) {
            Ok(ParseResult::Document(doc)) => {
                self.file_parses.insert(rel_key, doc);
                self.file_diagnostics.insert(uri.clone(), vec![]);
            }
            Ok(ParseResult::NotSupersigil(_)) => {
                self.file_parses.remove(&rel_key);
                self.file_diagnostics.insert(uri.clone(), vec![]);
            }
            Err(errs) => {
                self.file_parses.remove(&rel_key);
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

    fn run_verify_and_publish(&mut self, tier: DiagnosticsTier) {
        if tier < DiagnosticsTier::Verify {
            return;
        }

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
        let plugins = assemble_plugins(config);
        let scope = ProjectScope {
            project: None,
            project_root: project_root.clone(),
        };
        let plugin_evidence =
            collect_plugin_evidence(&plugins, &inputs.test_files, &scope, &self.graph);
        let explicit = extract_explicit_evidence(&self.graph, &inputs.tag_matches, &project_root);
        let artifact_graph = build_artifact_graph(&self.graph, explicit, plugin_evidence.evidence);

        match verify(
            &self.graph,
            config,
            &project_root,
            &options,
            &artifact_graph,
        ) {
            Ok(report) => {
                // Count example-coverable findings per document before
                // converting to diagnostics (conversion downgrades them
                // to HINT, losing the distinction).
                let mut example_coverable_counts: HashMap<String, usize> = HashMap::new();
                for finding in &report.findings {
                    if finding
                        .details
                        .as_ref()
                        .is_some_and(|d| d.example_coverable)
                        && let Some(doc_id) = &finding.doc_id
                    {
                        *example_coverable_counts.entry(doc_id.clone()).or_default() += 1;
                    }
                }

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

                // Add a single info diagnostic per document summarizing
                // how many criteria are gated behind example execution.
                for (doc_id, count) in &example_coverable_counts {
                    if let Some(path) = id_to_path.get(doc_id.as_str())
                        && let Some(url) = crate::path_to_url(path)
                    {
                        let message = if *count == 1 {
                            "1 criterion covered only by executable examples (not run by LSP). Use `supersigil verify` to confirm.".to_owned()
                        } else {
                            format!(
                                "{count} criteria covered only by executable examples (not run by LSP). Use `supersigil verify` to confirm."
                            )
                        };
                        self.graph_diagnostics
                            .entry(url)
                            .or_default()
                            .push(Diagnostic {
                                range: position::zero_range(position::raw_to_lsp(0, 0)),
                                severity: Some(lsp_types::DiagnosticSeverity::INFORMATION),
                                source: Some(crate::DIAGNOSTIC_SOURCE.to_string()),
                                message,
                                ..Diagnostic::default()
                            });
                    }
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "verify pipeline failed");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin evidence helpers (mirrors supersigil-cli/src/plugins.rs)
// ---------------------------------------------------------------------------

/// Assemble the enabled ecosystem plugin instances from the config.
fn assemble_plugins(config: &Config) -> Vec<Box<dyn EcosystemPlugin>> {
    let mut plugins: Vec<Box<dyn EcosystemPlugin>> = Vec::new();
    for name in &config.ecosystem.plugins {
        if name == "rust" {
            plugins.push(Box::new(supersigil_rust::RustPlugin));
        }
    }
    plugins
}

/// Collect evidence from all enabled plugins.
fn collect_plugin_evidence(
    plugins: &[Box<dyn EcosystemPlugin>],
    test_files: &[PathBuf],
    scope: &ProjectScope,
    documents: &DocumentGraph,
) -> PluginEvidenceResult {
    let mut evidence = Vec::new();
    for plugin in plugins {
        let files = plugin.plan_discovery_inputs(test_files, scope);
        match plugin.discover(&files, scope, documents) {
            Ok(result) => {
                evidence.extend(result.evidence);
            }
            Err(err) => {
                tracing::warn!(plugin = plugin.name(), error = %err, "plugin discovery failed");
            }
        }
    }
    PluginEvidenceResult { evidence }
}

struct PluginEvidenceResult {
    evidence: Vec<supersigil_evidence::VerificationEvidenceRecord>,
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
        // other MDX servers in non-Supersigil workspaces.
        let has_config = self
            .project_root
            .as_ref()
            .and_then(|r| find_config(r).ok().flatten())
            .is_some();

        let capabilities = if has_config {
            ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["<".to_owned(), "#".to_owned(), "\"".to_owned()]),
                    ..CompletionOptions::default()
                }),
                hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
                definition_provider: Some(lsp_types::OneOf::Left(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![commands::VERIFY_COMMAND.to_string()],
                    ..Default::default()
                }),
                ..ServerCapabilities::default()
            }
        } else {
            ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..ServerCapabilities::default()
            }
        };

        let result = InitializeResult {
            capabilities,
            ..InitializeResult::default()
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

        self.diagnostics_tier = config
            .lsp
            .as_ref()
            .map_or(DiagnosticsTier::default(), |lsp| lsp.diagnostics);

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
        self.diagnostics_tier = config
            .lsp
            .as_ref()
            .map_or(DiagnosticsTier::Verify, |lsp| lsp.diagnostics);
        self.config = Some(config);

        // Run verify on initial load so diagnostics appear immediately
        // without requiring a save.
        let tier = self.diagnostics_tier;
        self.run_verify_and_publish(tier);
        self.republish_all_diagnostics();

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
        params: DidChangeConfigurationParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        let tier_str = params.settings["supersigil"]["diagnostics"]
            .as_str()
            .unwrap_or("");

        if tier_str.is_empty() {
            return ControlFlow::Continue(());
        }

        if let Some(tier) = parse_tier(tier_str) {
            self.diagnostics_tier = tier;
            tracing::info!(?tier, "diagnostics tier updated");
        } else {
            tracing::warn!(value = tier_str, "invalid supersigil.diagnostics value");
            let _ = self
                .client
                .notify::<lsp_types::notification::ShowMessage>(ShowMessageParams {
                    typ: MessageType::WARNING,
                    message: format!(
                        "Supersigil: unknown diagnostics tier {tier_str:?}; \
                             expected one of \"lint\", \"verify\".",
                    ),
                });
        }

        ControlFlow::Continue(())
    }

    fn did_open(
        &mut self,
        params: DidOpenTextDocumentParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        let uri = params.text_document.uri;
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

                    let tier = self.diagnostics_tier;
                    self.run_verify_and_publish(tier);
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
                        self.file_parses.insert(rel_key, doc);
                    }
                    Ok(ParseResult::NotSupersigil(_)) | Err(_) => {
                        self.file_parses.remove(&rel_key);
                    }
                }
            } else {
                self.file_parses.remove(&rel_key);
            }
        }

        // Republish remaining diagnostics (graph-level only, since
        // file_diagnostics was cleared above). Publishes an empty set if
        // there are no graph diagnostics either.
        self.publish_merged_diagnostics(uri);

        ControlFlow::Continue(())
    }

    fn execute_command(
        &mut self,
        params: ExecuteCommandParams,
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, Self::Error>> {
        if params.command == commands::VERIFY_COMMAND {
            let tier =
                commands::parse_verify_tier(&params.arguments).unwrap_or(self.diagnostics_tier);

            let prev_closed_uris: Vec<Url> = self
                .graph_diagnostics
                .keys()
                .filter(|u| !self.open_files.contains_key(*u))
                .cloned()
                .collect();

            self.graph_diagnostics.clear();
            self.run_verify_and_publish(tier);
            self.republish_all_diagnostics();

            for uri in &prev_closed_uris {
                if !self.graph_diagnostics.contains_key(uri) {
                    self.publish_merged_diagnostics(uri);
                }
            }
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
        let position = params.text_document_position_params.position;
        let content = self.open_files.get(&uri).cloned();
        let graph = Arc::clone(&self.graph);

        Box::pin(async move {
            let Some(content) = content else {
                return Ok(None);
            };
            let byte_char = position::utf16_to_byte(&content, position.line, position.character);
            let Some(ref_str) =
                definition::find_ref_at_position(&content, position.line, byte_char)
            else {
                return Ok(None);
            };
            let location = definition::resolve_ref(&ref_str, &graph);
            Ok(location.map(GotoDefinitionResponse::Scalar))
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
        let position = params.text_document_position.position;
        let content = self.open_files.get(&uri).cloned();
        let graph = Arc::clone(&self.graph);
        let defs = Arc::clone(&self.component_defs);
        let config = self.config.clone();

        // Look up the current file's document type from parsed frontmatter.
        let doc_type = self
            .uri_to_relative_key(&uri)
            .and_then(|rel| self.file_parses.get(&rel))
            .and_then(|doc| doc.frontmatter.doc_type.clone());

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
    use super::*;

    #[test]
    fn empty_graph_builds_successfully() {
        let graph = empty_graph();
        assert_eq!(graph.doc_order().len(), 0);
    }

    #[test]
    fn collect_globs_single_project() {
        let config = Config {
            paths: Some(vec!["specs/**/*.mdx".into()]),
            ..Config::default()
        };
        let globs = collect_globs(&config);
        assert_eq!(globs, vec!["specs/**/*.mdx"]);
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
            path: dir.path().join("specs/my-feature.req.mdx"),
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
                code_blocks: vec![],
                position: supersigil_core::SourcePosition {
                    byte_offset: 0,
                    line: 5,
                    column: 1,
                },
            }],
        };

        let mut config = Config {
            paths: Some(vec!["specs/**/*.mdx".into()]),
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
        let plugins = assemble_plugins(&config);
        let scope = ProjectScope {
            project: None,
            project_root: project_root.clone(),
        };
        let plugin_result = collect_plugin_evidence(&plugins, &inputs.test_files, &scope, &graph);

        // Plugin should find evidence from the #[verifies] attribute
        assert!(
            !plugin_result.evidence.is_empty(),
            "Rust plugin should discover evidence from #[verifies] attribute"
        );

        let explicit = extract_explicit_evidence(&graph, &inputs.tag_matches, &project_root);
        let artifact_graph = build_artifact_graph(&graph, explicit, plugin_result.evidence);

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
}
