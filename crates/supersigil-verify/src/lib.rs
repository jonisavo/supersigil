//! Verification engine for supersigil spec documents.

pub mod affected;
pub(crate) mod artifact_graph;
mod error;
pub(crate) mod examples;
pub(crate) mod explicit_evidence;
pub mod git;
pub(crate) mod hooks;
mod plugins;
mod report;
mod rule_name;
mod rules;
mod scan;
mod severity;
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use supersigil_core::{Config, DocumentGraph, SpecDocument};

pub use affected::AffectedDocument;
pub use artifact_graph::{ArtifactGraph, build_artifact_graph};
pub use error::VerifyError;
pub use examples::executor::{
    ExampleProgressObserver, collect_examples, execute_examples, execute_examples_with_progress,
    results_to_evidence, results_to_findings,
};
pub use examples::types::{
    BodySpan, ExampleOutcome, ExampleResult, ExampleSpec, ExpectedSpec, MatchCheck, MatchFailure,
    MatchFormat,
};
pub use explicit_evidence::extract_explicit_evidence;
pub use hooks::run_hooks;
pub use plugins::{
    PluginEvidenceResult, assemble_plugins, build_evidence, collect_plugin_evidence,
};
pub use report::{
    EvidenceReportEntry, EvidenceSummary, Finding, FindingDetails, ReportSeverity, ResultStatus,
    RuleName, Summary, TargetCoverage, VerificationReport, format_json, format_markdown,
};
pub use scan::{TagMatch, scan_all_tags, scan_for_tag};
pub use severity::resolve_severity;

/// Why example execution did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExampleSkipReason {
    /// Structural errors in phase 1 gated example execution.
    StructuralErrors,
    /// The user explicitly requested `--skip-examples`.
    ExplicitSkip,
}

/// Options for the verify pipeline.
#[derive(Debug, Default)]
pub struct VerifyOptions {
    /// Filter findings to a specific project (multi-project mode).
    pub project: Option<String>,
    /// Git ref for staleness checks (e.g. `--since main`).
    pub since_ref: Option<String>,
    /// Only consider committed changes (not staged/unstaged).
    pub committed_only: bool,
    /// Use merge-base for git diff.
    pub use_merge_base: bool,
}

/// Pre-resolved inputs for structural verification.
///
/// Computing these is expensive (glob expansion + file I/O). Callers that
/// need both `verify_structural` and `extract_explicit_evidence` should
/// resolve once and pass the same inputs to both.
#[derive(Debug)]
pub struct VerifyInputs {
    /// Resolved test file paths from config glob expansion.
    pub test_files: Vec<PathBuf>,
    /// Tag matches from scanning all test files.
    pub tag_matches: Vec<TagMatch>,
}

impl VerifyInputs {
    /// Resolve test files and scan for tags.
    #[must_use]
    pub fn resolve(config: &Config, project_root: &Path) -> Self {
        let test_files = resolve_test_files(config, project_root);
        let tag_matches = scan::scan_all_tags(&test_files);
        Self {
            test_files,
            tag_matches,
        }
    }
}

/// Collect document IDs in scope for verification, optionally filtered by
/// `VerifyOptions::project`.
#[must_use]
pub fn scoped_doc_ids(graph: &DocumentGraph, options: &VerifyOptions) -> Vec<String> {
    if let Some(project) = &options.project {
        graph
            .documents()
            .filter(|(id, _)| graph.doc_project(id) == Some(project.as_str()))
            .map(|(id, _)| id.to_owned())
            .collect()
    } else {
        graph.documents().map(|(id, _)| id.to_owned()).collect()
    }
}

