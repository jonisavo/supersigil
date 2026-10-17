//! Names saved gate items from the current directory and parses typed names.
//! `supersigil_review::names` resolves those names to items.

use std::ffi::OsStr;
use std::path::Path;

use supersigil_git::RepoPath;
use supersigil_record::reviews::{Basis, PathBytes};
use supersigil_review::names::{ItemNames, Name, Parsed, parse};
use supersigil_review::printed::printed;
use supersigil_session::checkout::resolve;

use crate::error::CliError;
use crate::pipeline::{display_from, repo_path};

/// Names `basis`'s gate items as typed from `cwd` in `worktree`.
/// With `ignore_case`, paths differing only in ASCII case count as one path.
/// `unreconciled:` names match each pinned record's associated paths.
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

/// Spells repository `path` in `worktree` relative to `cwd`.
/// Returns `None` if the path is not UTF-8 or its spelling contains a control character.
fn typed(worktree: &Path, cwd: &Path, path: &PathBytes) -> Option<String> {
    let path = RepoPath::from_utf8(path.to_str()?);
    let typed = display_from(worktree, cwd, &path);
    printed(OsStr::new(&typed)).is_some().then_some(typed)
}

/// Parses `typed` from `cwd` in `worktree`. Converts ordinary paths to repository
/// paths, using an empty path for the root. Makes `unreconciled:` paths absolute
/// and canonical where they exist. Leaves prefixes as typed.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when an ordinary path lies outside `worktree`
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
