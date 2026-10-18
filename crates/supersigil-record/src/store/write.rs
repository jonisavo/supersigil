//! Writes record data under an exclusive lock and commits it through the manifest.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File};
use std::path::{Component, Path};
use std::{io, iter};

use super::manifest::{self, Association, Manifest, SourceCursor};
use super::{
    DERIVATIONS_DIR, INTERPRETATIONS_DIR, LOCK_FILE, MANIFEST_FILE, OBSERVATIONS_DIR,
    RecordSnapshot, Store, StoreError, annotation_document, annotation_revisions,
    derivations_document, io_error, log_path,
};
use crate::derivations::DerivationSet;
use crate::files::{self, Publication};
use crate::ids::{Revision, SessionId};
use crate::interpretations::{Annotation, AnnotationDocument, AnnotationRevision};
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
    /// What this transaction changed, synced at commit.
    publication: Publication,
}

impl<'a> WriteTx<'a> {
    /// Acquires the writer lock and reads the starting manifest, using `now`
    /// as this transaction's publication time.
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
            publication: Publication::new(store.root(), manifest.revision),
            starting: manifest.clone(),
            manifest,
            now,
            _lock: lock,
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
        self.publication.touched(path);
        Ok(())
    }

    /// Writes a session's derivations to a file named for the next store revision.
    ///
    /// Assigns a number to a new session. Repeated calls for the same session
    /// replace the pending document. Keeps replaced committed documents for
    /// earlier readers and lists them in `superseded` at the transaction's
    /// time; see [`Self::commit`].
    ///
    /// Writes a new temporary file and renames it into place, replacing any
    /// existing target, including uncommitted files and symbolic links,
    /// without writing through them.
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
        // Repeated writes replace only the unpublished pending file. List a
        // replaced committed document by its cleanup name ([`listed`]) and
        // keep the later replacement time to preserve its full grace period.
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
        self.publication.touched(path);
        Ok(())
    }

    /// Stores `document` as the next annotation revision of `session`, at
    /// the transaction's time, and returns that revision.
    ///
    /// `based_on` must be the session's current revision in the pending
    /// manifest, or `None` when it has none. A second call for the same
    /// session in one transaction therefore builds on the first's revision.
    /// Checks only the session and the revision built on; the caller
    /// validates the document.
    ///
    /// Writes `interpretations/<n>.a<revision>.json` as
    /// [`Self::put_derivations`] writes its file, replacing anything an
    /// uncommitted transaction left at that name. Never lists a revision as
    /// superseded, so every manifest that lists one keeps it readable.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::UnknownSession`] if the pending manifest holds
    /// no observations of `session`, [`StoreError::StaleAnnotation`] if
    /// `based_on` is not its current revision, [`StoreError::Json`] if
    /// serialization fails, or [`StoreError::Io`] if a directory or file
    /// operation fails. Neither refusal writes anything.
    ///
    /// # Panics
    ///
    /// Panics if the current revision is [`u32::MAX`].
    pub fn put_annotation(
        &mut self,
        session: &SessionId,
        based_on: Option<AnnotationRevision>,
        document: AnnotationDocument,
    ) -> Result<AnnotationRevision, StoreError> {
        let Some(&number) = self
            .manifest
            .sessions
            .get(session)
            .filter(|&&number| self.manifest.logs.contains_key(&log_path(number)))
        else {
            return Err(StoreError::UnknownSession(session.clone()));
        };
        let current = annotation_revisions(&self.manifest, number).pop();
        if based_on != current {
            return Err(StoreError::StaleAnnotation { based_on, current });
        }
        let revision = current.map_or(AnnotationRevision::FIRST, AnnotationRevision::next);
        let name = format!("{number}.a{revision}.json");
        let dir = self.store.root().join(INTERPRETATIONS_DIR);
        let path = dir.join(&name);
        let annotation = Annotation {
            revision,
            at: self.now,
            document,
        };
        let bytes = serde_json::to_vec_pretty(&annotation).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
        files::replace(&dir, &name, &bytes)?;
        self.manifest.documents.insert(
            annotation_document(number, revision),
            format!("{INTERPRETATIONS_DIR}/{name}"),
        );
        self.publication.touched(path);
        Ok(revision)
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
    /// First deletes documents superseded more than [`GRACE_SECONDS`] before
    /// the transaction's time and drops their entries. Keeps undeletable files,
    /// including open files on Windows, listed for the next commit. Only
    /// manifests published before the replacement name these documents.
    ///
    /// Drops entries for still-referenced files, unsupported document names,
    /// and symbolic links without deleting them. Deletes only regular files,
    /// without following symbolic links. Skips deletion if the manifest, lock,
    /// observations directory, or derivations directory is a link, or a referenced
    /// path is not plain and inside the record, crosses a link, or cannot be
    /// resolved on disk. Ignores file modification times.
    ///
    /// Then syncs changed files and the directories of changed and deleted
    /// files up to the record root, checks that the on-disk revision still
    /// matches the starting one, increments it, and publishes the manifest by
    /// atomic rename.
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
        self.publication.commit(&mut self.manifest)
    }

    /// Deletes documents superseded more than [`GRACE_SECONDS`] before the
    /// transaction's time and drops their entries. Treats missing files as
    /// deleted and keeps undeletable files listed. Drops unsupported entries
    /// ([`replaced_document`]) without deleting anything. If a referenced
    /// document or log has a non-plain path or leaves the record ([`location`]),
    /// skips deletion and keeps every entry.
    ///
    /// For due entries, skips deletion if the manifest, lock, or observations
    /// directory is linked or unreadable ([`files::unlinked_entries`]), if the
    /// derivations directory or any referenced path component is missing or
    /// linked ([`unlinked`]), or if a referenced file cannot be resolved.
    /// Keeps entries that passed the earlier name check.
    ///
    /// Drops due symbolic-link entries without deleting their links. Keeps
    /// non-regular files listed. Drops regular-file entries without deletion
    /// if their resolved target is still referenced by the pending manifest.
    /// Keeps targets referenced by the starting manifest for a later commit,
    /// since that manifest stays current until publication and readers may
    /// hold it. Deletes other due regular files and records them for
    /// [`Self::commit`] to sync before publishing.
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
        let publication = &mut self.publication;
        let pending: BTreeSet<&str> = documents
            .values()
            .chain(logs.keys())
            .map(String::as_str)
            .collect();
        let published: BTreeSet<&str> = starting
            .documents
            .values()
            .chain(starting.logs.keys())
            .map(String::as_str)
            .collect();
        // Check each path referenced by either manifest once.
        let referenced = || pending.union(&published).copied();
        let mut named = HashSet::new();
        for file in referenced() {
            let Some(spelled) = location(file) else {
                return;
            };
            if pending.contains(file) {
                named.insert(spelled);
            }
        }
        superseded.retain(|file, _| replaced_document(file, &named));
        let due = |replaced: &UtcTime| now.unix() - replaced.unix() > GRACE_SECONDS;
        if !superseded.values().any(due) {
            return;
        }
        if !files::unlinked_entries(root, &[MANIFEST_FILE, LOCK_FILE, OBSERVATIONS_DIR])
            || !iter::once(DERIVATIONS_DIR)
                .chain(referenced())
                .all(|file| unlinked(root, file))
        {
            return;
        }
        let mut current = HashSet::new();
        let mut held = HashSet::new();
        for file in referenced() {
            let Ok(target) = fs::canonicalize(root.join(file)) else {
                return;
            };
            if published.contains(file) {
                held.insert(target.clone());
            }
            if pending.contains(file) {
                current.insert(target);
            }
        }
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
                    Ok(_) => fs::remove_file(&entry).inspect(|()| {
                        publication.deleted(entry.clone());
                    }),
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

