use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;
use std::collections::HashMap;

use supersigil_core::{CRITERION, ContextOutput, VERIFIED_BY};
use supersigil_verify::ArtifactGraph;

use crate::commands::ContextArgs;
use crate::error::CliError;
use crate::format::{
    self, ColorConfig, OutputFormat, Token, status_token, verified_by_label, write_json,
    write_tasks,
};
use crate::loader;
use crate::plugins;

// ---------------------------------------------------------------------------
// Enriched output types
// ---------------------------------------------------------------------------

/// Enriched context output including verification coverage and evidence data.
#[derive(Debug, Serialize)]
struct EnrichedContextOutput {
    document: supersigil_core::SpecDocument,
    criteria: Vec<EnrichedTargetContext>,
    decisions: Vec<supersigil_core::DecisionContext>,
    linked_decisions: Vec<supersigil_core::LinkedDecision>,
    implemented_by: Vec<supersigil_core::DocRef>,
    referenced_by: Vec<String>,
    tasks: Vec<supersigil_core::TaskInfo>,
    /// True when a plugin discovery failure occurred and coverage data may be
    /// incomplete. Consumers should treat `covered: false` with caution when
    /// this flag is set.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    evidence_degraded: bool,
}

/// A verification target enriched with coverage status, strategies, and evidence.
#[derive(Debug, Serialize)]
struct EnrichedTargetContext {
    id: String,
    target_ref: String,
    body_text: Option<String>,
    covered: bool,
    verified_by: Vec<String>,
    evidence: Vec<EvidenceEntry>,
    referenced_by: Vec<supersigil_core::DocRef>,
}

/// A single evidence entry referencing a test that covers a criterion.
#[derive(Debug, Serialize)]
struct EvidenceEntry {
    test_name: String,
    file: String,
    line: usize,
}

// ---------------------------------------------------------------------------
// Enrichment
// ---------------------------------------------------------------------------

/// Enrich a `ContextOutput` with verification coverage data from the artifact graph.
fn enrich_context(
    ctx: ContextOutput,
    artifact_graph: &ArtifactGraph<'_>,
    project_root: &Path,
    evidence_degraded: bool,
) -> EnrichedContextOutput {
    let doc_id = &ctx.document.frontmatter.id;
    let verified_by_map = collect_all_verified_by(&ctx.document.components);

    let criteria = ctx
        .criteria
        .into_iter()
        .map(|crit| {
            let evidence_ids = artifact_graph.evidence_for(doc_id, &crit.id);
            let covered = evidence_ids.is_some_and(|ids| !ids.is_empty());

            let verified_by = verified_by_map
                .get(crit.id.as_str())
                .cloned()
                .unwrap_or_default();

            let evidence = evidence_ids
                .unwrap_or_default()
                .iter()
                .map(|id| {
                    let record = &artifact_graph.evidence[id.index()];
                    let file_path = &record.source_location.file;
                    let rel = file_path
                        .strip_prefix(project_root)
                        .unwrap_or(file_path)
                        .to_string_lossy()
                        .into_owned();
                    EvidenceEntry {
                        test_name: record.test.name.clone(),
                        file: rel,
                        line: record.source_location.line,
                    }
                })
                .collect();

            EnrichedTargetContext {
                id: crit.id,
                target_ref: crit.target_ref,
                body_text: crit.body_text,
                covered,
                verified_by,
                evidence,
                referenced_by: crit.referenced_by,
            }
        })
        .collect();

    EnrichedContextOutput {
        document: ctx.document,
        criteria,
        decisions: ctx.decisions,
        linked_decisions: ctx.linked_decisions,
        implemented_by: ctx.implemented_by,
        referenced_by: ctx.referenced_by,
        tasks: ctx.tasks,
        evidence_degraded,
    }
}

/// Single-pass collection of `VerifiedBy` labels for all criteria in the component tree.
fn collect_all_verified_by(
    components: &[supersigil_core::ExtractedComponent],
) -> HashMap<&str, Vec<String>> {
    let mut map = HashMap::new();
    collect_verified_by_recursive(components, &mut map);
    map
}

