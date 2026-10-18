//! Reads the log bytes and document files listed in one manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::{fs, io};

use serde::de::DeserializeOwned;
use sha2::{Digest as _, Sha256};

use super::{
    Manifest, StoreError, annotation_document, annotation_revisions, derivations_document,
    io_error, log_path,
};
use crate::derivations::DerivationSet;
use crate::files;
use crate::ids::{Revision, SessionId};
use crate::interpretations::{Annotation, AnnotationRevision};
use crate::observations::Observation;
use crate::reviews::PinnedLog;

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
    /// Returns [`StoreError::MissingDocument`] if the listed file is gone,
    /// [`StoreError::Io`] if it cannot be read, or [`StoreError::Json`] if
    /// its contents cannot be parsed as a derivation set.
    pub fn derivations(&self, session: &SessionId) -> Result<Option<DerivationSet>, StoreError> {
        self.derivation_document(session)
    }

    /// Reads the algorithm version from the session's pinned derivation
    /// document ([`Self::derivations`]) without constructing a derivation set.
    ///
    /// Returns `None` if the manifest lists no document for the session.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::MissingDocument`] if the listed file is gone,
    /// [`StoreError::Io`] if it cannot be read, or [`StoreError::Json`] if
    /// its contents are not a JSON object with an algorithm version.
    pub fn algorithm_version(&self, session: &SessionId) -> Result<Option<u32>, StoreError> {
        /// The one field of a derivation document read.
        #[derive(serde::Deserialize)]
        struct Versioned {
            algorithm_version: u32,
        }
        Ok(self
            .derivation_document::<Versioned>(session)?
            .map(|document| document.algorithm_version))
    }

    /// Parses the session's pinned derivation document as `T`
    /// ([`Self::document`]), returning `None` if the manifest lists none.
    ///
    /// # Errors
    ///
    /// As [`Self::document`].
    fn derivation_document<T: DeserializeOwned>(
        &self,
        session: &SessionId,
    ) -> Result<Option<T>, StoreError> {
        self.manifest
            .sessions
            .get(session)
            .and_then(|&number| self.manifest.documents.get(&derivations_document(number)))
            .map(|rel| self.document(rel))
            .transpose()
    }

    /// Returns the annotation revisions this snapshot's manifest lists for
    /// `session`, in ascending order. Empty if it lists none.
    #[must_use]
    pub fn annotation_revisions(&self, session: &SessionId) -> Vec<AnnotationRevision> {
        self.manifest
            .sessions
            .get(session)
            .map_or_else(Vec::new, |&number| {
                annotation_revisions(&self.manifest, number)
            })
    }

    /// Reads one annotation revision of `session` that this snapshot's
    /// manifest lists.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoSuchAnnotation`] if the manifest does not list
    /// the revision, [`StoreError::MissingDocument`] if its file is gone,
    /// [`StoreError::Io`] if the file cannot be read, or [`StoreError::Json`]
    /// if its contents cannot be parsed as an annotation.
    pub fn annotation(
        &self,
        session: &SessionId,
        revision: AnnotationRevision,
    ) -> Result<Annotation, StoreError> {
        let rel = self
            .manifest
            .sessions
            .get(session)
            .and_then(|&number| {
                self.manifest
                    .documents
                    .get(&annotation_document(number, revision))
            })
            .ok_or_else(|| StoreError::NoSuchAnnotation {
                session: session.clone(),
                revision,
            })?;
        self.document(rel)
    }

    /// Reads the session's current annotation, the highest revision this
    /// snapshot's manifest lists ([`Self::annotation`]). Returns `None` if
    /// it lists none.
    ///
    /// # Errors
    ///
    /// As [`Self::annotation`].
    pub fn current_annotation(
        &self,
        session: &SessionId,
    ) -> Result<Option<Annotation>, StoreError> {
        self.annotation_revisions(session)
            .pop()
            .map(|revision| self.annotation(session, revision))
            .transpose()
    }

    /// Parses the listed document `rel`, a path in the record, as `T`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::MissingDocument`] if the file is gone,
    /// [`StoreError::Io`] if it cannot be read, or [`StoreError::Json`] if
    /// its contents cannot be parsed as a `T`.
    fn document<T: DeserializeOwned>(&self, rel: &str) -> Result<T, StoreError> {
        let path = self.root.join(rel);
        let bytes = fs::read(&path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                StoreError::MissingDocument(path.clone())
            } else {
                io_error(&path, e)
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|source| StoreError::Json { path, source })
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
        let Some((log, length)) = self.log_of(session) else {
            return Ok(Vec::new());
        };
        files::read_log(&self.root.join(log), length)
    }

    /// Reads a session's pinned log bytes once and returns the observations
    /// ([`Self::observations`]), log path, pinned length, and SHA-256 digest.
    /// Later readers can use the digest to detect changes to those bytes.
    ///
    /// Returns `None` if this snapshot has no log for the session.
    ///
    /// # Errors
    ///
    /// As [`Self::observations`].
    pub fn session_log(&self, session: &SessionId) -> Result<Option<SessionLog>, StoreError> {
        let Some((log, length)) = self.log_of(session) else {
            return Ok(None);
        };
        let path = self.root.join(log);
        let bytes = files::read_prefix(&path, length)?;
        let pinned = PinnedLog {
            length,
            sha256: hex::encode(Sha256::digest(&bytes)),
        };
        Ok(Some(SessionLog {
            path: log.to_owned(),
            pinned,
            observations: files::parse_log(&path, &bytes)?,
        }))
    }

    /// Reads the pinned bytes of `log`, a path in the record such as
    /// `observations/0.jsonl`. Hashes prefixes for requested `lengths` within
    /// the pinned length in one pass, producing hex SHA-256 digests.
    ///
    /// Parses observations from the shortest requested length at a line boundary
    /// and records their end offsets. [`LogReading::since`] returns slices of
    /// observations appended after earlier pins.
    ///
    /// Returns `None` if this snapshot pins no log at `log`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Corrupt`] if the log is shorter than its pinned
    /// length, [`StoreError::Io`] if it cannot be read, or
    /// [`StoreError::Json`] if the observations read cannot be parsed.
    pub fn read_log(
        &self,
        log: &str,
        lengths: &BTreeSet<u64>,
    ) -> Result<Option<LogReading>, StoreError> {
        let Some(&length) = self.manifest.logs.get(log) else {
            return Ok(None);
        };
        let path = self.root.join(log);
        let bytes = files::read_prefix(&path, length)?;
        // Keep requested lengths within the bytes read, with their end indexes.
        let within: Vec<(u64, usize)> = lengths
            .iter()
            .filter_map(|&asked| {
                let end = usize::try_from(asked).ok()?;
                (end <= bytes.len()).then_some((asked, end))
            })
            .collect();
        let mut digests = BTreeMap::new();
        let mut prefix = Sha256::new();
        let mut hashed = 0;
        for &(asked, end) in &within {
            prefix.update(&bytes[hashed..end]);
            hashed = end;
            digests.insert(asked, hex::encode(prefix.clone().finalize()));
        }
        // Each observation occupies one line, with no raw newline inside its JSON.
        let from = within
            .iter()
            .map(|&(_, end)| end)
            .find(|&end| end == 0 || bytes[end - 1] == b'\n')
            .unwrap_or(bytes.len());
        let mut ends = Vec::new();
        let mut observations = Vec::new();
        let mut stream = serde_json::Deserializer::from_slice(&bytes[from..]).into_iter();
        while let Some(observation) = stream.next() {
            observations.push(observation.map_err(|source| StoreError::Json {
                path: path.clone(),
                source,
            })?);
            ends.push((from + stream.byte_offset()) as u64);
        }
        Ok(Some(LogReading {
            length,
            digests,
            from: from as u64,
            ends,
            observations,
        }))
    }

    /// The path of `session`'s log and its pinned length, if this snapshot
    /// has a log for the session.
    fn log_of(&self, session: &SessionId) -> Option<(&str, u64)> {
        self.manifest
            .sessions
            .get(session)
            .and_then(|&number| self.manifest.logs.get_key_value(&log_path(number)))
            .map(|(log, &length)| (log.as_str(), length))
    }
}

