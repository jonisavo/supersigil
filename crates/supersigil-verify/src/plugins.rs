//! Plugin assembly and evidence collection.
//!
//! Creates ecosystem plugin instances from the config and runs the full
//! evidence pipeline: plugin discovery + explicit `<VerifiedBy>` tags merged
//! into a single `ArtifactGraph`.

use std::path::{Path, PathBuf};

use supersigil_core::{Config, DocumentGraph};
use supersigil_evidence::{
    EcosystemPlugin, PluginDiagnostic, PluginDiscoveryResult, PluginError, ProjectScope,
    VerificationEvidenceRecord,
};

use crate::VerifyInputs;
use crate::artifact_graph::{ArtifactGraph, build_artifact_graph};
use crate::explicit_evidence::extract_explicit_evidence;
use crate::report::{Finding, FindingDetails, RuleName};

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
/// `inputs` should be pre-resolved via [`VerifyInputs::resolve`]
/// to avoid redundant glob expansion and tag scanning.
///
/// Returns the artifact graph and any plugin failure findings.
#[must_use]
pub fn build_evidence<'g>(
    config: &Config,
    graph: &'g DocumentGraph,
    project_root: &Path,
    project: Option<&str>,
    inputs: &VerifyInputs,
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
