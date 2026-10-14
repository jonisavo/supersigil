//! Gate items a person judges before completing a review.
//!
//! [`gate_items`] builds one item per origin and subject from the review and
//! [`GateInputs`]. The CLI supplies gaps between recorded edits of reviewed
//! files, on-disk states of uncaptured paths, and every recorded sighting of
//! reviewed edits, including payloads. Items use raw path bytes that the model
//! omits from serialization, so paths that print alike remain distinct.
//!
//! Each fingerprint part hashes a kind of judgment material as SHA-256 of its
//! JSON, or is unknown when the material cannot be established. A judgment
//! carries to a later item only when every part matches. Unknown parts never
//! match.
//!
//! A basis keeps [`explanations`] to detect changes in each file's evidence
//! and [`capture`] to retain the review's capture limitations.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use supersigil_record::RecordId;
use supersigil_record::observations::Edit;
use supersigil_record::reviews::{
    CaptureEntry, Explanation, Fingerprint, FingerprintPart, GateDetail, GateItem, PathBytes,
    Subject,
};

use crate::model::{EditInfo, FileCounts, FileKindInfo, FileReview, NotCapturedInfo, Review, Span};
use crate::summary::total_counts;

/// The on-disk state of an uncaptured or excluded path, read and hashed by
/// the CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OnDiskState {
    /// Nothing is at the path.
    Missing,
    /// A regular file.
    File {
        /// Whether its executable bit is set.
        executable: bool,
        /// Hex SHA-256 of its bytes.
        content: String,
    },
    /// A symbolic link.
    Symlink {
        /// Hex SHA-256 of the link's target.
        target: String,
    },
    /// A directory, such as an uncaptured submodule checkout, or a path
    /// that could not be read.
    Unknown,
}

impl OnDiskState {
    /// A short description for the item's row: `missing`, `file`,
    /// `executable file`, `symlink`, or `unknown`.
    #[must_use]
    pub const fn word(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::File {
                executable: false, ..
            } => "file",
            Self::File {
                executable: true, ..
            } => "executable file",
            Self::Symlink { .. } => "symlink",
            Self::Unknown => "unknown",
        }
    }

    /// The fingerprint part of the state, unknown for [`Self::Unknown`].
    fn part(&self) -> FingerprintPart {
        match self {
            Self::Unknown => FingerprintPart::Unknown,
            known => part(known),
        }
    }
}

/// Two recorded edits that map into a reviewed file but have different
/// earlier after-states and later before-states. Includes every sighting of
/// each edit, in any order, since later sightings may have different payloads
/// under the same ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap<'g> {
    /// The reviewed file.
    pub path: PathBytes,
    /// Every sighting of the earlier edit, whose after-state is compared.
    pub prev: Vec<&'g Edit>,
    /// Every sighting of the next edit, whose before-state differs from
    /// that after-state.
    pub next: Vec<&'g Edit>,
}

/// What the CLI found for an untracked file a recorded edit names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedEvidence<'g> {
    /// What is on disk at the path.
    pub on_disk: OnDiskState,
    /// Every sighting of every recorded edit naming the path, in any order.
    pub edits: Vec<&'g Edit>,
}

/// CLI inputs for building a review's gate. Borrows sightings from the
/// records the CLI read and uses the review entries' raw path bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateInputs<'g> {
    /// The gaps among edits of candidate sessions that map into a reviewed
    /// file, in any order.
    pub discontinuities: Vec<Gap<'g>>,
    /// Per untracked file with recorded edits, what the CLI found.
    pub untracked: BTreeMap<PathBytes, UntrackedEvidence<'g>>,
    /// Per path not captured, what is on disk.
    pub not_captured: BTreeMap<PathBytes, OnDiskState>,
    /// Every sighting of each edit in the review's `edits` map, by ID, in any
    /// order. Supplies the payloads and states those entries omit.
    pub edits: BTreeMap<String, Vec<&'g Edit>>,
}