/// One session's log as a snapshot pins it, read once
/// ([`RecordSnapshot::session_log`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLog {
    /// The log's path in the record, such as `observations/0.jsonl`.
    pub path: String,
    /// The pinned length and the SHA-256 of the pinned bytes.
    pub pinned: PinnedLog,
    /// The observations in the pinned bytes, in append order.
    pub observations: Vec<Observation>,
}

/// A log as a snapshot pins it, read once to compare with what earlier
/// revisions pinned of it ([`RecordSnapshot::read_log`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogReading {
    /// The pinned length.
    length: u64,
    /// For each length asked for within the pinned length, the hex SHA-256
    /// of the log's first that many bytes.
    digests: BTreeMap<u64, String>,
    /// Where the observations read begin.
    from: u64,
    /// The offset where each observation's JSON ends, in append order.
    ends: Vec<u64>,
    /// The observations from `from` up to the pinned length, in append
    /// order.
    observations: Vec<Observation>,
}

impl LogReading {
    /// Returns the hex SHA-256 of the first `length` bytes if that length
    /// was requested and lies within the pinned length.
    #[must_use]
    pub fn digest(&self, length: u64) -> Option<&str> {
        self.digests.get(&length).map(String::as_str)
    }

    /// Returns observations whose JSON ends after an earlier pin's `length`,
    /// in append order. Returns `None` if `length` precedes the parsed portion
    /// or exceeds the pinned length.
    #[must_use]
    pub fn since(&self, length: u64) -> Option<&[Observation]> {
        (self.from..=self.length).contains(&length).then(|| {
            let first = self.ends.partition_point(|&end| end <= length);
            &self.observations[first..]
        })
    }
}
