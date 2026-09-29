//! Incremental sync of transcripts into a record.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use supersigil_record::derive::derive;
use supersigil_record::observations::{CaptureLimitation, Observation, SessionStart, Source};
use supersigil_record::store::{Association, SourceCursor, Store, StoreError, WriteTx};
use supersigil_record::{ContentId, Revision, SessionId};

use crate::claude_code::{ParseOutcome, parse_transcript_with_session};

/// Errors from sync.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// The store failed.
    #[error("{0}")]
    Store(#[from] StoreError),
    /// A transcript could not be read.
    #[error("cannot read transcript {path}: {source}")]
    Io {
        /// Transcript path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// What one transcript contributed to a sync.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TranscriptReport {
    /// Transcript path as given.
    pub path: PathBuf,
    /// Session the transcript belongs to, once known.
    pub session: Option<SessionId>,
    /// Observations appended from this transcript.
    pub new_observations: usize,
    /// Bytes consumed by this call, counted from the cursor's previous
    /// offset (or from the start when the transcript was read again).
    pub consumed: u64,
    /// Whether the file ended in an incomplete line.
    pub trailing_partial: bool,
    /// Unknown record types skipped, by type.
    pub unknown_records: BTreeMap<String, u64>,
    /// Lines that were not valid JSON.
    pub malformed_lines: u64,
    /// Tool uses the agent moved past without a result.
    pub abandoned_tool_uses: u64,
    /// Editing tool uses the harness reported as failed; no edit was recorded.
    pub failed_tool_uses: u64,
    /// Edits outside the checkout that were dropped.
    pub outside_checkout: u64,
}

/// Result of one sync call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SyncReport {
    /// Record revision after the call.
    pub revision: Revision,
    /// Sessions that received observations.
    pub sessions: Vec<SessionId>,
    /// Observations appended in total.
    pub new_observations: usize,
    /// Per-transcript detail.
    pub transcripts: Vec<TranscriptReport>,
}

/// Appends new observations from `transcripts` to `store`, recomputes
/// derivations for every touched session, and commits one revision.
///
/// # Errors
///
/// Returns [`SyncError::Io`] if a transcript cannot be read, or the store's
/// error if the record is locked or cannot be written.
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
    let association = Association {
        checkout: checkout.to_path_buf(),
    };
    let association_is_new = !tx
        .manifest()
        .associations
        .iter()
        .any(|a| a.checkout == association.checkout);
    tx.add_association(association);

    let initial_cursors = tx.manifest().cursors.clone();
    let mut reports = Vec::new();
    let mut appended: BTreeMap<SessionId, Vec<Observation>> = BTreeMap::new();
    let mut total = 0usize;

    for path in transcripts {
        let report = sync_transcript(&mut tx, checkout, path, &mut appended)?;
        total += report.new_observations;
        reports.push(report);
    }

    let cursors_moved = tx.manifest().cursors != initial_cursors;
    if total == 0 && !association_is_new && !cursors_moved {
        return Ok(SyncReport {
            revision: pinned.revision(),
            sessions: Vec::new(),
            new_observations: 0,
            transcripts: reports,
        });
    }

    let next_revision = tx.manifest().revision.next();
    let touched: BTreeSet<SessionId> = appended.keys().cloned().collect();
    for session in &touched {
        let mut observations = pinned.observations(session)?;
        observations.extend(appended.remove(session).unwrap_or_default());
        let set = derive(session, &observations, next_revision);
        tx.put_derivations(&set)?;
    }
    let revision = tx.commit()?;
    Ok(SyncReport {
        revision,
        sessions: touched.into_iter().collect(),
        new_observations: total,
        transcripts: reports,
    })
}

