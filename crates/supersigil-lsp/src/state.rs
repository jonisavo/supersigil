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
    CONFIG_FILENAME, ComponentDefs, Config, DocumentGraph, ParseResult, SpecDocument, build_graph,
    expand_globs, find_config, load_config,
};
use supersigil_evidence::{EvidenceId, VerificationEvidenceRecord};
use supersigil_verify::{VerifyInputs, VerifyOptions, verify};

use crate::code_actions::{ActionRequestContext, CodeActionProvider};
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

mod access;
mod commands;
mod explorer;
mod indexing;
mod lifecycle;

#[cfg(test)]
mod tests;

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
    /// Monotonic revision for explorer runtime payloads.
    explorer_revision: u64,
    /// Last published explorer snapshot used for change diffing.
    last_explorer_snapshot: Option<crate::explorer_runtime::ExplorerSnapshot>,
    /// Last published explorer detail fingerprints used for selective invalidation.
    last_explorer_detail_fingerprints:
        Option<HashMap<String, crate::explorer_runtime::ExplorerDocumentFingerprint>>,
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
            graph: Arc::new(indexing::empty_graph()),
            component_defs: Arc::new(ComponentDefs::defaults()),
            file_diagnostics: HashMap::new(),
            graph_diagnostics: HashMap::new(),
            evidence_by_target: None,
            evidence_records: None,
            explorer_revision: 0,
            last_explorer_snapshot: None,
            last_explorer_detail_fingerprints: None,
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
        router.request::<crate::explorer_runtime::ExplorerSnapshotRequest, _>(
            Self::handle_explorer_snapshot,
        );
        router.request::<crate::explorer_runtime::ExplorerDocumentRequest, _>(
            Self::handle_explorer_document,
        );
        router.request::<crate::document_components::DocumentComponentsRequest, _>(
            Self::handle_document_components,
        );
        router
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
        self.initial_index();
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
        let content = Self::normalize_live_content(params.text_document.text);

        // Only reparse when config is loaded (server is active).
        if self.config.is_some() {
            self.reparse_and_publish(&uri, content.as_ref());
        }
        self.open_files.insert(uri, content);
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
            let content = Self::normalize_live_content(change.text);
            if self.config.is_some() {
                self.reparse_and_publish(&uri, content.as_ref());
            }
            self.open_files.insert(uri, content);
        }

        ControlFlow::Continue(())
    }

    fn did_save(
        &mut self,
        params: DidSaveTextDocumentParams,
    ) -> ControlFlow<async_lsp::Result<()>> {
        self.refresh_after_save(&params.text_document.uri);
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

        self.restore_closed_file_from_disk(uri);

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
        self.reload_after_watch_change(&params);
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
        self.dispatch_execute_command(params)
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
        let content = self.content_from_open_buffer(&uri);
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
            .current_or_partial_doc_for_uri(&uri)
            .and_then(|(doc, _stale)| {
                self.content_from_buffer_or_disk(&uri)
                    .map(|content| document_symbols::document_symbols(doc, &content))
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
        let content = self.content_from_open_buffer(&uri);
        let graph = Arc::clone(&self.graph);

        // References intentionally use the indexed doc ID, not partial parses.
        let doc_id = self.indexed_doc_id_for_uri(&uri);

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
        let content = self.content_from_open_buffer(&uri);
        let doc_id = self.indexed_doc_id_for_uri(&uri);

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
        let content = self.content_from_open_buffer(&uri);
        let graph = Arc::clone(&self.graph);
        let open_files = self.open_files.clone();
        let doc_id = self.indexed_doc_id_for_uri(&uri);

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

        let graph = Arc::clone(&self.graph);
        let evidence = self.evidence_by_target.clone();
        let lenses = self
            .current_or_partial_doc_for_uri(&uri)
            .and_then(|(doc, _stale)| {
                let doc_id = doc.frontmatter.id.clone();
                let content = self.content_from_buffer_or_disk(&uri)?;
                Some(crate::code_lens::build_code_lenses(
                    doc,
                    &doc_id,
                    &content,
                    &graph,
                    evidence.as_deref(),
                ))
            });

        Box::pin(
            async move { Ok(lenses.map(|lenses| if lenses.is_empty() { vec![] } else { lenses })) },
        )
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
        let content = self.content_from_open_buffer(&uri);
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
        let content = self.content_from_open_buffer(&uri);
        let graph = Arc::clone(&self.graph);
        let defs = Arc::clone(&self.component_defs);
        let config = self.config.clone();

        // Look up the current file's document type and ID from parsed frontmatter,
        // falling back to partial parses so completions work during editing errors.
        let (doc_type, doc_id) =
            self.current_or_partial_doc_for_uri(&uri)
                .map_or((None, None), |(doc, _stale)| {
                    (
                        doc.frontmatter.doc_type.clone(),
                        Some(doc.frontmatter.id.clone()),
                    )
                });

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
