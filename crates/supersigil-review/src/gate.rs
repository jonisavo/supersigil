//! The gate: what a person considers before completing a review.
//!
//! [`gate_items`] builds one item per origin and subject from the review and
//! what the CLI supplies beside it ([`GateInputs`]): the gaps between
//! recorded edits of reviewed files, what is on disk at the paths the
//! snapshot left out, and every recorded sighting of the edits the review
//! shows, payloads included. Items name paths by their raw bytes, which the model
//! carries unserialized, so two paths that print alike never share an item.
//!
//! Each item's fingerprint has one part per kind of material a judgment of
//! it saw: the SHA-256 of the material's JSON, or unknown when the material
//! could not be established. A judgment carries to a later review's item
//! only when every part matches, and an unknown part matches nothing.
//!
//! A basis also keeps [`explanations`], the digests of what the review
//! shows about each file, so that a later review can tell whether the
//! evidence behind the files changed, and [`capture`], the capture
//! limitations the review saw.

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

/// What is on disk at a path the snapshot did not capture or the scope
/// left out; the CLI reads it, and hashes what it finds.
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

/// A discontinuity between two recorded edits that map into a reviewed
/// file: the state one edit left is not the state the next one started
/// from. Each edit comes with every recorded sighting of it, in any order,
/// since a later sighting may carry another payload under the same id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// The reviewed file.
    pub path: PathBytes,
    /// Every sighting of the earlier edit, whose after-state is compared.
    pub prev: Vec<Edit>,
    /// Every sighting of the next edit, whose before-state differs from
    /// that after-state.
    pub next: Vec<Edit>,
}

/// What the CLI found for an untracked file a recorded edit names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedEvidence {
    /// What is on disk at the path.
    pub on_disk: OnDiskState,
    /// Every sighting of every recorded edit naming the path, in any order.
    pub edits: Vec<Edit>,
}

/// What the CLI supplies beside a review to build its gate. Paths are the
/// raw bytes the review's entries carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateInputs {
    /// The gaps among edits of candidate sessions that map into a reviewed
    /// file, in any order.
    pub discontinuities: Vec<Gap>,
    /// Per untracked file with recorded edits, what the CLI found.
    pub untracked: BTreeMap<PathBytes, UntrackedEvidence>,
    /// Per path not captured, what is on disk.
    pub not_captured: BTreeMap<PathBytes, OnDiskState>,
    /// Every recorded sighting of each edit in the review's `edits` map,
    /// by id, in any order: the `edits` entries leave out the payload and
    /// the states an edit was recorded with.
    pub edits: BTreeMap<String, Vec<Edit>>,
}

