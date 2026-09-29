//! File-backed store whose manifest pins exactly one revision.
//!
//! Layout inside a record directory:
//!
//! ```text
//! manifest.json                     identity, revision, pins, cursors
//! write.lock                        created by the first writer, never removed
//! observations/<n>.jsonl            append-only log, pinned by byte length
//! derivations/<n>.r<revision>.json  immutable document, pinned by name
//! ```
//!
//! `<n>` is the number the store assigns a session on its first write and
//! records in the manifest. Session ids come from transcripts, so no file is
//! named after one.
//!
//! Only the operating-system lock held on `write.lock` excludes writers; the
//! file's presence means nothing. Deleting it while a writer runs would let a
//! second writer lock a new file, so never clean it up.

mod manifest;
mod snapshot;
mod write;

use std::io;
use std::path::{Path, PathBuf};

pub use manifest::{Association, Manifest, SCHEMA_VERSION, SourceCursor};
pub use snapshot::RecordSnapshot;
pub use write::WriteTx;

use crate::ids::RecordId;

/// File name of the manifest inside a record directory.
pub const MANIFEST_FILE: &str = "manifest.json";
/// File name of the writer lock inside a record directory.
pub const LOCK_FILE: &str = "write.lock";

/// Relative path of the observation log of the session numbered `number`.
fn log_path(number: u64) -> String {
    format!("observations/{number}.jsonl")
}

/// Logical name of the derivations document of the session numbered
/// `number`.
fn derivations_document(number: u64) -> String {
    format!("derivations/{number}")
}

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// An I/O operation failed.
    #[error("i/o error at {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// A file did not contain the expected JSON.
    #[error("invalid json at {path}: {source}")]
    Json {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },
    /// The directory has no manifest.
    #[error("{0} is not a record directory (no manifest.json)")]
    NotARecord(PathBuf),
    /// The directory already holds a record.
    #[error("{0} already contains a record")]
    AlreadyExists(PathBuf),
    /// Another writer holds the lock.
    #[error("record at {0} is locked by another writer")]
    Locked(PathBuf),
    /// The manifest changed under the writer.
    #[error("manifest revision changed under the writer (expected {expected}, found {found})")]
    Conflict {
        /// Revision the writer started from.
        expected: u64,
        /// Revision found on disk at commit.
        found: u64,
    },
    /// The on-disk state violates an invariant.
    #[error("record is corrupt: {0}")]
    Corrupt(String),
    /// The manifest was written with a schema this binary does not support.
    /// The record is refused rather than rewritten, since rewriting it would
    /// drop whatever the newer schema added.
    #[error("record schema version {found} is not supported (this binary supports {supported})")]
    UnsupportedSchema {
        /// Schema version found in the manifest.
        found: u32,
        /// The only schema version this binary reads and writes.
        supported: u32,
    },
}

pub(crate) fn io_error(path: &Path, source: io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Handle to a record directory.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Creates a new record at `root` with a fresh identity, one checkout
    /// association, and revision zero.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if a manifest is already there,
    /// or an I/O error.
    pub fn create(root: &Path, association: Association) -> Result<Self, StoreError> {
        Self::create_with_id(root, RecordId::generate(), association)
    }

    /// Creates a new record at `root` with the given identity. Callers that
    /// name the directory after the id use this.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if a manifest is already there,
    /// or an I/O error.
    pub fn create_with_id(
        root: &Path,
        record_id: RecordId,
        association: Association,
    ) -> Result<Self, StoreError> {
        std::fs::create_dir_all(root).map_err(|e| io_error(root, e))?;
        let manifest = Manifest::new(record_id, association);
        manifest::create(root, &manifest)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Opens an existing record.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotARecord`] if there is no manifest file (the
    /// manifest or `root` does not exist, `root` is not a directory, or the
    /// manifest is not a file), and [`StoreError::Io`] if the manifest's
    /// metadata cannot be read for any other reason, for example missing
    /// permissions. An unreadable record is never mistaken for an absent one.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        let path = root.join(MANIFEST_FILE);
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => Ok(Self {
                root: root.to_path_buf(),
            }),
            Ok(_) => Err(StoreError::NotARecord(root.to_path_buf())),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                Err(StoreError::NotARecord(root.to_path_buf()))
            }
            Err(e) => Err(io_error(&path, e)),
        }
    }

    /// The record directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads the current manifest.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be read or parsed, and
    /// [`StoreError::UnsupportedSchema`] if it was written with a schema
    /// version other than [`SCHEMA_VERSION`].
    pub fn manifest(&self) -> Result<Manifest, StoreError> {
        manifest::read(&self.root)
    }

    /// A read view pinned to the manifest as it is right now.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be read, including
    /// [`StoreError::UnsupportedSchema`] for a manifest of another schema.
    pub fn snapshot(&self) -> Result<RecordSnapshot, StoreError> {
        Ok(RecordSnapshot::new(self.root.clone(), self.manifest()?))
    }

    /// Starts a write transaction, taking the writer lock.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Locked`] if another writer is active, and
    /// [`StoreError::UnsupportedSchema`] for a manifest of another schema,
    /// which is never rewritten.
    pub fn begin(&self) -> Result<WriteTx<'_>, StoreError> {
        WriteTx::begin(self)
    }
}
