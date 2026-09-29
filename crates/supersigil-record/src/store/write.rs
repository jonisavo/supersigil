//! A write transaction: lock, stage, fsync, publish.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::PathBuf;

use super::manifest::{self, Association, Manifest, SourceCursor};
use super::{
    LOCK_FILE, RecordSnapshot, Store, StoreError, io_error, observations_log, reject_reserved,
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
    expected: Revision,
    _lock: File,
    touched: Vec<PathBuf>,
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
            expected: manifest.revision,
            manifest,
            _lock: lock,
            touched: Vec::new(),
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
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidName`] for a name that is not a plain
    /// relative path, [`StoreError::Corrupt`] for a line with a newline or a
    /// file shorter than its pinned length, or an I/O error.
    pub fn append_log(&mut self, log: &str, lines: &[&[u8]]) -> Result<(), StoreError> {
        validate_name(log)?;
        reject_reserved(log)?;
        if self.manifest.documents.values().any(|rel| rel == log) {
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
        self.touched.push(path);
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
    /// Returns [`StoreError::InvalidName`] for a name that is not a plain
    /// relative path, or an I/O error.
    pub fn put_document(&mut self, logical: &str, bytes: &[u8]) -> Result<(), StoreError> {
        validate_name(logical)?;
        let rel = format!("{logical}.r{}.json", self.expected.next().get());
        reject_reserved(&rel)?;
        if self.manifest.logs.contains_key(&rel) {
            return Err(StoreError::InvalidName(rel));
        }
        let path = self.store.root().join(&rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        fs::write(&path, bytes).map_err(|e| io_error(&path, e))?;
        self.manifest.documents.insert(logical.to_owned(), rel);
        self.touched.push(path);
        Ok(())
    }

    /// Records where sync left off in a transcript.
    pub fn set_cursor(&mut self, key: &str, cursor: SourceCursor) {
        self.manifest.cursors.insert(key.to_owned(), cursor);
    }

    /// Adds a checkout association unless one with the same path exists.
    pub fn add_association(&mut self, association: Association) {
        if !self
            .manifest
            .associations
            .iter()
            .any(|a| a.checkout == association.checkout)
        {
            self.manifest.associations.push(association);
        }
    }

    /// Fsyncs every touched file, checks the manifest has not moved, and
    /// publishes the next revision.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if the on-disk revision is not the one
    /// this transaction started from, or an I/O error.
    pub fn commit(mut self) -> Result<Revision, StoreError> {
        for path in &self.touched {
            File::open(path)
                .and_then(|f| f.sync_all())
                .map_err(|e| io_error(path, e))?;
        }
        let on_disk = manifest::read(self.store.root())?;
        if on_disk.revision != self.expected {
            return Err(StoreError::Conflict {
                expected: self.expected.get(),
                found: on_disk.revision.get(),
            });
        }
        self.manifest.revision = self.expected.next();
        manifest::publish(self.store.root(), &self.manifest)?;
        Ok(self.manifest.revision)
    }
}

// No `Drop` impl: dropping `_lock` releases the OS lock, and the manifest on
// disk is untouched until `commit` publishes it.
