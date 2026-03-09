//! Plugin assembly: creates ecosystem plugin instances from the config.
//!
//! The CLI reads `ecosystem.plugins` from the config and creates the
//! corresponding plugin instances.  Unknown plugin names have already been
//! rejected at config-load time (`supersigil-core`), so this module only
//! needs a match arm for each known built-in plugin.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use supersigil_core::{Config, DocumentGraph};
use supersigil_evidence::{
    EcosystemPlugin, PluginDiagnostic, PluginDiscoveryResult, ProjectScope,
    VerificationEvidenceRecord,
};
use supersigil_verify::artifact_graph::{ArtifactGraph, build_artifact_graph};
use supersigil_verify::explicit_evidence::extract_explicit_evidence;
use supersigil_verify::{Finding, RuleName};

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
/// Runs each plugin's `discover` method against the resolved source files and
/// returns the aggregated evidence records. Plugin diagnostics and fatal errors
/// become verification findings rather than hard errors (req-8-6).
#[must_use]
pub fn collect_plugin_evidence(
    plugins: &[Box<dyn EcosystemPlugin>],
    files: &[PathBuf],
    scope: &ProjectScope,
    documents: &DocumentGraph,
) -> PluginEvidenceResult {
    let mut evidence = Vec::new();
    let mut findings = Vec::new();
    for plugin in plugins {
        match plugin.discover(files, scope, documents) {
            Ok(PluginDiscoveryResult {
                evidence: mut plugin_evidence,
                diagnostics,
            }) => {
                evidence.append(&mut plugin_evidence);
                findings.extend(
                    diagnostics
                        .into_iter()
                        .map(|diagnostic| plugin_diagnostic_to_finding(plugin.name(), diagnostic)),
                );
            }
            Err(err) => {
                findings.push(Finding::new(
                    RuleName::PluginDiscoveryFailure,
                    None,
                    format!("plugin '{}' failed: {err}", plugin.name()),
                    None,
                ));
            }
        }
    }
    PluginEvidenceResult { evidence, findings }
}

fn plugin_diagnostic_to_finding(plugin_name: &str, diagnostic: PluginDiagnostic) -> Finding {
    let message = match diagnostic.path {
        Some(path) => format!(
            "plugin '{plugin_name}': {}: {}",
            path.display(),
            diagnostic.message
        ),
        None => format!("plugin '{plugin_name}': {}", diagnostic.message),
    };

    Finding::new(RuleName::PluginDiscoveryWarning, None, message, None)
}

/// Resolve source files for plugin discovery.
///
/// Accepts pre-resolved test files (from `resolve_test_files`) and augments
/// them with inferred Rust source locations when the Rust plugin is enabled.
#[must_use]
pub fn resolve_plugin_files(
    test_files: &[PathBuf],
    config: &Config,
    project_root: &Path,
) -> Vec<PathBuf> {
    if !config.ecosystem.plugins.iter().any(|name| name == "rust") {
        return test_files.to_vec();
    }

    // For the Rust plugin, always include inferred Rust source locations
    // (src/, tests/, benches/, examples/) so that #[verifies] attributes
    // in inline unit tests under src/ are not missed when test globs are set.
    let inferred = infer_rust_source_files(project_root);

    if test_files.is_empty() && inferred.is_empty() {
        return Vec::new();
    }

    let mut combined = BTreeSet::new();
    for f in test_files {
        combined.insert(f.clone());
    }
    for f in inferred {
        combined.insert(f);
    }
    combined.into_iter().collect()
}

