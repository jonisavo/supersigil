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
mod verify_tests {
    use super::*;
    use std::collections::BTreeMap;

    use supersigil_evidence::{
        EvidenceId, PluginProvenance, SourceLocation, TestIdentity, TestKind, VerifiableRef,
        VerificationEvidenceRecord, VerificationTargets,
    };
    use tempfile::TempDir;
    use test_helpers::*;

    fn make_evidence_record(
        id: usize,
        doc_id: &str,
        target_id: &str,
        test_name: &str,
    ) -> VerificationEvidenceRecord {
        VerificationEvidenceRecord {
            id: EvidenceId::new(id),
            targets: VerificationTargets::single(VerifiableRef {
                doc_id: doc_id.into(),
                target_id: target_id.into(),
            }),
            test: TestIdentity {
                file: PathBuf::from("tests/auth_test.rs"),
                name: test_name.into(),
                kind: TestKind::Unit,
            },
            source_location: SourceLocation {
                file: PathBuf::from("tests/auth_test.rs"),
                line: 3,
                column: 1,
            },
            provenance: vec![PluginProvenance::RustAttribute {
                attribute_span: SourceLocation {
                    file: PathBuf::from("tests/auth_test.rs"),
                    line: 3,
                    column: 1,
                },
            }],
            metadata: BTreeMap::new(),
        }
    }

