//! `supersigil annotate prepare <session>`: the brief an annotator reads
//! about one session of the checkout's record or an ancestor checkout's.
//!
//! A destination `--out` that names a directory, or a file in a directory
//! that does not exist, is refused before anything else, so it publishes
//! nothing. The checkout's records are then reconciled as a review from
//! the checkout reconciles them, with reconcile's bounded wait; a record
//! that could not be reconciled is read as it is pinned, and the brief says
//! why. The records are reconciled before the session is resolved, so a
//! refused prefix may follow a sync: reconciling can add the session asked
//! for, and syncing is idempotent.
//!
//! The session is resolved among the records owning the checkout, which a
//! review from it reads ([`owners`]): its own record, then its ancestor
//! checkouts' records, nearest first. A prefix matching sessions in several
//! of them is refused; a session several of them hold is read from the
//! nearest. Everything else comes from one snapshot of the record holding
//! the session, while the corrections come from the review store of the
//! checkout's repository, whichever record that is:
//!
//! - **The pin.** The record, its revision, and the session's observation
//!   count and latest observation time in whole seconds, which the
//!   annotator copies into its document's `prepared` as the extent of what
//!   it saw.
//! - **Transcripts.** The main transcript first, then agent transcripts by
//!   path. Within each, turns in source order, a turn id once per sighting:
//!   a rewritten transcript sights an id again, maybe with other text. Each
//!   turn lists the edits and commands naming it, in log order, under the
//!   sighting logged last before them. One naming a turn its transcript
//!   lacks is listed under a final entry without a turn, so nothing is
//!   dropped. Each transcript also lists where its records could not be
//!   captured. No order is given across transcripts: an agent's positions
//!   count from the start of its own transcript, so sorting transcripts
//!   together would invent a chronology. The delegation steps relate a
//!   dispatch to the transcript it started.
//! - **Edits** carry their retained hunks; an edit without hunks carries its
//!   new text instead, cut at [`MAX_NEW_TEXT_CHARS`] characters, saying
//!   whether it was cut.
//! - **The current annotation**, as the record stores it, and the session's
//!   restores and discontinuities.
//! - **Corrections.** Every standing correction in the repository's review
//!   store naming the session, in any record and revision, in the order
//!   recorded: with the revisions of this record whose annotator saw it;
//!   the decision it addressed, read from the record and revision it names,
//!   or why that cannot be read here; and the current revision's decisions
//!   it anchors to, by the rule reviews use
//!   ([`corrections`](supersigil_review::decisions::corrections)). Only
//!   the current revision is held whole; each earlier one is read when
//!   needed and dropped. One that cannot be read fails nothing: it saw no
//!   correction, and a correction addressing it says why its decision is
//!   unknown.
//!
//! The brief carries retained text and ids unfiltered. It is compact JSON:
//! a long session makes a brief of many megabytes, read in parts.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use serde::Serialize;
use supersigil_record::derivations::{Discontinuity, Restore};
use supersigil_record::interpretations::{
    Annotation, AnnotationRevision, Decision, Observed, Rationale,
};
use supersigil_record::observations::{
    CaptureLimitation, Command, CommandCategory, Delegation, Edit, EditOperation, Hunk, Material,
    Outcome, Role, Turn, session_start,
};
use supersigil_record::reviews::{BasisId, CorrectionKind, DecisionRef, Judgment, Who};
use supersigil_record::store::Store;
use supersigil_record::time::UtcTime;
use supersigil_record::{
    EventId, JudgmentId, Observation, RecordId, RecordSnapshot, Revision, SessionId, StoreError,
    Timestamp, TurnId,
};
use supersigil_review::decisions::corrections::{Addressed, anchor};
use supersigil_review::decisions::{Annotated, every_decision, names};

use super::locate;
use crate::annotations::standing_corrections;
use crate::commands::session::{find_session, no_record_message};
use crate::commands::{AnnotatePrepareArgs, StoreArgs};
use crate::error::CliError;
use crate::pipeline::{claude_home, reconciled_worktrees};
use crate::reconcile::{LOCK_WAIT, reconcile};
use crate::record_dir::{owners, records_in};

