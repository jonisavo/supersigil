//! Writes a review store under its writer lock and commits it through the
//! manifest.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::path::Path;

use super::manifest::{self, BasisTimes, ReviewManifest};
use super::{
    BASES_DIR, Basis, BasisId, JUDGMENTS_FILE, Judgment, LOCK_FILE, ReviewStore, basis_file_name,
    judgment, written_in_bases,
};
use crate::files;
use crate::ids::Revision;
use crate::store::{MANIFEST_FILE, StoreError, io_error};
use crate::time::{GRACE_SECONDS, REMOVAL_DELAY_SECONDS, UtcTime};

/// Holds the writer lock while preparing the next review store revision.
///
/// Basis files and judgment lines reach the disk before [`Self::commit`],
/// but readers see them only once the published manifest lists them.
/// Dropping the transaction without committing leaves the manifest
/// unchanged and releases the lock; the next writer truncates the
/// judgments it appended, and a later cleanup deletes the basis files it
/// wrote.
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
    /// Whether this transaction appended to the judgments log.
    appended: bool,
    _lock: File,
}

impl<'a> ReviewTx<'a> {
    /// Acquires the writer lock and reads the starting manifest and the
    /// judgments it pins.
    ///
    /// A basis listed in both `bases` and `removed`, as no writer of this
    /// build leaves a manifest, is live: it leaves `removed`, so no cleanup
    /// deletes its file.
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
            starting: manifest.clone(),
            manifest,
            judged: judgments.iter().map(|j| j.basis.clone()).collect(),
            judgments,
            appended: false,
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

