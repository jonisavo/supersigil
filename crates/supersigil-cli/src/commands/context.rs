use std::io::{self, Write};
use std::path::Path;

use supersigil_core::ContextOutput;

use crate::commands::ContextArgs;
use crate::error::CliError;
use crate::format::{
    self, ColorConfig, OutputFormat, Token, status_token, write_json, write_tasks,
};
use crate::loader;

/// Run the `context` command: show structured view of a document.
///
/// # Errors
///
/// Returns `CliError` if the graph cannot be loaded, the document is not
/// found, or output fails.
pub fn run(args: &ContextArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;
    let ctx = match graph.context(&args.id) {
        Ok(ctx) => ctx,
        Err(e) => {
            format::hint(color, "Run `supersigil ls` to see available document IDs.");
            return Err(e.into());
        }
    };

    match args.format {
        OutputFormat::Json => {
            let mut ctx = ctx;
            if args.detail == format::Detail::Compact {
                ctx.document.components.clear();
            }
            write_json(&ctx)?;
        }
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            write_context_terminal(&mut out, &ctx, color)?;
        }
    }

    Ok(())
}

/// Write the context output in terminal format.
fn write_context_terminal(
    out: &mut impl Write,
    ctx: &ContextOutput,
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
            let body = crit.body_text.as_deref().unwrap_or("(no description)");
            writeln!(out, "- {}: {body}", c.paint(Token::DocId, &crit.id))?;
            for vref in &crit.referenced_by {
                let vstatus = vref.status.as_deref().unwrap_or("?");
                writeln!(
                    out,
                    "  -> Referenced by: {} ({vstatus})",
                    c.paint(Token::DocId, &vref.doc_id),
                )?;
            }
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
