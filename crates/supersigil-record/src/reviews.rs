//! A repository's saved reviews: each review's basis, which review each
//! worktree selected, and the judgments people recorded on them.
//!
//! One store serves a repository, named by the canonical path of its git
//! common directory, which every worktree shares. Stores live in
//! `<reviews dir>/<store id>/`; [`find`] and [`create_in`] look one up and
//! make one under the caller's lock on `<reviews dir>/.lock`.
//!
//! Layout inside a review store:
//!
//! ```text
//! manifest.json            store ID, revision, repository, judgments length,
//!                          bases with their times, removed bases, selections
//! .lock                    OS lock used to allow only one writer
//! bases/<basis id>.json    a basis, exactly the bytes its ID hashes
//! judgments.jsonl          judgments, read up to the committed length
//! ```
//!
//! The store follows the record store's protocol: a writer takes the lock,
//! drops any judgment bytes beyond the committed length, writes and syncs
//! new files and appended bytes, then publishes the manifest with the next
//! revision by rename. A reader uses one manifest and reads only what it
//! pins. Cleanup removes an unused basis in two writes, so no published
//! manifest names a deleted file (see [`ReviewTx::cleanup`]).

/// Defines a newtype over the 64 lowercase hexadecimal digits of a SHA-256
/// hash, which refuses any other string when deserialized.
macro_rules! sha256_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Wraps a finished SHA-256 hash.
            fn from_digest(digest: impl AsRef<[u8]>) -> Self {
                Self(hex::encode(digest))
            }

            /// Returns the 64 hexadecimal digits.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Returns the first 8 digits, the form output prints.
            #[must_use]
            pub fn short(&self) -> &str {
                &self.0[..8]
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = <String as serde::Deserialize>::deserialize(deserializer)?;
                if $crate::reviews::is_sha256_hex(&text) {
                    Ok(Self(text))
                } else {
                    Err(serde::de::Error::custom(format_args!(
                        "{text:?} is not 64 lowercase hexadecimal digits"
                    )))
                }
            }
        }
    };
}

mod basis;
mod gate;
mod judgment;
mod manifest;
mod path;
mod snapshot;
mod write;

use std::path::{Path, PathBuf};

pub use basis::{
    Algorithms, BASIS_SCHEMA, Basis, BasisBase, BasisCheckout, BasisHeader, BasisId, BasisRecord,
    BasisScope, BasisTarget, CaptureEntry, Explanation, PinnedLog, TargetKind,
};
pub use gate::{Fingerprint, FingerprintPart, GateDetail, GateItem, ItemId, Origin, Subject};
pub use judgment::{Judgment, JudgmentKind, Who, Withdrawal};
pub use manifest::{BasisTimes, ReviewManifest, SCHEMA_VERSION};
pub use path::PathBytes;
pub use snapshot::ReviewSnapshot;
pub use write::ReviewTx;

use crate::files;
use crate::ids::StoreId;
use crate::store::{StoreError, io_error, stores_in};

/// File name of the writer lock inside a review store.
const LOCK_FILE: &str = ".lock";

/// Directory of the basis files inside a review store.
const BASES_DIR: &str = "bases";

/// File name of the judgments log inside a review store.
const JUDGMENTS_FILE: &str = "judgments.jsonl";

/// File name of a basis inside [`BASES_DIR`].
fn basis_file_name(id: &BasisId) -> String {
    format!("{id}.json")
}

