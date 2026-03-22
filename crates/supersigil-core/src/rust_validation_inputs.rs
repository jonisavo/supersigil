//! Shared resolution of Rust compile-time validation inputs.
//!
//! This logic is shared by Rust-facing integrations that need the complete set
//! of spec files and config paths participating in compile-time validation.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{Config, RustProjectResolutionError, resolve_rust_project};

/// The resolved compile-time inputs for Rust validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustValidationInputs {
    /// The supersigil config file participating in validation.
    pub config_path: PathBuf,
    /// The spec files matched for the active project scope.
    pub spec_files: Vec<PathBuf>,
}

impl RustValidationInputs {
    /// Return every path that should participate in change detection.
    #[must_use]
    pub fn all_paths(&self) -> Vec<PathBuf> {
        std::iter::once(self.config_path.clone())
            .chain(self.spec_files.iter().cloned())
            .collect()
    }
}

/// Errors that can occur while resolving Rust compile-time validation inputs.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RustValidationInputResolutionError {
    /// The config did not provide single-project paths or multi-project entries.
    #[error("rust validation inputs require either `paths` or `projects` in the supersigil config")]
    MissingPathsAndProjects,
    /// Multi-project resolution failed while determining the active project.
    #[error(transparent)]
    ProjectResolution(#[from] RustProjectResolutionError),
}

impl RustValidationInputResolutionError {
    /// Return the wrapped project-resolution error when one is present.
    #[must_use]
    pub fn project_resolution(&self) -> Option<&RustProjectResolutionError> {
        match self {
            Self::ProjectResolution(error) => Some(error),
            Self::MissingPathsAndProjects => None,
        }
    }
}

/// Resolve the compile-time validation inputs for a Rust crate.
///
/// The returned paths are absolute and include:
/// - the workspace `supersigil.toml`
/// - every spec file matched by the active project's spec globs
///
/// In single-project mode, `config.paths` is expanded relative to
/// `project_root`. In multi-project mode, the active project is determined via
/// [`resolve_rust_project`] before expanding that project's `paths`.
///
/// # Errors
///
/// Returns [`RustValidationInputResolutionError`] when the config has no spec
/// path source or when active-project resolution fails in multi-project mode.
pub fn resolve_project_validation_inputs(
    config: &Config,
    manifest_dir: &Path,
    project_root: &Path,
) -> Result<RustValidationInputs, RustValidationInputResolutionError> {
    let globs = resolve_scoped_globs(config, manifest_dir, project_root)?;
    Ok(RustValidationInputs {
        config_path: project_root.join(crate::CONFIG_FILENAME),
        spec_files: crate::expand_globs(globs.iter().map(String::as_str), project_root),
    })
}

/// Resolve validation inputs covering ALL projects in the workspace.
///
/// Unlike [`resolve_project_validation_inputs`], this function does not scope to
/// a single project. It collects spec globs from every project entry (or from
/// `paths` in single-project mode) so the resulting graph contains all
/// workspace criteria. This is used by the proc macro for compile-time
/// validation, where a test in one crate may `#[verifies]` criteria owned by
/// a different project.
///
/// # Errors
///
/// Returns [`RustValidationInputResolutionError::MissingPathsAndProjects`]
/// when the config has neither `paths` nor `projects`.
pub fn resolve_workspace_validation_inputs(
    config: &Config,
    project_root: &Path,
) -> Result<RustValidationInputs, RustValidationInputResolutionError> {
    let globs = all_spec_globs(config)?;
    Ok(RustValidationInputs {
        config_path: project_root.join(crate::CONFIG_FILENAME),
        spec_files: crate::expand_globs(globs.iter().map(String::as_str), project_root),
    })
}

fn resolve_scoped_globs(
    config: &Config,
    manifest_dir: &Path,
    project_root: &Path,
) -> Result<Vec<String>, RustValidationInputResolutionError> {
    if let Some(paths) = &config.paths {
        return Ok(paths.clone());
    }

    let Some(projects) = config.projects.as_ref() else {
        return Err(RustValidationInputResolutionError::MissingPathsAndProjects);
    };

    let project_name = resolve_rust_project(config, manifest_dir, project_root)?
        .expect("multi-project configs always resolve to a project name");
    Ok(projects
        .get(&project_name)
        .expect("resolved project must exist in config")
        .paths
        .clone())
}

