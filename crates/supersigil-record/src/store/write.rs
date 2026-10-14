//! Writes record data under an exclusive lock and commits it through the manifest.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use std::{io, iter};

use super::manifest::{self, Association, Manifest, SourceCursor};
use super::{
    DERIVATIONS_DIR, LOCK_FILE, MANIFEST_FILE, RecordSnapshot, Store, StoreError,
    derivations_document, io_error, log_path,
};
use crate::derivations::DerivationSet;
use crate::files;
use crate::ids::{Revision, SessionId};
use crate::observations::Observation;
use crate::time::{GRACE_SECONDS, UtcTime};

/// Holds the writer lock while adding data for the next record revision.
///
/// Writes reach the data files before [`Self::commit`], but readers cannot see
/// them until commit publishes the new manifest. Dropping the transaction
/// without committing leaves the manifest unchanged and releases the lock.
/// Uncommitted log bytes and document files may remain on disk.
///
/// The OS lock on `write.lock` is released when the transaction drops or the
/// process exits. The lock file stays in place.
#[derive(Debug)]
pub struct WriteTx<'a> {
    store: &'a Store,
    /// Manifest with this transaction's changes, ready to publish at commit.
    manifest: Manifest,
    /// Manifest read after acquiring the writer lock.
    starting: Manifest,
    /// Time the commit is published at.
    now: UtcTime,
    _lock: File,
    /// Files to sync at commit, with duplicate paths removed.
    touched: BTreeSet<PathBuf>,
}

impl<'a> WriteTx<'a> {
    /// Acquires the writer lock and reads the starting manifest of a
    /// transaction published at `now`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Locked`] if another writer holds the lock,
    /// [`StoreError::Io`] if a lock operation fails, or an error from reading
    /// and validating the manifest.
    pub(super) fn begin(store: &'a Store, now: UtcTime) -> Result<Self, StoreError> {
        let lock = files::lock(store.root(), LOCK_FILE)?;
        let manifest = manifest::read(store.root())?;
        Ok(Self {
            store,
            starting: manifest.clone(),
            manifest,
            now,
            _lock: lock,
            touched: BTreeSet::new(),
        })
    }

    /// Returns the manifest with this transaction's changes applied.
    ///
    /// Its revision number advances only during [`Self::commit`].
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Creates a snapshot of the revision this transaction started from.
    ///
    /// It excludes this transaction's writes. When computing derivations for
    /// the next revision, combine its observations with those being appended.
    #[must_use]
    pub fn snapshot(&self) -> RecordSnapshot {
        RecordSnapshot::new(self.store.root().to_path_buf(), self.starting.clone())
    }

