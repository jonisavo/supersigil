//! JSON storage for session observations, computed derivations, and
//! annotations.
//!
//! The manifest lists each log's committed byte length and each document's
//! file name. A snapshot keeps one manifest and reads only those bytes and
//! files, even while a writer prepares the next revision.
//!
//! Layout inside a record directory:
//!
//! ```text
//! manifest.json                    record ID, revision, file references, sync cursors
//! write.lock                       OS lock used to allow only one writer
//! observations/<n>.jsonl           session log, read up to its committed byte length
//! derivations/<n>.r<revision>.json  derivation results for a session and store revision
//! interpretations/<n>.a<revision>.json  one annotation revision of a session
//! ```
//!
//! `<n>` is the number assigned to a session on its first write. Using this
//! number keeps source-provided session IDs out of file paths.
//!
//! Keeps replaced derivation documents for earlier readers and lists them
//! as superseded at the replacement revision's time. Later commits delete
//! eligible documents more than 14 days after replacement. Annotation
//! revisions are never superseded: the manifest lists each under its own
//! key, and cleanup never deletes one.
//!
//! Keep `write.lock` in place. Its presence does not mean a writer is active;
//! the operating-system lock controls access. Deleting the file would let a
//! second writer create and lock a different file while the first still runs.

mod manifest;
mod snapshot;
mod write;

use std::io;
use std::path::{Path, PathBuf};

pub use manifest::{Association, Manifest, SCHEMA_VERSION, SourceCursor};
pub use snapshot::{LogReading, RecordSnapshot, SessionLog};
pub use write::WriteTx;

use crate::files;
use crate::ids::{RecordId, SessionId};
use crate::interpretations::{AnnotationRevision, revision_phrase};
use crate::reviews::BasisId;
use crate::time::UtcTime;

/// File name of the manifest inside a record directory or review store.
pub const MANIFEST_FILE: &str = "manifest.json";
/// File name of the writer lock inside a record directory.
pub const LOCK_FILE: &str = "write.lock";

/// Directory of the observation logs inside a record directory.
const OBSERVATIONS_DIR: &str = "observations";

/// Returns the observation log path for a numbered session.
fn log_path(number: u64) -> String {
    format!("{OBSERVATIONS_DIR}/{number}.jsonl")
}

/// Directory of the derivation documents inside a record directory.
const DERIVATIONS_DIR: &str = "derivations";

/// Returns the manifest document key for a numbered session's derivations.
fn derivations_document(number: u64) -> String {
    format!("{DERIVATIONS_DIR}/{number}")
}

/// Directory of the interpretation documents inside a record directory.
const INTERPRETATIONS_DIR: &str = "interpretations";

/// Returns the manifest document key for one annotation revision of a
/// numbered session.
fn annotation_document(number: u64, revision: AnnotationRevision) -> String {
    format!("{INTERPRETATIONS_DIR}/{number}/{revision}")
}

/// Returns the annotation revisions `manifest` lists for a numbered
/// session, in ascending order.
fn annotation_revisions(manifest: &Manifest, number: u64) -> Vec<AnnotationRevision> {
    let prefix = format!("{INTERPRETATIONS_DIR}/{number}/");
    let mut revisions: Vec<AnnotationRevision> = manifest
        .documents
        .range(prefix.clone()..)
        .map_while(|(key, _)| key.strip_prefix(&prefix))
        .filter_map(|revision| revision.parse().ok().map(AnnotationRevision::new))
        .collect();
    // Keys sort as text, which puts revision 10 before revision 2.
    revisions.sort_unstable();
    revisions
}

