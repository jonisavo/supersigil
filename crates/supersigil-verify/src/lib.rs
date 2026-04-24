//! Verification engine for supersigil spec documents.

/// Affected-document detection based on git diffs and tracked-file globs.
pub mod affected;
pub(crate) mod artifact_graph;
/// Per-document component trees with verification status.
pub mod document_components;
mod error;
pub(crate) mod explicit_evidence;
/// Explorer runtime payload builders for lazy explorer clients.
pub mod explorer_runtime;
/// Git diff helpers for affected-document detection.
pub mod git;
mod glob_resolver;
/// JSON serialization of the document graph.
pub mod graph_json;
/// Language plugin discovery and evidence collection.
pub mod plugins;
mod report;
mod rules;
mod scan;
mod severity;
/// Edit-distance suggestion engine for broken references.
pub mod suggest;
#[cfg(any(test, feature = "test-helpers"))]
/// Test helpers for building spec documents and git repos.
pub mod test_helpers;

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use supersigil_core::{Config, DocumentGraph, SpecDocument, TestDiscoveryIgnoreMode};

pub use affected::AffectedDocument;
pub use artifact_graph::{ArtifactGraph, build_artifact_graph};
pub use error::VerifyError;
pub use explicit_evidence::extract_explicit_evidence;
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
    /// Git ref for affected-document checks (e.g. `--since main`).
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
        Self::resolve_for_project(config, project_root, None)
    }

    /// Resolve test files and scan for tags for one selected project.
    #[must_use]
    pub fn resolve_for_project(
        config: &Config,
        project_root: &Path,
        project: Option<&str>,
    ) -> Self {
        let test_files = resolve_test_files_for_project(config, project_root, project);
        let tag_matches = scan::scan_all_tags(&test_files);
        Self {
            test_files,
            tag_matches,
        }
    }
}

/// Combined result for the verification phase that builds evidence and runs
/// structural checks while sharing run-scoped caches.
#[derive(Debug)]
pub struct VerifyPhaseResult<'g> {
    /// Artifact graph built from explicit and plugin evidence.
    pub artifact_graph: ArtifactGraph<'g>,
    /// Non-fatal plugin diagnostics and failures surfaced as findings.
    pub plugin_findings: Vec<Finding>,
    /// Structural findings produced before severity resolution.
    pub structural_findings: Vec<Finding>,
    /// Document IDs included in the selected verification scope.
    pub doc_ids: Vec<String>,
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

/// Run all structural verification rules (everything except coverage).
///
/// Returns raw findings without severity resolution or filtering. The caller is
/// responsible for resolving severities and filtering `Off` findings.
///
/// Rules included:
/// - test mapping (`file_globs`, `tags`)
/// - tracked files (`empty_globs`)
/// - structural (`id_pattern`, `isolated`, `orphan_tags`, `verified_by_placement`)
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
    let mut glob_resolver = glob_resolver::GlobResolver::new(project_root);
    Ok(verify_structural_with_resolver(
        graph,
        config,
        options,
        inputs,
        &mut glob_resolver,
    ))
}

fn verify_structural_with_resolver(
    graph: &DocumentGraph,
    config: &Config,
    options: &VerifyOptions,
    inputs: &VerifyInputs,
    glob_resolver: &mut glob_resolver::GlobResolver,
) -> (Vec<Finding>, Vec<String>) {
    let doc_ids = scoped_doc_ids(graph, options);
    let docs: Vec<&SpecDocument> = doc_ids.iter().filter_map(|id| graph.document(id)).collect();

    let mut findings = Vec::new();

    // Test mapping
    findings.extend(rules::tests_rule::check_file_globs_with_resolver(
        &docs,
        glob_resolver,
    ));
    findings.extend(rules::tests_rule::check_tags(&docs, &inputs.tag_matches));

    // Tracked files
    findings.extend(rules::tracked::check_empty_globs_with_resolver(
        graph,
        Some(&doc_ids),
        glob_resolver,
    ));

    // Structural
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
    findings.extend(rules::structural::check_rationale_placement(&docs));
    findings.extend(rules::structural::check_alternative_placement(&docs));
    findings.extend(rules::structural::check_duplicate_rationale(&docs));
    findings.extend(rules::structural::check_alternative_status(&docs));
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

    (findings, doc_ids)
}

