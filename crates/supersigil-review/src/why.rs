//! `supersigil why <file>:<line>`: one line's outcome, with the evidence
//! context the review header shows.
//!
//! The line is read against the target alone: its outcome follows the same
//! ordered rules as an added diff line (a line the chains do not explain gets
//! the content-match fallback). The records and candidate transcripts are
//! always present, whatever the on-disk check found.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::Serialize;
use serde_json::Value;
use supersigil_anchor::{Chain, Conflict, PathStatus};
use supersigil_record::EventId;
use supersigil_record::observations::Material;

use crate::model::{EditInfo, EvidenceInfo, RecordInfo, analysis_edits, outcome_edits, text};
use crate::outcome::{AttributionState, Outcome, target_line_outcome};
use crate::summary::{class_words, context_lines, reason_words, relation_words};

/// The `why` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Why {
    /// [`crate::WHY_SCHEMA`].
    pub schema: &'static str,
    /// The path as the user named it, relative to the worktree.
    pub path: String,
    /// What was read.
    pub target: WhyTarget,
    /// Every involved record, with reconcile status; always present.
    pub records: Vec<RecordInfo>,
    /// Candidate transcripts and their capture limitations; always present.
    pub evidence: EvidenceInfo,
    /// Whether the line analysis read what is on disk.
    pub on_disk: OnDiskCheck,
    /// The line's analysis; null when the on-disk check failed.
    pub line: Option<WhyLine>,
    /// The edits this path's analysis refers to, keyed by edit id.
    pub edits: BTreeMap<String, EditInfo>,
    /// Edits for this path excluded as conflicting evidence.
    pub conflicting_edits: Vec<Conflict>,
}

/// What `why` read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WhyTarget {
    /// The worktree.
    pub worktree: String,
    /// The captured working-tree tree.
    pub tree: String,
    /// The path's blob in that tree.
    pub blob: Option<String>,
    /// Attribution-bytes status: `identical`, `converted`, or `failed: ...`.
    pub attribution_bytes: String,
}

/// Whether the captured target matches the file on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OnDiskCheck {
    /// The captured bytes are what is on disk, up to line endings.
    Captured,
    /// They differ; no line is explained.
    NotCaptured {
        /// Why, for example "assume-unchanged".
        reason: String,
    },
}

/// Whether the line differs from HEAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tristate {
    /// It differs.
    Yes,
    /// It is the same.
    No,
    /// Unknown: the file is not in HEAD, HEAD is unborn, or the line lies
    /// in a coarse hunk.
    Unknown,
}

/// One line's analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WhyLine {
    /// Line number, 1-based.
    pub number: usize,
    /// Line text with its terminator.
    pub text: String,
    /// Whether the line differs from HEAD.
    pub differs_from_head: Tristate,
    /// Why `differs_from_head` is unknown, for example "coarse diff"; null
    /// otherwise.
    pub differs_from_head_reason: Option<String>,
    /// Anchor's status for the path; null when attribution is unavailable.
    pub status: Option<PathStatus>,
    /// The chains not set aside.
    pub chains: Vec<Chain>,
    /// Alternatives that assume an unrecorded change.
    pub set_aside: Vec<Chain>,
    /// The line's outer outcome.
    pub outcome: Outcome,
    /// The line's full provenance.
    pub provenance: Value,
}

impl WhyLine {
    /// Every edit id this line's analysis names: the path's chains,
    /// set-aside chains, and stop reasons, and the edits the line's outcome
    /// names. The `why` result's `edits` map holds exactly these.
    #[must_use]
    pub fn referenced_edits(&self) -> BTreeSet<EventId> {
        let mut ids = BTreeSet::new();
        analysis_edits(
            self.status.as_ref(),
            &self.chains,
            &self.set_aside,
            &mut ids,
        );
        ids.extend(outcome_edits(&self.outcome).into_iter().cloned());
        ids
    }
}

/// Analyzes 0-based target line `line` by the same ordered rules as an added
/// diff line.
#[must_use]
pub fn why_line(
    attribution: &AttributionState<'_>,
    line: usize,
    target_blob_lines: &[&[u8]],
    base_blob_lines: &[&[u8]],
    differs_from_head: Tristate,
    differs_from_head_reason: Option<String>,
) -> WhyLine {
    let (outcome, provenance) =
        target_line_outcome(attribution, line, base_blob_lines, target_blob_lines);
    let (status, chains, set_aside) = match attribution {
        AttributionState::Available(attr) => (
            Some(attr.status.clone()),
            attr.chains.clone(),
            attr.set_aside.clone(),
        ),
        AttributionState::Unavailable { .. } => (None, Vec::new(), Vec::new()),
    };
    WhyLine {
        number: line + 1,
        text: target_blob_lines
            .get(line)
            .map(|line| text(line))
            .unwrap_or_default(),
        differs_from_head,
        differs_from_head_reason,
        status,
        chains,
        set_aside,
        outcome,
        provenance,
    }
}

