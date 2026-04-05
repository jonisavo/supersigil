//! CLI-specific plugin helpers: repository resolution and warning display.
//!
//! Evidence assembly and collection live in `supersigil_verify::plugins`.
//! This module provides CLI-specific wrappers that use `ColorConfig` for
//! formatted terminal output.

use std::path::Path;

use supersigil_core::{Config, RepositoryConfig};

use crate::format::ColorConfig;
use supersigil_evidence::{EcosystemPlugin, RepositoryInfo};

// Re-export for callers that use `plugins::build_evidence` etc.
pub use supersigil_verify::plugins::{
    PluginEvidenceResult, assemble_plugins, build_evidence, collect_plugin_evidence,
};

// ---------------------------------------------------------------------------
// Repository resolution helpers
// ---------------------------------------------------------------------------

/// Convert a `RepositoryConfig` to a `RepositoryInfo`, resolving defaults.
///
/// Returns `None` if the config is for Gitea without a host — the user must
/// provide a host for Gitea since there is no canonical default.
fn config_to_repository_info(
    config: &RepositoryConfig,
    color: ColorConfig,
) -> Option<RepositoryInfo> {
    let host = config
        .host
        .clone()
        .or_else(|| config.provider.default_host().map(str::to_owned));

    let Some(host) = host else {
        // Gitea with no host configured — config error.
        eprintln!(
            "{} repository provider 'gitea' requires an explicit `host` in \
             [documentation.repository]; skipping repository resolution",
            color.warn(),
        );
        return None;
    };

    Some(RepositoryInfo {
        provider: config.provider,
        repo: config.repo.clone(),
        host,
        main_branch: config
            .main_branch
            .clone()
            .unwrap_or_else(|| "main".to_owned()),
    })
}

// ---------------------------------------------------------------------------
// resolve_repository_info
// ---------------------------------------------------------------------------

/// Resolve repository information from config or plugin metadata.
///
/// 1. If `config.documentation.repository` is `Some`, converts it to
///    `RepositoryInfo` and returns immediately (config wins).
/// 2. Otherwise, iterates the enabled plugins calling
///    `workspace_metadata(workspace_root)`. The first `Some` repository wins.
/// 3. Plugin `Err` results are logged as warnings on stderr.
/// 4. Returns `None` if nothing is found.
#[must_use]
pub fn resolve_repository_info(
    config: &Config,
    plugins: &[Box<dyn EcosystemPlugin>],
    workspace_root: &Path,
    color: ColorConfig,
) -> Option<RepositoryInfo> {
    // 1. Config wins
    if let Some(repo_config) = &config.documentation.repository {
        return config_to_repository_info(repo_config, color);
    }

    // 2. Plugin fallback — first Some wins
    for plugin in plugins {
        match plugin.workspace_metadata(workspace_root) {
            Ok(meta) => {
                if meta.repository.is_some() {
                    return meta.repository;
                }
            }
            Err(err) => {
                eprintln!(
                    "{} plugin '{}' workspace_metadata failed: {err}",
                    color.warn(),
                    plugin.name(),
                );
            }
        }
    }

    // 3. Nothing found
    None
}

