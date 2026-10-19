//! What `review` and `why` read beside the evidence: each candidate
//! session's annotation, and the standing corrections of the decisions they
//! show.
//!
//! - **Which record.** A candidate session's annotation, open questions and
//!   basis pin come from one involved record, chosen by
//!   [`choose_record`](supersigil_review::decisions::choose_record): one
//!   annotating the session beats one that does not, then the one whose
//!   pinned log of the session is longest, then the smallest id. The
//!   annotation read is that record's current revision of the session. A
//!   printed command names a checkout whose review reads the record, and
//!   `annotate prepare` finds the session among that checkout's records:
//!   a candidate worktree that is or lies in one of the record's
//!   associations, the reviewed worktree first, so the command reads the
//!   review's own repository; else the record's most specific association
//!   lying in a candidate worktree; else its first association.
//! - **Corrections in scope.** A review shows the standing corrections
//!   naming a candidate session, in any record and revision, and those
//!   recorded on the reviews related to it, each once
//!   ([`in_scope`](crate::annotations::in_scope)), so a correction whose
//!   session's record is gone is still listed. A review that cannot be
//!   saved relates to the saved ones by its trees and scope all the same.
//!   `why` relates no review and shows the former.
//! - **What a correction addressed.** The decision a correction names is
//!   read from the record its reference names, when that record is
//!   involved. A record not involved, a revision that cannot be read, or one
//!   lacking the decision leaves nothing known of it, so the correction
//!   anchors only directly
//!   ([`anchor_corrections`](crate::annotations::anchor_corrections)).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use supersigil_git::{Git, GitError, Repo};
use supersigil_record::derivations::Restore;
use supersigil_record::interpretations::{Annotation, AnnotationRevision, Decision};
use supersigil_record::reviews::{self, BasisAnnotation, BasisId, DecisionRef, Judgment};
use supersigil_record::{EventId, RecordId, RecordSnapshot, SessionId, StoreError};
use supersigil_review::decisions::corrections::anchor;
use supersigil_review::decisions::{Annotated, RecordCandidate, choose_record};
use supersigil_review::judging::Ledger;
use supersigil_review::model::{AnnotationInfo, CorrectionInfo, DecisionInfo};
use supersigil_session::checkout::{Placement, placement, within};

use crate::commands::StoreArgs;
use crate::error::CliError;
use crate::pipeline::{Gathered, PinnedRecord};
use crate::review_dir::{canonical_common_dir, resolve_review_dir};

/// What a review read of one candidate session's annotation.
#[derive(Debug)]
struct SessionAnnotation {
    /// The session.
    session: SessionId,
    /// The record chosen for it; `None` when no involved record holds it.
    record: Option<RecordId>,
    /// How many involved records hold an annotation of it.
    annotated_records: usize,
    /// How many observations of it the chosen record holds.
    observations: u64,
    /// Its current annotation in the chosen record, with the restores that
    /// record derived from its edits.
    current: Option<(Annotation, Vec<Restore>)>,
}

/// The annotations of a review's candidate sessions, each read from the
/// record chosen for it.
#[derive(Debug, Default)]
pub struct Annotations {
    /// One per candidate session, sorted by session.
    sessions: Vec<SessionAnnotation>,
}