/// Failure to create, read, or write a record or a review store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// An I/O operation failed.
    #[error("i/o error at {path}: {source}")]
    Io {
        /// File or directory on which the operation failed.
        path: PathBuf,
        /// I/O error returned by the failed operation.
        #[source]
        source: io::Error,
    },
    /// Stored data could not be encoded as JSON or decoded from it.
    #[error("invalid json at {path}: {source}")]
    Json {
        /// File being read or written.
        path: PathBuf,
        /// JSON serialization or deserialization error.
        #[source]
        source: serde_json::Error,
    },
    /// The path has no `manifest.json` file.
    #[error("{0} is not a record or review store (no manifest.json)")]
    NotARecord(PathBuf),
    /// A manifest already exists where a new record or review store was
    /// requested.
    #[error("{0} already contains a record or review store")]
    AlreadyExists(PathBuf),
    /// Another writer holds the lock of the record or review store at this
    /// path.
    #[error("{0} is locked by another writer")]
    Locked(PathBuf),
    /// The on-disk revision differs from the transaction's starting revision.
    #[error("manifest revision changed under the writer (expected {expected}, found {found})")]
    Conflict {
        /// Revision the writer started from.
        expected: u64,
        /// Revision read from disk during commit.
        found: u64,
    },
    /// Stored files are inconsistent with the manifest, such as a log shorter
    /// than its committed byte length or a basis file that does not hash to
    /// its id.
    #[error("store is corrupt: {0}")]
    Corrupt(String),
    /// A document the manifest lists is gone from disk.
    #[error("{0} is missing, though the record's manifest lists it")]
    MissingDocument(PathBuf),
    /// The record holds no observations of the session, so it cannot be
    /// annotated.
    #[error("session {0} has no observations in this record")]
    UnknownSession(SessionId),
    /// An annotation was built on a revision other than the session's
    /// current one. Storing it would supersede a revision its annotator
    /// never saw.
    #[error(
        "annotation based on {}, but {} is current",
        revision_phrase(*based_on),
        revision_phrase(*current)
    )]
    StaleAnnotation {
        /// The revision the annotation was built on, `None` for none.
        based_on: Option<AnnotationRevision>,
        /// The session's current revision, `None` when it has none.
        current: Option<AnnotationRevision>,
    },
    /// The manifest lists no such annotation revision of the session.
    #[error("session {session} has no annotation revision {revision} in this record")]
    NoSuchAnnotation {
        /// The session asked for.
        session: SessionId,
        /// The revision asked for.
        revision: AnnotationRevision,
    },
    /// The manifest version differs from this crate's format. No migration
    /// exists; the record stays unchanged. A newer record needs a newer
    /// reader. An older one can be moved away or deleted; the next sync
    /// then rebuilds what the remaining transcripts hold.
    #[error(
        "record at {} uses record format {found}; this build reads format {supported}. {}",
        root.display(),
        unsupported_advice(*found, *supported)
    )]
    UnsupportedSchema {
        /// Record directory holding the manifest.
        root: PathBuf,
        /// Schema version found in the manifest.
        found: u32,
        /// Schema version this crate reads and writes.
        supported: u32,
    },
    /// The review store's manifest version differs from this crate's
    /// format. The store stays unchanged.
    #[error(
        "review store at {} uses review store format {found}; this build reads format \
         {supported} and left it unchanged",
        root.display()
    )]
    UnsupportedReviewStore {
        /// Review store directory holding the manifest.
        root: PathBuf,
        /// Schema version found in the manifest.
        found: u32,
        /// Schema version this crate reads and writes.
        supported: u32,
    },
    /// Several review stores name one repository. Picking one would hide
    /// the judgments in the others.
    #[error(
        "several review stores name the repository {}: {}. Remove or merge all but one.",
        repository.display(),
        joined(stores.iter().map(|store| store.display()))
    )]
    SeveralReviewStores {
        /// The repository's git common directory.
        repository: PathBuf,
        /// The stores' directories, sorted.
        stores: Vec<PathBuf>,
    },
    /// No basis in the manifest's `bases` has this id or id prefix.
    #[error("no saved review matches {0}")]
    NoSuchBasis(String),
    /// Several bases in the manifest's `bases` start with this prefix.
    #[error(
        "{prefix} matches several saved reviews: {}; give more digits",
        joined(candidates.iter())
    )]
    AmbiguousBasis {
        /// The prefix as given.
        prefix: String,
        /// The matching ids, sorted.
        candidates: Vec<BasisId>,
    },
    /// A review id prefix shorter than 8 digits or not hexadecimal.
    #[error("{0:?} is not a review id: give at least 8 of its hexadecimal digits")]
    InvalidBasisPrefix(String),
}

/// Advice for a record of format `found` refused by a build reading
/// `supported`. Preserve observations first: their transcripts may be
/// gone.
fn unsupported_advice(found: u32, supported: u32) -> &'static str {
    if found > supported {
        "A newer supersigil wrote it; use that version to read it. This build left it unchanged."
    } else {
        "To keep its observations, move that directory out of the records directory; to \
         discard them, delete it. The next sync then reads the transcripts that still exist."
    }
}

