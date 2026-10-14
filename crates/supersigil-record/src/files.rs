//! File operations shared by the record and review stores: the writer lock,
//! the manifest, pinned logs, and durable publication.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::store::{MANIFEST_FILE, StoreError, io_error};

/// Checks that `root/manifest.json` is a file, reading metadata only.
///
/// # Errors
///
/// Returns [`StoreError::NotARecord`] if the path is missing, `root` is not
/// a directory, or the manifest is not a file. Returns [`StoreError::Io`]
/// for other metadata errors, such as denied permission.
pub(crate) fn check_manifest(root: &Path) -> Result<(), StoreError> {
    let path = root.join(MANIFEST_FILE);
    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(StoreError::NotARecord(root.to_path_buf())),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Err(StoreError::NotARecord(root.to_path_buf()))
        }
        Err(e) => Err(io_error(&path, e)),
    }
}

/// The one manifest field every layout shares, read before the rest.
#[derive(serde::Deserialize)]
struct Versioned {
    schema_version: u32,
}

/// Reads `root/manifest.json` and checks that its version is `supported`.
///
/// Reads the version first so other layouts fail on their version, not a
/// missing field. Rejecting other versions also prevents writers from
/// dropping unknown fields when they serialize the manifest again.
///
/// # Errors
///
/// Returns [`StoreError::NotARecord`] for a missing manifest,
/// [`StoreError::Io`] for other read failures, [`StoreError::Json`] for
/// invalid manifest JSON, or what `unsupported` makes of another version.
pub(crate) fn read_manifest<T: DeserializeOwned>(
    root: &Path,
    supported: u32,
    unsupported: impl FnOnce(u32) -> StoreError,
) -> Result<T, StoreError> {
    let path = root.join(MANIFEST_FILE);
    let bytes = fs::read(&path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            StoreError::NotARecord(root.to_path_buf())
        } else {
            io_error(&path, e)
        }
    })?;
    let json = |source| StoreError::Json {
        path: path.clone(),
        source,
    };
    let versioned: Versioned = serde_json::from_slice(&bytes).map_err(json)?;
    if versioned.schema_version != supported {
        return Err(unsupported(versioned.schema_version));
    }
    serde_json::from_slice(&bytes).map_err(json)
}

/// Publishes a complete initial manifest without replacing an existing one
/// (see [`create_new`]).
///
/// # Errors
///
/// Returns [`StoreError::AlreadyExists`] if the manifest already exists,
/// [`StoreError::Json`] if serialization fails, or [`StoreError::Io`] if a
/// file or directory operation fails.
pub(crate) fn create_manifest(root: &Path, manifest: &impl Serialize) -> Result<(), StoreError> {
    create_new(root, MANIFEST_FILE, &manifest_json(root, manifest)?)
}

/// Replaces the manifest by rename (see [`replace`]).
///
/// # Errors
///
/// Returns [`StoreError::Json`] if serialization fails or [`StoreError::Io`]
/// if writing, renaming, or syncing fails.
pub(crate) fn publish_manifest(root: &Path, manifest: &impl Serialize) -> Result<(), StoreError> {
    replace(root, MANIFEST_FILE, &manifest_json(root, manifest)?)
}

/// Serializes a manifest as pretty JSON.
///
/// # Errors
///
/// Returns [`StoreError::Json`] if serialization fails.
fn manifest_json(root: &Path, manifest: &impl Serialize) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec_pretty(manifest).map_err(|source| StoreError::Json {
        path: root.join(MANIFEST_FILE),
        source,
    })
}

/// Takes the operating-system lock on `dir/name` without waiting, creating
/// the file if needed. The lock lasts as long as the returned file.
///
/// # Errors
///
/// Returns [`StoreError::Locked`] naming `dir` if another writer holds the
/// lock, or [`StoreError::Io`] if the file cannot be opened or locked.
pub(crate) fn lock(dir: &Path, name: &str) -> Result<File, StoreError> {
    let path = dir.join(name);
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| io_error(&path, e))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(StoreError::Locked(dir.to_path_buf())),
        Err(TryLockError::Error(e)) => Err(io_error(&path, e)),
    }
}

/// Whether each of `names`, a store's own entries in its directory `dir`,
/// is missing or, inspected without following it, not a symbolic link.
/// Cleanup deletes nothing otherwise: what a linked entry leads to, or
/// what an entry that cannot be inspected is, could be a file it deletes.
pub(crate) fn unlinked_entries(dir: &Path, names: &[&str]) -> bool {
    names
        .iter()
        .all(|name| match fs::symlink_metadata(dir.join(name)) {
            Ok(metadata) => !metadata.is_symlink(),
            Err(e) => e.kind() == io::ErrorKind::NotFound,
        })
}

