#![allow(
    dead_code,
    reason = "shared integration-test helpers are not used by every test crate"
)]

use std::path::{Path, PathBuf};

use supersigil_import::ImportConfig;

/// Workspace root relative to the crate's manifest directory.
#[must_use]
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Build an `ImportConfig` pointing at the given specs dir with no prefix.
#[must_use]
pub fn config_for(specs_dir: &Path, output_dir: &Path) -> ImportConfig {
    ImportConfig {
        kiro_specs_dir: specs_dir.to_path_buf(),
        output_dir: output_dir.to_path_buf(),
        id_prefix: None,
        force: false,
    }
}

/// Write a Kiro spec directory with the given files under `specs_dir/feature_name/`.
#[allow(
    clippy::missing_panics_doc,
    reason = "test helper intentionally panics on setup failures"
)]
pub fn write_kiro_spec(
    specs_dir: &Path,
    feature_name: &str,
    requirements_md: Option<&str>,
    design_md: Option<&str>,
    tasks_md: Option<&str>,
) {
    let feature_dir = specs_dir.join(feature_name);
    std::fs::create_dir_all(&feature_dir).unwrap();
    if let Some(content) = requirements_md {
        std::fs::write(feature_dir.join("requirements.md"), content).unwrap();
    }
    if let Some(content) = design_md {
        std::fs::write(feature_dir.join("design.md"), content).unwrap();
    }
    if let Some(content) = tasks_md {
        std::fs::write(feature_dir.join("tasks.md"), content).unwrap();
    }
}