/// Schema name of the brief.
const BRIEF_SCHEMA: &str = "supersigil.annotation-brief/1";
/// Most characters of an edit's new text the brief carries.
const MAX_NEW_TEXT_CHARS: usize = 2_000;

/// What an annotator reads about one session.
#[derive(Debug, Serialize)]
struct Brief<'a> {
    /// [`BRIEF_SCHEMA`].
    schema: &'static str,
    /// The record holding the session.
    record: &'a RecordId,
    /// The record's revision read.
    observation_revision: Revision,
    /// The session.
    session: &'a SessionId,
    /// The checkout, canonical.
    checkout: &'a Path,
    /// When the session's main transcript started, if known.
    started: Option<UtcTime>,
    /// The branch its main transcript started on, if known.
    branch: Option<&'a str>,
    /// How many observations of the session the record holds at this
    /// revision, and the latest time among them.
    observed: Observed,
    /// Whether the record was brought up to date before it was read.
    reconciled: bool,
    /// Why it was not, when it was not.
    #[serde(skip_serializing_if = "Option::is_none")]
    reconcile_note: Option<&'a str>,
    /// The repository's canonical git common directory, when it has a
    /// review store.
    repository: Option<&'a Path>,
    /// The session's current annotation, as stored.
    current: Option<&'a Annotation>,
    /// The standing corrections naming the session.
    corrections: Vec<Listed<'a>>,
    /// The session's transcripts.
    transcripts: Vec<TranscriptEntry<'a>>,
    /// The session's delegation steps, in log order.
    delegations: Vec<&'a Delegation>,
    /// The session's restores, as derived.
    restores: &'a [Restore],
    /// The session's discontinuities, as derived.
    discontinuities: &'a [Discontinuity],
}

/// A standing correction of the session.
#[derive(Debug, Serialize)]
struct Listed<'a> {
    /// The correction judgment.
    judgment: &'a JudgmentId,
    /// What it says about the decision.
    kind: CorrectionKind,
    /// The decision it addressed.
    decision: &'a DecisionRef,
    /// Why, for a `wrong` or `not_together` correction.
    reason: Option<&'a str>,
    /// The actual constraint, for a `constraint` correction.
    text: Option<&'a str>,
    /// Who recorded it.
    who: &'a Who,
    /// When.
    at: UtcTime,
    /// The revisions of the session in this record whose annotator saw it.
    seen_by: Vec<AnnotationRevision>,
    /// The review it was recorded on.
    review: &'a BasisId,
    /// What the decision it addressed said, when that can be read here.
    addressed: Option<&'a AddressedDecision>,
    /// Why it cannot, when it cannot.
    #[serde(skip_serializing_if = "Option::is_none")]
    addressed_unavailable: Option<&'a str>,
    /// The ids of the current revision's decisions it anchors to.
    anchors_to: Vec<String>,
}

/// What a corrected decision said in the revision a correction names: the
/// parts of it the brief gives, kept once for every correction naming it.
#[derive(Debug, Serialize)]
struct AddressedDecision {
    /// What was decided.
    title: String,
    /// Why, as the annotator wrote it.
    rationale: Vec<Rationale>,
    /// The edits and commands it cited.
    cites: Vec<EventId>,
}

impl From<Decision> for AddressedDecision {
    fn from(decision: Decision) -> Self {
        let Decision {
            title,
            rationale,
            cites,
            ..
        } = decision;
        Self {
            title,
            rationale,
            cites,
        }
    }
}

/// What the decision each reference names said, or why it cannot be read
/// here.
type AddressedByReference<'c> = BTreeMap<&'c DecisionRef, Result<AddressedDecision, String>>;

