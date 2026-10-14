//! Where saved reviews live and how a command opens its repository's review
//! store.
//!
//! Every lock wait is bounded, as reconcile's are: a store another command
//! holds is reported busy, never waited on indefinitely.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use supersigil_record::StoreError;
use supersigil_record::reviews::{self, ReviewStore, ReviewTx};

use crate::error::CliError;
use crate::record_dir::{lock_dir_within, poll, resolve_data_dir};

/// Resolves the reviews directory: the flag, then `SUPERSIGIL_REVIEW_DIR`,
/// then `$XDG_DATA_HOME/supersigil/reviews`, then
/// `$HOME/.local/share/supersigil/reviews`.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] if neither `XDG_DATA_HOME` nor `HOME`
/// is set and no flag was given.
pub fn resolve_review_dir(flag: Option<&Path>) -> Result<PathBuf, CliError> {
    resolve_data_dir(flag, "SUPERSIGIL_REVIEW_DIR", "reviews")
}

/// Finds the review store of `repository`, the canonical path of its git
/// common directory, in `reviews_dir`, or creates one there. Lookup and
/// creation happen under the lock on `<reviews_dir>/.lock`, tried every
/// 50 ms for up to `wait`; returns `None` when it stays held.
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

/// Begins a write transaction on `store`, trying its writer lock every
/// 50 ms for up to `wait`; returns `None` when it stays held.
///
/// # Errors
///
/// Returns the store errors of [`ReviewStore::begin`] other than a held
/// lock.
///
/// # Panics
///
/// Panics if `wait` is so large that the deadline overflows [`Instant`].
pub fn begin_within(store: &ReviewStore, wait: Duration) -> Result<Option<ReviewTx<'_>>, CliError> {
    poll(Instant::now() + wait, || match store.begin() {
        Ok(tx) => Ok(Some(tx)),
        Err(StoreError::Locked(_)) => Ok(None),
        Err(e) => Err(CliError::from(e)),
    })
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::path::Path;
    use std::time::Duration;

    use super::{begin_within, open_review_store_within};

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
        assert!(begin_within(&store, Duration::ZERO).unwrap().is_none());
        drop(held);
        assert!(begin_within(&store, Duration::ZERO).unwrap().is_some());
    }
}
