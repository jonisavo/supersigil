//! Optional `build.rs` freshness helpers.
//!
//! Provides utilities for consumer crates to integrate supersigil verification
//! into their build pipeline via `build.rs` scripts, including change detection
//! and incremental re-verification support.

use std::io::Write;
use std::path::{Path, PathBuf};

use supersigil_core::Config;

/// Collect all validation input paths that should trigger revalidation.
///
/// This includes:
/// - The `supersigil.toml` config file itself
/// - Every spec document file loaded from the config's path globs
///
/// The returned paths are absolute.
#[must_use]
pub fn validation_input_paths(
    _config: &Config,
    project_root: &Path,
    spec_files: &[PathBuf],
) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(1 + spec_files.len());
    paths.push(project_root.join("supersigil.toml"));
    paths.extend(spec_files.iter().cloned());
    paths
}

/// Emit `cargo:rerun-if-changed=` lines for all validation inputs.
///
/// Writes to the provided writer (typically `stdout` in a `build.rs`).
///
/// # Errors
///
/// Returns `std::io::Error` if writing to the writer fails.
pub fn emit_rerun_if_changed<W: Write>(
    writer: &mut W,
    config: &Config,
    project_root: &Path,
    spec_files: &[PathBuf],
) -> std::io::Result<()> {
    for path in validation_input_paths(config, project_root, spec_files) {
        writeln!(writer, "cargo:rerun-if-changed={}", path.display())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use supersigil_core::{
        DocumentsConfig, EcosystemConfig, HooksConfig, TestResultsConfig, VerifyConfig,
    };

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
        }
    }

    // -----------------------------------------------------------------------
    // validation_input_paths (req-5-3, req-5-4)
    // -----------------------------------------------------------------------

    #[test]
    fn input_paths_includes_supersigil_toml() {
        let config = minimal_config();
        let root = PathBuf::from("/workspace");
        let spec_files: Vec<PathBuf> = vec![];

        let paths = validation_input_paths(&config, &root, &spec_files);

        assert!(
            paths.contains(&PathBuf::from("/workspace/supersigil.toml")),
            "supersigil.toml must be listed as a validation input"
        );
    }

    #[test]
    fn input_paths_includes_spec_files() {
        let config = minimal_config();
        let root = PathBuf::from("/workspace");
        let spec_files = vec![
            PathBuf::from("/workspace/specs/auth.mdx"),
            PathBuf::from("/workspace/specs/api.mdx"),
        ];

        let paths = validation_input_paths(&config, &root, &spec_files);

        assert!(paths.contains(&PathBuf::from("/workspace/specs/auth.mdx")));
        assert!(paths.contains(&PathBuf::from("/workspace/specs/api.mdx")));
    }

    #[test]
    fn input_paths_combines_toml_and_specs() {
        let config = minimal_config();
        let root = PathBuf::from("/workspace");
        let spec_files = vec![PathBuf::from("/workspace/specs/auth.mdx")];

        let paths = validation_input_paths(&config, &root, &spec_files);

        // Must include both supersigil.toml AND the spec files
        assert!(paths.contains(&PathBuf::from("/workspace/supersigil.toml")));
        assert!(paths.contains(&PathBuf::from("/workspace/specs/auth.mdx")));
        // Should be exactly 2 entries (toml + one spec)
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn input_paths_empty_specs_still_has_toml() {
        let config = minimal_config();
        let root = PathBuf::from("/workspace");
        let spec_files: Vec<PathBuf> = vec![];

        let paths = validation_input_paths(&config, &root, &spec_files);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/workspace/supersigil.toml"));
    }

    // -----------------------------------------------------------------------
    // emit_rerun_if_changed (req-5-4)
    // -----------------------------------------------------------------------

    #[test]
    fn emit_writes_rerun_lines_for_toml() {
        let config = minimal_config();
        let root = PathBuf::from("/workspace");
        let spec_files: Vec<PathBuf> = vec![];
        let mut output = Vec::new();

        emit_rerun_if_changed(&mut output, &config, &root, &spec_files).unwrap();

        let text = String::from_utf8(output).unwrap();
        assert!(
            text.contains("cargo:rerun-if-changed=/workspace/supersigil.toml"),
            "must emit rerun-if-changed for supersigil.toml"
        );
    }

    #[test]
    fn emit_writes_rerun_lines_for_spec_files() {
        let config = minimal_config();
        let root = PathBuf::from("/workspace");
        let spec_files = vec![
            PathBuf::from("/workspace/specs/auth.mdx"),
            PathBuf::from("/workspace/specs/api.mdx"),
        ];
        let mut output = Vec::new();

        emit_rerun_if_changed(&mut output, &config, &root, &spec_files).unwrap();

        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("cargo:rerun-if-changed=/workspace/specs/auth.mdx"));
        assert!(text.contains("cargo:rerun-if-changed=/workspace/specs/api.mdx"));
    }

    #[test]
    fn emit_writes_one_line_per_input() {
        let config = minimal_config();
        let root = PathBuf::from("/workspace");
        let spec_files = vec![PathBuf::from("/workspace/specs/auth.mdx")];
        let mut output = Vec::new();

        emit_rerun_if_changed(&mut output, &config, &root, &spec_files).unwrap();

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
