//! Verification engine for supersigil spec documents.

pub mod affected;
pub(crate) mod artifact_graph;
mod error;
pub(crate) mod examples;
pub(crate) mod explicit_evidence;
pub mod git;
pub(crate) mod hooks;
mod report;
mod rules;
mod scan;
mod severity;
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

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
    ExampleOutcome, ExampleResult, ExampleSpec, ExpectedSpec, MatchCheck, MatchFailure, MatchFormat,
};
pub use explicit_evidence::extract_explicit_evidence;
pub use hooks::run_hooks;
pub use report::{
    EvidenceReportEntry, EvidenceSummary, Finding, FindingDetails, ReportSeverity, ResultStatus,
    RuleName, Summary, TargetCoverage, VerificationReport, format_json, format_markdown,
};
pub use scan::{TagMatch, scan_all_tags, scan_for_tag};
pub use severity::resolve_severity;

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

/// Collect document IDs, optionally filtered by project.
fn collect_doc_ids(graph: &DocumentGraph, options: &VerifyOptions) -> Vec<String> {
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
) -> Result<Vec<Finding>, VerifyError> {
    let doc_ids = collect_doc_ids(graph, options);
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

    Ok(findings)
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
    let doc_ids = collect_doc_ids(graph, options);
    let inputs = VerifyInputs::resolve(config, project_root);

    // Run structural rules and coverage rules
    let mut findings = verify_structural(graph, config, project_root, options, &inputs)?;
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
        findings.retain(|f| f.doc_id.as_ref().is_none_or(|id| doc_ids.contains(id)));
    }
    attach_doc_paths(findings, graph);
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

    globs
        .into_iter()
        .flat_map(|pattern| supersigil_core::expand_glob(pattern, project_root))
        .collect()
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

    fn make_artifact_graph_with_evidence(graph: &DocumentGraph) -> ArtifactGraph<'_> {
        build_artifact_graph(
            graph,
            vec![],
            vec![VerificationEvidenceRecord {
                id: EvidenceId::new(0),
                targets: VerificationTargets::single(VerifiableRef {
                    doc_id: "req/auth".into(),
                    target_id: "req-1".into(),
                }),
                test: TestIdentity {
                    file: PathBuf::from("tests/auth_test.rs"),
                    name: "login_succeeds".into(),
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
            }],
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
            Some("specs/req/auth.mdx"),
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
                paths: vec!["specs/**/*.mdx".into()],
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
                    paths: vec!["specs/**/*.mdx".into()],
                    tests: vec!["alpha_tests/**/*.rs".into()],
                    isolated: false,
                },
            ),
            (
                "beta".into(),
                supersigil_core::ProjectConfig {
                    paths: vec!["specs/**/*.mdx".into()],
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
        let findings =
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