/// Emit plugin findings as warnings on stderr.
///
/// Each finding's message is printed with the CLI's warning marker so that
/// plugin diagnostics remain visible even though they are non-fatal.
pub fn warn_plugin_findings(
    findings: &[supersigil_verify::Finding],
    color: crate::format::ColorConfig,
) {
    for f in findings {
        eprintln!("{} {}", color.warn(), f.message);
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::collections::{BTreeMap, HashMap};
    use std::path::PathBuf;

    use supersigil_core::EcosystemConfig;
    use supersigil_evidence::{ProjectScope, VerificationEvidenceRecord};
    use supersigil_rust::verifies;
    use supersigil_verify::test_helpers::pos;
    use supersigil_verify::{RuleName, build_artifact_graph};

    use super::*;

    fn base_config() -> Config {
        Config {
            paths: Some(vec!["specs/**/*.md".into()]),
            ecosystem: EcosystemConfig {
                plugins: vec![],
                rust: None,
            },
            ..Config::default()
        }
    }

    // -------------------------------------------------------------------
    // 1. Rust plugin assembled when enabled
    // -------------------------------------------------------------------

    #[verifies("ecosystem-plugins/req#req-1-1", "ecosystem-plugins/req#req-2-1")]
    #[test]
    fn rust_plugin_assembled_when_enabled() {
        let mut config = base_config();
        config.ecosystem.plugins = vec!["rust".into()];

        let plugins = assemble_plugins(&config);

        assert_eq!(plugins.len(), 1, "expected 1 plugin, got {}", plugins.len());
        assert_eq!(
            plugins[0].name(),
            "rust",
            "expected plugin name 'rust', got '{}'",
            plugins[0].name(),
        );
    }

    // -------------------------------------------------------------------
    // 2. No plugins when empty
    // -------------------------------------------------------------------

    #[test]
    fn no_plugins_when_empty() {
        let mut config = base_config();
        config.ecosystem.plugins = vec![];

        let plugins = assemble_plugins(&config);

        assert!(
            plugins.is_empty(),
            "expected empty plugin list, got {} plugins",
            plugins.len(),
        );
    }

    // -------------------------------------------------------------------
    // 3. Unknown plugin name is silently skipped
    // -------------------------------------------------------------------
    // (Unknown names are rejected at config load time; this tests
    //  the defensive fallback in assemble_plugins.)

    #[test]
    fn unknown_plugin_name_is_skipped() {
        let mut config = base_config();
        config.ecosystem.plugins = vec!["nonexistent".into()];

        let plugins = assemble_plugins(&config);

        assert!(
            plugins.is_empty(),
            "unknown plugin should be silently skipped, got {} plugins",
            plugins.len(),
        );
    }

    // -------------------------------------------------------------------
    // 4. Multiple plugins assembled preserves order
    // -------------------------------------------------------------------

    #[verifies("ecosystem-plugins/req#req-2-1")]
    #[test]
    fn multiple_plugins_assembled_in_order() {
        let mut config = base_config();
        // Currently only "rust" is known, but verify the pattern works
        // with duplicates (which config validation would normally reject).
        config.ecosystem.plugins = vec!["rust".into(), "rust".into()];

        let plugins = assemble_plugins(&config);

        assert_eq!(
            plugins.len(),
            2,
            "expected 2 plugins, got {}",
            plugins.len()
        );
        assert_eq!(plugins[0].name(), "rust");
        assert_eq!(plugins[1].name(), "rust");
    }

    // -------------------------------------------------------------------
    // 5. collect_plugin_evidence aggregates across plugins
    // -------------------------------------------------------------------

    #[test]
    fn collect_evidence_from_plugins_with_no_files() {
        let mut config = base_config();
        config.ecosystem.plugins = vec!["rust".into()];

        let plugins = assemble_plugins(&config);
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from("/tmp"),
        };
        let graph = {
            let cfg = base_config();
            supersigil_core::build_graph(vec![], &cfg).unwrap()
        };

        let result = collect_plugin_evidence(&plugins, &[], &scope, &graph);

        assert!(
            result.evidence.is_empty(),
            "expected no evidence from empty file list, got {} records",
            result.evidence.len(),
        );
        // Empty scope is gracefully handled — no findings emitted.
        assert!(
            result.findings.is_empty(),
            "empty scope should not produce plugin failure findings, got {:?}",
            result.findings,
        );
    }

    #[test]
    fn assembled_rust_plugin_plans_inferred_inputs_when_tests_absent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("tests").join("login_test.rs"),
            "#[test] fn ok() {}",
        )
        .unwrap();
        std::fs::write(dir.path().join("src").join("lib.rs"), "pub fn helper() {}").unwrap();

        let mut config = base_config();
        config.ecosystem.plugins = vec!["rust".into()];
        config.tests = None;

        let plugins = assemble_plugins(&config);
        assert_eq!(plugins.len(), 1);
        let test_files = supersigil_verify::resolve_test_files(&config, dir.path());
        let scope = ProjectScope {
            project: None,
            project_root: dir.path().to_path_buf(),
        };
        let files = plugins[0].plan_discovery_inputs(&test_files, &scope);

        assert!(
            files
                .iter()
                .any(|path| path.ends_with("tests/login_test.rs")),
            "expected inferred Rust discovery to include tests/**/*.rs, got {files:?}",
        );
        assert!(
            files.iter().any(|path| path.ends_with("src/lib.rs")),
            "expected inferred Rust discovery to include src/**/*.rs, got {files:?}",
        );
    }

    // -------------------------------------------------------------------
    // 5b. Plugin discovery failure produces finding (req-8-6)
    // -------------------------------------------------------------------

    #[derive(Debug)]
    struct FailingPlugin;

    impl supersigil_evidence::EcosystemPlugin for FailingPlugin {
        fn name(&self) -> &'static str {
            "failing"
        }

        fn discover(
            &self,
            _files: &[PathBuf],
            _scope: &ProjectScope,
            _documents: &supersigil_core::DocumentGraph,
        ) -> Result<supersigil_evidence::PluginDiscoveryResult, supersigil_evidence::PluginError>
        {
            Err(supersigil_evidence::PluginError::Discovery {
                plugin: "failing".into(),
                message: "simulated failure".into(),
                details: None,
            })
        }
    }

    #[verifies("ecosystem-plugins/req#req-3-3")]
    #[test]
    fn plugin_failure_produces_finding() {
        let plugins: Vec<Box<dyn supersigil_evidence::EcosystemPlugin>> =
            vec![Box::new(FailingPlugin)];
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from("/tmp"),
        };
        let graph = {
            let cfg = base_config();
            supersigil_core::build_graph(vec![], &cfg).unwrap()
        };

        let result = collect_plugin_evidence(&plugins, &[], &scope, &graph);

        assert!(
            result.evidence.is_empty(),
            "failing plugin should produce no evidence",
        );
        assert_eq!(
            result.findings.len(),
            1,
            "failing plugin should produce exactly 1 finding",
        );
        assert_eq!(
            result.findings[0].rule,
            RuleName::PluginDiscoveryFailure,
            "finding should be PluginDiscoveryFailure",
        );
        assert!(
            result.findings[0].message.contains("failing"),
            "finding message should mention plugin name, got: {}",
            result.findings[0].message,
        );
    }

    #[derive(Debug)]
    struct WarningPlugin;

    impl supersigil_evidence::EcosystemPlugin for WarningPlugin {
        fn name(&self) -> &'static str {
            "warning"
        }

        fn discover(
            &self,
            files: &[PathBuf],
            _scope: &ProjectScope,
            _documents: &supersigil_core::DocumentGraph,
        ) -> Result<supersigil_evidence::PluginDiscoveryResult, supersigil_evidence::PluginError>
        {
            Ok(supersigil_evidence::PluginDiscoveryResult {
                evidence: vec![VerificationEvidenceRecord {
                    id: supersigil_evidence::EvidenceId::new(0),
                    targets: supersigil_evidence::VerificationTargets::single(
                        supersigil_evidence::VerifiableRef {
                            doc_id: "req/mock".into(),
                            target_id: "crit-1".into(),
                        },
                    ),
                    test: supersigil_evidence::TestIdentity {
                        file: files[0].clone(),
                        name: "warning_test".into(),
                        kind: supersigil_evidence::TestKind::Unknown,
                    },
                    source_location: supersigil_evidence::SourceLocation {
                        file: files[0].clone(),
                        line: 1,
                        column: 1,
                    },
                    provenance: vec![supersigil_evidence::PluginProvenance::VerifiedByTag {
                        doc_id: "req/mock".into(),
                        tag: "warning".into(),
                    }],
                    metadata: BTreeMap::new(),
                }],
                diagnostics: vec![supersigil_evidence::PluginDiagnostic::warning(
                    "warning plugin skipped a file",
                )],
            })
        }
    }

    #[verifies("ecosystem-plugins/req#req-2-4", "ecosystem-plugins/req#req-3-3")]
    #[test]
    fn plugin_diagnostics_become_findings_without_dropping_evidence() {
        let plugins: Vec<Box<dyn supersigil_evidence::EcosystemPlugin>> =
            vec![Box::new(WarningPlugin)];
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from("/tmp"),
        };
        let graph = {
            let cfg = base_config();
            supersigil_core::build_graph(vec![], &cfg).unwrap()
        };
        let files = vec![PathBuf::from("/tmp/tests/warning_test.rs")];

        let result = collect_plugin_evidence(&plugins, &files, &scope, &graph);

        assert_eq!(
            result.evidence.len(),
            1,
            "warning plugin should keep evidence"
        );
        assert_eq!(
            result.findings.len(),
            1,
            "warning plugin should surface exactly 1 finding",
        );
        assert_eq!(result.findings[0].rule, RuleName::PluginDiscoveryWarning);
        assert!(
            result.findings[0]
                .message
                .contains("warning plugin skipped a file"),
            "finding should include structured diagnostic text, got: {}",
            result.findings[0].message,
        );
    }

    // -------------------------------------------------------------------
    // 5c. Plugin-owned discovery-input planning feeds discover()
    // -------------------------------------------------------------------

    #[verifies("ecosystem-plugins/req#req-2-2")]
    #[test]
    fn collect_plugin_evidence_uses_plugin_owned_discovery_inputs() {
        #[derive(Debug)]
        struct PlanningPlugin;

        impl supersigil_evidence::EcosystemPlugin for PlanningPlugin {
            fn name(&self) -> &'static str {
                "planning"
            }

            fn plan_discovery_inputs<'a>(
                &self,
                test_files: &'a [PathBuf],
                scope: &ProjectScope,
            ) -> Cow<'a, [PathBuf]> {
                let mut files = test_files.to_vec();
                files.push(scope.project_root.join("src/generated.rs"));
                Cow::Owned(files)
            }

            fn discover(
                &self,
                files: &[PathBuf],
                _scope: &ProjectScope,
                _documents: &supersigil_core::DocumentGraph,
            ) -> Result<supersigil_evidence::PluginDiscoveryResult, supersigil_evidence::PluginError>
            {
                let generated = files
                    .iter()
                    .find(|path| path.ends_with("src/generated.rs"))
                    .expect("planned discovery inputs should be passed to discover")
                    .clone();
                Ok(supersigil_evidence::PluginDiscoveryResult {
                    evidence: vec![VerificationEvidenceRecord {
                        id: supersigil_evidence::EvidenceId::new(0),
                        targets: supersigil_evidence::VerificationTargets::single(
                            supersigil_evidence::VerifiableRef {
                                doc_id: "req/mock".into(),
                                target_id: "crit-1".into(),
                            },
                        ),
                        test: supersigil_evidence::TestIdentity {
                            file: generated.clone(),
                            name: "planned_input_test".into(),
                            kind: supersigil_evidence::TestKind::Unknown,
                        },
                        source_location: supersigil_evidence::SourceLocation {
                            file: generated,
                            line: 1,
                            column: 1,
                        },
                        provenance: vec![supersigil_evidence::PluginProvenance::VerifiedByTag {
                            doc_id: "req/mock".into(),
                            tag: "planned".into(),
                        }],
                        metadata: BTreeMap::new(),
                    }],
                    diagnostics: Vec::new(),
                })
            }
        }

        let plugins: Vec<Box<dyn supersigil_evidence::EcosystemPlugin>> =
            vec![Box::new(PlanningPlugin)];
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from("/tmp/workspace"),
        };
        let graph = {
            let cfg = base_config();
            supersigil_core::build_graph(vec![], &cfg).unwrap()
        };

        let result = collect_plugin_evidence(&plugins, &[], &scope, &graph);

        assert_eq!(result.evidence.len(), 1);
        assert_eq!(
            result.evidence[0].test.file,
            PathBuf::from("/tmp/workspace/src/generated.rs"),
        );
        assert!(result.findings.is_empty());
    }

    // -------------------------------------------------------------------
    // 6. End-to-end: assembly -> discovery -> artifact graph
    // -------------------------------------------------------------------

    fn make_requirement_doc() -> supersigil_core::SpecDocument {
        supersigil_core::SpecDocument {
            path: PathBuf::from("specs/req/auth.md"),
            frontmatter: supersigil_core::Frontmatter {
                id: "req/auth".into(),
                doc_type: None,
                status: None,
            },
            extra: HashMap::new(),
            warnings: vec![],
            components: vec![supersigil_core::ExtractedComponent {
                name: "AcceptanceCriteria".into(),
                attributes: HashMap::new(),
                children: vec![supersigil_core::ExtractedComponent {
                    name: "Criterion".into(),
                    attributes: HashMap::from([("id".into(), "crit-1".into())]),
                    children: vec![supersigil_core::ExtractedComponent {
                        name: "VerifiedBy".into(),
                        attributes: HashMap::from([
                            ("strategy".into(), "tag".into()),
                            ("tag".into(), "prop:auth".into()),
                        ]),
                        children: vec![],
                        body_text: None,
                        body_text_offset: None,
                        body_text_end_offset: None,
                        code_blocks: vec![],
                        position: pos(11),
                        end_position: pos(11),
                    }],
                    body_text: Some("criterion crit-1".into()),
                    body_text_offset: None,
                    body_text_end_offset: None,
                    code_blocks: vec![],
                    position: pos(10),
                    end_position: pos(10),
                }],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: pos(9),
                end_position: pos(9),
            }],
        }
    }

    fn make_property_doc() -> supersigil_core::SpecDocument {
        supersigil_core::SpecDocument {
            path: PathBuf::from("specs/prop/auth.md"),
            frontmatter: supersigil_core::Frontmatter {
                id: "prop/auth".into(),
                doc_type: None,
                status: None,
            },
            extra: HashMap::new(),
            warnings: vec![],
            components: vec![supersigil_core::ExtractedComponent {
                name: "References".into(),
                attributes: HashMap::from([("refs".into(), "req/auth#crit-1".into())]),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: pos(5),
                end_position: pos(5),
            }],
        }
    }

    fn setup_e2e_fixture() -> (tempfile::TempDir, Config, supersigil_core::DocumentGraph) {
        let dir = tempfile::TempDir::new().unwrap();
        let test_dir = dir.path().join("tests");
        std::fs::create_dir_all(&test_dir).unwrap();

        // Rust test file with verifies attribute
        std::fs::write(
            test_dir.join("auth_test.rs"),
            "#[test]\n#[verifies(\"req/auth#crit-1\")]\nfn test_login_succeeds() {\n    assert!(true);\n}\n",
        )
        .unwrap();

        // Tag-based test file for explicit evidence
        std::fs::write(
            test_dir.join("tag_test.rs"),
            "// supersigil: prop:auth\nfn test_tag() {}\n",
        )
        .unwrap();

        let docs = vec![make_requirement_doc(), make_property_doc()];
        let mut config = base_config();
        config.ecosystem.plugins = vec!["rust".into()];
        config.tests = Some(vec!["tests/**/*.rs".into()]);
        let graph = supersigil_core::build_graph(docs, &config).unwrap();

        (dir, config, graph)
    }

    #[verifies("ecosystem-plugins/req#req-2-3")]
    #[test]
    fn end_to_end_plugin_assembly_and_artifact_graph() {
        let (dir, config, graph) = setup_e2e_fixture();

        // Assemble plugins and discover evidence
        let plugins = assemble_plugins(&config);
        assert_eq!(plugins.len(), 1);

        let test_files = supersigil_verify::resolve_test_files(&config, dir.path());
        assert!(!test_files.is_empty(), "should resolve test files");

        let scope = ProjectScope {
            project: None,
            project_root: dir.path().to_path_buf(),
        };
        let plugin_result = collect_plugin_evidence(&plugins, &test_files, &scope, &graph);
        assert!(
            !plugin_result.evidence.is_empty(),
            "Rust plugin should find evidence"
        );
        assert!(
            plugin_result.findings.is_empty(),
            "expected no plugin failures"
        );

        let tag_matches = supersigil_verify::scan_all_tags(&test_files);
        let explicit_evidence =
            supersigil_verify::extract_explicit_evidence(&graph, &tag_matches, dir.path());
        assert!(
            !explicit_evidence.is_empty(),
            "should find tag-based evidence"
        );

        // Build artifact graph merging both sources
        let ag = build_artifact_graph(&graph, explicit_evidence, plugin_result.evidence);
        assert!(
            !ag.evidence.is_empty(),
            "artifact graph should have evidence"
        );

        assert!(
            ag.has_evidence("req/auth", "crit-1"),
            "criterion req/auth#crit-1 should be indexed",
        );
        assert!(ag.conflicts.is_empty(), "expected no conflicts");
    }

    // -------------------------------------------------------------------
    // 7. Workspace member crate traversal
    // -------------------------------------------------------------------

    // -------------------------------------------------------------------
    // 8. resolve_repository_info
    // -------------------------------------------------------------------

    mod resolve_repository {
        use supersigil_core::{DocumentationConfig, RepositoryProvider};
        use supersigil_evidence::WorkspaceMetadata;
        use supersigil_rust::verifies;

        use super::*;

        fn color() -> ColorConfig {
            ColorConfig::no_color()
        }

        // Helper: stub plugin returning Some repository info
        #[derive(Debug)]
        struct MetadataPlugin {
            repo_info: Option<RepositoryInfo>,
        }

        impl supersigil_evidence::EcosystemPlugin for MetadataPlugin {
            fn name(&self) -> &'static str {
                "metadata"
            }

            fn workspace_metadata(
                &self,
                _workspace_root: &Path,
            ) -> Result<WorkspaceMetadata, supersigil_evidence::PluginError> {
                Ok(WorkspaceMetadata {
                    repository: self.repo_info.clone(),
                })
            }

            fn discover(
                &self,
                _files: &[PathBuf],
                _scope: &ProjectScope,
                _documents: &supersigil_core::DocumentGraph,
            ) -> Result<supersigil_evidence::PluginDiscoveryResult, supersigil_evidence::PluginError>
            {
                Ok(supersigil_evidence::PluginDiscoveryResult::default())
            }
        }

        // Helper: stub plugin that returns Err from workspace_metadata
        #[derive(Debug)]
        struct FailingMetadataPlugin;

        impl supersigil_evidence::EcosystemPlugin for FailingMetadataPlugin {
            fn name(&self) -> &'static str {
                "failing-meta"
            }

            fn workspace_metadata(
                &self,
                _workspace_root: &Path,
            ) -> Result<WorkspaceMetadata, supersigil_evidence::PluginError> {
                Err(supersigil_evidence::PluginError::Discovery {
                    plugin: "failing-meta".into(),
                    message: "simulated metadata failure".into(),
                    details: None,
                })
            }

            fn discover(
                &self,
                _files: &[PathBuf],
                _scope: &ProjectScope,
                _documents: &supersigil_core::DocumentGraph,
            ) -> Result<supersigil_evidence::PluginDiscoveryResult, supersigil_evidence::PluginError>
            {
                Ok(supersigil_evidence::PluginDiscoveryResult::default())
            }
        }

        fn github_config() -> RepositoryConfig {
            RepositoryConfig {
                provider: RepositoryProvider::GitHub,
                repo: "owner/repo".into(),
                host: None,
                main_branch: None,
            }
        }

        fn github_info() -> RepositoryInfo {
            RepositoryInfo {
                provider: RepositoryProvider::GitHub,
                repo: "plugin/discovered".into(),
                host: "github.com".into(),
                main_branch: "main".into(),
            }
        }

        // 8a. Config wins over plugins (req-5-3)
        #[verifies("ecosystem-plugins/req#req-5-3")]
        #[test]
        fn config_wins_over_plugins() {
            let mut config = base_config();
            config.documentation = DocumentationConfig {
                repository: Some(github_config()),
            };

            let plugins: Vec<Box<dyn supersigil_evidence::EcosystemPlugin>> =
                vec![Box::new(MetadataPlugin {
                    repo_info: Some(github_info()),
                })];

            let result = resolve_repository_info(&config, &plugins, Path::new("/tmp"), color());

            let info = result.expect("config should produce Some");
            assert_eq!(info.provider, RepositoryProvider::GitHub);
            assert_eq!(
                info.repo, "owner/repo",
                "config repo should win, not plugin's"
            );
            assert_eq!(info.host, "github.com");
            assert_eq!(info.main_branch, "main");
        }

        // 8b. Plugin fallback when config has no repository (req-5-1)
        #[verifies("ecosystem-plugins/req#req-5-1")]
        #[test]
        fn plugin_fallback_when_config_absent() {
            let config = base_config(); // no documentation.repository

            let plugins: Vec<Box<dyn supersigil_evidence::EcosystemPlugin>> =
                vec![Box::new(MetadataPlugin {
                    repo_info: Some(github_info()),
                })];

            let result = resolve_repository_info(&config, &plugins, Path::new("/tmp"), color());

            let info = result.expect("plugin should produce Some");
            assert_eq!(info.provider, RepositoryProvider::GitHub);
            assert_eq!(info.repo, "plugin/discovered");
        }

        // 8c. No result when config absent and plugins return None
        #[test]
        fn no_result_when_nothing_available() {
            let config = base_config();

            let plugins: Vec<Box<dyn supersigil_evidence::EcosystemPlugin>> =
                vec![Box::new(MetadataPlugin { repo_info: None })];

            let result = resolve_repository_info(&config, &plugins, Path::new("/tmp"), color());

            assert!(result.is_none(), "expected None when nothing available");
        }

        // 8d. Plugin errors logged as warnings, resolution continues
        #[verifies("ecosystem-plugins/req#req-5-1")]
        #[test]
        fn plugin_error_logged_and_skipped() {
            let config = base_config();

            // First plugin fails, second succeeds
            let plugins: Vec<Box<dyn supersigil_evidence::EcosystemPlugin>> = vec![
                Box::new(FailingMetadataPlugin),
                Box::new(MetadataPlugin {
                    repo_info: Some(github_info()),
                }),
            ];

            let result = resolve_repository_info(&config, &plugins, Path::new("/tmp"), color());

            let info = result.expect("second plugin should produce Some after first fails");
            assert_eq!(info.repo, "plugin/discovered");
        }

        // 8e. First plugin with Some wins (first-wins semantics)
        #[test]
        fn first_plugin_some_wins() {
            let config = base_config();

            let first_info = RepositoryInfo {
                provider: RepositoryProvider::GitLab,
                repo: "first/plugin".into(),
                host: "gitlab.com".into(),
                main_branch: "main".into(),
            };
            let second_info = RepositoryInfo {
                provider: RepositoryProvider::GitHub,
                repo: "second/plugin".into(),
                host: "github.com".into(),
                main_branch: "main".into(),
            };

            let plugins: Vec<Box<dyn supersigil_evidence::EcosystemPlugin>> = vec![
                Box::new(MetadataPlugin {
                    repo_info: Some(first_info),
                }),
                Box::new(MetadataPlugin {
                    repo_info: Some(second_info),
                }),
            ];

            let result = resolve_repository_info(&config, &plugins, Path::new("/tmp"), color());

            let info = result.expect("first plugin should win");
            assert_eq!(info.repo, "first/plugin");
            assert_eq!(info.provider, RepositoryProvider::GitLab);
        }

        // 8f. Config conversion: defaults resolved for each provider
        #[test]
        fn config_defaults_github() {
            let config_repo = RepositoryConfig {
                provider: RepositoryProvider::GitHub,
                repo: "owner/repo".into(),
                host: None,
                main_branch: None,
            };
            let info = config_to_repository_info(&config_repo, color()).unwrap();
            assert_eq!(info.host, "github.com");
            assert_eq!(info.main_branch, "main");
        }

        #[test]
        fn config_defaults_gitlab() {
            let config_repo = RepositoryConfig {
                provider: RepositoryProvider::GitLab,
                repo: "group/project".into(),
                host: None,
                main_branch: None,
            };
            let info = config_to_repository_info(&config_repo, color()).unwrap();
            assert_eq!(info.host, "gitlab.com");
            assert_eq!(info.provider, RepositoryProvider::GitLab);
        }

        #[test]
        fn config_defaults_bitbucket() {
            let config_repo = RepositoryConfig {
                provider: RepositoryProvider::Bitbucket,
                repo: "team/project".into(),
                host: None,
                main_branch: None,
            };
            let info = config_to_repository_info(&config_repo, color()).unwrap();
            assert_eq!(info.host, "bitbucket.org");
            assert_eq!(info.provider, RepositoryProvider::Bitbucket);
        }

        #[test]
        fn config_gitea_with_host() {
            let config_repo = RepositoryConfig {
                provider: RepositoryProvider::Gitea,
                repo: "owner/repo".into(),
                host: Some("gitea.example.com".into()),
                main_branch: Some("develop".into()),
            };
            let info = config_to_repository_info(&config_repo, color()).unwrap();
            assert_eq!(info.host, "gitea.example.com");
            assert_eq!(info.main_branch, "develop");
            assert_eq!(info.provider, RepositoryProvider::Gitea);
        }

        #[test]
        fn config_gitea_without_host_returns_none() {
            let config_repo = RepositoryConfig {
                provider: RepositoryProvider::Gitea,
                repo: "owner/repo".into(),
                host: None,
                main_branch: None,
            };
            let result = config_to_repository_info(&config_repo, color());
            assert!(
                result.is_none(),
                "Gitea without host should return None (config error)"
            );
        }

        #[test]
        fn config_custom_host_overrides_default() {
            let config_repo = RepositoryConfig {
                provider: RepositoryProvider::GitHub,
                repo: "owner/repo".into(),
                host: Some("github.enterprise.com".into()),
                main_branch: Some("trunk".into()),
            };
            let info = config_to_repository_info(&config_repo, color()).unwrap();
            assert_eq!(info.host, "github.enterprise.com");
            assert_eq!(info.main_branch, "trunk");
        }

        // 8g. Empty plugin list returns None
        #[test]
        fn empty_plugins_returns_none() {
            let config = base_config();
            let plugins: Vec<Box<dyn supersigil_evidence::EcosystemPlugin>> = vec![];

            let result = resolve_repository_info(&config, &plugins, Path::new("/tmp"), color());

            assert!(result.is_none());
        }
    }
}
