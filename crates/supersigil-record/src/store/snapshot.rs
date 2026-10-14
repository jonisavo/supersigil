//! Reads the log bytes and document files listed in one manifest.

use std::fs;
use std::path::PathBuf;

use super::{Manifest, StoreError, derivations_document, io_error, log_path};
use crate::derivations::DerivationSet;
use crate::files;
use crate::ids::{Revision, SessionId};
use crate::observations::Observation;

/// Reads a record at the revision captured by its manifest.
///
/// Log reads stop at the manifest's committed byte lengths. Document reads
/// use the file names listed in that same manifest. Later commits do not
/// change this snapshot's results.
#[derive(Debug, Clone)]
pub struct RecordSnapshot {
    root: PathBuf,
    manifest: Manifest,
}

impl RecordSnapshot {
    /// Creates a snapshot using the supplied directory and manifest.
    pub(super) fn new(root: PathBuf, manifest: Manifest) -> Self {
        Self { root, manifest }
    }

    /// Returns the manifest captured when this snapshot was created.
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Returns this snapshot's record revision.
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.manifest.revision
    }

    /// Reads the session's derivation document referenced by this snapshot.
    ///
    /// Returns `None` if the manifest lists no document for the session.
    /// The derivations may have been computed at an earlier observation revision.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] if the file cannot be read or
    /// [`StoreError::Json`] if its contents cannot be parsed as a derivation set.
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

    /// Returns the session IDs listed in this snapshot's manifest, sorted by ID.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionId> {
        self.manifest.sessions.keys().cloned().collect()
    }

    /// Reads a session's observations in append order, up to the committed byte length.
    ///
    /// Returns an empty vector if this snapshot has no log for the session.
    /// Bytes appended after this snapshot's revision are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Corrupt`] if the log is shorter than its committed
    /// length, [`StoreError::Io`] if it cannot be read, or [`StoreError::Json`]
    /// if its contents cannot be parsed as observations.
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
        let bytes = files::read_prefix(&path, pinned)?;
        serde_json::Deserializer::from_slice(&bytes)
            .into_iter()
            .collect::<Result<_, _>>()
            .map_err(|source| StoreError::Json { path, source })
    }
}