/// One transcript of the session.
#[derive(Debug, Serialize)]
struct TranscriptEntry<'a> {
    /// Its canonical path, if recorded.
    transcript: Option<&'a str>,
    /// The agent whose transcript it is; `None` for the main transcript.
    agent_id: Option<&'a str>,
    /// Whether the source marked it a subagent transcript.
    sidechain: bool,
    /// Its turns in source order, each sighting once.
    turns: Vec<TurnEntry<'a>>,
    /// Where its records could not be captured.
    capture_limitations: Vec<&'a CaptureLimitation>,
}

/// One sighting of a turn with what it issued, or the final entry of a
/// transcript's edits and commands whose turn it lacks, every turn field
/// `None`.
#[derive(Debug, Serialize)]
struct TurnEntry<'a> {
    /// The turn id.
    id: Option<&'a TurnId>,
    /// Which sighting of the id in the transcript, from 1 in log order.
    sighting: Option<usize>,
    /// The parent turn.
    parent: Option<&'a TurnId>,
    /// Who or what wrote it.
    role: Option<Role>,
    /// Its time, as the source wrote it.
    time: Option<&'a Timestamp>,
    /// Its line position in the transcript.
    ordinal: Option<u64>,
    /// Its text, as retained.
    text: Option<&'a Material<String>>,
    /// The edits naming it, in log order.
    edits: Vec<EditEntry<'a>>,
    /// The commands naming it, in log order.
    commands: Vec<CommandEntry<'a>>,
}

impl<'a> TurnEntry<'a> {
    /// The `sighting`th sighting of `turn`.
    fn of(turn: &'a Turn, sighting: usize) -> Self {
        Self {
            id: Some(&turn.id),
            sighting: Some(sighting),
            parent: turn.parent.as_ref(),
            role: Some(turn.role),
            time: Some(&turn.time),
            ordinal: Some(turn.source_ordinal),
            text: Some(&turn.text),
            edits: Vec::new(),
            commands: Vec::new(),
        }
    }

    /// The entry of edits and commands whose turn the transcript lacks.
    const fn without_turn() -> Self {
        Self {
            id: None,
            sighting: None,
            parent: None,
            role: None,
            time: None,
            ordinal: None,
            text: None,
            edits: Vec::new(),
            commands: Vec::new(),
        }
    }

    /// Lists `issued` under this entry.
    fn add(&mut self, issued: Issued<'a>) {
        match issued {
            Issued::Edit(edit) => self.edits.push(EditEntry::of(edit)),
            Issued::Command(command) => self.commands.push(CommandEntry::of(command)),
        }
    }
}

/// An edit as the brief lists it.
#[derive(Debug, Serialize)]
struct EditEntry<'a> {
    /// The edit's id.
    id: &'a EventId,
    /// The file, relative to its checkout.
    path: &'a Path,
    /// The editing operation.
    operation: EditOperation,
    /// Its hunks, when retained and not empty.
    patch: Option<&'a [Hunk]>,
    /// Without hunks, its new text when retained, cut at
    /// [`MAX_NEW_TEXT_CHARS`] characters.
    new_text: Option<&'a str>,
    /// Whether `new_text` was cut.
    truncated: bool,
}

impl<'a> EditEntry<'a> {
    fn of(edit: &'a Edit) -> Self {
        let patch = edit
            .patch
            .retained()
            .map(Vec::as_slice)
            .filter(|hunks| !hunks.is_empty());
        let (new_text, truncated) = match (patch, edit.new_text.retained()) {
            (None, Some(text)) => {
                let (kept, truncated) = capped(text);
                (Some(kept), truncated)
            }
            _ => (None, false),
        };
        Self {
            id: &edit.id,
            path: &edit.path,
            operation: edit.operation,
            patch,
            new_text,
            truncated,
        }
    }
}

/// `text` cut at [`MAX_NEW_TEXT_CHARS`] characters, and whether it was
/// cut.
fn capped(text: &str) -> (&str, bool) {
    text.char_indices()
        .nth(MAX_NEW_TEXT_CHARS)
        .map_or((text, false), |(end, _)| (&text[..end], true))
}

