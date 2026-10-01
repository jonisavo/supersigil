//! Brings every record a review involves up to date before it is read.
//!
//! One review can involve several records: a sibling worktree outside the
//! main checkout has its own, and `session sync --checkout <subdirectory>`
//! creates one associated only with that subdirectory. [`reconcile`] finds
//! them all, syncs each from the transcripts it can reach, and bounds every
//! lock wait, so a stop hook holding a record never blocks a review; what it
//! could not reconcile is reported, never hidden.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use supersigil_record::RecordId;
use supersigil_record::observations::Observation;
use supersigil_record::store::{RecordSnapshot, Store, StoreError};
use supersigil_session::checkout::{Placement, canonical, placement};
use supersigil_session::claude_code::parse_transcript;
use supersigil_session::discover::{discover_transcripts, encode_project_dir, transcripts_in};
use supersigil_session::sync::{SyncError, sync};

use crate::error::CliError;
use crate::record_dir::{Acquired, RETRY, open_or_create_record_within};

/// How long reconcile waits for each lock before reporting it held.
pub const LOCK_WAIT: Duration = Duration::from_secs(2);

/// Reason given for a record whose writer lock stayed held.
const WRITER_LOCKED: &str = "locked by another writer";
/// Reason given for a checkout whose record could not be created or
/// associated because the records-directory lock stayed held.
const DIRECTORY_LOCKED: &str = "records directory locked by another writer";

/// A record the review reads, and whether reconcile brought it up to date.
#[derive(Debug)]
pub struct InvolvedRecord {
    /// The record.
    pub store: Store,
    /// The record's id, read from its manifest.
    pub record_id: RecordId,
    /// Why the record was not reconciled, or `None` if it was.
    pub not_reconciled: Option<String>,
}

/// A checkout whose discovered transcripts have no record yet and could not
/// get one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreconciledCheckout {
    /// Working directory the transcripts recorded.
    pub checkout: PathBuf,
    /// Number of transcripts discovered for it.
    pub transcripts: usize,
    /// Why no record was created or associated.
    pub reason: String,
}

/// The records a review involves after reconciling them, and what was left out.
#[derive(Debug)]
pub struct Reconciliation {
    /// Every involved record, sorted by record directory.
    pub records: Vec<InvolvedRecord>,
    /// Checkouts with discovered transcripts that no record could take.
    pub unreconciled: Vec<UnreconciledCheckout>,
}

