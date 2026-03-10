//! Shared Rust multi-project resolution helpers.
//!
//! This logic is used by both the runtime Rust plugin and the proc macro so
//! that explicit `ecosystem.rust.project_scope` mappings and path-based
//! inference stay behaviorally aligned.

use std::path::{Path, PathBuf};

use crate::{Config, ProjectConfig, RustEcosystemConfig, RustProjectScope};

/// Errors that can occur while resolving the active Supersigil project for a
/// Rust crate.
#[derive(Debug, PartialEq, Eq)]
pub enum RustProjectResolutionError {
    /// Multi-project mode with explicit `project_scope`: no prefix matched.
    NoMatchingScope {
        manifest_dir: PathBuf,
        relative_manifest_dir: PathBuf,
    },
    /// Explicit `project_scope` resolved to a project name not present in
    /// `[projects]`.
    UnknownProject { project: String },
    /// Path-based inference found zero or multiple project candidates.
    AmbiguousProject {
        manifest_dir: PathBuf,
        relative_manifest_dir: PathBuf,
        candidates: Vec<String>,
    },
}

impl std::fmt::Display for RustProjectResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoMatchingScope {
                manifest_dir,
                relative_manifest_dir,
            } => write!(
                f,
                "no project_scope prefix matched manifest dir '{}' (relative: '{}')",
                manifest_dir.display(),
                relative_manifest_dir.display()
            ),
            Self::UnknownProject { project } => {
                write!(f, "project_scope maps to undefined project '{project}'")
            }
            Self::AmbiguousProject {
                manifest_dir,
                relative_manifest_dir,
                candidates,
            } if candidates.is_empty() => write!(
                f,
                "no project matched manifest dir '{}' (relative: '{}')",
                manifest_dir.display(),
                relative_manifest_dir.display()
            ),
            Self::AmbiguousProject {
                manifest_dir,
                relative_manifest_dir,
                candidates,
            } => write!(
                f,
                "ambiguous project for manifest dir '{}' (relative: '{}'): candidates {:?}",
                manifest_dir.display(),
                relative_manifest_dir.display(),
                candidates
            ),
        }
    }
}

impl std::error::Error for RustProjectResolutionError {}

/// Resolve the active Supersigil project for a Rust crate.
///
/// Returns `Ok(None)` in single-project mode and `Ok(Some(project_name))` in
/// multi-project mode.
///
/// # Errors
///
/// Returns [`RustProjectResolutionError`] when explicit scope matching finds
/// no match, when an explicit mapping references an undefined project, or
/// when path-based inference finds zero or multiple candidate projects.
pub fn resolve_rust_project(
    config: &Config,
    manifest_dir: &Path,
    project_root: &Path,
) -> Result<Option<String>, RustProjectResolutionError> {
    let Some(projects) = config.projects.as_ref() else {
        return Ok(None);
    };

    let relative_manifest_dir = manifest_dir
        .strip_prefix(project_root)
        .unwrap_or(manifest_dir)
        .to_path_buf();

    if let Some(rust_config) = &config.ecosystem.rust
        && !rust_config.project_scope.is_empty()
    {
        return resolve_explicit_scope(projects, rust_config, manifest_dir, &relative_manifest_dir)
            .map(Some);
    }

    resolve_by_path_inference(projects, manifest_dir, &relative_manifest_dir).map(Some)
}

/// Match a manifest directory against explicit `project_scope` entries and
/// return the project name from the longest matching prefix.
#[must_use]
pub fn match_rust_project_scope(
    scopes: &[RustProjectScope],
    manifest_dir: &Path,
) -> Option<String> {
    scopes
        .iter()
        .filter(|scope| manifest_dir.starts_with(&scope.manifest_dir_prefix))
        .max_by_key(|scope| scope.manifest_dir_prefix.len())
        .map(|scope| scope.project.clone())
}

fn resolve_explicit_scope(
    projects: &std::collections::HashMap<String, ProjectConfig>,
    rust_config: &RustEcosystemConfig,
    manifest_dir: &Path,
    relative_manifest_dir: &Path,
) -> Result<String, RustProjectResolutionError> {
    match match_rust_project_scope(&rust_config.project_scope, relative_manifest_dir) {
        Some(project) if projects.contains_key(&project) => Ok(project),
        Some(project) => Err(RustProjectResolutionError::UnknownProject { project }),
        None => Err(RustProjectResolutionError::NoMatchingScope {
            manifest_dir: manifest_dir.to_path_buf(),
            relative_manifest_dir: relative_manifest_dir.to_path_buf(),
        }),
    }
}

