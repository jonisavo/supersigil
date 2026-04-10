use std::path::{Path, PathBuf};

use crate::{Diagnostic, ImportError};

/// A discovered Kiro spec directory for a single feature.
#[derive(Debug, Clone)]
pub struct KiroSpecDir {
    /// Absolute path to the feature's spec directory.
    pub path: PathBuf,
    /// Name of the feature (directory basename).
    pub feature_name: String,
    /// Whether a `requirements.md` file exists.
    pub has_requirements: bool,
    /// Whether a `design.md` file exists.
    pub has_design: bool,
    /// Whether a `tasks.md` file exists.
    pub has_tasks: bool,
}

/// Discover all Kiro spec directories under the given path.
///
/// Returns discovered dirs sorted alphabetically by feature name (for deterministic output)
/// and diagnostics for skipped dirs.
///
/// # Errors
///
/// Returns `ImportError::SpecsDirNotFound` if the specs directory does not exist.
/// Returns `ImportError::Io` on filesystem errors.
pub fn discover_kiro_specs(
    specs_dir: &Path,
) -> Result<(Vec<KiroSpecDir>, Vec<Diagnostic>), ImportError> {
    let mut discovered = Vec::new();
    let mut diagnostics = Vec::new();

    let read_dir = std::fs::read_dir(specs_dir).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ImportError::SpecsDirNotFound {
                path: specs_dir.to_path_buf(),
            }
        } else {
            ImportError::Io { source: e }
        }
    })?;

    let mut entries: Vec<_> = Vec::new();
    for entry_result in read_dir {
        let entry = entry_result.map_err(|e| ImportError::Io { source: e })?;
        let ft = entry
            .file_type()
            .map_err(|e| ImportError::Io { source: e })?;
        if ft.is_dir() {
            entries.push(entry);
        }
    }
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let feature_name = entry.file_name().to_string_lossy().into_owned();

        let has_requirements = path.join("requirements.md").is_file();
        let has_design = path.join("design.md").is_file();
        let has_tasks = path.join("tasks.md").is_file();

        if has_requirements || has_design || has_tasks {
            discovered.push(KiroSpecDir {
                path,
                feature_name,
                has_requirements,
                has_design,
                has_tasks,
            });
        } else {
            diagnostics.push(Diagnostic::SkippedDir {
                path,
                reason: "no requirements.md, design.md, or tasks.md found".into(),
            });
        }
    }

    Ok((discovered, diagnostics))
}
