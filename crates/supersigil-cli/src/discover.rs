use std::path::{Path, PathBuf};

/// Resolve glob patterns relative to `base_dir`, returning matched file paths.
///
/// Each pattern in `globs` is joined with `base_dir` before expansion.
/// Duplicate paths (from overlapping globs) are deduplicated.
///
/// # Errors
///
/// Returns [`crate::error::CliError::Io`] if a glob pattern is invalid or a
/// matched entry cannot be read.
pub fn discover_spec_files<I, S>(
    globs: I,
    base_dir: &Path,
) -> Result<Vec<PathBuf>, crate::error::CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let patterns: Vec<String> = globs
        .into_iter()
        .map(|glob| glob.as_ref().to_owned())
        .collect();
    supersigil_core::expand_globs_checked(patterns.iter().map(String::as_str), base_dir)
        .map_err(Into::into)
}