impl Annotations {
    /// Reads the current annotation of each of `sessions` from the record
    /// [chosen](self) for it among the records `g` pinned, at their pinned
    /// revisions.
    ///
    /// # Errors
    ///
    /// Returns the store errors of reading a chosen record's annotation or
    /// derivations document.
    pub fn read<'s>(
        g: &Gathered,
        sessions: impl IntoIterator<Item = &'s SessionId>,
    ) -> Result<Self, CliError> {
        let mut annotations = Vec::new();
        for session in sessions {
            let holding: Vec<&PinnedRecord> = g
                .records
                .iter()
                .filter(|record| record.sessions.contains_key(session))
                .collect();
            let candidates: Vec<RecordCandidate<'_>> = holding
                .iter()
                .map(|record| RecordCandidate {
                    record: &record.record_id,
                    annotated: !record.snapshot.annotation_revisions(session).is_empty(),
                    log_bytes: record.sessions[session].log_bytes,
                })
                .collect();
            let chosen = choose_record(&candidates)
                .and_then(|chosen| holding.iter().find(|record| record.record_id == *chosen));
            annotations.push(SessionAnnotation {
                session: session.clone(),
                record: chosen.map(|record| record.record_id.clone()),
                annotated_records: candidates.iter().filter(|c| c.annotated).count(),
                observations: chosen.map_or(0, |record| record.sessions[session].observations),
                current: chosen
                    .map(|record| current_of(record, session))
                    .transpose()?
                    .flatten(),
            });
        }
        annotations.sort_by(|a, b| a.session.cmp(&b.session));
        Ok(Self {
            sessions: annotations,
        })
    }

    /// The annotated sessions, each with its current annotation, as the
    /// rules of decisions take them.
    #[must_use]
    pub fn annotated(&self) -> Vec<Annotated<'_>> {
        self.sessions
            .iter()
            .filter_map(|read| {
                let (annotation, restores) = read.current.as_ref()?;
                Some(Annotated {
                    session: &read.session,
                    record: read.record.as_ref()?,
                    annotation,
                    restores,
                })
            })
            .collect()
    }

    /// What was read of each session, sorted by session, with the checkout
    /// through which the review `g` gathered found each session's record
    /// ([module docs](self)).
    #[must_use]
    pub fn infos(&self, g: &Gathered) -> Vec<AnnotationInfo> {
        // Found once per record: every session it supplies shares it.
        let mut checkouts: HashMap<&RecordId, Option<PathBuf>> = HashMap::new();
        self.sessions
            .iter()
            .map(|read| {
                let annotation = read.current.as_ref().map(|(annotation, _)| annotation);
                let checkout = read.record.as_ref().and_then(|record| {
                    checkouts
                        .entry(record)
                        .or_insert_with(|| {
                            g.records
                                .iter()
                                .find(|pinned| pinned.record_id == *record)
                                .and_then(|pinned| {
                                    found_through(
                                        &pinned.associations,
                                        &g.worktree,
                                        &g.candidate_worktrees,
                                    )
                                })
                        })
                        .clone()
                });
                AnnotationInfo {
                    session: read.session.clone(),
                    record: read.record.clone(),
                    checkout,
                    revision: annotation.map(|annotation| annotation.revision),
                    observed: annotation
                        .map(|annotation| annotation.document.prepared.observed.clone()),
                    observations_now: read.observations,
                    annotated_records: read.annotated_records,
                }
            })
            .collect()
    }

    /// Each session with a chosen record, pinned with its current revision
    /// there as a basis pins them, sorted by record then session.
    #[must_use]
    pub fn pins(&self) -> Vec<BasisAnnotation> {
        let mut pins: Vec<BasisAnnotation> = self
            .sessions
            .iter()
            .filter_map(|read| {
                Some(BasisAnnotation {
                    record: read.record.clone()?,
                    session: read.session.clone(),
                    revision: read
                        .current
                        .as_ref()
                        .map(|(annotation, _)| annotation.revision),
                })
            })
            .collect();
        pins.sort_by(|a, b| (&a.record, &a.session).cmp(&(&b.record, &b.session)));
        pins
    }
}