fn collect_verified_by_recursive<'a>(
    components: &'a [supersigil_core::ExtractedComponent],
    map: &mut HashMap<&'a str, Vec<String>>,
) {
    for comp in components {
        if comp.name == CRITERION
            && let Some(crit_id) = comp.attributes.get("id")
        {
            let labels: Vec<String> = comp
                .children
                .iter()
                .filter(|child| child.name == VERIFIED_BY)
                .map(verified_by_label)
                .collect();
            if !labels.is_empty() {
                map.insert(crit_id.as_str(), labels);
            }
        }
        collect_verified_by_recursive(&comp.children, map);
    }
}

// ---------------------------------------------------------------------------
// Command entry point
// ---------------------------------------------------------------------------

/// Run the `context` command: show structured view of a document.
///
/// # Errors
///
/// Returns `CliError` if the graph cannot be loaded, the document is not
/// found, or output fails.
pub fn run(args: &ContextArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (config, graph) = loader::load_graph(config_path)?;

    // Resolve the target document before building evidence so that a missing
    // document ID fails fast without the cost of evidence discovery.
    let ctx = match graph.context(&args.id) {
        Ok(ctx) => ctx,
        Err(e) => {
            format::hint(color, "Run `supersigil ls` to see available document IDs.");
            return Err(e.into());
        }
    };

    let project_root = loader::project_root(config_path);
    let inputs = supersigil_verify::VerifyInputs::resolve(&config, project_root);
    let (artifact_graph, plugin_findings) =
        plugins::build_evidence(&config, &graph, project_root, None, &inputs);
    let evidence_degraded = plugin_findings
        .iter()
        .any(|f| f.rule == supersigil_verify::RuleName::PluginDiscoveryFailure);
    plugins::warn_plugin_findings(&plugin_findings, color);

    let mut enriched = enrich_context(ctx, &artifact_graph, project_root, evidence_degraded);
    if args.detail == format::Detail::Compact {
        enriched.document.components.clear();
    }

    match args.format {
        OutputFormat::Json => {
            write_json(&enriched)?;
        }
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            write_context_terminal(&mut out, &enriched, color)?;
        }
    }

    Ok(())
}

/// Write a single enriched criterion with coverage marker, verified-by,
/// evidence, and referenced-by lines.
fn write_criterion(
    out: &mut impl Write,
    crit: &EnrichedTargetContext,
    c: ColorConfig,
) -> io::Result<()> {
    let body = crit.body_text.as_deref().unwrap_or("(no description)");
    let coverage_marker = if crit.covered {
        c.paint(Token::StatusGood, "[covered]")
    } else {
        c.paint(Token::StatusBad, "[uncovered]")
    };
    writeln!(
        out,
        "- {}: {body} {coverage_marker}",
        c.paint(Token::DocId, &crit.id),
    )?;
    for vb in &crit.verified_by {
        writeln!(
            out,
            "  {} {}",
            c.paint(Token::Label, "verified by:"),
            c.paint(Token::Path, vb),
        )?;
    }
    for ev in &crit.evidence {
        writeln!(
            out,
            "  {} {} {}",
            c.paint(Token::Label, "evidence:"),
            ev.test_name,
            c.paint(Token::Path, &format!("({}:{})", ev.file, ev.line)),
        )?;
    }
    for vref in &crit.referenced_by {
        let vstatus = vref.status.as_deref().unwrap_or("?");
        writeln!(
            out,
            "  -> Referenced by: {} ({vstatus})",
            c.paint(Token::DocId, &vref.doc_id),
        )?;
    }
    Ok(())
}