fn all_spec_globs(config: &Config) -> Result<Vec<String>, RustValidationInputResolutionError> {
    if let Some(paths) = &config.paths {
        return Ok(paths.clone());
    }

    let Some(projects) = config.projects.as_ref() else {
        return Err(RustValidationInputResolutionError::MissingPathsAndProjects);
    };

    Ok(projects
        .values()
        .flat_map(|p| p.paths.iter().cloned())
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{resolve_project_validation_inputs, resolve_workspace_validation_inputs};
    use crate::{Config, EcosystemConfig, ProjectConfig, RustEcosystemConfig, RustProjectScope};

    fn touch(path: &Path) {
        let parent = path.parent().expect("test paths should have parents");
        fs::create_dir_all(parent).expect("create test directories");
        fs::write(path, "").expect("create test file");
    }

    #[test]
    fn resolve_project_validation_inputs_includes_single_project_specs_and_config() {
        let temp = tempdir().expect("create temp dir");
        let root = temp.path();
        touch(&root.join("supersigil.toml"));
        touch(&root.join("specs/auth/login.md"));
        touch(&root.join("specs/billing/refunds.md"));
        touch(&root.join("docs/ignored.md"));

        let config = Config {
            paths: Some(vec!["specs/**/*.md".to_string()]),
            ..Config::default()
        };
        let manifest_dir = root.join("crates/app");

        let inputs = resolve_project_validation_inputs(&config, &manifest_dir, root).unwrap();

        assert_eq!(
            BTreeSet::from([inputs.config_path])
                .into_iter()
                .chain(inputs.spec_files)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                root.join("supersigil.toml"),
                root.join("specs/auth/login.md"),
                root.join("specs/billing/refunds.md"),
            ]),
        );
    }

    #[test]
    fn resolve_project_validation_inputs_limits_multi_project_inputs_to_active_project() {
        let temp = tempdir().expect("create temp dir");
        let root = temp.path();
        touch(&root.join("supersigil.toml"));
        touch(&root.join("frontend/specs/ui/button.md"));
        touch(&root.join("backend/specs/api/orders.md"));

        let mut projects = HashMap::new();
        projects.insert(
            "frontend".to_string(),
            ProjectConfig {
                paths: vec!["frontend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );
        projects.insert(
            "backend".to_string(),
            ProjectConfig {
                paths: vec!["backend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );

        let config = Config {
            projects: Some(projects),
            ecosystem: EcosystemConfig {
                rust: Some(RustEcosystemConfig {
                    project_scope: vec![RustProjectScope {
                        manifest_dir_prefix: "crates/frontend-app".to_string(),
                        project: "frontend".to_string(),
                    }],
                    ..RustEcosystemConfig::default()
                }),
                ..EcosystemConfig::default()
            },
            ..Config::default()
        };
        let manifest_dir = root.join("crates/frontend-app");

        let inputs = resolve_project_validation_inputs(&config, &manifest_dir, root).unwrap();

        assert_eq!(
            BTreeSet::from([inputs.config_path])
                .into_iter()
                .chain(inputs.spec_files)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                root.join("supersigil.toml"),
                root.join("frontend/specs/ui/button.md"),
            ]),
        );
    }

    #[test]
    fn resolve_workspace_validation_inputs_includes_all_projects() {
        let temp = tempdir().expect("create temp dir");
        let root = temp.path();
        touch(&root.join("supersigil.toml"));
        touch(&root.join("frontend/specs/ui/button.md"));
        touch(&root.join("backend/specs/api/orders.md"));

        let mut projects = HashMap::new();
        projects.insert(
            "frontend".to_string(),
            ProjectConfig {
                paths: vec!["frontend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );
        projects.insert(
            "backend".to_string(),
            ProjectConfig {
                paths: vec!["backend/specs/**/*.md".to_string()],
                tests: Vec::new(),
                isolated: false,
            },
        );

        let config = Config {
            projects: Some(projects),
            ecosystem: EcosystemConfig {
                rust: Some(RustEcosystemConfig {
                    project_scope: vec![RustProjectScope {
                        manifest_dir_prefix: "crates/frontend-app".to_string(),
                        project: "frontend".to_string(),
                    }],
                    ..RustEcosystemConfig::default()
                }),
                ..EcosystemConfig::default()
            },
            ..Config::default()
        };

        // Scoped resolution returns only frontend specs.
        let scoped =
            resolve_project_validation_inputs(&config, &root.join("crates/frontend-app"), root)
                .unwrap();
        assert!(
            !scoped
                .spec_files
                .contains(&root.join("backend/specs/api/orders.md")),
            "scoped inputs must NOT include backend specs"
        );

        // Workspace resolution returns specs from ALL projects.
        let workspace = resolve_workspace_validation_inputs(&config, root).unwrap();
        let all_files: BTreeSet<_> = workspace.spec_files.into_iter().collect();
        assert!(
            all_files.contains(&root.join("frontend/specs/ui/button.md")),
            "workspace inputs must include frontend specs"
        );
        assert!(
            all_files.contains(&root.join("backend/specs/api/orders.md")),
            "workspace inputs must include backend specs"
        );
    }
}
