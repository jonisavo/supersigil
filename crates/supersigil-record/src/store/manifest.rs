//! The manifest: identity, revision, and what the revision pins.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{MANIFEST_FILE, StoreError, io_error};
use crate::ids::{ContentId, RecordId, Revision, SessionId};

/// Schema version written into new manifests.
pub const SCHEMA_VERSION: u32 = 1;

/// A checkout this record is associated with. Associations locate a record;
/// they never merge records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Association {
    /// Canonical path of the checkout (a worktree or the main working tree).
    pub checkout: PathBuf,
}

/// Where sync left off in one transcript file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCursor {
    /// Byte offset of the first unconsumed byte.
    pub offset: u64,
    /// Ordinal to assign to the next record.
    pub next_ordinal: u64,
    /// Session the transcript belongs to, once known.
    pub session: Option<SessionId>,
    /// Content id of the consumed prefix, the transcript's bytes up to
    /// `offset`. Sync compares it with the file before resuming, so a
    /// transcript rewritten in place, even to the same length, is read again
    /// from the start instead of being resumed inside unrelated bytes.
    /// Absent in cursors written before it existed; sync does not trust such
    /// a cursor past offset zero and reads the transcript from the start.
    #[serde(default)]
    pub prefix_hash: Option<ContentId>,
}

/// One revision of a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Record identity.
    pub record_id: RecordId,
    /// Schema version of the files this manifest describes.
    pub schema_version: u32,
    /// Revision number, incremented on every commit.
    pub revision: Revision,
    /// Checkouts this record is associated with.
    pub associations: Vec<Association>,
    /// Committed byte length per append-only log, keyed by relative path.
    #[serde(default)]
    pub logs: BTreeMap<String, u64>,
    /// Immutable file name per logical document.
    #[serde(default)]
    pub documents: BTreeMap<String, String>,
    /// Sync cursors keyed by transcript path.
    #[serde(default)]
    pub cursors: BTreeMap<String, SourceCursor>,
}

impl Manifest {
    /// A manifest at revision zero with one association.
    #[must_use]
    pub fn new(record_id: RecordId, association: Association) -> Self {
        Self {
            record_id,
            schema_version: SCHEMA_VERSION,
            revision: Revision::ZERO,
            associations: vec![association],
            logs: BTreeMap::new(),
            documents: BTreeMap::new(),
            cursors: BTreeMap::new(),
        }
    }
}

/// Reads and parses the manifest, refusing any schema version other than
/// [`SCHEMA_VERSION`]. Serde ignores fields it does not know, so a newer
/// manifest read by this binary and written back would silently lose them.
pub(super) fn read(root: &Path) -> Result<Manifest, StoreError> {
    let path = root.join(MANIFEST_FILE);
    let bytes = fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StoreError::NotARecord(root.to_path_buf())
        } else {
            io_error(&path, e)
        }
    })?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|source| StoreError::Json { path, source })?;
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchema {
            found: manifest.schema_version,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(manifest)
}

/// Publishes the initial manifest atomically and without overwriting.
///
/// The bytes go to a temporary file with a name unique to this attempt,
/// `manifest.json.<uuid>.tmp`, created with `create_new` so it can never
/// truncate an existing file, and are fsynced. The temporary file is then
/// hard-linked to `manifest.json`. Linking fails with `AlreadyExists` when a
/// manifest is there, so of two racing creators exactly one succeeds, and a
/// reader never sees a partially written manifest. The temporary link is
/// removed in either case; a creator killed before that leaves only a file
/// no later creator will ever open again.
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

/// Writes the manifest to a temporary file, fsyncs it, renames it into
/// place, and fsyncs the directory.
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

/// Creates or truncates `path`, writes `bytes`, and fsyncs the file.
fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut file = File::create(path).map_err(|e| io_error(path, e))?;
    file.write_all(bytes).map_err(|e| io_error(path, e))?;
    file.sync_all().map_err(|e| io_error(path, e))
}

/// Fsyncs a directory so the entries created in it survive a power loss.
///
/// Windows cannot open a directory as a file, so there a failure to open is
/// ignored; everywhere else a failure to open or to sync is an error, since
/// a manifest published after it could reference entries that are lost.
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