/// Joins displayed items with commas.
fn joined(items: impl Iterator<Item = impl std::fmt::Display>) -> String {
    items
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Adds the affected path to an I/O error.
pub(crate) fn io_error(path: &Path, source: io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Lists record or review-store entries in `dir`, sorted by path. The
/// returned iterator opens each with `open`, such as [`Store::open`]. Skips
/// entries without a manifest, including directory locks, and yields all other
/// errors so unreadable stores remain visible. A missing `dir` has no stores.
///
/// # Errors
///
/// Returns the I/O error of listing `dir` when it exists but cannot be
/// read. The iterator yields each error of `open` other than
/// [`StoreError::NotARecord`].
pub fn stores_in<T>(
    dir: &Path,
    open: impl Fn(&Path) -> Result<T, StoreError>,
) -> io::Result<impl Iterator<Item = Result<T, StoreError>>> {
    let mut candidates = Vec::new();
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                candidates.push(entry?.path());
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    candidates.sort();
    Ok(candidates
        .into_iter()
        .filter_map(move |candidate| match open(&candidate) {
            Err(StoreError::NotARecord(_)) => None,
            opened => Some(opened),
        }))
}

/// Path to a record directory, used to create snapshots and write transactions.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Creates a record at `root` with a random ID and the supplied checkout association.
    ///
    /// Creates missing directories and an empty manifest at revision zero.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if `manifest.json` already exists,
    /// [`StoreError::Json`] if serialization fails, or [`StoreError::Io`] if a
    /// directory or file operation fails.
    pub fn create(root: &Path, association: Association) -> Result<Self, StoreError> {
        Self::create_with_id(root, RecordId::generate(), association)
    }

    /// Creates a record at `root` with the supplied ID and checkout association.
    ///
    /// Use this when `root` includes an ID the caller has already generated.
    /// The initial manifest is empty and starts at revision zero.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if `manifest.json` already exists,
    /// [`StoreError::Json`] if serialization fails, or [`StoreError::Io`] if a
    /// directory or file operation fails.
    pub fn create_with_id(
        root: &Path,
        record_id: RecordId,
        association: Association,
    ) -> Result<Self, StoreError> {
        std::fs::create_dir_all(root).map_err(|e| io_error(root, e))?;
        files::create_manifest(root, &Manifest::new(record_id, association))?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Returns a handle after checking that `root/manifest.json` is a file.
    ///
    /// This checks file metadata only. [`Self::manifest`] reads and validates
    /// the manifest contents.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotARecord`] if the path is missing, `root` is not
    /// a directory, or the manifest is not a file. Returns [`StoreError::Io`]
    /// for other metadata errors, such as denied permission.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        files::check_manifest(root)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Returns the record directory path.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads the current manifest from disk and checks its schema version.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotARecord`] if the manifest is missing,
    /// [`StoreError::Io`] if it cannot be read, [`StoreError::Json`] if it
    /// cannot be parsed, or [`StoreError::UnsupportedSchema`] if its version
    /// differs from [`SCHEMA_VERSION`].
    pub fn manifest(&self) -> Result<Manifest, StoreError> {
        manifest::read(&self.root)
    }

    /// Reads the current manifest and creates a snapshot that keeps that revision.
    ///
    /// Observation logs and derivation documents are read when requested.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::manifest`].
    pub fn snapshot(&self) -> Result<RecordSnapshot, StoreError> {
        Ok(RecordSnapshot::new(self.root.clone(), self.manifest()?))
    }

    /// Acquires the writer lock and starts a transaction at the current
    /// revision, timed by the system clock (see [`Self::begin_at`]).
    ///
    /// # Errors
    ///
    /// As [`Self::begin_at`].
    pub fn begin(&self) -> Result<WriteTx<'_>, StoreError> {
        self.begin_at(UtcTime::now())
    }

    /// Acquires the writer lock and starts a transaction at the current
    /// revision, using `now` as its publication time. Lists replaced documents
    /// as superseded at `now` and deletes eligible documents superseded more
    /// than [`GRACE_SECONDS`] earlier at commit.
    ///
    /// [`GRACE_SECONDS`]: crate::time::GRACE_SECONDS
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Locked`] if another writer holds the lock,
    /// [`StoreError::Io`] if the lock file cannot be opened or locked, or any
    /// error returned by [`Self::manifest`].
    pub fn begin_at(&self, now: UtcTime) -> Result<WriteTx<'_>, StoreError> {
        WriteTx::begin(self, now)
    }
}