/// Renders `why` for a terminal, escaping untrusted text with `escape`.
#[must_use]
pub fn render_why(why: &Why, escape: fn(&str) -> String) -> String {
    let mut out = String::new();
    let path = escape(&why.path);
    match (&why.on_disk, &why.line) {
        (OnDiskCheck::NotCaptured { reason }, _) => {
            push(
                &mut out,
                &format!(
                    "{path}: not captured: {}; no line explained",
                    escape(reason)
                ),
            );
        }
        (OnDiskCheck::Captured, None) => push(&mut out, &format!("{path}: no line analyzed")),
        (OnDiskCheck::Captured, Some(line)) => {
            let content = line.text.trim_end_matches(['\n', '\r']);
            push(
                &mut out,
                &format!("{path}:{}: {}", line.number, escape(content)),
            );
            push(
                &mut out,
                &format!("  {}", outcome_sentence(&line.outcome, escape)),
            );
            let differs = match line.differs_from_head {
                Tristate::Yes => "yes",
                Tristate::No => "no",
                Tristate::Unknown => "unknown",
            };
            match &line.differs_from_head_reason {
                Some(reason) => push(
                    &mut out,
                    &format!("  differs from HEAD: {differs} ({})", escape(reason)),
                ),
                None => push(&mut out, &format!("  differs from HEAD: {differs}")),
            }
        }
    }
    // Only the edits the line's provenance names as contributing: an
    // unexplained, ambiguous, unresolved, or unanalyzed line has none, and
    // the other edits the analysis refers to stay in the JSON map.
    let contributors = why.line.as_ref().map(contributors).unwrap_or_default();
    let mut edits: Vec<(&EventId, Option<&EditInfo>)> = contributors
        .into_iter()
        .map(|id| (id, why.edits.get(id.as_str())))
        .collect();
    edits.sort_by(|a, b| {
        let time = |edit: Option<&EditInfo>| edit.map(|e| e.time.clone());
        (time(a.1), a.0).cmp(&(time(b.1), b.0))
    });
    for (id, edit) in edits {
        let Some(edit) = edit else {
            push(
                &mut out,
                &format!("  edit {}: no details recorded", escape(id.as_str())),
            );
            continue;
        };
        let prompt = edit.prompt.as_ref().map_or_else(
            || "no preceding message recorded".to_owned(),
            |prompt| match &prompt.excerpt {
                Material::Retained(excerpt) => {
                    let first = excerpt.lines().next().unwrap_or_default();
                    let short: String = first.chars().take(80).collect();
                    format!("after \"{}\"", escape(&short))
                }
                Material::Withheld { policy } => {
                    format!("preceding message withheld ({})", escape(policy))
                }
                Material::Unavailable { reason } => {
                    format!("preceding message unavailable ({})", escape(reason))
                }
            },
        );
        push(
            &mut out,
            &format!(
                "  edit {} at {}, session {}: {prompt}",
                escape(id.as_str()),
                escape(&edit.time),
                escape(&edit.session)
            ),
        );
    }
    for line in context_lines(&why.records, &why.evidence, escape) {
        push(&mut out, &format!("  {line}"));
    }
    out
}

/// The edits `line`'s provenance names as contributing: the introducing or
/// replacing edits of an attributed line, or the whitespace-only edits of a
/// whitespace-only change. A content match is not provenance (its sentence
/// names the edits), and no other outcome has contributors.
fn contributors(line: &WhyLine) -> BTreeSet<&EventId> {
    match &line.outcome {
        Outcome::Attributed { .. } | Outcome::WhitespaceOnly { .. } => {
            outcome_edits(&line.outcome).into_iter().collect()
        }
        Outcome::ContentMatch { .. }
        | Outcome::Unresolved { .. }
        | Outcome::Ambiguous { .. }
        | Outcome::Unattributed { .. }
        | Outcome::LineEndingChanged
        | Outcome::Realigned { .. } => BTreeSet::new(),
    }
}

fn outcome_sentence(outcome: &Outcome, escape: fn(&str) -> String) -> String {
    let ids = |edits: &mut dyn Iterator<Item = String>| -> String {
        edits
            .map(|id| escape(&id))
            .collect::<Vec<_>>()
            .join(" and ")
    };
    match outcome {
        Outcome::Attributed { edits } => {
            let parts: Vec<String> = edits
                .iter()
                .map(|edit| {
                    let class = edit
                        .classes
                        .iter()
                        .min()
                        .map_or("", |class| class_words(*class));
                    format!(
                        "{} by edit {} ({class})",
                        relation_words(edit.relation),
                        escape(edit.edit.as_str())
                    )
                })
                .collect();
            parts.join("; ")
        }
        Outcome::ContentMatch { edits, .. } => format!(
            "matches the text of edit {} (content match, not a composed chain)",
            ids(&mut edits.iter().map(|e| e.as_str().to_owned()))
        ),
        Outcome::WhitespaceOnly { edits } => format!(
            "only whitespace changed, by edit {}",
            ids(&mut edits.iter().map(|e| e.as_str().to_owned()))
        ),
        Outcome::Unattributed { reason } => {
            format!("not attributed: {}", reason_words(reason, escape))
        }
        Outcome::Ambiguous { .. } => {
            "ambiguous: the recorded edits allow more than one history for this line".to_owned()
        }
        Outcome::Unresolved { .. } => {
            "unresolved: the search did not complete within its budget".to_owned()
        }
        Outcome::LineEndingChanged => {
            "only the line ending differs; no recorded edit explains it".to_owned()
        }
        Outcome::Realigned { line } => format!("unchanged from line {}", line + 1),
    }
}

fn push(out: &mut String, line: &str) {
    let _ = writeln!(out, "{line}");
}
