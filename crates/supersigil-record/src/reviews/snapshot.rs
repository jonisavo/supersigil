//! Reads the bases and judgments one review store manifest pins.

use std::path::PathBuf;

use super::{
    BASES_DIR, Basis, BasisHeader, BasisId, JUDGMENTS_FILE, Judgment, ReviewManifest,
    basis_file_name, judgment, read_basis_file,
};
use crate::store::StoreError;

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
        let (path, bytes) = self.basis_bytes(id)?;
        serde_json::from_slice(&bytes).map_err(|source| StoreError::Json { path, source })
    }

    /// Checks a listed basis's hash as [`Self::basis`] does, then returns
    /// `None` if `wanted` rejects its header. Otherwise parses the full basis.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Self::basis`]. A rejected valid header skips
    /// full parsing and its JSON errors. An invalid header falls back to
    /// full parsing, which reports any JSON error.
    pub fn basis_if(
        &self,
        id: &BasisId,
        wanted: impl FnOnce(&BasisHeader) -> bool,
    ) -> Result<Option<Basis>, StoreError> {
        let (path, bytes) = self.basis_bytes(id)?;
        if let Ok(header) = serde_json::from_slice::<BasisHeader>(&bytes)
            && !wanted(&header)
        {
            return Ok(None);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| StoreError::Json { path, source })
    }

    /// Returns a listed basis's path and bytes after checking its hash against `id`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoSuchBasis`] if the manifest does not list
    /// `id` in `bases`, [`StoreError::Io`] if the file cannot be read, or
    /// [`StoreError::Corrupt`] if its bytes do not hash to `id`.
    fn basis_bytes(&self, id: &BasisId) -> Result<(PathBuf, Vec<u8>), StoreError> {
        if !self.manifest.bases.contains_key(id) {
            return Err(StoreError::NoSuchBasis(id.to_string()));
        }
        let path = self.root.join(BASES_DIR).join(basis_file_name(id));
        let bytes = read_basis_file(&path, id)?;
        Ok((path, bytes))
    }

    /// Reads the judgments this snapshot pins, in the order they were
    /// recorded. Bytes appended after this snapshot's revision are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Corrupt`] if the log is shorter than its
    /// committed length, [`StoreError::Io`] if it cannot be read, or
    /// [`StoreError::Json`] if its lines are not judgments.
    pub fn judgments(&self) -> Result<Vec<Judgment>, StoreError> {
        judgment::read_log(
            &self.root.join(JUDGMENTS_FILE),
            self.manifest.judgments_length,
        )
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