/// A command as the brief lists it.
#[derive(Debug, Serialize)]
struct CommandEntry<'a> {
    /// The command's id.
    id: &'a EventId,
    /// The command line.
    cmd: &'a str,
    /// The work it was classified as.
    category: CommandCategory,
    /// Its exit code, when reported.
    exit: Option<i32>,
    /// Success or failure, when inferred.
    outcome: Option<Outcome>,
    /// The tail of its standard output, as retained.
    stdout_tail: &'a Material<String>,
    /// The tail of its standard error, as retained.
    stderr_tail: &'a Material<String>,
}

impl<'a> CommandEntry<'a> {
    fn of(command: &'a Command) -> Self {
        Self {
            id: &command.id,
            cmd: &command.cmd,
            category: command.category,
            exit: command.exit,
            outcome: command.outcome,
            stdout_tail: &command.stdout_tail,
            stderr_tail: &command.stderr_tail,
        }
    }
}

/// An edit or a command, which a turn issued.
#[derive(Debug, Clone, Copy)]
enum Issued<'a> {
    /// An edit.
    Edit(&'a Edit),
    /// A command.
    Command(&'a Command),
}

impl Issued<'_> {
    /// The turn it names.
    fn turn(&self) -> &TurnId {
        match self {
            Self::Edit(edit) => &edit.turn,
            Self::Command(command) => &command.turn,
        }
    }
}

/// Runs `supersigil annotate prepare`.
///
/// # Errors
///
/// As [`super::run`].
pub(super) fn run(options: &StoreArgs, args: &AnnotatePrepareArgs) -> Result<(), CliError> {
    if let Some(out) = &args.out {
        check_destination(out)?;
    }
    // The checkout needs a record, though not a single one: the session may
    // be in an ancestor checkout's, and records at one distance are all
    // searched.
    let (records_dir, checkout) = locate(options)?;
    if owners(&records_dir, &checkout)?.is_empty() {
        return Err(CliError::CommandFailed(no_record_message(&checkout)));
    }
    // As a review from the checkout reconciles them.
    let (worktree, main_worktree) = reconciled_worktrees(&checkout)?;
    let reconciliation = reconcile(
        &records_dir,
        claude_home(options.claude_home.as_deref()).as_deref(),
        std::slice::from_ref(&worktree),
        &main_worktree,
        LOCK_WAIT,
    )?;
    // Found after reconciling, which can give an ancestor checkout a record.
    let snapshots = owners(&records_dir, &checkout)?
        .iter()
        .map(Store::snapshot)
        .collect::<Result<Vec<_>, _>>()?;
    let (snapshot, session) = find_session(snapshots, &args.session)?;
    let record = &snapshot.manifest().record_id;
    let reconcile_note = reconciliation
        .records
        .iter()
        .find(|involved| involved.record_id == *record)
        .and_then(|involved| involved.not_reconciled.as_deref());
    let observations = snapshot.observations(&session)?;
    let derivations = snapshot.derivations(&session)?;
    let current = snapshot.current_annotation(&session)?;

    let standing = standing_corrections(options, &checkout)?;
    let of_session: Vec<Correction<'_>> = standing
        .iter()
        .flat_map(|standing| &standing.judgments)
        .filter_map(|judgment| {
            Some(Correction {
                judgment,
                decision: judgment.decision.as_ref()?,
                kind: judgment.correction?,
            })
        })
        .filter(|correction| correction.decision.session == session)
        .collect();
    let others = other_records(
        &records_dir,
        record,
        of_session.iter().map(|correction| correction.decision),
    )?;
    let addressed = addressed(&of_session, &snapshot, &others);
    let restores = derivations
        .as_ref()
        .map(|set| set.restores.as_slice())
        .unwrap_or_default();

    let start = session_start(&observations).filter(|start| !start.sidechain);
    let brief = Brief {
        schema: BRIEF_SCHEMA,
        record,
        observation_revision: snapshot.revision(),
        session: &session,
        checkout: &checkout,
        started: start.and_then(|start| utc(&start.time)),
        branch: start.and_then(|start| start.branch.as_deref()),
        observed: observed(&observations),
        reconciled: reconcile_note.is_none(),
        reconcile_note,
        repository: standing
            .as_ref()
            .map(|standing| standing.repository.as_path()),
        current: current.as_ref(),
        corrections: listed(
            &snapshot,
            &session,
            restores,
            current.as_ref(),
            &of_session,
            &addressed,
        ),
        transcripts: transcripts(&observations),
        delegations: observations
            .iter()
            .filter_map(|observation| match observation {
                Observation::Delegation(delegation) => Some(delegation),
                _ => None,
            })
            .collect(),
        restores,
        discontinuities: derivations
            .as_ref()
            .map(|set| set.discontinuities.as_slice())
            .unwrap_or_default(),
    };
    match &args.out {
        None => write_brief(io::stdout().lock(), &brief)?,
        Some(out) => File::create(out)
            .and_then(|file| write_brief(file, &brief))
            .map_err(|error| cannot_write(out, error))?,
    }
    Ok(())
}

