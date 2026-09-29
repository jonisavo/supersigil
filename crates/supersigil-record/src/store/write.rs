//! Writes record data under an exclusive lock and commits it through the manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::PathBuf;

use super::manifest::{self, Association, Manifest, SourceCursor};
use super::{
    LOCK_FILE, RecordSnapshot, Store, StoreError, derivations_document, io_error, log_path,
};
use crate::derivations::DerivationSet;
use crate::ids::{Revision, SessionId};
use crate::observations::Observation;

/// Holds the writer lock while adding data for the next record revision.
///
/// Writes reach the data files before [`Self::commit`], but readers cannot see
/// them until commit publishes the new manifest. Dropping the transaction
/// without committing leaves the manifest unchanged and releases the lock.
/// Uncommitted log bytes and document files may remain on disk.
///
/// The OS lock on `write.lock` is released when the transaction drops or the
/// process exits. The lock file stays in place.
#[derive(Debug)]
pub struct WriteTx<'a> {
    store: &'a Store,
    /// Manifest with this transaction's changes, ready to publish at commit.
    manifest: Manifest,
    /// Manifest read after acquiring the writer lock.
    starting: Manifest,
    _lock: File,
    /// Files to sync at commit, with duplicate paths removed.
    touched: BTreeSet<PathBuf>,
}

