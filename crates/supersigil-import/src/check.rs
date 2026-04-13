use std::fs;
use std::path::{Path, PathBuf};

use crate::emit::MARKER_PREFIX;
use crate::{AmbiguityBreakdown, AmbiguityKind, ImportError};

/// Result of scanning for unresolved import markers.
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Individual marker locations found.
    pub markers: Vec<MarkerLocation>,
    /// Aggregate breakdown by category.
    pub breakdown: AmbiguityBreakdown,
}

/// A single import marker found in a file.
#[derive(Debug, Clone)]
pub struct MarkerLocation {
    /// Path to the file containing the marker.
    pub file: PathBuf,
    /// 1-based line number.
    pub line: usize,
    /// The marker message text (after the prefix).
    pub message: String,
    /// Categorized kind of this marker.
    pub kind: AmbiguityKind,
}

/// Scan previously-imported files for unresolved TODO markers.
///
/// Recursively walks `dir` for `.md` files and returns every line that
/// contains the `TODO(supersigil-import):` needle, together with an
/// aggregated per-category breakdown.
///
/// # Errors
///
/// Returns `ImportError::SpecsDirNotFound` if `dir` does not exist, or
/// `ImportError::Io` on filesystem errors.
pub fn check_markers(dir: &Path) -> Result<CheckResult, ImportError> {
    if !dir.exists() {
        return Err(ImportError::SpecsDirNotFound {
            path: dir.to_path_buf(),
        });
    }

    let needle = format!("{MARKER_PREFIX}:");
    let files = find_md_files(dir)?;

    let mut markers = Vec::new();
    let mut breakdown = AmbiguityBreakdown::default();

    for file in &files {
        let content = fs::read_to_string(file)?;
        for (idx, line) in content.lines().enumerate() {
            if let Some(pos) = line.find(&needle) {
                let after = &line[pos + needle.len()..];
                // Strip bold-closing `**` from blockquote markers (e.g. `**TODO(…):**`).
                let after = after.strip_prefix("**").unwrap_or(after);
                // Strip trailing `-->` from legacy HTML comment markers.
                let trimmed = after.trim().trim_end_matches("-->").trim_end();
                let message = trimmed.to_string();
                let kind = categorize_marker(&message);
                breakdown.record(kind);
                markers.push(MarkerLocation {
                    file: file.clone(),
                    line: idx + 1,
                    message,
                    kind,
                });
            }
        }
    }

    Ok(CheckResult { markers, breakdown })
}

fn categorize_marker(message: &str) -> AmbiguityKind {
    if message.starts_with("Duplicate ID") {
        AmbiguityKind::DuplicateId
    } else if message.starts_with("No requirements document") {
        AmbiguityKind::MissingContext
    } else if message.starts_with("Could not parse")
        || message.starts_with("Empty reference")
        || message.starts_with("Non-numeric range")
        || message.starts_with("Range has")
    {
        AmbiguityKind::UnparseableRef
    } else if message.contains("optional")
        || message.starts_with("Kiro metadata")
        || message.contains("non-requirement target")
    {
        AmbiguityKind::UnsupportedFeature
    } else {
        // "Could not resolve", "Only X of Y implements", etc.
        AmbiguityKind::UnresolvedRef
    }
}

