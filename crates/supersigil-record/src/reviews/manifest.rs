//! The review store's manifest and how it is read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::BasisId;
use crate::files;
use crate::ids::{Revision, StoreId};
use crate::store::StoreError;
use crate::time::UtcTime;

/// Review store manifest schema version supported for reading and writing.
pub const SCHEMA_VERSION: u32 = 1;

/// When a saved basis was first written and last reviewed. Kept outside
/// the basis so they do not change its ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasisTimes {
    /// When a review first saved the basis (again, after a removal).
    pub first_written: UtcTime,
    /// When a review last saved it.
    pub last_reviewed: UtcTime,
}

/// A review store's metadata and the files committed in one revision.
///
/// Readers see the bases listed in `bases`. Writers keep `bases` and
/// `removed` disjoint; a basis listed in both is live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewManifest {
    /// ID shared by all revisions of this store; names its directory.
    pub store_id: StoreId,
    /// Schema version of the files this manifest describes.
    pub schema_version: u32,
    /// Revision number, incremented on every commit.
    pub revision: Revision,
    /// Canonical path of the repository's git common directory.
    pub repository: PathBuf,
    /// Committed byte length of the judgments log.
    pub judgments_length: u64,
    /// Saved bases and their times.
    pub bases: BTreeMap<BasisId, BasisTimes>,
    /// Bases taken out of `bases` by cleanup, with the time that removal
    /// was published. Their files are deleted by a later write.
    pub removed: BTreeMap<BasisId, UtcTime>,
    /// Canonical worktree paths mapped to the basis `judge` applies to
    /// there.
    pub selections: BTreeMap<PathBuf, BasisId>,
}

impl ReviewManifest {
    /// Creates a revision-zero manifest with no bases.
    pub(super) fn new(store_id: StoreId, repository: &Path) -> Self {
        Self {
            store_id,
            schema_version: SCHEMA_VERSION,
            revision: Revision::ZERO,
            repository: repository.to_path_buf(),
            judgments_length: 0,
            bases: BTreeMap::new(),
            removed: BTreeMap::new(),
            selections: BTreeMap::new(),
        }
    }
}

/// Reads the manifest and checks that its version equals [`SCHEMA_VERSION`]
/// (see [`files::read_manifest`]).
///
/// # Errors
///
/// Returns [`StoreError::NotARecord`] for a missing manifest,
/// [`StoreError::Io`] for other read failures, [`StoreError::Json`] for
/// invalid manifest JSON, or [`StoreError::UnsupportedReviewStore`] for a
/// different version.
pub(super) fn read(root: &Path) -> Result<ReviewManifest, StoreError> {
    files::read_manifest(root, SCHEMA_VERSION, |found| {
        StoreError::UnsupportedReviewStore {
            root: root.to_path_buf(),
            found,
            supported: SCHEMA_VERSION,
        }
    })
}