    fn make_artifact_graph_with_evidence(graph: &DocumentGraph) -> ArtifactGraph<'_> {
        build_artifact_graph(
            graph,
            vec![],
            vec![make_evidence_record(
                0,
                "req/auth",
                "req-1",
                "login_succeeds",
            )],
        )
    }

    #[test]
    fn finalize_report_filters_off_findings() {
        let config = test_config();

        let mut kept = Finding::new(
            RuleName::InvalidIdPattern,
            Some("req/auth".into()),
            "kept".into(),
            None,
        );
        kept.effective_severity = ReportSeverity::Warning;

        let mut filtered = Finding::new(
            RuleName::MissingVerificationEvidence,
            Some("req/auth".into()),
            "filtered".into(),
            None,
        );
        filtered.effective_severity = ReportSeverity::Off;

        let report = finalize_report(&config, 1, vec![kept.clone(), filtered], None);

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].message, kept.message);
        assert_eq!(
            report.findings[0].effective_severity,
            ReportSeverity::Warning,
        );
        assert_eq!(report.summary.total_documents, 1);
        assert_eq!(report.summary.warning_count, 1);
        assert_eq!(report.summary.error_count, 0);
        assert_eq!(report.summary.info_count, 0);
        assert!(report.evidence_summary.is_none());
    }

    #[test]
    fn finalize_report_preserves_evidence_summary() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let artifact_graph = make_artifact_graph_with_evidence(&graph);

        let report = finalize_report(&config, 1, Vec::new(), Some(&artifact_graph));

        assert!(
            report.evidence_summary.is_some(),
            "artifact-backed reports should preserve evidence summary",
        );
        assert_eq!(
            report
                .evidence_summary
                .as_ref()
                .expect("evidence summary")
                .records
                .len(),
            1,
        );
    }

    #[test]
    fn scoped_doc_ids_respects_project_filter() {
        let mut alpha = make_doc("alpha/auth", vec![]);
        alpha.path = PathBuf::from("specs/alpha/auth.md");

        let mut beta = make_doc("beta/billing", vec![]);
        beta.path = PathBuf::from("specs/beta/billing.md");

        let mut config = test_config();
        config.projects = Some(std::collections::HashMap::from([
            (
                "alpha".into(),
                supersigil_core::ProjectConfig {
                    paths: vec!["specs/alpha/**/*.md".into()],
                    tests: vec![],
                    isolated: false,
                },
            ),
            (
                "beta".into(),
                supersigil_core::ProjectConfig {
                    paths: vec!["specs/beta/**/*.md".into()],
                    tests: vec![],
                    isolated: false,
                },
            ),
        ]));

        let graph = build_test_graph_with_config(vec![alpha, beta], &config);
        let options = VerifyOptions {
            project: Some("alpha".into()),
            ..VerifyOptions::default()
        };

        let doc_ids = scoped_doc_ids(&graph, &options);

        assert_eq!(doc_ids, vec!["alpha/auth".to_string()]);
    }

    #[test]
    fn filter_findings_to_doc_ids_keeps_global_findings() {
        let mut findings = vec![
            Finding::new(
                RuleName::InvalidIdPattern,
                Some("alpha/auth".into()),
                "alpha".into(),
                None,
            ),
            Finding::new(
                RuleName::InvalidIdPattern,
                Some("beta/billing".into()),
                "beta".into(),
                None,
            ),
            Finding::new(RuleName::OrphanTestTag, None, "global".into(), None),
        ];

        filter_findings_to_doc_ids(&mut findings, &[String::from("alpha/auth")]);

        assert_eq!(findings.len(), 2);
        assert!(
            findings
                .iter()
                .any(|finding| finding.doc_id.as_deref() == Some("alpha/auth"))
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.doc_id.is_none() && finding.message == "global")
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding.doc_id.as_deref() != Some("beta/billing"))
        );
    }

    #[test]
    fn artifact_conflict_findings_surface_conflicts_as_findings() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10), make_criterion("req-2", 20)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let artifact_graph = build_artifact_graph(
            &graph,
            vec![],
            vec![
                make_evidence_record(0, "req/auth", "req-1", "login_succeeds"),
                make_evidence_record(1, "req/auth", "req-2", "login_succeeds"),
            ],
        );

        let findings = artifact_conflict_findings(&artifact_graph);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::PluginDiscoveryFailure);
        assert!(
            findings[0].message.contains("evidence conflict for test"),
            "conflict finding should explain the conflicting test: {:?}",
            findings[0],
        );
        assert!(
            findings[0].message.contains("req/auth#req-1")
                && findings[0].message.contains("req/auth#req-2"),
            "conflict finding should list both conflicting target sets: {:?}",
            findings[0],
        );
    }

    #[test]
    fn finalize_example_findings_adds_structural_skip_info_finding() {
        let docs = vec![make_doc("req/auth", vec![])];
        let graph = build_test_graph(docs);
        let config = test_config();

        let findings = finalize_example_findings(
            Vec::new(),
            Some(ExampleSkipReason::StructuralErrors),
            &graph,
            &config,
        );

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::ExampleFailed);
        assert_eq!(findings[0].effective_severity, ReportSeverity::Info);
        assert_eq!(findings[0].raw_severity, ReportSeverity::Info);
        assert!(
            findings[0].message.contains("structural errors"),
            "skip finding should explain why examples were skipped: {:?}",
            findings[0],
        );
    }

    #[test]
    fn finalize_example_findings_resolves_severity_for_real_findings() {
        let docs = vec![make_doc_with_status("req/auth", "draft", vec![])];
        let graph = build_test_graph(docs);
        let config = test_config();
        let finding = Finding::new(
            RuleName::ExampleFailed,
            Some("req/auth".into()),
            "example failed".into(),
            None,
        );

        let findings = finalize_example_findings(vec![finding], None, &graph, &config);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].effective_severity, ReportSeverity::Info);
        assert_eq!(findings[0].raw_severity, ReportSeverity::Error);
    }

    #[test]
    fn empty_project_finding_warns_when_no_documents_are_in_scope() {
        let config = test_config();

        let finding = empty_project_finding(&config, 0).expect("empty project finding");

        assert_eq!(finding.rule, RuleName::EmptyProject);
        assert_eq!(finding.effective_severity, ReportSeverity::Warning);
        assert!(finding.message.contains("no documents found"));
    }

    #[test]
    fn verify_empty_graph_emits_empty_project_warning() {
        let graph = build_test_graph(Vec::new());
        let config = test_config();
        let options = VerifyOptions::default();
        let artifact_graph = ArtifactGraph::empty(&graph);

        let report = verify(
            &graph,
            &config,
            Path::new("/tmp"),
            &options,
            &artifact_graph,
        )
        .unwrap();

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.rule == RuleName::EmptyProject),
            "empty graphs should surface the shared empty-project finding: {:?}",
            report.findings,
        );
        assert_eq!(report.result_status(), ResultStatus::WarningsOnly);
    }

    #[test]
    fn verify_with_missing_verification_evidence() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == RuleName::MissingVerificationEvidence)
        );
        assert!(report.summary.error_count > 0);
    }

    #[test]
    fn verify_attaches_owning_doc_path_to_doc_backed_findings() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();

        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == RuleName::MissingVerificationEvidence)
            .expect("missing coverage finding");

        assert_eq!(
            finding
                .details
                .as_ref()
                .and_then(|details| details.path.as_deref()),
            Some("specs/req/auth.md"),
        );
    }

    #[test]
    fn verify_attaches_canonical_target_ref_to_missing_coverage_findings() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();

        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == RuleName::MissingVerificationEvidence)
            .expect("missing coverage finding");

        assert_eq!(
            finding
                .details
                .as_ref()
                .and_then(|details| details.target_ref.as_deref()),
            Some("req/auth#req-1"),
        );
    }

    #[test]
    fn verify_severity_resolution_applies() {
        // A finding from a draft document should get Info severity
        let docs = vec![make_doc_with_status(
            "req/auth",
            "draft",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        // All findings for draft docs should be Info
        for finding in &report.findings {
            if finding.doc_id.as_deref() == Some("req/auth") {
                assert_eq!(
                    finding.effective_severity,
                    ReportSeverity::Info,
                    "draft doc findings should be Info, got {:?} for rule {:?}",
                    finding.effective_severity,
                    finding.rule,
                );
            }
        }
    }

    #[test]
    fn verify_off_severity_filtered_out() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        // Turn off missing_verification_evidence
        config.verify.rules.insert(
            "missing_verification_evidence".into(),
            supersigil_core::Severity::Off,
        );
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        // Should not contain missing_verification_evidence findings
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.rule == RuleName::MissingVerificationEvidence),
            "Off-severity findings should be filtered out",
        );
    }

    #[test]
    fn verify_clean_report() {
        // All criteria covered via contextual VerifiedBy with evidence,
        // and a real test file exists containing the tag.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(
            dir.path().join("tests/auth_test.rs"),
            "// supersigil: req:auth\n",
        )
        .unwrap();

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "req-1",
                    make_verified_by_tag("req:auth", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        config.tests = Some(vec!["tests/**/*.rs".into()]);
        // Build artifact graph from explicit evidence so coverage is satisfied.
        let test_files = resolve_test_files(&config, dir.path());
        let tag_matches = scan::scan_all_tags(&test_files);
        let explicit =
            explicit_evidence::extract_explicit_evidence(&graph, &tag_matches, dir.path());
        let ag = artifact_graph::build_artifact_graph(&graph, explicit, vec![]);
        let options = VerifyOptions::default();
        let report = verify(&graph, &config, dir.path(), &options, &ag).unwrap();
        assert_eq!(
            report.result_status(),
            ResultStatus::Clean,
            "expected clean report but got findings: {:?}",
            report.findings,
        );
    }

    #[test]
    fn verify_multi_project_resolves_project_test_globs() {
        // In multi-project mode, test globs live under projects[*].tests,
        // not config.tests. resolve_test_files should use them.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(
            dir.path().join("tests/auth_test.rs"),
            "// supersigil: req:auth\n",
        )
        .unwrap();

        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "req-1",
                    make_verified_by_tag("req:auth", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        // Multi-project mode: tests are under projects, not top-level
        config.tests = None;
        config.projects = Some(std::collections::HashMap::from([(
            "core".into(),
            supersigil_core::ProjectConfig {
                paths: vec!["specs/**/*.md".into()],
                tests: vec!["tests/**/*.rs".into()],
                isolated: false,
            },
        )]));
        // Build artifact graph from explicit evidence so coverage is satisfied.
        let test_files = resolve_test_files(&config, dir.path());
        let tag_matches = scan::scan_all_tags(&test_files);
        let explicit =
            explicit_evidence::extract_explicit_evidence(&graph, &tag_matches, dir.path());
        let ag = artifact_graph::build_artifact_graph(&graph, explicit, vec![]);
        let options = VerifyOptions::default();
        let report = verify(&graph, &config, dir.path(), &options, &ag).unwrap();
        // Should NOT produce zero_tag_matches because tests/auth_test.rs
        // contains the tag, and the project tests glob should find it.
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.rule == RuleName::ZeroTagMatches),
            "multi-project test globs should be resolved; got findings: {:?}",
            report.findings,
        );
    }

    #[test]
    fn verify_multi_project_collects_test_files_from_all_projects() {
        // Two projects with different test glob patterns. Verification should
        // discover test files from BOTH projects, not just one.
        let dir = TempDir::new().unwrap();

        // Project "alpha" tests live under alpha_tests/
        std::fs::create_dir_all(dir.path().join("alpha_tests")).unwrap();
        std::fs::write(
            dir.path().join("alpha_tests/login_test.rs"),
            "// supersigil: req:auth\n",
        )
        .unwrap();

        // Project "beta" tests live under beta_tests/
        std::fs::create_dir_all(dir.path().join("beta_tests")).unwrap();
        std::fs::write(
            dir.path().join("beta_tests/payment_test.rs"),
            "// supersigil: req:pay\n",
        )
        .unwrap();

        let docs = vec![
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion_with_verified_by(
                        "req-1",
                        make_verified_by_tag("req:auth", 11),
                        10,
                    )],
                    9,
                )],
            ),
            make_doc(
                "req/pay",
                vec![make_acceptance_criteria(
                    vec![make_criterion_with_verified_by(
                        "pay-1",
                        make_verified_by_tag("req:pay", 21),
                        20,
                    )],
                    19,
                )],
            ),
        ];
        let mut config = test_config();
        config.paths = None;
        config.tests = None;
        config.projects = Some(std::collections::HashMap::from([
            (
                "alpha".into(),
                supersigil_core::ProjectConfig {
                    paths: vec!["specs/**/*.md".into()],
                    tests: vec!["alpha_tests/**/*.rs".into()],
                    isolated: false,
                },
            ),
            (
                "beta".into(),
                supersigil_core::ProjectConfig {
                    paths: vec!["specs/**/*.md".into()],
                    tests: vec!["beta_tests/**/*.rs".into()],
                    isolated: false,
                },
            ),
        ]));

        let graph = build_test_graph_with_config(docs, &config);

        // Verify that resolve_test_files finds files from BOTH projects
        let test_files = resolve_test_files(&config, dir.path());
        assert!(
            test_files
                .iter()
                .any(|p| p.to_string_lossy().contains("alpha_tests")),
            "should discover test files from project alpha, got: {test_files:?}",
        );
        assert!(
            test_files
                .iter()
                .any(|p| p.to_string_lossy().contains("beta_tests")),
            "should discover test files from project beta, got: {test_files:?}",
        );

        // Build artifact graph and run full verify
        let tag_matches = scan::scan_all_tags(&test_files);
        let explicit =
            explicit_evidence::extract_explicit_evidence(&graph, &tag_matches, dir.path());
        let ag = artifact_graph::build_artifact_graph(&graph, explicit, vec![]);
        let options = VerifyOptions::default();
        let report = verify(&graph, &config, dir.path(), &options, &ag).unwrap();

        // Neither document should have zero_tag_matches since both tags are found
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.rule == RuleName::ZeroTagMatches),
            "multi-project with two projects should resolve tags from both; got findings: {:?}",
            report.findings,
        );
    }

    #[test]
    fn verify_summary_counts_are_correct() {
        // Create a scenario with multiple finding types:
        // two uncovered criteria produce error-level findings.
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10), make_criterion("req-2", 20)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        assert_eq!(report.summary.total_documents, 1);
        // Error-level findings should be counted
        assert!(report.summary.error_count > 0);
        assert_eq!(
            report.summary.error_count + report.summary.warning_count + report.summary.info_count,
            report.findings.len(),
        );
    }

    #[test]
    fn verify_structural_excludes_coverage() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let inputs = VerifyInputs::resolve(&config, Path::new("/tmp"));
        let (findings, _doc_ids) =
            verify_structural(&graph, &config, Path::new("/tmp"), &options, &inputs).unwrap();
        assert!(
            !findings
                .iter()
                .any(|f| f.rule == RuleName::MissingVerificationEvidence)
        );
    }

    #[test]
    fn verify_coverage_returns_coverage_findings() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ag = ArtifactGraph::empty(&graph);
        let findings = verify_coverage(&graph, &ag);
        assert!(
            findings
                .iter()
                .any(|f| f.rule == RuleName::MissingVerificationEvidence)
        );
    }

    #[test]
    fn verify_sequential_id_order_finding_in_full_pipeline() {
        let docs = vec![make_doc(
            "feature/tasks",
            vec![make_task("task-2", 10), make_task("task-1", 20)],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == RuleName::SequentialIdOrder),
            "full pipeline should include SequentialIdOrder findings, got: {:?}",
            report.findings,
        );
    }

    #[test]
    fn verify_sequential_id_gap_finding_in_full_pipeline() {
        let docs = vec![make_doc(
            "feature/tasks",
            vec![make_task("task-1", 10), make_task("task-3", 30)],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == RuleName::SequentialIdGap),
            "full pipeline should include SequentialIdGap findings, got: {:?}",
            report.findings,
        );
    }

    #[test]
    fn verify_sequential_rules_draft_gating() {
        let docs = vec![make_doc_with_status(
            "feature/tasks",
            "draft",
            vec![make_task("task-2", 10), make_task("task-1", 20)],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        for finding in &report.findings {
            if finding.rule == RuleName::SequentialIdOrder {
                assert_eq!(
                    finding.effective_severity,
                    ReportSeverity::Info,
                    "draft doc sequential findings should be Info",
                );
            }
        }
    }

    #[test]
    fn verify_rationale_placement_in_full_pipeline() {
        let docs = vec![make_doc("adr/logging", vec![make_rationale(5)])];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == RuleName::InvalidRationalePlacement),
            "full pipeline should include InvalidRationalePlacement findings, got: {:?}",
            report.findings,
        );
    }

    #[test]
    fn verify_alternative_placement_in_full_pipeline() {
        let docs = vec![make_doc("adr/logging", vec![make_alternative("alt-1", 5)])];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == RuleName::InvalidAlternativePlacement),
            "full pipeline should include InvalidAlternativePlacement findings, got: {:?}",
            report.findings,
        );
    }

    #[test]
    fn verify_placement_rules_draft_gating() {
        let docs = vec![make_doc_with_status(
            "adr/logging",
            "draft",
            vec![make_rationale(5), make_alternative("alt-1", 6)],
        )];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        for finding in &report.findings {
            if finding.rule == RuleName::InvalidRationalePlacement
                || finding.rule == RuleName::InvalidAlternativePlacement
            {
                assert_eq!(
                    finding.effective_severity,
                    ReportSeverity::Info,
                    "draft doc placement findings should be Info, got {:?} for {:?}",
                    finding.effective_severity,
                    finding.rule,
                );
            }
        }
    }

    #[test]
    fn verify_sequential_rules_severity_override() {
        let docs = vec![make_doc(
            "feature/tasks",
            vec![make_task("task-2", 10), make_task("task-1", 20)],
        )];
        let graph = build_test_graph(docs);
        let mut config = test_config();
        config
            .verify
            .rules
            .insert("sequential_id_order".into(), supersigil_core::Severity::Off);
        let options = VerifyOptions::default();
        let ag = ArtifactGraph::empty(&graph);
        let report = verify(&graph, &config, Path::new("/tmp"), &options, &ag).unwrap();
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.rule == RuleName::SequentialIdOrder),
            "Off-severity sequential findings should be filtered out",
        );
    }
}
