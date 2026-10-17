//! The commit step both stores share: sync what a transaction changed, check
//! that no other writer published meanwhile, then publish the manifest.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{replace, sync_dir, sync_file};
use crate::ids::Revision;
use crate::store::{MANIFEST_FILE, StoreError};

/// A manifest [`Publication::commit`] can publish.
pub(crate) trait Published: Serialize + Sized {
    /// Reads the manifest published at `root`.
    ///
    /// # Errors
    ///
    /// Returns the store's error for a manifest it cannot read or accept.
    fn read(root: &Path) -> Result<Self, StoreError>;

    /// The manifest's revision.
    fn revision(&mut self) -> &mut Revision;
}

/// The paths a write transaction changed under its store root since it
/// began at revision `starting`.
///
/// Stores record each file they write or append to and each path they
/// delete. Nothing is synced until [`Self::commit`], which syncs them in
/// the order [`Self::syncs`] gives.
#[derive(Debug)]
pub(crate) struct Publication {
    root: PathBuf,
    starting: Revision,
    touched: BTreeSet<PathBuf>,
    deleted: BTreeSet<PathBuf>,
}

/// One sync in [`Publication::syncs`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step<'a> {
    /// Sync a written file's contents.
    File(&'a Path),
    /// Sync a directory's entries.
    Dir(&'a Path),
}

impl Publication {
    /// Starts recording changes under `root` for a transaction that began
    /// at revision `starting`.
    pub(crate) fn new(root: &Path, starting: Revision) -> Self {
        Self {
            root: root.to_path_buf(),
            starting,
            touched: BTreeSet::new(),
            deleted: BTreeSet::new(),
        }
    }

    /// Records that the file at `path`, under the root, was written or
    /// appended to.
    pub(crate) fn touched(&mut self, path: PathBuf) {
        self.touched.insert(path);
    }

    /// Records that `path`, under the root, was deleted.
    pub(crate) fn deleted(&mut self, path: PathBuf) {
        self.deleted.insert(path);
    }

    /// Returns the syncs that make the recorded changes durable: each
    /// touched file, then each directory that holds a touched or deleted
    /// path, with its ancestors up to the root. Each appears once, in path
    /// order. Nothing above the root is synced.
    pub(crate) fn syncs(&self) -> Vec<Step<'_>> {
        let dirs: BTreeSet<&Path> = self
            .touched
            .iter()
            .chain(&self.deleted)
            .flat_map(|path| {
                path.ancestors()
                    .skip(1)
                    .take_while(|dir| dir.starts_with(&self.root))
            })
            .collect();
        self.touched
            .iter()
            .map(|path| Step::File(path))
            .chain(dirs.into_iter().map(Step::Dir))
            .collect()
    }

    /// Syncs the recorded changes ([`Self::syncs`]), then checks that the
    /// published revision is still the starting one, sets `manifest`'s
    /// revision to the next and publishes it by atomic rename.
    ///
    /// On Windows, directories that cannot be opened are not synced. Their
    /// entries then rely on the file system's durability guarantees.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if the published revision changed,
    /// errors from reading or validating the published manifest, or
    /// [`StoreError::Json`] or [`StoreError::Io`] if syncing or publishing
    /// fails. An error syncing the root after the rename can occur after
    /// the new manifest is already visible.
    ///
    /// # Panics
    ///
    /// Panics if the starting revision is [`u64::MAX`].
    pub(crate) fn commit<M: Published>(&self, manifest: &mut M) -> Result<Revision, StoreError> {
        for step in self.syncs() {
            match step {
                Step::File(path) => sync_file(path)?,
                Step::Dir(dir) => sync_dir(dir)?,
            }
        }
        let found = *M::read(&self.root)?.revision();
        let next = next_revision(self.starting, found)?;
        *manifest.revision() = next;
        publish_manifest(&self.root, manifest)?;
        Ok(next)
    }
}

/// Checks that `found` equals `expected` and returns the next revision.
///
/// # Errors
///
/// Returns [`StoreError::Conflict`] if `found` differs from `expected`.
///
/// # Panics
///
/// Panics if `expected` is [`u64::MAX`].
fn next_revision(expected: Revision, found: Revision) -> Result<Revision, StoreError> {
    if found != expected {
        return Err(StoreError::Conflict {
            expected: expected.get(),
            found: found.get(),
        });
    }
    Ok(expected.next())
}

/// Replaces the manifest by rename (see [`replace`]).
///
/// # Errors
///
/// Returns [`StoreError::Json`] if serialization fails or [`StoreError::Io`]
/// if writing, renaming, or syncing fails.
fn publish_manifest(root: &Path, manifest: &impl Serialize) -> Result<(), StoreError> {
    replace(root, MANIFEST_FILE, &super::manifest_json(root, manifest)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publication() -> Publication {
        Publication::new(Path::new("/store"), Revision::ZERO)
    }

    #[test]
    fn nothing_changed_syncs_nothing() {
        assert_eq!(publication().syncs(), []);
    }

    #[test]
    fn touched_files_sync_before_every_directory_up_to_the_root() {
        let mut changes = publication();
        changes.touched("/store/sessions/1.jsonl".into());
        changes.touched("/store/derivations/1.r2.json".into());
        changes.touched("/store/sessions/1.jsonl".into());
        assert_eq!(
            changes.syncs(),
            [
                Step::File(Path::new("/store/derivations/1.r2.json")),
                Step::File(Path::new("/store/sessions/1.jsonl")),
                Step::Dir(Path::new("/store")),
                Step::Dir(Path::new("/store/derivations")),
                Step::Dir(Path::new("/store/sessions")),
            ]
        );
    }

    #[test]
    fn a_deleted_path_syncs_its_directories_but_not_itself() {
        let mut changes = publication();
        changes.deleted("/store/bases/a/b.json".into());
        assert_eq!(
            changes.syncs(),
            [
                Step::Dir(Path::new("/store")),
                Step::Dir(Path::new("/store/bases")),
                Step::Dir(Path::new("/store/bases/a")),
            ]
        );
    }

    #[test]
    fn a_file_in_the_root_syncs_only_the_root_directory() {
        let mut changes = publication();
        changes.touched("/store/judgments.jsonl".into());
        assert_eq!(
            changes.syncs(),
            [
                Step::File(Path::new("/store/judgments.jsonl")),
                Step::Dir(Path::new("/store")),
            ]
        );
    }
}
