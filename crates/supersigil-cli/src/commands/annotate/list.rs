//! `supersigil annotate list`: each session of the checkout's own record,
//! found as `session show` finds it, with its current annotation and the
//! corrections that annotation has not seen. Sessions that only an ancestor
//! checkout's record holds are not listed.

use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write as _};

use serde::Serialize;
use supersigil_record::interpretations::AnnotationRevision;
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, RecordId, SessionId};
use supersigil_review::printed::aligned;

use super::locate;
use crate::annotations::standing_corrections;
use crate::commands::session::no_record_message;
use crate::commands::{AnnotateListArgs, StoreArgs};
use crate::error::CliError;
use crate::format::{OutputFormat, write_json};
use crate::record_dir::find_record;

/// Schema name of `annotate list`'s JSON output.
const LIST_SCHEMA: &str = "supersigil.annotate-list/1";

/// What `annotate list` prints as JSON.
#[derive(Debug, Serialize)]
struct List {
    /// [`LIST_SCHEMA`].
    schema: &'static str,
    /// The checkout's record.
    record: RecordId,
    /// Each of its sessions, by id.
    sessions: Vec<Listed>,
}

/// A session as `annotate list` shows it.
#[derive(Debug, Serialize)]
struct Listed {
    /// The session.
    session: SessionId,
    /// Its current annotation revision, if any.
    revision: Option<AnnotationRevision>,
    /// When that revision was imported.
    at: Option<UtcTime>,
    /// How many decisions that revision holds.
    decisions: usize,
    /// How many open questions it holds.
    open_questions: usize,
    /// How many standing corrections naming the session it does not list
    /// as seen.
    unseen_corrections: usize,
}

/// Runs `supersigil annotate list`.
///
/// # Errors
///
/// As [`super::run`].
pub(super) fn run(options: &StoreArgs, args: &AnnotateListArgs) -> Result<(), CliError> {
    // The checkout's own record, which must be the only one nearest to it:
    // a command that selects the record itself has no better way to choose
    // between records at one distance.
    let (records_dir, checkout) = locate(options)?;
    let store = find_record(&records_dir, &checkout)?
        .ok_or_else(|| CliError::CommandFailed(no_record_message(&checkout)))?;
    let snapshot = store.snapshot()?;
    let standing = standing_corrections(options, &checkout)?;
    let mut corrections: BTreeMap<&SessionId, Vec<&JudgmentId>> = BTreeMap::new();
    for judgment in standing.iter().flat_map(|standing| &standing.judgments) {
        if let Some((decision, _)) = judgment.as_correction() {
            corrections
                .entry(&decision.session)
                .or_default()
                .push(&judgment.id);
        }
    }
    let mut sessions = Vec::new();
    for session in snapshot.sessions() {
        let current = snapshot.current_annotation(&session)?;
        let seen: HashSet<&JudgmentId> = current
            .iter()
            .flat_map(|annotation| &annotation.document.corrections_seen)
            .collect();
        let unseen_corrections = corrections
            .get(&session)
            .map_or(0, |ids| ids.iter().filter(|id| !seen.contains(*id)).count());
        let document = current.as_ref().map(|annotation| &annotation.document);
        sessions.push(Listed {
            revision: current.as_ref().map(|annotation| annotation.revision),
            at: current.as_ref().map(|annotation| annotation.at),
            decisions: document.map_or(0, |document| document.decisions.len()),
            open_questions: document.map_or(0, |document| document.open_questions.len()),
            unseen_corrections,
            session,
        });
    }
    let list = List {
        schema: LIST_SCHEMA,
        record: snapshot.manifest().record_id.clone(),
        sessions,
    };
    match args.format.resolve() {
        OutputFormat::Json => write_json(&list)?,
        OutputFormat::Terminal => {
            let mut out = io::stdout().lock();
            for line in lines(&list.sessions) {
                writeln!(out, "{line}")?;
            }
        }
    }
    Ok(())
}

/// A header and one row per session, or a line saying there are none.
fn lines(sessions: &[Listed]) -> Vec<String> {
    if sessions.is_empty() {
        return vec!["No sessions.".to_owned()];
    }
    let header = [
        "session",
        "revision",
        "imported (UTC)",
        "decisions",
        "open questions",
        "unseen corrections",
    ];
    let mut rows = vec![header.map(str::to_owned)];
    rows.extend(sessions.iter().map(|listed| {
        [
            listed.session.to_string(),
            listed
                .revision
                .map_or_else(|| "-".to_owned(), |revision| revision.to_string()),
            listed.at.map_or_else(|| "-".to_owned(), UtcTime::minute),
            listed.decisions.to_string(),
            listed.open_questions.to_string(),
            listed.unseen_corrections.to_string(),
        ]
    }));
    aligned(rows)
}