/// Write the context output in terminal format.
fn write_context_terminal(
    out: &mut impl Write,
    ctx: &EnrichedContextOutput,
    color: ColorConfig,
) -> io::Result<()> {
    let c = color;
    let doc = &ctx.document;
    let doc_type = doc.frontmatter.doc_type.as_deref().unwrap_or("document");
    let status = doc.frontmatter.status.as_deref().unwrap_or("(none)");

    writeln!(
        out,
        "{} {}",
        c.paint(Token::Header, &format!("# {doc_type}:")),
        c.paint(Token::DocId, &doc.frontmatter.id),
    )?;
    writeln!(
        out,
        "{} {}",
        c.paint(Token::Label, "Status:"),
        c.paint(status_token(status), status),
    )?;

    if !ctx.criteria.is_empty() {
        writeln!(
            out,
            "\n{}",
            c.paint(Token::Header, "## Verification targets:")
        )?;
        for crit in &ctx.criteria {
            write_criterion(out, crit, c)?;
        }
    }

    if !ctx.decisions.is_empty() {
        writeln!(out, "\n{}", c.paint(Token::Header, "## Decisions:"))?;
        for dec in &ctx.decisions {
            let body = dec.body_text.as_deref().unwrap_or("(no description)");
            writeln!(out, "- {}: {body}", c.paint(Token::DocId, &dec.id))?;
            if let Some(rationale) = &dec.rationale_text {
                writeln!(out, "  Rationale: {rationale}")?;
            }
            if !dec.alternatives.is_empty() {
                let alts: Vec<String> = dec
                    .alternatives
                    .iter()
                    .map(|a| format!("{} ({})", a.id, a.status))
                    .collect();
                writeln!(out, "  Alternatives: {}", alts.join(", "))?;
            }
        }
    }

    if !ctx.linked_decisions.is_empty() {
        writeln!(
            out,
            "\n{}",
            c.paint(Token::Header, "## Linked decisions (from other documents):")
        )?;
        for ld in &ctx.linked_decisions {
            let body = ld.body_text.as_deref().unwrap_or("(no description)");
            writeln!(
                out,
                "- {}#{}: {body}",
                c.paint(Token::DocId, &ld.source_doc_id),
                c.paint(Token::DocId, &ld.decision_id),
            )?;
        }
    }

    if !ctx.implemented_by.is_empty() {
        writeln!(out, "\n{}", c.paint(Token::Header, "## Implemented by:"))?;
        for imp in &ctx.implemented_by {
            let imp_status = imp.status.as_deref().unwrap_or("?");
            writeln!(
                out,
                "- {} ({imp_status})",
                c.paint(Token::DocId, &imp.doc_id),
            )?;
        }
    }

    if !ctx.referenced_by.is_empty() {
        writeln!(out, "\n{}", c.paint(Token::Header, "## Referenced by:"))?;
        for ref_doc in &ctx.referenced_by {
            writeln!(out, "- {ref_doc}")?;
        }
    }

    if !ctx.tasks.is_empty() {
        writeln!(
            out,
            "\n{}",
            c.paint(Token::Header, "## Tasks (in dependency order):"),
        )?;
        write_tasks(out, &ctx.tasks, color)?;
    }

    Ok(())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use supersigil_evidence::{
        EvidenceId, PluginProvenance, SourceLocation, TestIdentity, TestKind, VerifiableRef,
        VerificationTargets,
    };
    use supersigil_rust::verifies;
    use supersigil_verify::build_artifact_graph;
    use supersigil_verify::test_helpers::{
        build_test_graph, make_acceptance_criteria, make_criterion,
        make_criterion_with_verified_by, make_doc, make_verified_by_tag,
    };

    use super::*;

    /// Create a minimal evidence record for testing.
    fn make_evidence(
        id: usize,
        file: &str,
        name: &str,
        doc_id: &str,
        target_id: &str,
        line: usize,
    ) -> supersigil_evidence::VerificationEvidenceRecord {
        supersigil_evidence::VerificationEvidenceRecord {
            id: EvidenceId::new(id),
            targets: VerificationTargets::new(BTreeSet::from([VerifiableRef {
                doc_id: doc_id.into(),
                target_id: target_id.into(),
            }]))
            .expect("non-empty target set"),
            test: TestIdentity {
                file: PathBuf::from(file),
                name: name.into(),
                kind: TestKind::Unit,
            },
            source_location: SourceLocation {
                file: PathBuf::from(file),
                line,
                column: 1,
            },
            provenance: vec![PluginProvenance::RustAttribute {
                attribute_span: SourceLocation {
                    file: PathBuf::from(file),
                    line,
                    column: 1,
                },
            }],
            metadata: BTreeMap::default(),
        }
    }

    // -------------------------------------------------------------------
    // 1. Criterion with no evidence
    // -------------------------------------------------------------------

    #[verifies("work-queries/req#req-7-1")]
    #[test]
    fn enrich_criterion_no_evidence_yields_uncovered_empty_arrays() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ctx = graph.context("req/auth").unwrap();
        let artifact_graph = ArtifactGraph::empty(&graph);

        let enriched = enrich_context(ctx, &artifact_graph, Path::new("/project"), false);

        assert_eq!(enriched.criteria.len(), 1);
        let crit = &enriched.criteria[0];
        assert_eq!(crit.id, "crit-1");
        assert!(!crit.covered, "should be uncovered with no evidence");
        assert!(
            crit.verified_by.is_empty(),
            "verified_by should be empty array"
        );
        assert!(crit.evidence.is_empty(), "evidence should be empty array");
    }

    // -------------------------------------------------------------------
    // 2. Criterion with VerifiedBy tag strategy
    // -------------------------------------------------------------------

    #[verifies("work-queries/req#req-7-1")]
    #[test]
    fn enrich_criterion_with_verified_by_tag_strategy() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "crit-1",
                    make_verified_by_tag("prop:auth", 11),
                    10,
                )],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ctx = graph.context("req/auth").unwrap();
        let artifact_graph = ArtifactGraph::empty(&graph);

        let enriched = enrich_context(ctx, &artifact_graph, Path::new("/project"), false);

        assert_eq!(enriched.criteria.len(), 1);
        let crit = &enriched.criteria[0];
        assert_eq!(crit.verified_by, vec!["tag:prop:auth"]);
        assert!(!crit.covered, "no evidence, so still uncovered");
    }

    // -------------------------------------------------------------------
    // 3. Criterion with evidence
    // -------------------------------------------------------------------

    #[verifies("work-queries/req#req-7-4")]
    #[test]
    fn enrich_criterion_with_evidence_yields_covered_and_entries() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ctx = graph.context("req/auth").unwrap();

        let evidence = vec![make_evidence(
            0,
            "/project/tests/auth_test.rs",
            "test_login",
            "req/auth",
            "crit-1",
            42,
        )];
        let artifact_graph = build_artifact_graph(&graph, evidence, vec![]);

        let enriched = enrich_context(ctx, &artifact_graph, Path::new("/project"), false);

        assert_eq!(enriched.criteria.len(), 1);
        let crit = &enriched.criteria[0];
        assert!(crit.covered, "should be covered with evidence");
        assert_eq!(crit.evidence.len(), 1);
        assert_eq!(crit.evidence[0].test_name, "test_login");
        assert_eq!(crit.evidence[0].file, "tests/auth_test.rs");
        assert_eq!(crit.evidence[0].line, 42);
    }

    // -------------------------------------------------------------------
    // 4. Multiple criteria: mixed covered and uncovered
    // -------------------------------------------------------------------

    #[verifies("work-queries/req#req-7-1", "work-queries/req#req-7-4")]
    #[test]
    fn enrich_mixed_covered_and_uncovered_criteria() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10), make_criterion("crit-2", 11)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ctx = graph.context("req/auth").unwrap();

        let evidence = vec![make_evidence(
            0,
            "/project/tests/auth_test.rs",
            "test_login",
            "req/auth",
            "crit-1",
            42,
        )];
        let artifact_graph = build_artifact_graph(&graph, evidence, vec![]);

        let enriched = enrich_context(ctx, &artifact_graph, Path::new("/project"), false);

        assert_eq!(enriched.criteria.len(), 2);

        let crit1 = enriched.criteria.iter().find(|c| c.id == "crit-1").unwrap();
        assert!(crit1.covered, "crit-1 should be covered");
        assert_eq!(crit1.evidence.len(), 1);

        let crit2 = enriched.criteria.iter().find(|c| c.id == "crit-2").unwrap();
        assert!(!crit2.covered, "crit-2 should be uncovered");
        assert!(crit2.evidence.is_empty());
    }

    // -------------------------------------------------------------------
    // 5. Evidence file path is made project-root-relative
    // -------------------------------------------------------------------

    #[verifies("work-queries/req#req-7-4")]
    #[test]
    fn evidence_file_path_is_project_root_relative() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ctx = graph.context("req/auth").unwrap();

        let evidence = vec![make_evidence(
            0,
            "/home/user/project/tests/auth_test.rs",
            "test_login",
            "req/auth",
            "crit-1",
            10,
        )];
        let artifact_graph = build_artifact_graph(&graph, evidence, vec![]);

        let enriched = enrich_context(ctx, &artifact_graph, Path::new("/home/user/project"), false);

        let entry = &enriched.criteria[0].evidence[0];
        assert_eq!(
            entry.file, "tests/auth_test.rs",
            "path should be relative to project root"
        );
    }

    // -------------------------------------------------------------------
    // 6. Passthrough fields preserved
    // -------------------------------------------------------------------

    #[verifies("work-queries/req#req-7-1")]
    #[test]
    fn enrich_preserves_passthrough_fields() {
        let docs = vec![make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph(docs);
        let ctx = graph.context("req/auth").unwrap();
        let artifact_graph = ArtifactGraph::empty(&graph);

        let enriched = enrich_context(ctx, &artifact_graph, Path::new("/project"), false);

        assert_eq!(enriched.document.frontmatter.id, "req/auth");
        assert!(enriched.decisions.is_empty());
        assert!(enriched.linked_decisions.is_empty());
        assert!(enriched.implemented_by.is_empty());
        assert!(enriched.tasks.is_empty());
    }

    // -------------------------------------------------------------------
    // Helper: build an EnrichedContextOutput directly for renderer tests
    // -------------------------------------------------------------------

    fn make_enriched_output(criteria: Vec<EnrichedTargetContext>) -> EnrichedContextOutput {
        EnrichedContextOutput {
            document: supersigil_core::SpecDocument {
                path: PathBuf::from("specs/auth.md"),
                frontmatter: supersigil_core::Frontmatter {
                    id: "req/auth".into(),
                    doc_type: Some("requirements".into()),
                    status: Some("approved".into()),
                },
                extra: HashMap::new(),
                components: Vec::new(),
            },
            criteria,
            decisions: Vec::new(),
            linked_decisions: Vec::new(),
            implemented_by: Vec::new(),
            referenced_by: Vec::new(),
            tasks: Vec::new(),
            evidence_degraded: false,
        }
    }

    // -------------------------------------------------------------------
    // 7. Terminal: covered criterion shows marker, verified-by, evidence
    // -------------------------------------------------------------------

    #[verifies("work-queries/req#req-7-2", "work-queries/req#req-7-3")]
    #[test]
    fn terminal_covered_criterion_shows_marker_verified_by_and_evidence() {
        let enriched = make_enriched_output(vec![EnrichedTargetContext {
            id: "login-succeeds".into(),
            target_ref: "req/auth#login-succeeds".into(),
            body_text: Some("WHEN valid credentials THEN login succeeds".into()),
            covered: true,
            verified_by: vec!["tag:auth-login".into()],
            evidence: vec![
                EvidenceEntry {
                    test_name: "test_user_login".into(),
                    file: "tests/auth.rs".into(),
                    line: 42,
                },
                EvidenceEntry {
                    test_name: "test_login_with_mfa".into(),
                    file: "tests/auth.rs".into(),
                    line: 87,
                },
            ],
            referenced_by: vec![supersigil_core::DocRef {
                doc_id: "auth/design".into(),
                status: Some("in-progress".into()),
            }],
        }]);

        let mut buf = Vec::new();
        write_context_terminal(&mut buf, &enriched, ColorConfig::no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("login-succeeds: WHEN valid credentials THEN login succeeds [covered]"),
            "should show [covered] marker after body text.\nGot:\n{output}"
        );
        assert!(
            output.contains("  verified by: tag:auth-login"),
            "should show verified-by line.\nGot:\n{output}"
        );
        assert!(
            output.contains("  evidence: test_user_login (tests/auth.rs:42)"),
            "should show first evidence line.\nGot:\n{output}"
        );
        assert!(
            output.contains("  evidence: test_login_with_mfa (tests/auth.rs:87)"),
            "should show second evidence line.\nGot:\n{output}"
        );
        assert!(
            output.contains("  -> Referenced by: auth/design (in-progress)"),
            "should still show referenced-by lines.\nGot:\n{output}"
        );
    }

    // -------------------------------------------------------------------
    // 8. Terminal: uncovered criterion shows marker, no evidence
    // -------------------------------------------------------------------

    #[verifies("work-queries/req#req-7-2", "work-queries/req#req-7-3")]
    #[test]
    fn terminal_uncovered_criterion_shows_marker_and_no_evidence() {
        let enriched = make_enriched_output(vec![EnrichedTargetContext {
            id: "login-fails".into(),
            target_ref: "req/auth#login-fails".into(),
            body_text: Some("WHEN invalid credentials THEN login fails".into()),
            covered: false,
            verified_by: vec!["tag:auth-fail".into()],
            evidence: vec![],
            referenced_by: vec![supersigil_core::DocRef {
                doc_id: "auth/design".into(),
                status: Some("in-progress".into()),
            }],
        }]);

        let mut buf = Vec::new();
        write_context_terminal(&mut buf, &enriched, ColorConfig::no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("login-fails: WHEN invalid credentials THEN login fails [uncovered]"),
            "should show [uncovered] marker after body text.\nGot:\n{output}"
        );
        assert!(
            output.contains("  verified by: tag:auth-fail"),
            "should show verified-by line even when uncovered.\nGot:\n{output}"
        );
        assert!(
            !output.contains("evidence:"),
            "should not show evidence lines when uncovered.\nGot:\n{output}"
        );
    }

    // -------------------------------------------------------------------
    // 9. Terminal: mixed criteria in one document
    // -------------------------------------------------------------------

    #[verifies("work-queries/req#req-7-2", "work-queries/req#req-7-3")]
    #[test]
    fn terminal_mixed_criteria_shows_both_markers() {
        let enriched = make_enriched_output(vec![
            EnrichedTargetContext {
                id: "crit-covered".into(),
                target_ref: "req/auth#crit-covered".into(),
                body_text: Some("covered criterion".into()),
                covered: true,
                verified_by: vec!["tag:test".into()],
                evidence: vec![EvidenceEntry {
                    test_name: "test_it".into(),
                    file: "tests/it.rs".into(),
                    line: 10,
                }],
                referenced_by: vec![],
            },
            EnrichedTargetContext {
                id: "crit-uncovered".into(),
                target_ref: "req/auth#crit-uncovered".into(),
                body_text: Some("uncovered criterion".into()),
                covered: false,
                verified_by: vec![],
                evidence: vec![],
                referenced_by: vec![],
            },
        ]);

        let mut buf = Vec::new();
        write_context_terminal(&mut buf, &enriched, ColorConfig::no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("crit-covered: covered criterion [covered]"),
            "first criterion should be [covered].\nGot:\n{output}"
        );
        assert!(
            output.contains("  evidence: test_it (tests/it.rs:10)"),
            "first criterion should have evidence.\nGot:\n{output}"
        );
        assert!(
            output.contains("crit-uncovered: uncovered criterion [uncovered]"),
            "second criterion should be [uncovered].\nGot:\n{output}"
        );
    }
}
