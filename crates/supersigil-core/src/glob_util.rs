//! Shared glob expansion utilities.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Expand a single glob pattern relative to `base_dir`.
///
/// Silently skips invalid patterns and unreadable entries.
#[must_use]
pub fn expand_glob(pattern: &str, base_dir: &Path) -> Vec<PathBuf> {
    let full = base_dir.join(pattern).to_string_lossy().to_string();
    let mut files = Vec::new();
    if let Ok(entries) = glob::glob(&full) {
        for entry in entries.flatten() {
            files.push(entry);
        }
    }
    files
}

/// Expand multiple glob patterns relative to `base_dir`, deduplicating results.
///
/// Uses a [`BTreeSet`] internally so the output is sorted and deduplicated.
/// Silently skips invalid patterns and unreadable entries.
pub fn expand_globs<'a>(
    patterns: impl IntoIterator<Item = &'a str>,
    base_dir: &Path,
) -> Vec<PathBuf> {
    let paths: BTreeSet<PathBuf> = patterns
        .into_iter()
        .flat_map(|p| expand_glob(p, base_dir))
        .collect();
    paths.into_iter().collect()
}

/// Expand multiple glob patterns relative to `base_dir`, returning errors for
/// invalid patterns or unreadable matches.
///
/// Uses a [`BTreeSet`] internally so the output is sorted and deduplicated.
///
/// # Errors
///
/// Returns [`std::io::Error`] when a glob pattern is invalid or when a matched
/// entry cannot be read from the filesystem.
pub fn expand_globs_checked<'a>(
    patterns: impl IntoIterator<Item = &'a str>,
    base_dir: &Path,
) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut paths = BTreeSet::new();

    for pattern in patterns {
        let full = base_dir.join(pattern).to_string_lossy().to_string();
        let entries = glob::glob(&full)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error.msg))?;
        for entry in entries {
            let path = entry.map_err(|error| std::io::Error::other(error.to_string()))?;
            paths.insert(path);
        }
    }

    Ok(paths.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn expand_glob_matches_files() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "").unwrap();
        fs::write(dir.path().join("src/b.rs"), "").unwrap();
        fs::write(dir.path().join("src/c.txt"), "").unwrap();

        let result = expand_glob("src/**/*.rs", dir.path());
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|p| p.extension().unwrap() == "rs"));
    }

    #[test]
    fn expand_glob_no_matches_returns_empty() {
        let dir = tempdir().unwrap();
        let result = expand_glob("nonexistent/**/*.rs", dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn expand_glob_invalid_pattern_returns_empty() {
        let dir = tempdir().unwrap();
        let result = expand_glob("[invalid", dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn expand_globs_deduplicates_overlapping_patterns() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("specs/auth")).unwrap();
        fs::write(dir.path().join("specs/auth/login.mdx"), "").unwrap();
        fs::write(dir.path().join("specs/auth/signup.mdx"), "").unwrap();

        let result = expand_globs(["specs/**/*.mdx", "specs/auth/*.mdx"], dir.path());
        assert_eq!(result.len(), 2, "overlapping globs should be deduplicated");
    }

    #[test]
    fn expand_globs_combines_multiple_patterns() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a")).unwrap();
        fs::create_dir_all(dir.path().join("b")).unwrap();
        fs::write(dir.path().join("a/one.txt"), "").unwrap();
        fs::write(dir.path().join("b/two.txt"), "").unwrap();

        let result = expand_globs(["a/*.txt", "b/*.txt"], dir.path());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn expand_globs_output_is_sorted() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("specs")).unwrap();
        fs::write(dir.path().join("specs/c.mdx"), "").unwrap();
        fs::write(dir.path().join("specs/a.mdx"), "").unwrap();
        fs::write(dir.path().join("specs/b.mdx"), "").unwrap();

        let result = expand_globs(["specs/*.mdx"], dir.path());
        assert!(result.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn expand_globs_empty_patterns_returns_empty() {
        let dir = tempdir().unwrap();
        let result = expand_globs(std::iter::empty(), dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn expand_globs_checked_invalid_pattern_returns_error() {
        let dir = tempdir().unwrap();

        let err = expand_globs_checked(["[invalid"], dir.path()).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