/// Builds gate items sorted by origin, then subject.
///
/// Creates one item per file with unexplained lines, using the unattributed
/// summary's counts; per file not diffed; per untracked file with recorded
/// edits; per uncaptured path; per commit without an origin; per reviewed
/// file with edit gaps; and per unreconciled record or checkout.
///
/// Missing inputs for untracked or uncaptured paths or edits behind counted
/// lines produce unknown parts. Directories also have unknown on-disk states.
/// Unreconciled evidence is always unknown because the CLI did not read it.
#[must_use]
pub fn gate_items(review: &Review, inputs: &GateInputs<'_>) -> Vec<GateItem> {
    let files = &review.files;
    let mut items: Vec<GateItem> = files
        .iter()
        .filter_map(|file| unexplained_lines(file, &review.edits, &inputs.edits))
        .collect();
    items.extend(
        files
            .iter()
            .filter(|file| file.kind != FileKindInfo::Text)
            .map(not_diffed),
    );
    let scope = &review.scope;
    items.extend(scope.recorded_untracked().map(|untracked| {
        let path = &untracked.path_bytes;
        untracked_recorded(path, inputs.untracked.get(path))
    }));
    items.extend(
        scope
            .not_captured
            .iter()
            .map(|entry| not_captured(entry, inputs.not_captured.get(&entry.path_bytes))),
    );
    let origin = part(&review.origins.unavailable);
    items.extend(review.origins.without_origin.iter().map(|commit| {
        GateItem::new(
            Subject::Commit(commit.clone()),
            GateDetail::CommitWithoutOrigin,
            fingerprint([("origin", origin.clone())]),
        )
    }));
    items.extend(discontinuities(&inputs.discontinuities));
    items.extend(unreconciled(review));
    items.sort_by(|a, b| (a.origin(), &a.subject).cmp(&(b.origin(), &b.subject)));
    items
}

/// Builds an item for `file`'s unexplained lines, if any. Its evidence is
/// unknown when `sightings` lacks an edit those lines name.
fn unexplained_lines(
    file: &FileReview,
    edits: &BTreeMap<String, EditInfo>,
    sightings: &BTreeMap<String, Vec<&Edit>>,
) -> Option<GateItem> {
    let (counts, spans) = FileCounts::of(file);
    if spans.is_empty() {
        return None;
    }
    let named = named_edits(&spans, edits);
    let evidence = if named.keys().all(|id| sightings.contains_key(*id)) {
        part(&recorded_edits(named, sightings))
    } else {
        FingerprintPart::Unknown
    };
    let detail = GateDetail::UnexplainedLines {
        unattributed: counts.unattributed as u64,
        content_match_incomplete: counts.content_match_incomplete as u64,
        unresolved: counts.unresolved as u64,
        ambiguous: counts.ambiguous as u64,
        line_ending: counts.line_ending as u64,
        content_match_only: counts.content_match_only as u64,
    };
    let fingerprint = fingerprint([
        ("content", content(file)),
        ("outcomes", part(&spans)),
        ("evidence", evidence),
    ]);
    Some(GateItem::new(
        Subject::Path(file.path_bytes.clone()),
        detail,
        fingerprint,
    ))
}