    /// Returns the judgments as of the transaction's start, in the order
    /// they were recorded, excluding those this transaction appended. A
    /// writer checks them under the lock before appending.
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
        self.appended = true;
        Ok(())
    }

    /// Saves `basis` as reviewed at `now` and returns its ID.
    ///
    /// Writes `bases/<id>.json` through a temporary file in `bases/`, unless
    /// a regular file there already hashes to the ID; a file that does not,
    /// such as one a crashed writer left, is replaced, and so is a symbolic
    /// link. Sets the basis's `last_reviewed` to `now`, and its
    /// `first_written` when it is new. A basis listed in `removed` leaves it
    /// and is new again.
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
            files::sync_dir(root)?;
            files::replace(&dir, &name, &bytes)?;
        }
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

    /// Makes `basis` the review `judge` applies to in `worktree`, a
    /// canonical worktree path.
    pub fn select(&mut self, worktree: &Path, basis: &BasisId) {
        self.manifest
            .selections
            .insert(worktree.to_path_buf(), basis.clone());
    }

    /// Removes bases nobody uses, in two phases so no published manifest
    /// names a deleted file. A basis that a selection or any judgment names
    /// is kept, so a judged basis is kept forever.
    ///
    /// - Deletes the files of bases whose removal a published manifest
    ///   recorded more than [`REMOVAL_DELAY_SECONDS`] before `now`, after
    ///   checking again that no selection or judgment names them, and drops
    ///   them from `removed`. A removal this transaction made is never
    ///   deleted here.
    /// - Moves bases that nothing names and that were last reviewed more
    ///   than [`GRACE_SECONDS`] before `now` from `bases` to `removed` with
    ///   the time `now`. Committing publishes that removal; a later write
    ///   deletes the files.
    /// - Deletes files in `bases/` that the manifest neither lists nor marks
    ///   removed: leftovers of a write that never committed. Only a file
    ///   under a name the store writes there (a basis file's, or its
    ///   temporary file's) is deleted; any other file is kept.
    ///
    /// Only regular files are deleted, never a symbolic link nor anything
    /// through one: a removed basis whose file is a link leaves `removed`
    /// with its link kept, and nothing is deleted while `bases/`, the
    /// manifest, the lock, or the judgments log is a link or cannot be
    /// checked (what one leads to could be a file in `bases/`). Before
    /// deleting anything, the file of each basis in `bases` that is a link
    /// is replaced by a regular file holding what the link leads to, so no
    /// deletion removes what a live basis reads; while one cannot be (what
    /// it leads to cannot be read or does not hash to its ID), nothing is
    /// deleted. A file that cannot be deleted (on Windows, one another
    /// process has open) stays, with its entry, for the next write.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] if `bases/` exists but cannot be listed.
    pub fn cleanup(&mut self, now: UtcTime) -> Result<(), StoreError> {
        let root = self.store.root();
        let dir = root.join(BASES_DIR);
        let deletable = files::unlinked_entries(root, &[MANIFEST_FILE, LOCK_FILE, JUDGMENTS_FILE])
            && self.make_bases_regular(&dir);
        let kept: BTreeSet<&BasisId> = self
            .manifest
            .selections
            .values()
            .chain(&self.judged)
            .collect();
        for (id, removed) in &self.starting.removed {
            let due = now.unix() - removed.unix() > REMOVAL_DELAY_SECONDS;
            if !deletable
                || !due
                || kept.contains(id)
                || self.manifest.removed.get(id) != Some(removed)
            {
                continue;
            }
            if remove_regular(&dir.join(basis_file_name(id))) {
                self.manifest.removed.remove(id);
            }
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
            .map(basis_file_name)
            .collect();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
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
            // A leftover that cannot be deleted is tried again next write.
            remove_regular(&entry.path());
        }
        Ok(())
    }

    /// Replaces the file of each basis in `bases` that is a symbolic link
    /// with a regular file holding what the link leads to (see
    /// [`make_regular`]). Returns whether cleanup may delete files in
    /// `dir`, the `bases/` directory: it is missing, or a directory that is
    /// not a link and in which every such file was replaced.
    fn make_bases_regular(&self, dir: &Path) -> bool {
        match fs::symlink_metadata(dir) {
            Ok(metadata) if metadata.is_dir() => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return true,
            _ => return false,
        }
        self.manifest
            .bases
            .keys()
            .all(|id| make_regular(dir, id).is_ok())
    }

    /// Publishes this transaction's changes and returns the new revision.
    ///
    /// Syncs the appended judgments and the store directory, then checks
    /// that the on-disk revision still matches the starting one, increments
    /// it, and publishes the manifest by atomic rename. Basis files were
    /// synced when they were written.
    ///
    /// On Windows, the store directory is not synced; its entries then rely
    /// on the file system's durability guarantees.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if the on-disk revision changed,
    /// errors from reading or validating the current manifest, or
    /// [`StoreError::Json`] or [`StoreError::Io`] if syncing or publishing
    /// fails. An error syncing the store directory after the rename can
    /// occur after the new manifest is already visible.
    ///
    /// # Panics
    ///
    /// Panics if the starting revision is [`u64::MAX`].
    pub fn commit(mut self) -> Result<Revision, StoreError> {
        let root = self.store.root();
        if self.appended {
            files::sync_file(&root.join(JUDGMENTS_FILE))?;
            files::sync_dir(root)?;
        }
        let on_disk = manifest::read(root)?;
        let expected = self.starting.revision;
        if on_disk.revision != expected {
            return Err(StoreError::Conflict {
                expected: expected.get(),
                found: on_disk.revision.get(),
            });
        }
        self.manifest.revision = expected.next();
        files::publish_manifest(root, &self.manifest)?;
        Ok(self.manifest.revision)
    }
}

/// Replaces the file of basis `id` in `dir` with a regular file holding
/// what the file leads to, written as [`ReviewTx::save_basis`] writes, when
/// it is a symbolic link; anything else is left as it is.
///
/// # Errors
///
/// Returns [`StoreError::Corrupt`] if what the link leads to does not hash
/// to `id`, or [`StoreError::Io`] if reading or replacing the file fails.
fn make_regular(dir: &Path, id: &BasisId) -> Result<(), StoreError> {
    let name = basis_file_name(id);
    let path = dir.join(&name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_symlink() => {}
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(io_error(&path, e)),
        _ => return Ok(()),
    }
    let bytes = fs::read(&path).map_err(|e| io_error(&path, e))?;
    if BasisId::of(&bytes) != *id {
        return Err(StoreError::Corrupt(format!(
            "{} does not hash to its basis id",
            path.display()
        )));
    }
    files::replace(dir, &name, &bytes)
}

/// Deletes `path` if it is a regular file; cleanup never deletes a
/// symbolic link, nor what one leads to. Returns whether nothing is left
/// for cleanup to delete: the file was deleted or is missing, or `path` is
/// a link.
fn remove_regular(path: &Path) -> bool {
    let removed = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_symlink() => return true,
        Ok(metadata) if metadata.is_file() => fs::remove_file(path),
        Ok(_) => return false,
        Err(e) => Err(e),
    };
    match removed {
        Ok(()) => true,
        Err(e) => e.kind() == io::ErrorKind::NotFound,
    }
}
