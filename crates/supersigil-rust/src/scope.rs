//! Single-project and multi-project Cargo workspace resolution.
//!
//! Determines the project boundaries for evidence discovery by inspecting
//! `Cargo.toml` workspace configuration and mapping source files to their
//! owning crate.

use std::path::Path;

use supersigil_core::{Config, RustProjectResolutionError, resolve_rust_project};
use supersigil_evidence::ProjectScope;

/// Error type for Rust project scope resolution.
pub type ScopeError = RustProjectResolutionError;
pub use supersigil_core::match_rust_project_scope as match_explicit_scope;

/// Resolve the `ProjectScope` for a crate given the supersigil configuration
/// and the crate's `CARGO_MANIFEST_DIR`.
///
/// # Single-project mode
///
/// When the config has no `projects` map (i.e. `config.projects` is `None`),
/// the workspace contains a single supersigil project. Returns a `ProjectScope`
/// with `project: None`.
///
/// # Multi-project mode with explicit Rust config
///
/// When `ecosystem.rust.project_scope` entries are present, the longest
/// `manifest_dir_prefix` that is a prefix of `manifest_dir` wins.
///
/// # Multi-project mode with path-based inference
///
/// When there are multiple supersigil projects but no explicit Rust scope
/// config, attempt path-based inference. If ambiguous or no match, return
/// an error. The shared decision logic lives in `supersigil-core` so the
/// runtime helper and proc macro stay aligned.
///
/// # Errors
///
/// Returns [`ScopeError`] when:
/// - No explicit scope prefix matches the manifest directory.
/// - Path-based inference finds zero or multiple candidate projects.
///
/// # Panics
///
/// Panics if `config.projects` is `Some` but empty (unreachable in valid configs).
pub fn resolve_scope(
    config: &Config,
    manifest_dir: &Path,
    project_root: &Path,
) -> Result<ProjectScope, ScopeError> {
    Ok(ProjectScope {
        project: resolve_rust_project(config, manifest_dir, project_root)?,
        project_root: project_root.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use supersigil_core::{ProjectConfig, RustEcosystemConfig, RustProjectScope};

    use super::*;

    /// Helper: build a minimal single-project `Config`.
    fn single_project_config() -> Config {
        Config {
            paths: Some(vec!["specs/**/*.md".to_string()]),
            ..Config::default()
        }
    }

    /// Helper: build a multi-project `Config` with given project names.
    fn multi_project_config(project_names: &[&str]) -> Config {
        let projects = project_names
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    ProjectConfig {
                        paths: vec![format!("{name}/specs/**/*.md")],
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

    /// Helper: attach explicit Rust `project_scope` entries to a config.
    fn with_rust_scopes(mut config: Config, scopes: Vec<RustProjectScope>) -> Config {
        config.ecosystem.rust = Some(RustEcosystemConfig {
            project_scope: scopes,
            ..Default::default()
        });
        config
    }

    // -----------------------------------------------------------------------
    // Single-project mode (req-5-1)
    // -----------------------------------------------------------------------

    #[test]
    fn single_project_resolves_to_none_project() {
        let config = single_project_config();
        let root = PathBuf::from("/workspace");
        let manifest_dir = PathBuf::from("/workspace/crates/my-crate");

        let scope = resolve_scope(&config, &manifest_dir, &root).unwrap();

        assert_eq!(scope.project, None);
        assert_eq!(scope.project_root, root);
    }

    // -----------------------------------------------------------------------
    // Multi-project mode with explicit rust config (req-5-2)
    // -----------------------------------------------------------------------

    #[test]
    fn multi_project_explicit_scope_longest_prefix_wins() {
        let config = with_rust_scopes(
            multi_project_config(&["alpha", "alpha-sub", "beta"]),
            vec![
                RustProjectScope {
                    manifest_dir_prefix: "crates/alpha".to_string(),
                    project: "alpha".to_string(),
                },
                RustProjectScope {
                    manifest_dir_prefix: "crates/alpha/sub".to_string(),
                    project: "alpha-sub".to_string(),
                },
                RustProjectScope {
                    manifest_dir_prefix: "crates/beta".to_string(),
                    project: "beta".to_string(),
                },
            ],
        );
        let root = PathBuf::from("/workspace");
        // Should match "crates/alpha/sub" (longest prefix) -> "alpha-sub"
        let manifest_dir = PathBuf::from("/workspace/crates/alpha/sub/deep");

        let scope = resolve_scope(&config, &manifest_dir, &root).unwrap();

        assert_eq!(scope.project, Some("alpha-sub".to_string()));
    }

    #[test]
    fn multi_project_explicit_scope_simple_match() {
        let config = with_rust_scopes(
            multi_project_config(&["alpha", "beta"]),
            vec![
                RustProjectScope {
                    manifest_dir_prefix: "crates/alpha".to_string(),
                    project: "alpha".to_string(),
                },
                RustProjectScope {
                    manifest_dir_prefix: "crates/beta".to_string(),
                    project: "beta".to_string(),
                },
            ],
        );
        let root = PathBuf::from("/workspace");
        let manifest_dir = PathBuf::from("/workspace/crates/beta/my-crate");

        let scope = resolve_scope(&config, &manifest_dir, &root).unwrap();

        assert_eq!(scope.project, Some("beta".to_string()));
        assert_eq!(scope.project_root, root);
    }

    #[test]
    fn multi_project_explicit_scope_no_match_errors() {
        let config = with_rust_scopes(
            multi_project_config(&["alpha", "beta"]),
            vec![
                RustProjectScope {
                    manifest_dir_prefix: "crates/alpha".to_string(),
                    project: "alpha".to_string(),
                },
                RustProjectScope {
                    manifest_dir_prefix: "crates/beta".to_string(),
                    project: "beta".to_string(),
                },
            ],
        );
        let root = PathBuf::from("/workspace");
        let manifest_dir = PathBuf::from("/workspace/crates/gamma/my-crate");

        let result = resolve_scope(&config, &manifest_dir, &root);

        assert!(result.is_err());
        match result.unwrap_err() {
            ScopeError::NoMatchingScope {
                manifest_dir: dir, ..
            } => {
                assert_eq!(dir, manifest_dir);
            }
            other @ (ScopeError::UnknownProject { .. } | ScopeError::AmbiguousProject { .. }) => {
                panic!("expected NoMatchingScope, got {other:?}")
            }
        }
    }

    #[test]
    fn multi_project_explicit_scope_unknown_project_errors() {
        let config = with_rust_scopes(
            multi_project_config(&["alpha", "beta"]),
            vec![RustProjectScope {
                manifest_dir_prefix: "crates/alpha".to_string(),
                project: "gamma".to_string(),
            }],
        );
        let root = PathBuf::from("/workspace");
        let manifest_dir = PathBuf::from("/workspace/crates/alpha/my-crate");

        let result = resolve_scope(&config, &manifest_dir, &root);

        assert!(result.is_err());
        match result.unwrap_err() {
            ScopeError::UnknownProject { project } => {
                assert_eq!(project, "gamma");
            }
            other => panic!("expected UnknownProject, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Multi-project mode with path-based inference (req-5-2)
    // -----------------------------------------------------------------------

    #[test]
    fn multi_project_path_inference_unambiguous_resolves() {
        // Multiple projects, no explicit rust config, but manifest dir
        // clearly belongs to one project (path contains project name).
        let config = multi_project_config(&["alpha", "beta"]);
        let root = PathBuf::from("/workspace");
        let manifest_dir = PathBuf::from("/workspace/alpha/crates/my-crate");

        let scope = resolve_scope(&config, &manifest_dir, &root).unwrap();

        assert_eq!(scope.project, Some("alpha".to_string()));
        assert_eq!(scope.project_root, root);
    }

    #[test]
    fn multi_project_path_inference_ambiguous_errors() {
        // Multiple projects, no explicit rust config -> path-based inference
        let config = multi_project_config(&["alpha", "beta"]);
        let root = PathBuf::from("/workspace");
        // Manifest dir doesn't clearly belong to one project
        let manifest_dir = PathBuf::from("/workspace/shared/my-crate");

        let result = resolve_scope(&config, &manifest_dir, &root);

        assert!(result.is_err());
        match result.unwrap_err() {
            ScopeError::AmbiguousProject { .. } => {}
            other @ (ScopeError::UnknownProject { .. } | ScopeError::NoMatchingScope { .. }) => {
                panic!("expected AmbiguousProject, got {other:?}")
            }
        }
    }

    // -----------------------------------------------------------------------
    // match_explicit_scope helper
    // -----------------------------------------------------------------------

    #[test]
    fn match_explicit_scope_returns_longest_prefix() {
        let scopes = vec![
            RustProjectScope {
                manifest_dir_prefix: "crates/foo".to_string(),
                project: "foo".to_string(),
            },
            RustProjectScope {
                manifest_dir_prefix: "crates/foo/bar".to_string(),
                project: "foo-bar".to_string(),
            },
        ];

        let result = match_explicit_scope(&scopes, Path::new("crates/foo/bar/baz"));
        assert_eq!(result, Some("foo-bar".to_string()));
    }

    #[test]
    fn match_explicit_scope_returns_none_when_no_prefix_matches() {
        let scopes = vec![RustProjectScope {
            manifest_dir_prefix: "crates/foo".to_string(),
            project: "foo".to_string(),
        }];

        let result = match_explicit_scope(&scopes, Path::new("other/path"));
        assert_eq!(result, None);
    }
}