/// Reads the basis file at `path`, checking that its bytes hash to `id`.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if the file cannot be read or
/// [`StoreError::Corrupt`] if its bytes do not hash to `id`.
fn read_basis_file(path: &Path, id: &BasisId) -> Result<Vec<u8>, StoreError> {
    let bytes = std::fs::read(path).map_err(|e| io_error(path, e))?;
    if BasisId::of(&bytes) != *id {
        return Err(StoreError::Corrupt(format!(
            "{} does not hash to its basis id",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Whether `name` is one the store writes inside [`BASES_DIR`]: a basis
/// file's ([`basis_file_name`]), or that of a temporary file
/// [`files::replace`] writes one through ([`files::temporary_target`]).
fn written_in_bases(name: &str) -> bool {
    files::temporary_target(name)
        .unwrap_or(name)
        .strip_suffix(".json")
        .is_some_and(is_sha256_hex)
}

/// Whether `text` is 64 lowercase hexadecimal digits, as a SHA-256 hash
/// is written.
fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Path to a review store, used to create snapshots and write transactions.
#[derive(Debug, Clone)]
pub struct ReviewStore {
    root: PathBuf,
}

impl ReviewStore {
    /// Creates a review store for `repository` at `root` with a random ID.
    ///
    /// Creates missing directories and publishes an empty manifest at
    /// revision zero without replacing an existing one.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if `manifest.json` already
    /// exists, [`StoreError::Json`] if serialization fails, or
    /// [`StoreError::Io`] if a directory or file operation fails.
    pub fn create(root: &Path, repository: &Path) -> Result<Self, StoreError> {
        Self::create_with_id(root, StoreId::generate(), repository)
    }

    /// [`Self::create`] with the ID the caller named `root` after.
    ///
    /// # Errors
    ///
    /// As [`Self::create`].
    fn create_with_id(root: &Path, id: StoreId, repository: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(root).map_err(|e| io_error(root, e))?;
        files::create_manifest(root, &ReviewManifest::new(id, repository))?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Returns a handle after checking that `root/manifest.json` is a file.
    ///
    /// This checks file metadata only. [`Self::snapshot`] reads and
    /// validates the manifest contents.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotARecord`] if the path is missing, `root` is
    /// not a directory, or the manifest is not a file. Returns
    /// [`StoreError::Io`] for other metadata errors.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        files::check_manifest(root)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Returns the review store's directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads the current manifest and creates a snapshot that keeps it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotARecord`] if the manifest is missing,
    /// [`StoreError::Io`] if it cannot be read, [`StoreError::Json`] if it
    /// cannot be parsed, or [`StoreError::UnsupportedReviewStore`] if its
    /// version differs from [`SCHEMA_VERSION`].
    pub fn snapshot(&self) -> Result<ReviewSnapshot, StoreError> {
        Ok(ReviewSnapshot::new(
            self.root.clone(),
            manifest::read(&self.root)?,
        ))
    }

    /// Takes the writer lock without waiting and starts a transaction at
    /// the current revision, reading the judgments it pins.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Locked`] if another writer holds the lock,
    /// [`StoreError::Io`] if the lock file cannot be opened or locked, or
    /// any error returned by [`Self::snapshot`] or
    /// [`ReviewSnapshot::judgments`].
    pub fn begin(&self) -> Result<ReviewTx<'_>, StoreError> {
        ReviewTx::begin(self)
    }
}

/// Finds the review store in `reviews_dir` whose manifest names
/// `repository`, the canonical path of a git common directory.
///
/// Entries without a manifest, such as the directory lock, are skipped; a
/// missing `reviews_dir` has no stores. The caller holds the directory lock
/// so no store is created meanwhile.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if `reviews_dir` exists but cannot be read,
/// an error from reading a store's manifest (an unreadable store is never
/// mistaken for an absent one), or [`StoreError::SeveralReviewStores`] if
/// more than one store names `repository`.
pub fn find(reviews_dir: &Path, repository: &Path) -> Result<Option<ReviewStore>, StoreError> {
    let stores = stores_in(reviews_dir, ReviewStore::open).map_err(|e| io_error(reviews_dir, e))?;
    let mut found = Vec::new();
    for store in stores {
        let store = store?;
        if manifest::read(store.root())?.repository == repository {
            found.push(store);
        }
    }
    if found.len() > 1 {
        return Err(StoreError::SeveralReviewStores {
            repository: repository.to_path_buf(),
            stores: found.into_iter().map(|store| store.root).collect(),
        });
    }
    Ok(found.pop())
}

/// Creates a review store for `repository` at `<reviews_dir>/<new id>`.
///
/// The caller holds the directory lock and has found no store for
/// `repository` with [`find`].
///
/// # Errors
///
/// As [`ReviewStore::create`].
pub fn create_in(reviews_dir: &Path, repository: &Path) -> Result<ReviewStore, StoreError> {
    let id = StoreId::generate();
    ReviewStore::create_with_id(&reviews_dir.join(id.as_str()), id, repository)
}