/// Checks that every component of the plain record-relative `file` exists
/// and is not a symbolic link.
fn unlinked(root: &Path, file: &str) -> bool {
    let mut path = root.to_path_buf();
    Path::new(file).components().all(|component| {
        path.push(component);
        fs::symlink_metadata(&path).is_ok_and(|metadata| !metadata.is_symlink())
    })
}

/// Checks that `file` names a derivation document directly in the derivations
/// directory ([`document_name`]) and its [`location`] is absent from the
/// manifest's referenced documents and logs. Rejects absolute paths, `..`,
/// other directories, and unsupported names.
fn replaced_document(file: &str, referenced: &HashSet<String>) -> bool {
    file.strip_prefix(DERIVATIONS_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(document_name)
        && location(file).is_some_and(|location| !referenced.contains(&location))
}

/// Checks whether `name` has the `<number>.r<revision>.json` document form,
/// with non-empty decimal numbers.
fn document_name(name: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    name.strip_suffix(".json")
        .and_then(|stem| stem.split_once(".r"))
        .is_some_and(|(number, revision)| digits(number) && digits(revision))
}

/// Normalizes record-relative `file` ([`normalized`]) and lowercases ASCII
/// letters for comparison on file systems that may ignore case. Returns
/// `None` unless the path is plain and stays inside the record.
fn location(file: &str) -> Option<String> {
    normalized(file).map(|plain| plain.to_ascii_lowercase())
}

/// Resolves record-relative `file` as [`resolved`] does, but rejects any
/// `..` component to require a plain path inside the record.
fn normalized(file: &str) -> Option<String> {
    let parent = Path::new(&file.replace('\\', "/"))
        .components()
        .any(|component| component == Component::ParentDir);
    if parent { None } else { resolved(file) }
}

/// Finds the cleanup name for `replaced`. First resolves it on disk, including
/// absolute paths and links. If the target is directly in the record's
/// derivations directory, returns `derivations/<name>` in ASCII lowercase.
/// Otherwise returns its lexically resolved record-relative path ([`resolved`])
/// in ASCII lowercase, or the original spelling if resolution fails.
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

/// Resolves record-relative `file` lexically. Joins names with `/`, treats
/// `\` as a separator, drops `.`, and removes the preceding name for each
/// `..`. Returns `None` for empty or absolute paths, or paths that leave the record.
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

#[cfg(test)]
mod tests;
