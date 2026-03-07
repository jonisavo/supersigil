use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use crate::commands::StatusArgs;
use crate::error::CliError;
use crate::format::{self, ColorConfig, OutputFormat, Token, status_token, write_json};
use crate::loader;

#[derive(Debug, Serialize)]
struct ProjectStatus {
    total_documents: usize,
    by_type: BTreeMap<String, usize>,
    by_status: BTreeMap<String, usize>,
    criteria_total: usize,
    criteria_covered: usize,
}

#[derive(Debug, Serialize)]
struct DocumentStatus {
    id: String,
    doc_type: Option<String>,
    status: Option<String>,
    criteria: Vec<CriterionStatus>,
    tracked_files: Vec<String>,
    verified_by: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CriterionStatus {
    id: String,
    covered: bool,
}

/// Run the `status` command.
///
/// # Errors
///
/// Returns `CliError` if loading fails.
pub fn run(args: &StatusArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;

    match &args.id {
        None => run_project_wide(&args.format, &graph, color),
        Some(id) => run_per_document(id, &args.format, &graph, color),
    }
}

fn run_project_wide(
    fmt: &OutputFormat,
    graph: &supersigil_core::DocumentGraph,
    color: ColorConfig,
) -> Result<(), CliError> {
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0;
    let mut criteria_total = 0;
    let mut criteria_covered = 0;

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

        count_criteria_recursive(
            graph,
            id,
            &doc.components,
            &mut criteria_total,
            &mut criteria_covered,
        );
    }

    let status = ProjectStatus {
        total_documents: total,
        by_type,
        by_status,
        criteria_total,
        criteria_covered,
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
        reason = "criteria counts are small enough for f64"
    )]
    let pct = if status.criteria_total > 0 {
        status.criteria_covered as f64 / status.criteria_total as f64 * 100.0
    } else {
        100.0
    };
    writeln!(
        out,
        "{} {}/{} ({pct:.0}%)",
        c.paint(Token::Label, "Criteria coverage:"),
        c.paint(Token::Count, &status.criteria_covered.to_string()),
        c.paint(Token::Count, &status.criteria_total.to_string()),
    )?;

    if status.criteria_covered < status.criteria_total {
        format::hint(
            color,
            "Some criteria are uncovered. Run `supersigil verify` to see details.",
        );
    } else {
        format::hint(
            color,
            "All criteria covered. Run `supersigil verify` for full verification.",
        );
    }

    Ok(())
}

fn run_per_document(
    id: &str,
    fmt: &OutputFormat,
    graph: &supersigil_core::DocumentGraph,
    color: ColorConfig,
) -> Result<(), CliError> {
    let ctx = graph.context(id)?;
    let doc = &ctx.document;

    let criteria: Vec<CriterionStatus> = ctx
        .criteria
        .iter()
        .map(|c| CriterionStatus {
            id: c.id.clone(),
            covered: !c.validated_by.is_empty(),
        })
        .collect();

    let tracked_files = graph
        .tracked_files(id)
        .map(<[String]>::to_vec)
        .unwrap_or_default();

    // Extract VerifiedBy tags from components
    let verified_by: Vec<String> = doc
        .components
        .iter()
        .filter(|c| c.name == "VerifiedBy")
        .filter_map(|c| c.attributes.get("tag").cloned())
        .collect();

    let status = DocumentStatus {
        id: id.to_owned(),
        doc_type: doc.frontmatter.doc_type.clone(),
        status: doc.frontmatter.status.clone(),
        criteria,
        tracked_files,
        verified_by,
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
                writeln!(out, "\n{}", c.paint(Token::Label, "Criteria:"))?;
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
                }
            }

            if !status.tracked_files.is_empty() {
                writeln!(out, "\n{}", c.paint(Token::Label, "Tracked files:"))?;
                for tf in &status.tracked_files {
                    writeln!(out, "  - {}", c.paint(Token::Path, tf))?;
                }
            }

            if !status.verified_by.is_empty() {
                writeln!(out, "\n{}", c.paint(Token::Label, "Verified by:"))?;
                for tag in &status.verified_by {
                    writeln!(out, "  - {tag}")?;
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

fn count_criteria_recursive(
    graph: &supersigil_core::DocumentGraph,
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    total: &mut usize,
    covered: &mut usize,
) {
    for comp in components {
        if comp.name == "Criterion" {
            *total += 1;
            if let Some(crit_id) = comp.attributes.get("id")
                && !graph.validates(doc_id, Some(crit_id)).is_empty()
            {
                *covered += 1;
            }
        }
        count_criteria_recursive(graph, doc_id, &comp.children, total, covered);
    }
}
