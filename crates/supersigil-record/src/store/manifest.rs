//! The manifest: identity, revision, and what the revision pins.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{MANIFEST_FILE, StoreError, io_error};
use crate::ids::{RecordId, Revision, SessionId};

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

pub(super) fn read(root: &Path) -> Result<Manifest, StoreError> {
    let path = root.join(MANIFEST_FILE);
    let bytes = fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StoreError::NotARecord(root.to_path_buf())
        } else {
            io_error(&path, e)
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|source| StoreError::Json { path, source })
}

/// Publishes the initial manifest atomically and without overwriting.
///
/// The bytes go to `manifest.json.<record id>.tmp` first and are fsynced,
/// then the temporary file is hard-linked to `manifest.json`. Linking fails
/// with `AlreadyExists` when a manifest is there, so of two racing creators
/// exactly one succeeds, and a reader never sees a partially written
/// manifest. A creator killed before linking leaves only its temporary file,
/// which does not make the directory a record.
pub(super) fn create(root: &Path, manifest: &Manifest) -> Result<(), StoreError> {
    let path = root.join(MANIFEST_FILE);
    let tmp_path = root.join(format!(
        "{MANIFEST_FILE}.{}.tmp",
        manifest.record_id.as_str()
    ));
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|source| StoreError::Json {
        path: path.clone(),
        source,
    })?;
    write_synced(&tmp_path, &bytes)?;
    let linked = fs::hard_link(&tmp_path, &path);
    // The temporary name is private to this creator and harmless if it
    // survives, so failing to remove it does not fail the creation.
    let _ = fs::remove_file(&tmp_path);
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
