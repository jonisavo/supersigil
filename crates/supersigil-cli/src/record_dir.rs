//! Where records live and which record belongs to a checkout.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use supersigil_record::RecordId;
use supersigil_record::store::{Association, Store, StoreError};

use crate::error::CliError;

/// Resolves the records directory: the flag, then `SUPERSIGIL_RECORD_DIR`,
/// then `$XDG_DATA_HOME/supersigil/records`, then
/// `$HOME/.local/share/supersigil/records`.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] if neither `XDG_DATA_HOME` nor `HOME`
/// is set and no flag was given.
pub fn resolve_record_dir(flag: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(dir) = flag {
        return Ok(dir.to_path_buf());
    }
    if let Some(dir) = std::env::var_os("SUPERSIGIL_RECORD_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg).join("supersigil").join("records"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".local/share/supersigil/records"));
    }
    Err(CliError::CommandFailed(
        "cannot locate the records directory: set SUPERSIGIL_RECORD_DIR or HOME".to_owned(),
    ))
}

/// The checkout to operate on, canonicalized.
///
/// # Errors
///
/// Returns an I/O error if the path does not exist.
pub fn canonical_checkout(flag: Option<&Path>) -> Result<PathBuf, CliError> {
    let path = match flag {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Ok(path.canonicalize()?)
}

/// Finds the record associated with `checkout`, if any. Two records
/// claiming the same checkout is an error that names both; picking one
/// silently would hide evidence written to the other.
///
/// # Errors
///
/// Returns [`CliError::Io`] if the records directory exists but cannot be
/// read, a store error if a record's manifest cannot be accessed or read
/// (only entries without a manifest are skipped, so an unreadable record is
/// never mistaken for an absent one), or [`CliError::CommandFailed`] on
/// duplicate associations.
pub fn find_record(records_dir: &Path, checkout: &Path) -> Result<Option<Store>, CliError> {
    let entries = match std::fs::read_dir(records_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        candidates.push(entry?.path());
    }
    candidates.sort();
    let mut matches = Vec::new();
    for candidate in candidates {
        let store = match Store::open(&candidate) {
            Ok(store) => store,
            Err(StoreError::NotARecord(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        if store
            .manifest()?
            .associations
            .iter()
            .any(|a| a.checkout == checkout)
        {
            matches.push(store);
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => {
            let roots: Vec<String> = matches
                .iter()
                .map(|s| s.root().to_string_lossy().into_owned())
                .collect();
            // The paths come from the file system; the binary escapes every
            // error message before it reaches a terminal.
            Err(CliError::CommandFailed(format!(
                "{} is associated with several records: {}. Remove or merge all but one.",
                checkout.to_string_lossy(),
                roots.join(", ")
            )))
        }
    }
}

/// Finds the record for `checkout` or creates one at `<records_dir>/<id>`.
/// Lookup and creation happen under an exclusive lock on
/// `<records_dir>/.lock`, so two first syncs racing on one checkout cannot
/// both create a record.
///
/// # Errors
///
/// Returns a store error if the record cannot be created or read, or an I/O
/// error if the records directory or its lock cannot be created.
pub fn open_or_create_record(records_dir: &Path, checkout: &Path) -> Result<Store, CliError> {
    std::fs::create_dir_all(records_dir)?;
    let guard = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(records_dir.join(".lock"))?;
    guard.lock()?;
    let store = if let Some(store) = find_record(records_dir, checkout)? {
        store
    } else {
        let id = RecordId::generate();
        let root = records_dir.join(id.as_str());
        Store::create_with_id(
            &root,
            id,
            Association {
                checkout: checkout.to_path_buf(),
            },
        )?
    };
    drop(guard);
    Ok(store)
}
