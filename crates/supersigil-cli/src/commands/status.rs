use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;
use supersigil_core::{CRITERION, VERIFIED_BY};
use supersigil_verify::ArtifactGraph;

use crate::commands::StatusArgs;
use crate::error::CliError;
use crate::format::{self, ColorConfig, OutputFormat, Token, status_token, write_json};
use crate::loader;
use crate::plugins;

#[derive(Debug, Serialize)]
struct ProjectStatus {
    total_documents: usize,
    by_type: BTreeMap<String, usize>,
    by_status: BTreeMap<String, usize>,
    targets_total: usize,
    targets_covered: usize,
}

#[derive(Debug, Serialize)]
struct DocumentStatus {
    id: String,
    doc_type: Option<String>,
    status: Option<String>,
    criteria: Vec<TargetStatus>,
    tracked_files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TargetStatus {
    id: String,
    covered: bool,
    /// `VerifiedBy` strategies associated with this criterion (e.g. `"tag:my_tag"`,
    /// `"file-glob:tests/**"`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    verified_by: Vec<String>,
}

/// Run the `status` command.
///
/// # Errors
///
/// Returns `CliError` if loading fails.
pub fn run(args: &StatusArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);
    let inputs = supersigil_verify::VerifyInputs::resolve(&config, project_root);
    let (artifact_graph, plugin_findings) =
        plugins::build_evidence(&config, &graph, project_root, None, &inputs);
    plugins::warn_plugin_findings(&plugin_findings, color);

    match &args.id {
        None => run_project_wide(None, &args.format, &graph, &artifact_graph, color),
        Some(id) => {
            // Exact match → per-document detail view.
            if graph.document(id).is_some() {
                return run_per_document(id, &args.format, &graph, &artifact_graph, color);
            }
            // Prefix match → aggregated project-style summary for matching docs.
            let has_prefix_match = graph
                .documents()
                .any(|(doc_id, _)| doc_id.starts_with(id.as_str()));
            if has_prefix_match {
                return run_project_wide(Some(id), &args.format, &graph, &artifact_graph, color);
            }
            format::hint(color, "Run `supersigil ls` to see available document IDs.");
            Err(supersigil_core::QueryError::NoMatchingDocuments { query: id.clone() }.into())
        }
    }
}

fn run_project_wide(
    prefix: Option<&str>,
    fmt: &OutputFormat,
    graph: &supersigil_core::DocumentGraph,
    artifact_graph: &ArtifactGraph<'_>,
    color: ColorConfig,
) -> Result<(), CliError> {
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0;
    let mut targets = TargetCounts::default();

    for (id, doc) in graph
        .documents()
        .filter(|(id, _)| prefix.is_none_or(|p| id.starts_with(p)))
    {
        total += 1;
        let t = doc
            .frontmatter
            .doc_type
            .as_deref()
            .unwrap_or("untyped")
            .to_owned();
        *by_type.entry(t).or_default() += 1;
        let s = doc
            .frontmatter
            .status
            .as_deref()
            .unwrap_or("(none)")
            .to_owned();
        *by_status.entry(s).or_default() += 1;

        count_targets_recursive(id, &doc.components, artifact_graph, &mut targets);
    }

    let status = ProjectStatus {
        total_documents: total,
        by_type,
        by_status,
        targets_total: targets.total,
        targets_covered: targets.covered,
    };

    match fmt {
        OutputFormat::Json => write_json(&status)?,
        OutputFormat::Terminal => write_project_terminal(&status, prefix, color)?,
    }

    Ok(())
}

fn write_project_terminal(
    status: &ProjectStatus,
    prefix: Option<&str>,
    color: ColorConfig,
) -> Result<(), CliError> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let c = color;
    let heading = match prefix {
        Some(p) => format!("## Status: {p}*"),
        None => "## Project Status".to_owned(),
    };
    writeln!(out, "{}", c.paint(Token::Header, &heading))?;
    writeln!(
        out,
        "{} {}",
        c.paint(Token::Label, "Documents:"),
        c.paint(Token::Count, &status.total_documents.to_string()),
    )?;
    writeln!(out)?;
    writeln!(out, "{}", c.paint(Token::Label, "By type:"))?;
    for (t, count) in &status.by_type {
        writeln!(
            out,
            "  {}: {}",
            c.paint(Token::DocType, t),
            c.paint(Token::Count, &count.to_string()),
        )?;
    }
    writeln!(out)?;
    writeln!(out, "{}", c.paint(Token::Label, "By status:"))?;
    for (s, count) in &status.by_status {
        let tok = status_token(s);
        writeln!(
            out,
            "  {}: {}",
            c.paint(tok, s),
            c.paint(Token::Count, &count.to_string()),
        )?;
    }
    writeln!(out)?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "target counts are small enough for f64"
    )]
    let pct = if status.targets_total > 0 {
        status.targets_covered as f64 / status.targets_total as f64 * 100.0
    } else {
        100.0
    };
    writeln!(
        out,
        "{} {}/{} ({pct:.0}%)",
        c.paint(Token::Label, "Verification coverage:"),
        c.paint(Token::Count, &status.targets_covered.to_string()),
        c.paint(Token::Count, &status.targets_total.to_string()),
    )?;

    let uncovered = status.targets_total.saturating_sub(status.targets_covered);
    if uncovered > 0 {
        format::hint(
            color,
            "Some verification targets are uncovered. Run `supersigil verify` to see details.",
        );
    } else {
        format::hint(
            color,
            "All verification targets covered. Run `supersigil verify` for full verification.",
        );
    }

    Ok(())
}

