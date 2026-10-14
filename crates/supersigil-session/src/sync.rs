//! Appends new transcript events to a record and saves where reading stopped.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use supersigil_record::derivations::ALGORITHM_VERSION;
use supersigil_record::derive::derive;
use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, Edit, Observation, SessionStart, Source,
};
use supersigil_record::store::{Association, SourceCursor, Store, StoreError, WriteTx};
use supersigil_record::{ContentId, Revision, SessionId, Timestamp};

use crate::checkout::{Placement, canonical, file_location, placement, within};
use crate::claude_code::{ParseOutcome, parse_transcript};

/// Failure to read a transcript or update the record during sync.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// A record operation failed, such as locking, reading, or committing.
    #[error("{0}")]
    Store(#[from] StoreError),
    /// A transcript could not be read.
    #[error("cannot read transcript {path}: {source}")]
    Io {
        /// Transcript path.
        path: PathBuf,
        /// I/O error returned when reading the transcript.
        #[source]
        source: std::io::Error,
    },
}

/// Observations added, bytes consumed, and capture problems for one transcript.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TranscriptReport {
    /// Transcript path supplied to [`sync`].
    pub path: PathBuf,
    /// Session ID saved in the transcript's cursor, if known.
    pub session: Option<SessionId>,
    /// Number of observations appended, including start and capture-limitation events.
    pub new_observations: usize,
    /// Bytes newly consumed, measured from the previous cursor offset.
    /// After a transcript rewrite, counting starts at byte zero.
    pub consumed: u64,
    /// Whether the input ended without a newline. Always `false` for skipped transcripts.
    pub trailing_partial: bool,
    /// Capture problems counted during this sync, serialized as fields of this
    /// object. Counts are cleared when the transcript belongs to another checkout.
    #[serde(flatten)]
    pub counts: CaptureCounts,
    /// Reason the transcript was skipped, or `None` if it was accepted.
    /// Skipped transcripts name an outside checkout, have no known checkout,
    /// or were [removed before they could be read](REMOVED_BEFORE_READ).
    /// They add no observations and leave their stored cursors unchanged.
    #[serde(default)]
    pub skipped: Option<String>,
    /// Transcript checkout path when it is a descendant of the requested checkout.
    /// Sync preserves this path in observations without adding a record association.
    #[serde(default)]
    pub nested_checkout: Option<PathBuf>,
}

/// Revision and per-transcript results returned by [`sync`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SyncReport {
    /// Record revision after the call.
    pub revision: Revision,
    /// IDs of sessions that received observations, sorted by ID.
    pub sessions: Vec<SessionId>,
    /// Total number of observations appended across all transcripts.
    pub new_observations: usize,
    /// Results for each transcript, in the order supplied to [`sync`].
    pub transcripts: Vec<TranscriptReport>,
}

