use std::io::{self, Write};
use std::path::Path;

use crate::commands::AffectedArgs;
use crate::error::CliError;
use crate::format::{self, ColorConfig, OutputFormat, Token, write_json};
use crate::loader;

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
        OutputFormat::Json => write_json(&affected)?,
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
                let c = color;
                for doc in &affected {
                    let path_str = doc.path.display().to_string();
                    writeln!(
                        out,
                        "{} ({})",
                        c.paint(Token::DocId, &doc.id),
                        c.paint(Token::Path, &path_str),
                    )?;
                    for glob in &doc.matched_globs {
                        writeln!(out, "  glob: {}", c.paint(Token::Hint, glob))?;
                    }
                    for file in &doc.changed_files {
                        let file_str = file.display().to_string();
                        writeln!(out, "  changed: {}", c.paint(Token::Path, &file_str))?;
                    }
                }
            }
        }
    }

    Ok(())
}
