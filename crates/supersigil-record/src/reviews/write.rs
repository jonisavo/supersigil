//! Writes a review store under its writer lock and commits it through the
//! manifest.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use super::manifest::{self, BasisTimes, ReviewManifest};
use super::{
    BASES_DIR, Basis, BasisId, JUDGMENTS_FILE, Judgment, LOCK_FILE, ReviewStore, basis_file_name,
    judgment, read_basis_file, written_in_bases,
};
use crate::files::{self, Publication};
use crate::ids::Revision;
use crate::store::{MANIFEST_FILE, StoreError, io_error};
use crate::time::{GRACE_SECONDS, REMOVAL_DELAY_SECONDS, UtcTime};

/// Holds the writer lock while preparing the next review store revision.
///
/// Basis files and judgment lines reach the disk before [`Self::commit`],
/// but readers see them only once the published manifest lists them.
/// Dropping the transaction without committing leaves the manifest unchanged
/// and releases the lock. The next append truncates uncommitted judgment bytes.
/// The next commit deletes unlisted basis files this transaction wrote.
#[derive(Debug)]
pub struct ReviewTx<'a> {
    store: &'a ReviewStore,
    /// Manifest with this transaction's changes, ready to publish at commit.
    manifest: ReviewManifest,
    /// Manifest read after acquiring the writer lock.
    starting: ReviewManifest,
    /// Judgments the starting manifest pins.
    judgments: Vec<Judgment>,
    /// Bases named by any judgment, including this transaction's.
    judged: BTreeSet<BasisId>,
    /// What this transaction changed, synced at commit.
    publication: Publication,
    _lock: File,
}

impl<'a> ReviewTx<'a> {
    /// Acquires the writer lock and reads the starting manifest and the
    /// judgments it pins.
    ///
    /// Treats a basis listed in both `bases` and `removed` as live and removes
    /// its `removed` entry to prevent deletion. This build never writes that overlap.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Locked`] if another writer holds the lock,
    /// [`StoreError::Io`] if a lock operation fails, or an error from
    /// reading and validating the manifest or the judgments.
    pub(super) fn begin(store: &'a ReviewStore) -> Result<Self, StoreError> {
        let lock = files::lock(store.root(), LOCK_FILE)?;
        let mut manifest = manifest::read(store.root())?;
        manifest
            .removed
            .retain(|id, _| !manifest.bases.contains_key(id));
        let judgments = judgment::read_log(
            &store.root().join(JUDGMENTS_FILE),
            manifest.judgments_length,
        )?;
        Ok(Self {
            store,
            publication: Publication::new(store.root(), manifest.revision),
            starting: manifest.clone(),
            manifest,
            judged: judgments.iter().map(|j| j.basis.clone()).collect(),
            judgments,
            _lock: lock,
        })
    }

    /// Returns the manifest with this transaction's changes applied.
    ///
    /// Its revision number advances only during [`Self::commit`].
    #[must_use]
    pub fn manifest(&self) -> &ReviewManifest {
        &self.manifest
    }

    /// Returns judgments in recorded order as of the transaction's start,
    /// excluding its appends. Writers check these under the lock before appending.
    #[must_use]
    pub fn judgments(&self) -> &[Judgment] {
        &self.judgments
    }