/// Build evidence and run structural checks in one shared phase.
///
/// This preserves the existing public wrappers while allowing callers that run
/// both steps back-to-back to reuse a single run-scoped glob cache.
///
/// # Errors
///
/// Returns [`VerifyError`] if structural verification fails.
pub fn build_evidence_and_verify_structural<'g>(
    graph: &'g DocumentGraph,
    config: &Config,
    project_root: &Path,
    options: &VerifyOptions,
    inputs: &VerifyInputs,
) -> Result<VerifyPhaseResult<'g>, VerifyError> {
    let mut glob_resolver = glob_resolver::GlobResolver::new(project_root);
    let (artifact_graph, plugin_findings) = plugins::build_evidence_with_resolver(
        config,
        graph,
        project_root,
        options.project.as_deref(),
        inputs,
        &mut glob_resolver,
    );
    let (structural_findings, doc_ids) =
        verify_structural_with_resolver(graph, config, options, inputs, &mut glob_resolver);

    Ok(VerifyPhaseResult {
        artifact_graph,
        plugin_findings,
        structural_findings,
        doc_ids,
    })
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
    let inputs =
        VerifyInputs::resolve_for_project(config, project_root, options.project.as_deref());

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
        doc_ids.len(),
        findings,
        Some(artifact_graph),
    ))
}

/// Build the final verification report after callers have assembled findings.
///
/// This centralizes shared report policy: `Off` findings are filtered, summary
/// counts are rebuilt, and evidence summary metadata is attached when artifact
/// evidence exists.
#[must_use]
pub fn finalize_report(
    doc_count: usize,
    mut findings: Vec<Finding>,
    artifact_graph: Option<&ArtifactGraph<'_>>,
) -> VerificationReport {
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

/// Convert [`GraphError`]s into [`Finding`]s, enriching broken-ref errors
/// with "did you mean?" suggestions based on edit distance.
///
/// `known_doc_ids` provides the candidate set for suggestion matching.
/// Non-`BrokenRef` errors are converted to findings without suggestions.
#[must_use]
pub fn graph_error_findings(
    errors: &[supersigil_core::GraphError],
    known_doc_ids: &[&str],
) -> Vec<Finding> {
    errors
        .iter()
        .map(|error| graph_error_to_finding(error, known_doc_ids))
        .collect()
}

fn graph_error_to_finding(error: &supersigil_core::GraphError, known_doc_ids: &[&str]) -> Finding {
    let mut finding = match error {
        supersigil_core::GraphError::BrokenRef {
            doc_id,
            ref_str,
            reason,
            position,
        } => {
            let message = format!(
                "{doc_id}:{}:{}: broken ref `{ref_str}`: {reason}",
                position.line, position.column
            );

            // Only suggest when a document reference was not found —
            // not for fragment-missing, task-dependency, cross-project, or
            // component-type errors.
            let is_doc_not_found = reason.starts_with("document ") && reason.contains("not found");
            let suggestion = if is_doc_not_found {
                let (target_doc_id, fragment) = match ref_str.find('#') {
                    Some(pos) => (&ref_str[..pos], Some(&ref_str[pos..])),
                    None => (ref_str.as_str(), None),
                };
                suggest::closest_match(target_doc_id, known_doc_ids.iter().copied()).map(
                    |matched| match fragment {
                        Some(frag) => format!("{matched}{frag}"),
                        None => matched.to_owned(),
                    },
                )
            } else {
                None
            };

            let mut f = Finding::new(
                RuleName::BrokenRef,
                Some(doc_id.clone()),
                message,
                Some(*position),
            );
            if let Some(s) = suggestion {
                f = f.with_suggestion(s);
            }
            f
        }
        other => Finding::new(RuleName::BrokenRef, None, other.to_string(), None),
    };

    // All graph errors are fatal — mark as error severity.
    finding.effective_severity = ReportSeverity::Error;
    finding.raw_severity = ReportSeverity::Error;
    finding
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

/// Populate `details.path` for every finding that has a `doc_id` and no path yet.
pub fn attach_doc_paths(findings: &mut [Finding], graph: &DocumentGraph) {
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
#[must_use]
pub fn resolve_test_files(config: &Config, project_root: &Path) -> Vec<std::path::PathBuf> {
    resolve_test_files_for_project(config, project_root, None)
}

/// Resolve test file paths for one selected project, or all projects when
/// `project` is `None`.
#[must_use]
pub fn resolve_test_files_for_project(
    config: &Config,
    project_root: &Path,
    project: Option<&str>,
) -> Vec<std::path::PathBuf> {
    let globs = collect_test_globs_for_project(config, project);
    resolve_test_globs(&globs, project_root, config.test_discovery.ignore)
}

fn collect_test_globs_for_project<'a>(config: &'a Config, project: Option<&str>) -> Vec<&'a str> {
    let mut globs = Vec::new();

    if let Some(ref test_globs) = config.tests {
        globs.extend(test_globs.iter().map(String::as_str));
    }

    if let Some(ref projects) = config.projects {
        if let Some(project_name) = project {
            if let Some(project) = projects.get(project_name) {
                globs.extend(project.tests.iter().map(String::as_str));
            }
        } else {
            for project in projects.values() {
                globs.extend(project.tests.iter().map(String::as_str));
            }
        }
    }

    globs
}

fn resolve_test_globs(
    globs: &[&str],
    project_root: &Path,
    ignore: TestDiscoveryIgnoreMode,
) -> Vec<PathBuf> {
    match ignore {
        TestDiscoveryIgnoreMode::Standard => resolve_test_globs_standard(globs, project_root),
        TestDiscoveryIgnoreMode::Off => {
            supersigil_core::expand_globs(globs.iter().copied(), project_root)
        }
    }
}

fn resolve_test_globs_standard(globs: &[&str], project_root: &Path) -> Vec<PathBuf> {
    if globs.is_empty() {
        return Vec::new();
    }

    let absolute_globs: Vec<_> = globs
        .iter()
        .map(|glob| absolute_glob_pattern(glob, project_root))
        .collect();
    let glob_set = build_glob_set(absolute_globs.iter().map(String::as_str));

    if glob_set.is_none() {
        return Vec::new();
    }

    let mut paths = BTreeSet::new();
    for scope in walk_scopes_for_globs(globs, project_root) {
        let match_roots = scope.match_roots.clone();
        let walker = WalkBuilder::new(&scope.walk_root)
            .standard_filters(true)
            .filter_entry(move |entry| should_visit_walk_entry(entry.path(), &match_roots))
            .build();

        for entry in walker.filter_map(Result::ok) {
            if !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                continue;
            }

            let path = entry.into_path();
            if glob_set
                .as_ref()
                .is_some_and(|glob_set| glob_set.is_match(&path))
            {
                paths.insert(path);
            }
        }
    }

    paths.into_iter().collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WalkScope {
    walk_root: PathBuf,
    match_roots: Vec<PathBuf>,
}

fn walk_scopes_for_globs(globs: &[&str], project_root: &Path) -> Vec<WalkScope> {
    let roots: Vec<PathBuf> = globs
        .iter()
        .map(|glob| walk_root_for_glob(glob, project_root))
        .collect();

    let mut walk_roots: Vec<PathBuf> = roots
        .iter()
        .map(|root| walk_context_root(root, project_root))
        .collect();
    walk_roots.sort();
    walk_roots.dedup();

    let mut scopes = Vec::new();

    for walk_root in walk_roots {
        let match_roots = minimal_walk_roots(
            roots
                .iter()
                .filter(|root| walk_context_root(root, project_root) == walk_root)
                .cloned()
                .collect(),
        );
        scopes.push(WalkScope {
            walk_root,
            match_roots,
        });
    }

    scopes
}

fn minimal_walk_roots(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
    roots.sort();
    roots.dedup();

    let mut minimal_roots: Vec<PathBuf> = Vec::new();
    for root in roots {
        if minimal_roots
            .iter()
            .any(|existing| root.starts_with(existing))
        {
            continue;
        }
        minimal_roots.push(root);
    }

    minimal_roots
}

fn walk_context_root(root: &Path, project_root: &Path) -> PathBuf {
    let root = normalize_path_lexically(root);
    let project_root = normalize_path_lexically(project_root);

    if root.starts_with(&project_root) {
        project_root
    } else {
        nearest_vcs_root(&root).unwrap_or(root)
    }
}

fn should_visit_walk_entry(path: &Path, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| root.starts_with(path) || path.starts_with(root))
}

