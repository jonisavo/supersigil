//! Appends new transcript events to a record and saves where reading stopped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use supersigil_record::derive::derive;
use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, Observation, SessionStart, Source,
};
use supersigil_record::store::{Association, SourceCursor, Store, StoreError, WriteTx};
use supersigil_record::{ContentId, Revision, SessionId, Timestamp};

use crate::checkout::{Placement, canonical, placement};
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
    /// Skipped transcripts name an outside checkout or have no known checkout.
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
/// falling back to the supplied path if resolution fails. Line positions can
/// be compared only within a single transcript.
///
/// # Errors
///
/// Returns [`SyncError::Io`] if a transcript cannot be read or
/// [`SyncError::Store`] if locking, reading, writing, or committing the record fails.
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

    let next_revision = tx.manifest().revision.next();
    let mut sessions = Vec::with_capacity(appended.len());
    for (session, new) in appended {
        let mut observations = pinned.observations(&session)?;
        observations.extend(new);
        let set = derive(&session, &observations, next_revision);
        tx.put_derivations(&set)?;
        sessions.push(session);
    }
    let revision = tx.commit()?;
    Ok(SyncReport {
        revision,
        sessions,
        new_observations: total,
        transcripts: reports,
    })
}

/// Appends one transcript's new observations and updates its cursor in `tx`.
///
/// Also collects the observations in `appended` for recomputing derivations.
/// Adds a capture-limitation event for parser counts. A transcript with an
/// outside or unknown checkout is reported as skipped without updating the cursor.
///
/// # Errors
///
/// Returns [`SyncError::Io`] if reading the transcript fails or
/// [`SyncError::Store`] if appending observations fails.
fn sync_transcript(
    tx: &mut WriteTx<'_>,
    checkout: &Path,
    path: &Path,
    appended: &mut BTreeMap<SessionId, Vec<Observation>>,
) -> Result<TranscriptReport, SyncError> {
    let bytes = std::fs::read(path).map_err(|source| SyncError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // One identity per file, whatever spelling named it: the cursor key and
    // the stamp on every observation.
    let key = canonical(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string();
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

/// Reason a transcript was not accepted for sync.
enum Skip {
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
    let inside = |dir: &Path| {
        associations
            .iter()
            .any(|a| placement(dir, &a.checkout) != Placement::Outside)
    };
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

/// Sets the transcript path on a turn, edit, or command observation.
fn stamp_transcript(observation: &mut Observation, transcript: &str) {
    let field = match observation {
        Observation::Turn(turn) => &mut turn.transcript,
        Observation::Edit(edit) => &mut edit.transcript,
        Observation::Command(command) => &mut command.transcript,
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
