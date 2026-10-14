//! Manifest data and how it is read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::StoreError;
use crate::files;
use crate::ids::{ContentId, RecordId, Revision, SessionId};
use crate::time::UtcTime;

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
    /// Replaced document paths relative to the record directory, mapped to
    /// their replacement times. Commits delete eligible files more than
    /// [`GRACE_SECONDS`] after replacement and drop their entries. Cleanup
    /// keeps files still needed by readers and retries undeletable files.
    /// It drops invalid entries without deleting files ([`WriteTx::commit`]).
    /// Serialization omits this map when empty.
    ///
    /// [`GRACE_SECONDS`]: crate::time::GRACE_SECONDS
    /// [`WriteTx::commit`]: crate::store::WriteTx::commit
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub superseded: BTreeMap<String, UtcTime>,
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
            superseded: BTreeMap::new(),
            cursors: BTreeMap::new(),
        }
    }
}

/// Reads the manifest and checks that its version equals [`SCHEMA_VERSION`]
/// (see [`files::read_manifest`]).
///
/// # Errors
///
/// Returns [`StoreError::NotARecord`] for a missing manifest,
/// [`StoreError::Io`] for other read failures, [`StoreError::Json`] for invalid
/// manifest JSON, or [`StoreError::UnsupportedSchema`] for a different version.
pub(super) fn read(root: &Path) -> Result<Manifest, StoreError> {
    files::read_manifest(root, SCHEMA_VERSION, |found| {
        StoreError::UnsupportedSchema {
            root: root.to_path_buf(),
            found,
            supported: SCHEMA_VERSION,
        }
    })
}
