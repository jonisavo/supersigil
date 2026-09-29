//! A read view pinned to one manifest.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use super::{Manifest, StoreError, derivations_document, io_error, log_path};
use crate::derivations::DerivationSet;
use crate::ids::{Revision, SessionId};
use crate::observations::Observation;

/// Reads only what its manifest pins, so it never mixes revisions.
#[derive(Debug, Clone)]
pub struct RecordSnapshot {
    root: PathBuf,
    manifest: Manifest,
}

impl RecordSnapshot {
    pub(super) fn new(root: PathBuf, manifest: Manifest) -> Self {
        Self { root, manifest }
    }

    /// The pinned manifest.
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The pinned revision.
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.manifest.revision
    }

    /// A session's derivations at this revision, if any were written.
    ///
    /// # Errors
    ///
    /// Returns an error if the document cannot be read or parsed.
    pub fn derivations(&self, session: &SessionId) -> Result<Option<DerivationSet>, StoreError> {
        let Some(rel) = self
            .manifest
            .sessions
            .get(session)
            .and_then(|&number| self.manifest.documents.get(&derivations_document(number)))
        else {
            return Ok(None);
        };
        let path = self.root.join(rel);
        let bytes = fs::read(&path).map_err(|e| io_error(&path, e))?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| StoreError::Json { path, source })
    }

    /// Sessions the store has numbered by this revision, in id order.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionId> {
        self.manifest.sessions.keys().cloned().collect()
    }

    /// All observations of a session up to the log's committed length, in
    /// log order. A session without a log in this revision has none.
    ///
    /// # Errors
    ///
    /// Returns an error if the log is shorter than its pinned length or
    /// cannot be read, or a line is not a valid observation.
    pub fn observations(&self, session: &SessionId) -> Result<Vec<Observation>, StoreError> {
        let Some((log, &pinned)) = self
            .manifest
            .sessions
            .get(session)
            .and_then(|&number| self.manifest.logs.get_key_value(&log_path(number)))
        else {
            return Ok(Vec::new());
        };
        let path = self.root.join(log);
        let bytes = read_prefix(&path, pinned)?;
        serde_json::Deserializer::from_slice(&bytes)
            .into_iter()
            .collect::<Result<_, _>>()
            .map_err(|source| StoreError::Json { path, source })
    }
}

fn read_prefix(path: &Path, len: u64) -> Result<Vec<u8>, StoreError> {
    let file = File::open(path).map_err(|e| io_error(path, e))?;
    let mut bytes = Vec::new();
    file.take(len)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error(path, e))?;
    if bytes.len() as u64 != len {
        return Err(StoreError::Corrupt(format!(
            "{} is shorter ({} bytes) than its pinned length ({len})",
            path.display(),
            bytes.len()
        )));
    }
    Ok(bytes)
}
