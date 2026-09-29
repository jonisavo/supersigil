//! Incremental sync of transcripts into a record.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use supersigil_record::derive::derive;
use supersigil_record::observations::{
    CaptureCounts, CaptureLimitation, Observation, SessionStart, Source,
};
use supersigil_record::store::{Association, SourceCursor, Store, StoreError, WriteTx};
use supersigil_record::{ContentHasher, Revision, SessionId, Timestamp};

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
    /// What this call could not turn into evidence, flattened into this
    /// object.
    #[serde(flatten)]
    pub counts: CaptureCounts,
    /// Why nothing was taken from this transcript, when it was skipped: it
    /// names a checkout outside the one being synced, or it has produced
    /// evidence before any record named a checkout. A skipped
    /// transcript appends nothing and its cursor does not move.
    #[serde(default)]
    pub skipped: Option<String>,
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
/// Transcripts are read in the given order, each from its cursor. A
/// transcript is identified by its canonical path (the path as given when it
/// cannot be canonicalized), so two spellings of one file share a cursor.
///
/// A transcript whose records name a working directory outside `checkout`
/// is skipped and reported, so another repository's history never enters
/// this record; so is one whose evidence arrives before any record names a
/// checkout. The checkout first learned is kept on the cursor and checked on
/// every later read. One inside `checkout`, such as a subagent's worktree,
/// is taken in, its directory is added as an association of the record, and
/// its observations keep their own checkout.
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
    tx.add_association(Association {
        checkout: checkout.to_path_buf(),
    });

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

/// Reads one transcript from its cursor, appends the new observations to
/// `tx`, records them in `appended` by session, and advances the cursor. A
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
    let Resume {
        mut cursor,
        start,
        hasher,
    } = resume_cursor(tx.manifest().cursors.get(&key), &bytes);
    let from_ordinal = cursor.next_ordinal;
    // Appended records need not repeat the session id, so a resumed parse
    // starts from the session the cursor already knows.
    let mut outcome = parse_transcript_with_session(
        &bytes[start..],
        cursor.next_ordinal,
        cursor.session.as_ref(),
    );
    // The checkout as first learned decides every later read too: a resumed
    // chunk's records need not repeat it.
    let recorded = cursor.checkout.clone().or_else(|| outcome.checkout.clone());
    if let Some(reason) = admit(
        tx,
        checkout,
        recorded.as_deref(),
        !outcome.observations.is_empty(),
    ) {
        return Ok(TranscriptReport {
            path: path.to_path_buf(),
            session: cursor.session,
            new_observations: 0,
            consumed: 0,
            trailing_partial: false,
            counts: CaptureCounts::default(),
            skipped: Some(reason),
        });
    }
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
            checkout: recorded.clone().unwrap_or_else(|| checkout.to_path_buf()),
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
    cursor.checkout = recorded;
    advance(&mut cursor, &bytes, start, &outcome, hasher);
    tx.set_cursor(&key, cursor.clone());
    Ok(TranscriptReport {
        path: path.to_path_buf(),
        session: cursor.session,
        new_observations: count,
        consumed: outcome.consumed,
        trailing_partial: outcome.trailing_partial,
        counts: outcome.counts,
        skipped: None,
    })
}

/// Decides whether a transcript whose records name the working directory
/// `recorded` belongs in the record of `checkout`. A directory nested inside
/// the checkout is added as an association. Returns why the transcript is
/// skipped: its directory lies outside, or no record has named one yet while
/// the parse produced `observations`, which cannot be admitted blind.
fn admit(
    tx: &mut WriteTx<'_>,
    checkout: &Path,
    recorded: Option<&Path>,
    observations: bool,
) -> Option<String> {
    let Some(recorded) = recorded else {
        return observations.then(|| "checkout unknown".to_owned());
    };
    match placement(recorded, checkout) {
        Placement::Same => None,
        Placement::Nested => {
            tx.add_association(Association {
                checkout: recorded.to_path_buf(),
            });
            None
        }
        Placement::Outside => Some(format!(
            "checkout {} does not match {}",
            recorded.display(),
            checkout.display()
        )),
    }
}

/// Where a transcript's working directory lies relative to the checkout
/// being synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// The checkout itself.
    Same,
    /// Strictly inside it, such as a worktree under `.claude/worktrees`.
    Nested,
    /// Anywhere else.
    Outside,
}

/// Where `recorded` lies relative to `checkout`. When both exist on disk
/// their canonical paths decide; otherwise the paths as written are
/// compared by [`comparable`] components. Inside means below it through
/// normal components only, so `/work/repo/../other` is outside `/work/repo`.
/// Only the comparison normalizes: the record keeps the transcript's own
/// spelling of its checkout.
fn placement(recorded: &Path, checkout: &Path) -> Placement {
    match (
        std::fs::canonicalize(recorded),
        std::fs::canonicalize(checkout),
    ) {
        (Ok(recorded), Ok(checkout)) => placement_of(&recorded, &checkout),
        _ => placement_of(recorded, checkout),
    }
}

/// [`placement`] of two paths as written, by [`comparable`] components.
fn placement_of(recorded: &Path, checkout: &Path) -> Placement {
    let (recorded, checkout) = (comparable(recorded), comparable(checkout));
    match recorded.strip_prefix(checkout.as_slice()) {
        Some([]) => Placement::Same,
        Some(rest) if rest.iter().all(|c| c != "..") => Placement::Nested,
        _ => Placement::Outside,
    }
}