/// Build an `ArtifactGraph` from the full evidence pipeline.
///
/// Assembles ecosystem plugins, resolves source files, collects plugin
/// evidence, extracts explicit `<VerifiedBy>` evidence, and merges
/// everything into an `ArtifactGraph`.
///
/// Test file glob expansion (`resolve_test_files`) is performed once and the
/// resolved list is shared by both plugin discovery and explicit evidence
/// extraction (E2).
///
/// Returns the artifact graph and any plugin failure findings.
#[must_use]
pub fn build_evidence<'g>(
    config: &Config,
    graph: &'g DocumentGraph,
    project_root: &Path,
    project: Option<&str>,
) -> (ArtifactGraph<'g>, Vec<Finding>) {
    let enabled_plugins = assemble_plugins(config);
    let test_files = supersigil_verify::resolve_test_files(config, project_root);
    let source_files = resolve_plugin_files(&test_files, config, project_root);
    let scope = ProjectScope {
        project: project.map(str::to_owned),
        project_root: project_root.to_path_buf(),
    };
    let plugin_result = collect_plugin_evidence(&enabled_plugins, &source_files, &scope, graph);
    let explicit_evidence = extract_explicit_evidence(graph, &test_files, project_root);
    let artifact_graph = build_artifact_graph(graph, explicit_evidence, plugin_result.evidence);
    (artifact_graph, plugin_result.findings)
}

/// Emit plugin findings as warnings on stderr.
///
/// Each finding's message is printed with the CLI's warning marker so that
/// plugin diagnostics remain visible even though they are non-fatal.
pub fn warn_plugin_findings(
    findings: &[supersigil_verify::Finding],
    color: &crate::format::ColorConfig,
) {
    for f in findings {
        eprintln!("{} {}", color.warn(), f.message);
    }
}

fn infer_rust_source_files(project_root: &Path) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();

    // Collect root-level Rust sources (single-crate layout).
    let root_dirs = ["tests", "src", "benches", "examples"];
    for dir in root_dirs {
        glob_rs_files(&project_root.join(dir), &mut files);
    }

    // Traverse workspace member crates from Cargo.toml.
    for member_dir in read_workspace_member_dirs(project_root) {
        for dir in root_dirs {
            glob_rs_files(&member_dir.join(dir), &mut files);
        }
    }

    files.into_iter().collect()
}

/// Glob `**/*.rs` under `dir` and insert matches into `files`.
fn glob_rs_files(dir: &Path, files: &mut BTreeSet<PathBuf>) {
    let pattern = dir.join("**/*.rs").to_string_lossy().to_string();
    if let Ok(entries) = glob::glob(&pattern) {
        for entry in entries.flatten() {
            if !path_contains_fixture_dir(&entry) {
                files.insert(entry);
            }
        }
    }
}

fn path_contains_fixture_dir(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "fixtures")
}

