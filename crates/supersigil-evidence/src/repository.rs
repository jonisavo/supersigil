//! Repository metadata types and URL parsing utilities.
//!
//! Provides [`RepositoryInfo`], [`WorkspaceMetadata`], and the
//! [`parse_repository_url`] helper that extracts structured repository
//! information from HTTPS and SSH URLs.
//!
//! [`RepositoryProvider`] lives in `supersigil-core` and is re-exported here.

use serde::Serialize;
use supersigil_core::RepositoryProvider;

// ---------------------------------------------------------------------------
// RepositoryInfo
// ---------------------------------------------------------------------------

/// Parsed repository coordinates extracted from a URL or explicit config.
///
/// Serialized as camelCase JSON for JavaScript consumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    /// Hosting provider (e.g. GitHub, GitLab).
    pub provider: RepositoryProvider,
    /// Repository path: `"owner/repo"` or `"group/subgroup/project"`.
    pub repo: String,
    /// Hostname of the hosting service (e.g. `"github.com"`).
    pub host: String,
    /// Default branch assumed for source links (e.g. `"main"`).
    pub main_branch: String,
}

// ---------------------------------------------------------------------------
// WorkspaceMetadata
// ---------------------------------------------------------------------------

/// Optional workspace-level metadata surfaced by ecosystem plugins.
///
/// Constructed by plugins via [`super::EcosystemPlugin::workspace_metadata`],
/// not deserialized directly.
#[derive(Debug, Clone)]
pub struct WorkspaceMetadata {
    /// Repository information, if discoverable.
    pub repository: Option<RepositoryInfo>,
}

// ---------------------------------------------------------------------------
// parse_repository_url
// ---------------------------------------------------------------------------

/// Parse a repository URL into structured info.
///
/// Handles:
/// - `https://github.com/owner/repo`
/// - `https://github.com/owner/repo.git`
/// - `git@github.com:owner/repo.git` (scp-style SSH)
/// - `ssh://git@github.com/owner/repo.git` (standard SSH)
///
/// Infers [`RepositoryProvider`] from hostname. Returns `None` for
/// unrecognized hosts (self-hosted instances need explicit config).
/// Sets `main_branch` to `"main"` by default.
#[must_use]
pub fn parse_repository_url(url: &str) -> Option<RepositoryInfo> {
    let (host, path) = if let Some(rest) = url.strip_prefix("https://") {
        let (host, path) = rest.split_once('/')?;
        (host, path)
    } else if let Some(rest) = url.strip_prefix("ssh://") {
        // ssh://[user@]host/path — strip optional user@ prefix
        let rest = rest.split_once('@').map_or(rest, |(_, after)| after);
        let (host, path) = rest.split_once('/')?;
        (host, path)
    } else if let Some(rest) = url.strip_prefix("git@") {
        // scp-style: git@host:path
        let (host, path) = rest.split_once(':')?;
        (host, path)
    } else {
        return None;
    };

    let provider = provider_from_host(host)?;

    let repo = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');

    if repo.is_empty() {
        return None;
    }

    Some(RepositoryInfo {
        provider,
        repo: repo.to_string(),
        host: host.to_string(),
        main_branch: "main".to_string(),
    })
}