/// Imports new observations and recomputes derivations for sessions that changed.
///
/// Reads transcripts in the supplied order while holding the record's writer
/// lock. Each file is parsed from the start for context, but only observations
/// after its saved cursor are appended. If previously consumed bytes have
/// changed, the cursor resets and observations are appended from the start.
///
/// Commits one revision if observations were added or cursors changed.
/// Otherwise returns the existing revision. Capture problems are saved as
/// [`CaptureLimitation`] observations in the same commit as the cursor update.
///
/// Derivations group each session's edits by [`file_location`], so edits of
/// one file are compared whatever directory the session named it from. A
/// commit also derives again every other session's stored set that an older
/// [`ALGORITHM_VERSION`] produced, from the observations already recorded.
///
/// The first working directory recorded for a transcript's session determines
/// whether it is accepted. It must be `checkout` or a descendant, as checked
/// by [`placement`]. An outside or unknown checkout is reported as skipped.
/// Individual edits and commands issued from outside every checkout
/// associated with the record are also omitted and counted, even in an
/// accepted transcript. Nested
/// checkouts keep their paths; sync does not add checkout associations.
///
/// Adds a [`SessionStart`] when a transcript's cursor first learns its session.
/// Main and subagent transcripts can therefore add starts for the same session.
/// Use [`session_start`](supersigil_record::observations::session_start) to
/// choose the session's display metadata.
///
/// Cursor keys and observation transcript paths use the canonical file path,
/// or the supplied path if the file vanishes before resolution. Line
/// positions are comparable only within one transcript.
///
/// A transcript that is gone, or no longer a regular file, when read is
/// reported as skipped with the reason [`REMOVED_BEFORE_READ`], and its
/// cursor and recorded observations stay. If it reappears, a later sync
/// resumes from that cursor once the consumed bytes still match.
///
/// # Errors
///
/// Returns [`SyncError::Io`] if a transcript cannot be read for another
/// reason or its path cannot be resolved, or [`SyncError::Store`] if
/// locking, reading, writing, or committing the record fails.
///
/// # Panics
///
/// Panics if a commit is needed and the current revision is [`u64::MAX`].
pub fn sync(
    store: &Store,
    checkout: &Path,
    transcripts: &[PathBuf],
) -> Result<SyncReport, SyncError> {
    let mut tx = store.begin()?;
    // Taken under the lock. Derivations below are computed from this
    // snapshot plus the observations this transaction appends, so another
    // writer's commit cannot slip in between reading and deriving.
    let pinned = tx.snapshot();

    let mut reports = Vec::new();
    let mut appended: BTreeMap<SessionId, Vec<Observation>> = BTreeMap::new();
    let mut total = 0usize;

    for path in transcripts {
        let report = sync_transcript(&mut tx, checkout, path, &mut appended)?;
        total += report.new_observations;
        reports.push(report);
    }

    let cursors_moved = tx.manifest().cursors != pinned.manifest().cursors;
    if total == 0 && !cursors_moved {
        return Ok(SyncReport {
            revision: pinned.revision(),
            sessions: Vec::new(),
            new_observations: 0,
            transcripts: reports,
        });
    }

    // Edits of one file are compared however the session spelled it.
    let key = |edit: &Edit| Some(file_location(&edit.checkout, &edit.path));
    let next_revision = tx.manifest().revision.next();
    let mut sessions = Vec::with_capacity(appended.len());
    for (session, new) in appended {
        let mut observations = pinned.observations(&session)?;
        observations.extend(new);
        tx.put_derivations(&derive(&session, &observations, next_revision, key))?;
        sessions.push(session);
    }
    // Sets an older algorithm derived are derived again from the pinned
    // observations, so no set this revision lists is stale.
    for session in pinned.sessions() {
        if sessions.contains(&session) {
            continue;
        }
        if let Some(set) = pinned.derivations(&session)?
            && set.algorithm_version < ALGORITHM_VERSION
        {
            let observations = pinned.observations(&session)?;
            tx.put_derivations(&derive(&session, &observations, pinned.revision(), key))?;
        }
    }
    let revision = tx.commit()?;
    Ok(SyncReport {
        revision,
        sessions,
        new_observations: total,
        transcripts: reports,
    })
}

/// Reason a transcript is skipped when it was listed for reading but was
/// gone, or no longer a regular file, when read ([`read_transcript`]).
pub const REMOVED_BEFORE_READ: &str = "removed before it could be read";

/// Reads the transcript at `path`, or returns `None` if it was removed
/// before it could be read: it is not found, or what is there, following a
/// symbolic link, is no longer a regular file. The type is checked before
/// the file is opened, since a device reads as anything, and again on the
/// opened file, which may have replaced it since. Opening never waits: on
/// Unix the file is opened non-blocking, since opening a FIFO to read
/// otherwise waits for a writer.
///
/// # Errors
///
/// Returns the I/O error of inspecting what is at `path`, or of opening or
/// reading the regular file there.
pub fn read_transcript(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let regular = crate::found(std::fs::metadata(path))?.is_some_and(|metadata| metadata.is_file());
    if !regular {
        return Ok(None);
    }
    let Some(mut file) = open_regular(path)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// Opens the file at `path` to read it, or returns `None` if it is no
/// longer a regular file: what was inspected was removed or replaced since.
/// On Unix it is opened non-blocking, so a FIFO opens at once and is then
/// skipped by its type; reading a regular file is unaffected.
///
/// # Errors
///
/// Returns the I/O error of opening the file while a regular file is still
/// there ([`open_failed`]), or of inspecting the opened file.
fn open_regular(path: &Path) -> std::io::Result<Option<File>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    match options.open(path) {
        // Replaced since it was inspected.
        Ok(file) => Ok(file.metadata()?.is_file().then_some(file)),
        Err(error) => open_failed(path, error),
    }
}