/// Read workspace member directories from `Cargo.toml`.
///
/// Parses `[workspace] members = [...]` and expands glob patterns in member
/// paths (e.g., `"crates/*"`). Returns absolute paths to member directories.
fn read_workspace_member_dirs(project_root: &Path) -> Vec<PathBuf> {
    let cargo_path = project_root.join("Cargo.toml");
    let Ok(content) = std::fs::read_to_string(&cargo_path) else {
        return Vec::new();
    };
    let Ok(table) = content.parse::<toml::Table>() else {
        return Vec::new();
    };

    let Some(members) = table
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    else {
        return Vec::new();
    };

    let mut dirs = Vec::new();
    for member in members {
        let Some(pattern) = member.as_str() else {
            continue;
        };
        let full = project_root.join(pattern).to_string_lossy().to_string();
        if let Ok(entries) = glob::glob(&full) {
            for entry in entries.flatten() {
                if entry.is_dir() {
                    dirs.push(entry);
                }
            }
        } else {
            // Non-glob literal path.
            let dir = project_root.join(pattern);
            if dir.is_dir() {
                dirs.push(dir);
            }
        }
    }
    dirs
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use supersigil_core::EcosystemConfig;

    use super::*;

    fn base_config() -> Config {
        Config {
            paths: Some(vec!["specs/**/*.mdx".into()]),
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
    fn resolve_plugin_files_infers_rust_defaults_when_tests_absent() {
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

        let test_files = supersigil_verify::resolve_test_files(&config, dir.path());
        let files = resolve_plugin_files(&test_files, &config, dir.path());

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

    #[test]
    fn collect_plugin_evidence_graceful_on_empty_rust_scope() {
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

        assert!(result.evidence.is_empty());
        assert!(
            result.findings.is_empty(),
            "empty Rust scope should produce no findings, got {:?}",
            result.findings,
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
            })
        }
    }

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
                    id: supersigil_evidence::EvidenceId(0),
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
                    evidence_kind: supersigil_evidence::EvidenceKind::Tag,
                    provenance: vec![],
                    metadata: BTreeMap::new(),
                }],
                diagnostics: vec![supersigil_evidence::PluginDiagnostic::warning(
                    "warning plugin skipped a file",
                )],
            })
        }
    }

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
    // 5c. resolve_plugin_files includes src/ even when tests globs are set
    // -------------------------------------------------------------------

    #[test]
    fn resolve_plugin_files_includes_src_when_test_globs_configured() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("tests/integration_test.rs"),
            "#[test] fn ok() {}",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "#[cfg(test)] mod tests { #[test] fn unit() {} }",
        )
        .unwrap();

        let mut config = base_config();
        config.ecosystem.plugins = vec!["rust".into()];
        config.tests = Some(vec!["tests/**/*.rs".into()]);

        let test_files = supersigil_verify::resolve_test_files(&config, dir.path());
        let files = resolve_plugin_files(&test_files, &config, dir.path());

        assert!(
            files
                .iter()
                .any(|p| p.ends_with("tests/integration_test.rs")),
            "should include explicit test files, got {files:?}",
        );
        assert!(
            files.iter().any(|p| p.ends_with("src/lib.rs")),
            "should also include src/**/*.rs for Rust plugin unit test discovery, got {files:?}",
        );
    }

    // -------------------------------------------------------------------
    // 6. resolve_plugin_files returns empty when no explicit or inferred files
    // -------------------------------------------------------------------

    #[test]
    fn resolve_plugin_files_empty_when_no_explicit_or_inferred_matches() {
        let mut config = base_config();
        config.ecosystem.plugins = vec!["rust".into()];
        let test_files = supersigil_verify::resolve_test_files(&config, Path::new("/tmp"));
        let files = resolve_plugin_files(&test_files, &config, Path::new("/tmp"));

        assert!(
            files.is_empty(),
            "expected empty file list when discovery finds no Rust files, got {} files",
            files.len(),
        );
    }

    // -------------------------------------------------------------------
    // 7. End-to-end: assembly -> discovery -> artifact graph
    // -------------------------------------------------------------------

    fn pos(line: usize) -> supersigil_core::SourcePosition {
        supersigil_core::SourcePosition {
            byte_offset: line * 40,
            line,
            column: 1,
        }
    }

    fn make_requirement_doc() -> supersigil_core::SpecDocument {
        supersigil_core::SpecDocument {
            path: PathBuf::from("specs/req/auth.mdx"),
            frontmatter: supersigil_core::Frontmatter {
                id: "req/auth".into(),
                doc_type: None,
                status: None,
            },
            extra: HashMap::new(),
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
                        position: pos(11),
                    }],
                    body_text: Some("criterion crit-1".into()),
                    position: pos(10),
                }],
                body_text: None,
                position: pos(9),
            }],
        }
    }

    fn make_property_doc() -> supersigil_core::SpecDocument {
        supersigil_core::SpecDocument {
            path: PathBuf::from("specs/prop/auth.mdx"),
            frontmatter: supersigil_core::Frontmatter {
                id: "prop/auth".into(),
                doc_type: None,
                status: None,
            },
            extra: HashMap::new(),
            components: vec![supersigil_core::ExtractedComponent {
                name: "References".into(),
                attributes: HashMap::from([("refs".into(), "req/auth#crit-1".into())]),
                children: vec![],
                body_text: None,
                position: pos(5),
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

    #[test]
    fn end_to_end_plugin_assembly_and_artifact_graph() {
        use supersigil_verify::artifact_graph::build_artifact_graph;
        use supersigil_verify::explicit_evidence::extract_explicit_evidence;

        let (dir, config, graph) = setup_e2e_fixture();

        // Assemble plugins and discover evidence
        let plugins = assemble_plugins(&config);
        assert_eq!(plugins.len(), 1);

        let test_files = supersigil_verify::resolve_test_files(&config, dir.path());
        let source_files = resolve_plugin_files(&test_files, &config, dir.path());
        assert!(!source_files.is_empty(), "should resolve test files");

        let scope = ProjectScope {
            project: None,
            project_root: dir.path().to_path_buf(),
        };
        let plugin_result = collect_plugin_evidence(&plugins, &source_files, &scope, &graph);
        assert!(
            !plugin_result.evidence.is_empty(),
            "Rust plugin should find evidence"
        );
        assert!(
            plugin_result.findings.is_empty(),
            "expected no plugin failures"
        );

        let explicit_evidence = extract_explicit_evidence(&graph, &test_files, dir.path());
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

        let crit = supersigil_evidence::VerifiableRef {
            doc_id: "req/auth".into(),
            target_id: "crit-1".into(),
        };
        assert!(
            ag.evidence_by_target.contains_key(&crit),
            "criterion req/auth#crit-1 should be indexed",
        );
        assert!(ag.conflicts.is_empty(), "expected no conflicts");
    }

    // -------------------------------------------------------------------
    // 8. Workspace member crate traversal
    // -------------------------------------------------------------------

    #[test]
    fn infer_rust_source_files_traverses_workspace_members() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create a workspace Cargo.toml.
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/my-crate\"]\n",
        )
        .unwrap();

        // Create member crate source file.
        std::fs::create_dir_all(dir.path().join("crates/my-crate/src")).unwrap();
        std::fs::write(
            dir.path().join("crates/my-crate/src/lib.rs"),
            "pub fn hello() {}",
        )
        .unwrap();

        // Create member crate test file.
        std::fs::create_dir_all(dir.path().join("crates/my-crate/tests")).unwrap();
        std::fs::write(
            dir.path().join("crates/my-crate/tests/integration.rs"),
            "#[test] fn ok() {}",
        )
        .unwrap();

        let files = infer_rust_source_files(dir.path());

        assert!(
            files
                .iter()
                .any(|p| p.ends_with("crates/my-crate/src/lib.rs")),
            "should include workspace member src files, got {files:?}",
        );
        assert!(
            files
                .iter()
                .any(|p| p.ends_with("crates/my-crate/tests/integration.rs")),
            "should include workspace member test files, got {files:?}",
        );
    }

    #[test]
    fn infer_rust_source_files_skips_fixture_directories() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests/fixtures/fail")).unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();

        std::fs::write(
            dir.path().join("tests/fixtures/fail/bad_case.rs"),
            "#[verifies(\"req/auth\")]\n#[test]\nfn bad_case() {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tests/real_test.rs"),
            "#[test]\nfn real_test() {}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "pub fn helper() {}\n").unwrap();

        let files = infer_rust_source_files(dir.path());

        assert!(
            files.iter().any(|p| p.ends_with("tests/real_test.rs")),
            "real test files should still be inferred, got {files:?}",
        );
        assert!(
            files.iter().any(|p| p.ends_with("src/lib.rs")),
            "src files should still be inferred, got {files:?}",
        );
        assert!(
            files
                .iter()
                .all(|p| !p.ends_with("tests/fixtures/fail/bad_case.rs")),
            "fixture files should be excluded from inferred Rust discovery, got {files:?}",
        );
    }

    #[test]
    fn read_workspace_member_dirs_handles_missing_cargo_toml() {
        let dirs = read_workspace_member_dirs(Path::new("/nonexistent"));
        assert!(dirs.is_empty());
    }

    #[test]
    fn read_workspace_member_dirs_handles_non_workspace() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"single-crate\"\n",
        )
        .unwrap();

        let dirs = read_workspace_member_dirs(dir.path());
        assert!(dirs.is_empty());
    }
}
