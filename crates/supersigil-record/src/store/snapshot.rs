//! A read view pinned to one manifest.

use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use super::{Manifest, StoreError, decode_storage_key, io_error, observations_log, validate_name};
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

    /// Lines of a log up to its committed length. An unpinned log is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is shorter than its pinned length or
    /// cannot be read.
    pub fn read_log(&self, log: &str) -> Result<Vec<Vec<u8>>, StoreError> {
        validate_name(log)?;
        let Some(&pinned) = self.manifest.logs.get(log) else {
            return Ok(Vec::new());
        };
        let path = self.root.join(log);
        let bytes = read_prefix(&path, pinned)?;
        let mut lines: Vec<Vec<u8>> = bytes.split(|b| *b == b'\n').map(<[u8]>::to_vec).collect();
        // Every line ends in a newline, so the last segment is always empty.
        if lines.last().is_some_and(Vec::is_empty) {
            lines.pop();
        }
        Ok(lines)
    }

    /// Bytes of a logical document at this revision, if it exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the pinned file cannot be read.
    pub fn read_document(&self, logical: &str) -> Result<Option<Vec<u8>>, StoreError> {
        validate_name(logical)?;
        let Some(rel) = self.manifest.documents.get(logical) else {
            return Ok(None);
        };
        validate_name(rel)?;
        let path = self.root.join(rel);
        std::fs::read(&path)
            .map(Some)
            .map_err(|e| io_error(&path, e))
    }

    /// Sessions that have an observation log in this revision.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionId> {
        self.manifest
            .logs
            .keys()
            .filter_map(|log| {
                log.strip_prefix("observations/")?
                    .strip_suffix("/events.jsonl")
                    .map(decode_storage_key)
            })
            .collect()
    }

    /// All observations of a session, in log order.
    ///
    /// # Errors
    ///
    /// Returns an error if the log cannot be read or a line is not a valid
    /// observation.
    pub fn observations(&self, session: &SessionId) -> Result<Vec<Observation>, StoreError> {
        let log = observations_log(session);
        let path = self.root.join(&log);
        self.read_log(&log)?
            .iter()
            .map(|line| {
                serde_json::from_slice(line).map_err(|source| StoreError::Json {
                    path: path.clone(),
                    source,
                })
            })
            .collect()
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