fn nearest_vcs_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

fn walk_root_for_glob(pattern: &str, project_root: &Path) -> PathBuf {
    let (root, _) = split_glob_pattern(pattern);
    resolve_glob_root(pattern, root, project_root)
}

fn absolute_glob_pattern(pattern: &str, project_root: &Path) -> String {
    let (root, suffix) = split_glob_pattern(pattern);
    let mut pattern = resolve_glob_root(pattern, root, project_root);
    if !suffix.as_os_str().is_empty() {
        pattern.push(suffix);
    }
    pattern.to_string_lossy().into_owned()
}

fn split_glob_pattern(pattern: &str) -> (PathBuf, PathBuf) {
    let pattern_path = Path::new(pattern);
    let mut root = PathBuf::new();
    let mut suffix = PathBuf::new();
    let mut in_suffix = false;

    for component in pattern_path.components() {
        if !in_suffix && component_has_glob_meta(&component) {
            in_suffix = true;
        }

        if in_suffix {
            suffix.push(component.as_os_str());
        } else {
            root.push(component.as_os_str());
        }
    }

    (root, suffix)
}

fn resolve_glob_root(pattern: &str, root: PathBuf, project_root: &Path) -> PathBuf {
    let pattern_path = Path::new(pattern);
    if root.as_os_str().is_empty() {
        if pattern_path.is_absolute() {
            normalize_path_lexically(Path::new(&std::path::MAIN_SEPARATOR.to_string()))
        } else {
            normalize_path_lexically(project_root)
        }
    } else if root.is_absolute() {
        normalize_path_lexically(&root)
    } else {
        normalize_path_lexically(&project_root.join(root))
    }
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if normalized
                    .components()
                    .next_back()
                    .is_some_and(|component| matches!(component, std::path::Component::Normal(_)))
                {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push("..");
                }
            }
            std::path::Component::Normal(value) => normalized.push(value),
        }
    }

    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn component_has_glob_meta(component: &std::path::Component<'_>) -> bool {
    match component {
        std::path::Component::Normal(value) => {
            let value = value.to_string_lossy();
            value.contains('*') || value.contains('?') || value.contains('[') || value.contains('{')
        }
        _ => false,
    }
}