/// The review's gate items, sorted by origin, then subject.
///
/// One item per file with lines no recorded edit explains (counted as the
/// unattributed summary counts them), per file not diffed, per untracked
/// file with recorded edits, per path not captured, per commit without an
/// origin, per reviewed file with gaps between its recorded edits, and per
/// record or checkout not reconciled. Evidence the inputs lack for an
/// untracked or uncaptured path, or for an edit behind counted lines, gives
/// unknown parts, as a directory does; an unreconciled item's evidence is
/// always unknown, since it was not read.
#[must_use]
pub fn gate_items(review: &Review, inputs: &GateInputs) -> Vec<GateItem> {
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
    items.extend(
        scope
            .untracked_excluded
            .iter()
            .filter(|untracked| untracked.recorded_edit)
            .map(|untracked| {
                let path = &untracked.path_bytes;
                untracked_recorded(path, inputs.untracked.get(path))
            }),
    );
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

/// The item of `file`'s lines no recorded edit explains, if it has any.
/// Its evidence is unknown when `sightings` lacks an edit those lines name.
fn unexplained_lines(
    file: &FileReview,
    edits: &BTreeMap<String, EditInfo>,
    sightings: &BTreeMap<String, Vec<Edit>>,
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

/// The entries of `edits` that `spans` name, by id. Outcomes name edits as
/// ids, and anchor's provenance names them inside JSON values, so every
/// string in the spans' JSON that is an edit's id counts.
fn named_edits<'e>(
    spans: &[&Span],
    edits: &'e BTreeMap<String, EditInfo>,
) -> BTreeMap<&'e str, &'e EditInfo> {
    let mut named = BTreeMap::new();
    let mut pending: Vec<Value> = spans
        .iter()
        .filter_map(|span| serde_json::to_value(span).ok())
        .collect();
    while let Some(value) = pending.pop() {
        match value {
            Value::String(text) => {
                if let Some((id, info)) = edits.get_key_value(&text) {
                    named.insert(id.as_str(), info);
                }
            }
            Value::Array(values) => pending.extend(values),
            Value::Object(fields) => pending.extend(fields.into_iter().map(|(_, value)| value)),
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

/// Each edit of `named`, entries of the review's `edits` map by id, with
/// its sightings in `sightings` (none when it lacks them).
fn recorded_edits<'e>(
    named: BTreeMap<&'e str, &'e EditInfo>,
    sightings: &'e BTreeMap<String, Vec<Edit>>,
) -> BTreeMap<&'e str, RecordedEdit<'e>> {
    named
        .into_iter()
        .map(|(id, entry)| {
            let seen = sightings.get(id).into_iter().flatten().collect();
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

/// The item of a file listed but not diffed.
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

/// The `content` part of a file's item: its blob ids and modes.
fn content(file: &FileReview) -> FingerprintPart {
    part(&(
        &file.old_blob,
        &file.old_mode,
        &file.new_blob,
        &file.new_mode,
    ))
}

/// The item of an untracked file with recorded edits, from what the CLI
/// found for it.
fn untracked_recorded(path: &PathBytes, evidence: Option<&UntrackedEvidence>) -> GateItem {
    let sightings = evidence.map(|evidence| canonical(evidence.edits.iter().collect()));
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

/// The item of a path not captured, with what is on disk there.
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

/// One item per file with gaps, holding every distinct gap with every
/// sighting of its two edits.
fn discontinuities(gaps: &[Gap]) -> Vec<GateItem> {
    type Sightings<'g> = Vec<&'g Edit>;
    let mut by_path: BTreeMap<&PathBytes, Vec<(Sightings, Sightings)>> = BTreeMap::new();
    for gap in gaps {
        by_path.entry(&gap.path).or_default().push((
            canonical(gap.prev.iter().collect()),
            canonical(gap.next.iter().collect()),
        ));
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

/// One item per record or checkout not reconciled, with its distinct
/// reasons.
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

/// A fingerprint of the named parts.
fn fingerprint<const N: usize>(parts: [(&str, FingerprintPart); N]) -> Fingerprint {
    Fingerprint(
        parts
            .into_iter()
            .map(|(name, part)| (name.to_owned(), part))
            .collect(),
    )
}

/// The part for `material`: the SHA-256 of its JSON, or unknown if it
/// cannot be serialized.
fn part(material: &impl Serialize) -> FingerprintPart {
    serde_json::to_vec(material).map_or(FingerprintPart::Unknown, |json| FingerprintPart::of(&json))
}

/// `material` in the order of its JSON and without repeats, so neither the
/// order the CLI found it in nor how often changes its part.
fn canonical<T: Serialize + PartialEq>(mut material: Vec<T>) -> Vec<T> {
    material.sort_by_cached_key(|piece| serde_json::to_vec(piece).ok());
    material.dedup();
    material
}

/// Per reviewed file, the digest of what the review shows about it: its
/// entry without its mentions, and the `edits` entries it references, by
/// id, each with every recorded sighting of it in `sightings`
/// ([`GateInputs::edits`]), so a payload recorded again under the same id
/// changes the digest. Sorted by path bytes.
///
/// Mentions, the commands whose text names the file, are shown as hints
/// but never explain a line, and most are reads: a command that reads a
/// reviewed file leaves its digest as it was.
///
/// # Panics
///
/// Panics if a file's entry cannot be serialized as JSON, which its types
/// (text, numbers, and JSON values) rule out.
#[must_use]
pub fn explanations(review: &Review, sightings: &BTreeMap<String, Vec<Edit>>) -> Vec<Explanation> {
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
            let entry = FileReview {
                mentions: Vec::new(),
                ..file.clone()
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

/// Each candidate transcript with any capture limitation, with its summed
/// counts, sorted by transcript.
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