fn resolve_by_path_inference(
    projects: &std::collections::HashMap<String, ProjectConfig>,
    manifest_dir: &Path,
    relative_manifest_dir: &Path,
) -> Result<String, RustProjectResolutionError> {
    let mut candidates: Vec<String> = projects
        .keys()
        .filter(|name| {
            relative_manifest_dir
                .components()
                .any(|component| component.as_os_str() == name.as_str())
        })
        .cloned()
        .collect();
    candidates.sort();

    match candidates.len() {
        1 => Ok(candidates.pop().expect("length checked")),
        _ => Err(RustProjectResolutionError::AmbiguousProject {
            manifest_dir: manifest_dir.to_path_buf(),
            relative_manifest_dir: relative_manifest_dir.to_path_buf(),
            candidates,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_project_config() -> Config {
        Config {
            paths: Some(vec!["specs/**/*.mdx".to_string()]),
            ..Config::default()
        }
    }

    fn multi_project_config(project_names: &[&str]) -> Config {
        let projects = project_names
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    ProjectConfig {
                        paths: vec![format!("{name}/specs/**/*.mdx")],
                        tests: vec![],
                        isolated: false,
                    },
                )
            })
            .collect();
        Config {
            projects: Some(projects),
            ..Config::default()
        }
    }

    fn with_rust_scopes(mut config: Config, scopes: Vec<RustProjectScope>) -> Config {
        config.ecosystem.rust = Some(RustEcosystemConfig {
            project_scope: scopes,
            ..Default::default()
        });
        config
    }

    #[test]
    fn resolve_rust_project_returns_none_in_single_project_mode() {
        let config = single_project_config();
        let root = PathBuf::from("/workspace");
        let manifest_dir = PathBuf::from("/workspace/crates/my-crate");

        let project = resolve_rust_project(&config, &manifest_dir, &root).unwrap();

        assert_eq!(project, None);
    }

    #[test]
    fn resolve_rust_project_prefers_longest_explicit_scope_prefix() {
        let config = with_rust_scopes(
            multi_project_config(&["alpha", "beta"]),
            vec![
                RustProjectScope {
                    manifest_dir_prefix: "crates/alpha".to_string(),
                    project: "alpha".to_string(),
                },
                RustProjectScope {
                    manifest_dir_prefix: "crates/alpha/sub".to_string(),
                    project: "beta".to_string(),
                },
            ],
        );
        let root = PathBuf::from("/workspace");
        let manifest_dir = PathBuf::from("/workspace/crates/alpha/sub/deep");

        let project = resolve_rust_project(&config, &manifest_dir, &root).unwrap();

        assert_eq!(project, Some("beta".to_string()));
    }

    #[test]
    fn resolve_rust_project_errors_for_unknown_explicit_scope_target() {
        let config = with_rust_scopes(
            multi_project_config(&["alpha", "beta"]),
            vec![RustProjectScope {
                manifest_dir_prefix: "crates/alpha".to_string(),
                project: "gamma".to_string(),
            }],
        );
        let root = PathBuf::from("/workspace");
        let manifest_dir = PathBuf::from("/workspace/crates/alpha/my-crate");

        let error = resolve_rust_project(&config, &manifest_dir, &root).unwrap_err();

        assert_eq!(
            error,
            RustProjectResolutionError::UnknownProject {
                project: "gamma".to_string(),
            }
        );
    }

    #[test]
    fn resolve_rust_project_errors_for_ambiguous_path_inference() {
        let config = multi_project_config(&["alpha", "beta"]);
        let root = PathBuf::from("/workspace");
        let manifest_dir = PathBuf::from("/workspace/shared/my-crate");

        let error = resolve_rust_project(&config, &manifest_dir, &root).unwrap_err();

        assert_eq!(
            error,
            RustProjectResolutionError::AmbiguousProject {
                manifest_dir: manifest_dir.clone(),
                relative_manifest_dir: PathBuf::from("shared/my-crate"),
                candidates: Vec::new(),
            }
        );
    }
}