/// Map a hostname to a known provider, or `None` for unrecognized hosts.
///
/// Only well-known hosts are recognized. Self-hosted instances (e.g.
/// `gitlab.mycompany.com`, `gitea.internal`) need explicit
/// `[documentation.repository]` configuration. Codeberg is the only
/// well-known Gitea host.
fn provider_from_host(host: &str) -> Option<RepositoryProvider> {
    match host {
        "github.com" => Some(RepositoryProvider::GitHub),
        "gitlab.com" => Some(RepositoryProvider::GitLab),
        "bitbucket.org" => Some(RepositoryProvider::Bitbucket),
        "codeberg.org" => Some(RepositoryProvider::Gitea),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // === RepositoryProvider =================================================

    #[test]
    fn provider_debug_and_clone() {
        let p = RepositoryProvider::GitHub;
        let cloned = p;
        assert_eq!(format!("{cloned:?}"), "GitHub");
    }

    #[test]
    fn provider_equality() {
        assert_eq!(RepositoryProvider::GitHub, RepositoryProvider::GitHub);
        assert_ne!(RepositoryProvider::GitHub, RepositoryProvider::GitLab);
    }

    #[test]
    fn provider_serde_roundtrip() {
        let json = serde_json::to_string(&RepositoryProvider::GitHub).unwrap();
        assert_eq!(json, r#""github""#);

        let deserialized: RepositoryProvider = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, RepositoryProvider::GitHub);
    }

    #[test]
    fn provider_serde_all_variants() {
        let cases = [
            (RepositoryProvider::GitHub, r#""github""#),
            (RepositoryProvider::GitLab, r#""gitlab""#),
            (RepositoryProvider::Bitbucket, r#""bitbucket""#),
            (RepositoryProvider::Gitea, r#""gitea""#),
        ];
        for (variant, expected_json) in cases {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_json, "serialization of {variant:?}");
            let back: RepositoryProvider = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant, "deserialization of {expected_json}");
        }
    }

    // === RepositoryInfo =====================================================

    #[test]
    fn repository_info_serializes() {
        let info = RepositoryInfo {
            provider: RepositoryProvider::GitHub,
            repo: "owner/repo".into(),
            host: "github.com".into(),
            main_branch: "main".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains(r#""provider":"github""#));
        assert!(json.contains(r#""repo":"owner/repo""#));
        assert!(json.contains(r#""host":"github.com""#));
        assert!(json.contains(r#""mainBranch":"main""#));
    }

    // === parse_repository_url: GitHub =======================================

    #[test]
    fn parse_github_https() {
        let info = parse_repository_url("https://github.com/owner/repo").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitHub);
        assert_eq!(info.repo, "owner/repo");
        assert_eq!(info.host, "github.com");
        assert_eq!(info.main_branch, "main");
    }

    #[test]
    fn parse_github_https_dot_git() {
        let info = parse_repository_url("https://github.com/owner/repo.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitHub);
        assert_eq!(info.repo, "owner/repo");
    }

    #[test]
    fn parse_github_ssh() {
        let info = parse_repository_url("git@github.com:owner/repo.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitHub);
        assert_eq!(info.repo, "owner/repo");
        assert_eq!(info.host, "github.com");
    }

    #[test]
    fn parse_github_ssh_no_dot_git() {
        let info = parse_repository_url("git@github.com:owner/repo").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitHub);
        assert_eq!(info.repo, "owner/repo");
    }

    // === parse_repository_url: GitLab =======================================

    #[test]
    fn parse_gitlab_https() {
        let info = parse_repository_url("https://gitlab.com/owner/repo").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitLab);
        assert_eq!(info.repo, "owner/repo");
        assert_eq!(info.host, "gitlab.com");
    }

    #[test]
    fn parse_gitlab_https_dot_git() {
        let info = parse_repository_url("https://gitlab.com/owner/repo.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitLab);
        assert_eq!(info.repo, "owner/repo");
    }

    #[test]
    fn parse_gitlab_ssh() {
        let info = parse_repository_url("git@gitlab.com:group/project.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitLab);
        assert_eq!(info.repo, "group/project");
        assert_eq!(info.host, "gitlab.com");
    }

    #[test]
    fn parse_gitlab_nested_groups() {
        let info = parse_repository_url("https://gitlab.com/group/subgroup/project.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitLab);
        assert_eq!(info.repo, "group/subgroup/project");
    }

    // === parse_repository_url: Bitbucket ====================================

    #[test]
    fn parse_bitbucket_https() {
        let info = parse_repository_url("https://bitbucket.org/owner/repo").unwrap();
        assert_eq!(info.provider, RepositoryProvider::Bitbucket);
        assert_eq!(info.repo, "owner/repo");
        assert_eq!(info.host, "bitbucket.org");
    }

    #[test]
    fn parse_bitbucket_https_dot_git() {
        let info = parse_repository_url("https://bitbucket.org/owner/repo.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::Bitbucket);
        assert_eq!(info.repo, "owner/repo");
    }

    #[test]
    fn parse_bitbucket_ssh() {
        let info = parse_repository_url("git@bitbucket.org:team/project.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::Bitbucket);
        assert_eq!(info.repo, "team/project");
        assert_eq!(info.host, "bitbucket.org");
    }

    // === parse_repository_url: Gitea (Codeberg) =============================

    #[test]
    fn parse_gitea_https() {
        let info = parse_repository_url("https://codeberg.org/owner/repo").unwrap();
        assert_eq!(info.provider, RepositoryProvider::Gitea);
        assert_eq!(info.repo, "owner/repo");
        assert_eq!(info.host, "codeberg.org");
        assert_eq!(info.main_branch, "main");
    }

    #[test]
    fn parse_gitea_https_dot_git() {
        let info = parse_repository_url("https://codeberg.org/owner/repo.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::Gitea);
        assert_eq!(info.repo, "owner/repo");
    }

    #[test]
    fn parse_gitea_ssh() {
        let info = parse_repository_url("git@codeberg.org:owner/repo.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::Gitea);
        assert_eq!(info.repo, "owner/repo");
        assert_eq!(info.host, "codeberg.org");
    }

    // === parse_repository_url: unrecognized hosts ===========================

    #[test]
    fn parse_self_hosted_gitlab_returns_none() {
        assert!(parse_repository_url("https://gitlab.example.com/org/repo").is_none());
    }

    #[test]
    fn parse_self_hosted_gitea_returns_none() {
        assert!(parse_repository_url("https://gitea.mycompany.io/org/repo").is_none());
    }

    #[test]
    fn parse_unknown_host_ssh_returns_none() {
        assert!(parse_repository_url("git@code.internal:team/project.git").is_none());
    }

    // === parse_repository_url: ssh:// scheme =================================

    #[test]
    fn parse_github_ssh_scheme() {
        let info = parse_repository_url("ssh://git@github.com/owner/repo.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitHub);
        assert_eq!(info.repo, "owner/repo");
        assert_eq!(info.host, "github.com");
    }

    #[test]
    fn parse_gitlab_ssh_scheme() {
        let info = parse_repository_url("ssh://git@gitlab.com/group/project.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitLab);
        assert_eq!(info.repo, "group/project");
        assert_eq!(info.host, "gitlab.com");
    }

    #[test]
    fn parse_bitbucket_ssh_scheme() {
        let info = parse_repository_url("ssh://git@bitbucket.org/team/project.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::Bitbucket);
        assert_eq!(info.repo, "team/project");
        assert_eq!(info.host, "bitbucket.org");
    }

    #[test]
    fn parse_ssh_scheme_without_user() {
        let info = parse_repository_url("ssh://github.com/owner/repo").unwrap();
        assert_eq!(info.provider, RepositoryProvider::GitHub);
        assert_eq!(info.repo, "owner/repo");
    }

    #[test]
    fn parse_codeberg_ssh_scheme() {
        let info = parse_repository_url("ssh://git@codeberg.org/owner/repo.git").unwrap();
        assert_eq!(info.provider, RepositoryProvider::Gitea);
        assert_eq!(info.repo, "owner/repo");
        assert_eq!(info.host, "codeberg.org");
    }

    #[test]
    fn parse_ssh_scheme_unknown_host_returns_none() {
        assert!(parse_repository_url("ssh://git@code.internal/team/project.git").is_none());
    }

    // === parse_repository_url: edge cases ===================================

    #[test]
    fn parse_empty_string_returns_none() {
        assert!(parse_repository_url("").is_none());
    }

    #[test]
    fn parse_garbage_returns_none() {
        assert!(parse_repository_url("not-a-url").is_none());
    }

    #[test]
    fn parse_trailing_slash_stripped() {
        let info = parse_repository_url("https://github.com/owner/repo/").unwrap();
        assert_eq!(info.repo, "owner/repo");
    }

    #[test]
    fn parse_http_not_supported() {
        // We only support https, not plain http.
        assert!(parse_repository_url("http://github.com/owner/repo").is_none());
    }

    // === WorkspaceMetadata ==================================================

    #[test]
    fn workspace_metadata_none_repository() {
        let meta = WorkspaceMetadata { repository: None };
        assert!(meta.repository.is_none());
    }

    #[test]
    fn workspace_metadata_with_repository() {
        let meta = WorkspaceMetadata {
            repository: Some(RepositoryInfo {
                provider: RepositoryProvider::GitHub,
                repo: "owner/repo".into(),
                host: "github.com".into(),
                main_branch: "main".into(),
            }),
        };
        assert_eq!(
            meta.repository.as_ref().unwrap().provider,
            RepositoryProvider::GitHub
        );
    }
}