/// Refuses `out` when it names a directory, or a file in a directory that
/// does not exist, before anything is reconciled, so a destination the
/// brief cannot be written to publishes nothing. Other write errors appear
/// when the brief is written.
///
/// # Errors
///
/// Returns [`CliError::CommandFailed`] saying why `out` cannot be written.
fn check_destination(out: &Path) -> Result<(), CliError> {
    let missing_directory = || {
        out.parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty() && !parent.is_dir())
    };
    if out.is_dir() {
        Err(cannot_write(out, "it is a directory"))
    } else if missing_directory() {
        Err(cannot_write(out, "its directory does not exist"))
    } else {
        Ok(())
    }
}

/// The error of a brief that cannot be written to `out`, for `reason`.
fn cannot_write(out: &Path, reason: impl fmt::Display) -> CliError {
    CliError::CommandFailed(format!("cannot write {}: {reason}", out.display()))
}

/// A standing correction of the session, with the decision and kind it
/// names.
#[derive(Debug, Clone, Copy)]
struct Correction<'a> {
    /// The correction judgment.
    judgment: &'a Judgment,
    /// The decision it addressed.
    decision: &'a DecisionRef,
    /// What it says about the decision.
    kind: CorrectionKind,
}

/// Lists `corrections` of `session`, each with what `addressed` holds for
/// the decision it names, the revisions of the session in `snapshot`'s
/// record that saw it, and the decisions of `current`, its current
/// revision, it anchors to. `restores` are the session's.
fn listed<'a>(
    snapshot: &RecordSnapshot,
    session: &SessionId,
    restores: &[Restore],
    current: Option<&Annotation>,
    corrections: &[Correction<'a>],
    addressed: &'a AddressedByReference<'_>,
) -> Vec<Listed<'a>> {
    let mut anchors = anchors(
        session,
        &snapshot.manifest().record_id,
        restores,
        current,
        corrections,
        addressed,
    );
    let mut seen_by = seen_by(snapshot, session, current, corrections);
    corrections
        .iter()
        .map(|correction| {
            let judgment = correction.judgment;
            // `addressed` holds every reference `corrections` names.
            let decision = &addressed[&correction.decision];
            Listed {
                judgment: &judgment.id,
                kind: correction.kind,
                decision: correction.decision,
                reason: judgment.reason.as_deref(),
                text: judgment.text.as_deref(),
                who: &judgment.who,
                at: judgment.at,
                seen_by: seen_by.remove(&judgment.id).unwrap_or_default(),
                review: &judgment.basis,
                addressed: decision.as_ref().ok(),
                addressed_unavailable: decision.as_ref().err().map(String::as_str),
                anchors_to: anchors.remove(&judgment.id).unwrap_or_default(),
            }
        })
        .collect()
}

/// Writes `brief` to `writer` as compact JSON and a newline.
///
/// # Errors
///
/// Returns the I/O error of writing, or of serializing a path that is not
/// UTF-8.
fn write_brief(writer: impl Write, brief: &Brief<'_>) -> io::Result<()> {
    let mut out = BufWriter::new(writer);
    serde_json::to_writer(&mut out, brief)?;
    out.write_all(b"\n")?;
    out.flush()
}