    /// Appends observations as JSON lines to each session's log.
    ///
    /// Assigns numbers to new sessions and preserves input order within each
    /// session. Does not remove duplicate events. The appended observations
    /// become visible to new snapshots after [`Self::commit`].
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Corrupt`] if a log is shorter than the length in
    /// the transaction's manifest, [`StoreError::Json`] if serialization fails,
    /// or [`StoreError::Io`] if a directory or file operation fails.
    pub fn append_observations(&mut self, observations: &[Observation]) -> Result<(), StoreError> {
        let mut lines: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        for observation in observations {
            let number = self.session_number(observation.session());
            let bytes = lines.entry(number).or_default();
            serde_json::to_writer(&mut *bytes, observation).map_err(|source| StoreError::Json {
                path: self.store.root().join(log_path(number)),
                source,
            })?;
            bytes.push(b'\n');
        }
        for (number, bytes) in &lines {
            self.append_log(&log_path(*number), bytes)?;
        }
        Ok(())
    }

    /// Removes bytes beyond the transaction's recorded log length, then appends `bytes`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Corrupt`] if the log is shorter than the recorded
    /// length or [`StoreError::Io`] if a directory or file operation fails.
    fn append_log(&mut self, log: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.store.root().join(log);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        let pinned = self.manifest.logs.get(log).copied().unwrap_or(0);
        files::append_at(&path, pinned, bytes)?;
        self.manifest
            .logs
            .insert(log.to_owned(), pinned + bytes.len() as u64);
        self.touched.insert(path);
        Ok(())
    }

    /// Writes a session's derivations to a file named for the next store revision.
    ///
    /// Assigns a number to a new session. Calling this again for the same
    /// session in this transaction replaces the pending document. A
    /// committed document this replaces stays for readers of earlier
    /// revisions, listed in the manifest's `superseded` with the
    /// transaction's time; see [`Self::commit`].
    ///
    /// The document is written to a new temporary file and renamed into
    /// place, so anything already at its path, such as a file a writer that
    /// never committed left or a symbolic link, is replaced, never written
    /// through.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Json`] if serialization fails or [`StoreError::Io`]
    /// if a directory or file operation fails.
    ///
    /// # Panics
    ///
    /// Panics if the starting revision is [`u64::MAX`].
    pub fn put_derivations(&mut self, set: &DerivationSet) -> Result<(), StoreError> {
        let number = self.session_number(&set.session);
        let logical = derivations_document(number);
        let name = format!("{number}.r{}.json", self.starting.revision.next().get());
        let rel = format!("{DERIVATIONS_DIR}/{name}");
        let dir = self.store.root().join(DERIVATIONS_DIR);
        let path = dir.join(&name);
        let bytes = serde_json::to_vec_pretty(set).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
        files::replace(&dir, &name, &bytes)?;
        // A second write in this transaction replaces its own pending file,
        // which no manifest names. The replaced document is listed under the
        // name cleanup deletes it by ([`listed`]), whatever reached it, and
        // keeps the later of two replacement times, so it gets its full
        // grace period.
        if let Some(replaced) = self.manifest.documents.insert(logical, rel.clone())
            && replaced != rel
        {
            let listed = listed(self.store.root(), &replaced);
            let now = self.now;
            self.manifest
                .superseded
                .entry(listed)
                .and_modify(|time| *time = (*time).max(now))
                .or_insert(now);
        }
        self.touched.insert(path);
        Ok(())
    }

    /// Returns the session's number, assigning the next unused number if needed.
    fn session_number(&mut self, session: &SessionId) -> u64 {
        if let Some(&number) = self.manifest.sessions.get(session) {
            return number;
        }
        let number = self.manifest.sessions.len() as u64;
        self.manifest.sessions.insert(session.clone(), number);
        number
    }

    /// Sets the sync cursor for the transcript path `key` in the pending manifest.
    pub fn set_cursor(&mut self, key: &str, cursor: SourceCursor) {
        self.manifest.cursors.insert(key.to_owned(), cursor);
    }

    /// Adds a checkout to the pending manifest if its path is not already listed.
    ///
    /// Returns `true` if added, or `false` if an equal path was already present.
    pub fn add_association(&mut self, association: Association) -> bool {
        let known = self
            .manifest
            .associations
            .iter()
            .any(|a| a.checkout == association.checkout);
        if !known {
            self.manifest.associations.push(association);
        }
        !known
    }

    /// Publishes this transaction's changes and returns the new revision number.
    ///
    /// First deletes the documents superseded more than [`GRACE_SECONDS`]
    /// before the transaction's time and drops their entries; one that
    /// cannot be deleted (on Windows, a file another process has open) keeps
    /// its entry for the next commit. Only manifests published before the
    /// replacement name such a document. An entry naming anything else, such
    /// as a file the manifest still references under any spelling, a name
    /// no derivation document is written under, or a symbolic link, is
    /// dropped and never deleted. Only regular files are deleted, never
    /// through a symbolic link. Nothing is deleted while the manifest, the
    /// lock, or the observations or derivations directory is a symbolic
    /// link, or while the manifest references a file by anything but a
    /// plain path inside the record, through a symbolic link, or a file
    /// that cannot be resolved on disk. File modification times are never
    /// consulted.
    ///
    /// Syncs changed files and their parent directories up to the record root,
    /// then checks that the on-disk revision still matches the starting one.
    /// Increments the revision and publishes the manifest by atomic rename.
    ///
    /// On Windows, directories that cannot be opened are not synced. Their
    /// entries then rely on the file system's durability guarantees.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if the on-disk revision changed,
    /// errors from reading or validating the current manifest, or
    /// [`StoreError::Json`] or [`StoreError::Io`] if syncing or publishing fails.
    /// An error syncing the record directory after the rename can occur after
    /// the new manifest is already visible.
    ///
    /// # Panics
    ///
    /// Panics if the starting revision is [`u64::MAX`].
    pub fn commit(mut self) -> Result<Revision, StoreError> {
        self.delete_superseded();
        let root = self.store.root();
        let mut dirs = BTreeSet::new();
        for path in &self.touched {
            files::sync_file(path)?;
            for dir in path.ancestors().skip(1) {
                dirs.insert(dir.to_path_buf());
                if dir == root {
                    break;
                }
            }
        }
        for dir in &dirs {
            files::sync_dir(dir)?;
        }
        let on_disk = manifest::read(self.store.root())?;
        let expected = self.starting.revision;
        if on_disk.revision != expected {
            return Err(StoreError::Conflict {
                expected: expected.get(),
                found: on_disk.revision.get(),
            });
        }
        self.manifest.revision = expected.next();
        files::publish_manifest(self.store.root(), &self.manifest)?;
        Ok(self.manifest.revision)
    }

    /// Deletes the documents superseded more than [`GRACE_SECONDS`] before
    /// the transaction's time and drops them from `superseded`. A missing
    /// file counts as deleted; one that cannot be deleted keeps its entry.
    /// An entry that is not a replaced derivation document
    /// ([`replaced_document`]) is dropped without deleting anything. While
    /// the manifest references a document or log by anything but a plain
    /// path inside the record ([`location`]), which may name any file,
    /// nothing is deleted and every entry is kept.
    ///
    /// Once an entry is due, cleanup reads the disk. Nothing is deleted
    /// while the manifest, the lock, or the observations directory is a
    /// symbolic link or cannot be checked ([`files::unlinked_entries`]),
    /// the derivations directory or any component of a referenced document
    /// or log is missing or a symbolic link ([`unlinked`]), or a referenced
    /// file cannot be resolved; every entry is kept. A due entry that is a
    /// symbolic link is dropped without deleting it; one that is not a
    /// regular file keeps its entry. A regular file that resolves to a file
    /// the pending manifest references is dropped without deleting it; one
    /// that resolves to a file the published manifest this transaction
    /// started from references keeps its entry for a later commit (that
    /// manifest stays current until this commit publishes, and a reader may
    /// hold it); otherwise it is deleted.
    fn delete_superseded(&mut self) {
        let root = self.store.root();
        let now = self.now;
        let starting = &self.starting;
        let Manifest {
            logs,
            documents,
            superseded,
            ..
        } = &mut self.manifest;
        let pending = || documents.values().chain(logs.keys()).map(String::as_str);
        let published = || {
            starting
                .documents
                .values()
                .chain(starting.logs.keys())
                .map(String::as_str)
        };
        let Some(named) = pending().map(location).collect::<Option<HashSet<String>>>() else {
            return;
        };
        if published().any(|file| location(file).is_none()) {
            return;
        }
        superseded.retain(|file, _| replaced_document(file, &named));
        let due = |replaced: &UtcTime| now.unix() - replaced.unix() > GRACE_SECONDS;
        if !superseded.values().any(due) {
            return;
        }
        if !files::unlinked_entries(root, &[MANIFEST_FILE, LOCK_FILE, "observations"])
            || !iter::once("derivations")
                .chain(pending())
                .chain(published())
                .all(|file| unlinked(root, file))
        {
            return;
        }
        let resolved = |files: &mut dyn Iterator<Item = &str>| {
            files
                .map(|file| fs::canonicalize(root.join(file)))
                .collect::<io::Result<HashSet<PathBuf>>>()
        };
        let (Ok(current), Ok(held)) = (resolved(&mut pending()), resolved(&mut published())) else {
            return;
        };
        superseded.retain(|file, replaced| {
            if !due(replaced) {
                return true;
            }
            let entry = root.join(file);
            let deleted = match fs::symlink_metadata(&entry) {
                Ok(metadata) if metadata.is_symlink() => return false,
                Ok(metadata) if !metadata.is_file() => return true,
                Ok(_) => match fs::canonicalize(&entry) {
                    Ok(target) if current.contains(&target) => return false,
                    Ok(target) if held.contains(&target) => return true,
                    Ok(_) => fs::remove_file(&entry),
                    Err(e) => Err(e),
                },
                Err(e) => Err(e),
            };
            match deleted {
                Ok(()) => false,
                Err(e) => e.kind() != io::ErrorKind::NotFound,
            }
        });
    }
}

