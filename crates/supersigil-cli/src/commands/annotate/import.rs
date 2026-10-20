//! `supersigil annotate import <file | ->`: checks an annotation document
//! and stores it as the session's next revision in the record it names.
//!
//! A document that is not JSON of the document's shape, such as one with a
//! wrong type or an unknown field, is refused at the JSON path of its first
//! structural problem: reading stops there, so structural problems are
//! reported one at a time. A document of the right shape names its record
//! in `prepared.record`, which must be the checkout's own record or an
//! ancestor checkout's; any other is refused. Against that record, the
//! document is checked by every rule, and all its problems are listed.

use std::fs::File;
use std::io::{self, Read, Write as _};
use std::path::Path;

use serde::Serialize;
use supersigil_record::interpretations::{AnnotationDocument, AnnotationRevision, revision_phrase};
use supersigil_record::store::Store;
use supersigil_record::{RecordId, RecordSnapshot, SessionId, StoreError};
use supersigil_review::annotating::{Context, Problem, validate};
use supersigil_review::printed::Printed;
use supersigil_review::summary::counted;

use super::{locate, owning_records};
use crate::commands::{AnnotateImportArgs, StoreArgs};
use crate::error::CliError;
use crate::format::{OutputFormat, write_json};
use crate::reconcile::LOCK_WAIT;
use crate::record_dir::{begin_within, records_in};

/// Schema name of `annotate import`'s JSON output.
const IMPORT_SCHEMA: &str = "supersigil.annotate-import/1";
/// Most bytes an annotation document holds: 16 MiB.
const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
/// Most problems a refusal lists before counting the rest.
const MAX_LISTED_PROBLEMS: usize = 100;

/// What `annotate import` stored, as its JSON output prints it.
#[derive(Debug, Serialize)]
struct Imported {
    /// [`IMPORT_SCHEMA`].
    schema: &'static str,
    /// The record stored in.
    record: RecordId,
    /// The session annotated.
    session: SessionId,
    /// The revision stored.
    revision: AnnotationRevision,
    /// How many decisions the document holds.
    decisions: usize,
    /// How many open questions it holds.
    open_questions: usize,
}

/// Runs `supersigil annotate import`.
///
/// # Errors
///
/// As [`super::run`].
pub(super) fn run(options: &StoreArgs, args: &AnnotateImportArgs) -> Result<(), CliError> {
    let document = read(&args.file)?;
    // The document names which of the checkout's records it annotates, so
    // the checkout needs no single record of its own.
    let (records_dir, checkout) = locate(options)?;
    // Found by `prepared.record`, so the rules' check of it passes.
    let (store, snapshot) = named_record(&records_dir, &checkout, &document.prepared.record)?;
    let record = &snapshot.manifest().record_id;
    let current = snapshot
        .annotation_revisions(&document.session)
        .last()
        .copied();
    // An unknown session has no observations, which the rules refuse.
    let observations = snapshot.observations(&document.session)?;
    let context = Context {
        record,
        session: &document.session,
        current,
        record_revision: snapshot.revision(),
        observations: &observations,
    };
    validate(&context, &document)
        .map_err(|problems| problem_refusal(&problems, document.based_on, current))?;

    let session = document.session.clone();
    let (decisions, open_questions) = (document.decisions.len(), document.open_questions.len());
    let Some(mut tx) = begin_within(LOCK_WAIT, || store.begin())? else {
        return Err(CliError::refused(["The record is busy; try again."].into()));
    };
    // Under the lock, the store refuses the document if another import
    // stored a revision since the snapshot.
    let revision = tx.put_annotation(document).map_err(|error| match error {
        StoreError::StaleAnnotation { based_on, current } => {
            CliError::refused([stale(based_on, current)].into())
        }
        error => error.into(),
    })?;
    tx.commit()?;

    let imported = Imported {
        schema: IMPORT_SCHEMA,
        record: record.clone(),
        session,
        revision,
        decisions,
        open_questions,
    };
    match args.format.resolve() {
        OutputFormat::Json => write_json(&imported)?,
        OutputFormat::Terminal => {
            let line = format!(
                "Imported revision {revision} of session {} ({}, {}).",
                imported.session.short(),
                counted(decisions as u64, "decision"),
                counted(open_questions as u64, "open question")
            );
            io::stdout()
                .lock()
                .write_all(Printed::from([line]).text().as_bytes())?;
        }
    }
    Ok(())
}