    /// Appends `judgment` as one line to the judgments log.
    ///
    /// The first append drops any bytes beyond the committed length, which
    /// a writer that never committed left. The line becomes visible to new
    /// snapshots after [`Self::commit`].
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoSuchBasis`] if the judgment's basis is not
    /// in this transaction's `bases`, [`StoreError::Corrupt`] if the log is
    /// shorter than its committed length, [`StoreError::Json`] if
    /// serialization fails, or [`StoreError::Io`] if a file operation
    /// fails.
    pub fn append_judgment(&mut self, judgment: &Judgment) -> Result<(), StoreError> {
        if !self.manifest.bases.contains_key(&judgment.basis) {
            return Err(StoreError::NoSuchBasis(judgment.basis.to_string()));
        }
        let path = self.store.root().join(JUDGMENTS_FILE);
        let mut line = serde_json::to_vec(judgment).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        line.push(b'\n');
        files::append_at(&path, self.manifest.judgments_length, &line)?;
        self.manifest.judgments_length += line.len() as u64;
        self.judged.insert(judgment.basis.clone());
        self.publication.touched(path);
        Ok(())
    }

    /// Saves `basis` as reviewed at `now` and returns its ID.
    ///
    /// Reuses an existing regular `bases/<id>.json` if its bytes hash to the
    /// ID. Otherwise writes through a temporary file in `bases/` and replaces
    /// the target, including symbolic links. Either way, [`Self::commit`]
    /// syncs the file, since a crash may have left a reused one renamed into
    /// place but not durable.
    /// Sets `last_reviewed` to `now` and sets `first_written` for a new basis.
    /// Removes the basis from `removed`; a previously removed basis is new again.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Json`] if the basis cannot be serialized (a
    /// path in it is not valid UTF-8) or [`StoreError::Io`] if reading,
    /// writing, or syncing a file or directory fails.
    pub fn save_basis(&mut self, basis: &Basis, now: UtcTime) -> Result<BasisId, StoreError> {
        let root = self.store.root();
        let dir = root.join(BASES_DIR);
        let bytes = serde_json::to_vec(basis).map_err(|source| StoreError::Json {
            path: dir.clone(),
            source,
        })?;
        let id = BasisId::of(&bytes);
        let name = basis_file_name(&id);
        let path = dir.join(&name);
        let intact = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_symlink() => false,
            Ok(_) => BasisId::of(&fs::read(&path).map_err(|e| io_error(&path, e))?) == id,
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(io_error(&path, e)),
        };
        if !intact {
            fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
            files::replace(&dir, &name, &bytes)?;
        }
        self.publication.touched(path);
        self.manifest.removed.remove(&id);
        self.manifest
            .bases
            .entry(id.clone())
            .and_modify(|times| times.last_reviewed = now)
            .or_insert(BasisTimes {
                first_written: now,
                last_reviewed: now,
            });
        Ok(id)
    }

    /// Selects `basis` for `judge` in the canonical path `worktree`.
    pub fn select(&mut self, worktree: &Path, basis: &BasisId) {
        self.manifest
            .selections
            .insert(worktree.to_path_buf(), basis.clone());
    }

    /// Removes unused bases at `now`, then publishes this transaction's
    /// changes and returns the new revision.
    ///
    /// Bases are removed in two phases. Bases named by a selection or any
    /// judgment are kept, so judged bases stay forever. Each removal is
    /// published before a later write deletes its file.
    ///
    /// - Deletes the files of bases whose removal a published manifest
    ///   recorded more than [`REMOVAL_DELAY_SECONDS`] before `now`, after
    ///   checking again that no selection or judgment names them, and drops
    ///   them from `removed`. A removal this transaction made is never
    ///   deleted here.
    /// - Moves bases that nothing names and that were last reviewed more
    ///   than [`GRACE_SECONDS`] before `now` from `bases` to `removed` with
    ///   the time `now`. This commit publishes that removal; a later write
    ///   deletes the files.
    /// - Deletes leftover files in `bases/` that no manifest entry, selection,
    ///   or judgment names. Deletes only basis files and their temporary files
    ///   under names the store writes; keeps all other names.
    ///
    /// Deletes only regular files, without following symbolic links. A removed
    /// basis whose file is a link leaves `removed`, but keeps its link. Skips
    /// all deletions if `bases/`, the manifest, the lock, or the judgments log
    /// is a link or cannot be checked, since it could reference a deletion target.
    ///
    /// Before deleting, replaces linked files of live, selected, or judged
    /// bases with regular files containing the same bytes. Skips all deletions
    /// if any such link cannot be read or its bytes do not hash to the basis ID.
    /// Files that cannot be deleted, including open files on Windows, keep
    /// their entries for the next write.
    ///
    /// Publishing then syncs written files and the directories of written
    /// and deleted files up to the store directory, checks that the on-disk
    /// revision still matches the starting one, increments it, and
    /// publishes the manifest by atomic rename.
    ///
    /// On Windows, skips directory syncing if the directory cannot be opened.
    /// Its entries then rely on the file system's durability guarantees.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] if `bases/` exists but cannot be listed,
    /// [`StoreError::Conflict`] if the on-disk revision changed, errors from
    /// reading or validating the current manifest, or [`StoreError::Json`]
    /// or [`StoreError::Io`] if syncing or publishing fails. An error
    /// syncing the store directory after the rename can occur after the new
    /// manifest is already visible.
    ///
    /// # Panics
    ///
    /// Panics if the starting revision is [`u64::MAX`].
    pub fn commit(mut self, now: UtcTime) -> Result<Revision, StoreError> {
        self.cleanup(now)?;
        self.publication.commit(&mut self.manifest)
    }

    /// Removes unused bases at `now` as [`Self::commit`] describes, recording
    /// each deletion and replacement for it to sync.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] if `bases/` exists but cannot be listed.
    fn cleanup(&mut self, now: UtcTime) -> Result<(), StoreError> {
        let root = self.store.root();
        let dir = root.join(BASES_DIR);
        let kept: BTreeSet<&BasisId> = self
            .manifest
            .selections
            .values()
            .chain(&self.judged)
            .collect();
        let deletable = files::unlinked_entries(root, &[MANIFEST_FILE, LOCK_FILE, JUDGMENTS_FILE])
            && make_bases_regular(&self.manifest, &dir, &kept, &mut self.publication);
        for (id, removed) in &self.starting.removed {
            let due = now.unix() - removed.unix() > REMOVAL_DELAY_SECONDS;
            if !deletable
                || !due
                || kept.contains(id)
                || self.manifest.removed.get(id) != Some(removed)
            {
                continue;
            }
            let path = dir.join(basis_file_name(id));
            match remove_regular(&path) {
                Removal::Deleted => self.publication.deleted(path),
                // An earlier write may have deleted it and stopped before
                // syncing `bases/`, which a missing directory no longer needs.
                Removal::Missing
                    if fs::symlink_metadata(&dir).is_ok_and(|metadata| metadata.is_dir()) =>
                {
                    self.publication.deleted(path);
                }
                Removal::Missing | Removal::Kept => {}
                Removal::Failed => continue,
            }
            self.manifest.removed.remove(id);
        }

        let unused: Vec<BasisId> = self
            .manifest
            .bases
            .iter()
            .filter(|(id, times)| {
                now.unix() - times.last_reviewed.unix() > GRACE_SECONDS && !kept.contains(id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in unused {
            self.manifest.bases.remove(&id);
            self.manifest.removed.insert(id, now);
        }

        if !deletable {
            return Ok(());
        }
        let listed: BTreeSet<String> = self
            .manifest
            .bases
            .keys()
            .chain(self.manifest.removed.keys())
            .chain(kept)
            .map(basis_file_name)
            .collect();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            // Nothing was deleted from a directory that is not there.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io_error(&dir, e)),
        };
        for entry in entries {
            let entry = entry.map_err(|e| io_error(&dir, e))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if listed.contains(name) || !written_in_bases(name) {
                continue;
            }
            // Retry undeletable leftovers on the next write.
            let path = entry.path();
            if remove_regular(&path) == Removal::Deleted {
                self.publication.deleted(path);
            }
        }
        Ok(())
    }
}

/// Replaces linked files of bases in `manifest`'s `bases` or `kept` with
/// regular files, in ID order (see [`make_regular`]), recording each
/// replacement in `publication`. `kept` contains selected or judged bases.
/// Allows deletion only if `dir` is missing, or is an unlinked directory
/// where every required replacement succeeded.
fn make_bases_regular(
    manifest: &ReviewManifest,
    dir: &Path,
    kept: &BTreeSet<&BasisId>,
    publication: &mut Publication,
) -> bool {
    match fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.is_dir() => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return true,
        _ => return false,
    }
    let live: BTreeSet<&BasisId> = manifest.bases.keys().chain(kept.iter().copied()).collect();
    live.into_iter().all(|id| match make_regular(dir, id) {
        Ok(Some(path)) => {
            publication.touched(path);
            true
        }
        Ok(None) => true,
        Err(_) => false,
    })
}