/// Publishes `dir/name` with `bytes` without replacing an existing file.
///
/// Writes a temporary file ([`write_temporary`]), then hard-links it to
/// `name`. If two creators race, only one can create that link. Removes the
/// temporary name after the link attempt. An interrupted creator may leave
/// a temporary file, which later attempts do not reuse.
///
/// # Errors
///
/// Returns [`StoreError::AlreadyExists`] naming `dir` if `dir/name` already
/// exists, or [`StoreError::Io`] if a file or directory operation fails.
fn create_new(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
    let path = dir.join(name);
    let tmp_path = write_temporary(dir, name, bytes)?;
    let linked = fs::hard_link(&tmp_path, &path);
    fs::remove_file(&tmp_path).map_err(|e| io_error(&tmp_path, e))?;
    linked.map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            StoreError::AlreadyExists(dir.to_path_buf())
        } else {
            io_error(&path, e)
        }
    })?;
    sync_dir(dir)
}

/// Replaces `dir/name` with `bytes` by writing a temporary file
/// ([`write_temporary`]), renaming it to `name`, then syncing `dir`. A
/// failed rename removes the temporary file.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if writing, renaming, or syncing fails.
pub(crate) fn replace(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
    let final_path = dir.join(name);
    let tmp_path = write_temporary(dir, name, bytes)?;
    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(io_error(&final_path, e));
    }
    sync_dir(dir)
}

/// Writes and syncs `bytes` to a new file `<name>.<uuid>.tmp` in `dir`,
/// with a fresh UUID, and returns its path. The file is created
/// exclusively, so the write never opens, truncates, or follows an
/// existing path. A failed write removes the file.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if creating, writing, or syncing the file
/// fails.
fn write_temporary(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, StoreError> {
    let path = dir.join(format!("{name}.{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| io_error(&path, e))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = fs::remove_file(&path);
        return Err(io_error(&path, e));
    }
    Ok(path)
}

/// The name of the file a temporary file named `name` was written for, if
/// `name` is one [`write_temporary`] gives: `<target>.<uuid>.tmp`.
pub(crate) fn temporary_target(name: &str) -> Option<&str> {
    let (target, unique) = name.strip_suffix(".tmp")?.rsplit_once('.')?;
    uuid::Uuid::try_parse(unique)
        .is_ok_and(|id| id.to_string() == unique)
        .then_some(target)
}

/// Removes bytes of `path` beyond `pinned`, a tail no manifest published,
/// then appends `bytes`. Creates the file if needed; the caller syncs it
/// before publishing.
///
/// # Errors
///
/// Returns [`StoreError::Corrupt`] if the file is shorter than `pinned` or
/// [`StoreError::Io`] if a file operation fails.
pub(crate) fn append_at(path: &Path, pinned: u64, bytes: &[u8]) -> Result<(), StoreError> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|e| io_error(path, e))?;
    let len = file.metadata().map_err(|e| io_error(path, e))?.len();
    if len < pinned {
        return Err(StoreError::Corrupt(format!(
            "{} is shorter ({len} bytes) than its pinned length ({pinned})",
            path.display()
        )));
    }
    file.set_len(pinned).map_err(|e| io_error(path, e))?;
    file.seek(SeekFrom::Start(pinned))
        .map_err(|e| io_error(path, e))?;
    file.write_all(bytes).map_err(|e| io_error(path, e))
}

/// Reads exactly the first `len` bytes of a file.
///
/// # Errors
///
/// Returns [`StoreError::Corrupt`] if the file is too short or
/// [`StoreError::Io`] if opening or reading it fails.
pub(crate) fn read_prefix(path: &Path, len: u64) -> Result<Vec<u8>, StoreError> {
    let file = File::open(path).map_err(|e| io_error(path, e))?;
    let mut bytes = Vec::new();
    file.take(len)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error(path, e))?;
    if bytes.len() as u64 != len {
        return Err(StoreError::Corrupt(format!(
            "{} is shorter ({} bytes) than its pinned length ({len})",
            path.display(),
            bytes.len()
        )));
    }
    Ok(bytes)
}

/// Syncs a written file's contents to disk.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if the file cannot be opened or synced.
pub(crate) fn sync_file(path: &Path) -> Result<(), StoreError> {
    // Write access: Windows flushes through `FlushFileBuffers`, which needs
    // it.
    OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| io_error(path, e))
}

/// Syncs a directory's entries to disk.
///
/// On Windows, a failure to open the directory is ignored because directories
/// cannot be opened as ordinary files there.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if opening fails on a non-Windows platform, or
/// if syncing an opened directory fails on any platform.
pub(crate) fn sync_dir(dir: &Path) -> Result<(), StoreError> {
    match File::open(dir) {
        Ok(handle) => handle.sync_all().map_err(|e| io_error(dir, e)),
        Err(_) if cfg!(windows) => Ok(()),
        Err(e) => Err(io_error(dir, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_replace_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        // A directory in the way: the rename fails.
        fs::create_dir(dir.path().join("name")).unwrap();
        assert!(matches!(
            replace(dir.path(), "name", b"bytes"),
            Err(StoreError::Io { .. })
        ));
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["name"]);
    }

    #[test]
    fn sync_dir_syncs_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        sync_dir(dir.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sync_dir_of_a_missing_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            sync_dir(&dir.path().join("missing")),
            Err(StoreError::Io { .. })
        ));
    }
}