/// Run all structural verification rules (everything except coverage and hooks).
///
/// Returns raw findings without severity resolution or filtering. The caller is
/// responsible for resolving severities, running hooks, and filtering `Off`
/// findings.
///
/// Rules included:
/// - test mapping (`file_globs`, `tags`)
/// - tracked files (`empty_globs`, `staleness`)
/// - structural (`required_components`, `id_pattern`, `isolated`, `orphan_tags`,
///   `verified_by_placement`)
/// - status
///
/// # Errors
///
/// Returns [`VerifyError`] if an underlying I/O or git operation fails.
pub fn verify_structural(
    graph: &DocumentGraph,
    config: &Config,
    project_root: &Path,
    options: &VerifyOptions,
    inputs: &VerifyInputs,
) -> Result<(Vec<Finding>, Vec<String>), VerifyError> {
    let doc_ids = scoped_doc_ids(graph, options);
    let docs: Vec<&SpecDocument> = doc_ids.iter().filter_map(|id| graph.document(id)).collect();

    let mut findings = Vec::new();

    // Test mapping
    findings.extend(rules::tests_rule::check_file_globs(&docs, project_root));
    findings.extend(rules::tests_rule::check_tags(&docs, &inputs.tag_matches));

    // Tracked files
    findings.extend(rules::tracked::check_empty_globs(graph, project_root));
    if let Some(ref since) = options.since_ref {
        findings.extend(rules::tracked::check_staleness(
            graph,
            project_root,
            since,
            options.committed_only,
            options.use_merge_base,
        ));
    }

    // Structural
    findings.extend(rules::structural::check_required_components(graph, config));
    findings.extend(rules::structural::check_id_pattern(graph, config));
    findings.extend(rules::structural::check_isolated(graph));
    findings.extend(rules::structural::check_orphan_tags(
        &docs,
        &inputs.tag_matches,
    ));
    let component_defs = graph.component_defs();
    findings.extend(rules::structural::check_verified_by_placement(
        &docs,
        component_defs,
    ));
    findings.extend(rules::structural::check_expected_placement(&docs));
    findings.extend(rules::structural::check_rationale_placement(&docs));
    findings.extend(rules::structural::check_alternative_placement(&docs));
    findings.extend(rules::structural::check_duplicate_rationale(&docs));
    findings.extend(rules::structural::check_alternative_status(&docs));
    findings.extend(rules::structural::check_code_block_cardinality(&docs));
    findings.extend(rules::structural::check_expected_cardinality(&docs));
    findings.extend(rules::structural::check_inline_example_lang(&docs));
    findings.extend(rules::structural::check_code_ref_conflicts(&docs));
    findings.extend(rules::structural::check_env_format(&docs));
    let (sequential_id_order, sequential_id_gap) = rules::structural::check_sequential_ids(&docs);
    findings.extend(sequential_id_order);
    findings.extend(sequential_id_gap);

    // Decision
    findings.extend(rules::decision::check_incomplete(&docs));
    findings.extend(rules::decision::check_orphan(&docs, graph));
    findings.extend(rules::decision::check_coverage(&docs, graph));

    // Status
    findings.extend(rules::status::check(graph));

    scope_and_annotate(&mut findings, graph, &doc_ids, options.project.is_some());

    Ok((findings, doc_ids))
}

/// Run only the coverage verification rule.
///
/// Returns raw findings without severity resolution or filtering.
#[must_use]
pub fn verify_coverage(graph: &DocumentGraph, artifact_graph: &ArtifactGraph<'_>) -> Vec<Finding> {
    rules::coverage::check(graph, artifact_graph)
}

/// Run the full verification pipeline.
///
/// Collects findings from all built-in rules, resolves severities, filters
/// out `Off` findings, and builds a summary report.
///
/// Evidence-aware rules (such as `missing_verification_evidence`) consult the
/// `artifact_graph` to check for merged evidence before emitting findings.
/// Pass [`ArtifactGraph::empty`] when no evidence sources are available.
///
/// # Errors
///
/// Returns [`VerifyError`] if an underlying I/O or git operation fails
/// fatally (most git errors are handled gracefully within individual rules).
pub fn verify(
    graph: &DocumentGraph,
    config: &Config,
    project_root: &Path,
    options: &VerifyOptions,
    artifact_graph: &ArtifactGraph<'_>,
) -> Result<VerificationReport, VerifyError> {
    let inputs = VerifyInputs::resolve(config, project_root);

    // Run structural rules and coverage rules
    let (mut findings, doc_ids) = verify_structural(graph, config, project_root, options, &inputs)?;
    let mut coverage_findings = verify_coverage(graph, artifact_graph);

    scope_and_annotate(
        &mut coverage_findings,
        graph,
        &doc_ids,
        options.project.is_some(),
    );

    findings.extend(coverage_findings);

    // Resolve severities
    resolve_finding_severities(&mut findings, graph, config);
    if let Some(finding) = empty_project_finding(config, doc_ids.len()) {
        findings.push(finding);
    }
    Ok(finalize_report(
        config,
        doc_ids.len(),
        findings,
        Some(artifact_graph),
    ))
}

/// Build the final verification report after callers have assembled findings.
///
/// This centralizes shared report policy: post-verify hooks run against the
/// interim report, `Off` findings are filtered, summary counts are rebuilt, and
/// evidence summary metadata is attached when artifact evidence exists.
#[must_use]
pub fn finalize_report(
    config: &Config,
    doc_count: usize,
    mut findings: Vec<Finding>,
    artifact_graph: Option<&ArtifactGraph<'_>>,
) -> VerificationReport {
    if !config.hooks.post_verify.is_empty() {
        let interim = VerificationReport::new(
            findings.clone(),
            Summary::from_findings(doc_count, &findings),
            None,
        );
        let interim_json = serde_json::to_string(&interim).unwrap_or_default();
        findings.extend(hooks::run_hooks(
            &config.hooks.post_verify,
            &interim_json,
            config.hooks.timeout_seconds,
        ));
    }

    findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    let summary = Summary::from_findings(doc_count, &findings);
    let evidence_summary = artifact_graph
        .filter(|graph| !graph.evidence.is_empty())
        .map(EvidenceSummary::from_artifact_graph);

    VerificationReport::new(findings, summary, evidence_summary)
}

