//! Verification engine for supersigil spec documents.

pub mod affected;
pub mod artifact_graph;
mod error;
pub mod explicit_evidence;
pub mod git;
pub mod hooks;
mod report;
mod rules;
mod scan;
mod severity;
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

use std::path::Path;

use supersigil_core::{Config, DocumentGraph, SpecDocument};

use artifact_graph::ArtifactGraph;

pub use affected::AffectedDocument;
pub use error::VerifyError;
pub use report::{
    EvidenceReportEntry, EvidenceSummary, Finding, ReportSeverity, ResultStatus, RuleName, Summary,
    TargetCoverage, VerificationReport, format_json, format_markdown,
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
    // 1. Collect doc IDs (filter by project if specified)
    let doc_ids: Vec<String> = if let Some(project) = &options.project {
        graph
            .documents()
            .filter(|(id, _)| graph.doc_project(id) == Some(project.as_str()))
            .map(|(id, _)| id.to_owned())
            .collect()
    } else {
        graph.documents().map(|(id, _)| id.to_owned()).collect()
    };

    // Collect SpecDocument refs for rules that need document details
    let docs: Vec<&SpecDocument> = doc_ids.iter().filter_map(|id| graph.document(id)).collect();

    // 2. Resolve test file paths from config
    let test_files = resolve_test_files(config, project_root);

    // 3. Run all rules
    let mut findings = Vec::new();

    // Coverage
    findings.extend(rules::coverage::check(graph, artifact_graph));

    // Test mapping
    findings.extend(rules::tests_rule::check_file_globs(&docs, project_root));
    findings.extend(rules::tests_rule::check_tags(&docs, &test_files));

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
    findings.extend(rules::structural::check_orphan_tags(&docs, &test_files));
    let component_defs = match supersigil_core::ComponentDefs::merge(
        supersigil_core::ComponentDefs::defaults(),
        config.components.clone(),
    ) {
        Ok(defs) => defs,
        Err(errors) => {
            for err in &errors {
                findings.push(Finding::new(
                    RuleName::InvalidVerifiedByPlacement,
                    None,
                    format!("component definition error: {err}"),
                    None,
                ));
            }
            supersigil_core::ComponentDefs::defaults()
        }
    };
    findings.extend(rules::structural::check_verified_by_placement(
        &docs,
        &component_defs,
    ));

    // Status
    findings.extend(rules::status::check(graph));

    // 4. Filter to project scope
    if options.project.is_some() {
        findings.retain(|f| f.doc_id.as_ref().is_none_or(|id| doc_ids.contains(id)));
    }

    // 5. Resolve severities
    for finding in &mut findings {
        let doc_status = finding
            .doc_id
            .as_ref()
            .and_then(|id| graph.document(id))
            .and_then(|doc| doc.frontmatter.status.as_deref());
        finding.effective_severity = resolve_severity(&finding.rule, doc_status, &config.verify);
    }

    // 6. Run post-verify hooks (if any)
    if !config.hooks.post_verify.is_empty() {
        let interim = VerificationReport {
            findings: findings.clone(),
            summary: Summary::from_findings(doc_ids.len(), &findings),
            evidence_summary: None,
        };
        let interim_json = serde_json::to_string(&interim).unwrap_or_default();
        findings.extend(hooks::run_hooks(
            &config.hooks.post_verify,
            &interim_json,
            config.hooks.timeout_seconds,
        ));
    }

    // 7. Filter out Off findings
    findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    // 8. Build summary
    let summary = Summary::from_findings(doc_ids.len(), &findings);

    Ok(VerificationReport {
        findings,
        summary,
        evidence_summary: None,
    })
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
        .flat_map(|pattern| expand_glob(pattern, project_root))
        .collect()
}

/// Expand a glob pattern relative to `base_dir`, returning matched file paths.
pub(crate) fn expand_glob(pattern: &str, base_dir: &Path) -> Vec<std::path::PathBuf> {
    let full = base_dir.join(pattern).to_string_lossy().to_string();
    let mut files = Vec::new();
    if let Ok(entries) = glob::glob(&full) {
        for entry in entries.flatten() {
            files.push(entry);
        }
    }
    files
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod verify_tests {
    use super::*;
    use tempfile::TempDir;
    use test_helpers::*;

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
        let explicit =
            explicit_evidence::extract_explicit_evidence(&graph, &test_files, dir.path());
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
        let explicit =
            explicit_evidence::extract_explicit_evidence(&graph, &test_files, dir.path());
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
}
