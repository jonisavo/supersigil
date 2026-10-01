//! Where records live and which record belongs to a checkout.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use supersigil_record::RecordId;
use supersigil_record::store::{Association, Store, StoreError};
use supersigil_session::checkout::{self, Placement, placement};

use crate::error::CliError;

/// Resolves the records directory: the flag, then `SUPERSIGIL_RECORD_DIR`,
/// then `$XDG_DATA_HOME/supersigil/records`, then
/// `$HOME/.local/share/supersigil/records`.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] if neither `XDG_DATA_HOME` nor `HOME`
/// is set and no flag was given.
pub fn resolve_record_dir(flag: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(dir) = flag {
        return Ok(dir.to_path_buf());
    }
    if let Some(dir) = std::env::var_os("SUPERSIGIL_RECORD_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg).join("supersigil").join("records"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".local/share/supersigil/records"));
    }
    Err(CliError::CommandFailed(
        "cannot locate the records directory: set SUPERSIGIL_RECORD_DIR or HOME".to_owned(),
    ))
}

/// The checkout to operate on, in its [`checkout::canonical`] spelling,
/// which is the one records store.
///
/// # Errors
///
/// Returns an I/O error if the path does not exist.
pub fn canonical_checkout(flag: Option<&Path>) -> Result<PathBuf, CliError> {
    let path = match flag {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Ok(checkout::canonical(&path)?)
}

/// Finds the record that owns `checkout`, if any.
///
/// A nested checkout belongs to its nearest ancestor's record: a record owns
/// `checkout` when one of its associations is `checkout` itself or a
/// directory containing it, and among such records the one whose association
/// is closest wins, so an exact match beats any ancestor. Paths are compared
/// by [`placement`], which normalizes their spelling. Two records at the same
/// distance is an error that names both; picking one silently would hide
/// evidence written to the other.
///
/// # Errors
///
/// Returns [`CliError::Io`] if the records directory exists but cannot be
/// read, a store error if a record's manifest cannot be accessed or read
/// (only entries without a manifest are skipped, so an unreadable record is
/// never mistaken for an absent one), or [`CliError::CommandFailed`] when
/// two records own `checkout` at the same distance.
pub fn find_record(records_dir: &Path, checkout: &Path) -> Result<Option<Store>, CliError> {
    Ok(find_owner(records_dir, checkout)?.map(|(store, _)| store))
}

/// [`find_record`], with how far the owning association lies above
/// `checkout`: zero for an exact match.
fn find_owner(records_dir: &Path, checkout: &Path) -> Result<Option<(Store, usize)>, CliError> {
    let entries = match std::fs::read_dir(records_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        candidates.push(entry?.path());
    }
    candidates.sort();
    let mut matches: Vec<(Store, usize)> = Vec::new();
    for candidate in candidates {
        let store = match Store::open(&candidate) {
            Ok(store) => store,
            Err(StoreError::NotARecord(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let distance = store
            .manifest()?
            .associations
            .iter()
            .filter_map(|a| rank(&a.checkout, checkout))
            .min();
        if let Some(distance) = distance {
            matches.push((store, distance));
        }
    }
    let Some(nearest) = matches.iter().map(|(_, distance)| *distance).min() else {
        return Ok(None);
    };
    matches.retain(|(_, distance)| *distance == nearest);
    if matches.len() == 1 {
        return Ok(matches.pop());
    }
    let roots: Vec<String> = matches
        .iter()
        .map(|(s, _)| s.root().to_string_lossy().into_owned())
        .collect();
    Err(CliError::CommandFailed(format!(
        "{} is associated with several records: {}. Remove or merge all but one.",
        checkout.to_string_lossy(),
        roots.join(", ")
    )))
}

/// How many components `association` lies above `checkout`: zero when it is
/// the checkout, `None` when it does not contain it.
fn rank(association: &Path, checkout: &Path) -> Option<usize> {
    match placement(checkout, association) {
        Placement::Same => Some(0),
        Placement::Nested(levels) => Some(levels),
        Placement::Outside => None,
    }
}

/// Finds the record for `checkout` or creates one at `<records_dir>/<id>`.
/// Lookup and creation happen under an exclusive lock on
/// `<records_dir>/.lock`, so two first syncs racing on one checkout cannot
/// both create a record. A record found through an ancestor of `checkout`
/// gains `checkout` itself as an association before it is returned.
///
/// Waits as long as the lock is held; [`open_or_create_record_within`] is the
/// bounded variant.
///
/// # Errors
///
/// Returns a store error if the record cannot be created, read, or written,
/// or an I/O error if the records directory or its lock cannot be created.
pub fn open_or_create_record(records_dir: &Path, checkout: &Path) -> Result<Store, CliError> {
    let guard = records_lock_file(records_dir)?;
    guard.lock()?;
    let store = match find_owner(records_dir, checkout)? {
        Some((store, 0)) => store,
        Some((store, _)) => {
            let mut tx = store.begin()?;
            tx.add_association(Association {
                checkout: checkout.to_path_buf(),
            });
            tx.commit()?;
            store
        }
        None => create_record(records_dir, checkout)?,
    };
    drop(guard);
    Ok(store)
}

/// A lock-protected value, or the report that a lock stayed held.
#[derive(Debug)]
pub enum Acquired<T> {
    /// The locks were taken and the value produced.
    Ready(T),
    /// A lock was still held by another writer when the wait ran out.
    Busy,
}

/// [`open_or_create_record`] with every lock wait bounded by `wait`.
///
/// Tries the records-directory lock, and the found record's writer lock
/// when `checkout` must be added as an association, every 50 ms until
/// `wait` has passed since the call began. Returns [`Acquired::Busy`]
/// instead of blocking when either stays held, so a review never waits on a
/// stop hook indefinitely.
///
/// # Errors
///
/// Returns a store error if the record cannot be created, read, or written
/// for a reason other than a held lock, or an I/O error if the records
/// directory or its lock cannot be created or locked.
pub fn open_or_create_record_within(
    records_dir: &Path,
    checkout: &Path,
    wait: Duration,
) -> Result<Acquired<Store>, CliError> {
    let deadline = Instant::now() + wait;
    let guard = records_lock_file(records_dir)?;
    loop {
        match guard.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(RETRY);
            }
            Err(TryLockError::WouldBlock) => return Ok(Acquired::Busy),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
    }
    let acquired = match find_owner(records_dir, checkout)? {
        Some((store, 0)) => Acquired::Ready(store),
        Some((store, _)) => loop {
            match store.begin() {
                Ok(mut tx) => {
                    tx.add_association(Association {
                        checkout: checkout.to_path_buf(),
                    });
                    tx.commit()?;
                    break Acquired::Ready(store);
                }
                Err(StoreError::Locked(_)) if Instant::now() < deadline => {
                    std::thread::sleep(RETRY);
                }
                Err(StoreError::Locked(_)) => break Acquired::Busy,
                Err(e) => return Err(e.into()),
            }
        },
        None => Acquired::Ready(create_record(records_dir, checkout)?),
    };
    drop(guard);
    Ok(acquired)
}

/// Interval between attempts on a held lock.
pub(crate) const RETRY: Duration = Duration::from_millis(50);

/// Opens `<records_dir>/.lock`, creating the directory and the file if needed.
///
/// # Errors
///
/// Returns an I/O error if either cannot be created or opened.
fn records_lock_file(records_dir: &Path) -> Result<File, CliError> {
    std::fs::create_dir_all(records_dir)?;
    Ok(OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(records_dir.join(".lock"))?)
}

/// Creates a record at `<records_dir>/<new id>` associated with `checkout`.
///
/// # Errors
///
/// Returns a store error if the record cannot be created.
fn create_record(records_dir: &Path, checkout: &Path) -> Result<Store, CliError> {
    let id = RecordId::generate();
    let root = records_dir.join(id.as_str());
    Ok(Store::create_with_id(
        &root,
        id,
        Association {
            checkout: checkout.to_path_buf(),
        },
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nested_checkout_ranks_below_an_exact_match_by_its_distance() {
        assert_eq!(
            rank(Path::new("/work/repo"), Path::new("/work/repo")),
            Some(0)
        );
        assert_eq!(
            rank(
                Path::new("/work/repo"),
                Path::new("/work/repo/.claude/worktrees/x")
            ),
            Some(3)
        );
        assert_eq!(
            rank(
                Path::new("/work/repo/.claude/worktrees/x"),
                Path::new("/work/repo")
            ),
            None
        );
        assert_eq!(
            rank(Path::new("/work/repo2"), Path::new("/work/repo")),
            None
        );
        // Windows spellings, compared on every platform.
        assert_eq!(
            rank(
                Path::new(r"C:\repo"),
                Path::new(r"c:\repo\.claude\worktrees\x")
            ),
            Some(3)
        );
        assert_eq!(rank(Path::new(r"C:\repo"), Path::new("C:/repo")), Some(0));
    }
}
