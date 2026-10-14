//! Reads the bases listed in one review store manifest.

use std::fs;
use std::path::PathBuf;

use super::{BASES_DIR, Basis, BasisId, ReviewManifest, basis_file_name};
use crate::store::{StoreError, io_error};

/// Reads a review store at the revision captured by its manifest.
///
/// Later commits do not change what this snapshot lists.
#[derive(Debug, Clone)]
pub struct ReviewSnapshot {
    root: PathBuf,
    manifest: ReviewManifest,
}

impl ReviewSnapshot {
    /// Creates a snapshot using the supplied directory and manifest.
    pub(super) fn new(root: PathBuf, manifest: ReviewManifest) -> Self {
        Self { root, manifest }
    }

    /// Returns the manifest captured when this snapshot was created.
    #[must_use]
    pub fn manifest(&self) -> &ReviewManifest {
        &self.manifest
    }

    /// Reads a basis this snapshot lists in `bases`, checking that the file
    /// hashes to `id`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoSuchBasis`] if the manifest does not list
    /// `id` in `bases`, [`StoreError::Io`] if the file cannot be read,
    /// [`StoreError::Corrupt`] if its bytes do not hash to `id`, or
    /// [`StoreError::Json`] if they are not a basis.
    pub fn basis(&self, id: &BasisId) -> Result<Basis, StoreError> {
        if !self.manifest.bases.contains_key(id) {
            return Err(StoreError::NoSuchBasis(id.to_string()));
        }
        let path = self.root.join(BASES_DIR).join(basis_file_name(id));
        let bytes = fs::read(&path).map_err(|e| io_error(&path, e))?;
        if BasisId::of(&bytes) != *id {
            return Err(StoreError::Corrupt(format!(
                "{} does not hash to its basis id",
                path.display()
            )));
        }
        serde_json::from_slice(&bytes).map_err(|source| StoreError::Json { path, source })
    }

    /// Resolves a full basis ID, or a prefix of at least 8 hexadecimal
    /// digits in either case, among the bases this snapshot lists in
    /// `bases`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBasisPrefix`] if `prefix` is shorter
    /// than 8 digits or not hexadecimal, [`StoreError::NoSuchBasis`] if no
    /// basis starts with it, or [`StoreError::AmbiguousBasis`] listing
    /// every basis that does when there are several.
    pub fn find_basis(&self, prefix: &str) -> Result<BasisId, StoreError> {
        if prefix.len() < 8 || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(StoreError::InvalidBasisPrefix(prefix.to_owned()));
        }
        let lowercase = prefix.to_ascii_lowercase();
        let mut candidates: Vec<BasisId> = self
            .manifest
            .bases
            .keys()
            .filter(|id| id.as_str().starts_with(&lowercase))
            .cloned()
            .collect();
        match candidates.len() {
            0 => Err(StoreError::NoSuchBasis(prefix.to_owned())),
            1 => Ok(candidates.remove(0)),
            _ => Err(StoreError::AmbiguousBasis {
                prefix: prefix.to_owned(),
                candidates,
            }),
        }
    }
}
