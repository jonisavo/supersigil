use std::io::{self, Write};
use std::path::Path;

use crate::error::CliError;
use crate::format::{self, ColorConfig, Token};
use crate::loader;

/// Run per-file structural lint checks.
/// Returns `Ok(true)` if clean, `Ok(false)` if errors found.
///
/// # Errors
///
/// Returns `CliError` if configuration loading or file discovery fails.
pub fn run(config_path: &Path, color: ColorConfig) -> Result<bool, CliError> {
    let parse_result = loader::parse_all_with_stats(config_path)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if parse_result.errors.is_empty() {
        if parse_result.documents.is_empty() {
            writeln!(
                out,
                "{} no documents found matching configured paths",
                color.paint(Token::Warning, "warning:"),
            )?;
            format::hint(
                color,
                "Run `supersigil new requirements <name>` to create a spec document, or check that existing files have valid `supersigil:` frontmatter.",
            );
        } else {
            writeln!(
                out,
                "{} {} files checked, no errors",
                color.ok(),
                color.paint(Token::Count, &parse_result.files_checked.to_string()),
            )?;
            format::hint(
                color,
                "All clean. Run `supersigil verify` to check cross-document rules.",
            );
        }
        Ok(true)
    } else {
        for err in &parse_result.errors {
            writeln!(out, "{} {err}", color.paint(Token::Error, "error:"))?;
        }
        writeln!(
            out,
            "\n{} files checked, {} error(s)",
            color.paint(Token::Count, &parse_result.files_checked.to_string()),
            color.paint(Token::Count, &parse_result.errors.len().to_string()),
        )?;
        Ok(false)
    }
}