/// A snapshot of each record in `records_dir` other than `record` that one
/// of `references` names, by record id. A record the directory lacks is
/// absent.
///
/// # Errors
///
/// Returns the errors of [`records_in`] and the store error of reading a
/// record's snapshot.
fn other_records<'r>(
    records_dir: &Path,
    record: &RecordId,
    references: impl Iterator<Item = &'r DecisionRef>,
) -> Result<HashMap<RecordId, RecordSnapshot>, CliError> {
    let named: HashSet<&RecordId> = references
        .map(|reference| &reference.record)
        .filter(|named| *named != record)
        .collect();
    let mut found = HashMap::new();
    if named.is_empty() {
        return Ok(found);
    }
    for store in records_in(records_dir)? {
        let snapshot = store.snapshot()?;
        if named.contains(&snapshot.manifest().record_id) {
            found.insert(snapshot.manifest().record_id.clone(), snapshot);
        }
    }
    Ok(found)
}

/// What the decision each of `corrections` names said, by reference, from
/// the revision the reference names of the record it names: `snapshot`'s
/// record or one of `others`. Otherwise why it cannot be read here. Each
/// revision is read once, and dropped once the decisions named in it are
/// taken; each of those is kept once, as the brief gives it, however many
/// corrections name it.
fn addressed<'c>(
    corrections: &[Correction<'c>],
    snapshot: &RecordSnapshot,
    others: &HashMap<RecordId, RecordSnapshot>,
) -> AddressedByReference<'c> {
    let mut by_revision: BTreeMap<
        (&RecordId, &SessionId, AnnotationRevision),
        HashMap<&str, &DecisionRef>,
    > = BTreeMap::new();
    for correction in corrections {
        let reference = correction.decision;
        by_revision
            .entry((&reference.record, &reference.session, reference.revision))
            .or_default()
            .insert(&reference.id, reference);
    }
    let mut addressed = BTreeMap::new();
    for ((record, session, revision), mut named) in by_revision {
        let holding = if *record == snapshot.manifest().record_id {
            Some(snapshot)
        } else {
            others.get(record)
        };
        let annotation = holding
            .ok_or_else(|| "record not found".to_owned())
            .and_then(|holding| match holding.annotation(session, revision) {
                Ok(annotation) => Ok(annotation),
                Err(StoreError::NoSuchAnnotation { .. }) => Err("revision not found".to_owned()),
                Err(error) => Err(format!("revision not readable: {error}")),
            });
        match annotation {
            Ok(annotation) => {
                for decision in annotation.document.decisions {
                    if let Some(reference) = named.remove(decision.id.as_str()) {
                        addressed.insert(reference, Ok(decision.into()));
                    }
                }
                // What is left names no decision of the revision.
                for reference in named.into_values() {
                    addressed.insert(reference, Err("decision not found in revision".to_owned()));
                }
            }
            Err(reason) => {
                for reference in named.into_values() {
                    addressed.insert(reference, Err(reason.clone()));
                }
            }
        }
    }
    addressed
}