/// Replaces a symbolic link for basis `id` in `dir` with its target's bytes,
/// written as [`ReviewTx::save_basis`] writes, and returns the replaced
/// path. Leaves other entries unchanged and returns `None` for them.
///
/// # Errors
///
/// Returns [`StoreError::Corrupt`] if what the link leads to does not hash
/// to `id`, or [`StoreError::Io`] if reading or replacing the file fails.
fn make_regular(dir: &Path, id: &BasisId) -> Result<Option<PathBuf>, StoreError> {
    let name = basis_file_name(id);
    let path = dir.join(&name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_symlink() => {}
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(io_error(&path, e)),
        _ => return Ok(None),
    }
    let bytes = read_basis_file(&path, id)?;
    files::replace(dir, &name, &bytes)?;
    Ok(Some(path))
}

/// What [`remove_regular`] did with a path.
#[derive(Debug, PartialEq, Eq)]
enum Removal {
    /// Deleted a regular file.
    Deleted,
    /// Found nothing at the path.
    Missing,
    /// Found a symbolic link, which cleanup keeps.
    Kept,
    /// Could not delete what is there.
    Failed,
}

/// Deletes `path` only if it is a regular file, without following links.
fn remove_regular(path: &Path) -> Removal {
    let removed = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_symlink() => return Removal::Kept,
        Ok(metadata) if metadata.is_file() => fs::remove_file(path),
        Ok(_) => return Removal::Failed,
        Err(e) => Err(e),
    };
    match removed {
        Ok(()) => Removal::Deleted,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Removal::Missing,
        Err(_) => Removal::Failed,
    }
}

#[cfg(test)]
mod tests;
