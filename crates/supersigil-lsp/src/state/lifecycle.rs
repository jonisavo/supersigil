#[allow(
    clippy::wildcard_imports,
    reason = "lifecycle helpers share the parent imports"
)]
use super::*;

impl SupersigilLsp {
    pub(super) fn initial_index(&mut self) {
        let Some(root) = self.project_root.clone() else {
            tracing::info!("no workspace root; skipping initial indexing");
            return;
        };

        let Some((config_path, config)) = indexing::discover_config(&root) else {
            tracing::info!("no supersigil.toml found; skipping initial indexing");
            return;
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
        let files = indexing::discover_files(&config, &project_root);
        let (parses, _errors) = indexing::parse_all_files(&files, &self.component_defs);
        tracing::info!(file_count = files.len(), "files parsed");

        let parses: HashMap<PathBuf, SpecDocument> = parses
            .into_iter()
            .map(|(path, doc)| {
                let rel = path
                    .strip_prefix(&project_root)
                    .map(Path::to_path_buf)
                    .unwrap_or(path);
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
    }

    pub(super) fn refresh_after_save(&mut self, uri: &Url) {
        let Some(config) = self.config.clone() else {
            return;
        };
        if !self.is_project_file(uri) {
            return;
        }

        let prev_closed_uris: Vec<Url> = self
            .graph_diagnostics
            .keys()
            .filter(|uri| !self.open_files.contains_key(*uri))
            .cloned()
            .collect();

        let docs: Vec<SpecDocument> = self.file_parses.values().cloned().collect();
        self.rebuild_graph_or_record_diagnostics(docs, &config, "save");

        self.republish_all_diagnostics();
        self.notify_documents_changed();

        // Clear stale diagnostics for closed files that no longer have issues.
        for uri in &prev_closed_uris {
            if !self.graph_diagnostics.contains_key(uri) {
                self.publish_merged_diagnostics(uri);
            }
        }
    }

    pub(super) fn restore_closed_file_from_disk(&mut self, uri: &Url) {
        // Restore file_parses from disk so unsaved in-memory edits don't
        // linger in the graph after the buffer is discarded.
        if self.config.is_some()
            && let Some(rel_key) = self.uri_to_relative_key(uri)
        {
            let abs_path = match &self.project_root {
                Some(root) => root.join(&rel_key),
                None => rel_key.clone(),
            };
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
        }
    }

    pub(super) fn reload_after_watch_change(
        &mut self,
        params: &lsp_types::DidChangeWatchedFilesParams,
    ) {
        if self.config.is_none() {
            return;
        }
        let Some(project_root) = self.project_root.clone() else {
            return;
        };

        // Check if supersigil.toml itself changed — reload config.
        if params.changes.iter().any(|change| {
            change
                .uri
                .to_file_path()
                .ok()
                .is_some_and(|path| path.ends_with(CONFIG_FILENAME))
        }) && let Some((_, new_config)) = indexing::discover_config(&project_root)
        {
            self.component_defs = Arc::new(Self::effective_component_defs(Some(&new_config)));
            self.config = Some(new_config);
        }

        let config = self.config.clone().unwrap();

        // Re-discover and re-parse all files, then rebuild the graph.
        let files = indexing::discover_files(&config, &project_root);
        let (parses, _errors) = indexing::parse_all_files(&files, &self.component_defs);
        let parses: HashMap<PathBuf, SpecDocument> = parses
            .into_iter()
            .map(|(path, doc)| {
                let rel = path
                    .strip_prefix(&project_root)
                    .map(Path::to_path_buf)
                    .unwrap_or(path);
                (rel, doc)
            })
            .collect();

        // Evict open-file buffers whose backing file was deleted (e.g.
        // during a rename). Without this, the stale buffer would be
        // re-inserted below, producing a duplicate-ID error when the
        // renamed file is also discovered on disk.
        let deleted_uris: Vec<Url> = params
            .changes
            .iter()
            .filter(|change| change.typ == lsp_types::FileChangeType::DELETED)
            .map(|change| change.uri.clone())
            .collect();
        for uri in &deleted_uris {
            self.open_files.remove(uri);
            self.file_diagnostics.remove(uri);
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

        self.file_parses = merged;
        self.partial_file_parses = partial_parses;
        let docs: Vec<SpecDocument> = self.file_parses.values().cloned().collect();
        self.rebuild_graph_or_record_diagnostics(docs, &config, "file watch event");

        self.republish_all_diagnostics();
        for uri in &deleted_uris {
            self.publish_merged_diagnostics(uri);
        }
        self.notify_documents_changed();

        tracing::info!(
            changes = params.changes.len(),
            "re-indexed after file watch event"
        );
    }

    fn rebuild_graph_or_record_diagnostics(
        &mut self,
        docs: Vec<SpecDocument>,
        config: &Config,
        context: &'static str,
    ) {
        self.graph_diagnostics.clear();
        match build_graph(docs, config) {
            Ok(graph) => {
                self.graph = Arc::new(graph);
                self.run_verify_and_publish();
            }
            Err(errors) => {
                tracing::warn!(
                    error_count = errors.len(),
                    context,
                    "graph rebuild failed, retaining last-good graph"
                );
                let id_to_path = self.build_id_to_path();
                let pairs: Vec<(Url, Diagnostic)> = errors
                    .iter()
                    .flat_map(|error| {
                        graph_error_to_diagnostic_with_lookup(error, |doc_id| {
                            id_to_path.get(doc_id).cloned()
                        })
                    })
                    .collect();
                for (uri, diagnostics) in group_by_url(pairs) {
                    self.graph_diagnostics.insert(uri, diagnostics);
                }
            }
        }
    }
}
