use supersigil_core::{Config, DocumentGraph};
use supersigil_verify::{
    ArtifactGraph, Finding, ReportSeverity, VerificationReport, artifact_conflict_findings,
    empty_project_finding, filter_findings_to_doc_ids, finalize_report, resolve_finding_severities,
    verify_coverage,
};

pub(super) struct ReportPhaseInput<'a> {
    pub(super) graph: &'a DocumentGraph,
    pub(super) config: &'a Config,
    pub(super) doc_ids: &'a [String],
    pub(super) project_filter: bool,
    pub(super) artifact_graph: ArtifactGraph<'a>,
    pub(super) structural_findings: Vec<Finding>,
    pub(super) plugin_findings: Vec<Finding>,
}

pub(super) fn assemble_report(input: ReportPhaseInput<'_>) -> VerificationReport {
    let ReportPhaseInput {
        graph,
        config,
        doc_ids,
        project_filter,
        artifact_graph,
        structural_findings,
        mut plugin_findings,
    } = input;

    let mut coverage_findings = verify_coverage(graph, &artifact_graph);
    resolve_finding_severities(&mut coverage_findings, graph, config);

    resolve_finding_severities(&mut plugin_findings, graph, config);
    plugin_findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    let mut conflict_findings = artifact_conflict_findings(&artifact_graph);
    resolve_finding_severities(&mut conflict_findings, graph, config);
    conflict_findings.retain(|f| f.effective_severity != ReportSeverity::Off);

    if project_filter {
        filter_findings_to_doc_ids(&mut coverage_findings, doc_ids);
        filter_findings_to_doc_ids(&mut plugin_findings, doc_ids);
        filter_findings_to_doc_ids(&mut conflict_findings, doc_ids);
    }

    let doc_count = doc_ids.len();
    let mut all_findings = structural_findings;
    all_findings.extend(coverage_findings);
    all_findings.extend(plugin_findings);
    all_findings.extend(conflict_findings);

    if let Some(finding) = empty_project_finding(config, doc_count) {
        all_findings.push(finding);
    }

    finalize_report(config, doc_count, all_findings, Some(&artifact_graph))
}
