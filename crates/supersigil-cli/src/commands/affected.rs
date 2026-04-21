use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use crate::commands::AffectedArgs;
use crate::error::CliError;
use crate::format::{self, ColorConfig, OutputFormat, Token, write_json};
use crate::loader;

#[derive(Serialize)]
struct AffectedOutput<'a> {
    documents: &'a [supersigil_verify::AffectedDocument],
}

pub(crate) fn format_terminal_output(
    affected: &[supersigil_verify::AffectedDocument],
    color: ColorConfig,
) -> String {
    let mut out = String::new();

    if affected.is_empty() {
        return out;
    }

    let c = color;
    for doc in affected {
        let path_str = path_to_string(&doc.path);
        let _ = writeln!(
            out,
            "{} ({})",
            c.paint(Token::DocId, &doc.id),
            c.paint(Token::Path, &path_str),
        );

        if let Some(via) = &doc.transitive_from {
            let _ = writeln!(
                out,
                "  transitively affected via {}",
                c.paint(Token::DocId, via),
            );
        } else {
            for glob in &doc.matched_globs {
                let _ = writeln!(out, "  glob: {}", c.paint(Token::Hint, glob));
            }
            for file in &doc.changed_files {
                let file_str = path_to_string(file);
                let _ = writeln!(out, "  changed: {}", c.paint(Token::Path, &file_str));
            }
        }
    }

    let total = affected.len();
    let transitive_count = affected
        .iter()
        .filter(|d| d.transitive_from.is_some())
        .count();
    let direct_count = total - transitive_count;

    out.push('\n');
    if transitive_count > 0 {
        let _ = writeln!(
            out,
            "{} documents affected ({} direct, {} transitive)",
            c.paint(Token::Count, &total.to_string()),
            direct_count,
            transitive_count,
        );
    } else {
        let _ = writeln!(
            out,
            "{} documents affected",
            c.paint(Token::Count, &total.to_string()),
        );
    }

    out
}

fn path_to_string(path: &Path) -> String {
    path.display().to_string()
}

/// Run the `affected` command: find documents affected by file changes.
///
/// # Errors
///
/// Returns `CliError` if loading or git operations fail.
pub fn run(args: &AffectedArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);

    let affected = supersigil_verify::affected::affected(
        &graph,
        project_root,
        &args.since,
        args.committed_only,
        args.merge_base,
    )
    .map_err(supersigil_verify::VerifyError::from)?;

    match args.format {
        OutputFormat::Json => {
            write_json(&AffectedOutput {
                documents: &affected,
            })?;
        }
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();

            if affected.is_empty() {
                writeln!(
                    out,
                    "No documents affected by changes since `{}`.",
                    args.since
                )?;
                format::hint(
                    color,
                    "Try a different --since ref, or check your TrackedFiles globs.",
                );
            } else {
                write!(out, "{}", format_terminal_output(&affected, color))?;
            }
        }
    }

    Ok(())
}
