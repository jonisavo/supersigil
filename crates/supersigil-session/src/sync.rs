//! Incremental sync of transcripts into a record.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use supersigil_record::derive::derive;
use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, Observation, SessionStart, Source,
};
use supersigil_record::store::{Association, SourceCursor, Store, StoreError, WriteTx};
use supersigil_record::{ContentId, Revision, SessionId, Timestamp};

use crate::checkout::{Placement, placement};
use crate::claude_code::{ParseOutcome, parse_transcript};

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
    /// offset (or from the start when the transcript was rewritten).
    pub consumed: u64,
    /// Whether the file ended in an incomplete line.
    pub trailing_partial: bool,
    /// What this call could not turn into evidence, flattened into this
    /// object.
    #[serde(flatten)]
    pub counts: CaptureCounts,
    /// Why nothing was taken from this transcript, when it was skipped: it
    /// names a checkout outside the one being synced, or no record has named
    /// its checkout yet (`checkout unknown`, reported with its counts). A
    /// skipped transcript appends nothing and its cursor does not move.
    #[serde(default)]
    pub skipped: Option<String>,
    /// The transcript's checkout when it lies strictly inside the one being
    /// synced, such as a subagent's worktree. Its observations keep that
    /// checkout; the record gains no association for it.
    #[serde(default)]
    pub nested_checkout: Option<PathBuf>,
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
/// Transcripts are read in the given order. Each is parsed whole every time,
/// and only what lies past its cursor is appended, so syncing a growing file
/// in stages records what syncing it once would. A transcript is identified
/// by its canonical path (the path as given when it cannot be
/// canonicalized), so two spellings of one file share a cursor.
///
/// A transcript whose records name a working directory outside `checkout`
/// is skipped and reported, so another repository's history never enters
/// this record; so is one whose records have not named a checkout yet, since
/// a session start never borrows `checkout`. The first working directory a
/// transcript names decides. An edit issued later from outside every
/// checkout the record is associated with is counted as outside the
/// checkout instead of recorded; the record's associations decide, not
/// `checkout`, since a transcript's cursor is shared by every checkout of
/// the record and what it skips is never read again.
///
/// A transcript inside `checkout`, such as a subagent's worktree, is taken
/// in and reported as nested, and its observations keep their own checkout.
/// Which record owns a nested checkout is left to record lookup; sync adds
/// only `checkout` itself as an association. Placement is decided by
/// [`placement`](crate::checkout::placement).
///
/// A transcript whose cursor first learns its session records a
/// [`SessionStart`] from its own metadata, marked as a sidechain when a
/// subagent record named the session. A subagent transcript carries its
/// parent's session id, so a session can have several starts; which one
/// describes the session is a read model over them
/// ([`session_start`](supersigil_record::observations::session_start)), so a
/// main transcript synced later still wins. Every turn, edit, command, and
/// capture limitation is stamped with its transcript's canonical path, since
/// ordinals from different transcripts share no order.
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
    // Record lookup may already have added `checkout` in another spelling.
    let associated = tx
        .manifest()
        .associations
        .iter()
        .any(|a| placement(checkout, &a.checkout) == Placement::Same);
    if !associated {
        tx.add_association(Association {
            checkout: checkout.to_path_buf(),
        });
    }

    let mut reports = Vec::new();
    let mut appended: BTreeMap<SessionId, Vec<Observation>> = BTreeMap::new();
    let mut total = 0usize;

    for path in transcripts {
        let report = sync_transcript(&mut tx, checkout, path, &mut appended)?;
        total += report.new_observations;
        reports.push(report);
    }

    let started = pinned.manifest();
    let cursors_moved = tx.manifest().cursors != started.cursors;
    let associations_added = tx.manifest().associations != started.associations;
    if total == 0 && !associations_added && !cursors_moved {
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

/// Parses one transcript, appends the observations past its cursor to `tx`,
/// records them in `appended` by session, and advances the cursor. A
/// transcript from another checkout is reported as skipped instead.
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
    let bytes = std::fs::read(path).map_err(|source| SyncError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // One identity per file, whatever spelling named it: the cursor key and
    // the stamp on every observation.
    let key = std::fs::canonicalize(path)
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
    drop_edits_outside(&mut outcome, &tx.manifest().associations);
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

/// Why a transcript is skipped.
enum Skip {
    /// No record has named its working directory yet.
    Unknown,
    /// Its working directory lies outside the checkout being synced; the
    /// reason names both.
    Outside(String),
}

impl Skip {
    /// The report for a transcript skipped for this reason. Counts are shown
    /// for a transcript that may yet name this checkout, never for another
    /// checkout's.
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

/// Decides whether a transcript belongs in the record of `checkout`, and
/// returns the working directory it names first, with its placement, when it
/// does: `checkout` itself or a directory inside it. A transcript whose directory is unknown
/// is skipped, whatever it holds, since its session start would otherwise
/// claim a checkout no record named.
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

/// Counts as outside the checkout, instead of keeping, every edit issued
/// from a working directory outside all of `associations`, as when a
/// session changed directory after the one it named first.
fn drop_edits_outside(outcome: &mut ParseOutcome, associations: &[Association]) {
    let before = outcome.observations.len();
    outcome
        .observations
        .retain(|observation| match observation {
            Observation::Edit(edit) => associations
                .iter()
                .any(|a| placement(&edit.checkout, &a.checkout) != Placement::Outside),
            _ => true,
        });
    outcome.counts.outside_checkout += (before - outcome.observations.len()) as u64;
}

/// Records which transcript a turn, edit, or command came from.
fn stamp_transcript(observation: &mut Observation, transcript: &str) {
    let field = match observation {
        Observation::Turn(turn) => &mut turn.transcript,
        Observation::Edit(edit) => &mut edit.transcript,
        Observation::Command(command) => &mut command.transcript,
        _ => return,
    };
    *field = Some(transcript.to_owned());
}

/// The stored cursor when the consumed prefix of `bytes` still hashes to
/// the recorded one; otherwise a cursor at the start, so a transcript that
/// shrank or was rewritten in place has its observations appended again.
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

/// A capture limitation for the ordinals `[from_ordinal, next_ordinal)` of
/// `outcome`, or `None` when the parse lost nothing.
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
