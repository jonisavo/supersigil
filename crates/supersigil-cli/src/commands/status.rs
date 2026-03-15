use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;
use supersigil_core::{CRITERION, EXAMPLE, VERIFIED_BY};
use supersigil_verify::artifact_graph::ArtifactGraph;

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
    #[serde(skip_serializing_if = "is_zero")]
    targets_example_pending: usize,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires &T"
)]
const fn is_zero(n: &usize) -> bool {
    *n == 0
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
    let (artifact_graph, plugin_findings) =
        plugins::build_evidence(&config, &graph, project_root, None);
    plugins::warn_plugin_findings(&plugin_findings, &color);

    match &args.id {
        None => run_project_wide(&args.format, &graph, &artifact_graph, color),
        Some(id) => run_per_document(id, &args.format, &graph, &artifact_graph, color),
    }
}

fn run_project_wide(
    fmt: &OutputFormat,
    graph: &supersigil_core::DocumentGraph,
    artifact_graph: &ArtifactGraph<'_>,
    color: ColorConfig,
) -> Result<(), CliError> {
    let example_refs = collect_example_verifies_refs(graph);

    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0;
    let mut targets_total = 0;
    let mut targets_covered = 0;
    let mut targets_example_pending = 0;

    for (id, doc) in graph.documents() {
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

        count_targets_recursive(
            id,
            &doc.components,
            artifact_graph,
            &example_refs,
            &mut targets_total,
            &mut targets_covered,
            &mut targets_example_pending,
        );
    }

    let status = ProjectStatus {
        total_documents: total,
        by_type,
        by_status,
        targets_total,
        targets_covered,
        targets_example_pending,
    };

    match fmt {
        OutputFormat::Json => write_json(&status)?,
        OutputFormat::Terminal => write_project_terminal(&status, color)?,
    }

    Ok(())
}

fn write_project_terminal(status: &ProjectStatus, color: ColorConfig) -> Result<(), CliError> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let c = color;
    writeln!(out, "{}", c.paint(Token::Header, "## Project Status"))?;
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
        (status.targets_covered + status.targets_example_pending) as f64
            / status.targets_total as f64
            * 100.0
    } else {
        100.0
    };
    let display_covered = status.targets_covered + status.targets_example_pending;
    writeln!(
        out,
        "{} {}/{} ({pct:.0}%)",
        c.paint(Token::Label, "Verification coverage:"),
        c.paint(Token::Count, &display_covered.to_string()),
        c.paint(Token::Count, &status.targets_total.to_string()),
    )?;

    if status.targets_example_pending > 0 {
        format::hint(
            color,
            &format!(
                "{} criteria covered only by examples (not yet executed). \
                 Run `supersigil verify` to confirm.",
                status.targets_example_pending,
            ),
        );
    }

    let uncovered = status.targets_total.saturating_sub(display_covered);
    if uncovered > 0 {
        format::hint(
            color,
            "Some verification targets are uncovered. Run `supersigil verify` to see details.",
        );
    } else if status.targets_example_pending == 0 {
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

fn count_targets_recursive(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    artifact_graph: &ArtifactGraph<'_>,
    example_refs: &HashSet<String>,
    total: &mut usize,
    covered: &mut usize,
    example_pending: &mut usize,
) {
    for comp in components {
        if comp.name == CRITERION {
            *total += 1;
            if let Some(crit_id) = comp.attributes.get("id") {
                if artifact_graph.has_evidence(doc_id, crit_id) {
                    *covered += 1;
                } else if example_refs.contains(&format!("{doc_id}#{crit_id}")) {
                    *example_pending += 1;
                }
            }
        }
        count_targets_recursive(
            doc_id,
            &comp.children,
            artifact_graph,
            example_refs,
            total,
            covered,
            example_pending,
        );
    }
}

/// Collect all criterion refs declared in `<Example verifies="...">` attributes
/// across all documents, without executing the examples.
fn collect_example_verifies_refs(graph: &supersigil_core::DocumentGraph) -> HashSet<String> {
    let mut refs = HashSet::new();
    for (_, doc) in graph.documents() {
        collect_example_refs_recursive(&doc.components, &mut refs);
    }
    refs
}

fn collect_example_refs_recursive(
    components: &[supersigil_core::ExtractedComponent],
    refs: &mut HashSet<String>,
) {
    for comp in components {
        if comp.name == EXAMPLE
            && let Some(verifies) = comp.attributes.get("verifies")
        {
            for r in verifies.split(',') {
                let trimmed = r.trim();
                if !trimmed.is_empty() {
                    refs.insert(trimmed.to_string());
                }
            }
        }
        collect_example_refs_recursive(&comp.children, refs);
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

            result.push(TargetStatus {
                id: crit_id.clone(),
                covered: artifact_graph.has_evidence(doc_id, crit_id),
                verified_by,
            });
        }
        // Recurse into children (e.g. AcceptanceCriteria -> Criterion).
        collect_targets_status(doc_id, &comp.children, artifact_graph, result);
    }
}
