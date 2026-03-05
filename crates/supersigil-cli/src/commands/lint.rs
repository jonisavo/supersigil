use std::io::{self, Write};
use std::path::Path;

use crate::error::CliError;
use crate::loader;

/// Run per-file structural lint checks.
/// Returns `Ok(true)` if clean, `Ok(false)` if errors found.
///
/// # Errors
///
/// Returns `CliError` if configuration loading or file discovery fails.
pub fn run(config_path: &Path) -> Result<bool, CliError> {
    let parse_result = loader::parse_all_with_stats(config_path)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if parse_result.errors.is_empty() {
        writeln!(
            out,
            "{} files checked, no errors",
            parse_result.files_checked
        )?;
        Ok(true)
    } else {
        for err in &parse_result.errors {
            writeln!(out, "error: {err}")?;
        }
        writeln!(
            out,
            "\n{} files checked, {} error(s)",
            parse_result.files_checked,
            parse_result.errors.len()
        )?;
        Ok(false)
    }
}
