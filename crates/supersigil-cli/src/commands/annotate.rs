//! `supersigil annotate` prepares what an annotator reads about a session,
//! stores what the annotator says the session decided, and lists each
//! session's annotation.
//!
//! `prepare` reconciles the checkout's records with reconcile's bounded
//! wait, finds the session in the checkout's own record or the nearest
//! ancestor checkout's record holding it, and writes the annotator's brief
//! for it from one snapshot of that record: its transcripts with their
//! edits and commands, its derivations, its current annotation, and the
//! standing corrections naming it in the checkout's repository.
//!
//! `import` reads an annotation document of at most 16 MiB, never reading
//! on past that, and writes to the record its `prepared.record` names,
//! which must be one owning the checkout: its own, or an ancestor
//! checkout's ([`owners`](crate::record_dir::owners)). The document names
//! the record, so several records owning the checkout at one distance are
//! no obstacle. It checks the document against
//! one snapshot of that record by the rules of
//! [`annotating`](supersigil_review::annotating), and stores it as the
//! session's next revision. A document naming another record, or breaking
//! any rule, is refused whole, and nothing is written. The record's writer
//! lock is taken only for the write, with reconcile's bounded wait; if
//! another import stored a revision in between, the store refuses the
//! document as built on an earlier one.
//!
//! `list` shows each session of the checkout's own record, found as
//! `session show` finds it, and not those of its ancestors' records, with
//! its current revision, and counts the standing corrections that revision
//! has not seen: corrections in the review store of the checkout's
//! repository whose decision names the session, in any record and revision,
//! and that its `corrections_seen` does not list.
//!
//! Agents may run all three. `import` and `list` never sync the record, and
//! none creates a review store.

mod import;
mod list;
mod prepare;

use std::path::{Path, PathBuf};

use supersigil_git::{Git, GitError, Repo};
use supersigil_record::reviews::{self, Judgment, JudgmentKind};
use supersigil_record::store::Store;
use supersigil_review::judging::Ledger;

use crate::commands::session::no_record_message;
use crate::commands::{AnnotateArgs, AnnotateCommand, StoreArgs};
use crate::error::CliError;
use crate::record_dir;
use crate::review_dir::{canonical_common_dir, resolve_review_dir};

/// Runs `supersigil annotate`.
///
/// # Errors
///
/// Returns [`CliError::Refused`] when an import is refused, writing
/// nothing; [`CliError::CommandFailed`] when the document cannot be read,
/// the brief cannot be written to `--out`, no record owns the checkout, the
/// session prefix matches no session or several, or no directory can be
/// resolved; [`CliError::Record`] when a record or the review store cannot
/// be read or written; [`CliError::Session`] when reconciling cannot sync a
/// transcript; [`CliError::Git`] when git fails to open the checkout's
/// repository for a reason other than its absence; and an I/O error when
/// writing the output fails.
pub fn run(args: &AnnotateArgs) -> Result<(), CliError> {
    match &args.command {
        AnnotateCommand::Prepare(prepare) => prepare::run(&args.store, prepare),
        AnnotateCommand::Import(import) => import::run(&args.store, import),
        AnnotateCommand::List(list) => list::run(&args.store, list),
    }
}

/// The checkout `options` names and its record, found as `session show`
/// finds it.
#[derive(Debug)]
struct CheckoutRecord {
    /// The checkout, canonical.
    checkout: PathBuf,
    /// The record that owns it.
    store: Store,
}

/// The records directory and the canonical checkout `options` name, without
/// looking for the checkout's record.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when no directory can be resolved,
/// and an I/O error when the checkout does not exist.
fn locate(options: &StoreArgs) -> Result<(PathBuf, PathBuf), CliError> {
    let records_dir = record_dir::resolve_record_dir(options.record_dir.as_deref())?;
    let checkout = record_dir::canonical_checkout(options.checkout.as_deref())?;
    Ok((records_dir, checkout))
}

/// Finds the checkout `options` names and its record, which must be the
/// only one nearest to it: a command that selects the record itself has no
/// better way to choose between records at one distance.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] saying to sync first when no record
/// owns the checkout, and the errors of [`locate`] and of
/// [`record_dir::find_record`].
fn checkout_record(options: &StoreArgs) -> Result<CheckoutRecord, CliError> {
    let (records_dir, checkout) = locate(options)?;
    let store = record_dir::find_record(&records_dir, &checkout)?
        .ok_or_else(|| CliError::CommandFailed(no_record_message(&checkout)))?;
    Ok(CheckoutRecord { checkout, store })
}

/// The standing corrections in the review store of a checkout's
/// repository.
#[derive(Debug)]
struct Corrections {
    /// The repository's canonical git common directory, which names its
    /// review store.
    repository: PathBuf,
    /// Every correction judgment in the store that names a decision and a
    /// correction kind and is not withdrawn, in the order recorded.
    judgments: Vec<Judgment>,
}

/// The standing corrections in the review store of `checkout`'s
/// repository, in the reviews directory `options` names. `None` when the
/// checkout is outside a git worktree or its repository has no review
/// store.
///
/// # Errors
///
/// Returns [`CliError::Git`] when git fails for another reason,
/// [`CliError::CommandFailed`] when no reviews directory can be resolved,
/// and the store errors of finding the store and reading its judgments.
fn corrections(options: &StoreArgs, checkout: &Path) -> Result<Option<Corrections>, CliError> {
    let repo = match Repo::open(Git::new(checkout)) {
        Ok(repo) => repo,
        Err(GitError::NotAWorktree(_) | GitError::Bare(_)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let reviews_dir = resolve_review_dir(options.review_dir.as_deref())?;
    let repository = canonical_common_dir(&repo)?;
    let Some(store) = reviews::find(&reviews_dir, &repository)? else {
        return Ok(None);
    };
    let judgments = store.snapshot()?.judgments()?;
    let ledger = Ledger::new(&judgments);
    let standing = judgments
        .iter()
        .filter(|judgment| {
            judgment.kind == JudgmentKind::Correct
                && judgment.decision.is_some()
                && judgment.correction.is_some()
                && ledger.stands(judgment)
        })
        .cloned()
        .collect();
    Ok(Some(Corrections {
        repository,
        judgments: standing,
    }))
}