/// The checkout through which a review of `reviewed` and its
/// `candidate_worktrees` found the record with `associations`. Each rule
/// names a checkout the record owns, as itself or an ancestor, so a review
/// of that checkout reads the record and `annotate prepare --checkout`
/// finds the session among that checkout's records; the first names the
/// review's own repository when it can:
///
/// 1. a candidate worktree that is one of the associations or lies in one:
///    `reviewed` when it is one, else the first;
/// 2. else an association lying in a candidate worktree, the record of a
///    checkout nested in the one reviewed: the most specific, with the most
///    components, then the first in the record's order;
/// 3. else the first association.
///
/// Of a repository nested in another, the first rule names the nested
/// one's checkout even when only the outer one is associated, since the
/// outer one names another review store.
fn found_through(
    associations: &[PathBuf],
    reviewed: &Path,
    candidate_worktrees: &[PathBuf],
) -> Option<PathBuf> {
    // `min_by_key` keeps the first of equals.
    let worktree = candidate_worktrees
        .iter()
        .filter(|worktree| {
            associations
                .iter()
                .any(|association| within(worktree, association))
        })
        .min_by_key(|worktree| placement(worktree, reviewed) != Placement::Same);
    let association = || {
        associations
            .iter()
            .filter(|association| {
                candidate_worktrees
                    .iter()
                    .any(|worktree| within(association, worktree))
            })
            .min_by_key(|association| Reverse(association.components().count()))
            .or_else(|| associations.first())
    };
    worktree.or_else(association).cloned()
}

/// `record`'s current annotation of `session`, with the restores it derived
/// from the session's edits; `None` when it holds no annotation of it.
///
/// # Errors
///
/// Returns the store errors of reading the annotation or derivations
/// document.
fn current_of(
    record: &PinnedRecord,
    session: &SessionId,
) -> Result<Option<(Annotation, Vec<Restore>)>, CliError> {
    let Some(annotation) = record.snapshot.current_annotation(session)? else {
        return Ok(None);
    };
    let restores = record
        .snapshot
        .derivations(session)?
        .map(|set| set.restores)
        .unwrap_or_default();
    Ok(Some((annotation, restores)))
}

/// Whether `judgment` is a standing correction: a correction
/// ([`Judgment::as_correction`]) that `ledger`, every judgment in its
/// store, shows no undo withdrew.
#[must_use]
pub fn is_standing_correction(ledger: &Ledger<'_>, judgment: &Judgment) -> bool {
    judgment.as_correction().is_some() && ledger.stands(judgment)
}

/// The standing corrections among `judgments`, every judgment in a store in
/// the order recorded and indexed by `ledger`, that a review shows: those
/// whose decision names one of `sessions`, and those recorded on a review
/// `related` accepts. Each is listed once, in the order recorded.
#[must_use]
pub fn in_scope<'j>(
    judgments: &'j [Judgment],
    ledger: &Ledger<'_>,
    sessions: &BTreeSet<&SessionId>,
    related: impl Fn(&BasisId) -> bool,
) -> Vec<&'j Judgment> {
    let mut listed = HashSet::new();
    judgments
        .iter()
        .filter(|judgment| is_standing_correction(ledger, judgment))
        .filter(|judgment| {
            judgment
                .as_correction()
                .is_some_and(|(decision, _)| sessions.contains(&decision.session))
                || related(&judgment.basis)
        })
        .filter(|judgment| listed.insert(&judgment.id))
        .collect()
}

/// Anchors `corrections` to `decisions`, the decisions in view of the
/// sessions of `annotations`, by the rules of
/// [`corrections`](supersigil_review::decisions::corrections), and returns
/// those anchored to none, oldest first. What a correction's decision cited
/// is read from the pinned `records` ([module docs](self)).
#[must_use]
pub fn anchor_corrections(
    records: &[PinnedRecord],
    annotations: &Annotations,
    corrections: &[&Judgment],
    decisions: &mut [DecisionInfo],
) -> Vec<CorrectionInfo> {
    let annotated = annotations.annotated();
    let cited = cited(records, &annotated, corrections);
    let addressed = |reference: &DecisionRef| cited.get(reference).map(Vec::as_slice);
    anchor(corrections, addressed, &annotated, decisions)
}