/// Whether each component of `file`, a plain path inside the record at
/// `root`, is on disk and none is a symbolic link, so nothing reached
/// through `file` is reached through a link.
fn unlinked(root: &Path, file: &str) -> bool {
    let mut path = root.to_path_buf();
    Path::new(file).components().all(|component| {
        path.push(component);
        fs::symlink_metadata(&path).is_ok_and(|metadata| !metadata.is_symlink())
    })
}

/// Whether `file`, an entry of a manifest's `superseded`, names a
/// derivation document the manifest no longer references: a name
/// [`WriteTx::put_derivations`] writes directly in the derivations
/// directory ([`document_name`]), never an absolute path, `..`, another
/// directory, or another name, whose [`location`] is none of `referenced`,
/// those of the manifest's document files and logs.
fn replaced_document(file: &str, referenced: &HashSet<String>) -> bool {
    file.strip_prefix("derivations/").is_some_and(document_name)
        && location(file).is_some_and(|location| !referenced.contains(&location))
}

/// Whether `name` is one a derivation document is written under:
/// `<number>.r<revision>.json`, both numbers in decimal digits.
fn document_name(name: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    name.strip_suffix(".json")
        .and_then(|stem| stem.split_once(".r"))
        .is_some_and(|(number, revision)| digits(number) && digits(revision))
}