fn find_md_files(dir: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut files = Vec::new();
    collect_md_files(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_md_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ImportError> {
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        // Use entry.file_type() which does not follow symlinks, avoiding
        // infinite recursion on symlink cycles.
        let ft = entry.file_type()?;
        let path = entry.path();
        if ft.is_dir() {
            collect_md_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_md(dir: &Path, name: &str, content: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn check_finds_no_markers_in_clean_files() {
        let tmp = TempDir::new().unwrap();
        write_md(tmp.path(), "clean.md", "# Clean\n\nNo markers here.\n");
        let result = check_markers(tmp.path()).unwrap();
        assert!(result.markers.is_empty());
        assert_eq!(result.breakdown.total(), 0);
    }

    #[test]
    fn check_finds_blockquote_markers() {
        let tmp = TempDir::new().unwrap();
        write_md(
            tmp.path(),
            "imported.md",
            "# Doc\n\n> **TODO(supersigil-import):** Duplicate ID 'task-1', renamed to 'task-1-2'\n",
        );
        let result = check_markers(tmp.path()).unwrap();
        assert_eq!(result.markers.len(), 1);
        assert_eq!(result.breakdown.duplicate_id, 1);
        assert_eq!(result.markers[0].line, 3);
        assert_eq!(result.markers[0].kind, AmbiguityKind::DuplicateId);
    }

    #[test]
    fn check_finds_legacy_html_comment_markers() {
        let tmp = TempDir::new().unwrap();
        write_md(
            tmp.path(),
            "legacy.md",
            "# Doc\n\n<!-- TODO(supersigil-import): Could not resolve reference 'Requirements 1.2' -->\n",
        );
        let result = check_markers(tmp.path()).unwrap();
        assert_eq!(result.markers.len(), 1);
        assert_eq!(result.breakdown.unresolved_ref, 1);
    }

    #[test]
    fn check_scans_subdirectories() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("feature");
        write_md(
            &sub,
            "feature.req.md",
            "# Req\n\n> **TODO(supersigil-import):** Empty reference string in 'Requirements'\n",
        );
        let result = check_markers(tmp.path()).unwrap();
        assert_eq!(result.markers.len(), 1);
        assert_eq!(result.breakdown.unparseable_ref, 1);
    }

    #[test]
    fn check_categorizes_all_kinds() {
        let tmp = TempDir::new().unwrap();
        let content = "\
> **TODO(supersigil-import):** Duplicate ID 'x', renamed to 'x-2'
> **TODO(supersigil-import):** Could not resolve reference 'Requirements 1.2' to a criterion ID
> **TODO(supersigil-import):** Could not parse reference token 'abc'
> **TODO(supersigil-import):** No requirements document found for this feature
> **TODO(supersigil-import):** This task was marked as optional in Kiro
";
        write_md(tmp.path(), "all-kinds.md", content);
        let result = check_markers(tmp.path()).unwrap();
        assert_eq!(result.markers.len(), 5);
        assert_eq!(result.breakdown.duplicate_id, 1);
        assert_eq!(result.breakdown.unresolved_ref, 1);
        assert_eq!(result.breakdown.unparseable_ref, 1);
        assert_eq!(result.breakdown.missing_context, 1);
        assert_eq!(result.breakdown.unsupported_feature, 1);
    }

    #[test]
    fn check_nonexistent_dir_returns_error() {
        let result = check_markers(Path::new("/nonexistent/path"));
        result.unwrap_err();
    }

    #[test]
    fn categorize_duplicate_id() {
        assert_eq!(
            categorize_marker("Duplicate ID 'x', renamed to 'x-2'"),
            AmbiguityKind::DuplicateId
        );
    }

    #[test]
    fn categorize_unresolved_ref() {
        assert_eq!(
            categorize_marker("Could not resolve reference 'Requirements 1.2' to a criterion ID"),
            AmbiguityKind::UnresolvedRef
        );
        assert_eq!(
            categorize_marker("Could not resolve implements references 'x' for task t"),
            AmbiguityKind::UnresolvedRef
        );
        assert_eq!(
            categorize_marker("Only 1 of 3 implements references resolved for task t"),
            AmbiguityKind::UnresolvedRef
        );
        assert_eq!(
            categorize_marker("Could not resolve Validates references in 'x'"),
            AmbiguityKind::UnresolvedRef
        );
    }

    #[test]
    fn categorize_unparseable_ref() {
        assert_eq!(
            categorize_marker("Could not parse reference token 'abc'"),
            AmbiguityKind::UnparseableRef
        );
        assert_eq!(
            categorize_marker("Empty reference string in 'Requirements'"),
            AmbiguityKind::UnparseableRef
        );
        assert_eq!(
            categorize_marker("Non-numeric range indices in '1.a-1.b', cannot expand"),
            AmbiguityKind::UnparseableRef
        );
        assert_eq!(
            categorize_marker("Range has start > end: '1.5-1.2'"),
            AmbiguityKind::UnparseableRef
        );
        assert_eq!(
            categorize_marker("Range has different requirement numbers: '1' vs '2' in '1.1-2.1'"),
            AmbiguityKind::UnparseableRef
        );
    }

    #[test]
    fn categorize_missing_context() {
        assert_eq!(
            categorize_marker("No requirements document found for this feature"),
            AmbiguityKind::MissingContext
        );
    }

    #[test]
    fn categorize_unsupported_feature() {
        assert_eq!(
            categorize_marker("This task was marked as optional in Kiro"),
            AmbiguityKind::UnsupportedFeature
        );
        assert_eq!(
            categorize_marker("Kiro metadata for task task-1: some note"),
            AmbiguityKind::UnsupportedFeature
        );
        assert_eq!(
            categorize_marker(
                "Validates line references non-requirement target: 'Design Decision 5'"
            ),
            AmbiguityKind::UnsupportedFeature
        );
    }
}
