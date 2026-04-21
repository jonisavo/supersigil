#[allow(
    clippy::wildcard_imports,
    reason = "task scaffold requires `use super::*`"
)]
use super::*;

impl SupersigilLsp {
    pub(super) fn effective_component_defs(config: Option<&Config>) -> ComponentDefs {
        config
            .and_then(|c| {
                ComponentDefs::merge(ComponentDefs::defaults(), c.components.clone()).ok()
            })
            .unwrap_or_else(ComponentDefs::defaults)
    }

    pub(super) fn normalize_live_content(content: String) -> Arc<String> {
        if content.contains('\r') || content.starts_with('\u{feff}') {
            Arc::new(supersigil_parser::normalize(&content))
        } else {
            Arc::new(content)
        }
    }

    pub(super) fn publish_merged_diagnostics(&self, uri: &Url) {
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
    pub(super) fn uri_is_owned(&self, uri: &lsp_types::Url) -> bool {
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
            if root.join(parent).join(CONFIG_FILENAME).is_file() {
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
    pub(super) fn uri_to_relative_key(&self, uri: &lsp_types::Url) -> Option<PathBuf> {
        let abs = uri.to_file_path().ok()?;
        if let Some(root) = &self.project_root {
            Some(abs.strip_prefix(root).map(Path::to_path_buf).unwrap_or(abs))
        } else {
            Some(abs)
        }
    }

    pub(super) fn build_id_to_path(&self) -> HashMap<String, PathBuf> {
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
    pub(super) fn is_project_file(&self, uri: &Url) -> bool {
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

    pub(super) fn reparse_and_publish(&mut self, uri: &Url, content: &str) {
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

    pub(super) fn republish_all_diagnostics(&self) {
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

    pub(super) fn run_verify_and_publish(&mut self) {
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
}

pub(super) fn discover_config(root: &Path) -> Option<(PathBuf, Config)> {
    let config_path = find_config(root).ok()??;
    let config = load_config(&config_path).ok()?;
    Some((config_path, config))
}

pub(super) fn discover_files(config: &Config, root: &Path) -> Vec<PathBuf> {
    let globs = collect_globs(config);
    expand_globs(globs, root)
}

pub(super) fn collect_globs(config: &Config) -> Vec<&str> {
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

pub(super) fn parse_all_files(
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

pub(super) fn empty_graph() -> DocumentGraph {
    build_graph(Vec::new(), &Config::default()).unwrap_or_else(|_| {
        panic!("failed to build empty graph");
    })
}