/// The file `file`, a path relative to the record root, names, spelled
/// one way for comparison: [`normalized`], with ASCII letters in lowercase
/// (a file system may ignore case). `None` unless `file` is a plain path
/// inside the record.
fn location(file: &str) -> Option<String> {
    normalized(file).map(|plain| plain.to_ascii_lowercase())
}

/// `file`, a path relative to the record root, as a plain path: as
/// [`resolved`], but `None` when `file` holds a `..`, so that it is not a
/// plain path inside the record.
fn normalized(file: &str) -> Option<String> {
    let parent = Path::new(&file.replace('\\', "/"))
        .components()
        .any(|component| component == Component::ParentDir);
    if parent { None } else { resolved(file) }
}

/// The name under which a document a manifest stopped referencing as
/// `replaced` is listed as replaced: when the file that reference reaches on
/// disk (through an absolute path, a link, or any spelling) is a file
/// directly in the record's derivations directory, `derivations/<its name>`
/// in lowercase, as the record names its documents (a file system may
/// ignore case); otherwise the reference resolved lexically inside the
/// record ([`resolved`]), in lowercase; otherwise as written.
fn listed(root: &Path, replaced: &str) -> String {
    let on_disk = || {
        let dir = fs::canonicalize(root.join(DERIVATIONS_DIR)).ok()?;
        let file = fs::canonicalize(root.join(replaced)).ok()?;
        let name = file.file_name()?.to_str()?.to_ascii_lowercase();
        (file.parent() == Some(dir.as_path())).then(|| format!("{DERIVATIONS_DIR}/{name}"))
    };
    on_disk()
        .or_else(|| resolved(replaced).map(|file| file.to_ascii_lowercase()))
        .unwrap_or_else(|| replaced.to_owned())
}

/// `file`, a path relative to the record root, resolved lexically: its
/// names joined by `/`, with `\` read as a separator, `.` dropped, and
/// each `..` removing the name before it. `None` when `file` is empty,
/// absolute, or leaves the record.
fn resolved(file: &str) -> Option<String> {
    let unified = file.replace('\\', "/");
    let mut names = Vec::new();
    for component in Path::new(&unified).components() {
        match component {
            Component::Normal(name) => names.push(name.to_str()?),
            Component::CurDir => {}
            Component::ParentDir => {
                names.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!names.is_empty()).then(|| names.join("/"))
}