/// What the decision each of `corrections` names cited, by reference, read
/// from the pinned `records` ([`read_addressed`]). A reference to the
/// revision `annotated` shows of its session is left out, since anchoring
/// never reads it, as is one whose decision cannot be read.
fn cited<'j>(
    records: &[PinnedRecord],
    annotated: &[Annotated<'_>],
    corrections: &[&'j Judgment],
) -> BTreeMap<&'j DecisionRef, Vec<EventId>> {
    let shown: HashMap<&SessionId, (&RecordId, AnnotationRevision)> = annotated
        .iter()
        .map(|entry| (entry.session, (entry.record, entry.annotation.revision)))
        .collect();
    let references = corrections
        .iter()
        .filter_map(|judgment| judgment.as_correction())
        .map(|(reference, _)| reference)
        .filter(|reference| {
            shown.get(&reference.session) != Some(&(&reference.record, reference.revision))
        });
    let snapshot_of = |record: &RecordId| {
        records
            .iter()
            .find(|pinned| pinned.record_id == *record)
            .map(|pinned| &pinned.snapshot)
    };
    read_addressed(references, snapshot_of)
        .into_iter()
        .filter_map(|(reference, decision)| Some((reference, decision.ok()?.cites)))
        .collect()
}

/// Why the decision a correction's reference names cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unread {
    /// The record the reference names is not among those at hand.
    RecordNotFound,
    /// The record holds no such annotation revision of the session.
    RevisionNotFound,
    /// The revision cannot be read, for the store's reason.
    Unreadable(String),
    /// The revision holds no decision of that id.
    DecisionNotFound,
}

impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecordNotFound => f.write_str("record not found"),
            Self::RevisionNotFound => f.write_str("revision not found"),
            Self::Unreadable(error) => write!(f, "revision not readable: {error}"),
            Self::DecisionNotFound => f.write_str("decision not found in revision"),
        }
    }
}

/// A revision of a session in a record.
type RevisionKey<'a> = (&'a RecordId, &'a SessionId, AnnotationRevision);

/// Reads the decision each of `references` names from the revision it names
/// of the record it names, as `snapshot_of` gives the record, or says why
/// it cannot be read. The references are grouped by revision, and each
/// revision is read once, gives the decisions named in it, and is dropped
/// before the next is read; a reference given twice is read once.
#[must_use]
pub fn read_addressed<'j, 'r>(
    references: impl IntoIterator<Item = &'j DecisionRef>,
    snapshot_of: impl Fn(&RecordId) -> Option<&'r RecordSnapshot>,
) -> BTreeMap<&'j DecisionRef, Result<Decision, Unread>> {
    let mut by_revision: BTreeMap<RevisionKey<'j>, HashMap<&'j str, &'j DecisionRef>> =
        BTreeMap::new();
    for reference in references {
        by_revision
            .entry((&reference.record, &reference.session, reference.revision))
            .or_default()
            .insert(&reference.id, reference);
    }
    let mut addressed = BTreeMap::new();
    for ((record, session, revision), mut named) in by_revision {
        let annotation = snapshot_of(record)
            .ok_or(Unread::RecordNotFound)
            .and_then(|snapshot| match snapshot.annotation(session, revision) {
                Ok(annotation) => Ok(annotation),
                Err(StoreError::NoSuchAnnotation { .. }) => Err(Unread::RevisionNotFound),
                Err(error) => Err(Unread::Unreadable(error.to_string())),
            });
        match annotation {
            Ok(annotation) => {
                for decision in annotation.document.decisions {
                    if let Some(reference) = named.remove(decision.id.as_str()) {
                        addressed.insert(reference, Ok(decision));
                    }
                }
                // What is left names no decision of the revision.
                for reference in named.into_values() {
                    addressed.insert(reference, Err(Unread::DecisionNotFound));
                }
            }
            Err(unread) => {
                for reference in named.into_values() {
                    addressed.insert(reference, Err(unread.clone()));
                }
            }
        }
    }
    addressed
}