/// Finds entries of `edits` named by `spans`, keyed by ID. Outcomes and
/// anchor provenance name edits in JSON, so any JSON string matching an edit
/// ID counts. Converts only span sides and outcomes; provenance is already
/// JSON.
fn named_edits<'e>(
    spans: &[&Span],
    edits: &'e BTreeMap<String, EditInfo>,
) -> BTreeMap<&'e str, &'e EditInfo> {
    let converted: Vec<Value> = spans
        .iter()
        .flat_map(|span| {
            [
                serde_json::to_value(span.side),
                serde_json::to_value(&span.outcome),
            ]
        })
        .filter_map(Result::ok)
        .collect();
    let provenance = spans.iter().flat_map(|span| &span.provenance);
    let mut pending: Vec<&Value> = converted.iter().chain(provenance).collect();
    let mut named = BTreeMap::new();
    while let Some(value) = pending.pop() {
        match value {
            Value::String(text) => {
                if let Some((id, info)) = edits.get_key_value(text) {
                    named.insert(id.as_str(), info);
                }
            }
            Value::Array(values) => pending.extend(values),
            Value::Object(fields) => pending.extend(fields.values()),
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    named
}

/// A recorded edit as a review shows it and as the records hold it.
#[derive(Serialize)]
struct RecordedEdit<'e> {
    /// Its entry in the review's `edits` map.
    entry: &'e EditInfo,
    /// Every recorded sighting of it, payload and states included, in the
    /// order of their JSON and without repeats.
    sightings: Vec<&'e Edit>,
}

/// Pairs each review edit in `named` with its recorded sightings, keyed by
/// ID. Uses an empty list when `sightings` lacks the edit.
fn recorded_edits<'e>(
    named: BTreeMap<&'e str, &'e EditInfo>,
    sightings: &BTreeMap<String, Vec<&'e Edit>>,
) -> BTreeMap<&'e str, RecordedEdit<'e>> {
    named
        .into_iter()
        .map(|(id, entry)| {
            let seen = sightings.get(id).into_iter().flatten().copied().collect();
            (
                id,
                RecordedEdit {
                    entry,
                    sightings: canonical(seen),
                },
            )
        })
        .collect()
}

fn not_diffed(file: &FileReview) -> GateItem {
    let kind = file.kind.as_str();
    GateItem::new(
        Subject::Path(file.path_bytes.clone()),
        GateDetail::NotDiffed {
            kind: kind.to_owned(),
        },
        fingerprint([("content", content(file)), ("kind", part(&kind))]),
    )
}

/// Hashes a file's blob IDs and modes for its `content` part.
fn content(file: &FileReview) -> FingerprintPart {
    part(&(
        &file.old_blob,
        &file.old_mode,
        &file.new_blob,
        &file.new_mode,
    ))
}

fn untracked_recorded(path: &PathBytes, evidence: Option<&UntrackedEvidence<'_>>) -> GateItem {
    let sightings = evidence.map(|evidence| canonical(evidence.edits.clone()));
    let edits: BTreeSet<_> = sightings.iter().flatten().map(|edit| &edit.id).collect();
    GateItem::new(
        Subject::Path(path.clone()),
        GateDetail::UntrackedRecorded {
            edits: edits.len() as u64,
        },
        fingerprint([
            (
                "on_disk",
                evidence.map_or(FingerprintPart::Unknown, |evidence| evidence.on_disk.part()),
            ),
            (
                "edits",
                sightings.map_or(FingerprintPart::Unknown, |sightings| part(&sightings)),
            ),
        ]),
    )
}

fn not_captured(entry: &NotCapturedInfo, on_disk: Option<&OnDiskState>) -> GateItem {
    let on_disk = on_disk.unwrap_or(&OnDiskState::Unknown);
    GateItem::new(
        Subject::Path(entry.path_bytes.clone()),
        GateDetail::NotCaptured {
            cause: entry.cause.clone(),
            on_disk: on_disk.word().to_owned(),
        },
        fingerprint([("cause", part(&entry.cause)), ("on_disk", on_disk.part())]),
    )
}