/// The ids of the decisions of `session`'s `current` annotation in `record`
/// that each of `corrections` anchors to, sorted, by judgment id; none
/// without a current annotation. `restores` are the session's, and
/// `addressed` holds what the decision each correction names said, when it
/// could be read.
fn anchors(
    session: &SessionId,
    record: &RecordId,
    restores: &[Restore],
    current: Option<&Annotation>,
    corrections: &[Correction<'_>],
    addressed: &AddressedByReference<'_>,
) -> HashMap<JudgmentId, Vec<String>> {
    let mut anchors: HashMap<JudgmentId, Vec<String>> = HashMap::new();
    let Some(annotation) = current else {
        return anchors;
    };
    let shown = [Annotated {
        session,
        record,
        annotation,
        restores,
    }];
    let mut decisions = every_decision(&shown, &names(&shown));
    let cited: BTreeMap<DecisionRef, Addressed<'_>> = addressed
        .iter()
        .filter_map(|(&reference, decision)| {
            let cites = &decision.as_ref().ok()?.cites;
            Some((reference.clone(), Addressed { cites }))
        })
        .collect();
    let judgments: Vec<&Judgment> = corrections
        .iter()
        .map(|correction| correction.judgment)
        .collect();
    // A correction anchored to none anchors to no current decision.
    let _unanchored = anchor(&judgments, &cited, &shown, &mut decisions);
    for decision in decisions {
        for correction in decision.corrections {
            anchors
                .entry(correction.judgment)
                .or_default()
                .push(decision.id.clone());
        }
    }
    anchors
}

/// The revisions of `session` in `snapshot` whose `corrections_seen` lists
/// each of `corrections`, ascending, by judgment id. `current` is the
/// current revision, already read; each earlier one is read in turn and
/// dropped before the next, and one that cannot be read is left out.
fn seen_by<'j>(
    snapshot: &RecordSnapshot,
    session: &SessionId,
    current: Option<&Annotation>,
    corrections: &[Correction<'j>],
) -> HashMap<&'j JudgmentId, Vec<AnnotationRevision>> {
    let listed: HashSet<&JudgmentId> = corrections
        .iter()
        .map(|correction| &correction.judgment.id)
        .collect();
    let mut seen_by: HashMap<&JudgmentId, Vec<AnnotationRevision>> = HashMap::new();
    if listed.is_empty() {
        return seen_by;
    }
    let mut note = |annotation: &Annotation| {
        for judgment in &annotation.document.corrections_seen {
            if let Some(&judgment) = listed.get(judgment) {
                let seen = seen_by.entry(judgment).or_default();
                if seen.last() != Some(&annotation.revision) {
                    seen.push(annotation.revision);
                }
            }
        }
    };
    for revision in snapshot.annotation_revisions(session) {
        match current {
            Some(current) if current.revision == revision => note(current),
            _ => {
                if let Ok(annotation) = snapshot.annotation(session, revision) {
                    note(&annotation);
                }
            }
        }
    }
    seen_by
}

/// How many `observations` there are, and the latest time among them.
fn observed(observations: &[Observation]) -> Observed {
    Observed {
        count: observations.len() as u64,
        last: observations.iter().flat_map(times).filter_map(utc).max(),
    }
}

/// The times `observation` carries.
fn times(observation: &Observation) -> impl Iterator<Item = &Timestamp> {
    let (time, ended) = match observation {
        Observation::SessionStart(start) => (Some(&start.time), None),
        Observation::SessionEnd(end) => (Some(&end.time), None),
        Observation::Turn(turn) => (Some(&turn.time), None),
        Observation::Edit(edit) => (Some(&edit.time), None),
        Observation::Command(command) => (Some(&command.started), command.ended.as_ref()),
        Observation::CaptureLimitation(_) | Observation::Delegation(_) => (None, None),
    };
    time.into_iter().chain(ended)
}

/// `time` without its fraction of a second, when it is a UTC time as
/// Claude Code writes one: `YYYY-MM-DDTHH:MM:SS`, an optional fraction, and
/// `Z`.
fn utc(time: &Timestamp) -> Option<UtcTime> {
    let (seconds, rest) = time.as_str().split_at_checked(19)?;
    let fraction = rest.strip_suffix('Z')?;
    let digits = fraction.is_empty()
        || fraction.strip_prefix('.').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        });
    digits.then(|| format!("{seconds}Z").parse().ok()).flatten()
}