impl<'a> WriteTx<'a> {
    /// Acquires the writer lock and reads the starting manifest.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Locked`] if another writer holds the lock,
    /// [`StoreError::Io`] if a lock operation fails, or an error from reading
    /// and validating the manifest.
    pub(super) fn begin(store: &'a Store) -> Result<Self, StoreError> {
        let lock_path = store.root().join(LOCK_FILE);
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| io_error(&lock_path, e))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(StoreError::Locked(store.root().to_path_buf()));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(io_error(&lock_path, e)),
        }
        let manifest = manifest::read(store.root())?;
        Ok(Self {
            store,
            starting: manifest.clone(),
            manifest,
            _lock: lock,
            touched: BTreeSet::new(),
        })
    }

    /// Returns the manifest with this transaction's changes applied.
    ///
    /// Its revision number advances only during [`Self::commit`].
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Creates a snapshot of the revision this transaction started from.
    ///
    /// It excludes this transaction's writes. When computing derivations for
    /// the next revision, combine its observations with those being appended.
    #[must_use]
    pub fn snapshot(&self) -> RecordSnapshot {
        RecordSnapshot::new(self.store.root().to_path_buf(), self.starting.clone())
    }

    /// Appends observations as JSON lines to each session's log.
    ///
    /// Assigns numbers to new sessions and preserves input order within each
    /// session. Does not remove duplicate events. The appended observations
    /// become visible to new snapshots after [`Self::commit`].
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Corrupt`] if a log is shorter than the length in
    /// the transaction's manifest, [`StoreError::Json`] if serialization fails,
    /// or [`StoreError::Io`] if a directory or file operation fails.
    pub fn append_observations(&mut self, observations: &[Observation]) -> Result<(), StoreError> {
        let mut lines: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        for observation in observations {
            let number = self.session_number(observation.session());
            let bytes = lines.entry(number).or_default();
            serde_json::to_writer(&mut *bytes, observation).map_err(|source| StoreError::Json {
                path: self.store.root().join(log_path(number)),
                source,
            })?;
            bytes.push(b'\n');
        }
        for (number, bytes) in &lines {
            self.append_log(&log_path(*number), bytes)?;
        }
        Ok(())
    }

    /// Removes bytes beyond the transaction's recorded log length, then appends `bytes`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Corrupt`] if the log is shorter than the recorded
    /// length or [`StoreError::Io`] if a directory or file operation fails.
    fn append_log(&mut self, log: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.store.root().join(log);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        let pinned = self.manifest.logs.get(log).copied().unwrap_or(0);
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| io_error(&path, e))?;
        let len = file.metadata().map_err(|e| io_error(&path, e))?.len();
        if len < pinned {
            return Err(StoreError::Corrupt(format!(
                "{} is shorter ({len} bytes) than its pinned length ({pinned})",
                path.display()
            )));
        }
        file.set_len(pinned).map_err(|e| io_error(&path, e))?;
        file.seek(SeekFrom::Start(pinned))
            .map_err(|e| io_error(&path, e))?;
        file.write_all(bytes).map_err(|e| io_error(&path, e))?;
        self.manifest
            .logs
            .insert(log.to_owned(), pinned + bytes.len() as u64);
        self.touched.insert(path);
        Ok(())
    }

    /// Writes a session's derivations to a file named for the next store revision.
    ///
    /// Assigns a number to a new session. Calling this again for the same
    /// session in this transaction replaces the pending document. Previously
    /// committed documents remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Json`] if serialization fails or [`StoreError::Io`]
    /// if a directory or file operation fails.
    ///
    /// # Panics
    ///
    /// Panics if the starting revision is [`u64::MAX`].
    pub fn put_derivations(&mut self, set: &DerivationSet) -> Result<(), StoreError> {
        let logical = derivations_document(self.session_number(&set.session));
        let rel = format!("{logical}.r{}.json", self.starting.revision.next().get());
        let path = self.store.root().join(&rel);
        let bytes = serde_json::to_vec_pretty(set).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        fs::write(&path, bytes).map_err(|e| io_error(&path, e))?;
        self.manifest.documents.insert(logical, rel);
        self.touched.insert(path);
        Ok(())
    }

    /// Returns the session's number, assigning the next unused number if needed.
    fn session_number(&mut self, session: &SessionId) -> u64 {
        if let Some(&number) = self.manifest.sessions.get(session) {
            return number;
        }
        let number = self.manifest.sessions.len() as u64;
        self.manifest.sessions.insert(session.clone(), number);
        number
    }

    /// Sets the sync cursor for the transcript path `key` in the pending manifest.
    pub fn set_cursor(&mut self, key: &str, cursor: SourceCursor) {
        self.manifest.cursors.insert(key.to_owned(), cursor);
    }

    /// Adds a checkout to the pending manifest if its path is not already listed.
    ///
    /// Returns `true` if added, or `false` if an equal path was already present.
    pub fn add_association(&mut self, association: Association) -> bool {
        let known = self
            .manifest
            .associations
            .iter()
            .any(|a| a.checkout == association.checkout);
        if !known {
            self.manifest.associations.push(association);
        }
        !known
    }

    /// Publishes this transaction's changes and returns the new revision number.
    ///
    /// Syncs changed files and their parent directories up to the record root,
    /// then checks that the on-disk revision still matches the starting one.
    /// Increments the revision and publishes the manifest by atomic rename.
    ///
    /// On Windows, directories that cannot be opened are not synced. Their
    /// entries then rely on the file system's durability guarantees.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if the on-disk revision changed,
    /// errors from reading or validating the current manifest, or
    /// [`StoreError::Json`] or [`StoreError::Io`] if syncing or publishing fails.
    /// An error syncing the record directory after the rename can occur after
    /// the new manifest is already visible.
    ///
    /// # Panics
    ///
    /// Panics if the starting revision is [`u64::MAX`].
    pub fn commit(mut self) -> Result<Revision, StoreError> {
        let root = self.store.root();
        let mut dirs = BTreeSet::new();
        for path in &self.touched {
            // Write access: Windows flushes through `FlushFileBuffers`, which
            // needs it.
            OpenOptions::new()
                .write(true)
                .open(path)
                .and_then(|f| f.sync_all())
                .map_err(|e| io_error(path, e))?;
            for dir in path.ancestors().skip(1) {
                dirs.insert(dir.to_path_buf());
                if dir == root {
                    break;
                }
            }
        }
        for dir in &dirs {
            manifest::sync_dir(dir)?;
        }
        let on_disk = manifest::read(self.store.root())?;
        let expected = self.starting.revision;
        if on_disk.revision != expected {
            return Err(StoreError::Conflict {
                expected: expected.get(),
                found: on_disk.revision.get(),
            });
        }
        self.manifest.revision = expected.next();
        manifest::publish(self.store.root(), &self.manifest)?;
        Ok(self.manifest.revision)
    }
}