/// Finds every record with an association equal to, inside, or containing
/// one of `worktrees`, sorted by record directory.
///
/// Paths are compared with [`placement`], which normalizes their spelling.
/// A worktree can have several records (a sibling worktree's own, a
/// subdirectory's), and one record can cover several worktrees. Entries of
/// `records_dir` without a manifest are skipped; a missing `records_dir`
/// has no records.
///
/// # Errors
///
/// Returns [`CliError::Io`] if `records_dir` exists but cannot be read, or a
/// store error if a record's manifest cannot be accessed or read.
pub fn involved_records(records_dir: &Path, worktrees: &[PathBuf]) -> Result<Vec<Store>, CliError> {
    let entries = match std::fs::read_dir(records_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        candidates.push(entry?.path());
    }
    candidates.sort();
    let mut involved = Vec::new();
    for candidate in candidates {
        let store = match Store::open(&candidate) {
            Ok(store) => store,
            Err(StoreError::NotARecord(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let touches = store.manifest()?.associations.iter().any(|a| {
            worktrees.iter().any(|w| {
                placement(&a.checkout, w) != Placement::Outside
                    || placement(w, &a.checkout) != Placement::Outside
            })
        });
        if touches {
            involved.push(store);
        }
    }
    Ok(involved)
}

/// Lists the transcripts in the Claude Code project directories at or below
/// `worktree`: the directory whose name is `worktree` encoded, and every one
/// whose name extends it with `-` (a nested worktree such as
/// `.claude/worktrees/<name>`, or a session started in a subdirectory).
///
/// The encoding maps every non-alphanumeric character to `-`, so a sibling
/// such as `repo-other` looks nested too; callers admit transcripts by the
/// working directory they record. Directories are scanned in name order;
/// within each, main transcripts come before subagent ones. A missing
/// `projects` directory has no transcripts.
///
/// # Errors
///
/// Returns an I/O error if an existing directory or entry cannot be read.
pub fn transcripts_below(claude_home: &Path, worktree: &Path) -> std::io::Result<Vec<PathBuf>> {
    let projects = claude_home.join("projects");
    let own = encode_project_dir(worktree);
    let nested = format!("{own}-");
    let entries = match std::fs::read_dir(&projects) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if (name == own || name.starts_with(&nested)) && entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    let mut found = Vec::new();
    for dir in dirs {
        found.extend(transcripts_in(&dir)?);
    }
    Ok(found)
}

/// Syncs every record `worktrees` involve and returns them with what could
/// not be reconciled.
///
/// 1. Each record from [`involved_records`] is synced once per association,
///    with that association as the checkout, from the transcripts
///    discovered for it under `claude_home` plus the transcripts the record
///    already holds cursors for whose recorded checkout lies under it (a
///    cursor whose session never recorded a checkout is fed to every
///    association; sync admits it only where it belongs). A transcript
///    that no longer exists is left for later.
/// 2. Transcripts that step 1 did not read are gathered from two places
///    for each worktree: the project directories at or below it
///    ([`transcripts_below`]), whose transcripts are admitted when the
///    working directory they record lies inside the worktree; and the
///    project directory of each of its ancestors up to and including
///    `main_worktree`, the root of the repository's main worktree, whose
///    transcripts are admitted when their working directory lies at or
///    below that ancestor. The ancestors cover a session in the main
///    checkout that edited files inside a nested worktree: Claude Code
///    names its project directory after the main checkout. Admitted
///    transcripts are grouped by the working directory they record and get
///    a record exactly as `session sync` would create or associate one.
///    Directories whose encoded name only looks like the right one are
///    rejected by the working directory check.
///
/// Every lock is tried for up to `wait`: a record whose writer lock stays
/// held is returned with [`InvolvedRecord::not_reconciled`] set, and a
/// checkout whose record cannot be created or associated because the
/// records-directory lock stays held is returned in
/// [`Reconciliation::unreconciled`] with its transcript count.
///
/// # Errors
///
/// Returns [`CliError::Io`] if a directory or transcript cannot be read, a
/// store error if a record cannot be read or written for a reason other
/// than a held lock, or a sync error if a transcript cannot be synced.
pub fn reconcile(
    records_dir: &Path,
    claude_home: Option<&Path>,
    worktrees: &[PathBuf],
    main_worktree: &Path,
    wait: Duration,
) -> Result<Reconciliation, CliError> {
    let mut reasons: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut fed: BTreeSet<PathBuf> = BTreeSet::new();
    for store in involved_records(records_dir, worktrees)? {
        if let Some(reason) = sync_record(&store, claude_home, wait, &mut fed)? {
            reasons.insert(store.root().to_path_buf(), reason);
        }
    }

    let mut unreconciled = Vec::new();
    if let Some(home) = claude_home {
        for (checkout, transcripts) in unowned_transcripts(home, worktrees, main_worktree, &fed)? {
            match open_or_create_record_within(records_dir, &checkout, wait)? {
                Acquired::Ready(store) => {
                    if let Some(reason) = sync_within(&store, &checkout, &transcripts, wait)? {
                        reasons.insert(store.root().to_path_buf(), reason);
                    }
                }
                Acquired::Busy => unreconciled.push(UnreconciledCheckout {
                    checkout,
                    transcripts: transcripts.len(),
                    reason: DIRECTORY_LOCKED.to_owned(),
                }),
            }
        }
    }

    let mut records = Vec::new();
    for store in involved_records(records_dir, worktrees)? {
        let record_id = store.manifest()?.record_id;
        let not_reconciled = reasons.get(store.root()).cloned();
        records.push(InvolvedRecord {
            store,
            record_id,
            not_reconciled,
        });
    }
    Ok(Reconciliation {
        records,
        unreconciled,
    })
}

/// Syncs one record per association, adding every transcript it reads to
/// `fed`. Returns the reason it stopped early, if a lock stayed held.
///
/// # Errors
///
/// Returns the errors of [`reconcile`].
fn sync_record(
    store: &Store,
    claude_home: Option<&Path>,
    wait: Duration,
    fed: &mut BTreeSet<PathBuf>,
) -> Result<Option<String>, CliError> {
    let snapshot = store.snapshot()?;
    let recorded = recorded_checkouts(&snapshot)?;
    let manifest = snapshot.manifest();
    for association in &manifest.associations {
        let mut transcripts: Vec<PathBuf> = Vec::new();
        if let Some(home) = claude_home {
            transcripts.extend(discover_transcripts(&association.checkout, home)?);
        }
        for key in manifest.cursors.keys() {
            let under = recorded.get(key).is_none_or(|checkout| {
                placement(checkout, &association.checkout) != Placement::Outside
            });
            let path = PathBuf::from(key);
            if under && is_present_file(&path)? && !transcripts.contains(&path) {
                transcripts.push(path);
            }
        }
        if transcripts.is_empty() {
            continue;
        }
        for transcript in &transcripts {
            fed.insert(identity(transcript)?);
        }
        if let Some(reason) = sync_within(store, &association.checkout, &transcripts, wait)? {
            return Ok(Some(reason));
        }
    }
    Ok(None)
}

/// Maps each transcript path the record holds a cursor for to the checkout
/// its session start recorded.
///
/// # Errors
///
/// Returns a store error if the snapshot's logs cannot be read.
fn recorded_checkouts(snapshot: &RecordSnapshot) -> Result<BTreeMap<String, PathBuf>, CliError> {
    let mut checkouts = BTreeMap::new();
    for session in snapshot.sessions() {
        for observation in snapshot.observations(&session)? {
            if let Observation::SessionStart(start) = observation
                && let Some(path) = start.source_ids.get("path")
            {
                checkouts.entry(path.clone()).or_insert(start.checkout);
            }
        }
    }
    Ok(checkouts)
}

/// Transcripts that no involved record read, grouped by the canonical
/// working directory they record: those below each worktree whose working
/// directory lies inside it, and those in the project directory of each of
/// the worktree's ancestors up to `main_worktree` whose working directory
/// lies at or below that ancestor.
///
/// # Errors
///
/// Returns [`CliError::Io`] if a directory or transcript cannot be read.
fn unowned_transcripts(
    claude_home: &Path,
    worktrees: &[PathBuf],
    main_worktree: &Path,
    fed: &BTreeSet<PathBuf>,
) -> Result<BTreeMap<PathBuf, Vec<PathBuf>>, CliError> {
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for worktree in worktrees {
        let mut sources: Vec<(PathBuf, PathBuf)> = transcripts_below(claude_home, worktree)?
            .into_iter()
            .map(|transcript| (transcript, worktree.clone()))
            .collect();
        for ancestor in ancestors_within(worktree, main_worktree) {
            let dir = claude_home
                .join("projects")
                .join(encode_project_dir(&ancestor));
            for transcript in transcripts_in(&dir)? {
                sources.push((transcript, ancestor.clone()));
            }
        }
        for (transcript, admit_under) in sources {
            if fed.contains(&identity(&transcript)?) {
                continue;
            }
            let bytes = std::fs::read(&transcript)?;
            let Some(cwd) = parse_transcript(&bytes, 0).checkout else {
                continue;
            };
            if placement(&cwd, &admit_under) == Placement::Outside {
                continue;
            }
            let key = identity(&cwd)?;
            let group = groups.entry(key).or_default();
            if !group.contains(&transcript) {
                group.push(transcript);
            }
        }
    }
    Ok(groups)
}

/// The proper ancestors of `worktree` that lie at or below `main_worktree`,
/// nearest first: none for the main worktree itself or for a sibling
/// worktree outside it.
fn ancestors_within(worktree: &Path, main_worktree: &Path) -> Vec<PathBuf> {
    worktree
        .ancestors()
        .skip(1)
        .take_while(|ancestor| placement(ancestor, main_worktree) != Placement::Outside)
        .map(Path::to_path_buf)
        .collect()
}

/// Syncs `transcripts` into `store` for `checkout`, retrying every 50 ms
/// while the record is locked or another writer commits first, for up to
/// `wait`. Returns the reason when the wait runs out.
///
/// # Errors
///
/// Returns a sync error other than a held lock or a lost race.
fn sync_within(
    store: &Store,
    checkout: &Path,
    transcripts: &[PathBuf],
    wait: Duration,
) -> Result<Option<String>, CliError> {
    let deadline = Instant::now() + wait;
    loop {
        match sync(store, checkout, transcripts) {
            Ok(_) => return Ok(None),
            Err(SyncError::Store(StoreError::Locked(_) | StoreError::Conflict { .. })) => {
                if Instant::now() >= deadline {
                    return Ok(Some(WRITER_LOCKED.to_owned()));
                }
                std::thread::sleep(RETRY);
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Whether `path` is an existing file. A missing path is `false`, so a
/// transcript deleted since the cursor was written is left for later; any
/// other failure to look at it is an error.
///
/// # Errors
///
/// Returns [`CliError::Io`] if the path cannot be inspected for a reason
/// other than not existing.
fn is_present_file(path: &Path) -> Result<bool, CliError> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// The path sync keys a path by: canonical when it exists, as written when
/// it does not.
///
/// # Errors
///
/// Returns [`CliError::Io`] if the path exists but cannot be resolved.
fn identity(path: &Path) -> Result<PathBuf, CliError> {
    match canonical(path) {
        Ok(resolved) => Ok(resolved),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(e) => Err(e.into()),
    }
}
