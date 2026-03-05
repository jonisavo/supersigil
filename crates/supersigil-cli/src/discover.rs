use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Resolve glob patterns relative to `base_dir`, returning matched file paths.
///
/// Each pattern in `globs` is joined with `base_dir` before expansion.
/// Duplicate paths (from overlapping globs) are deduplicated.
///
/// # Errors
///
/// Returns `CliError::Io` if a glob pattern is invalid or a matched entry
/// cannot be read.
pub fn discover_spec_files<I, S>(
    globs: I,
    base_dir: &Path,
) -> Result<Vec<PathBuf>, crate::error::CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut paths = BTreeSet::new();

    for pattern in globs {
        let full_pattern = base_dir.join(pattern.as_ref());
        let pattern_str = full_pattern.to_string_lossy();

        let entries = glob::glob(pattern_str.as_ref())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;

        for entry in entries {
            let path = entry.map_err(|e| std::io::Error::other(e.to_string()))?;
            paths.insert(path);
        }
    }

    Ok(paths.into_iter().collect())
}