/// Builds one item per file with gaps, including every distinct gap and
/// every sighting of its two edits.
fn discontinuities(gaps: &[Gap<'_>]) -> Vec<GateItem> {
    type Sightings<'g> = Vec<&'g Edit>;
    let mut by_path: BTreeMap<&PathBytes, Vec<(Sightings, Sightings)>> = BTreeMap::new();
    for gap in gaps {
        by_path
            .entry(&gap.path)
            .or_default()
            .push((canonical(gap.prev.clone()), canonical(gap.next.clone())));
    }
    by_path
        .into_iter()
        .map(|(path, pairs)| {
            let pairs = canonical(pairs);
            GateItem::new(
                Subject::Path(path.clone()),
                GateDetail::Discontinuity {
                    gaps: pairs.len() as u64,
                },
                fingerprint([("gaps", part(&pairs))]),
            )
        })
        .collect()
}

/// Builds one item per unreconciled record or checkout with its distinct reasons.
fn unreconciled(review: &Review) -> Vec<GateItem> {
    let records = review
        .records
        .iter()
        .filter(|record| !record.reconciled)
        .map(|record| {
            let reason = record.reason.as_deref().unwrap_or("unknown reason");
            (Subject::Record(RecordId::new(record.id.clone())), reason)
        });
    let checkouts = review.evidence.unreconciled_checkouts.iter().map(|entry| {
        let checkout = Subject::Checkout(PathBuf::from(&entry.checkout));
        (checkout, entry.reason.as_str())
    });
    let mut reasons: BTreeMap<Subject, BTreeSet<&str>> = BTreeMap::new();
    for (subject, reason) in records.chain(checkouts) {
        reasons.entry(subject).or_default().insert(reason);
    }
    reasons
        .into_iter()
        .map(|(subject, reasons)| {
            let reason = reasons.into_iter().collect::<Vec<_>>().join("; ");
            GateItem::new(
                subject,
                GateDetail::Unreconciled { reason },
                fingerprint([("evidence", FingerprintPart::Unknown)]),
            )
        })
        .collect()
}

fn fingerprint<const N: usize>(parts: [(&str, FingerprintPart); N]) -> Fingerprint {
    Fingerprint(
        parts
            .into_iter()
            .map(|(name, part)| (name.to_owned(), part))
            .collect(),
    )
}

/// Hashes `material` as SHA-256 of its JSON, or returns an unknown part if
/// serialization fails.
fn part(material: &impl Serialize) -> FingerprintPart {
    serde_json::to_vec(material).map_or(FingerprintPart::Unknown, |json| FingerprintPart::of(&json))
}

/// Sorts `material` by its JSON and removes duplicates so input order and
/// repeated sightings do not change its fingerprint part.
fn canonical<T: Serialize + PartialEq>(mut material: Vec<T>) -> Vec<T> {
    material.sort_by_cached_key(|piece| serde_json::to_vec(piece).ok());
    material.dedup();
    material
}

/// Builds each reviewed file's explanation digest, sorted by path bytes.
/// Hashes the file entry without mentions and its referenced `edits` entries,
/// keyed by ID, with every recorded sighting from `sightings`
/// ([`GateInputs::edits`]). A new payload under the same ID changes the digest.
///
/// Command text that names a file is a hint, not a line explanation. These
/// mentions, including read commands, do not change the digest.
///
/// # Panics
///
/// Panics if a file entry cannot serialize as JSON. Its types, text, numbers,
/// and JSON values, rule out this failure.
#[must_use]
pub fn explanations(review: &Review, sightings: &BTreeMap<String, Vec<&Edit>>) -> Vec<Explanation> {
    let mut explanations: Vec<Explanation> = review
        .files
        .iter()
        .map(|file| {
            let named = file
                .referenced_edits()
                .iter()
                .filter_map(|id| review.edits.get_key_value(id.as_str()))
                .map(|(id, info)| (id.as_str(), info))
                .collect();
            let edits = recorded_edits(named, sightings);
            // Clone only when there are mentions to remove.
            let entry = if file.mentions.is_empty() {
                Cow::Borrowed(file)
            } else {
                Cow::Owned(FileReview {
                    mentions: Vec::new(),
                    ..file.clone()
                })
            };
            let json = serde_json::to_vec(&(entry, edits)).expect("a file's review serializes");
            Explanation {
                path: file.path_bytes.clone(),
                digest: hex::encode(Sha256::digest(json)),
            }
        })
        .collect();
    explanations.sort_by(|a, b| a.path.cmp(&b.path));
    explanations
}

/// Sums capture limitation counts for each candidate transcript with any
/// limitations, sorted by transcript.
#[must_use]
pub fn capture(review: &Review) -> Vec<CaptureEntry> {
    let mut entries: Vec<CaptureEntry> = review
        .evidence
        .candidate_transcripts
        .iter()
        .map(|transcript| CaptureEntry {
            transcript: transcript.transcript.clone(),
            counts: total_counts(&transcript.capture_limitations),
        })
        .filter(|entry| !entry.counts.is_empty())
        .collect();
    entries.sort_by(|a, b| a.transcript.cmp(&b.transcript));
    entries
}
