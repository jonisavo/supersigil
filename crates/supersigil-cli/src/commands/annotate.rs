//! `supersigil annotate` stores what an annotator says a session decided,
//! and lists each session's annotation.
//!
//! `import` reads an annotation document of at most 16 MiB and writes to
//! the record its `prepared.record` names, which must be one owning the
//! checkout: its own, or an ancestor checkout's
//! ([`owners`](crate::record_dir::owners)). It checks the document against
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
//! Agents may run both. Neither syncs the record or creates a store.

mod import;
mod list;

use std::path::PathBuf;

use supersigil_record::store::Store;

use crate::commands::session::no_record_message;
use crate::commands::{AnnotateArgs, AnnotateCommand, StoreArgs};
use crate::error::CliError;
use crate::record_dir;

/// Runs `supersigil annotate`.
///
/// # Errors
///
/// Returns [`CliError::Refused`] when an import is refused, writing
/// nothing; [`CliError::CommandFailed`] when the document cannot be read,
/// no record owns the checkout, or no directory can be resolved;
/// [`CliError::Record`] when the record or the review store cannot be read
/// or written; [`CliError::Git`] when git fails to open the checkout's
/// repository for a reason other than its absence; and an I/O error when
/// writing the output fails.
pub fn run(args: &AnnotateArgs) -> Result<(), CliError> {
    match &args.command {
        AnnotateCommand::Import(import) => import::run(&args.store, import),
        AnnotateCommand::List(list) => list::run(&args.store, list),
    }
}

/// The checkout `options` names and its record, found as `session show`
/// finds it.
#[derive(Debug)]
struct CheckoutRecord {
    /// The records directory searched.
    records_dir: PathBuf,
    /// The checkout, canonical.
    checkout: PathBuf,
    /// The record that owns it.
    store: Store,
}

/// Finds the checkout `options` names and its record.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] saying to sync first when no record
/// owns the checkout, and the errors of resolving the records directory
/// and the checkout and of [`record_dir::find_record`].
fn checkout_record(options: &StoreArgs) -> Result<CheckoutRecord, CliError> {
    let records_dir = record_dir::resolve_record_dir(options.record_dir.as_deref())?;
    let checkout = record_dir::canonical_checkout(options.checkout.as_deref())?;
    let store = record_dir::find_record(&records_dir, &checkout)?
        .ok_or_else(|| CliError::CommandFailed(no_record_message(&checkout)))?;
    Ok(CheckoutRecord {
        records_dir,
        checkout,
        store,
    })
}
