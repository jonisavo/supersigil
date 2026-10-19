//! Locates review stores and loads saved reviews related to the current review.
//!
//! Lock waits are bounded, as in reconcile. A command reports the store as busy
//! when another command holds its lock beyond the deadline.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use supersigil_git::retention::common_dir;
use supersigil_git::{GitError, Repo};
use supersigil_record::reviews::{self, Basis, BasisId, Judgment, ReviewSnapshot, ReviewStore};
use supersigil_review::judging::relates;
use supersigil_session::checkout::canonical;

use crate::error::CliError;
use crate::record_dir::{lock_dir_within, resolve_data_dir};

/// Resolves the reviews directory from the flag, then `SUPERSIGIL_REVIEW_DIR`,
/// then `$XDG_DATA_HOME/supersigil/reviews`, then
/// `$HOME/.local/share/supersigil/reviews`.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] if neither `XDG_DATA_HOME` nor `HOME`
/// is set and neither the flag nor `SUPERSIGIL_REVIEW_DIR` was given.
pub fn resolve_review_dir(flag: Option<&Path>) -> Result<PathBuf, CliError> {
    resolve_data_dir(flag, "SUPERSIGIL_REVIEW_DIR", "reviews")
}

/// Returns the canonical path of `repo`'s git common directory ([`common_dir`]).
/// This path identifies the repository's review store. It uses [`canonical`],
/// as worktree selections do, so all worktrees find the same store.
///
/// # Errors
///
/// Returns [`CliError::Git`] if git cannot name the common directory or
/// [`canonical`] cannot resolve it.
pub fn canonical_common_dir(repo: &Repo) -> Result<PathBuf, CliError> {
    let path = common_dir(repo)?;
    canonical(&path).map_err(|source| {
        CliError::Git(GitError::Io {
            context: format!("cannot resolve {}", path.display()),
            source,
        })
    })
}

/// Finds the review store of `repository`, the canonical path of its git
/// common directory ([`canonical_common_dir`]), in `reviews_dir`, or
/// creates one there. Lookup and creation happen under the lock on
/// `<reviews_dir>/.lock`, tried every 50 ms for up to `wait`; returns
/// `None` when it stays held.
///
/// # Errors
///
/// Returns an I/O error if the directory or its lock cannot be created or
/// locked, or the store errors of [`reviews::find`] and
/// [`reviews::create_in`].
///
/// # Panics
///
/// Panics if `wait` is so large that the deadline overflows [`Instant`].
pub fn open_review_store_within(
    reviews_dir: &Path,
    repository: &Path,
    wait: Duration,
) -> Result<Option<ReviewStore>, CliError> {
    let Some(guard) = lock_dir_within(reviews_dir, Instant::now() + wait)? else {
        return Ok(None);
    };
    let store = match reviews::find(reviews_dir, repository)? {
        Some(store) => store,
        None => reviews::create_in(reviews_dir, repository)?,
    };
    drop(guard);
    Ok(Some(store))
}

/// Loads judged reviews related to `basis`, excluding its own review `id`
/// when it has one, for carry-forward ([`relates`]). A basis that cannot be
/// saved has no id, and relates to saved reviews all the same. Reads and
/// checks each basis once, and parses the full basis only when it relates.
///
/// # Errors
///
/// Returns the store errors of [`ReviewSnapshot::basis_if`].
pub fn judged_bases(
    snapshot: &ReviewSnapshot,
    judgments: &[Judgment],
    id: Option<&BasisId>,
    basis: &Basis,
) -> Result<BTreeMap<BasisId, Basis>, CliError> {
    let mut read = BTreeSet::new();
    let mut related = BTreeMap::new();
    for other in judgments.iter().map(|judgment| &judgment.basis) {
        if Some(other) != id
            && read.insert(other)
            && let Some(found) = snapshot.basis_if(other, |header| relates(basis, header))?
        {
            related.insert(other.clone(), found);
        }
    }
    Ok(related)
}

/// Loads each basis named by `ids` from `snapshot` once.
///
/// # Errors
///
/// Returns the store errors of [`ReviewSnapshot::basis`].
pub fn load_bases<'i>(
    snapshot: &ReviewSnapshot,
    ids: impl IntoIterator<Item = &'i BasisId>,
) -> Result<BTreeMap<BasisId, Basis>, CliError> {
    let mut bases = BTreeMap::new();
    for id in ids {
        if !bases.contains_key(id) {
            bases.insert(id.clone(), snapshot.basis(id)?);
        }
    }
    Ok(bases)
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::path::Path;
    use std::time::Duration;

    use super::open_review_store_within;
    use crate::record_dir::begin_within;

    #[cfg(unix)]
    #[test]
    fn a_common_directory_reached_through_a_link_names_the_store_of_its_target() {
        use supersigil_git::retention::common_dir;
        use supersigil_git::{Git, Repo};
        use supersigil_session::checkout::canonical;

        use super::canonical_common_dir;

        let dir = tempfile::tempdir().unwrap();
        let home = canonical(dir.path()).unwrap();
        let root = home.join("repo");
        std::fs::create_dir(&root).unwrap();
        let git = Git::new(&root)
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
            .with_env("GIT_CONFIG_COUNT", "0")
            .with_env("GIT_CONFIG_PARAMETERS", "")
            .with_env("HOME", &home)
            .with_env("XDG_CONFIG_HOME", home.join("xdg"));
        git.output(["init", "-q"]).unwrap();
        // The git directory moved away, and reached through a link where it
        // was: git names it through the link.
        let target = home.join("store.git");
        std::fs::rename(root.join(".git"), &target).unwrap();
        std::os::unix::fs::symlink(&target, root.join(".git")).unwrap();
        let repo = Repo::open(git).unwrap();
        assert_ne!(common_dir(&repo).unwrap(), target);

        assert_eq!(canonical_common_dir(&repo).unwrap(), target);
    }

    #[test]
    fn a_store_is_found_or_created_unless_the_directory_stays_locked() {
        let dir = tempfile::tempdir().unwrap();
        let reviews = dir.path().join("reviews");
        let repository = Path::new("/work/repo/.git");
        let open = || open_review_store_within(&reviews, repository, Duration::ZERO).unwrap();

        let created = open().unwrap();
        assert_eq!(open().unwrap().root(), created.root());
        let held = OpenOptions::new()
            .write(true)
            .open(reviews.join(".lock"))
            .unwrap();
        held.lock().unwrap();
        assert!(open().is_none());
    }

    #[test]
    fn a_transaction_begins_unless_the_writer_lock_stays_held() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            open_review_store_within(dir.path(), Path::new("/work/repo/.git"), Duration::ZERO)
                .unwrap()
                .unwrap();

        let held = store.begin().unwrap();
        assert!(
            begin_within(Duration::ZERO, || store.begin())
                .unwrap()
                .is_none()
        );
        drop(held);
        assert!(
            begin_within(Duration::ZERO, || store.begin())
                .unwrap()
                .is_some()
        );
    }
}