fn build_glob_set<'a>(globs: impl Iterator<Item = &'a str>) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let mut has_glob = false;

    for glob in globs {
        let Ok(glob) = GlobBuilder::new(glob).literal_separator(true).build() else {
            continue;
        };
        builder.add(glob);
        has_glob = true;
    }

    has_glob.then(|| builder.build().ok()).flatten()
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

    fn relative_paths(root: &Path, paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn git_tempdir() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        dir
    }

    fn write_file(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn config_with_tests(patterns: &[&str]) -> Config {
        let mut config = test_config();
        config.tests = Some(
            patterns
                .iter()
                .map(|pattern| (*pattern).to_owned())
                .collect(),
        );
        config
    }

    fn ignored_node_modules_fixture() -> TempDir {
        let dir = git_tempdir();
        std::fs::write(dir.path().join(".gitignore"), "node_modules/\ndist/\n").unwrap();
        write_file(dir.path(), "tests/unit/auth.test.ts", "test");
        write_file(dir.path(), "tests/node_modules/ignored.test.ts", "test");
        write_file(dir.path(), "tests/dist/ignored.test.ts", "test");
        dir
    }

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

        let report = finalize_report(1, vec![kept.clone(), filtered], None);

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
        let artifact_graph = make_artifact_graph_with_evidence(&graph);

        let report = finalize_report(1, Vec::new(), Some(&artifact_graph));

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
    fn resolve_test_files_standard_mode_excludes_gitignored_paths() {
        let dir = ignored_node_modules_fixture();
        let config = config_with_tests(&["tests/**/*.test.ts"]);

        let test_files = resolve_test_files(&config, dir.path());

        assert_eq!(
            relative_paths(dir.path(), &test_files),
            vec!["tests/unit/auth.test.ts"],
        );
    }

    #[test]
    fn resolve_test_files_off_mode_includes_gitignored_matches() {
        let dir = ignored_node_modules_fixture();
        let mut config = config_with_tests(&["tests/**/*.test.ts"]);
        config.test_discovery.ignore = supersigil_core::TestDiscoveryIgnoreMode::Off;

        let test_files = resolve_test_files(&config, dir.path());

        assert_eq!(
            relative_paths(dir.path(), &test_files),
            vec![
                "tests/dist/ignored.test.ts",
                "tests/node_modules/ignored.test.ts",
                "tests/unit/auth.test.ts",
            ],
        );
    }

    #[test]
    fn resolve_test_files_standard_mode_respects_nested_gitignore() {
        let dir = git_tempdir();
        write_file(dir.path(), "packages/app/.gitignore", "generated/\n");
        write_file(dir.path(), "packages/app/generated/ignored.test.ts", "test");
        write_file(dir.path(), "packages/app/src/auth.test.ts", "test");
        let config = config_with_tests(&["packages/**/*.test.ts"]);

        let test_files = resolve_test_files(&config, dir.path());

        assert_eq!(
            relative_paths(dir.path(), &test_files),
            vec!["packages/app/src/auth.test.ts"],
        );
    }

    #[test]
    fn resolve_test_files_standard_mode_excludes_ignored_literal_glob_root() {
        let dir = git_tempdir();
        std::fs::write(dir.path().join(".gitignore"), "dist/\n").unwrap();
        write_file(dir.path(), "dist/ignored.test.ts", "test");
        let config = config_with_tests(&["dist/**/*.test.ts"]);

        let test_files = resolve_test_files(&config, dir.path());

        assert!(
            test_files.is_empty(),
            "ignored literal glob roots should not be scanned: {test_files:?}",
        );
    }

    #[test]
    fn resolve_test_files_standard_mode_single_star_does_not_match_nested_paths() {
        let dir = git_tempdir();
        write_file(dir.path(), "tests/direct_test.rs", "test");
        write_file(
            dir.path(),
            "tests/nested/ignored_by_pattern_test.rs",
            "test",
        );
        let config = config_with_tests(&["tests/*.rs"]);

        let test_files = resolve_test_files(&config, dir.path());

        assert_eq!(
            relative_paths(dir.path(), &test_files),
            vec!["tests/direct_test.rs"],
        );
    }

    #[test]
    fn resolve_test_files_standard_mode_supports_absolute_globs() {
        let project = git_tempdir();
        let external = git_tempdir();
        write_file(external.path(), "external_tests/auth_test.rs", "test");
        let absolute_glob = external
            .path()
            .join("external_tests/**/*.rs")
            .to_string_lossy()
            .into_owned();
        let config = config_with_tests(&[&absolute_glob]);

        let test_files = resolve_test_files(&config, project.path());

        assert_eq!(
            test_files,
            vec![external.path().join("external_tests/auth_test.rs")],
        );
    }

    #[test]
    fn resolve_test_files_standard_mode_excludes_ignored_absolute_literal_glob_root() {
        let project = git_tempdir();
        let external = git_tempdir();
        std::fs::write(external.path().join(".gitignore"), "external_tests/\n").unwrap();
        write_file(external.path(), "external_tests/ignored_test.rs", "test");
        let absolute_glob = external
            .path()
            .join("external_tests/**/*.rs")
            .to_string_lossy()
            .into_owned();
        let config = config_with_tests(&[&absolute_glob]);

        let test_files = resolve_test_files(&config, project.path());

        assert!(
            test_files.is_empty(),
            "ignored absolute literal glob roots should not be scanned: {test_files:?}",
        );
    }

    #[test]
    fn resolve_test_files_standard_mode_does_not_match_relative_globs_outside_project() {
        let project = git_tempdir();
        let external = git_tempdir();
        write_file(project.path(), "tests/project_test.rs", "test");
        write_file(external.path(), "external_tests/expected.test.ts", "test");
        write_file(external.path(), "external_tests/leaked.rs", "test");
        let absolute_glob = external
            .path()
            .join("external_tests/*.test.ts")
            .to_string_lossy()
            .into_owned();
        let config = config_with_tests(&["**/*.rs", &absolute_glob]);

        let test_files = resolve_test_files(&config, project.path());
        let mut expected = vec![
            project.path().join("tests/project_test.rs"),
            external.path().join("external_tests/expected.test.ts"),
        ];
        expected.sort();

        assert_eq!(test_files, expected);
    }

    #[test]
    fn resolve_test_files_standard_mode_supports_parent_relative_globs() {
        let workspace = TempDir::new().unwrap();
        let project_root = workspace.path().join("project");
        std::fs::create_dir_all(project_root.join(".git")).unwrap();
        write_file(workspace.path(), "shared/auth_test.rs", "test");
        write_file(workspace.path(), "project/local_test.rs", "test");
        let config = config_with_tests(&["../shared/**/*.rs"]);

        let test_files = resolve_test_files(&config, &project_root);

        assert_eq!(
            test_files,
            vec![workspace.path().join("shared/auth_test.rs")],
        );
    }

    #[test]
    fn standard_mode_walk_roots_are_limited_to_literal_glob_prefixes() {
        let project = Path::new("/workspace");

        assert_eq!(
            walk_scopes_for_globs(&["tests/**/*.rs"], project),
            vec![WalkScope {
                walk_root: PathBuf::from("/workspace"),
                match_roots: vec![PathBuf::from("/workspace/tests")],
            }],
        );
        assert_eq!(
            walk_scopes_for_globs(&["tests/**/*.rs", "tests/unit/*.rs"], project),
            vec![WalkScope {
                walk_root: PathBuf::from("/workspace"),
                match_roots: vec![PathBuf::from("/workspace/tests")],
            }],
        );
        assert_eq!(
            walk_scopes_for_globs(&["**/*.rs"], project),
            vec![WalkScope {
                walk_root: PathBuf::from("/workspace"),
                match_roots: vec![PathBuf::from("/workspace")],
            }],
        );
        assert_eq!(
            walk_scopes_for_globs(&["../shared/**/*.rs"], Path::new("/workspace/project")),
            vec![WalkScope {
                walk_root: PathBuf::from("/workspace/shared"),
                match_roots: vec![PathBuf::from("/workspace/shared")],
            }],
        );
    }

    #[test]
    fn resolve_test_files_remains_sorted_and_deduplicated() {
        let dir = git_tempdir();
        write_file(dir.path(), "tests/unit/b_test.rs", "test");
        write_file(dir.path(), "tests/unit/a_test.rs", "test");
        let config = config_with_tests(&["tests/**/*.rs", "tests/unit/*.rs"]);

        let test_files = resolve_test_files(&config, dir.path());

        assert_eq!(
            relative_paths(dir.path(), &test_files),
            vec!["tests/unit/a_test.rs", "tests/unit/b_test.rs"],
        );
    }

    #[test]
    fn resolve_test_files_off_mode_remains_sorted_and_deduplicated() {
        let dir = git_tempdir();
        write_file(dir.path(), "tests/unit/b_test.rs", "test");
        write_file(dir.path(), "tests/unit/a_test.rs", "test");
        let mut config = config_with_tests(&["tests/**/*.rs", "tests/unit/*.rs"]);
        config.test_discovery.ignore = supersigil_core::TestDiscoveryIgnoreMode::Off;

        let test_files = resolve_test_files(&config, dir.path());

        assert_eq!(
            relative_paths(dir.path(), &test_files),
            vec!["tests/unit/a_test.rs", "tests/unit/b_test.rs"],
        );
    }

    #[test]
    fn resolve_test_files_for_project_applies_policy_to_selected_project_only() {
        let dir = git_tempdir();
        std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        write_file(dir.path(), "alpha/tests/ignored/bad.rs", "test");
        write_file(dir.path(), "alpha/tests/unit/good.rs", "test");
        write_file(dir.path(), "beta/tests/unit/other.rs", "test");

        let config = Config {
            paths: None,
            tests: None,
            projects: Some(std::collections::HashMap::from([
                (
                    "alpha".into(),
                    supersigil_core::ProjectConfig {
                        paths: vec!["alpha/specs/**/*.md".into()],
                        tests: vec!["alpha/tests/**/*.rs".into()],
                        isolated: false,
                    },
                ),
                (
                    "beta".into(),
                    supersigil_core::ProjectConfig {
                        paths: vec!["beta/specs/**/*.md".into()],
                        tests: vec!["beta/tests/**/*.rs".into()],
                        isolated: false,
                    },
                ),
            ])),
            ..test_config()
        };

        let test_files = resolve_test_files_for_project(&config, dir.path(), Some("alpha"));

        assert_eq!(
            relative_paths(dir.path(), &test_files),
            vec!["alpha/tests/unit/good.rs"],
        );
    }

    #[test]
    fn verify_inputs_standard_mode_does_not_scan_tags_from_ignored_files() {
        let dir = git_tempdir();
        std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        write_file(
            dir.path(),
            "tests/ignored/tagged_test.rs",
            "// supersigil: req:auth\n",
        );
        write_file(dir.path(), "tests/unit/untagged_test.rs", "// no tag\n");
        let config = config_with_tests(&["tests/**/*.rs"]);

        let inputs = VerifyInputs::resolve(&config, dir.path());

        assert_eq!(
            relative_paths(dir.path(), &inputs.test_files),
            vec!["tests/unit/untagged_test.rs"],
        );
        assert!(
            inputs
                .tag_matches
                .iter()
                .all(|tag_match| tag_match.tag != "req:auth"),
            "ignored files should not contribute tag matches: {:?}",
            inputs.tag_matches,
        );
    }

    #[test]
    fn js_plugin_does_not_warn_for_ignored_malformed_test_file() {
        let dir = git_tempdir();
        std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        write_file(
            dir.path(),
            "tests/ignored/broken.test.ts",
            r#"test("broken", () => { verifies("req/auth#crit-1")"#,
        );

        let mut config = config_with_tests(&["tests/**/*.test.ts"]);
        config.ecosystem.plugins = vec!["js".into()];
        let graph = build_test_graph(Vec::new());
        let inputs = VerifyInputs::resolve(&config, dir.path());

        let (_artifact_graph, plugin_findings) =
            plugins::build_evidence(&config, &graph, dir.path(), None, &inputs);

        assert!(
            plugin_findings.is_empty(),
            "ignored malformed JS tests should not reach plugin discovery: {plugin_findings:?}",
        );
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
    fn verify_project_scope_ignores_tags_from_other_projects() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("alpha_tests")).unwrap();
        std::fs::create_dir_all(dir.path().join("beta_tests")).unwrap();
        std::fs::write(
            dir.path().join("alpha_tests/alpha_test.rs"),
            "// no tag here\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("beta_tests/beta_test.rs"),
            "// supersigil: req:auth\n",
        )
        .unwrap();

        let mut alpha_doc = make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "req-1",
                    make_verified_by_tag("req:auth", 11),
                    10,
                )],
                9,
            )],
        );
        alpha_doc.path = dir.path().join("alpha/specs/auth.md");

        let mut beta_doc = make_doc(
            "req/pay",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "pay-1",
                    make_verified_by_tag("req:pay", 21),
                    20,
                )],
                19,
            )],
        );
        beta_doc.path = dir.path().join("beta/specs/pay.md");

        let config = Config {
            paths: None,
            tests: None,
            projects: Some(std::collections::HashMap::from([
                (
                    "alpha".into(),
                    supersigil_core::ProjectConfig {
                        paths: vec!["alpha/specs/**/*.md".into()],
                        tests: vec!["alpha_tests/**/*.rs".into()],
                        isolated: false,
                    },
                ),
                (
                    "beta".into(),
                    supersigil_core::ProjectConfig {
                        paths: vec!["beta/specs/**/*.md".into()],
                        tests: vec!["beta_tests/**/*.rs".into()],
                        isolated: false,
                    },
                ),
            ])),
            ..test_config()
        };

        let graph = build_test_graph_with_config(vec![alpha_doc, beta_doc], &config);
        let inputs = VerifyInputs::resolve_for_project(&config, dir.path(), Some("alpha"));
        let (findings, doc_ids) = verify_structural(
            &graph,
            &config,
            dir.path(),
            &VerifyOptions {
                project: Some("alpha".into()),
                ..VerifyOptions::default()
            },
            &inputs,
        )
        .unwrap();

        assert_eq!(doc_ids, vec!["req/auth"]);
        assert!(
            findings.iter().any(|finding| {
                finding.rule == RuleName::ZeroTagMatches
                    && finding.doc_id.as_deref() == Some("req/auth")
            }),
            "selected project should not see tags from other projects: {findings:?}",
        );
    }

    #[test]
    fn verify_project_scope_skips_out_of_scope_tracked_glob_expansion() {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALLS: AtomicUsize = AtomicUsize::new(0);

        fn counting_loader(pattern: &str, _base_dir: &Path) -> Vec<PathBuf> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            vec![PathBuf::from(pattern)]
        }

        let dir = TempDir::new().unwrap();

        let mut alpha_doc = make_doc("req/auth", vec![]);
        alpha_doc.path = dir.path().join("alpha/specs/auth.md");

        let mut beta_doc = make_doc("req/pay", vec![make_tracked_files("missing/**/*.rs", 12)]);
        beta_doc.path = dir.path().join("beta/specs/pay.md");

        let config = Config {
            paths: None,
            tests: None,
            projects: Some(std::collections::HashMap::from([
                (
                    "alpha".into(),
                    supersigil_core::ProjectConfig {
                        paths: vec!["alpha/specs/**/*.md".into()],
                        tests: vec!["alpha_tests/**/*.rs".into()],
                        isolated: false,
                    },
                ),
                (
                    "beta".into(),
                    supersigil_core::ProjectConfig {
                        paths: vec!["beta/specs/**/*.md".into()],
                        tests: vec!["beta_tests/**/*.rs".into()],
                        isolated: false,
                    },
                ),
            ])),
            ..test_config()
        };

        let graph = build_test_graph_with_config(vec![alpha_doc, beta_doc], &config);
        let inputs = VerifyInputs::resolve_for_project(&config, dir.path(), Some("alpha"));
        let mut glob_resolver =
            crate::glob_resolver::GlobResolver::with_loader_for_tests(dir.path(), counting_loader);
        let (findings, doc_ids) = verify_structural_with_resolver(
            &graph,
            &config,
            &VerifyOptions {
                project: Some("alpha".into()),
                ..VerifyOptions::default()
            },
            &inputs,
            &mut glob_resolver,
        );

        assert_eq!(doc_ids, vec!["req/auth"]);
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            0,
            "out-of-scope tracked globs should not be expanded during structural verify",
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding.rule != RuleName::EmptyTrackedGlob),
            "tracked-glob findings from other projects should be skipped: {findings:?}",
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

    // -----------------------------------------------------------------------
    // graph_error_findings
    // -----------------------------------------------------------------------

    #[test]
    fn broken_ref_finding_suggests_close_match() {
        use supersigil_core::{GraphError, SourcePosition};

        let errors = vec![GraphError::BrokenRef {
            doc_id: "tasks/auth".into(),
            ref_str: "auth/reqs".into(),
            reason: "document `auth/reqs` not found".into(),
            position: SourcePosition {
                byte_offset: 0,
                line: 5,
                column: 1,
            },
        }];

        let findings = graph_error_findings(&errors, &["auth/req", "design/auth"]);

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("auth/req"),
            "should suggest closest doc ID",
        );
        assert!(findings[0].message.contains("broken ref"));
    }

    #[test]
    fn broken_ref_finding_no_suggestion_for_distant_match() {
        use supersigil_core::{GraphError, SourcePosition};

        let errors = vec![GraphError::BrokenRef {
            doc_id: "tasks/auth".into(),
            ref_str: "completely/different".into(),
            reason: "document `completely/different` not found".into(),
            position: SourcePosition {
                byte_offset: 0,
                line: 3,
                column: 1,
            },
        }];

        let findings = graph_error_findings(&errors, &["auth/req", "design/auth"]);

        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].suggestion.is_none(),
            "no suggestion for distant match",
        );
    }

    #[test]
    fn broken_ref_with_fragment_preserves_fragment_in_suggestion() {
        use supersigil_core::{GraphError, SourcePosition};

        let errors = vec![GraphError::BrokenRef {
            doc_id: "tasks/auth".into(),
            ref_str: "auth/reqs#crit-1".into(),
            reason: "document `auth/reqs` not found".into(),
            position: SourcePosition {
                byte_offset: 0,
                line: 7,
                column: 1,
            },
        }];

        let findings = graph_error_findings(&errors, &["auth/req", "design/auth"]);

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("auth/req#crit-1"),
            "should preserve fragment in suggestion",
        );
    }

    #[test]
    fn broken_ref_fragment_not_found_has_no_suggestion() {
        use supersigil_core::{GraphError, SourcePosition};

        let errors = vec![GraphError::BrokenRef {
            doc_id: "tasks/auth".into(),
            ref_str: "auth/req#typo".into(),
            reason: "fragment `typo` not found in document `auth/req`".into(),
            position: SourcePosition {
                byte_offset: 0,
                line: 5,
                column: 1,
            },
        }];

        let findings = graph_error_findings(&errors, &["auth/req", "design/auth"]);

        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].suggestion.is_none(),
            "fragment-not-found should not suggest a doc ID: {:?}",
            findings[0].suggestion,
        );
    }

    #[test]
    fn non_broken_ref_graph_error_has_no_suggestion_and_error_severity() {
        use supersigil_core::GraphError;

        let errors = vec![GraphError::DuplicateId {
            id: "auth/req".into(),
            paths: vec!["a.md".into(), "b.md".into()],
        }];

        let findings = graph_error_findings(&errors, &["auth/req"]);

        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].suggestion.is_none(),
            "non-BrokenRef errors should not have suggestions",
        );
        assert_eq!(
            findings[0].effective_severity,
            ReportSeverity::Error,
            "graph errors should always be error severity",
        );
    }

    #[test]
    fn task_dependency_broken_ref_has_no_doc_suggestion() {
        use supersigil_core::{GraphError, SourcePosition};

        let errors = vec![GraphError::BrokenRef {
            doc_id: "tasks/auth".into(),
            ref_str: "auth/req".into(),
            reason: "task `auth/req` not found among sibling tasks".into(),
            position: SourcePosition {
                byte_offset: 0,
                line: 5,
                column: 1,
            },
        }];

        let findings = graph_error_findings(&errors, &["auth/req", "design/auth"]);

        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].suggestion.is_none(),
            "task dependency errors should not suggest document IDs: {:?}",
            findings[0].suggestion,
        );
    }

    #[test]
    fn broken_ref_finding_always_error_severity() {
        use supersigil_core::{GraphError, SourcePosition};

        let errors = vec![GraphError::BrokenRef {
            doc_id: "tasks/auth".into(),
            ref_str: "auth/reqs".into(),
            reason: "document `auth/reqs` not found".into(),
            position: SourcePosition {
                byte_offset: 0,
                line: 5,
                column: 1,
            },
        }];

        let findings = graph_error_findings(&errors, &["auth/req"]);

        assert_eq!(
            findings[0].effective_severity,
            ReportSeverity::Error,
            "broken ref findings should be error severity",
        );
        assert_eq!(
            findings[0].raw_severity,
            ReportSeverity::Error,
            "broken ref findings should have error raw severity",
        );
    }

    #[test]
    fn glob_resolver_memoizes_repeated_patterns() {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALLS: AtomicUsize = AtomicUsize::new(0);

        fn counting_loader(pattern: &str, _base_dir: &Path) -> Vec<PathBuf> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            vec![PathBuf::from(pattern)]
        }

        CALLS.store(0, Ordering::SeqCst);
        let dir = TempDir::new().unwrap();
        let mut resolver =
            crate::glob_resolver::GlobResolver::with_loader_for_tests(dir.path(), counting_loader);

        assert_eq!(
            resolver.expand("tests/**/*.rs"),
            &[PathBuf::from("tests/**/*.rs")]
        );
        assert_eq!(
            resolver.expand("tests/**/*.rs"),
            &[PathBuf::from("tests/**/*.rs")]
        );
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            1,
            "repeated patterns should be loaded once per verify run",
        );
    }

    #[test]
    fn glob_resolver_loads_distinct_patterns_independently() {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALLS: AtomicUsize = AtomicUsize::new(0);

        fn counting_loader(pattern: &str, _base_dir: &Path) -> Vec<PathBuf> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            vec![PathBuf::from(pattern)]
        }

        CALLS.store(0, Ordering::SeqCst);
        let dir = TempDir::new().unwrap();
        let mut resolver =
            crate::glob_resolver::GlobResolver::with_loader_for_tests(dir.path(), counting_loader);

        let _ = resolver.expand("tests/**/*.rs");
        let _ = resolver.expand("src/**/*.rs");
        let _ = resolver.expand("tests/**/*.rs");

        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            2,
            "distinct patterns should still resolve independently",
        );
    }
}
