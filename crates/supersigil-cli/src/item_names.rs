//! The CLI's half of naming gate items: how a saved review's items read
//! from the current directory, and what a typed name means there.
//! `supersigil_review::names` does the rest.

use std::ffi::OsStr;
use std::path::Path;

use supersigil_git::RepoPath;
use supersigil_record::reviews::{Basis, PathBytes};
use supersigil_review::names::{ItemNames, Name, Parsed, parse};
use supersigil_review::summary::printed;
use supersigil_session::checkout::resolve;

use crate::error::CliError;
use crate::pipeline::{display_from, repo_path};

/// The names of `basis`'s gate items as typed from `cwd` in `worktree`,
/// paths that differ only in ASCII case being one path when `ignore_case`
/// is set, and `unreconciled:` names matching the paths associated with
/// each record `basis` pinned.
#[must_use]
pub fn item_names<'b>(
    basis: &'b Basis,
    ignore_case: bool,
    worktree: &Path,
    cwd: &Path,
) -> ItemNames<'b> {
    let associations = basis
        .records
        .iter()
        .map(|record| (record.id.clone(), record.associations.clone()))
        .collect();
    ItemNames::new(&basis.gate, associations, ignore_case, |path| {
        typed(worktree, cwd, path)
    })
}

/// `path`, a repository path in `worktree`, as a person types it from
/// `cwd`, or `None` when it cannot be typed: it is not UTF-8, or that
/// spelling holds a control character.
fn typed(worktree: &Path, cwd: &Path, path: &PathBytes) -> Option<String> {
    let path = RepoPath::from_utf8(path.to_str()?);
    let typed = display_from(worktree, cwd, &path);
    printed(OsStr::new(&typed)).is_some().then_some(typed)
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