/// The session's transcripts, the main transcript first, then by path.
fn transcripts(observations: &[Observation]) -> Vec<TranscriptEntry<'_>> {
    let mut gathered: Vec<Gathering<'_>> = Vec::new();
    let mut by_path: HashMap<Option<&str>, usize> = HashMap::new();
    for observation in observations {
        let path = match observation {
            Observation::SessionStart(start) => start.source_ids.get("path").map(String::as_str),
            Observation::Turn(turn) => turn.transcript.as_deref(),
            Observation::Edit(edit) => edit.transcript.as_deref(),
            Observation::Command(command) => command.transcript.as_deref(),
            Observation::CaptureLimitation(limitation) => Some(limitation.transcript.as_str()),
            Observation::SessionEnd(_) | Observation::Delegation(_) => continue,
        };
        let at = *by_path.entry(path).or_insert_with(|| {
            gathered.push(Gathering::new(path));
            gathered.len() - 1
        });
        gathered[at].add(observation);
    }
    let mut transcripts: Vec<TranscriptEntry<'_>> =
        gathered.into_iter().map(Gathering::finish).collect();
    transcripts.sort_by(|a, b| (a.sidechain, a.transcript).cmp(&(b.sidechain, b.transcript)));
    transcripts
}

/// One transcript as its observations are read in log order.
#[derive(Debug)]
struct Gathering<'a> {
    /// The transcript so far, its turns in log order.
    transcript: TranscriptEntry<'a>,
    /// Each sighting of each turn id, in log order: its position in the
    /// transcript's turns.
    sightings: HashMap<&'a TurnId, Vec<usize>>,
    /// The edits and commands in log order, each with the sighting of its
    /// turn logged last before it, if any.
    issued: Vec<(Option<usize>, Issued<'a>)>,
}

impl<'a> Gathering<'a> {
    fn new(path: Option<&'a str>) -> Self {
        Self {
            transcript: TranscriptEntry {
                transcript: path,
                agent_id: None,
                sidechain: false,
                turns: Vec::new(),
                capture_limitations: Vec::new(),
            },
            sightings: HashMap::new(),
            issued: Vec::new(),
        }
    }

    /// Adds `observation`, the next in the log.
    fn add(&mut self, observation: &'a Observation) {
        let agent_id = match observation {
            Observation::SessionStart(start) => {
                self.transcript.sidechain |= start.sidechain;
                None
            }
            Observation::Turn(turn) => {
                let transcript = &mut self.transcript;
                transcript.sidechain |= turn.sidechain;
                let sightings = self.sightings.entry(&turn.id).or_default();
                sightings.push(transcript.turns.len());
                transcript.turns.push(TurnEntry::of(turn, sightings.len()));
                turn.agent_id.as_deref()
            }
            Observation::Edit(edit) => {
                self.issue(Issued::Edit(edit));
                edit.agent_id.as_deref()
            }
            Observation::Command(command) => {
                self.issue(Issued::Command(command));
                command.agent_id.as_deref()
            }
            Observation::CaptureLimitation(limitation) => {
                self.transcript.capture_limitations.push(limitation);
                None
            }
            Observation::SessionEnd(_) | Observation::Delegation(_) => None,
        };
        if self.transcript.agent_id.is_none() {
            self.transcript.agent_id = agent_id;
        }
    }

    /// Adds `issued`, the next in the log, with the sighting of its turn
    /// logged last so far, so grouping reads the log once.
    fn issue(&mut self, issued: Issued<'a>) {
        let preceding = self
            .sightings
            .get(issued.turn())
            .and_then(|sightings| sightings.last().copied());
        self.issued.push((preceding, issued));
    }

    /// The transcript, each edit and command listed under the sighting of
    /// its turn logged last before it, or else the first, or else the
    /// final entry; turns in source order.
    fn finish(mut self) -> TranscriptEntry<'a> {
        let mut unplaced = TurnEntry::without_turn();
        for (preceding, issued) in self.issued {
            let sighting = preceding.or_else(|| {
                self.sightings
                    .get(issued.turn())
                    .and_then(|sightings| sightings.first().copied())
            });
            match sighting {
                Some(at) => self.transcript.turns[at].add(issued),
                None => unplaced.add(issued),
            }
        }
        // A stable sort, so the sightings of one id keep their log order.
        self.transcript.turns.sort_by_key(|turn| turn.ordinal);
        if !(unplaced.edits.is_empty() && unplaced.commands.is_empty()) {
            self.transcript.turns.push(unplaced);
        }
        self.transcript
    }
}
