#[allow(
    clippy::wildcard_imports,
    reason = "task scaffold explicitly requires `use super::*;`"
)]
use super::*;
use crate::commands as lsp_commands;

impl SupersigilLsp {
    pub(super) fn handle_document_list(
        &mut self,
        _params: serde_json::Value,
    ) -> BoxFuture<'static, Result<crate::document_list::DocumentListResult, ResponseError>> {
        let graph = Arc::clone(&self.graph);
        let project_root = self.project_root.clone().unwrap_or_default();

        let documents = crate::document_list::build_document_entries(&graph, &project_root);

        Box::pin(async move { Ok(crate::document_list::DocumentListResult { documents }) })
    }

    pub(super) fn handle_explorer_snapshot(
        &mut self,
        _params: serde_json::Value,
    ) -> BoxFuture<'static, Result<crate::explorer_runtime::ExplorerSnapshot, ResponseError>> {
        let snapshot = self.current_explorer_snapshot();
        Box::pin(async move { Ok(snapshot) })
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "async_lsp Router requires params by value"
    )]
    pub(super) fn handle_explorer_document(
        &mut self,
        params: crate::explorer_runtime::ExplorerDocumentParams,
    ) -> BoxFuture<'static, Result<crate::explorer_runtime::ExplorerDocument, ResponseError>> {
        let revision = self.current_explorer_revision();
        let Some((doc, stale)) = self.find_document_by_id(&params.document_id) else {
            let empty = crate::explorer_runtime::ExplorerDocument {
                revision,
                document_id: params.document_id,
                stale: false,
                fences: Vec::new(),
                edges: Vec::new(),
            };
            return Box::pin(async move { Ok(empty) });
        };

        let document_components = self.build_document_components_for_doc(&doc, stale);

        Box::pin(async move {
            Ok(crate::explorer_runtime::build_explorer_document(
                &crate::explorer_runtime::BuildExplorerDocumentInput {
                    revision: &revision,
                    document_components,
                },
            ))
        })
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "async_lsp Router requires params by value"
    )]
    pub(super) fn handle_document_components(
        &mut self,
        params: crate::document_components::DocumentComponentsParams,
    ) -> BoxFuture<
        'static,
        Result<crate::document_components::DocumentComponentsResult, ResponseError>,
    > {
        use crate::document_components::DocumentComponentsResult;

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

        let Some((doc, stale)) = self.current_or_partial_doc_for_uri(&uri) else {
            return empty();
        };

        let result = self.build_document_components_for_doc(doc, stale);

        Box::pin(async move { Ok(result) })
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "mirrors LanguageServer::execute_command params"
    )]
    pub(super) fn dispatch_execute_command(
        &mut self,
        params: ExecuteCommandParams,
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, ResponseError>> {
        if params.command == lsp_commands::VERIFY_COMMAND {
            return self.execute_verify_command();
        }
        if params.command == lsp_commands::CREATE_DOCUMENT_COMMAND {
            return self.execute_create_document(&params.arguments);
        }
        if params.command == lsp_commands::DOCUMENT_LIST_COMMAND {
            return self.execute_document_list_command();
        }
        if params.command == lsp_commands::EXPLORER_SNAPSHOT_COMMAND {
            return self.execute_explorer_snapshot_command();
        }
        if params.command == lsp_commands::DOCUMENT_COMPONENTS_COMMAND {
            return self.execute_document_components_command(&params.arguments);
        }
        if params.command == lsp_commands::EXPLORER_DOCUMENT_COMMAND {
            return self.execute_explorer_document_command(&params.arguments);
        }

        Box::pin(async { Ok(None) })
    }

    pub(super) fn execute_verify_command(
        &mut self,
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, ResponseError>> {
        let prev_closed_uris: Vec<Url> = self
            .graph_diagnostics
            .keys()
            .filter(|uri| !self.open_files.contains_key(*uri))
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

        Box::pin(async { Ok(None) })
    }

    pub(super) fn execute_document_list_command(
        &mut self,
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, ResponseError>> {
        let future = self.handle_document_list(serde_json::Value::Null);
        Box::pin(async move {
            let result = future.await?;
            Ok(Some(serde_json::to_value(result).unwrap_or_default()))
        })
    }

    pub(super) fn execute_explorer_snapshot_command(
        &mut self,
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, ResponseError>> {
        let future = self.handle_explorer_snapshot(serde_json::Value::Null);
        Box::pin(async move {
            let result = future.await?;
            Ok(Some(serde_json::to_value(result).unwrap_or_default()))
        })
    }

    pub(super) fn execute_document_components_command(
        &mut self,
        arguments: &[serde_json::Value],
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, ResponseError>> {
        let uri_arg = arguments
            .first()
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();
        let params = crate::document_components::DocumentComponentsParams { uri: uri_arg };
        let future = self.handle_document_components(params);
        Box::pin(async move {
            let result = future.await?;
            Ok(Some(serde_json::to_value(result).unwrap_or_default()))
        })
    }

    pub(super) fn execute_explorer_document_command(
        &mut self,
        arguments: &[serde_json::Value],
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, ResponseError>> {
        let request_params = arguments
            .first()
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or(crate::explorer_runtime::ExplorerDocumentParams {
                document_id: String::new(),
                revision: String::new(),
            });
        let future = self.handle_explorer_document(request_params);
        Box::pin(async move {
            let result = future.await?;
            Ok(Some(serde_json::to_value(result).unwrap_or_default()))
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
    pub(super) fn execute_create_document(
        &self,
        arguments: &[serde_json::Value],
    ) -> BoxFuture<'static, Result<Option<serde_json::Value>, ResponseError>> {
        let Some(args) = arguments.first().cloned().and_then(|value| {
            serde_json::from_value::<crate::commands::CreateDocumentParams>(value).ok()
        }) else {
            return Box::pin(async { Ok(None) });
        };
        let crate::commands::CreateDocumentParams {
            feature,
            target_ref,
            full_type,
        } = args;

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
