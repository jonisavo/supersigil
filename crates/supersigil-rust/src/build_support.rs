//! Optional `build.rs` freshness helpers.
//!
//! Provides utilities for consumer crates to integrate supersigil verification
//! into their build pipeline via `build.rs` scripts, including change detection
//! and incremental re-verification support.

use std::io::Write;
use std::path::{Path, PathBuf};

use supersigil_core::{Config, RustValidationInputResolutionError, resolve_rust_validation_inputs};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildSupportError {
    #[error(transparent)]
    InputResolution(#[from] RustValidationInputResolutionError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Collect all validation input paths that should trigger revalidation.
///
/// This includes the shared Rust compile-time validation input set:
/// - The `supersigil.toml` config file itself
/// - Every spec document selected for the current Rust manifest scope
///
/// The returned paths are absolute.
///
/// # Errors
///
/// Returns [`RustValidationInputResolutionError`] when multi-project
/// resolution cannot identify a unique project for the given manifest
/// directory.
pub fn validation_input_paths(
    config: &Config,
    project_root: &Path,
    manifest_dir: &Path,
) -> Result<Vec<PathBuf>, RustValidationInputResolutionError> {
    Ok(resolve_rust_validation_inputs(config, manifest_dir, project_root)?.all_paths())
}

/// Emit `cargo:rerun-if-changed=` lines for all validation inputs.
///
/// Writes to the provided writer (typically `stdout` in a `build.rs`).
///
/// # Errors
///
/// Returns [`BuildSupportError`] if validation inputs cannot be resolved or
/// if writing to the writer fails.
pub fn emit_rerun_if_changed<W: Write>(
    writer: &mut W,
    config: &Config,
    project_root: &Path,
    manifest_dir: &Path,
) -> Result<(), BuildSupportError> {
    for path in validation_input_paths(config, project_root, manifest_dir)? {
        writeln!(writer, "cargo:rerun-if-changed={}", path.display())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use supersigil_core::{
        DocumentsConfig, EcosystemConfig, ExamplesConfig, HooksConfig, SkillsConfig,
        TestResultsConfig, VerifyConfig,
    };
    use tempfile::tempdir;

    use super::*;

    /// Helper: build a minimal single-project config.
    fn minimal_config() -> Config {
        Config {
            paths: Some(vec!["specs/**/*.mdx".to_string()]),
            tests: None,
            projects: None,
            id_pattern: None,
            documents: DocumentsConfig::default(),
            components: HashMap::new(),
            verify: VerifyConfig::default(),
            ecosystem: EcosystemConfig::default(),
            hooks: HooksConfig::default(),
            test_results: TestResultsConfig::default(),
            examples: ExamplesConfig::default(),
            skills: SkillsConfig::default(),
        }
    }

    fn create_project(spec_files: &[&str]) -> (tempfile::TempDir, Config, PathBuf, PathBuf) {
        let temp = tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(
            root.join("supersigil.toml"),
            "paths = [\"specs/**/*.mdx\"]\n",
        )
        .unwrap();
        for relative in spec_files {
            let path = root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, "---\nsupersigil:\n  id: test/doc\n---\n").unwrap();
        }
        let manifest_dir = root.join("crates/app");
        fs::create_dir_all(&manifest_dir).unwrap();
        (temp, minimal_config(), root, manifest_dir)
    }

    // -----------------------------------------------------------------------
    // validation_input_paths (req-5-3, req-5-4)
    // -----------------------------------------------------------------------

    #[test]
    fn input_paths_includes_supersigil_toml() {
        let (_temp, config, root, manifest_dir) = create_project(&[]);

        let paths = validation_input_paths(&config, &root, &manifest_dir).unwrap();

        assert!(
            paths.contains(&root.join("supersigil.toml")),
            "supersigil.toml must be listed as a validation input"
        );
    }

    #[test]
    fn input_paths_includes_spec_files() {
        let (_temp, config, root, manifest_dir) =
            create_project(&["specs/auth.mdx", "specs/api.mdx"]);

        let paths = validation_input_paths(&config, &root, &manifest_dir).unwrap();

        assert!(paths.contains(&root.join("specs/auth.mdx")));
        assert!(paths.contains(&root.join("specs/api.mdx")));
    }

    #[test]
    fn input_paths_combines_toml_and_specs() {
        let (_temp, config, root, manifest_dir) = create_project(&["specs/auth.mdx"]);

        let paths = validation_input_paths(&config, &root, &manifest_dir).unwrap();

        // Must include both supersigil.toml AND the spec files
        assert!(paths.contains(&root.join("supersigil.toml")));
        assert!(paths.contains(&root.join("specs/auth.mdx")));
        // Should be exactly 2 entries (toml + one spec)
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn input_paths_empty_specs_still_has_toml() {
        let (_temp, config, root, manifest_dir) = create_project(&[]);

        let paths = validation_input_paths(&config, &root, &manifest_dir).unwrap();

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], root.join("supersigil.toml"));
    }

    // -----------------------------------------------------------------------
    // emit_rerun_if_changed (req-5-4)
    // -----------------------------------------------------------------------

    #[test]
    fn emit_writes_rerun_lines_for_toml() {
        let (_temp, config, root, manifest_dir) = create_project(&[]);
        let mut output = Vec::new();

        emit_rerun_if_changed(&mut output, &config, &root, &manifest_dir).unwrap();

        let text = String::from_utf8(output).unwrap();
        assert!(
            text.contains(&format!(
                "cargo:rerun-if-changed={}",
                root.join("supersigil.toml").display()
            )),
            "must emit rerun-if-changed for supersigil.toml"
        );
    }

    #[test]
    fn emit_writes_rerun_lines_for_spec_files() {
        let (_temp, config, root, manifest_dir) =
            create_project(&["specs/auth.mdx", "specs/api.mdx"]);
        let mut output = Vec::new();

        emit_rerun_if_changed(&mut output, &config, &root, &manifest_dir).unwrap();

        let text = String::from_utf8(output).unwrap();
        assert!(text.contains(&format!(
            "cargo:rerun-if-changed={}",
            root.join("specs/auth.mdx").display()
        )));
        assert!(text.contains(&format!(
            "cargo:rerun-if-changed={}",
            root.join("specs/api.mdx").display()
        )));
    }

    #[test]
    fn emit_writes_one_line_per_input() {
        let (_temp, config, root, manifest_dir) = create_project(&["specs/auth.mdx"]);
        let mut output = Vec::new();

        emit_rerun_if_changed(&mut output, &config, &root, &manifest_dir).unwrap();

        let text = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        // Should have exactly 2 lines: one for supersigil.toml, one for the spec
        assert_eq!(lines.len(), 2);
        assert!(
            lines
                .iter()
                .all(|line| line.starts_with("cargo:rerun-if-changed="))
        );
    }
}