/// What `error`, a failure to open `path`, means once what is there is
/// inspected again: a removal (`None`) when nothing or something other than
/// a regular file is there, as when a directory replaced the file (opening
/// one fails on Windows).
///
/// # Errors
///
/// Returns `error` while a regular file is at `path`, or when what is
/// there cannot be inspected.
fn open_failed(path: &Path, error: std::io::Error) -> std::io::Result<Option<File>> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Err(error),
        Ok(_) => Ok(None),
        Err(inspecting) if inspecting.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(error),
    }
}

/// Appends one transcript's new observations and updates its cursor in `tx`.
///
/// Also collects the observations in `appended` for recomputing derivations.
/// Adds a capture-limitation event for parser counts. A transcript removed
/// before it could be read, or with an outside or unknown checkout, is
/// reported as skipped without updating the cursor.
///
/// # Errors
///
/// Returns [`SyncError::Io`] if reading the transcript fails for a reason
/// other than its removal or resolving its path fails, or
/// [`SyncError::Store`] if appending observations fails.
fn sync_transcript(
    tx: &mut WriteTx<'_>,
    checkout: &Path,
    path: &Path,
    appended: &mut BTreeMap<SessionId, Vec<Observation>>,
) -> Result<TranscriptReport, SyncError> {
    let read = read_transcript(path).map_err(|source| SyncError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let Some(bytes) = read else {
        return Ok(Skip::Removed.report(path, None, CaptureCounts::default()));
    };
    // One identity per file, whatever spelling named it: the cursor key and
    // the stamp on every observation.
    let key = transcript_key(path, canonical(path))?;
    let mut cursor = resume_cursor(tx.manifest().cursors.get(&key), &bytes);
    let start = cursor.offset;
    let from_ordinal = cursor.next_ordinal;
    let mut outcome = parse_transcript(&bytes, from_ordinal);
    let (own, place) = match admit(checkout, outcome.checkout.as_deref()) {
        Ok(admitted) => admitted,
        Err(skip) => return Ok(skip.report(path, cursor.session, outcome.counts)),
    };
    drop_outside(&mut outcome, &tx.manifest().associations);
    let nested_checkout = (place != Placement::Same).then(|| own.clone());
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut new = Vec::new();
    if cursor.session.is_none()
        && let Some(session) = &outcome.session
    {
        new.push(Observation::SessionStart(SessionStart {
            session: session.clone(),
            source: Source::ClaudeCode,
            source_ids: BTreeMap::from([
                ("transcript".to_owned(), file_name),
                ("path".to_owned(), key.clone()),
            ]),
            checkout: own.clone(),
            branch: outcome.branch.clone(),
            time: outcome
                .first_time
                .clone()
                .unwrap_or_else(|| Timestamp::new("")),
            sidechain: outcome.sidechain,
        }));
        cursor.session = Some(session.clone());
    }
    let limitation = cursor
        .session
        .as_ref()
        .and_then(|session| capture_limitation(session, &key, from_ordinal, &outcome));
    for observation in &mut outcome.observations {
        stamp_transcript(observation, &key);
    }
    new.append(&mut outcome.observations);
    new.extend(limitation);
    let count = new.len();
    if count > 0 {
        tx.append_observations(&new)?;
        for observation in new {
            appended
                .entry(observation.session().clone())
                .or_default()
                .push(observation);
        }
    }
    if outcome.consumed != start {
        let end = usize::try_from(outcome.consumed)
            .expect("the parse consumes at most the bytes it was given");
        cursor.offset = outcome.consumed;
        cursor.next_ordinal = outcome.next_ordinal;
        cursor.prefix_hash = ContentId::of(&bytes[..end]);
    }
    tx.set_cursor(&key, cursor.clone());
    Ok(TranscriptReport {
        path: path.to_path_buf(),
        session: cursor.session,
        new_observations: count,
        consumed: outcome.consumed - start,
        trailing_partial: outcome.trailing_partial,
        counts: outcome.counts,
        skipped: None,
        nested_checkout,
    })
}

/// Identity of the transcript at `path`: canonical `resolved`, or the
/// written `path` if it vanished after reading.
///
/// # Errors
///
/// Returns [`SyncError::Io`] for other resolution failures. Falling back
/// then would give the transcript a second identity and reread it from the
/// start.
fn transcript_key(path: &Path, resolved: std::io::Result<PathBuf>) -> Result<String, SyncError> {
    let key = crate::found(resolved)
        .map_err(|source| SyncError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .unwrap_or_else(|| path.to_path_buf());
    Ok(key.display().to_string())
}

/// Reason a transcript was not accepted for sync.
enum Skip {
    /// The transcript was removed before it could be read
    /// ([`read_transcript`]).
    Removed,
    /// The parser has no working directory for this transcript.
    Unknown,
    /// The working directory is outside the requested checkout, with a message
    /// naming both paths.
    Outside(String),
}

impl Skip {
    /// Creates a skipped-transcript report with no observations or consumed bytes.
    /// Keeps counts for an unknown checkout and clears them for an outside checkout.
    fn report(
        self,
        path: &Path,
        session: Option<SessionId>,
        counts: CaptureCounts,
    ) -> TranscriptReport {
        let (reason, counts) = match self {
            Self::Removed => (REMOVED_BEFORE_READ.to_owned(), counts),
            Self::Unknown => ("checkout unknown".to_owned(), counts),
            Self::Outside(reason) => (reason, CaptureCounts::default()),
        };
        TranscriptReport {
            path: path.to_path_buf(),
            session,
            new_observations: 0,
            consumed: 0,
            trailing_partial: false,
            counts,
            skipped: Some(reason),
            nested_checkout: None,
        }
    }
}

/// Accepts the transcript's named directory if it equals or is inside `checkout`.
///
/// # Errors
///
/// Returns [`Skip::Unknown`] if no directory is known or [`Skip::Outside`] if
/// the named directory is outside `checkout`.
fn admit(checkout: &Path, named: Option<&Path>) -> Result<(PathBuf, Placement), Skip> {
    let named = named.ok_or(Skip::Unknown)?;
    match placement(named, checkout) {
        Placement::Outside => Err(Skip::Outside(format!(
            "checkout {} does not match {}",
            named.display(),
            checkout.display()
        ))),
        place => Ok((named.to_path_buf(), place)),
    }
}

/// Removes edits and commands whose working directories are outside every
/// associated checkout. Adds the number removed to `outside_checkout`;
/// leaves other observations unchanged.
fn drop_outside(outcome: &mut ParseOutcome, associations: &[Association]) {
    let inside = |dir: &Path| associations.iter().any(|a| within(dir, &a.checkout));
    let before = outcome.observations.len();
    outcome
        .observations
        .retain(|observation| match observation {
            Observation::Edit(edit) => inside(&edit.checkout),
            Observation::Command(command) => inside(&command.checkout),
            _ => true,
        });
    outcome.counts.outside_checkout += (before - outcome.observations.len()) as u64;
}

/// Sets the transcript path on a turn, edit, command, or delegation
/// observation.
fn stamp_transcript(observation: &mut Observation, transcript: &str) {
    let field = match observation {
        Observation::Turn(turn) => &mut turn.transcript,
        Observation::Edit(edit) => &mut edit.transcript,
        Observation::Command(command) => &mut command.transcript,
        Observation::Delegation(delegation) => &mut delegation.transcript,
        _ => return,
    };
    *field = Some(transcript.to_owned());
}

/// Reuses the stored cursor if the previously consumed bytes still match its hash.
/// Returns a cursor at byte zero if none exists, the file shrank, or those bytes changed.
fn resume_cursor(stored: Option<&SourceCursor>, bytes: &[u8]) -> SourceCursor {
    stored
        .filter(|cursor| {
            usize::try_from(cursor.offset)
                .ok()
                .and_then(|offset| bytes.get(..offset))
                .is_some_and(|prefix| ContentId::of(prefix) == cursor.prefix_hash)
        })
        .cloned()
        .unwrap_or_else(|| SourceCursor {
            offset: 0,
            next_ordinal: 0,
            session: None,
            prefix_hash: ContentId::of(&[]),
        })
}

/// Wraps parser counts in a capture-limitation event for `from_ordinal..next_ordinal`.
/// Returns `None` if all capture counts are empty.
fn capture_limitation(
    session: &SessionId,
    transcript: &str,
    from_ordinal: u64,
    outcome: &ParseOutcome,
) -> Option<Observation> {
    (!outcome.counts.is_empty()).then(|| {
        Observation::CaptureLimitation(CaptureLimitation {
            session: session.clone(),
            transcript: transcript.to_owned(),
            from_ordinal,
            to_ordinal: outcome.next_ordinal,
            counts: outcome.counts.clone(),
        })
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Error, ErrorKind};

    use super::*;

    #[test]
    fn a_transcript_is_read_unless_it_was_removed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.jsonl");
        std::fs::write(&file, b"{}\n").unwrap();
        assert_eq!(read_transcript(&file).unwrap(), Some(b"{}\n".to_vec()));
        // Gone, or no longer a regular file: removed.
        assert_eq!(
            read_transcript(&dir.path().join("gone.jsonl")).unwrap(),
            None
        );
        assert_eq!(read_transcript(dir.path()).unwrap(), None);
    }

    /// What `read` returns for `path`, failing the test unless it returns
    /// within a few seconds.
    #[cfg(unix)]
    fn promptly<T: Send + 'static>(path: &Path, read: fn(&Path) -> T) -> T {
        let (done, result) = std::sync::mpsc::channel();
        let path = path.to_path_buf();
        std::thread::spawn(move || {
            let _ = done.send(read(&path));
        });
        result
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the read returns promptly")
    }

    /// Makes a FIFO at `path`.
    #[cfg(unix)]
    fn mkfifo(path: &Path) {
        let made = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .unwrap();
        assert!(made.success());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_read_only_when_it_leads_to_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.jsonl");
        std::fs::write(&file, b"{}\n").unwrap();
        let link = dir.path().join("link.jsonl");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert_eq!(
            promptly(&link, read_transcript).unwrap(),
            Some(b"{}\n".to_vec())
        );
        // A device reads as no lines at all, which would pass for a
        // transcript with nothing new.
        let device = dir.path().join("device.jsonl");
        std::os::unix::fs::symlink("/dev/null", &device).unwrap();
        assert_eq!(promptly(&device, read_transcript).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_removed_without_waiting_for_a_writer() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("t.jsonl");
        mkfifo(&fifo);
        // Opening it to read would wait for a writer that never comes.
        assert_eq!(promptly(&fifo, read_transcript).unwrap(), None);
    }

    // A FIFO that replaced the transcript after its type was checked.
    #[cfg(unix)]
    #[test]
    fn a_fifo_is_opened_without_waiting_for_a_writer_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("t.jsonl");
        mkfifo(&fifo);
        assert!(promptly(&fifo, open_regular).unwrap().is_none());
    }

    #[test]
    fn a_failed_open_is_a_removal_once_no_regular_file_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let denied = || Error::from(ErrorKind::PermissionDenied);
        // Opening a directory fails on Windows.
        assert!(open_failed(dir.path(), denied()).unwrap().is_none());
        let gone = dir.path().join("gone.jsonl");
        assert!(open_failed(&gone, denied()).unwrap().is_none());
        let file = dir.path().join("t.jsonl");
        std::fs::write(&file, b"{}\n").unwrap();
        let error = open_failed(&file, denied()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        // However opening fails, a directory is skipped.
        assert!(open_regular(dir.path()).unwrap().is_none());
    }

    // Unix reports a path beneath a regular file as "not a directory".
    #[cfg(unix)]
    #[test]
    fn a_transcript_that_cannot_be_read_for_another_reason_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, b"").unwrap();
        let error = read_transcript(&file.join("t.jsonl")).unwrap_err();
        assert_ne!(error.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn a_resolved_transcript_is_keyed_by_its_canonical_path() {
        let key = transcript_key(Path::new("link.jsonl"), Ok(PathBuf::from("/real/t.jsonl")));
        assert_eq!(Path::new(&key.unwrap()), Path::new("/real/t.jsonl"));
    }

    #[test]
    fn a_vanished_transcript_is_keyed_as_written() {
        let key = transcript_key(
            Path::new("gone.jsonl"),
            Err(Error::from(ErrorKind::NotFound)),
        );
        assert_eq!(Path::new(&key.unwrap()), Path::new("gone.jsonl"));
    }

    #[test]
    fn any_other_resolution_failure_is_an_error_naming_the_transcript() {
        let error = transcript_key(
            Path::new("denied.jsonl"),
            Err(Error::from(ErrorKind::PermissionDenied)),
        )
        .unwrap_err();
        assert!(
            matches!(
                &error,
                SyncError::Io { path, source }
                    if path == Path::new("denied.jsonl")
                        && source.kind() == ErrorKind::PermissionDenied
            ),
            "{error:?}"
        );
    }
}
