//! A write transaction: lock, stage, fsync, publish.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::PathBuf;

use super::manifest::{self, Association, Manifest, SourceCursor};
use super::{
    LOCK_FILE, RecordSnapshot, Store, StoreError, io_error, is_log_name, observations_log,
    validate_name,
};
use crate::derivations::DerivationSet;
use crate::ids::Revision;
use crate::observations::Observation;

/// An open write transaction. Dropping it without [`WriteTx::commit`]
/// releases the lock and leaves the manifest untouched.
///
/// The lock is an exclusive OS lock on `write.lock` held through `_lock`.
/// The kernel releases it when the handle drops or the process dies, so a
/// killed writer never blocks the next one. The file itself is never removed.
#[derive(Debug)]
pub struct WriteTx<'a> {
    store: &'a Store,
    /// The manifest as it will be published, with staged changes applied.
    manifest: Manifest,
    /// The manifest as it was when the lock was taken.
    starting: Manifest,
    _lock: File,
    /// Files to fsync at commit; a set, so a log appended twice syncs once.
    touched: BTreeSet<PathBuf>,
}

impl<'a> WriteTx<'a> {
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

    /// The manifest as it will be published, including staged changes.
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// A read view pinned to the manifest this transaction started from.
    /// This transaction's own appends are not visible through it, so
    /// derivations computed from it plus the staged observations are
    /// complete and consistent.
    #[must_use]
    pub fn snapshot(&self) -> RecordSnapshot {
        RecordSnapshot::new(self.store.root().to_path_buf(), self.starting.clone())
    }

    /// Appends lines to a log. Any bytes beyond the pinned length (a crash
    /// tail) are truncated first. Lines must not contain newlines.
    ///
    /// Log names end in `.jsonl`, which keeps them disjoint from documents
    /// and the store's own files (see the [module docs](super)).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidName`] for a name that is not a canonical
    /// relative path or does not end in `.jsonl`,
    /// [`StoreError::Corrupt`] for a line with a newline or a file shorter
    /// than its pinned length, or an I/O error.
    pub fn append_log(&mut self, log: &str, lines: &[&[u8]]) -> Result<(), StoreError> {
        validate_name(log)?;
        if !is_log_name(log) {
            return Err(StoreError::InvalidName(log.to_owned()));
        }
        if let Some(bad) = lines.iter().find(|l| l.contains(&b'\n')) {
            return Err(StoreError::Corrupt(format!(
                "log line contains a newline: {}",
                String::from_utf8_lossy(bad)
            )));
        }
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
        let mut written = pinned;
        for line in lines {
            file.write_all(line).map_err(|e| io_error(&path, e))?;
            file.write_all(b"\n").map_err(|e| io_error(&path, e))?;
            written += line.len() as u64 + 1;
        }
        self.manifest.logs.insert(log.to_owned(), written);
        self.touched.insert(path);
        Ok(())
    }

    /// Appends observations to their sessions' logs as JSON lines.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or the append fails.
    pub fn append_observations(&mut self, observations: &[Observation]) -> Result<(), StoreError> {
        let mut by_log: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
        for observation in observations {
            let log = observations_log(observation.session());
            let line = serde_json::to_vec(observation).map_err(|source| StoreError::Json {
                path: self.store.root().join(&log),
                source,
            })?;
            by_log.entry(log).or_default().push(line);
        }
        for (log, lines) in &by_log {
            let refs: Vec<&[u8]> = lines.iter().map(Vec::as_slice).collect();
            self.append_log(log, &refs)?;
        }
        Ok(())
    }

    /// Writes a session's derivations as this revision's document.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or the write fails.
    ///
    /// # Panics
    ///
    /// Panics if the next revision number would overflow `u64`, as
    /// [`Revision::next`] does.
    pub fn put_derivations(&mut self, set: &DerivationSet) -> Result<(), StoreError> {
        let logical = DerivationSet::document_name(&set.session);
        let bytes = serde_json::to_vec_pretty(set).map_err(|source| StoreError::Json {
            path: self.store.root().join(&logical),
            source,
        })?;
        self.put_document(&logical, &bytes)
    }

    /// Writes an immutable document for this revision and points the logical
    /// name at it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidName`] for a name that is not a canonical
    /// relative path, or an I/O error.
    ///
    /// # Panics
    ///
    /// Panics if the next revision number would overflow `u64`, as
    /// [`Revision::next`] does.
    pub fn put_document(&mut self, logical: &str, bytes: &[u8]) -> Result<(), StoreError> {
        validate_name(logical)?;
        let rel = format!("{logical}.r{}.json", self.starting.revision.next().get());
        let path = self.store.root().join(&rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        fs::write(&path, bytes).map_err(|e| io_error(&path, e))?;
        self.manifest.documents.insert(logical.to_owned(), rel);
        self.touched.insert(path);
        Ok(())
    }

    /// Records where sync left off in a transcript.
    pub fn set_cursor(&mut self, key: &str, cursor: SourceCursor) {
        self.manifest.cursors.insert(key.to_owned(), cursor);
    }

    /// Adds a checkout association unless one with the same path exists.
    /// Returns whether it was added.
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

    /// Fsyncs every touched file and every directory from each file's parent
    /// up to the record root, checks the manifest has not moved, and
    /// publishes the next revision. Where directories can be synced, nothing
    /// the new manifest references can be lost to a power failure after it
    /// is published; on Windows, which cannot open a directory to sync it,
    /// directory syncing is skipped and new entries rely on the file system.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if the on-disk revision is not the one
    /// this transaction started from, or an I/O error, including a failure to
    /// sync a directory where the platform supports it.
    ///
    /// # Panics
    ///
    /// Panics if the next revision number would overflow `u64`, as
    /// [`Revision::next`] does.
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
