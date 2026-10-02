//! Manifest data and the file operations that publish a revision.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{MANIFEST_FILE, StoreError, io_error};
use crate::ids::{ContentId, RecordId, Revision, SessionId};

/// Manifest schema version supported for reading and writing.
pub const SCHEMA_VERSION: u32 = 2;

/// A checkout path used to find this record.
///
/// Adding an association does not merge the histories of separate records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Association {
    /// Canonical path of a main checkout or linked git worktree.
    pub checkout: PathBuf,
}

/// Position at which the next sync should resume reading a transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCursor {
    /// Byte offset of the next unread line, or the file length if fully read.
    pub offset: u64,
    /// Zero-based line position at `offset`.
    pub next_ordinal: u64,
    /// Session ID, once the transcript has identified its session.
    pub session: Option<SessionId>,
    /// Hash of the transcript bytes before `offset`.
    /// Sync checks this hash to detect rewritten transcripts, including
    /// replacements of the same length. A mismatch causes a full reread.
    pub prefix_hash: ContentId,
}

/// Record metadata and the file contents committed in one revision.
///
/// Log lengths and document file names define what readers can see. Writers
/// replace `manifest.json` after writing and syncing the referenced files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// ID shared by all revisions of this record.
    pub record_id: RecordId,
    /// Schema version of the files this manifest describes.
    pub schema_version: u32,
    /// Revision number, incremented on every commit.
    pub revision: Revision,
    /// Checkouts this record is associated with.
    pub associations: Vec<Association>,
    /// Session IDs mapped to the numbers used in their file names.
    /// The store assigns each number on the session's first write.
    pub sessions: BTreeMap<SessionId, u64>,
    /// Log paths relative to the record directory, mapped to committed byte lengths.
    pub logs: BTreeMap<String, u64>,
    /// Document keys mapped to files relative to the record directory, for
    /// example `derivations/0` mapped to `derivations/0.r3.json`.
    pub documents: BTreeMap<String, String>,
    /// Transcript paths mapped to the positions where sync should resume.
    pub cursors: BTreeMap<String, SourceCursor>,
}

impl Manifest {
    /// Creates a revision-zero manifest with one checkout and no session data.
    #[must_use]
    pub fn new(record_id: RecordId, association: Association) -> Self {
        Self {
            record_id,
            schema_version: SCHEMA_VERSION,
            revision: Revision::ZERO,
            associations: vec![association],
            sessions: BTreeMap::new(),
            logs: BTreeMap::new(),
            documents: BTreeMap::new(),
            cursors: BTreeMap::new(),
        }
    }
}

/// The one manifest field every layout shares, read before the rest.
#[derive(Deserialize)]
struct Versioned {
    schema_version: u32,
}

/// Reads the manifest and checks that its version equals [`SCHEMA_VERSION`].
///
/// The version is read alone first, so a record in another layout fails on
/// its version and not on whichever field that layout lacks. Rejecting
/// other versions also prevents a writer from dropping fields it does not
/// understand when it serializes the manifest again.
///
/// # Errors
///
/// Returns [`StoreError::NotARecord`] for a missing manifest,
/// [`StoreError::Io`] for other read failures, [`StoreError::Json`] for invalid
/// manifest JSON, or [`StoreError::UnsupportedSchema`] for a different version.
pub(super) fn read(root: &Path) -> Result<Manifest, StoreError> {
    let path = root.join(MANIFEST_FILE);
    let bytes = fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StoreError::NotARecord(root.to_path_buf())
        } else {
            io_error(&path, e)
        }
    })?;
    let json = |source| StoreError::Json {
        path: path.clone(),
        source,
    };
    let versioned: Versioned = serde_json::from_slice(&bytes).map_err(json)?;
    if versioned.schema_version != SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchema {
            root: root.to_path_buf(),
            found: versioned.schema_version,
            supported: SCHEMA_VERSION,
        });
    }
    serde_json::from_slice(&bytes).map_err(json)
}

/// Publishes a complete initial manifest without replacing an existing one.
///
/// Writes and syncs `manifest.json.<uuid>.tmp`, then hard-links it to
/// `manifest.json`. If two creators race, only one can create that link.
/// Removes the temporary name after the link attempt. An interrupted creator
/// may leave a temporary file, which later attempts do not reuse.
///
/// # Errors
///
/// Returns [`StoreError::AlreadyExists`] if the manifest already exists,
/// [`StoreError::Json`] if serialization fails, or [`StoreError::Io`] if a
/// file or directory operation fails.
pub(super) fn create(root: &Path, manifest: &Manifest) -> Result<(), StoreError> {
    let path = root.join(MANIFEST_FILE);
    let tmp_path = root.join(format!("{MANIFEST_FILE}.{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|source| StoreError::Json {
        path: path.clone(),
        source,
    })?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .map_err(|e| io_error(&tmp_path, e))?;
    file.write_all(&bytes).map_err(|e| io_error(&tmp_path, e))?;
    file.sync_all().map_err(|e| io_error(&tmp_path, e))?;
    drop(file);
    let linked = fs::hard_link(&tmp_path, &path);
    fs::remove_file(&tmp_path).map_err(|e| io_error(&tmp_path, e))?;
    linked.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            StoreError::AlreadyExists(root.to_path_buf())
        } else {
            io_error(&path, e)
        }
    })?;
    sync_dir(root)
}

/// Replaces the manifest by writing and syncing a temporary file, renaming
/// it to `manifest.json`, then syncing the directory.
///
/// # Errors
///
/// Returns [`StoreError::Json`] if serialization fails or [`StoreError::Io`]
/// if writing, renaming, or syncing fails.
pub(super) fn publish(root: &Path, manifest: &Manifest) -> Result<(), StoreError> {
    let final_path = root.join(MANIFEST_FILE);
    let tmp_path = root.join(format!("{MANIFEST_FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|source| StoreError::Json {
        path: final_path.clone(),
        source,
    })?;
    write_synced(&tmp_path, &bytes)?;
    fs::rename(&tmp_path, &final_path).map_err(|e| io_error(&final_path, e))?;
    sync_dir(root)
}

/// Replaces the contents of `path` with `bytes` and syncs the file to disk.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if creating, writing, or syncing the file fails.
fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut file = File::create(path).map_err(|e| io_error(path, e))?;
    file.write_all(bytes).map_err(|e| io_error(path, e))?;
    file.sync_all().map_err(|e| io_error(path, e))
}

/// Syncs a directory's entries to disk.
///
/// On Windows, a failure to open the directory is ignored because directories
/// cannot be opened as ordinary files there.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if opening fails on a non-Windows platform, or
/// if syncing an opened directory fails on any platform.
pub(super) fn sync_dir(dir: &Path) -> Result<(), StoreError> {
    match File::open(dir) {
        Ok(handle) => handle.sync_all().map_err(|e| io_error(dir, e)),
        Err(_) if cfg!(windows) => Ok(()),
        Err(e) => Err(io_error(dir, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_dir_syncs_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        sync_dir(dir.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sync_dir_of_a_missing_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            sync_dir(&dir.path().join("missing")),
            Err(StoreError::Io { .. })
        ));
    }
}