/// The record `named` in `records_dir`, with its snapshot, which must be
/// one of the records owning `checkout`, its own or an ancestor checkout's
/// ([`owning_records`]); any number of them may lie at one distance.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] saying to sync first when no record
/// owns `checkout`. Refuses `named` when it is not among the owners, saying
/// whether `records_dir` holds it at all, and returns the errors of listing
/// the records and reading their manifests.
fn named_record(
    records_dir: &Path,
    checkout: &Path,
    named: &RecordId,
) -> Result<(Store, RecordSnapshot), CliError> {
    for (store, snapshot) in owning_records(records_dir, checkout)? {
        if snapshot.manifest().record_id == *named {
            return Ok((store, snapshot));
        }
    }
    let manifests = records_in(records_dir)?
        .iter()
        .map(Store::manifest)
        .collect::<Result<Vec<_>, _>>()?;
    let held = manifests
        .iter()
        .any(|manifest| manifest.record_id == *named);
    let which = if held {
        "is not this checkout's or an ancestor's"
    } else {
        "does not exist"
    };
    let line = format!("The annotation names record {named}, which {which}.");
    Err(CliError::refused([line].into()))
}

/// Reads the annotation document in `file`, or on standard input for `-`.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] when the file cannot be read, and
/// refuses a document of more than [`MAX_DOCUMENT_BYTES`], one that is not
/// UTF-8, and one that is not JSON of the document's shape, naming the
/// JSON path of the first structural problem.
fn read(file: &Path) -> Result<AnnotationDocument, CliError> {
    let read = if file == Path::new("-") {
        read_capped(io::stdin().lock())
    } else {
        File::open(file).and_then(read_capped)
    };
    let bytes = read.map_err(|error| {
        CliError::CommandFailed(format!("cannot read {}: {error}", file.display()))
    })?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(CliError::refused(["The annotation is over 16 MiB."].into()));
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Err(CliError::refused(["The annotation is not UTF-8."].into()));
    };
    parse(text).map_err(|error| {
        CliError::refused([format!("The annotation is not a valid document: {error}")].into())
    })
}

/// Parses `text` as one annotation document.
///
/// # Errors
///
/// Returns the first structural problem, after the JSON path where it was
/// found unless that is the whole document, as serde describes it.
fn parse(text: &str) -> Result<AnnotationDocument, String> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let document =
        serde_path_to_error::deserialize(&mut deserializer).map_err(|error| error.to_string())?;
    // Only whitespace may follow the document.
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(document)
}

/// Reads `reader` up to its end or [`MAX_DOCUMENT_BYTES`] plus one byte,
/// whichever comes first. The extra byte shows the input is over the limit,
/// and nothing past it is read, so an input of any length, or none, ends the
/// read.
///
/// # Errors
///
/// Returns the I/O error of reading.
fn read_capped(reader: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The refusal of a document that breaks the rules: the first
/// [`MAX_LISTED_PROBLEMS`] of `problems` and a count of the rest, then,
/// when the document was built on `based_on` and `current` is the
/// session's current revision instead, the step to take. Changing
/// `based_on` alone would replace a revision the annotator never saw.
fn problem_refusal(
    problems: &[Problem],
    based_on: Option<AnnotationRevision>,
    current: Option<AnnotationRevision>,
) -> CliError {
    let mut out = Printed::new();
    out.line("Annotation refused:");
    out.lines(
        problems
            .iter()
            .take(MAX_LISTED_PROBLEMS)
            .map(|problem| format!("  {problem}")),
    );
    let rest = problems.len().saturating_sub(MAX_LISTED_PROBLEMS);
    if rest > 0 {
        out.line(format!("  … and {}.", counted(rest as u64, "more problem")));
    }
    if based_on != current {
        out.line(stale(based_on, current));
    }
    CliError::refused(out)
}

/// What to say of a document built on `based_on` while `current` is the
/// session's current revision.
fn stale(based_on: Option<AnnotationRevision>, current: Option<AnnotationRevision>) -> String {
    format!(
        "Annotation based on {}, but {} is current; run annotate prepare again.",
        revision_phrase(based_on),
        revision_phrase(current)
    )
}
