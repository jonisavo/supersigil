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

/// Walks the path's recorded edits, replays and combines the chains that
/// were not set aside, and adds the content-match fallback.
///
/// Replaying the chains has its own budget of `budget_bytes`, shared by
/// every chain (see [`replay`]). When it runs out, the status is
/// [`PathStatus::SearchIncomplete`], and the line outcomes combine only the
/// chains replayed in full; every chain found is still listed.
///
/// Content matching runs only when the search completed (after an
/// incomplete search every line is unresolved, so the fallback is never
/// consulted, and its matches stay empty) and only for the lines whose
/// combined outcome needs it, with its own budget of `budget_bytes`.
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