/// Convert artifact-graph conflicts into global verification findings.
#[must_use]
pub fn artifact_conflict_findings(artifact_graph: &ArtifactGraph<'_>) -> Vec<Finding> {
    artifact_graph
        .conflicts
        .iter()
        .map(|conflict| {
            let test_name = format!("{}::{}", conflict.test.file.display(), conflict.test.name);
            let left: Vec<String> = conflict.left.iter().map(ToString::to_string).collect();
            let right: Vec<String> = conflict.right.iter().map(ToString::to_string).collect();
            let message = format!(
                "evidence conflict for test `{test_name}`: \
                 criterion sets disagree — [{}] vs [{}]",
                left.join(", "),
                right.join(", "),
            );
            Finding::new(RuleName::PluginDiscoveryFailure, None, message, None)
        })
        .collect()
}

/// Finalize example findings by attaching any skip note and resolving
/// severities for real execution failures.
#[must_use]
pub fn finalize_example_findings(
    mut findings: Vec<Finding>,
    skip_reason: Option<ExampleSkipReason>,
    graph: &DocumentGraph,
    config: &Config,
) -> Vec<Finding> {
    if let Some(skip_reason) = skip_reason {
        let message = match skip_reason {
            ExampleSkipReason::StructuralErrors => {
                "example execution skipped due to structural errors in phase 1"
            }
            ExampleSkipReason::ExplicitSkip => "example execution skipped via --skip-examples",
        };

        let mut skip_finding =
            Finding::new(RuleName::ExampleFailed, None, message.to_string(), None);
        skip_finding.effective_severity = ReportSeverity::Info;
        skip_finding.raw_severity = ReportSeverity::Info;
        findings.push(skip_finding);
    }

    for finding in &mut findings {
        if finding.raw_severity == ReportSeverity::Info {
            continue;
        }

        let doc_status = finding
            .doc_id
            .as_ref()
            .and_then(|id| graph.document(id))
            .and_then(|doc| doc.frontmatter.status.as_deref());
        finding.effective_severity = resolve_severity(&finding.rule, doc_status, &config.verify);
    }

    findings
}

/// Build the empty-project warning finding when no documents are in scope.
#[must_use]
pub fn empty_project_finding(config: &Config, doc_count: usize) -> Option<Finding> {
    if doc_count != 0 {
        return None;
    }

    let mut finding = Finding::new(
        RuleName::EmptyProject,
        None,
        "no documents found — run `supersigil new requirements <name>` to create one, or check that existing files have valid `supersigil:` frontmatter".to_string(),
        None,
    );
    finding.effective_severity = resolve_severity(&finding.rule, None, &config.verify);
    Some(finding)
}

/// Resolve effective severities for a batch of findings.
pub fn resolve_finding_severities(
    findings: &mut [Finding],
    graph: &DocumentGraph,
    config: &Config,
) {
    for finding in findings {
        let doc_status = finding
            .doc_id
            .as_ref()
            .and_then(|id| graph.document(id))
            .and_then(|doc| doc.frontmatter.status.as_deref());
        finding.effective_severity = resolve_severity(&finding.rule, doc_status, &config.verify);
    }
}

/// Filter findings to project scope (if enabled) and attach owning spec paths.
fn scope_and_annotate(
    findings: &mut Vec<Finding>,
    graph: &DocumentGraph,
    doc_ids: &[String],
    project_filter: bool,
) {
    if project_filter {
        filter_findings_to_doc_ids(findings, doc_ids);
    }
    attach_doc_paths(findings, graph);
}

/// Filter a finding list to a selected document scope while preserving global
/// findings whose `doc_id` is absent.
pub fn filter_findings_to_doc_ids(findings: &mut Vec<Finding>, doc_ids: &[String]) {
    let ids: HashSet<&str> = doc_ids.iter().map(String::as_str).collect();
    findings.retain(|finding| {
        finding
            .doc_id
            .as_ref()
            .is_none_or(|id| ids.contains(id.as_str()))
    });
}

fn attach_doc_paths(findings: &mut [Finding], graph: &DocumentGraph) {
    for finding in findings {
        let Some(doc_id) = finding.doc_id.as_deref() else {
            continue;
        };
        let Some(doc) = graph.document(doc_id) else {
            continue;
        };

        let details = finding
            .details
            .get_or_insert_with(|| Box::new(FindingDetails::default()));
        if details.path.is_none() {
            details.path = Some(doc.path.to_string_lossy().into_owned());
        }
    }
}

/// Resolve test file paths by expanding test globs relative to `project_root`.
///
/// In single-project mode, uses `config.tests`. In multi-project mode, uses
/// `config.projects[*].tests` (all projects combined).
pub fn resolve_test_files(config: &Config, project_root: &Path) -> Vec<std::path::PathBuf> {
    let mut globs: Vec<&str> = Vec::new();

    if let Some(ref test_globs) = config.tests {
        globs.extend(test_globs.iter().map(String::as_str));
    }

    if let Some(ref projects) = config.projects {
        for project in projects.values() {
            globs.extend(project.tests.iter().map(String::as_str));
        }
    }

    supersigil_core::expand_globs(globs, project_root)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
#[path = "lib_tests.rs"]
mod verify_tests;
