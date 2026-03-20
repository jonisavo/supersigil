use supersigil_core::{Config, DocumentGraph};
use supersigil_evidence::VerificationEvidenceRecord;
use supersigil_verify::{
    ArtifactGraph, ExampleSkipReason, Finding, ReportSeverity, VerificationReport,
    artifact_conflict_findings, empty_project_finding, filter_findings_to_doc_ids,
    finalize_example_findings, finalize_report, resolve_finding_severities, verify_coverage,
};

use super::example_phase::ExamplePhaseResult;
use super::output::{ExampleExecutionSummary, ExampleProgressDisplay};

pub(super) struct PreparedReport {
    pub(super) report: VerificationReport,
    pub(super) example_summary: Option<ExampleExecutionSummary>,
    pub(super) example_progress_display: Option<ExampleProgressDisplay>,
    pub(super) example_skip_reason: Option<ExampleSkipReason>,
}

pub(super) struct ReportPhaseInput<'a> {
    pub(super) graph: &'a DocumentGraph,
    pub(super) config: &'a Config,
    pub(super) doc_ids: &'a [String],
    pub(super) project_filter: bool,
    pub(super) artifact_graph: ArtifactGraph<'a>,
    pub(super) structural_findings: Vec<Finding>,
    pub(super) plugin_findings: Vec<Finding>,
    pub(super) example_phase: ExamplePhaseResult,
}

pub(super) fn assemble_report(input: ReportPhaseInput<'_>) -> PreparedReport {
    let ReportPhaseInput {
        graph,
        config,
        doc_ids,
        project_filter,
        mut artifact_graph,
        structural_findings,
        mut plugin_findings,
        example_phase,
    } = input;

    let ExamplePhaseResult {
        findings: mut example_findings,
        evidence: example_evidence,
        summary: example_summary,
        progress_display: example_progress_display,
        skip_reason: example_skip_reason,
    } = example_phase;

    if !example_evidence.is_empty() {
        artifact_graph = merge_example_evidence(graph, artifact_graph, example_evidence);
    }

    let mut coverage_findings = verify_coverage(graph, &artifact_graph);
    resolve_finding_severities(&mut coverage_findings, graph, config);

    resolve_finding_severities(&mut plugin_findings, graph, config);
    plugin_findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    let mut conflict_findings = artifact_conflict_findings(&artifact_graph);
    resolve_finding_severities(&mut conflict_findings, graph, config);
    conflict_findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    example_findings =
        finalize_example_findings(example_findings, example_skip_reason, graph, config);

    if project_filter {
        filter_findings_to_doc_ids(&mut coverage_findings, doc_ids);
        filter_findings_to_doc_ids(&mut plugin_findings, doc_ids);
        filter_findings_to_doc_ids(&mut conflict_findings, doc_ids);
        filter_findings_to_doc_ids(&mut example_findings, doc_ids);
    }

    let doc_count = doc_ids.len();
    let mut all_findings = structural_findings;
    all_findings.extend(coverage_findings);
    all_findings.extend(plugin_findings);
    all_findings.extend(conflict_findings);
    all_findings.extend(example_findings);

    if let Some(finding) = empty_project_finding(config, doc_count) {
        all_findings.push(finding);
    }

    PreparedReport {
        report: finalize_report(config, doc_count, all_findings, Some(&artifact_graph)),
        example_summary,
        example_progress_display,
        example_skip_reason,
    }
}

fn merge_example_evidence<'a>(
    graph: &'a DocumentGraph,
    artifact_graph: ArtifactGraph<'a>,
    example_evidence: Vec<VerificationEvidenceRecord>,
) -> ArtifactGraph<'a> {
    let mut all_plugin_evidence: Vec<_> = artifact_graph.evidence.into_iter().collect();
    all_plugin_evidence.extend(example_evidence);
    supersigil_verify::build_artifact_graph(graph, vec![], all_plugin_evidence)
}
