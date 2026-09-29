//! The manifest: identity, revision, and what the revision pins.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
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

/// Writes the initial manifest with `create_new`, so of two racing creators
/// exactly one succeeds and the other gets [`StoreError::AlreadyExists`].
pub(super) fn create(root: &Path, manifest: &Manifest) -> Result<(), StoreError> {
    let path = root.join(MANIFEST_FILE);
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|source| StoreError::Json {
        path: path.clone(),
        source,
    })?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                StoreError::AlreadyExists(root.to_path_buf())
            } else {
                io_error(&path, e)
            }
        })?;
    file.write_all(&bytes).map_err(|e| io_error(&path, e))?;
    file.sync_all().map_err(|e| io_error(&path, e))?;
    drop(file);
    sync_dir(root);
    Ok(())
}

/// Writes the manifest to a temporary file, fsyncs it, renames it into
/// place, and fsyncs the directory where the platform allows.
pub(super) fn publish(root: &Path, manifest: &Manifest) -> Result<(), StoreError> {
    let final_path = root.join(MANIFEST_FILE);
    let tmp_path = root.join(format!("{MANIFEST_FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|source| StoreError::Json {
        path: final_path.clone(),
        source,
    })?;
    let mut file = File::create(&tmp_path).map_err(|e| io_error(&tmp_path, e))?;
    file.write_all(&bytes).map_err(|e| io_error(&tmp_path, e))?;
    file.sync_all().map_err(|e| io_error(&tmp_path, e))?;
    drop(file);
    fs::rename(&tmp_path, &final_path).map_err(|e| io_error(&final_path, e))?;
    sync_dir(root);
    Ok(())
}

/// Best-effort directory fsync; directories cannot be opened on every
/// platform, and a failure here does not lose data already fsynced.
pub(super) fn sync_dir(dir: &Path) {
    if let Ok(handle) = File::open(dir) {
        let _ = handle.sync_all();
    }
}
