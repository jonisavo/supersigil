//! The entry point: attributes one path.

use std::collections::BTreeSet;

use crate::combine::combine;
use crate::content::content_matches;
use crate::input::Request;
use crate::lines::line_starts;
use crate::provenance::{ChainProvenance, replay};
use crate::result::{Chain, ContentMatches, PathAttribution, PathStatus};
use crate::step::Budget;
use crate::walk::walk;

/// Walks the path's accepted edits, replays and combines the chains that
/// were not set aside, and adds the content-match fallback.
///
/// Replay shares one `budget_bytes` budget across all surviving chains.
/// Exhaustion sets [`PathStatus::SearchIncomplete`]. All discovered chains
/// remain listed; only fully replayed chains contribute line outcomes.
///
/// Content matching runs only after a complete search, for lines that
/// need fallback evidence. It has a separate `budget_bytes` budget.
/// An incomplete search produces no content matches.
///
/// No outcome claims capture completeness: a chain exact from the base
/// says only that the recorded replacements transform the base into the
/// target.
#[must_use]
pub fn attribute(request: Request) -> PathAttribution {
    let walked = walk(&request);
    let Request {
        base,
        target,
        budget_bytes,
        ..
    } = request;
    let base_lines = line_starts(base.bytes().unwrap_or_default()).len();
    let target_lines = line_starts(target.bytes().unwrap_or_default()).len();
    let mut budget = Budget::new(budget_bytes);
    let mut provenance: Vec<(&Chain, ChainProvenance)> = Vec::new();
    for chain in &walked.chains {
        let Some(replayed) = replay(chain, base_lines, &mut budget) else {
            break;
        };
        provenance.push((&chain.chain, replayed));
    }
    let status = if budget.exhausted() {
        PathStatus::SearchIncomplete
    } else {
        walked.status
    };
    let (target_outcomes, base_outcomes) = combine(provenance, target_lines, base_lines);
    let content = if status == PathStatus::SearchIncomplete {
        ContentMatches {
            target: vec![BTreeSet::new(); target_lines],
            base: vec![BTreeSet::new(); base_lines],
            incomplete: false,
        }
    } else {
        content_matches(
            &base,
            &target,
            &walked.accepted,
            &target_outcomes,
            &base_outcomes,
            budget_bytes,
        )
    };
    PathAttribution {
        status,
        chains: walked.chains.iter().map(|w| w.chain.clone()).collect(),
        set_aside: walked.set_aside.iter().map(|w| w.chain.clone()).collect(),
        conflicts: walked.conflicts,
        accepted: walked.accepted,
        target: target_outcomes,
        base: base_outcomes,
        content,
    }
}
