//! Plugin assembly: creates ecosystem plugin instances from the config.
//!
//! The CLI reads `ecosystem.plugins` from the config and creates the
//! corresponding plugin instances.  Unknown plugin names have already been
//! rejected at config-load time (`supersigil-core`), so this module only
//! needs a match arm for each known built-in plugin.

use std::path::{Path, PathBuf};

use supersigil_core::{Config, DocumentGraph};
use supersigil_evidence::{
    EcosystemPlugin, PluginDiagnostic, PluginDiscoveryResult, PluginError, ProjectScope,
    VerificationEvidenceRecord,
};
use supersigil_verify::extract_explicit_evidence;
use supersigil_verify::{ArtifactGraph, build_artifact_graph};
use supersigil_verify::{Finding, FindingDetails, RuleName};

/// Assemble the enabled ecosystem plugin instances from the config.
///
/// Reads `config.ecosystem.plugins` and creates the corresponding plugin
/// instances.  Unknown plugin names have already been rejected at config load
/// time, so if we encounter an unrecognised name here it means a new plugin
/// was added to the config schema but not to this match arm.
#[must_use]
pub fn assemble_plugins(config: &Config) -> Vec<Box<dyn EcosystemPlugin>> {
    let mut plugins: Vec<Box<dyn EcosystemPlugin>> = Vec::new();
    for name in &config.ecosystem.plugins {
        // Use match rather than if-else to make it easy to add new plugins.
        #[allow(
            clippy::single_match_else,
            reason = "match is clearer for future plugin arms"
        )]
        match name.as_str() {
            "rust" => plugins.push(Box::new(supersigil_rust::RustPlugin)),
            _ => {
                // Unknown plugins are rejected at config load time.
                // If we get here, it means a new plugin was added to the
                // config schema but not to this match arm.
            }
        }
    }
    plugins
}

/// Result of collecting plugin evidence, including any plugin warning/failure
/// findings.
#[derive(Debug)]
pub struct PluginEvidenceResult {
    /// Evidence records from all successful plugins.
    pub evidence: Vec<VerificationEvidenceRecord>,
    /// Findings for plugin diagnostics and fatal failures surfaced as
    /// verification findings.
    pub findings: Vec<Finding>,
}

/// Collect evidence from all enabled plugins.
///
/// Lets each plugin plan its effective discovery inputs from the shared test
/// files and project scope, then runs `discover` against those plugin-owned
/// inputs. Plugin diagnostics and fatal errors become verification findings
/// rather than hard errors (req-8-6).
#[must_use]
pub fn collect_plugin_evidence(
    plugins: &[Box<dyn EcosystemPlugin>],
    test_files: &[PathBuf],
    scope: &ProjectScope,
    documents: &DocumentGraph,
) -> PluginEvidenceResult {
    let mut evidence = Vec::new();
    let mut findings = Vec::new();
    for plugin in plugins {
        let files = plugin.plan_discovery_inputs(test_files, scope);
        match plugin.discover(&files, scope, documents) {
            Ok(PluginDiscoveryResult {
                evidence: mut plugin_evidence,
                diagnostics,
            }) => {
                evidence.append(&mut plugin_evidence);
                findings.extend(
                    diagnostics
                        .into_iter()
                        .map(|diagnostic| plugin_diagnostic_to_finding(plugin.name(), &diagnostic)),
                );
            }
            Err(err) => {
                findings.push(
                    Finding::new(
                        RuleName::PluginDiscoveryFailure,
                        None,
                        plugin_failure_message(plugin.name(), &err),
                        None,
                    )
                    .with_details(plugin_error_details(plugin.name(), &err)),
                );
            }
        }
    }
    PluginEvidenceResult { evidence, findings }
}

fn plugin_diagnostic_to_finding(plugin_name: &str, diagnostic: &PluginDiagnostic) -> Finding {
    let path = diagnostic
        .path
        .as_ref()
        .map(|value| value.to_string_lossy().into_owned());
    let message = match diagnostic.path.as_ref() {
        Some(path) => format!(
            "plugin '{plugin_name}': {}: {}",
            path.display(),
            diagnostic.message
        ),
        None => format!("plugin '{plugin_name}': {}", diagnostic.message),
    };

    Finding::new(RuleName::PluginDiscoveryWarning, None, message, None).with_details(
        FindingDetails {
            plugin: Some(plugin_name.to_string()),
            path,
            ..FindingDetails::default()
        },
    )
}

