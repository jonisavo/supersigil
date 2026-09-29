//! JSON storage for session observations and computed derivations.
//!
//! The manifest lists each log's committed byte length and each derivation
//! document's file name. A snapshot keeps one manifest and reads only those
//! bytes and files, even while a writer prepares the next revision.
//!
//! Layout inside a record directory:
//!
//! ```text
//! manifest.json                    record ID, revision, file references, sync cursors
//! write.lock                       OS lock used to allow only one writer
//! observations/<n>.jsonl           session log, read up to its committed byte length
//! derivations/<n>.r<revision>.json  derivation results for a session and store revision
//! ```
//!
//! `<n>` is the number assigned to a session on its first write. Using this
//! number keeps source-provided session IDs out of file paths.
//!
//! Keep `write.lock` in place. Its presence does not mean a writer is active;
//! the operating-system lock controls access. Deleting the file would let a
//! second writer create and lock a different file while the first still runs.

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

/// Returns the observation log path for a numbered session.
fn log_path(number: u64) -> String {
    format!("observations/{number}.jsonl")
}

/// Returns the manifest document key for a numbered session's derivations.
fn derivations_document(number: u64) -> String {
    format!("derivations/{number}")
}

/// Failure to create, read, or write a record.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// An I/O operation failed.
    #[error("i/o error at {path}: {source}")]
    Io {
        /// File or directory on which the operation failed.
        path: PathBuf,
        /// I/O error returned by the failed operation.
        #[source]
        source: io::Error,
    },
    /// Record data could not be encoded as JSON or decoded from it.
    #[error("invalid json at {path}: {source}")]
    Json {
        /// File being read or written.
        path: PathBuf,
        /// JSON serialization or deserialization error.
        #[source]
        source: serde_json::Error,
    },
    /// The record path has no `manifest.json` file.
    #[error("{0} is not a record directory (no manifest.json)")]
    NotARecord(PathBuf),
    /// A manifest already exists where a new record was requested.
    #[error("{0} already contains a record")]
    AlreadyExists(PathBuf),
    /// Another writer holds the lock.
    #[error("record at {0} is locked by another writer")]
    Locked(PathBuf),
    /// The on-disk revision differs from the transaction's starting revision.
    #[error("manifest revision changed under the writer (expected {expected}, found {found})")]
    Conflict {
        /// Revision the writer started from.
        expected: u64,
        /// Revision read from disk during commit.
        found: u64,
    },
    /// Stored files are inconsistent with the manifest, such as a log shorter
    /// than its committed byte length.
    #[error("record is corrupt: {0}")]
    Corrupt(String),
    /// The manifest's schema version is not supported by this version of the crate.
    #[error("record schema version {found} is not supported (this binary supports {supported})")]
    UnsupportedSchema {
        /// Schema version found in the manifest.
        found: u32,
        /// Schema version this crate reads and writes.
        supported: u32,
    },
}

/// Adds the affected path to an I/O error.
pub(crate) fn io_error(path: &Path, source: io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Path to a record directory, used to create snapshots and write transactions.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Creates a record at `root` with a random ID and the supplied checkout association.
    ///
    /// Creates missing directories and an empty manifest at revision zero.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if `manifest.json` already exists,
    /// [`StoreError::Json`] if serialization fails, or [`StoreError::Io`] if a
    /// directory or file operation fails.
    pub fn create(root: &Path, association: Association) -> Result<Self, StoreError> {
        Self::create_with_id(root, RecordId::generate(), association)
    }

    /// Creates a record at `root` with the supplied ID and checkout association.
    ///
    /// Use this when `root` includes an ID the caller has already generated.
    /// The initial manifest is empty and starts at revision zero.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if `manifest.json` already exists,
    /// [`StoreError::Json`] if serialization fails, or [`StoreError::Io`] if a
    /// directory or file operation fails.
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

    /// Returns a handle after checking that `root/manifest.json` is a file.
    ///
    /// This checks file metadata only. [`Self::manifest`] reads and validates
    /// the manifest contents.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotARecord`] if the path is missing, `root` is not
    /// a directory, or the manifest is not a file. Returns [`StoreError::Io`]
    /// for other metadata errors, such as denied permission.
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

    /// Returns the record directory path.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads the current manifest from disk and checks its schema version.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotARecord`] if the manifest is missing,
    /// [`StoreError::Io`] if it cannot be read, [`StoreError::Json`] if it
    /// cannot be parsed, or [`StoreError::UnsupportedSchema`] if its version
    /// differs from [`SCHEMA_VERSION`].
    pub fn manifest(&self) -> Result<Manifest, StoreError> {
        manifest::read(&self.root)
    }

    /// Reads the current manifest and creates a snapshot that keeps that revision.
    ///
    /// Observation logs and derivation documents are read when requested.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::manifest`].
    pub fn snapshot(&self) -> Result<RecordSnapshot, StoreError> {
        Ok(RecordSnapshot::new(self.root.clone(), self.manifest()?))
    }

    /// Acquires the writer lock and starts a transaction at the current revision.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Locked`] if another writer holds the lock,
    /// [`StoreError::Io`] if the lock file cannot be opened or locked, or any
    /// error returned by [`Self::manifest`].
    pub fn begin(&self) -> Result<WriteTx<'_>, StoreError> {
        WriteTx::begin(self)
    }
}
