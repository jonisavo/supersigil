//! The CLI's half of naming gate items: how a saved review's items read
//! from the current directory, and what a typed name means there.
//! `supersigil_review::names` does the rest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use supersigil_git::RepoPath;
use supersigil_record::RecordId;
use supersigil_record::reviews::{Basis, PathBytes};
use supersigil_review::names::{Name, Parsed, parse};
use supersigil_session::checkout::resolve;

use crate::error::CliError;
use crate::pipeline::{display_from, repo_path};

/// The paths associated with each record `basis` pinned, which
/// `unreconciled:` names match.
#[must_use]
pub fn associations(basis: &Basis) -> BTreeMap<RecordId, Vec<PathBuf>> {
    basis
        .records
        .iter()
        .map(|record| (record.id.clone(), record.associations.clone()))
        .collect()
}

/// `path`, a repository path in `worktree`, as a person types it from
/// `cwd`, or `None` when it cannot be typed: it is not UTF-8, or that
/// spelling holds a control character.
#[must_use]
pub fn typed(worktree: &Path, cwd: &Path, path: &PathBytes) -> Option<String> {
    let path = RepoPath::from_utf8(path.to_str()?);
    Some(display_from(worktree, cwd, &path)).filter(|typed| !typed.chars().any(char::is_control))
}

/// What `typed`, a name a person typed in `cwd`, means in `worktree`: a
/// path relative to `cwd` as a repository path (the root as the empty
/// path), an `unreconciled:` path made absolute and canonical where it
/// exists, and a prefix as typed.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when a path lies outside `worktree`
/// or is not valid UTF-8.
pub fn name(worktree: &Path, cwd: &Path, typed: &str) -> Result<Name, CliError> {
    Ok(match parse(typed) {
        Parsed::Path { origin, path } => Name::Path {
            origin,
            path: PathBytes::new(
                repo_path(worktree, cwd, path)?
                    .map_or_else(Vec::new, |path| path.as_bytes().to_vec()),
            ),
        },
        Parsed::Commit(prefix) => Name::Commit(prefix.to_owned()),
        Parsed::Unreconciled(path) => Name::Unreconciled(resolve(&cwd.join(path))),
        Parsed::Id(prefix) => Name::Id(prefix.to_owned()),
    })
}