/// The components of `path` for comparing checkouts, alike on every
/// platform, so a transcript's `C:\work\repo` matches the `\\?\C:\work\repo`
/// that canonicalization yields on Windows.
///
/// A verbatim prefix is dropped (`\\?\UNC\server\share` reads as
/// `\\server\share`). In a Windows path (one starting with a drive letter or
/// `\\`, or any path on Windows) `\` separates components as `/` does. `.`
/// and empty components are dropped, and a drive letter is upper-cased. The
/// first component records the root: `/`, `//` for a UNC path, or nothing.
fn comparable(path: &Path) -> Vec<String> {
    let text = path.to_string_lossy();
    let text = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        text.into_owned()
    };
    let windows = cfg!(windows) || text.starts_with(r"\\") || has_drive(&text);
    let separators: &[char] = if windows { &['/', '\\'] } else { &['/'] };
    let root = if windows && (text.starts_with(r"\\") || text.starts_with("//")) {
        "//"
    } else if text.starts_with(separators) {
        "/"
    } else {
        ""
    };
    let mut components = vec![root.to_owned()];
    for (i, component) in text
        .split(separators)
        .filter(|c| !c.is_empty() && *c != ".")
        .enumerate()
    {
        if i == 0 && has_drive(component) {
            components.push(component.to_ascii_uppercase());
        } else {
            components.push(component.to_owned());
        }
    }
    components
}

/// Whether `text` starts with a drive letter and a colon, as in `C:`.
fn has_drive(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
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

/// Where to resume a transcript.
struct Resume {
    /// The cursor to continue from, stored or fresh.
    cursor: SourceCursor,
    /// Byte offset of the cursor, validated against the transcript.
    start: usize,
    /// A hasher that has seen exactly the transcript's first `start` bytes.
    hasher: ContentHasher,
}

/// Where to resume `bytes` from. The stored cursor is trusted only when its
/// consumed prefix still hashes to the recorded one; a transcript shorter
/// than the cursor, rewritten in place, or with an unverifiable prefix is
/// read again from the start.
fn resume_cursor(stored: Option<&SourceCursor>, bytes: &[u8]) -> Resume {
    let fresh = || Resume {
        cursor: SourceCursor {
            offset: 0,
            next_ordinal: 0,
            session: None,
            prefix_hash: None,
            checkout: None,
        },
        start: 0,
        hasher: ContentHasher::new(),
    };
    let Some(cursor) = stored else {
        return fresh();
    };
    let Some(start) = usize::try_from(cursor.offset)
        .ok()
        .filter(|offset| *offset <= bytes.len())
    else {
        return fresh();
    };
    let mut hasher = ContentHasher::new();
    if start > 0 {
        // An offset without a hash to check it against cannot be trusted.
        let Some(expected) = &cursor.prefix_hash else {
            return fresh();
        };
        hasher.update(&bytes[..start]);
        if hasher.clone().finish() != *expected {
            return fresh();
        }
    }
    Resume {
        cursor: cursor.clone(),
        start,
        hasher,
    }
}

/// Moves `cursor` past what `outcome` consumed of `bytes[start..]`: its
/// offset, next ordinal, and prefix hash change together. `hasher` has seen
/// `bytes[..start]` and is continued over the consumed bytes.
fn advance(
    cursor: &mut SourceCursor,
    bytes: &[u8],
    start: usize,
    outcome: &ParseOutcome,
    mut hasher: ContentHasher,
) {
    let consumed = usize::try_from(outcome.consumed)
        .expect("the parse consumes at most the bytes it was given");
    let end = start + consumed;
    hasher.update(&bytes[start..end]);
    cursor.offset = end as u64;
    cursor.next_ordinal = outcome.next_ordinal;
    cursor.prefix_hash = Some(hasher.finish());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn relation(recorded: &str, checkout: &str) -> Placement {
        placement_of(Path::new(recorded), Path::new(checkout))
    }

    #[test]
    fn windows_verbatim_prefixes_do_not_change_the_placement() {
        assert_eq!(
            relation(r"\\?\C:\work\repo", r"C:\work\repo"),
            Placement::Same
        );
        assert_eq!(
            relation(r"C:\work\repo", r"\\?\C:\work\repo"),
            Placement::Same
        );
        assert_eq!(
            relation(r"C:\work\repo\.claude\worktrees\x", r"\\?\C:\work\repo"),
            Placement::Nested
        );
        assert_eq!(
            relation(r"C:\work\repo\.claude\worktrees\x", r"C:\work\repo"),
            Placement::Nested
        );
        assert_eq!(
            relation(r"\\?\UNC\server\share\repo", r"\\server\share\repo"),
            Placement::Same
        );
        assert_eq!(
            relation(r"C:\work\repo2", r"C:\work\repo"),
            Placement::Outside
        );
        assert_eq!(
            relation(r"C:\work\repo\..\other", r"C:\work\repo"),
            Placement::Outside
        );
    }

    #[test]
    fn unix_paths_are_compared_by_component() {
        assert_eq!(relation("/work/repo", "/work/repo"), Placement::Same);
        assert_eq!(
            relation("/work/repo/.claude/worktrees/x", "/work/repo"),
            Placement::Nested
        );
        assert_eq!(relation("/other/repo", "/work/repo"), Placement::Outside);
        assert_eq!(relation("/work/repo2", "/work/repo"), Placement::Outside);
        assert_eq!(
            relation("/work/repo/../other", "/work/repo"),
            Placement::Outside
        );
        assert_eq!(relation("/work", "/work/repo"), Placement::Outside);
    }
}