fn plugin_failure_message(plugin_name: &str, err: &PluginError) -> String {
    if let PluginError::Discovery {
        message, details, ..
    } = err
        && let Some(location) = details.as_deref().and_then(|details| {
            format_plugin_location(details.path.as_deref(), details.line, details.column)
        })
    {
        format!("plugin '{plugin_name}' failed: {location}: {message}")
    } else {
        err.to_string()
    }
}

fn plugin_error_details(plugin_name: &str, err: &PluginError) -> FindingDetails {
    match err {
        PluginError::ParseFailure { file, .. } => FindingDetails {
            plugin: Some(plugin_name.to_string()),
            path: Some(file.to_string_lossy().into_owned()),
            code: Some("parse_failure".to_string()),
            ..FindingDetails::default()
        },
        PluginError::Discovery { details, .. } => details.as_deref().map_or_else(
            || FindingDetails {
                plugin: Some(plugin_name.to_string()),
                ..FindingDetails::default()
            },
            |details| FindingDetails {
                plugin: Some(plugin_name.to_string()),
                path: details
                    .path
                    .as_ref()
                    .map(|value| value.to_string_lossy().into_owned()),
                line: details.line,
                column: details.column,
                code: details.code.clone(),
                suggestion: details.suggestion.clone(),
                ..FindingDetails::default()
            },
        ),
        PluginError::Io { path, .. } => FindingDetails {
            plugin: Some(plugin_name.to_string()),
            path: Some(path.to_string_lossy().into_owned()),
            code: Some("io_error".to_string()),
            ..FindingDetails::default()
        },
    }
}

fn format_plugin_location(
    path: Option<&Path>,
    line: Option<usize>,
    column: Option<usize>,
) -> Option<String> {
    use std::fmt::Write;
    let path = path?;
    let mut location = path.display().to_string();
    if let Some(line) = line {
        let _ = write!(location, ":{line}");
        if let Some(column) = column {
            let _ = write!(location, ":{column}");
        }
    }
    Some(location)
}

/// Build an `ArtifactGraph` from the full evidence pipeline.
///
/// Assembles ecosystem plugins, collects plugin evidence using pre-resolved
/// test files, extracts explicit `<VerifiedBy>` evidence using pre-scanned
/// tags, and merges everything into an `ArtifactGraph`.
///
/// `inputs` should be pre-resolved via [`supersigil_verify::VerifyInputs::resolve`]
/// to avoid redundant glob expansion and tag scanning.
///
/// Returns the artifact graph and any plugin failure findings.
#[must_use]
pub fn build_evidence<'g>(
    config: &Config,
    graph: &'g DocumentGraph,
    project_root: &Path,
    project: Option<&str>,
    inputs: &supersigil_verify::VerifyInputs,
) -> (ArtifactGraph<'g>, Vec<Finding>) {
    let enabled_plugins = assemble_plugins(config);
    let scope = ProjectScope {
        project: project.map(str::to_owned),
        project_root: project_root.to_path_buf(),
    };
    let plugin_result =
        collect_plugin_evidence(&enabled_plugins, &inputs.test_files, &scope, graph);
    let explicit_evidence = extract_explicit_evidence(graph, &inputs.tag_matches, project_root);
    let artifact_graph = build_artifact_graph(graph, explicit_evidence, plugin_result.evidence);
    (artifact_graph, plugin_result.findings)
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

    use supersigil_core::EcosystemConfig;
    use supersigil_rust::verifies;
    use supersigil_verify::test_helpers::pos;

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
            _documents: &DocumentGraph,
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
            _documents: &DocumentGraph,
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
                _documents: &DocumentGraph,
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
        use supersigil_verify::build_artifact_graph;
        use supersigil_verify::extract_explicit_evidence;

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
        let explicit_evidence = extract_explicit_evidence(&graph, &tag_matches, dir.path());
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
}