/// Reads one transcript from its cursor, appends the new observations to
/// `tx`, records them in `appended` by session, and advances the cursor.
///
/// Anything the parse could not turn into evidence is appended as one
/// [`CaptureLimitation`] after the parsed observations, in the same
/// transaction as the cursor advance, so the limitation outlives this call.
fn sync_transcript(
    tx: &mut WriteTx<'_>,
    checkout: &Path,
    path: &Path,
    appended: &mut BTreeMap<SessionId, Vec<Observation>>,
) -> Result<TranscriptReport, SyncError> {
    let key = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|source| SyncError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut cursor = resume_cursor(tx.manifest().cursors.get(&key), &bytes);
    let from_ordinal = cursor.next_ordinal;
    // A transcript larger than the address space is not a case this tool handles.
    let start = usize::try_from(cursor.offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    // Appended records need not repeat the session id, so a resumed parse
    // starts from the session the cursor already knows.
    let outcome = parse_transcript_with_session(
        &bytes[start..],
        cursor.next_ordinal,
        cursor.session.as_ref(),
    );
    let transcript = path
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
            source_ids: BTreeMap::from([("transcript".to_owned(), transcript.clone())]),
            checkout: outcome
                .checkout
                .clone()
                .unwrap_or_else(|| checkout.to_path_buf()),
            branch: outcome.branch.clone(),
            time: outcome
                .first_time
                .clone()
                .unwrap_or_else(|| supersigil_record::Timestamp::new("")),
        }));
        cursor.session = Some(session.clone());
    }
    let limitation = cursor
        .session
        .as_ref()
        .and_then(|session| capture_limitation(session, transcript, from_ordinal, &outcome));
    new.extend(outcome.observations);
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
    cursor.offset += outcome.consumed;
    cursor.next_ordinal = outcome.next_ordinal;
    let end = usize::try_from(cursor.offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    cursor.prefix_hash = Some(ContentId::of(&bytes[..end]));
    tx.set_cursor(&key, cursor.clone());
    Ok(TranscriptReport {
        path: path.to_path_buf(),
        session: cursor.session,
        new_observations: count,
        consumed: outcome.consumed,
        trailing_partial: outcome.trailing_partial,
        unknown_records: outcome.unknown_records,
        malformed_lines: outcome.malformed_lines,
        abandoned_tool_uses: outcome.abandoned_tool_uses,
        failed_tool_uses: outcome.failed_tool_uses,
        outside_checkout: outcome.outside_checkout,
    })
}

/// The cursor to resume `bytes` from. A transcript shorter than the cursor,
/// or whose consumed prefix no longer hashes to the recorded one, was
/// rewritten and is read again from the start.
fn resume_cursor(stored: Option<&SourceCursor>, bytes: &[u8]) -> SourceCursor {
    let fresh = SourceCursor {
        offset: 0,
        next_ordinal: 0,
        session: None,
        prefix_hash: None,
    };
    let Some(cursor) = stored else {
        return fresh;
    };
    let Some(prefix) = usize::try_from(cursor.offset)
        .ok()
        .and_then(|offset| bytes.get(..offset))
    else {
        return fresh;
    };
    match &cursor.prefix_hash {
        Some(hash) if *hash != ContentId::of(prefix) => fresh,
        _ => cursor.clone(),
    }
}

/// A capture limitation for the ordinals `[from_ordinal, next_ordinal)` of
/// `outcome`, or `None` when the parse lost nothing.
fn capture_limitation(
    session: &SessionId,
    transcript: String,
    from_ordinal: u64,
    outcome: &ParseOutcome,
) -> Option<Observation> {
    let lost = !outcome.unknown_records.is_empty()
        || outcome.malformed_lines > 0
        || outcome.abandoned_tool_uses > 0
        || outcome.failed_tool_uses > 0
        || outcome.outside_checkout > 0;
    lost.then(|| {
        Observation::CaptureLimitation(CaptureLimitation {
            session: session.clone(),
            transcript,
            from_ordinal,
            to_ordinal: outcome.next_ordinal,
            unknown_records: outcome.unknown_records.clone(),
            malformed_lines: outcome.malformed_lines,
            abandoned_tool_uses: outcome.abandoned_tool_uses,
            failed_tool_uses: outcome.failed_tool_uses,
            outside_checkout: outcome.outside_checkout,
        })
    })
}