/// The standing corrections in the review store of a checkout's
/// repository.
#[derive(Debug)]
pub struct Corrections {
    /// The repository's canonical git common directory, which names its
    /// review store.
    pub repository: PathBuf,
    /// Every standing correction in the store ([`is_standing_correction`]),
    /// in the order recorded.
    pub judgments: Vec<Judgment>,
}

/// The standing corrections in the review store of `checkout`'s
/// repository ([`standing_corrections_in`]). `None` when the checkout is
/// outside a git worktree.
///
/// # Errors
///
/// Returns [`CliError::Git`] when git fails for another reason, and the
/// errors of [`standing_corrections_in`].
pub fn standing_corrections(
    options: &StoreArgs,
    checkout: &Path,
) -> Result<Option<Corrections>, CliError> {
    match Repo::open(Git::new(checkout)) {
        Ok(repo) => standing_corrections_in(options, &repo),
        Err(GitError::NotAWorktree(_) | GitError::Bare(_)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// The standing corrections in the review store of `repo`'s repository, in
/// the reviews directory `options` names. `None` when the repository has
/// no review store; none is created.
///
/// # Errors
///
/// Returns [`CliError::Git`] when the repository's common directory cannot
/// be resolved, [`CliError::CommandFailed`] when no reviews directory can
/// be resolved, and the store errors of finding the store and reading its
/// judgments.
pub fn standing_corrections_in(
    options: &StoreArgs,
    repo: &Repo,
) -> Result<Option<Corrections>, CliError> {
    let reviews_dir = resolve_review_dir(options.review_dir.as_deref())?;
    let repository = canonical_common_dir(repo)?;
    let Some(store) = reviews::find(&reviews_dir, &repository)? else {
        return Ok(None);
    };
    let mut judgments = store.snapshot()?.judgments()?;
    let standing: Vec<bool> = {
        let ledger = Ledger::new(&judgments);
        judgments
            .iter()
            .map(|judgment| is_standing_correction(&ledger, judgment))
            .collect()
    };
    // `retain` visits the judgments in order, as the mask was built.
    let mut standing = standing.into_iter();
    judgments.retain(|_| standing.next().unwrap_or(false));
    Ok(Some(Corrections {
        repository,
        judgments,
    }))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::found_through;

    fn paths(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    fn found(associations: &[&str], reviewed: &str, candidates: &[&str]) -> Option<PathBuf> {
        found_through(
            &paths(associations),
            Path::new(reviewed),
            &paths(candidates),
        )
    }

    #[test]
    fn a_candidate_worktree_in_an_association_is_named_the_reviewed_one_first() {
        let candidates = ["/work/p", "/work/p/wt"];
        assert_eq!(
            found(&["/work/p"], "/work/p/wt", &candidates),
            Some(PathBuf::from("/work/p/wt"))
        );
        assert_eq!(
            found(&["/work/p"], "/work/p", &candidates),
            Some(PathBuf::from("/work/p"))
        );
        // A reviewed worktree that is no candidate leaves the first.
        assert_eq!(
            found(&["/work/p"], "/work/q", &candidates),
            Some(PathBuf::from("/work/p"))
        );
        // Lying in an association is enough: the outer repository holds
        // the nested one's checkout.
        assert_eq!(
            found(&["/elsewhere", "/work"], "/work/p/n", &["/work/p/n"]),
            Some(PathBuf::from("/work/p/n"))
        );
    }

    #[test]
    fn else_the_most_specific_association_in_a_candidate_worktree_is_named() {
        let associations = ["/elsewhere", "/work/p/a", "/work/p/a/b", "/work/p/c/d"];
        assert_eq!(
            found(&associations, "/work/p", &["/work/p"]),
            Some(PathBuf::from("/work/p/a/b"))
        );
    }

    #[test]
    fn else_the_first_association_is_named() {
        assert_eq!(
            found(&["/elsewhere", "/other"], "/work/p", &["/work/p"]),
            Some(PathBuf::from("/elsewhere"))
        );
        assert_eq!(found(&[], "/work/p", &["/work/p"]), None);
    }
}
