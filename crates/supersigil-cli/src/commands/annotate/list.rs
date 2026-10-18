//! `supersigil annotate list`: each session of the checkout's own record,
//! found as `session show` finds it, with its current annotation and the
//! corrections that annotation has not seen. Sessions that only an ancestor
//! checkout's record holds are not listed.

use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write as _};
use std::path::Path;

use serde::Serialize;
use supersigil_git::{Git, GitError, Repo};
use supersigil_record::interpretations::AnnotationRevision;
use supersigil_record::reviews::{self, JudgmentKind};
use supersigil_record::time::UtcTime;
use supersigil_record::{JudgmentId, RecordId, SessionId};
use supersigil_review::judging::Ledger;
use supersigil_review::printed::aligned;

use super::{CheckoutRecord, checkout_record};
use crate::commands::{AnnotateListArgs, StoreArgs};
use crate::error::CliError;
use crate::format::{OutputFormat, write_json};
use crate::review_dir::{canonical_common_dir, resolve_review_dir};

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
    let CheckoutRecord {
        checkout, store, ..
    } = checkout_record(options)?;
    let snapshot = store.snapshot()?;
    let corrections = corrections(options, &checkout)?;
    let mut sessions = Vec::new();
    for session in snapshot.sessions() {
        let current = snapshot.current_annotation(&session)?;
        let seen: HashSet<&JudgmentId> = current
            .iter()
            .flat_map(|annotation| &annotation.document.corrections_seen)
            .collect();
        let unseen_corrections = corrections
            .get(&session)
            .map_or(0, |ids| ids.iter().filter(|id| !seen.contains(id)).count());
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

/// The standing corrections in the review store of `checkout`'s
/// repository, by the session their decision names. None when the checkout
/// is outside a git worktree or its repository has no review store.
///
/// # Errors
///
/// Returns [`CliError::Git`] when git fails for another reason,
/// [`CliError::CommandFailed`] when no reviews directory can be resolved,
/// and the store errors of finding the store and reading its judgments.
fn corrections(
    options: &StoreArgs,
    checkout: &Path,
) -> Result<BTreeMap<SessionId, Vec<JudgmentId>>, CliError> {
    let repo = match Repo::open(Git::new(checkout)) {
        Ok(repo) => repo,
        Err(GitError::NotAWorktree(_) | GitError::Bare(_)) => return Ok(BTreeMap::new()),
        Err(error) => return Err(error.into()),
    };
    let reviews_dir = resolve_review_dir(options.review_dir.as_deref())?;
    let Some(store) = reviews::find(&reviews_dir, &canonical_common_dir(&repo)?)? else {
        return Ok(BTreeMap::new());
    };
    let judgments = store.snapshot()?.judgments()?;
    let ledger = Ledger::new(&judgments);
    let mut corrections: BTreeMap<SessionId, Vec<JudgmentId>> = BTreeMap::new();
    for judgment in &judgments {
        if judgment.kind == JudgmentKind::Correct
            && ledger.stands(judgment)
            && let Some(decision) = &judgment.decision
        {
            corrections
                .entry(decision.session.clone())
                .or_default()
                .push(judgment.id.clone());
        }
    }
    Ok(corrections)
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
            listed.at.map_or_else(|| "-".to_owned(), minute),
            listed.decisions.to_string(),
            listed.open_questions.to_string(),
            listed.unseen_corrections.to_string(),
        ]
    }));
    aligned(rows)
}

/// `at` as `YYYY-MM-DD HH:MM`, in UTC.
fn minute(at: UtcTime) -> String {
    let seconds = at.unix().rem_euclid(86_400);
    format!(
        "{} {:02}:{:02}",
        at.date(),
        seconds / 3_600,
        seconds / 60 % 60
    )
}