fn run_per_document(
    id: &str,
    fmt: &OutputFormat,
    graph: &supersigil_core::DocumentGraph,
    artifact_graph: &ArtifactGraph<'_>,
    color: ColorConfig,
) -> Result<(), CliError> {
    let ctx = graph.context(id)?;
    let doc = &ctx.document;

    // Build per-criterion status with VerifiedBy info extracted from the
    // component tree (VerifiedBy is always nested inside Criterion).
    let criteria = build_targets_status(id, &doc.components, artifact_graph);

    let tracked_files = graph
        .tracked_files(id)
        .map(<[String]>::to_vec)
        .unwrap_or_default();

    let status = DocumentStatus {
        id: id.to_owned(),
        doc_type: doc.frontmatter.doc_type.clone(),
        status: doc.frontmatter.status.clone(),
        criteria,
        tracked_files,
    };

    match fmt {
        OutputFormat::Json => write_json(&status)?,
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            let c = color;
            let doc_type = status.doc_type.as_deref().unwrap_or("document");
            let doc_status = status.status.as_deref().unwrap_or("(none)");
            writeln!(
                out,
                "{} {}",
                c.paint(Token::Header, &format!("# {doc_type}:")),
                c.paint(Token::DocId, &status.id),
            )?;
            writeln!(
                out,
                "{} {}",
                c.paint(Token::Label, "Status:"),
                c.paint(status_token(doc_status), doc_status),
            )?;

            if !status.criteria.is_empty() {
                writeln!(out, "\n{}", c.paint(Token::Label, "Verification targets:"))?;
                for crit in &status.criteria {
                    let (marker, tok) = if crit.covered {
                        ("covered", Token::StatusGood)
                    } else {
                        ("uncovered", Token::StatusBad)
                    };
                    writeln!(
                        out,
                        "  - {}: {}",
                        c.paint(Token::DocId, &crit.id),
                        c.paint(tok, marker),
                    )?;
                    for vb in &crit.verified_by {
                        writeln!(out, "    verified by: {vb}")?;
                    }
                }
            }

            if !status.tracked_files.is_empty() {
                writeln!(out, "\n{}", c.paint(Token::Label, "Tracked files:"))?;
                for tf in &status.tracked_files {
                    writeln!(out, "  - {}", c.paint(Token::Path, tf))?;
                }
            }

            format::hint(
                color,
                "Run `supersigil context <id>` for relationship details.",
            );
        }
    }

    Ok(())
}

#[derive(Default)]
struct TargetCounts {
    total: usize,
    covered: usize,
}

fn count_targets_recursive(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    artifact_graph: &ArtifactGraph<'_>,
    counts: &mut TargetCounts,
) {
    for comp in components {
        if comp.name == CRITERION {
            counts.total += 1;
            if let Some(crit_id) = comp.attributes.get("id")
                && artifact_graph.has_evidence(doc_id, crit_id)
            {
                counts.covered += 1;
            }
        }
        count_targets_recursive(doc_id, &comp.children, artifact_graph, counts);
    }
}

/// Recursively walk the component tree, building a [`TargetStatus`] for each
/// verifiable component found.  `VerifiedBy` children are collected into the
/// target's `verified_by` list as human-readable labels (e.g.
/// `"tag:my_tag"`, `"file-glob:tests/**"`).
fn build_targets_status(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    artifact_graph: &ArtifactGraph<'_>,
) -> Vec<TargetStatus> {
    let mut result = Vec::new();
    collect_targets_status(doc_id, components, artifact_graph, &mut result);
    result
}

fn collect_targets_status(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    artifact_graph: &ArtifactGraph<'_>,
    result: &mut Vec<TargetStatus>,
) {
    for comp in components {
        if comp.name == CRITERION
            && let Some(crit_id) = comp.attributes.get("id")
        {
            let verified_by: Vec<String> = comp
                .children
                .iter()
                .filter(|child| child.name == VERIFIED_BY)
                .map(|child| {
                    let strategy = child
                        .attributes
                        .get("strategy")
                        .map_or("unknown", String::as_str);
                    match strategy {
                        "tag" => {
                            let tag = child.attributes.get("tag").map_or("?", String::as_str);
                            format!("tag:{tag}")
                        }
                        "file-glob" => {
                            let paths = child.attributes.get("paths").map_or("?", String::as_str);
                            format!("file-glob:{paths}")
                        }
                        other => other.to_owned(),
                    }
                })
                .collect();

            let covered = artifact_graph.has_evidence(doc_id, crit_id);
            result.push(TargetStatus {
                id: crit_id.clone(),
                covered,
                verified_by,
            });
        }
        // Recurse into children (e.g. AcceptanceCriteria -> Criterion).
        collect_targets_status(doc_id, &comp.children, artifact_graph, result);
    }
}
