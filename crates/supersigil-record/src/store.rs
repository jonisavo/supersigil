//! File-backed store whose manifest pins exactly one revision.
//!
//! Layout inside a record directory:
//!
//! ```text
//! manifest.json                       identity, revision, pins, cursors
//! write.lock                          present only while a writer is active
//! observations/<session>/events.jsonl append-only log, pinned by byte length
//! <logical>.r<revision>.json          immutable documents, pinned by name
//! ```

mod manifest;
mod snapshot;
mod write;

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

pub use manifest::{Association, Manifest, SCHEMA_VERSION, SourceCursor};
pub use snapshot::RecordSnapshot;
pub use write::WriteTx;

use crate::ids::{RecordId, SessionId};

/// File name of the manifest inside a record directory.
pub const MANIFEST_FILE: &str = "manifest.json";
/// File name of the writer lock inside a record directory.
pub const LOCK_FILE: &str = "write.lock";

/// Names the store owns; a log or document may never use them.
const RESERVED_NAMES: [&str; 3] = [MANIFEST_FILE, "manifest.json.tmp", LOCK_FILE];

/// Encodes a session id into one safe path component: every byte outside
/// `A-Z a-z 0-9 _ -` becomes `%XX`, so an id taken from a transcript can never
/// contain a separator or spell `..`.
#[must_use]
pub fn storage_key(session: &SessionId) -> String {
    let mut key = String::new();
    for byte in session.as_str().bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' {
            key.push(char::from(byte));
        } else {
            let _ = write!(key, "%{byte:02X}");
        }
    }
    key
}

/// Inverts [`storage_key`]. A malformed escape is kept as-is.
#[must_use]
pub fn decode_storage_key(key: &str) -> SessionId {
    let bytes = key.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    SessionId::new(String::from_utf8_lossy(&out).into_owned())
}

/// Relative path of a session's observation log.
#[must_use]
pub fn observations_log(session: &SessionId) -> String {
    format!("observations/{}/events.jsonl", storage_key(session))
}

/// Checks that a log or document name is a relative path of plain
/// components: no `..`, no root, no `.`, no empty component.
pub(crate) fn validate_name(name: &str) -> Result<(), StoreError> {
    let plain = !name.is_empty()
        && !name.contains("//")
        && !name.ends_with('/')
        && Path::new(name)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
    if plain {
        Ok(())
    } else {
        Err(StoreError::InvalidName(name.to_owned()))
    }
}

/// Rejects the store's own file names.
pub(crate) fn reject_reserved(name: &str) -> Result<(), StoreError> {
    if RESERVED_NAMES.contains(&name) {
        Err(StoreError::InvalidName(name.to_owned()))
    } else {
        Ok(())
    }
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
    /// A log or document name is not a plain relative path.
    #[error("invalid record file name: {0:?}")]
    InvalidName(String),
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
    /// Returns [`StoreError::NotARecord`] if there is no manifest.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        if !root.join(MANIFEST_FILE).is_file() {
            return Err(StoreError::NotARecord(root.to_path_buf()));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
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
    /// Returns an error if the manifest cannot be read or parsed.
    pub fn manifest(&self) -> Result<Manifest, StoreError> {
        manifest::read(&self.root)
    }

    /// A read view pinned to the manifest as it is right now.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be read.
    pub fn snapshot(&self) -> Result<RecordSnapshot, StoreError> {
        Ok(RecordSnapshot::new(self.root.clone(), self.manifest()?))
    }

    /// Starts a write transaction, taking the writer lock.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Locked`] if another writer is active.
    pub fn begin(&self) -> Result<WriteTx<'_>, StoreError> {
        WriteTx::begin(self)
    }
}
