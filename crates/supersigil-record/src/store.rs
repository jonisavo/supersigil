//! File-backed store whose manifest pins exactly one revision.
//!
//! Layout inside a record directory:
//!
//! ```text
//! manifest.json                       identity, revision, pins, cursors
//! write.lock                          created by the first writer, never removed
//! observations/<session>/events.jsonl append-only log, pinned by byte length
//! <logical>.r<revision>.json          immutable documents, pinned by name
//! ```
//!
//! Only the operating-system lock held on `write.lock` excludes writers; the
//! file's presence means nothing. Deleting it while a writer runs would let a
//! second writer lock a new file, so never clean it up.
//!
//! The namespaces are disjoint by suffix. Log names must end in `.jsonl`;
//! document files end in `.r<revision>.json`; the store's own files
//! (`manifest.json`, its temporary files, and `write.lock`) end in neither
//! `.jsonl` nor a revision suffix. So a log can never alias a document of any
//! revision or a store file, and no document can alias either.

mod manifest;
mod snapshot;
mod write;

use std::fmt::Write as _;
use std::io;
use std::path::{Component, Path, PathBuf};

pub use manifest::{Association, Manifest, SCHEMA_VERSION, SourceCursor};
pub use snapshot::RecordSnapshot;
pub use write::WriteTx;

use crate::ids::{RecordId, SessionId};

/// File name of the manifest inside a record directory.
pub const MANIFEST_FILE: &str = "manifest.json";
/// File name of the writer lock inside a record directory.
pub const LOCK_FILE: &str = "write.lock";

/// Encodes a session id into one safe path component: every byte outside
/// `a-z 0-9 _ -` becomes `%XX`, so an id taken from a transcript can never
/// contain a separator or spell `..`. Uppercase letters are escaped too, so
/// two ids that differ only in case stay two files on a case-insensitive
/// file system: the only uppercase letters in a key are escape hex digits.
#[must_use]
pub fn storage_key(session: &SessionId) -> String {
    let mut key = String::new();
    for byte in session.as_str().bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-' {
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

/// Checks that a log or document name is canonical: `/`-separated segments,
/// each non-empty and neither `.` nor `..`, with no `\`, `:`, or NUL byte
/// and no leading `/`. The raw string is checked rather than only
/// `Path::components()`, which silently drops interior `.` and repeated
/// separators and so would let two spellings name one file.
///
/// The components are checked as well: a name the platform parses as a
/// drive prefix or a rooted path (`C:/x`, `C:x`, `//server/share`) would make
/// `root.join(name)` discard the record root. Rejecting `:` outright makes
/// the drive forms invalid on every platform, not only where they parse.
pub(crate) fn validate_name(name: &str) -> Result<(), StoreError> {
    let canonical = !name.is_empty()
        && !name.starts_with('/')
        && name.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.contains(['\\', '\0', ':'])
        })
        && Path::new(name)
            .components()
            .all(|component| !matches!(component, Component::Prefix(_) | Component::RootDir));
    if canonical {
        Ok(())
    } else {
        Err(StoreError::InvalidName(name.to_owned()))
    }
}

/// Suffix of every log name. Compared case-sensitively on purpose: names
/// are canonical, so `.JSONL` is not a log.
const LOG_SUFFIX: &str = ".jsonl";

/// Whether `name` is in the log namespace: it ends in `.jsonl`, which no
/// document or store file does.
pub(crate) fn is_log_name(name: &str) -> bool {
    name.ends_with(LOG_SUFFIX)
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
